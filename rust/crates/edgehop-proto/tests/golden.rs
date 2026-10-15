//! Byte-compatibility with the Swift core: `golden.json` is produced by the Swift implementation
//! (`scripts/golden-vectors.sh gen`). Every vector must encode and decode identically here.

use edgehop_proto::{
    encode_frame, ping_payload, proof, verify, ControlMessage, Frame, FrameDecoder, FrameType,
    HelloMessage, InputEvent, Modifiers, MonitorInfo, MouseButton,
};
use serde_json::Value;

const GOLDEN: &str = include_str!("golden.json");

fn golden() -> Value {
    serde_json::from_str(GOLDEN).expect("golden.json is valid JSON")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Vec<u8> {
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}

fn int(v: &Value, key: &str) -> i64 {
    v[key]
        .as_i64()
        .unwrap_or_else(|| panic!("missing integer {key} in {v}"))
}

fn event_from(v: &Value) -> InputEvent {
    match v["kind"].as_str().unwrap() {
        "mouseMoveAbsolute" => InputEvent::MouseMoveAbsolute {
            x: int(v, "x") as i32,
            y: int(v, "y") as i32,
        },
        "mouseMoveDelta" => InputEvent::MouseMoveDelta {
            dx: int(v, "dx") as i32,
            dy: int(v, "dy") as i32,
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
            horizontal: int(v, "horizontal") as i32,
            vertical: int(v, "vertical") as i32,
        },
        "key" => InputEvent::Key {
            code: int(v, "code") as u16,
            is_down: v["isDown"].as_bool().unwrap(),
            modifiers: Modifiers(int(v, "modifiers") as u8),
        },
        other => panic!("unknown event kind {other}"),
    }
}

#[test]
fn input_records_match_swift_byte_for_byte() {
    let golden = golden();
    let entries = golden["events"].as_array().unwrap();
    assert!(entries.len() >= 7);
    for entry in entries {
        let expected_hex = entry["hex"].as_str().unwrap();
        let event = event_from(&entry["event"]);
        assert_eq!(hex(&event.encode()), expected_hex, "encoding {event:?}");
        assert_eq!(
            InputEvent::decode(&unhex(expected_hex)),
            Ok(event),
            "decoding {expected_hex}"
        );
    }
}

#[test]
fn frames_match_swift_byte_for_byte() {
    let golden = golden();
    let entries = golden["frames"].as_array().unwrap();
    assert!(entries.len() >= 3);
    for entry in entries {
        let frame_hex = entry["hex"].as_str().unwrap();
        let payload = unhex(entry["payloadHex"].as_str().unwrap());
        let frame_type = match int(entry, "type") {
            1 => FrameType::Hello,
            2 => FrameType::Layout,
            3 => FrameType::Input,
            4 => FrameType::Control,
            5 => FrameType::Ping,
            6 => FrameType::Pong,
            other => panic!("unknown type {other}"),
        };
        let frame = Frame {
            frame_type,
            flags: int(entry, "flags") as u8,
            payload,
        };
        assert_eq!(
            hex(&encode_frame(&frame).unwrap()),
            frame_hex,
            "encoding {frame:?}"
        );
        assert_eq!(
            FrameDecoder::new().feed(&unhex(frame_hex)).unwrap(),
            vec![frame],
            "decoding {frame_hex}"
        );
    }
}

#[test]
fn handshake_proof_matches_swift() {
    let golden = golden();
    let entry = &golden["auth"][0];
    let token = entry["token"].as_str().unwrap();
    let nonce = unhex(entry["nonceHex"].as_str().unwrap());
    let expected = unhex(entry["proofHex"].as_str().unwrap());
    assert_eq!(proof(token, &nonce).to_vec(), expected);
    assert!(verify(token, &nonce, &expected));
}

fn hello_from(entry: &Value) -> HelloMessage {
    let frame = Frame::new(
        FrameType::Hello,
        entry["json"].as_str().unwrap().as_bytes().to_vec(),
    );
    HelloMessage::from_frame(&frame).expect("Swift hello JSON parses")
}

#[test]
fn hello_messages_written_by_swift_parse_here() {
    let golden = golden();
    let hellos: Vec<HelloMessage> = golden["hello"]
        .as_array()
        .unwrap()
        .iter()
        .map(hello_from)
        .collect();
    assert_eq!(hellos.len(), 4);

    // Challenge: the server sends only a nonce.
    assert_eq!(hellos[0].name, "Mac");
    assert_eq!(
        hellos[0].nonce.as_deref(),
        Some("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
    );
    assert!(
        hellos[0].proof.is_none() && hellos[0].accepted.is_none() && hellos[0].monitors.is_none()
    );

    // Client answer: proof and monitors.
    assert_eq!(hellos[1].name, "PC");
    assert!(hellos[1].proof.is_some());
    let monitors = hellos[1].monitors.as_ref().unwrap();
    assert_eq!(
        monitors[1],
        MonitorInfo {
            id: "d2".into(),
            x: -223.0,
            y: -1080.0,
            width: 1920.0,
            height: 1080.0
        }
    );

    // Accept / reject.
    assert_eq!(hellos[2].accepted, Some(true));
    assert_eq!(hellos[3].accepted, Some(false));
}

#[test]
fn the_swift_proof_inside_a_hello_verifies_here() {
    let golden = golden();
    let hello = hello_from(&golden["hello"][1]);
    let proof_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        hello.proof.unwrap(),
    )
    .unwrap();
    let auth = &golden["auth"][0];
    assert!(verify(
        auth["token"].as_str().unwrap(),
        &unhex(auth["nonceHex"].as_str().unwrap()),
        &proof_bytes
    ));
}

#[test]
fn control_and_ping_match_swift() {
    let golden = golden();
    let json = golden["control"][0]["json"].as_str().unwrap();
    assert_eq!(
        ControlMessage::from_frame(&Frame::new(FrameType::Control, json.as_bytes().to_vec())),
        Some(ControlMessage::ReleaseAll)
    );
    assert_eq!(
        ControlMessage::ReleaseAll.to_frame().payload,
        json.as_bytes()
    );

    let ping = &golden["ping"][0];
    let now = ping["nowMillis"].as_u64().unwrap();
    assert_eq!(
        hex(&ping_payload(now)),
        ping["payloadHex"].as_str().unwrap()
    );
}
