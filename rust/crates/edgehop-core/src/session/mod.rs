//! The real backend: receiver and controller sessions joining the TCP link, the input layer and the edge
//! switching on top of a [`Platform`]. Everything here is platform-independent and tested with fakes.

mod controller;
mod peers;
mod receiver;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex, PoisonError};

use edgehop_input::Platform;

use crate::backend::{
    Backend, BackendError, ControllerConfig, ControllerState, Events, PermissionKind, Permissions,
    ReceiverConfig, ReceiverState, Source,
};

use controller::ControllerSessions;
use peers::Peers;
use receiver::ReceiverSessions;

/// A value that announces each change exactly once, in order.
pub(crate) struct Tracked<T> {
    value: Mutex<T>,
}

impl<T: Clone + PartialEq> Tracked<T> {
    pub(crate) fn new(value: T) -> Self {
        Self {
            value: Mutex::new(value),
        }
    }

    pub(crate) fn get(&self) -> T {
        self.value
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stores `next` and calls `announce` with it if it differs. The announcement happens under the lock so
    /// that concurrent changes are announced in the order they were made.
    pub(crate) fn set(&self, next: T, announce: impl FnOnce(&T)) {
        let mut value = self.value.lock().unwrap_or_else(PoisonError::into_inner);
        if *value != next {
            *value = next;
            announce(&value);
        }
    }
}

pub(crate) fn log(events: &Events, source: Source, line: impl AsRef<str>) {
    events.log(source, line.as_ref());
}

/// [`Backend`] for any [`Platform`]: runs a receiver and a controller session. Computers are reached by name when
/// the platform can discover them, otherwise by address.
pub struct SessionBackend<P: Platform> {
    platform: Arc<P>,
    peers: Arc<Peers>,
    receiver: ReceiverSessions<P>,
    controller: ControllerSessions<P>,
}

impl<P: Platform> SessionBackend<P> {
    pub fn new(platform: P) -> Self {
        let platform = Arc::new(platform);
        let peers = Arc::new(Peers::new(platform.discovery(), platform.device_name()));
        Self {
            receiver: ReceiverSessions::new(Arc::clone(&platform)),
            controller: ControllerSessions::new(Arc::clone(&platform), Arc::clone(&peers)),
            peers,
            platform,
        }
    }
}

impl<P: Platform> Backend for SessionBackend<P> {
    fn platform(&self) -> &'static str {
        self.platform.os_name()
    }

    fn device_name(&self) -> String {
        self.platform.device_name()
    }

    fn permissions(&self) -> Permissions {
        let granted = self.platform.input_permitted();
        Permissions {
            accessibility: granted,
            input_monitoring: granted,
        }
    }

    fn request_permission(&self, _kind: PermissionKind) {}

    fn watch_peers(&self, enabled: bool, events: Events) {
        self.peers.watch(enabled, events);
    }

    fn peers(&self) -> Vec<String> {
        self.peers.names()
    }

    fn controller_state(&self) -> ControllerState {
        self.controller.state()
    }

    fn start_controller(
        &self,
        config: ControllerConfig,
        events: Events,
    ) -> Result<(), BackendError> {
        self.controller.start(config, events)
    }

    fn stop_controller(&self) {
        self.controller.stop();
    }

    fn receiver_state(&self) -> ReceiverState {
        self.receiver.state()
    }

    fn start_receiver(&self, config: ReceiverConfig, events: Events) -> Result<u16, BackendError> {
        self.receiver.start(config, events)
    }

    fn stop_receiver(&self) {
        self.receiver.stop();
    }

    fn shutdown(&self) {
        self.controller.stop();
        self.receiver.stop();
        self.peers.stop();
    }
}
