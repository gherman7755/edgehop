//! Computers found on the local network: what `peers.watch` / `peers.list` report and what a controller resolves a
//! computer name to.

use std::sync::{Arc, Mutex, PoisonError};

use edgehop_input::{DiscoveredPeer, Discovery, DiscoveryGuard};

use super::log;
use crate::backend::{Events, Source};

#[derive(Default)]
struct State {
    peers: Vec<DiscoveredPeer>,
    /// The running search; dropping it stops it.
    search: Option<Box<dyn DiscoveryGuard>>,
}

pub(super) struct Peers {
    discovery: Option<Arc<dyn Discovery>>,
    own_name: String,
    state: Arc<Mutex<State>>,
    /// Serialises `watch`: the search may report to its callback while it is being started, so `state` must not
    /// be held across that.
    control: Mutex<()>,
}

impl Peers {
    pub(super) fn new(discovery: Option<Arc<dyn Discovery>>, own_name: String) -> Self {
        Self {
            discovery,
            own_name,
            state: Arc::default(),
            control: Mutex::new(()),
        }
    }

    /// Whether this computer can find others at all.
    pub(super) fn supported(&self) -> bool {
        self.discovery.is_some()
    }

    /// Starts or stops looking. While looking, every change is reported as a `peers.changed` event. Looking only
    /// starts when the UI asks (on some systems it triggers a permission prompt).
    pub(super) fn watch(&self, enabled: bool, events: Events) {
        let Some(discovery) = &self.discovery else {
            return;
        };
        let _serial = self.control.lock().unwrap_or_else(PoisonError::into_inner);
        if !enabled {
            let (search, had_any) = {
                let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
                let had_any = !state.peers.is_empty();
                state.peers.clear();
                (state.search.take(), had_any)
            };
            drop(search); // stops the search, outside the lock
            if had_any {
                events.peers_changed(&[]);
            }
            return;
        }
        if self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .search
            .is_some()
        {
            return;
        }
        let shared = Arc::clone(&self.state);
        let callback_events = events.clone();
        let search = discovery.browse(
            &self.own_name,
            Box::new(move |peers| {
                let names: Vec<String> = peers.iter().map(|p| p.name.clone()).collect();
                shared.lock().unwrap_or_else(PoisonError::into_inner).peers = peers;
                callback_events.peers_changed(&names);
            }),
        );
        match search {
            Ok(guard) => {
                self.state
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .search = Some(guard);
            }
            Err(error) => log(
                &events,
                Source::Controller,
                format!("WARNING: cannot look for computers on the network ({error})"),
            ),
        }
    }

    pub(super) fn names(&self) -> Vec<String> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .peers
            .iter()
            .map(|p| p.name.clone())
            .collect()
    }

    pub(super) fn resolve(&self, name: &str) -> Option<DiscoveredPeer> {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .peers
            .iter()
            .find(|p| p.name == name)
            .cloned()
    }

    /// Stops looking without announcing anything (shutdown).
    pub(super) fn stop(&self) {
        let search = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .search
            .take();
        drop(search);
    }
}
