use serde::{Deserialize, Serialize};

use crate::frame::{Frame, FrameType};
use crate::PROTOCOL_VERSION;

/// A display as advertised to the other computer: global points, the primary display at (0, 0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub id: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// JSON payload of `hello` frames, used for the whole handshake:
/// server to client `{nonce}`; client to server `{proof, name, monitors}`; server to client `{accepted}`.
/// `nonce` and `proof` are base64.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloMessage {
    pub version: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorInfo>>,
    /// `"hid"` if this computer sends and understands HID key codes (`keymap.rs`). Absent: macOS keycodes, so
    /// no keys can be exchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keymap: Option<String>,
}

impl HelloMessage {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            name: name.into(),
            nonce: None,
            proof: None,
            accepted: None,
            monitors: None,
            keymap: None,
        }
    }

    /// Announces HID key codes.
    pub fn with_keymap(mut self) -> Self {
        self.keymap = Some(crate::KEYMAP_NAME.to_string());
        self
    }

    /// True if the sender announced HID key codes.
    pub fn speaks_hid_keys(&self) -> bool {
        self.keymap.as_deref() == Some(crate::KEYMAP_NAME)
    }

    pub fn to_frame(&self) -> Frame {
        // Serialising plain strings, numbers and options cannot fail.
        Frame::new(
            FrameType::Hello,
            serde_json::to_vec(self).expect("hello message serialises"),
        )
    }

    /// `None` if the frame is not a hello or its payload is not valid hello JSON.
    pub fn from_frame(frame: &Frame) -> Option<Self> {
        if frame.frame_type != FrameType::Hello {
            return None;
        }
        serde_json::from_slice(&frame.payload).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_a_frame() {
        let mut hello = HelloMessage::new("PC");
        hello.proof = Some("abc".into());
        hello.monitors = Some(vec![MonitorInfo {
            id: "d1".into(),
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
        }]);
        assert_eq!(HelloMessage::from_frame(&hello.to_frame()), Some(hello));
    }

    #[test]
    fn omits_absent_fields() {
        let json = String::from_utf8(HelloMessage::new("PC").to_frame().payload).unwrap();
        assert_eq!(json, r#"{"version":1,"name":"PC"}"#);
    }

    #[test]
    fn rejects_other_frame_types_and_garbage() {
        assert_eq!(
            HelloMessage::from_frame(&Frame::new(
                FrameType::Input,
                br#"{"version":1,"name":"x"}"#.to_vec()
            )),
            None
        );
        assert_eq!(
            HelloMessage::from_frame(&Frame::new(FrameType::Hello, b"not json".to_vec())),
            None
        );
        assert_eq!(
            HelloMessage::from_frame(&Frame::new(
                FrameType::Hello,
                br#"{"name":"missing version"}"#.to_vec()
            )),
            None
        );
    }
}
