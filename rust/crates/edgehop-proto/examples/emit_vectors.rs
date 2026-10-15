//! Prints protocol vectors produced by the Rust implementation, in the schema of `golden.json`.
//! `scripts/check-rust-compat.sh` feeds them to the Swift core to prove Swift accepts what Rust writes.

use edgehop_proto::{
    encode_frame, make_nonce, ping_payload, proof, ControlMessage, Frame, FrameType, HelloMessage,
    InputEvent, Modifiers, MonitorInfo, MouseButton,
};
use serde_json::{json, Value};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
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

fn main() {
    let events = [
        InputEvent::MouseMoveAbsolute { x: 1919, y: -3 },
        InputEvent::MouseMoveDelta { dx: -7, dy: 12 },
        InputEvent::MouseButton {
            button: MouseButton::Middle,
            is_down: true,
        },
        InputEvent::MouseButton {
            button: MouseButton::Back,
            is_down: false,
        },
        InputEvent::Scroll {
            horizontal: 0,
            vertical: -240,
        },
        InputEvent::Key {
            code: 0,
            is_down: false,
            modifiers: Modifiers::CONTROL | Modifiers::OPTION,
        },
        InputEvent::MouseMoveAbsolute {
            x: i32::MIN,
            y: i32::MAX,
        },
    ];
    let frames = [
        Frame::new(
            FrameType::Input,
            InputEvent::Scroll {
                horizontal: 1,
                vertical: 2,
            }
            .encode()
            .to_vec(),
        ),
        Frame::new(FrameType::Pong, 42u64.to_be_bytes().to_vec()),
        Frame {
            frame_type: FrameType::Layout,
            flags: 0x7f,
            payload: b"{}".to_vec(),
        },
    ];

    // One fixed nonce and one freshly generated, so Swift verifies proofs it has never seen.
    let fresh = make_nonce().expect("operating system random generator");
    let auth: Vec<Value> = [[7u8; 32], fresh]
        .iter()
        .map(|nonce| json!({"token": "correct-horse-battery", "nonceHex": hex(nonce), "proofHex": hex(&proof("correct-horse-battery", nonce))}))
        .collect();

    let mut challenge = HelloMessage::new("PC");
    challenge.nonce = Some("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".into());
    let mut answer = HelloMessage::new("PC");
    answer.proof = Some("AAAA".into());
    answer.monitors = Some(vec![MonitorInfo {
        id: "w1".into(),
        x: 0.0,
        y: 0.0,
        width: 2560.0,
        height: 1440.5,
    }]);
    let mut accepted = HelloMessage::new("PC");
    accepted.accepted = Some(true);
    let hellos: Vec<Value> = [challenge, answer, accepted]
        .iter()
        .map(|h| json!({"json": String::from_utf8(h.to_frame().payload).unwrap()}))
        .collect();

    let now: u64 = 1_234_567_890_123;
    let root = json!({
        "events": events.iter().map(|e| json!({"event": describe(e), "hex": hex(&e.encode())})).collect::<Vec<_>>(),
        "frames": frames.iter().map(|f| json!({
            "type": f.frame_type as u8, "flags": f.flags, "payloadHex": hex(&f.payload), "hex": hex(&encode_frame(f).unwrap()),
        })).collect::<Vec<_>>(),
        "auth": auth,
        "hello": hellos,
        "control": [{"json": String::from_utf8(ControlMessage::ReleaseAll.to_frame().payload).unwrap()}],
        "ping": [{"nowMillis": now, "payloadHex": hex(&ping_payload(now))}],
    });
    println!("{}", serde_json::to_string_pretty(&root).unwrap());
}
