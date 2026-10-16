//! Prints protocol vectors produced by the Rust implementation, in the schema of `golden.json`.
//! `scripts/check-rust-compat.sh` feeds them to the Swift core to prove Swift accepts what Rust writes.

use edgehop_proto::{
    encode_frame, ping_payload, ControlMessage, Frame, FrameType, HelloMessage, InputEvent,
    Modifiers, MonitorInfo, MouseButton,
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

    let challenge = HelloMessage::new("PC").with_keymap();
    let mut answer = HelloMessage::new("PC").with_keymap();
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
        "noise": [noise_vector()],
        "hello": hellos,
        "control": [{"json": String::from_utf8(ControlMessage::ReleaseAll.to_frame().payload).unwrap()}],
        "ping": [{"nowMillis": now, "payloadHex": hex(&ping_payload(now))}],
    });
    println!("{}", serde_json::to_string_pretty(&root).unwrap());
}

fn fixed(byte: u8) -> [u8; 32] {
    [byte; 32]
}

/// One complete handshake with fixed keys (never use these anywhere else), the controller's hello as the payload of
/// message 3, and a few transport messages in both directions.
fn noise_vector() -> Value {
    use edgehop_proto::{psk_from_token, Handshake, Role};
    let token = "correct-horse-battery";
    let psk = psk_from_token(token);
    let mut initiator =
        Handshake::with_fixed_keys_for_testing(Role::Initiator, &psk, &fixed(0x11), &fixed(0x22))
            .expect("initiator");
    let mut responder =
        Handshake::with_fixed_keys_for_testing(Role::Responder, &psk, &fixed(0x33), &fixed(0x44))
            .expect("responder");
    let payload = br#"{"version":2,"name":"PC","keymap":"hid"}"#;
    let m1 = initiator.write(&[]).unwrap();
    responder.read(&m1).unwrap();
    let m2 = responder.write(&[]).unwrap();
    initiator.read(&m2).unwrap();
    let m3 = initiator.write(payload).unwrap();
    assert_eq!(responder.read(&m3).unwrap(), payload);
    let (mut initiator_seal, _) = initiator.into_transport().unwrap();
    let (mut responder_seal, _) = responder.into_transport().unwrap();
    let transport = [
        ("initiator", b"first frame bytes".as_slice()),
        ("responder", b"answer".as_slice()),
        ("initiator", b"second".as_slice()),
    ]
    .iter()
    .map(|(from, plaintext)| {
        let wire = if *from == "initiator" {
            initiator_seal.seal(plaintext)
        } else {
            responder_seal.seal(plaintext)
        }
        .unwrap();
        json!({"from": from, "plaintextHex": hex(plaintext), "wireHex": hex(&wire)})
    })
    .collect::<Vec<_>>();
    json!({
        "token": token,
        "initiatorStaticHex": hex(&fixed(0x11)),
        "initiatorEphemeralHex": hex(&fixed(0x22)),
        "responderStaticHex": hex(&fixed(0x33)),
        "responderEphemeralHex": hex(&fixed(0x44)),
        "payloadHex": hex(payload),
        "messagesHex": [hex(&m1), hex(&m2), hex(&m3)],
        "transport": transport,
    })
}
