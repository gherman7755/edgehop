//! LAN discovery over mDNS / DNS-SD: a receiver announces itself as `_edgehop._tcp` and controllers list the
//! computers that do. The Swift core uses Bonjour with the same service type and plain instance names (the
//! computer name, no TXT records), so Macs and Windows PCs find each other.
//!
//! Built on `mdns-sd` (pure Rust, no Bonjour service needed on Windows). One daemon thread per [`MdnsDiscovery`],
//! started the first time it is used.

use std::collections::BTreeMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use edgehop_input::{DiscoveredPeer, Discovery, DiscoveryGuard, PlatformError};
use edgehop_proto::SERVICE_TYPE;
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};

/// `_edgehop._tcp.local.`
fn service_type() -> String {
    format!("{SERVICE_TYPE}.local.")
}

/// The DNS host name for a computer name: letters, digits and hyphens only (`Central’s MacBook Pro` becomes
/// `Central-s-MacBook-Pro.local.`). The service instance keeps the real name.
pub fn host_name_for(computer_name: &str) -> String {
    let mut label: String = computer_name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    label = label.trim_matches('-').to_string();
    if label.is_empty() {
        label = "edgehop".to_string();
    }
    label.truncate(63);
    format!("{label}.local.")
}

/// The instance name of a full service name: `Central’s Mac._edgehop._tcp.local.` gives `Central’s Mac`.
pub fn instance_name(fullname: &str, ty_domain: &str) -> String {
    let suffix = format!(".{ty_domain}");
    let name = fullname.strip_suffix(&suffix).unwrap_or(fullname);
    // The library escapes dots and backslashes inside instance names.
    let mut out = String::new();
    let mut chars = name.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// The addresses worth trying, best first: private IPv4 (the local network), other IPv4, IPv6, and loopback last
/// (a computer announcing on this very machine only shows loopback). IPv6 link-local addresses are dropped: they
/// are useless without the interface they belong to.
fn ordered(addresses: impl IntoIterator<Item = IpAddr>) -> Vec<IpAddr> {
    let rank = |a: &IpAddr| match a {
        IpAddr::V4(v4) if v4.is_loopback() => 4,
        IpAddr::V4(v4) if v4.is_private() => 0,
        IpAddr::V4(_) => 1,
        IpAddr::V6(v6) if v6.is_loopback() => 5,
        IpAddr::V6(_) => 2,
    };
    let mut list: Vec<IpAddr> = addresses
        .into_iter()
        .filter(|a| !matches!(a, IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80))
        .filter(|a| !a.is_unspecified())
        .collect();
    list.sort_by_key(|a| (rank(a), *a));
    list.dedup();
    list
}

/// [`Discovery`] over mDNS.
#[derive(Default)]
pub struct MdnsDiscovery {
    daemon: Mutex<Option<ServiceDaemon>>,
}

impl MdnsDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    fn daemon(&self) -> Result<ServiceDaemon, PlatformError> {
        let mut slot = self.daemon.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(daemon) = slot.as_ref() {
            return Ok(daemon.clone());
        }
        let daemon = ServiceDaemon::new()
            .map_err(|e| PlatformError(format!("cannot start network discovery: {e}")))?;
        *slot = Some(daemon.clone());
        Ok(daemon)
    }
}

impl Drop for MdnsDiscovery {
    fn drop(&mut self) {
        if let Some(daemon) = self
            .daemon
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            let _ = daemon.shutdown();
        }
    }
}

struct Announcement {
    daemon: ServiceDaemon,
    fullname: String,
}

impl DiscoveryGuard for Announcement {}

impl Drop for Announcement {
    fn drop(&mut self) {
        // Goodbye packets let other computers drop us from their lists at once; do not wait long for them.
        if let Ok(done) = self.daemon.unregister(&self.fullname) {
            let _ = done.recv_timeout(Duration::from_millis(500));
        }
    }
}

struct Search {
    daemon: ServiceDaemon,
    stopped: Arc<AtomicBool>,
}

impl DiscoveryGuard for Search {}

impl Drop for Search {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        let _ = self.daemon.stop_browse(&service_type());
    }
}

impl Discovery for MdnsDiscovery {
    fn advertise(&self, name: &str, port: u16) -> Result<Box<dyn DiscoveryGuard>, PlatformError> {
        let daemon = self.daemon()?;
        let no_properties: &[(&str, &str)] = &[];
        let info = ServiceInfo::new(
            &service_type(),
            name,
            &host_name_for(name),
            "", // addresses are filled in (and kept current) by the library
            port,
            no_properties,
        )
        .map_err(|e| PlatformError(format!("cannot announce this computer: {e}")))?
        .enable_addr_auto();
        let fullname = info.get_fullname().to_string();
        daemon
            .register(info)
            .map_err(|e| PlatformError(format!("cannot announce this computer: {e}")))?;
        Ok(Box::new(Announcement { daemon, fullname }))
    }

    fn browse(
        &self,
        own_name: &str,
        on_change: Box<dyn Fn(Vec<DiscoveredPeer>) + Send + Sync>,
    ) -> Result<Box<dyn DiscoveryGuard>, PlatformError> {
        let daemon = self.daemon()?;
        let events = daemon
            .browse(&service_type())
            .map_err(|e| PlatformError(format!("cannot look for computers: {e}")))?;
        let stopped = Arc::new(AtomicBool::new(false));
        let thread_stopped = Arc::clone(&stopped);
        let own = own_name.to_string();
        thread::Builder::new()
            .name("edgehop-discovery-browse".into())
            .spawn(move || {
                // fullname -> peer
                let mut found: BTreeMap<String, DiscoveredPeer> = BTreeMap::new();
                while let Ok(event) = events.recv() {
                    if thread_stopped.load(Ordering::SeqCst) {
                        break;
                    }
                    let changed = match event {
                        ServiceEvent::ServiceResolved(resolved) => {
                            let name = instance_name(&resolved.fullname, &resolved.ty_domain);
                            let addresses =
                                ordered(resolved.addresses.iter().map(|a| a.to_ip_addr()));
                            if name == own || addresses.is_empty() {
                                false
                            } else {
                                let peer = DiscoveredPeer {
                                    name,
                                    addresses,
                                    port: resolved.port,
                                };
                                found.insert(resolved.fullname.clone(), peer.clone()) != Some(peer)
                            }
                        }
                        ServiceEvent::ServiceRemoved(_, fullname) => {
                            found.remove(&fullname).is_some()
                        }
                        _ => false,
                    };
                    if changed {
                        let mut peers: Vec<DiscoveredPeer> = found.values().cloned().collect();
                        peers.sort_by(|a, b| a.name.cmp(&b.name));
                        on_change(peers);
                    }
                }
            })
            .map_err(|e| PlatformError(format!("cannot look for computers: {e}")))?;
        Ok(Box::new(Search { daemon, stopped }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_are_plain_dns_labels() {
        assert_eq!(host_name_for("DESKTOP-7K2P"), "DESKTOP-7K2P.local.");
        assert_eq!(
            host_name_for("Central’s MacBook Pro"),
            "Central-s-MacBook-Pro.local."
        );
        assert_eq!(host_name_for("  "), "edgehop.local.");
        assert!(host_name_for(&"x".repeat(200)).len() <= 63 + ".local.".len());
    }

    #[test]
    fn instance_names_lose_the_service_suffix_and_the_escapes() {
        assert_eq!(
            instance_name("DESKTOP-7K2P._edgehop._tcp.local.", "_edgehop._tcp.local."),
            "DESKTOP-7K2P"
        );
        assert_eq!(
            instance_name("Central’s Mac._edgehop._tcp.local.", "_edgehop._tcp.local."),
            "Central’s Mac"
        );
        assert_eq!(
            instance_name(r"Dr\. Who\\PC._edgehop._tcp.local.", "_edgehop._tcp.local."),
            r"Dr. Who\PC"
        );
    }

    #[test]
    fn addresses_are_ordered_lan_first_and_unusable_ones_dropped() {
        let ip = |s: &str| s.parse::<IpAddr>().unwrap();
        let list = ordered([
            ip("127.0.0.1"),
            ip("fe80::1"),
            ip("::1"),
            ip("2001:db8::7"),
            ip("192.168.0.7"),
            ip("169.254.9.9"),
            ip("10.0.0.2"),
            ip("192.168.0.7"),
            ip("0.0.0.0"),
        ]);
        assert_eq!(
            list,
            vec![
                ip("10.0.0.2"),
                ip("192.168.0.7"),
                ip("169.254.9.9"),
                ip("2001:db8::7"),
                ip("127.0.0.1"),
                ip("::1"),
            ]
        );
    }

    /// Needs a network interface with multicast. Run it by hand: `cargo test -p edgehop-discovery -- --ignored`.
    #[test]
    #[ignore = "uses real multicast on this machine"]
    fn an_announced_receiver_is_found_by_a_second_browser() {
        let announcer = MdnsDiscovery::new();
        let browser = MdnsDiscovery::new();
        let name = format!("edgehop-test-{}", std::process::id());
        let (tx, rx) = std::sync::mpsc::channel();
        let _search = browser
            .browse(
                "someone else",
                Box::new(move |peers| {
                    let _ = tx.send(peers);
                }),
            )
            .expect("browse");
        let announcement = announcer.advertise(&name, 47999).expect("advertise");

        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        let mut found = None;
        while std::time::Instant::now() < deadline && found.is_none() {
            if let Ok(peers) = rx.recv_timeout(Duration::from_millis(500)) {
                found = peers.into_iter().find(|p| p.name == name);
            }
        }
        let peer = found.expect("the announced receiver was never found");
        assert_eq!(peer.port, 47999);
        assert!(!peer.addresses.is_empty());

        drop(announcement);
        let mut gone = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(15);
        while std::time::Instant::now() < deadline && !gone {
            if let Ok(peers) = rx.recv_timeout(Duration::from_millis(500)) {
                gone = !peers.iter().any(|p| p.name == name);
            }
        }
        assert!(gone, "the withdrawn receiver stayed in the list");
    }
}
