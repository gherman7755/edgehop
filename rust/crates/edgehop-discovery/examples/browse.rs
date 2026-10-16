//! Lists computers announcing `_edgehop._tcp` for interop checks (the Swift side announces): `browse SECONDS`.
//! With `--connect NAME TOKEN` it connects to that computer as soon as it appears, which proves that the
//! address in its announcement works.

use std::io::Write;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use edgehop_discovery::MdnsDiscovery;
use edgehop_input::{DiscoveredPeer, Discovery};
use edgehop_link::{ClientConfig, ClientHandler, LinkClient};

struct Quiet;
impl ClientHandler for Quiet {}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let seconds: u64 = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);
    let connect = args
        .iter()
        .position(|a| a == "--connect")
        .map(|i| (args[i + 1].clone(), args[i + 2].clone()));

    let discovery = MdnsDiscovery::new();
    let (tx, rx) = mpsc::channel::<Vec<DiscoveredPeer>>();
    let _search = discovery
        .browse(
            "rust-browser",
            Box::new(move |peers| {
                let _ = tx.send(peers);
            }),
        )
        .unwrap_or_else(|e| {
            eprintln!("browse: {e}");
            std::process::exit(1);
        });

    let deadline = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < deadline {
        let Ok(peers) = rx.recv_timeout(Duration::from_millis(250)) else {
            continue;
        };
        let names: Vec<String> = peers
            .iter()
            .map(|p| format!("{} {:?}:{}", p.name, p.addresses, p.port))
            .collect();
        println!(
            "found: {}",
            if names.is_empty() {
                "(nothing)".into()
            } else {
                names.join(", ")
            }
        );
        let _ = std::io::stdout().flush();
        let Some((wanted, token)) = &connect else {
            continue;
        };
        if let Some(peer) = peers.iter().find(|p| &p.name == wanted) {
            for address in &peer.addresses {
                let config = ClientConfig::new(token.as_str(), "rust-browser");
                match LinkClient::connect(&address.to_string(), peer.port, &config, Arc::new(Quiet))
                {
                    Ok((_client, hello)) => {
                        println!("connected to {:?} at {address}", hello.name);
                        return;
                    }
                    Err(error) => println!("could not connect to {address}: {error}"),
                }
            }
        }
    }
    if connect.is_some() {
        eprintln!("never connected");
        std::process::exit(1);
    }
}
