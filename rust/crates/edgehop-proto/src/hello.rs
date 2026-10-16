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

/// JSON payload of the hellos: the controller's, carried as the payload of the last handshake message, and the
/// receiver's answer, the first encrypted frame (`accepted`, with its name and monitors).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HelloMessage {
    pub version: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitors: Option<Vec<MonitorInfo>>,
    /// `"hid"` if this computer sends and understands HID key codes (`keymap.rs`). Absent: macOS keycodes, so
    /// no keys can be exchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keymap: Option<String>,
    /// `true` (in a receiver's hello) if that computer cannot apply input right now, for example because a macOS
    /// permission is missing. Absent: it can.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        rename = "inputBlocked"
    )]
    pub input_blocked: Option<bool>,
}

impl HelloMessage {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            name: name.into(),
            accepted: None,
            monitors: None,
            keymap: None,
            input_blocked: None,
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

    /// True if the sender announced that it cannot apply input.
    pub fn is_input_blocked(&self) -> bool {
        self.input_blocked == Some(true)
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
        hello.accepted = Some(true);
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
        assert_eq!(json, r#"{"version":2,"name":"PC"}"#);
    }

    #[test]
    fn reads_the_input_blocked_announcement_of_the_swift_core() {
        let frame = Frame::new(
            FrameType::Hello,
            br#"{"version":2,"name":"Mac","accepted":true,"inputBlocked":true}"#.to_vec(),
        );
        assert!(HelloMessage::from_frame(&frame).unwrap().is_input_blocked());
        let plain = HelloMessage::new("PC");
        assert!(!plain.is_input_blocked());
        let mut blocked = plain;
        blocked.input_blocked = Some(true);
        let json = String::from_utf8(blocked.to_frame().payload).unwrap();
        assert_eq!(json, r#"{"version":2,"name":"PC","inputBlocked":true}"#);
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
