//! What a platform must provide to the core: sessions (controller and receiver), discovery and permissions.
//! The Windows core implements [`Backend`] with the real link and input layers; [`crate::stub`] is the
//! placeholder used until then.

use serde_json::{json, Value};
use thiserror::Error;

use crate::ipc::Emitter;

/// Where the other computer sits relative to this one.
pub use edgehop_switch::Side;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    Host {
        host: String,
        port: u16,
    },
    /// A computer name from `peers.list`.
    Peer(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerConfig {
    pub target: Target,
    pub token: String,
    pub side: Side,
    /// Edge switching; `false` mirrors the mouse on both computers.
    pub switching: bool,
    pub forward_keys: bool,
    pub debug: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiverConfig {
    pub token: String,
    pub allow_keys: bool,
    pub debug: bool,
    /// 0 picks a free port.
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControllerState {
    Idle,
    Connecting,
    Connected { peer: String, remote_active: bool },
    Failed { message: String },
}

impl ControllerState {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Idle => json!({"state": "idle"}),
            Self::Connecting => json!({"state": "connecting"}),
            Self::Connected {
                peer,
                remote_active,
            } => json!({"state": "connected", "peer": peer, "remoteActive": remote_active}),
            Self::Failed { message } => json!({"state": "failed", "message": message}),
        }
    }

    /// True while a session is running or starting (a second start would be `busy`).
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Connecting | Self::Connected { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiverState {
    Stopped,
    Listening { port: u16 },
    Connected { peer: String },
}

impl ReceiverState {
    pub fn to_json(&self) -> Value {
        match self {
            Self::Stopped => json!({"state": "stopped"}),
            Self::Listening { port } => json!({"state": "listening", "port": port}),
            Self::Connected { peer } => json!({"state": "connected", "peer": peer}),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    pub accessibility: bool,
    pub input_monitoring: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
    Accessibility,
    InputMonitoring,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Controller,
    Receiver,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum BackendError {
    /// This build cannot do it (yet).
    #[error("{0}")]
    Unsupported(String),
    /// It was tried and failed (for example the port is taken).
    #[error("{0}")]
    Failed(String),
}

/// Typed access to the IPC event stream for backends (docs/IPC.md, "Events").
#[derive(Clone)]
pub struct Events(Emitter);

impl Events {
    pub fn new(emitter: Emitter) -> Self {
        Self(emitter)
    }

    pub fn controller_state(&self, state: &ControllerState) {
        self.0.emit("controller.state", state.to_json());
    }

    pub fn receiver_state(&self, state: &ReceiverState) {
        self.0.emit("receiver.state", state.to_json());
    }

    pub fn latency(&self, millis: f64) {
        self.0.emit("latency", json!({"ms": millis}));
    }

    pub fn log(&self, source: Source, line: &str) {
        let source = match source {
            Source::Controller => "controller",
            Source::Receiver => "receiver",
        };
        self.0.emit("log", json!({"source": source, "line": line}));
    }

    pub fn peers_changed(&self, names: &[String]) {
        self.0.emit(
            "peers.changed",
            json!({"peers": names.iter().map(|n| json!({"name": n})).collect::<Vec<_>>()}),
        );
    }
}

/// The platform side of the core. Methods may be called from the IPC thread while the backend's own threads
/// emit events, hence `Send + Sync`.
pub trait Backend: Send + Sync {
    /// `"windows"`, `"macos"`, ... reported by `core.hello`.
    fn platform(&self) -> &'static str;
    fn device_name(&self) -> String;

    fn permissions(&self) -> Permissions;
    /// Show the system prompt, if the platform has one.
    fn request_permission(&self, kind: PermissionKind);

    /// Start or stop discovery. While enabled the backend emits `peers.changed` through `events`.
    fn watch_peers(&self, enabled: bool, events: Events);
    fn peers(&self) -> Vec<String>;

    fn controller_state(&self) -> ControllerState;
    /// Returns once the session is starting; progress arrives as `controller.state` events.
    fn start_controller(
        &self,
        config: ControllerConfig,
        events: Events,
    ) -> Result<(), BackendError>;
    fn stop_controller(&self);

    fn receiver_state(&self) -> ReceiverState;
    /// Returns the port actually listened on.
    fn start_receiver(&self, config: ReceiverConfig, events: Events) -> Result<u16, BackendError>;
    fn stop_receiver(&self);

    /// Stop everything and give input back to the user. Must be idempotent.
    fn shutdown(&self);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_encode_like_the_swift_core() {
        assert_eq!(ControllerState::Idle.to_json(), json!({"state": "idle"}));
        assert_eq!(
            ControllerState::Connecting.to_json(),
            json!({"state": "connecting"})
        );
        assert_eq!(
            ControllerState::Connected {
                peer: "Mac".into(),
                remote_active: true
            }
            .to_json(),
            json!({"state": "connected", "peer": "Mac", "remoteActive": true})
        );
        assert_eq!(
            ControllerState::Failed {
                message: "no".into()
            }
            .to_json(),
            json!({"state": "failed", "message": "no"})
        );
        assert_eq!(
            ReceiverState::Stopped.to_json(),
            json!({"state": "stopped"})
        );
        assert_eq!(
            ReceiverState::Listening { port: 47474 }.to_json(),
            json!({"state": "listening", "port": 47474})
        );
        assert_eq!(
            ReceiverState::Connected { peer: "Mac".into() }.to_json(),
            json!({"state": "connected", "peer": "Mac"})
        );
    }

    #[test]
    fn only_connecting_and_connected_are_active() {
        assert!(!ControllerState::Idle.is_active());
        assert!(!ControllerState::Failed {
            message: String::new()
        }
        .is_active());
        assert!(ControllerState::Connecting.is_active());
        assert!(ControllerState::Connected {
            peer: String::new(),
            remote_active: false
        }
        .is_active());
    }

    #[test]
    fn sides_parse_strictly() {
        assert_eq!(Side::parse("left"), Some(Side::Left));
        assert_eq!(Side::parse("bottom"), Some(Side::Bottom));
        assert_eq!(Side::parse("up"), None);
        assert_eq!(Side::parse("Right"), None);
    }
}
