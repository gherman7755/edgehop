//! Every method of docs/IPC.md on top of a [`Backend`], with the same validation as the Swift core.

use serde_json::{json, Map, Value};

use crate::backend::{
    Backend, BackendError, ControllerConfig, Events, PermissionKind, ReceiverConfig, Side, Target,
};
use crate::ipc::{Emitter, Handler, IpcError};
use crate::params::Params;
use crate::{IPC_VERSION, VERSION};

/// Tokens shorter than this are rejected.
pub const MIN_TOKEN_LEN: usize = 12;

/// Letters and digits without 0/O/1/I (easy to read aloud). 32 symbols, so `byte & 31` is unbiased.
const TOKEN_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn generate_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 20];
    getrandom::fill(&mut bytes)?;
    Ok(bytes
        .iter()
        .map(|b| char::from(TOKEN_ALPHABET[usize::from(b & 31)]))
        .collect())
}

pub struct CoreService<B: Backend> {
    backend: B,
    events: Events,
}

impl<B: Backend> CoreService<B> {
    pub fn new(backend: B, emitter: Emitter) -> Self {
        Self {
            backend,
            events: Events::new(emitter),
        }
    }

    pub fn backend(&self) -> &B {
        &self.backend
    }

    fn token(params: &Params) -> Result<String, IpcError> {
        let token = params.required_string("token")?;
        if token.chars().count() < MIN_TOKEN_LEN {
            return Err(IpcError::InvalidParams(format!(
                "token must be at least {MIN_TOKEN_LEN} characters"
            )));
        }
        Ok(token.to_string())
    }

    fn start_controller(&self, params: &Params) -> Result<Value, IpcError> {
        let token = Self::token(params)?;
        let peer = params.string("peer")?;
        let host = params.string("host")?;
        if peer.is_some() == host.is_some() {
            return Err(IpcError::InvalidParams(
                "give exactly one of \"peer\" and \"host\"".into(),
            ));
        }
        let port =
            u16::try_from(params.int("port", 1..=65535)?.unwrap_or(47474)).expect("range-checked");
        let side = Side::parse(params.string("side")?.unwrap_or("right")).ok_or_else(|| {
            IpcError::InvalidParams("\"side\" must be left, right, top or bottom".into())
        })?;
        let target = if let Some(name) = peer {
            if !self.backend.peers().iter().any(|p| p == name) {
                return Err(IpcError::InvalidParams(format!(
                    "no computer named \"{name}\" was found nearby (is peers.watch on?)"
                )));
            }
            Target::Peer(name.to_string())
        } else {
            let host = host.unwrap_or_default().trim();
            if host.is_empty() || host.contains('@') {
                return Err(IpcError::InvalidParams(
                    "\"host\" must be an address without \"user@\"".into(),
                ));
            }
            Target::Host {
                host: host.to_string(),
                port,
            }
        };
        let config = ControllerConfig {
            target,
            token,
            side,
            switching: params.bool("switching", true)?,
            forward_keys: params.bool("forwardKeys", false)?,
            debug: params.bool("debug", false)?,
        };
        if self.backend.controller_state().is_active() {
            return Err(IpcError::Busy(
                "a controller session is already running".into(),
            ));
        }
        self.backend
            .start_controller(config, self.events.clone())
            .map_err(into_ipc)?;
        Ok(json!({}))
    }

    fn start_receiver(&self, params: &Params) -> Result<Value, IpcError> {
        let token = Self::token(params)?;
        let port =
            u16::try_from(params.int("port", 0..=65535)?.unwrap_or(47474)).expect("range-checked");
        let config = ReceiverConfig {
            token,
            allow_keys: params.bool("allowKeys", false)?,
            debug: params.bool("debug", false)?,
            port,
        };
        if self.backend.receiver_state() != crate::backend::ReceiverState::Stopped {
            return Err(IpcError::Busy("the receiver is already running".into()));
        }
        let actual = self
            .backend
            .start_receiver(config, self.events.clone())
            .map_err(into_ipc)?;
        Ok(json!({"port": actual}))
    }
}

fn into_ipc(error: BackendError) -> IpcError {
    IpcError::Internal(error.to_string())
}

impl<B: Backend> Handler for CoreService<B> {
    fn handle(&self, method: &str, raw: &Map<String, Value>) -> Result<Value, IpcError> {
        let params = Params(raw);
        match method {
            "core.hello" => Ok(json!({
                "name": "EdgeHop core",
                "version": VERSION,
                "ipcVersion": IPC_VERSION,
                "platform": self.backend.platform(),
                "deviceName": self.backend.device_name(),
            })),
            "permissions.get" => {
                let permissions = self.backend.permissions();
                Ok(
                    json!({"accessibility": permissions.accessibility, "inputMonitoring": permissions.input_monitoring}),
                )
            }
            "permissions.request" => {
                let kind = match params.required_string("kind")? {
                    "accessibility" => PermissionKind::Accessibility,
                    "inputMonitoring" => PermissionKind::InputMonitoring,
                    _ => {
                        return Err(IpcError::InvalidParams(
                            "\"kind\" must be \"accessibility\" or \"inputMonitoring\"".into(),
                        ))
                    }
                };
                self.backend.request_permission(kind);
                Ok(json!({}))
            }
            "token.generate" => {
                let token = generate_token()
                    .map_err(|e| IpcError::Internal(format!("no random numbers: {e}")))?;
                Ok(json!({"token": token}))
            }
            "peers.watch" => {
                self.backend
                    .watch_peers(params.bool("enabled", true)?, self.events.clone());
                Ok(json!({}))
            }
            "peers.list" => Ok(
                json!({"peers": self.backend.peers().iter().map(|n| json!({"name": n})).collect::<Vec<_>>()}),
            ),
            "controller.start" => self.start_controller(&params),
            "controller.stop" => {
                self.backend.stop_controller();
                Ok(json!({}))
            }
            "controller.state" => Ok(self.backend.controller_state().to_json()),
            "receiver.start" => self.start_receiver(&params),
            "receiver.stop" => {
                self.backend.stop_receiver();
                Ok(json!({}))
            }
            "receiver.state" => Ok(self.backend.receiver_state().to_json()),
            other => Err(IpcError::UnknownMethod(format!("unknown method {other}"))),
        }
    }

    fn shutdown(&self) {
        self.backend.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{ControllerState, Permissions, ReceiverState, Source};
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Inner {
        controller: Option<ControllerState>,
        receiver: Option<ReceiverState>,
        peers: Vec<String>,
        started_controller: Vec<ControllerConfig>,
        started_receiver: Vec<ReceiverConfig>,
        receiver_result: Option<Result<u16, BackendError>>,
        watched: Vec<bool>,
        requested: Vec<PermissionKind>,
        stops: u32,
        shutdowns: u32,
    }

    #[derive(Clone, Default)]
    struct Fake(Arc<Mutex<Inner>>);

    impl Backend for Fake {
        fn platform(&self) -> &'static str {
            "testos"
        }
        fn device_name(&self) -> String {
            "Test PC".into()
        }
        fn permissions(&self) -> Permissions {
            Permissions {
                accessibility: true,
                input_monitoring: false,
            }
        }
        fn request_permission(&self, kind: PermissionKind) {
            self.0.lock().unwrap().requested.push(kind);
        }
        fn watch_peers(&self, enabled: bool, _events: Events) {
            self.0.lock().unwrap().watched.push(enabled);
        }
        fn peers(&self) -> Vec<String> {
            self.0.lock().unwrap().peers.clone()
        }
        fn controller_state(&self) -> ControllerState {
            self.0
                .lock()
                .unwrap()
                .controller
                .clone()
                .unwrap_or(ControllerState::Idle)
        }
        fn start_controller(
            &self,
            config: ControllerConfig,
            _events: Events,
        ) -> Result<(), BackendError> {
            self.0.lock().unwrap().started_controller.push(config);
            Ok(())
        }
        fn stop_controller(&self) {
            self.0.lock().unwrap().stops += 1;
        }
        fn receiver_state(&self) -> ReceiverState {
            self.0
                .lock()
                .unwrap()
                .receiver
                .clone()
                .unwrap_or(ReceiverState::Stopped)
        }
        fn start_receiver(
            &self,
            config: ReceiverConfig,
            _events: Events,
        ) -> Result<u16, BackendError> {
            let mut inner = self.0.lock().unwrap();
            inner.started_receiver.push(config.clone());
            inner.receiver_result.clone().unwrap_or(Ok(config.port))
        }
        fn stop_receiver(&self) {
            self.0.lock().unwrap().stops += 1;
        }
        fn shutdown(&self) {
            self.0.lock().unwrap().shutdowns += 1;
        }
    }

    const TOKEN: &str = "ABCDEFGHJKLMNPQRSTUV";

    fn setup() -> (CoreService<Fake>, Fake) {
        let backend = Fake::default();
        let service = CoreService::new(backend.clone(), Emitter::new(std::io::sink()));
        (service, backend)
    }

    fn call(service: &CoreService<Fake>, method: &str, params: Value) -> Result<Value, IpcError> {
        service.handle(method, params.as_object().unwrap())
    }

    fn invalid(service: &CoreService<Fake>, method: &str, params: Value, containing: &str) {
        match call(service, method, params.clone()) {
            Err(IpcError::InvalidParams(message)) => {
                assert!(
                    message.contains(containing),
                    "message was {message:?} for {params}"
                )
            }
            other => panic!("expected invalid_params for {params}, got {other:?}"),
        }
    }

    #[test]
    fn hello_describes_the_core() {
        let (service, _) = setup();
        let hello = call(&service, "core.hello", json!({})).unwrap();
        assert_eq!(hello["ipcVersion"], json!(1));
        assert_eq!(hello["platform"], json!("testos"));
        assert_eq!(hello["deviceName"], json!("Test PC"));
        assert_eq!(hello["name"], json!("EdgeHop core"));
    }

    #[test]
    fn tokens_are_twenty_readable_characters_and_differ() {
        let (service, _) = setup();
        let a = call(&service, "token.generate", json!({})).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string();
        let b = call(&service, "token.generate", json!({})).unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string();
        assert_eq!(a.len(), 20);
        assert_ne!(a, b);
        assert!(
            a.bytes().all(|c| TOKEN_ALPHABET.contains(&c)),
            "unexpected character in {a}"
        );
    }

    #[test]
    fn permissions_come_from_the_backend() {
        let (service, backend) = setup();
        assert_eq!(
            call(&service, "permissions.get", json!({})).unwrap(),
            json!({"accessibility": true, "inputMonitoring": false})
        );
        call(
            &service,
            "permissions.request",
            json!({"kind": "inputMonitoring"}),
        )
        .unwrap();
        assert_eq!(
            backend.0.lock().unwrap().requested,
            vec![PermissionKind::InputMonitoring]
        );
        invalid(
            &service,
            "permissions.request",
            json!({"kind": "camera"}),
            "kind",
        );
        invalid(&service, "permissions.request", json!({}), "kind");
    }

    #[test]
    fn idle_states_and_harmless_stops() {
        let (service, backend) = setup();
        assert_eq!(
            call(&service, "controller.state", json!({})).unwrap(),
            json!({"state": "idle"})
        );
        assert_eq!(
            call(&service, "receiver.state", json!({})).unwrap(),
            json!({"state": "stopped"})
        );
        assert_eq!(
            call(&service, "controller.stop", json!({})).unwrap(),
            json!({})
        );
        assert_eq!(
            call(&service, "receiver.stop", json!({})).unwrap(),
            json!({})
        );
        assert_eq!(
            call(&service, "peers.list", json!({})).unwrap(),
            json!({"peers": []})
        );
        assert_eq!(backend.0.lock().unwrap().stops, 2);
    }

    #[test]
    fn unknown_methods_are_reported() {
        let (service, _) = setup();
        assert_eq!(
            call(&service, "nope", json!({})),
            Err(IpcError::UnknownMethod("unknown method nope".into()))
        );
    }

    #[test]
    fn controller_start_validates_params() {
        let (service, backend) = setup();
        invalid(&service, "controller.start", json!({}), "token");
        invalid(
            &service,
            "controller.start",
            json!({"token": "short", "host": "10.0.0.2"}),
            "at least 12",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": 12345}),
            "string",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN}),
            "exactly one",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "peer": "Mac"}),
            "exactly one",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "me@10.0.0.2"}),
            "user@",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "   "}),
            "host",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "side": "up"}),
            "side",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "port": 70000}),
            "port",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "port": true}),
            "port",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "switching": 1}),
            "switching",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "host": "10.0.0.2", "forwardKeys": "yes"}),
            "forwardKeys",
        );
        invalid(
            &service,
            "controller.start",
            json!({"token": TOKEN, "peer": "Nobody"}),
            "no computer named",
        );
        assert!(
            backend.0.lock().unwrap().started_controller.is_empty(),
            "nothing may start on invalid input"
        );
    }

    #[test]
    fn controller_start_passes_a_host_config_with_defaults() {
        let (service, backend) = setup();
        assert_eq!(
            call(
                &service,
                "controller.start",
                json!({"token": TOKEN, "host": " 10.0.0.2 "})
            )
            .unwrap(),
            json!({})
        );
        let started = backend.0.lock().unwrap().started_controller.clone();
        assert_eq!(
            started,
            vec![ControllerConfig {
                target: Target::Host {
                    host: "10.0.0.2".into(),
                    port: 47474
                },
                token: TOKEN.into(),
                side: Side::Right,
                switching: true,
                forward_keys: false,
                debug: false,
            }]
        );
    }

    #[test]
    fn controller_start_resolves_a_nearby_peer() {
        let (service, backend) = setup();
        backend.0.lock().unwrap().peers = vec!["MacBook Pro".into()];
        call(
            &service,
            "controller.start",
            json!({"token": TOKEN, "peer": "MacBook Pro", "side": "left", "switching": false, "forwardKeys": true, "debug": true}),
        )
        .unwrap();
        let started = backend.0.lock().unwrap().started_controller.clone();
        assert_eq!(started[0].target, Target::Peer("MacBook Pro".into()));
        assert_eq!(started[0].side, Side::Left);
        assert!(!started[0].switching && started[0].forward_keys && started[0].debug);
    }

    #[test]
    fn a_second_controller_start_is_busy_but_a_failed_one_can_be_retried() {
        let (service, backend) = setup();
        let params = json!({"token": TOKEN, "host": "10.0.0.2"});
        backend.0.lock().unwrap().controller = Some(ControllerState::Connecting);
        assert!(matches!(
            call(&service, "controller.start", params.clone()),
            Err(IpcError::Busy(_))
        ));
        backend.0.lock().unwrap().controller = Some(ControllerState::Connected {
            peer: "Mac".into(),
            remote_active: false,
        });
        assert!(matches!(
            call(&service, "controller.start", params.clone()),
            Err(IpcError::Busy(_))
        ));
        backend.0.lock().unwrap().controller = Some(ControllerState::Failed {
            message: "x".into(),
        });
        assert!(call(&service, "controller.start", params).is_ok());
    }

    #[test]
    fn receiver_start_validates_params_and_returns_the_actual_port() {
        let (service, backend) = setup();
        invalid(&service, "receiver.start", json!({}), "token");
        invalid(
            &service,
            "receiver.start",
            json!({"token": "short"}),
            "at least 12",
        );
        invalid(
            &service,
            "receiver.start",
            json!({"token": TOKEN, "port": -1}),
            "port",
        );
        invalid(
            &service,
            "receiver.start",
            json!({"token": TOKEN, "allowKeys": "no"}),
            "allowKeys",
        );
        assert!(backend.0.lock().unwrap().started_receiver.is_empty());

        assert_eq!(
            call(&service, "receiver.start", json!({"token": TOKEN})).unwrap(),
            json!({"port": 47474})
        );
        assert_eq!(
            call(
                &service,
                "receiver.start",
                json!({"token": TOKEN, "port": 0})
            )
            .unwrap(),
            json!({"port": 0})
        );
        assert_eq!(
            backend.0.lock().unwrap().started_receiver[0],
            ReceiverConfig {
                token: TOKEN.into(),
                allow_keys: false,
                debug: false,
                port: 47474
            }
        );
    }

    #[test]
    fn receiver_start_is_busy_when_running_and_reports_backend_failures() {
        let (service, backend) = setup();
        backend.0.lock().unwrap().receiver = Some(ReceiverState::Listening { port: 1 });
        assert!(matches!(
            call(&service, "receiver.start", json!({"token": TOKEN})),
            Err(IpcError::Busy(_))
        ));
        backend.0.lock().unwrap().receiver = None;
        backend.0.lock().unwrap().receiver_result =
            Some(Err(BackendError::Failed("port 47474 is taken".into())));
        assert_eq!(
            call(&service, "receiver.start", json!({"token": TOKEN})),
            Err(IpcError::Internal("port 47474 is taken".into()))
        );
    }

    #[test]
    fn peers_watch_toggles_discovery_and_defaults_to_on() {
        let (service, backend) = setup();
        call(&service, "peers.watch", json!({})).unwrap();
        call(&service, "peers.watch", json!({"enabled": false})).unwrap();
        assert_eq!(backend.0.lock().unwrap().watched, vec![true, false]);
        invalid(&service, "peers.watch", json!({"enabled": 1}), "enabled");
        backend.0.lock().unwrap().peers = vec!["A".into(), "B".into()];
        assert_eq!(
            call(&service, "peers.list", json!({})).unwrap(),
            json!({"peers": [{"name": "A"}, {"name": "B"}]})
        );
    }

    #[test]
    fn shutdown_reaches_the_backend() {
        let (service, backend) = setup();
        service.shutdown();
        assert_eq!(backend.0.lock().unwrap().shutdowns, 1);
    }

    #[test]
    fn events_are_written_in_the_documented_shapes() {
        #[derive(Clone, Default)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink::default();
        let events = Events::new(Emitter::new(sink.clone()));
        events.controller_state(&ControllerState::Connecting);
        events.receiver_state(&ReceiverState::Listening { port: 5 });
        events.latency(12.0);
        events.log(Source::Receiver, "hello");
        events.peers_changed(&["A".to_string()]);
        let text = String::from_utf8(sink.0.lock().unwrap().clone()).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            lines[0],
            json!({"event": "controller.state", "data": {"state": "connecting"}})
        );
        assert_eq!(
            lines[1],
            json!({"event": "receiver.state", "data": {"state": "listening", "port": 5}})
        );
        assert_eq!(lines[2], json!({"event": "latency", "data": {"ms": 12.0}}));
        assert_eq!(
            lines[3],
            json!({"event": "log", "data": {"source": "receiver", "line": "hello"}})
        );
        assert_eq!(
            lines[4],
            json!({"event": "peers.changed", "data": {"peers": [{"name": "A"}]}})
        );
    }
}
