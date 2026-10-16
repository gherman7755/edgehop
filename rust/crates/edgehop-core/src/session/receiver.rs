//! The receiver: the computer being controlled. Listens, authenticates one controller and injects its input.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use edgehop_input::{Injector, Platform};
use edgehop_link::{LinkServer, ServerConfig, ServerHandler};
use edgehop_proto::{ControlMessage, HelloMessage, InputEvent};

use super::{log, Tracked};
use crate::backend::{BackendError, Events, ReceiverConfig, ReceiverState, Source};

struct Handler<P: Platform> {
    injector: Mutex<Injector<P::Output>>,
    allow_keys: bool,
    debug: bool,
    received: AtomicUsize,
    /// Set before the server is stopped so the disconnect that follows does not announce "listening" again.
    stopping: AtomicBool,
    port: Mutex<u16>,
    state: Arc<Tracked<ReceiverState>>,
    events: Events,
}

impl<P: Platform> Handler<P> {
    fn release_all(&self) {
        self.injector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .release_all();
    }

    fn set_state(&self, state: ReceiverState) {
        if !self.stopping.load(Ordering::SeqCst) {
            self.state.set(state, |s| self.events.receiver_state(s));
        }
    }
}

impl<P: Platform> ServerHandler for Handler<P> {
    fn on_peer_connected(&self, hello: &HelloMessage) {
        self.received.store(0, Ordering::SeqCst);
        log(
            &self.events,
            Source::Receiver,
            format!("peer connected: {}", hello.name),
        );
        self.set_state(ReceiverState::Connected {
            peer: hello.name.clone(),
        });
    }

    fn on_input(&self, event: InputEvent) {
        let count = self.received.fetch_add(1, Ordering::SeqCst) + 1;
        if matches!(event, InputEvent::Key { .. }) && !self.allow_keys {
            return;
        }
        self.injector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .inject(&event);
        if self.debug && (count <= 8 || count % 100 == 0) {
            log(
                &self.events,
                Source::Receiver,
                format!("inject #{count} {event:?}"),
            );
        }
    }

    fn on_control(&self, message: ControlMessage) {
        if message == ControlMessage::ReleaseAll {
            self.release_all();
            log(
                &self.events,
                Source::Receiver,
                "peer asked to release all keys/buttons",
            );
        }
    }

    fn on_peer_disconnected(&self) {
        self.release_all();
        log(
            &self.events,
            Source::Receiver,
            "peer disconnected (all keys/buttons released)",
        );
        let port = *self.port.lock().unwrap_or_else(PoisonError::into_inner);
        self.set_state(ReceiverState::Listening { port });
    }
}

struct Run<P: Platform> {
    server: LinkServer,
    handler: Arc<Handler<P>>,
    events: Events,
    state: Arc<Tracked<ReceiverState>>,
}

impl<P: Platform> Run<P> {
    fn stop(self) {
        self.handler.stopping.store(true, Ordering::SeqCst);
        self.server.stop();
        self.handler.release_all();
        log(&self.events, Source::Receiver, "stopped");
        self.state
            .set(ReceiverState::Stopped, |s| self.events.receiver_state(s));
    }
}

pub(super) struct ReceiverSessions<P: Platform> {
    platform: Arc<P>,
    state: Arc<Tracked<ReceiverState>>,
    run: Mutex<Option<Run<P>>>,
}

impl<P: Platform> ReceiverSessions<P> {
    pub(super) fn new(platform: Arc<P>) -> Self {
        Self {
            platform,
            state: Arc::new(Tracked::new(ReceiverState::Stopped)),
            run: Mutex::new(None),
        }
    }

    pub(super) fn state(&self) -> ReceiverState {
        self.state.get()
    }

    pub(super) fn start(
        &self,
        config: ReceiverConfig,
        events: Events,
    ) -> Result<u16, BackendError> {
        let mut slot = self.run.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.is_some() {
            return Err(BackendError::Failed(
                "the receiver is already running".into(),
            ));
        }
        let name = self.platform.device_name();
        let handler = Arc::new(Handler::<P> {
            injector: Mutex::new(Injector::new(self.platform.output())),
            allow_keys: config.allow_keys,
            debug: config.debug,
            received: AtomicUsize::new(0),
            stopping: AtomicBool::new(false),
            port: Mutex::new(0),
            state: Arc::clone(&self.state),
            events: events.clone(),
        });
        let mut server_config = ServerConfig::new(config.token.clone(), name.clone());
        server_config.port = config.port;
        server_config.monitors = self.platform.displays();
        let server = LinkServer::start(
            server_config,
            Arc::clone(&handler) as Arc<dyn ServerHandler>,
        )
        .map_err(|e| {
            BackendError::Failed(if config.port == 0 {
                format!("cannot listen: {e}")
            } else {
                format!("cannot listen on port {}: {e}", config.port)
            })
        })?;
        let port = server.port();
        *handler.port.lock().unwrap_or_else(PoisonError::into_inner) = port;
        log(
            &events,
            Source::Receiver,
            format!(
                "listening on port {port} as \"{name}\" (keys: {})",
                if config.allow_keys {
                    "ALLOWED, unencrypted!"
                } else {
                    "dropped"
                }
            ),
        );
        self.state.set(ReceiverState::Listening { port }, |s| {
            events.receiver_state(s)
        });
        *slot = Some(Run {
            server,
            handler,
            events,
            state: Arc::clone(&self.state),
        });
        Ok(port)
    }

    pub(super) fn stop(&self) {
        let run = self
            .run
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(run) = run {
            run.stop();
        }
    }
}
