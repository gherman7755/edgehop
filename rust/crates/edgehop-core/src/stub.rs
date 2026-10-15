//! Placeholder backend: answers every IPC method truthfully but cannot run sessions yet (the network link and
//! the Windows input layer come later). It lets the UI be developed and the protocol be tested end to end.

use crate::backend::{
    Backend, BackendError, ControllerConfig, ControllerState, Events, PermissionKind, Permissions,
    ReceiverConfig, ReceiverState,
};

#[derive(Default)]
pub struct StubBackend;

impl StubBackend {
    pub fn new() -> Self {
        Self
    }
}

const NOT_YET: &str = "this core build cannot run sessions yet (the network link and input layer are not implemented)";

impl Backend for StubBackend {
    fn platform(&self) -> &'static str {
        std::env::consts::OS
    }

    fn device_name(&self) -> String {
        std::env::var("COMPUTERNAME")
            .or_else(|_| std::env::var("HOSTNAME"))
            .unwrap_or_else(|_| "this computer".to_string())
    }

    fn permissions(&self) -> Permissions {
        // Windows has no per-app input permissions; elsewhere the stub cannot claim any.
        let granted = cfg!(windows);
        Permissions {
            accessibility: granted,
            input_monitoring: granted,
        }
    }

    fn request_permission(&self, _kind: PermissionKind) {}

    fn watch_peers(&self, _enabled: bool, _events: Events) {}

    fn peers(&self) -> Vec<String> {
        Vec::new()
    }

    fn controller_state(&self) -> ControllerState {
        ControllerState::Idle
    }

    fn start_controller(
        &self,
        _config: ControllerConfig,
        _events: Events,
    ) -> Result<(), BackendError> {
        Err(BackendError::Unsupported(NOT_YET.into()))
    }

    fn stop_controller(&self) {}

    fn receiver_state(&self) -> ReceiverState {
        ReceiverState::Stopped
    }

    fn start_receiver(
        &self,
        _config: ReceiverConfig,
        _events: Events,
    ) -> Result<u16, BackendError> {
        Err(BackendError::Unsupported(NOT_YET.into()))
    }

    fn stop_receiver(&self) {}

    fn shutdown(&self) {}
}
