use serde::{Deserialize, Serialize};

use crate::frame::{Frame, FrameType};

/// JSON payload of `control` frames: `{"t": "<message>"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControlMessage {
    /// Release every key and button the sender pressed here (sent when control returns to the sender).
    #[serde(rename = "release_all")]
    ReleaseAll,
}

#[derive(Serialize, Deserialize)]
struct ControlFrame {
    t: ControlMessage,
}

impl ControlMessage {
    pub fn to_frame(self) -> Frame {
        Frame::new(
            FrameType::Control,
            serde_json::to_vec(&ControlFrame { t: self }).expect("control message serialises"),
        )
    }

    /// `None` for other frame types, invalid JSON or an unknown message (peers may send newer messages).
    pub fn from_frame(frame: &Frame) -> Option<Self> {
        if frame.frame_type != FrameType::Control {
            return None;
        }
        serde_json::from_slice::<ControlFrame>(&frame.payload)
            .ok()
            .map(|c| c.t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_all_round_trips_with_the_documented_json() {
        let frame = ControlMessage::ReleaseAll.to_frame();
        assert_eq!(frame.payload, br#"{"t":"release_all"}"#);
        assert_eq!(
            ControlMessage::from_frame(&frame),
            Some(ControlMessage::ReleaseAll)
        );
    }

    #[test]
    fn ignores_unknown_messages_and_other_frame_types() {
        assert_eq!(
            ControlMessage::from_frame(&Frame::new(
                FrameType::Control,
                br#"{"t":"from_the_future"}"#.to_vec()
            )),
            None
        );
        assert_eq!(
            ControlMessage::from_frame(&Frame::new(
                FrameType::Input,
                br#"{"t":"release_all"}"#.to_vec()
            )),
            None
        );
    }
}
