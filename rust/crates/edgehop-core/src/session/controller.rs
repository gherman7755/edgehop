//! The controller: the computer whose keyboard and mouse are shared. Connects to a receiver, captures local
//! input and hands it over at the screen edge.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use edgehop_input::{CaptureHandle, CaptureSink, Platform};
use edgehop_link::{ClientConfig, ClientHandler, LinkClient, LinkError};
use edgehop_proto::{is_panic_chord, ControlMessage, InputEvent, MonitorInfo};
use edgehop_switch::{Actions, Mode, Point, Rect, SwitchController};

use super::{log, Tracked};
use crate::backend::{BackendError, ControllerConfig, ControllerState, Events, Source, Target};

const PING_INTERVAL: Duration = Duration::from_secs(2);

type Controller = SwitchController<SessionActions>;

/// State shared by the start thread, the link's reader thread and the user's stop.
struct Shared {
    state: Tracked<ControllerState>,
    /// Bumped by every start and stop: a thread that finds it changed has been cancelled and must not touch
    /// the state any more.
    generation: AtomicU64,
    run: Mutex<Option<Run>>,
    /// Where the current session's events go: a stop has no events handle of its own.
    events: Mutex<Option<Events>>,
}

impl Shared {
    fn set_state(&self, events: &Events, state: ControllerState) {
        self.state.set(state, |s| events.controller_state(s));
    }

    fn take_run(&self) -> Option<Run> {
        self.run
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }
}

/// Everything a running session owns.
struct Run {
    client: Arc<LinkClient>,
    controller: Option<Arc<Controller>>,
    capture: Arc<dyn CaptureHandle>,
    ping_stop: mpsc::Sender<()>,
    ping: Option<JoinHandle<()>>,
}

impl Run {
    /// Always gives the cursor back, whatever ends the session.
    fn teardown(self, events: &Events) {
        if let Some(controller) = &self.controller {
            controller.return_to_local();
        }
        self.capture.stop();
        drop(self.ping_stop);
        if let Some(ping) = self.ping {
            if ping.thread().id() != thread::current().id() {
                let _ = ping.join();
            }
        }
        self.client.close();
        log(events, Source::Controller, "session ended");
    }
}

struct LinkEvents {
    shared: Arc<Shared>,
    generation: u64,
    closed: Arc<AtomicBool>,
    events: Events,
}

impl LinkEvents {
    /// Ends the session if the link is gone. Safe to call any number of times.
    fn finish_if_closed(&self) {
        if self.shared.generation.load(Ordering::SeqCst) != self.generation {
            return;
        }
        if let Some(run) = self.shared.take_run() {
            log(
                &self.events,
                Source::Controller,
                "link closed by the other computer",
            );
            run.teardown(&self.events);
            self.shared.set_state(&self.events, ControllerState::Idle);
        }
    }
}

impl ClientHandler for LinkEvents {
    fn on_pong(&self, rtt_millis: f64) {
        self.events.latency(rtt_millis);
    }

    fn on_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.finish_if_closed();
    }
}

/// Carries out what the switch controller decides.
struct SessionActions {
    client: Arc<LinkClient>,
    capture: Arc<OnceLock<Arc<dyn CaptureHandle>>>,
    shared: Arc<Shared>,
    events: Events,
    peer: String,
    debug: bool,
    sent: AtomicUsize,
}

impl Actions for SessionActions {
    fn send(&self, event: InputEvent) {
        let _ = self.client.send_input(&event);
        if self.debug {
            let count = self.sent.fetch_add(1, Ordering::SeqCst) + 1;
            if count <= 8 || count % 60 == 0 {
                log(
                    &self.events,
                    Source::Controller,
                    format!("send #{count} {event:?}"),
                );
            }
        }
    }

    fn enter_remote(&self) {
        self.sent.store(0, Ordering::SeqCst);
        let mut parked = String::new();
        if let Some(capture) = self.capture.get() {
            capture.enter_remote();
            if let Some((x, y)) = capture.cursor_position() {
                parked = format!("; local cursor now at ({}, {})", x as i64, y as i64);
            }
        }
        log(
            &self.events,
            Source::Controller,
            format!("-> REMOTE ({}){parked}", self.peer),
        );
        self.shared.set_state(
            &self.events,
            ControllerState::Connected {
                peer: self.peer.clone(),
                remote_active: true,
            },
        );
    }

    fn leave_remote(&self, to: Point) {
        if let Some(capture) = self.capture.get() {
            capture.leave_remote((to.x, to.y));
        }
        log(
            &self.events,
            Source::Controller,
            format!("<- LOCAL at ({}, {})", to.x as i64, to.y as i64),
        );
        self.shared.set_state(
            &self.events,
            ControllerState::Connected {
                peer: self.peer.clone(),
                remote_active: false,
            },
        );
    }

    fn release_remote(&self) {
        let _ = self.client.send_control(ControlMessage::ReleaseAll);
    }
}

/// Receives the captured input.
struct Sink {
    /// `None` in mirror mode: both cursors move.
    controller: Option<Arc<Controller>>,
    client: Arc<LinkClient>,
    forward_keys: bool,
    last_position: Mutex<Option<(f64, f64)>>,
    /// The panic chord was typed: stop sharing. Runs in the capture thread, so it must only hand the work over.
    on_panic: Box<dyn Fn() + Send + Sync>,
    /// Debug logging of the motion path: where the local cursor really is while control is remote, and how long
    /// the slowest call took (the hook has about 300 ms before Windows abandons it).
    diagnostics: Option<Diagnostics>,
}

struct Diagnostics {
    events: Events,
    capture: Arc<OnceLock<Arc<dyn CaptureHandle>>>,
    motions: AtomicUsize,
    slowest_micros: AtomicU64,
}

impl Diagnostics {
    fn record(&self, started: Instant, position: (f64, f64), delta: (f64, f64)) {
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        self.slowest_micros.fetch_max(micros, Ordering::SeqCst);
        let count = self.motions.fetch_add(1, Ordering::SeqCst) + 1;
        if count <= 8 || count % 60 == 0 {
            let cursor = self
                .capture
                .get()
                .and_then(|capture| capture.cursor_position())
                .map_or_else(
                    || "?".to_string(),
                    |(x, y)| format!("({}, {})", x as i64, y as i64),
                );
            log(
                &self.events,
                Source::Controller,
                format!(
                    "motion #{count} pos=({}, {}) delta=({}, {}) local cursor={cursor} slowest={} us",
                    position.0 as i64,
                    position.1 as i64,
                    delta.0 as i64,
                    delta.1 as i64,
                    self.slowest_micros.swap(0, Ordering::SeqCst)
                ),
            );
        }
    }
}

impl CaptureSink for Sink {
    fn motion(&self, position: (f64, f64), delta: (f64, f64)) {
        if let Some(controller) = &self.controller {
            let started = Instant::now();
            let was_remote = controller.mode() == Mode::Remote;
            controller.handle_motion(
                Point {
                    x: position.0,
                    y: position.1,
                },
                Point {
                    x: delta.0,
                    y: delta.1,
                },
            );
            if let Some(diagnostics) = &self.diagnostics {
                let is_remote = controller.mode() == Mode::Remote;
                if is_remote && !was_remote {
                    diagnostics.motions.store(0, Ordering::SeqCst); // a new crossing: log its first moves
                }
                if is_remote {
                    diagnostics.record(started, position, delta);
                }
            }
            return;
        }
        // Mirror mode: the remote cursor follows the movement this one makes, so screens may differ in size.
        let previous = self
            .last_position
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(position);
        if let Some(previous) = previous {
            let dx = (position.0 - previous.0).round() as i32;
            let dy = (position.1 - previous.1).round() as i32;
            if dx != 0 || dy != 0 {
                let _ = self
                    .client
                    .send_input(&InputEvent::MouseMoveDelta { dx, dy });
            }
        }
    }

    fn event(&self, event: InputEvent) {
        if is_panic_chord(&event) {
            (self.on_panic)();
            return; // the chord is for EdgeHop, not for either computer
        }
        if let Some(controller) = &self.controller {
            if let (Some(diagnostics), InputEvent::MouseButton { button, is_down }) =
                (&self.diagnostics, &event)
            {
                // Before `handle`, so the mode shown is the one the button was pressed in.
                log(
                    &diagnostics.events,
                    Source::Controller,
                    format!(
                        "button {button:?} {} while {:?}",
                        if *is_down { "down" } else { "up" },
                        controller.mode()
                    ),
                );
            }
            controller.handle(&event);
            return;
        }
        match event {
            InputEvent::MouseMoveAbsolute { .. } | InputEvent::MouseMoveDelta { .. } => {}
            InputEvent::Key { .. } if !self.forward_keys => {}
            _ => {
                let _ = self.client.send_input(&event);
            }
        }
    }

    fn should_swallow(&self, event: &InputEvent) -> bool {
        self.controller
            .as_ref()
            .is_some_and(|controller| controller.should_swallow(event))
    }
}

fn describe(error: &LinkError) -> String {
    match error {
        LinkError::Rejected => {
            "The other computer rejected the token. Check that both use the same one.".into()
        }
        LinkError::Protocol(text) => format!("Protocol error: {text}"),
        LinkError::Connection(text) => text.clone(),
    }
}

fn size_text(monitors: &[MonitorInfo]) -> String {
    monitors
        .iter()
        .map(|m| format!("{}x{}", m.width as i64, m.height as i64))
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) struct ControllerSessions<P: Platform> {
    platform: Arc<P>,
    shared: Arc<Shared>,
    start_lock: Mutex<()>,
}

impl<P: Platform> ControllerSessions<P> {
    pub(super) fn new(platform: Arc<P>) -> Self {
        Self {
            platform,
            shared: Arc::new(Shared {
                state: Tracked::new(ControllerState::Idle),
                generation: AtomicU64::new(0),
                run: Mutex::new(None),
                events: Mutex::new(None),
            }),
            start_lock: Mutex::new(()),
        }
    }

    pub(super) fn state(&self) -> ControllerState {
        self.shared.state.get()
    }

    pub(super) fn start(
        &self,
        config: ControllerConfig,
        events: Events,
    ) -> Result<(), BackendError> {
        let Target::Host { host, port } = config.target.clone() else {
            return Err(BackendError::Unsupported(
                "this build cannot find computers by name yet: connect by address".into(),
            ));
        };
        let _guard = self
            .start_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.shared.state.get().is_active() {
            return Err(BackendError::Failed(
                "a controller session is already running".into(),
            ));
        }
        let generation = self.shared.generation.fetch_add(1, Ordering::SeqCst) + 1;
        *self
            .shared
            .events
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(events.clone());
        self.shared.set_state(&events, ControllerState::Connecting);

        let platform = Arc::clone(&self.platform);
        let shared = Arc::clone(&self.shared);
        let thread_events = events.clone();
        let spawned = thread::Builder::new()
            .name("edgehop-controller".into())
            .spawn(move || {
                let outcome = run(
                    &*platform,
                    &shared,
                    generation,
                    &config,
                    &host,
                    port,
                    &thread_events,
                );
                if let Err(message) = outcome {
                    if shared.generation.load(Ordering::SeqCst) == generation {
                        log(
                            &thread_events,
                            Source::Controller,
                            format!("failed: {message}"),
                        );
                        if let Some(run) = shared.take_run() {
                            run.teardown(&thread_events);
                        }
                        shared.set_state(&thread_events, ControllerState::Failed { message });
                    }
                }
            });
        if let Err(error) = spawned {
            let message = format!("cannot start the session: {error}");
            self.shared.set_state(
                &events,
                ControllerState::Failed {
                    message: message.clone(),
                },
            );
            return Err(BackendError::Failed(message));
        }
        Ok(())
    }

    pub(super) fn stop(&self) {
        stop_session(&self.shared);
    }
}

/// Ends the session whatever its state: cancels a connection in progress, gives control back, goes idle.
fn stop_session(shared: &Shared) {
    shared.generation.fetch_add(1, Ordering::SeqCst);
    let events = shared
        .events
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    let Some(events) = events else { return };
    if let Some(run) = shared.take_run() {
        run.teardown(&events);
    }
    shared.set_state(&events, ControllerState::Idle);
}

/// Connects, starts capturing and registers the session. An `Err` is the message to show the user.
fn run<P: Platform>(
    platform: &P,
    shared: &Arc<Shared>,
    generation: u64,
    config: &ControllerConfig,
    host: &str,
    port: u16,
    events: &Events,
) -> Result<(), String> {
    log(
        events,
        Source::Controller,
        format!("connecting to {host}:{port}…"),
    );
    let closed = Arc::new(AtomicBool::new(false));
    let handler = Arc::new(LinkEvents {
        shared: Arc::clone(shared),
        generation,
        closed: Arc::clone(&closed),
        events: events.clone(),
    });
    let mut client_config = ClientConfig::new(config.token.clone(), platform.device_name());
    client_config.monitors = platform.displays();
    let (client, hello) =
        LinkClient::connect(host, port, &client_config, handler).map_err(|e| describe(&e))?;
    let client = Arc::new(client);
    if shared.generation.load(Ordering::SeqCst) != generation {
        client.close(); // stopped while connecting
        return Ok(());
    }
    let monitors = hello.monitors.clone().unwrap_or_default();
    // Keys only go to a computer that reads them the same way.
    let forward_keys = config.forward_keys && hello.speaks_hid_keys();
    if config.forward_keys && !forward_keys {
        log(
            events,
            Source::Controller,
            "the other computer does not announce the shared key codes (update EdgeHop there): the keyboard is not shared",
        );
    }
    log(
        events,
        Source::Controller,
        format!("connected to \"{}\" {}", hello.name, size_text(&monitors)),
    );

    let capture_slot: Arc<OnceLock<Arc<dyn CaptureHandle>>> = Arc::new(OnceLock::new());
    let controller = if config.switching {
        let Some(remote) = monitors
            .iter()
            .find(|m| m.x == 0.0 && m.y == 0.0)
            .or(monitors.first())
        else {
            client.close();
            return Err("the other computer reported no displays".into());
        };
        let displays: Vec<Rect> = platform
            .displays()
            .iter()
            .map(|m| Rect {
                x: m.x,
                y: m.y,
                width: m.width,
                height: m.height,
            })
            .collect();
        log(
            events,
            Source::Controller,
            format!(
                "local displays: {}; remote on the {}",
                displays
                    .iter()
                    .map(|d| format!(
                        "{}x{}@{},{}",
                        d.width as i64, d.height as i64, d.x as i64, d.y as i64
                    ))
                    .collect::<Vec<_>>()
                    .join(" "),
                format!("{:?}", config.side).to_lowercase()
            ),
        );
        Some(Arc::new(SwitchController::new(
            displays,
            config.side,
            remote.width,
            remote.height,
            forward_keys,
            SessionActions {
                client: Arc::clone(&client),
                capture: Arc::clone(&capture_slot),
                shared: Arc::clone(shared),
                events: events.clone(),
                peer: hello.name.clone(),
                debug: config.debug,
                sent: AtomicUsize::new(0),
            },
        )))
    } else {
        None
    };

    let sink = Arc::new(Sink {
        controller: controller.clone(),
        client: Arc::clone(&client),
        forward_keys,
        last_position: Mutex::new(None),
        on_panic: {
            let shared = Arc::clone(shared);
            let events = events.clone();
            Box::new(move || {
                let shared = Arc::clone(&shared);
                let events = events.clone();
                // Stopping joins the capture thread this runs on: do it elsewhere.
                let _ = thread::Builder::new()
                    .name("edgehop-controller-panic".into())
                    .spawn(move || {
                        log(&events, Source::Controller, "panic chord: stopping");
                        stop_session(&shared);
                    });
            })
        },
        diagnostics: config.debug.then(|| Diagnostics {
            events: events.clone(),
            capture: Arc::clone(&capture_slot),
            motions: AtomicUsize::new(0),
            slowest_micros: AtomicU64::new(0),
        }),
    });
    let capture = match platform.start_capture(sink) {
        Ok(capture) => capture,
        Err(error) => {
            client.close();
            return Err(format!("Cannot capture input: {error}"));
        }
    };
    let _ = capture_slot.set(Arc::clone(&capture));

    let (ping_stop, ping_wait) = mpsc::channel::<()>();
    let ping_client = Arc::clone(&client);
    let ping = thread::Builder::new()
        .name("edgehop-controller-ping".into())
        .spawn(move || {
            while let Err(mpsc::RecvTimeoutError::Timeout) = ping_wait.recv_timeout(PING_INTERVAL) {
                if ping_client.send_ping().is_err() {
                    break;
                }
            }
        })
        .ok();

    *shared.run.lock().unwrap_or_else(PoisonError::into_inner) = Some(Run {
        client,
        controller,
        capture,
        ping_stop,
        ping,
    });
    shared.set_state(
        events,
        ControllerState::Connected {
            peer: hello.name,
            remote_active: false,
        },
    );
    log(
        events,
        Source::Controller,
        if config.switching {
            format!(
                "switching on: push the cursor through the {:?} edge",
                config.side
            )
        } else {
            "mirror mode: both cursors move".to_string()
        },
    );
    // The link may have dropped while the session was being set up.
    if closed.load(Ordering::SeqCst) {
        LinkEvents {
            shared: Arc::clone(shared),
            generation,
            closed,
            events: events.clone(),
        }
        .finish_if_closed();
    }
    Ok(())
}
