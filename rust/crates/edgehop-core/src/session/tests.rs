//! Sessions end to end over real TCP on loopback, with a fake platform standing in for the operating system on
//! both computers.

use std::io::Write;
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use edgehop_input::{
    CaptureHandle, CaptureSink, OsInput, Platform, PlatformError, Rect as InputRect,
};
use edgehop_proto::{InputEvent, Modifiers, MonitorInfo, MouseButton};
use serde_json::Value;

use super::SessionBackend;
use crate::backend::{
    Backend, BackendError, ControllerConfig, ControllerState, Events, ReceiverConfig,
    ReceiverState, Side, Target,
};
use crate::ipc::Emitter;

const TOKEN: &str = "secret-token-123456";

#[derive(Debug, Clone, PartialEq)]
enum Op {
    Warp(f64, f64),
    Move { x: f64, y: f64 },
    Button { button: MouseButton, down: bool },
    Scroll(i32, i32),
    Key { code: u16, down: bool },
}

#[derive(Clone, Default)]
struct Ops(Arc<Mutex<Vec<Op>>>);

impl Ops {
    fn all(&self) -> Vec<Op> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn push(&self, op: Op) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(op);
    }
}

struct FakeOs {
    ops: Ops,
    position: (f64, f64),
    bounds: InputRect,
}

impl OsInput for FakeOs {
    fn cursor_position(&mut self) -> Option<(f64, f64)> {
        Some(self.position)
    }

    fn desktop_bounds(&mut self) -> InputRect {
        self.bounds
    }

    fn warp_cursor(&mut self, x: f64, y: f64) {
        self.position = (x, y);
        self.ops.push(Op::Warp(x, y));
    }

    fn move_cursor(&mut self, x: f64, y: f64, _dx: f64, _dy: f64) {
        self.position = (x, y);
        self.ops.push(Op::Move { x, y });
    }

    fn button(&mut self, button: MouseButton, is_down: bool, _x: f64, _y: f64) {
        self.ops.push(Op::Button {
            button,
            down: is_down,
        });
    }

    fn scroll(&mut self, horizontal: i32, vertical: i32) {
        self.ops.push(Op::Scroll(horizontal, vertical));
    }

    fn key(&mut self, code: u16, is_down: bool, _modifiers: Modifiers) {
        self.ops.push(Op::Key {
            code,
            down: is_down,
        });
    }
}

struct FakeCapture {
    sink: Arc<dyn CaptureSink>,
    calls: Mutex<Vec<String>>,
    stopped: AtomicBool,
}

impl FakeCapture {
    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl CaptureHandle for FakeCapture {
    fn enter_remote(&self) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("enter".into());
    }

    fn leave_remote(&self, to: (f64, f64)) {
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(format!("leave {} {}", to.0, to.1));
    }

    fn cursor_position(&self) -> Option<(f64, f64)> {
        Some((400.0, 300.0))
    }

    fn stop(&self) {
        self.stopped.store(true, Ordering::SeqCst);
        self.calls
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push("stop".into());
    }
}

struct FakePlatform {
    name: &'static str,
    width: f64,
    height: f64,
    ops: Ops,
    capture: Arc<Mutex<Option<Arc<FakeCapture>>>>,
    fail_capture: bool,
}

impl FakePlatform {
    fn new(name: &'static str, width: f64, height: f64) -> Self {
        Self {
            name,
            width,
            height,
            ops: Ops::default(),
            capture: Arc::default(),
            fail_capture: false,
        }
    }
}

impl Platform for FakePlatform {
    type Output = FakeOs;

    fn os_name(&self) -> &'static str {
        "fake"
    }

    fn device_name(&self) -> String {
        self.name.into()
    }

    fn input_permitted(&self) -> bool {
        true
    }

    fn displays(&self) -> Vec<MonitorInfo> {
        vec![MonitorInfo {
            id: "m0".into(),
            x: 0.0,
            y: 0.0,
            width: self.width,
            height: self.height,
        }]
    }

    fn output(&self) -> FakeOs {
        FakeOs {
            ops: self.ops.clone(),
            position: (0.0, 0.0),
            bounds: InputRect::new(0.0, 0.0, self.width, self.height),
        }
    }

    fn start_capture(
        &self,
        sink: Arc<dyn CaptureSink>,
    ) -> Result<Arc<dyn CaptureHandle>, PlatformError> {
        if self.fail_capture {
            return Err(PlatformError("the mouse hook was refused".into()));
        }
        let capture = Arc::new(FakeCapture {
            sink,
            calls: Mutex::new(Vec::new()),
            stopped: AtomicBool::new(false),
        });
        *self.capture.lock().unwrap_or_else(PoisonError::into_inner) = Some(Arc::clone(&capture));
        Ok(capture)
    }
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Buffer {
    /// The `data` of every event with this name, in order.
    fn events(&self, name: &str) -> Vec<Value> {
        let text = String::from_utf8_lossy(&self.0.lock().unwrap_or_else(PoisonError::into_inner))
            .to_string();
        text.lines()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|value| value["event"] == name)
            .map(|value| value["data"].clone())
            .collect()
    }

    fn states(&self, name: &str) -> Vec<String> {
        self.events(name)
            .iter()
            .map(|data| {
                let mut text = data["state"].as_str().unwrap_or("?").to_string();
                if let Some(active) = data["remoteActive"].as_bool() {
                    text.push_str(if active { ":remote" } else { ":local" });
                }
                text
            })
            .collect()
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A receiver and a controller computer, each with its own backend, events and fake operating system.
struct Rig {
    receiver: SessionBackend<FakePlatform>,
    receiver_ops: Ops,
    receiver_events: (Events, Buffer),
    controller: SessionBackend<FakePlatform>,
    controller_capture: Arc<Mutex<Option<Arc<FakeCapture>>>>,
    controller_events: (Events, Buffer),
    port: u16,
}

fn events() -> (Events, Buffer) {
    let buffer = Buffer::default();
    (Events::new(Emitter::new(buffer.clone())), buffer)
}

impl Rig {
    fn new(allow_keys: bool) -> Self {
        Self::with(allow_keys, false)
    }

    fn with(allow_keys: bool, fail_capture: bool) -> Self {
        let receiver_platform = FakePlatform::new("receiver-pc", 800.0, 600.0);
        let receiver_ops = receiver_platform.ops.clone();
        let mut controller_platform = FakePlatform::new("controller-pc", 1000.0, 800.0);
        controller_platform.fail_capture = fail_capture;
        let controller_capture = Arc::clone(&controller_platform.capture);
        let receiver = SessionBackend::new(receiver_platform);
        let receiver_events = events();
        let port = receiver
            .start_receiver(
                ReceiverConfig {
                    token: TOKEN.into(),
                    allow_keys,
                    debug: false,
                    port: 0,
                },
                receiver_events.0.clone(),
            )
            .expect("receiver starts");
        Self {
            receiver,
            receiver_ops,
            receiver_events,
            controller: SessionBackend::new(controller_platform),
            controller_capture,
            controller_events: events(),
            port,
        }
    }

    fn config(&self, token: &str, switching: bool, forward_keys: bool) -> ControllerConfig {
        ControllerConfig {
            target: Target::Host {
                host: "127.0.0.1".into(),
                port: self.port,
            },
            token: token.into(),
            side: Side::Right,
            switching,
            forward_keys,
            debug: false,
        }
    }

    fn start(&self, switching: bool, forward_keys: bool) -> Arc<FakeCapture> {
        self.controller
            .start_controller(
                self.config(TOKEN, switching, forward_keys),
                self.controller_events.0.clone(),
            )
            .expect("controller starts");
        wait_until("the controller to connect", || {
            matches!(
                self.controller.controller_state(),
                ControllerState::Connected { .. }
            )
        });
        let capture = self
            .controller_capture
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .expect("capture started");
        capture
    }

    fn wait_for_receiver_op(&self, what: &str, matches: impl Fn(&Op) -> bool) {
        wait_until(what, || self.receiver_ops.all().iter().any(&matches));
    }
}

fn right_edge_push(capture: &FakeCapture) {
    capture.sink.motion((999.0, 400.0), (5.0, 0.0));
}

#[test]
fn the_receiver_reports_each_stage_of_a_connection() {
    let rig = Rig::new(false);
    assert_eq!(
        rig.receiver.receiver_state(),
        ReceiverState::Listening { port: rig.port }
    );
    rig.start(true, false);
    wait_until("the receiver to see the peer", || {
        rig.receiver.receiver_state()
            == ReceiverState::Connected {
                peer: "controller-pc".into(),
            }
    });
    rig.controller.stop_controller();
    wait_until("the receiver to listen again", || {
        rig.receiver.receiver_state() == ReceiverState::Listening { port: rig.port }
    });
    rig.receiver.stop_receiver();
    assert_eq!(
        rig.receiver_events.1.states("receiver.state"),
        ["listening", "connected", "listening", "stopped"]
    );
    assert_eq!(rig.receiver.receiver_state(), ReceiverState::Stopped);
}

#[test]
fn the_controller_reports_connecting_connected_and_idle() {
    let rig = Rig::new(false);
    rig.start(true, false);
    rig.controller.stop_controller();
    assert_eq!(rig.controller.controller_state(), ControllerState::Idle);
    assert_eq!(
        rig.controller_events.1.states("controller.state"),
        ["connecting", "connected:local", "idle"]
    );
}

#[test]
fn pushing_through_the_edge_moves_the_remote_cursor() {
    let rig = Rig::new(false);
    let capture = rig.start(true, false);

    right_edge_push(&capture);
    assert_eq!(capture.calls(), ["enter"]);
    // The remote cursor appears at the same height on the other computer's left edge.
    rig.wait_for_receiver_op(
        "the entry move",
        |op| matches!(op, Op::Warp(x, y) | Op::Move { x, y } if (*x, *y) == (0.0, 400.0)),
    );

    // From now on device movement drives the remote cursor, and applications here do not see it.
    capture.sink.motion((520.0, 300.0), (10.0, 5.0));
    rig.wait_for_receiver_op(
        "the relative move",
        |op| matches!(op, Op::Move { x, y } if (*x, *y) == (10.0, 405.0)),
    );
    let move_event = InputEvent::MouseMoveAbsolute { x: 520, y: 300 };
    assert!(capture.sink.should_swallow(&move_event));

    capture.sink.event(InputEvent::MouseButton {
        button: MouseButton::Left,
        is_down: true,
    });
    capture.sink.event(InputEvent::Scroll {
        horizontal: 0,
        vertical: 120,
    });
    rig.wait_for_receiver_op("the click", |op| {
        matches!(
            op,
            Op::Button {
                button: MouseButton::Left,
                down: true
            }
        )
    });
    rig.wait_for_receiver_op("the scroll", |op| *op == Op::Scroll(0, 120));

    assert_eq!(
        rig.controller.controller_state(),
        ControllerState::Connected {
            peer: "receiver-pc".into(),
            remote_active: true
        }
    );
}

#[test]
fn pushing_back_through_the_facing_edge_returns_control() {
    let rig = Rig::new(false);
    let capture = rig.start(true, false);
    right_edge_push(&capture);
    capture.sink.motion((520.0, 300.0), (10.0, 0.0));
    capture.sink.motion((510.0, 300.0), (-50.0, 0.0));

    let calls = capture.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(calls[1].starts_with("leave 999"), "{calls:?}");
    assert_eq!(
        rig.controller_events.1.states("controller.state"),
        [
            "connecting",
            "connected:local",
            "connected:remote",
            "connected:local"
        ]
    );
    // The first move after returning is the one that predates the warp and is swallowed.
    let local_move = InputEvent::MouseMoveAbsolute { x: 999, y: 400 };
    assert!(capture.sink.should_swallow(&local_move));
    assert!(!capture.sink.should_swallow(&local_move));
}

#[test]
fn stopping_while_remote_gives_the_cursor_back_and_releases_the_buttons() {
    let rig = Rig::new(false);
    let capture = rig.start(true, false);
    right_edge_push(&capture);
    capture.sink.event(InputEvent::MouseButton {
        button: MouseButton::Left,
        is_down: true,
    });
    rig.wait_for_receiver_op("the click", |op| {
        matches!(op, Op::Button { down: true, .. })
    });

    rig.controller.stop_controller();
    let calls = capture.calls();
    assert!(calls.iter().any(|c| c.starts_with("leave")), "{calls:?}");
    assert_eq!(calls.last().map(String::as_str), Some("stop"));
    rig.wait_for_receiver_op("the release", |op| {
        matches!(op, Op::Button { down: false, .. })
    });
}

#[test]
fn mirror_mode_follows_the_local_movement_and_keeps_keys_back() {
    let rig = Rig::new(true);
    let capture = rig.start(false, false);

    capture.sink.motion((100.0, 100.0), (0.0, 0.0));
    capture
        .sink
        .event(InputEvent::MouseMoveAbsolute { x: 100, y: 100 });
    capture.sink.motion((110.0, 105.0), (10.0, 5.0));
    capture
        .sink
        .event(InputEvent::MouseMoveAbsolute { x: 110, y: 105 });
    capture.sink.event(InputEvent::MouseButton {
        button: MouseButton::Right,
        is_down: true,
    });
    capture.sink.event(InputEvent::Key {
        code: 0,
        is_down: true,
        modifiers: Modifiers::NONE,
    });
    rig.wait_for_receiver_op("the click", |op| {
        matches!(
            op,
            Op::Button {
                button: MouseButton::Right,
                down: true
            }
        )
    });
    let ops = rig.receiver_ops.all();
    assert!(
        ops.iter()
            .any(|op| matches!(op, Op::Move { .. } | Op::Warp(..))),
        "{ops:?}"
    );
    assert!(
        !ops.iter().any(|op| matches!(op, Op::Key { .. })),
        "keys are not forwarded unless asked: {ops:?}"
    );
    assert!(!capture
        .sink
        .should_swallow(&InputEvent::MouseMoveAbsolute { x: 1, y: 1 }));
}

#[test]
fn keys_cross_only_when_both_sides_agree() {
    for allow_keys in [false, true] {
        let rig = Rig::new(allow_keys);
        let capture = rig.start(true, true);
        right_edge_push(&capture);
        capture.sink.event(InputEvent::Key {
            code: 12,
            is_down: true,
            modifiers: Modifiers::NONE,
        });
        // A mouse event sent after the key is applied after it, so once it arrives the key was decided.
        capture.sink.event(InputEvent::Scroll {
            horizontal: 0,
            vertical: 1,
        });
        rig.wait_for_receiver_op("the scroll", |op| *op == Op::Scroll(0, 1));
        let delivered = rig.receiver_ops.all().iter().any(|op| {
            *op == Op::Key {
                code: 12,
                down: true,
            }
        });
        assert_eq!(delivered, allow_keys, "allow_keys = {allow_keys}");
    }
}

#[test]
fn a_wrong_token_fails_with_a_readable_message() {
    let rig = Rig::new(false);
    rig.controller
        .start_controller(
            rig.config("another-token-99999", true, false),
            rig.controller_events.0.clone(),
        )
        .expect("starting returns at once");
    wait_until("the failure", || {
        matches!(
            rig.controller.controller_state(),
            ControllerState::Failed { .. }
        )
    });
    assert_eq!(
        rig.controller.controller_state(),
        ControllerState::Failed {
            message: "The other computer rejected the token. Check that both use the same one."
                .into()
        }
    );
    assert!(
        rig.controller_capture.lock().unwrap().is_none(),
        "nothing was captured"
    );
}

#[test]
fn a_capture_failure_ends_the_session_and_closes_the_link() {
    let rig = Rig::with(false, true);
    rig.controller
        .start_controller(
            rig.config(TOKEN, true, false),
            rig.controller_events.0.clone(),
        )
        .expect("starting returns at once");
    wait_until("the failure", || {
        matches!(
            rig.controller.controller_state(),
            ControllerState::Failed { .. }
        )
    });
    match rig.controller.controller_state() {
        ControllerState::Failed { message } => {
            assert!(message.contains("Cannot capture input"), "{message}");
            assert!(message.contains("the mouse hook was refused"), "{message}");
        }
        other => panic!("{other:?}"),
    }
    wait_until("the receiver to listen again", || {
        rig.receiver.receiver_state() == ReceiverState::Listening { port: rig.port }
    });
}

#[test]
fn the_receiver_stopping_ends_the_controller_session() {
    let rig = Rig::new(false);
    let capture = rig.start(true, false);
    right_edge_push(&capture);
    rig.receiver.stop_receiver();
    wait_until("the controller to notice", || {
        rig.controller.controller_state() == ControllerState::Idle
    });
    assert!(capture.stopped.load(Ordering::SeqCst));
    let calls = capture.calls();
    assert!(
        calls.iter().any(|c| c.starts_with("leave")),
        "control came back: {calls:?}"
    );
}

#[test]
fn a_second_session_is_refused_and_the_first_keeps_running() {
    let rig = Rig::new(false);
    rig.start(true, false);
    let again = rig.controller.start_controller(
        rig.config(TOKEN, true, false),
        rig.controller_events.0.clone(),
    );
    assert!(matches!(again, Err(BackendError::Failed(_))), "{again:?}");
    assert!(matches!(
        rig.controller.controller_state(),
        ControllerState::Connected { .. }
    ));
    let receiver_again = rig.receiver.start_receiver(
        ReceiverConfig {
            token: TOKEN.into(),
            allow_keys: false,
            debug: false,
            port: 0,
        },
        rig.receiver_events.0.clone(),
    );
    assert!(matches!(receiver_again, Err(BackendError::Failed(_))));
}

#[test]
fn computers_can_only_be_reached_by_address_for_now() {
    let backend = SessionBackend::new(FakePlatform::new("x", 100.0, 100.0));
    let result = backend.start_controller(
        ControllerConfig {
            target: Target::Peer("Mac".into()),
            token: TOKEN.into(),
            side: Side::Right,
            switching: true,
            forward_keys: false,
            debug: false,
        },
        events().0,
    );
    assert!(
        matches!(result, Err(BackendError::Unsupported(_))),
        "{result:?}"
    );
    assert_eq!(backend.controller_state(), ControllerState::Idle);
}

#[test]
fn a_taken_port_is_reported() {
    let taken = TcpListener::bind("0.0.0.0:0").unwrap();
    let port = taken.local_addr().unwrap().port();
    let backend = SessionBackend::new(FakePlatform::new("x", 100.0, 100.0));
    let result = backend.start_receiver(
        ReceiverConfig {
            token: TOKEN.into(),
            allow_keys: false,
            debug: false,
            port,
        },
        events().0,
    );
    match result {
        Err(BackendError::Failed(message)) => {
            assert!(
                message.contains(&format!("cannot listen on port {port}")),
                "{message}"
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(backend.receiver_state(), ReceiverState::Stopped);
}

#[test]
fn shutdown_stops_everything_and_is_repeatable() {
    let rig = Rig::new(false);
    let capture = rig.start(true, false);
    rig.controller.shutdown();
    rig.receiver.shutdown();
    rig.receiver.shutdown();
    assert!(capture.stopped.load(Ordering::SeqCst));
    assert_eq!(rig.controller.controller_state(), ControllerState::Idle);
    assert_eq!(rig.receiver.receiver_state(), ReceiverState::Stopped);
}

#[test]
fn stopping_the_receiver_while_connected_ends_in_stopped() {
    let rig = Rig::new(false);
    rig.start(true, false);
    wait_until("the receiver to see the peer", || {
        matches!(
            rig.receiver.receiver_state(),
            ReceiverState::Connected { .. }
        )
    });
    rig.receiver.stop_receiver();
    wait_until("the controller to notice", || {
        rig.controller.controller_state() == ControllerState::Idle
    });
    // The peer thread notices the closed link a moment later: that must not announce "listening" again.
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rig.receiver.receiver_state(), ReceiverState::Stopped);
    assert_eq!(
        rig.receiver_events.1.states("receiver.state"),
        ["listening", "connected", "stopped"]
    );
}

#[test]
fn a_dropped_link_releases_what_the_controller_left_pressed() {
    use edgehop_link::{ClientConfig, ClientHandler, LinkClient};
    struct Quiet;
    impl ClientHandler for Quiet {}

    let rig = Rig::new(false);
    let (client, _) = LinkClient::connect(
        "127.0.0.1",
        rig.port,
        &ClientConfig::new(TOKEN, "raw"),
        Arc::new(Quiet),
    )
    .expect("connects");
    client
        .send_input(&InputEvent::MouseButton {
            button: MouseButton::Left,
            is_down: true,
        })
        .unwrap();
    rig.wait_for_receiver_op("the press", |op| {
        matches!(op, Op::Button { down: true, .. })
    });
    drop(client); // gone without saying release_all
    rig.wait_for_receiver_op("the release", |op| {
        matches!(op, Op::Button { down: false, .. })
    });
}

#[test]
fn stopping_while_connecting_is_final() {
    // A computer that accepts the connection and never speaks.
    let silent = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = silent.local_addr().unwrap().port();
    let backend = SessionBackend::new(FakePlatform::new("controller-pc", 1000.0, 800.0));
    let (events, buffer) = events();
    backend
        .start_controller(
            ControllerConfig {
                target: Target::Host {
                    host: "127.0.0.1".into(),
                    port,
                },
                token: TOKEN.into(),
                side: Side::Right,
                switching: true,
                forward_keys: false,
                debug: false,
            },
            events,
        )
        .unwrap();
    let (held, _) = silent.accept().unwrap();
    assert_eq!(backend.controller_state(), ControllerState::Connecting);
    backend.stop_controller();
    assert_eq!(backend.controller_state(), ControllerState::Idle);
    // Now the attempt dies with an error; nobody is waiting for it any more.
    drop(held);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(backend.controller_state(), ControllerState::Idle);
    assert_eq!(buffer.states("controller.state"), ["connecting", "idle"]);
}

#[test]
fn debug_logs_show_where_the_local_cursor_really_is_while_remote() {
    let rig = Rig::new(false);
    let mut config = rig.config(TOKEN, true, false);
    config.debug = true;
    rig.controller
        .start_controller(config, rig.controller_events.0.clone())
        .unwrap();
    wait_until("the controller to connect", || {
        matches!(
            rig.controller.controller_state(),
            ControllerState::Connected { .. }
        )
    });
    let capture = rig
        .controller_capture
        .lock()
        .unwrap()
        .clone()
        .expect("capture started");
    capture.sink.event(InputEvent::MouseButton {
        button: MouseButton::Right,
        is_down: true,
    });
    capture.sink.event(InputEvent::MouseButton {
        button: MouseButton::Right,
        is_down: false,
    });
    right_edge_push(&capture);
    capture.sink.motion((410.0, 300.0), (10.0, 0.0));

    let lines: Vec<String> = rig
        .controller_events
        .1
        .events("log")
        .iter()
        .filter_map(|data| data["line"].as_str().map(str::to_string))
        .collect();
    assert!(
        lines
            .iter()
            .any(|l| l.contains("-> REMOTE (receiver-pc); local cursor now at (400, 300)")),
        "{lines:?}"
    );
    for expected in [
        "button Right down while Local",
        "button Right up while Local",
    ] {
        assert!(lines.iter().any(|l| l == expected), "{lines:?}");
    }
    assert!(
        lines.iter().any(|l| l.starts_with(
            "motion #2 pos=(410, 300) delta=(10, 0) local cursor=(400, 300) slowest="
        )),
        "{lines:?}"
    );
}
