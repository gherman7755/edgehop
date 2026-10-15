//! Differential test against the Swift core: `scenarios.json` holds scripted and pseudo-random scenarios that
//! the Swift `SwitchController` ran, together with everything it did (every action it asked for, every
//! swallow decision, the mode after each step). The Rust port must produce exactly the same trace.
//! Regenerate with `scripts/golden-vectors.sh scenarios > rust/crates/edgehop-switch/tests/scenarios.json`.

use std::sync::{Arc, Mutex};

use edgehop_proto::{InputEvent, Modifiers, MouseButton};
use edgehop_switch::{Actions, Mode, Point, Rect, Side, SwitchController};
use serde_json::{json, Value};

const SCENARIOS: &str = include_str!("scenarios.json");

fn number(v: &Value, key: &str) -> f64 {
    v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("missing number {key} in {v}"))
}

fn event_from(v: &Value) -> InputEvent {
    let int = |key: &str| number(v, key) as i32;
    match v["kind"].as_str().unwrap() {
        "mouseMoveAbsolute" => InputEvent::MouseMoveAbsolute {
            x: int("x"),
            y: int("y"),
        },
        "mouseMoveDelta" => InputEvent::MouseMoveDelta {
            dx: int("dx"),
            dy: int("dy"),
        },
        "mouseButton" => InputEvent::MouseButton {
            button: match v["button"].as_str().unwrap() {
                "left" => MouseButton::Left,
                "right" => MouseButton::Right,
                "middle" => MouseButton::Middle,
                "back" => MouseButton::Back,
                "forward" => MouseButton::Forward,
                other => panic!("unknown button {other}"),
            },
            is_down: v["isDown"].as_bool().unwrap(),
        },
        "scroll" => InputEvent::Scroll {
            horizontal: int("horizontal"),
            vertical: int("vertical"),
        },
        "key" => InputEvent::Key {
            code: int("code") as u16,
            is_down: v["isDown"].as_bool().unwrap(),
            modifiers: Modifiers(int("modifiers") as u8),
        },
        other => panic!("unknown event kind {other}"),
    }
}

fn describe(event: &InputEvent) -> Value {
    match *event {
        InputEvent::MouseMoveAbsolute { x, y } => {
            json!({"kind": "mouseMoveAbsolute", "x": x, "y": y})
        }
        InputEvent::MouseMoveDelta { dx, dy } => {
            json!({"kind": "mouseMoveDelta", "dx": dx, "dy": dy})
        }
        InputEvent::MouseButton { button, is_down } => {
            let name = match button {
                MouseButton::Left => "left",
                MouseButton::Right => "right",
                MouseButton::Middle => "middle",
                MouseButton::Back => "back",
                MouseButton::Forward => "forward",
            };
            json!({"kind": "mouseButton", "button": name, "isDown": is_down})
        }
        InputEvent::Scroll {
            horizontal,
            vertical,
        } => json!({"kind": "scroll", "horizontal": horizontal, "vertical": vertical}),
        InputEvent::Key {
            code,
            is_down,
            modifiers,
        } => {
            json!({"kind": "key", "code": code, "isDown": is_down, "modifiers": modifiers.0})
        }
    }
}

/// Records what the controller asks the platform to do, in the same shape the Swift tool wrote.
#[derive(Clone, Default)]
struct Trace(Arc<Mutex<Vec<Value>>>);

impl Trace {
    fn push(&self, entry: Value) {
        self.0.lock().unwrap().push(entry);
    }
}

impl Actions for Trace {
    fn send(&self, event: InputEvent) {
        self.push(json!({"action": "send", "event": describe(&event)}));
    }
    fn enter_remote(&self) {
        self.push(json!({"action": "enter"}));
    }
    fn leave_remote(&self, to: Point) {
        self.push(json!({"action": "leave", "x": to.x, "y": to.y}));
    }
    fn release_remote(&self) {
        self.push(json!({"action": "release"}));
    }
}

/// JSON numbers from Swift may be integers where Rust writes floats: compare every number as an `f64`.
fn canonical(value: &Value) -> Value {
    match value {
        Value::Number(n) => json!(n.as_f64().unwrap()),
        Value::Array(items) => Value::Array(items.iter().map(canonical).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), canonical(v))).collect())
        }
        other => other.clone(),
    }
}

fn replay(scenario: &Value) -> Vec<Value> {
    let displays: Vec<Rect> = scenario["displays"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            Rect::new(
                number(d, "x"),
                number(d, "y"),
                number(d, "width"),
                number(d, "height"),
            )
        })
        .collect();
    let trace = Trace::default();
    let controller = SwitchController::new(
        displays,
        Side::parse(scenario["side"].as_str().unwrap()).unwrap(),
        number(scenario, "remoteWidth"),
        number(scenario, "remoteHeight"),
        scenario["forwardKeys"].as_bool().unwrap(),
        trace.clone(),
    );
    for (index, step) in scenario["steps"].as_array().unwrap().iter().enumerate() {
        trace.push(json!({"step": index}));
        match step["op"].as_str().unwrap() {
            "motion" => controller.handle_motion(
                Point::new(number(step, "x"), number(step, "y")),
                Point::new(number(step, "dx"), number(step, "dy")),
            ),
            "handle" => controller.handle(&event_from(&step["event"])),
            "swallow" => {
                let swallowed = controller.should_swallow(&event_from(&step["event"]));
                trace.push(json!({"swallow": swallowed}));
            }
            "return" => controller.return_to_local(),
            other => panic!("unknown step {other}"),
        }
        trace.push(
            json!({"mode": if controller.mode() == Mode::Remote { "remote" } else { "local" }}),
        );
    }
    let recorded = trace.0.lock().unwrap().clone();
    recorded
}

#[test]
fn the_rust_controller_behaves_exactly_like_the_swift_one() {
    let file: Value = serde_json::from_str(SCENARIOS).expect("scenarios.json is valid JSON");
    let scenarios = file["scenarios"].as_array().unwrap();
    assert!(
        scenarios.len() >= 20,
        "expected many scenarios, found {}",
        scenarios.len()
    );

    let mut steps = 0;
    for scenario in scenarios {
        let name = scenario["name"].as_str().unwrap();
        let expected: Vec<Value> = scenario["trace"]
            .as_array()
            .unwrap()
            .iter()
            .map(canonical)
            .collect();
        let actual: Vec<Value> = replay(scenario).iter().map(canonical).collect();
        if expected != actual {
            let at = expected
                .iter()
                .zip(&actual)
                .position(|(e, a)| e != a)
                .unwrap_or(expected.len().min(actual.len()));
            panic!(
                "scenario {name:?} diverges at trace entry {at}:\n  swift: {:?}\n  rust:  {:?}",
                expected.get(at),
                actual.get(at)
            );
        }
        steps += scenario["steps"].as_array().unwrap().len();
    }
    // Make sure the scenarios exercise real switching, not just idle motion.
    let trace_has = |action: &str| {
        scenarios.iter().any(|s| {
            s["trace"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["action"] == action)
        })
    };
    assert!(trace_has("enter") && trace_has("leave") && trace_has("release") && trace_has("send"));
    println!(
        "{} scenarios, {steps} steps: identical traces",
        scenarios.len()
    );
}
