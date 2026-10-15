use thiserror::Error;

/// Frame types of the control/input stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameType {
    Hello = 1,
    Layout = 2,
    Input = 3,
    Control = 4,
    Ping = 5,
    Pong = 6,
}

impl FrameType {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Hello),
            2 => Some(Self::Layout),
            3 => Some(Self::Input),
            4 => Some(Self::Control),
            5 => Some(Self::Ping),
            6 => Some(Self::Pong),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub frame_type: FrameType,
    pub flags: u8,
    pub payload: Vec<u8>,
}

impl Frame {
    pub fn new(frame_type: FrameType, payload: Vec<u8>) -> Self {
        Self {
            frame_type,
            flags: 0,
            payload,
        }
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum FrameError {
    #[error("bad frame magic")]
    BadMagic,
    #[error("unknown frame type {0}")]
    UnknownType(u8),
    #[error("payload of {0} bytes exceeds the {MAX_PAYLOAD} byte limit")]
    PayloadTooLarge(usize),
}

/// Frame magic: ASCII "EH".
pub const MAGIC: [u8; 2] = [0x45, 0x48];
/// Magic (2), type (1), flags (1), big-endian length (4).
pub const HEADER_SIZE: usize = 8;
/// Largest accepted payload.
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// Encodes one frame. Fails if the payload is over `MAX_PAYLOAD`.
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    if frame.payload.len() > MAX_PAYLOAD {
        return Err(FrameError::PayloadTooLarge(frame.payload.len()));
    }
    let mut out = Vec::with_capacity(HEADER_SIZE + frame.payload.len());
    out.extend_from_slice(&MAGIC);
    out.push(frame.frame_type as u8);
    out.push(frame.flags);
    // Cannot truncate: bounded by MAX_PAYLOAD above.
    out.extend_from_slice(&(frame.payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&frame.payload);
    Ok(out)
}

/// Incremental decoder for a byte stream (TCP delivers arbitrary chunks).
/// After an error the connection must be dropped; the decoder is not usable any more.
#[derive(Debug, Default)]
pub struct FrameDecoder {
    buffer: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn feed(&mut self, chunk: &[u8]) -> Result<Vec<Frame>, FrameError> {
        self.buffer.extend_from_slice(chunk);
        let mut frames = Vec::new();
        while self.buffer.len() >= HEADER_SIZE {
            let header = &self.buffer[..HEADER_SIZE];
            if header[..2] != MAGIC {
                return Err(FrameError::BadMagic);
            }
            let frame_type =
                FrameType::from_wire(header[2]).ok_or(FrameError::UnknownType(header[2]))?;
            let flags = header[3];
            let length = u32::from_be_bytes([header[4], header[5], header[6], header[7]]) as usize;
            if length > MAX_PAYLOAD {
                return Err(FrameError::PayloadTooLarge(length));
            }
            if self.buffer.len() < HEADER_SIZE + length {
                break;
            }
            let payload = self.buffer[HEADER_SIZE..HEADER_SIZE + length].to_vec();
            self.buffer.drain(..HEADER_SIZE + length);
            frames.push(Frame {
                frame_type,
                flags,
                payload,
            });
        }
        Ok(frames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_frames_split_across_chunks() {
        let a = encode_frame(&Frame::new(FrameType::Input, vec![1, 2, 3])).unwrap();
        let b = encode_frame(&Frame::new(FrameType::Ping, vec![])).unwrap();
        let stream = [a, b].concat();
        let mut decoder = FrameDecoder::new();
        let mut frames = Vec::new();
        // One byte at a time: worst case for a TCP stream.
        for byte in &stream {
            frames.extend(decoder.feed(&[*byte]).unwrap());
        }
        assert_eq!(
            frames,
            vec![
                Frame::new(FrameType::Input, vec![1, 2, 3]),
                Frame::new(FrameType::Ping, vec![])
            ]
        );
    }

    #[test]
    fn decodes_two_frames_in_one_chunk() {
        let stream = [
            encode_frame(&Frame::new(FrameType::Ping, vec![])).unwrap(),
            encode_frame(&Frame::new(FrameType::Pong, vec![])).unwrap(),
        ]
        .concat();
        let frames = FrameDecoder::new().feed(&stream).unwrap();
        assert_eq!(
            frames.iter().map(|f| f.frame_type).collect::<Vec<_>>(),
            vec![FrameType::Ping, FrameType::Pong]
        );
    }

    #[test]
    fn keeps_flags() {
        let frame = Frame {
            frame_type: FrameType::Control,
            flags: 0x80,
            payload: vec![9],
        };
        let decoded = FrameDecoder::new()
            .feed(&encode_frame(&frame).unwrap())
            .unwrap();
        assert_eq!(decoded, vec![frame]);
    }

    #[test]
    fn rejects_bad_magic_unknown_type_and_huge_payload() {
        assert_eq!(
            FrameDecoder::new().feed(&[0, 0, 1, 0, 0, 0, 0, 0]),
            Err(FrameError::BadMagic)
        );
        assert_eq!(
            FrameDecoder::new().feed(&[0x45, 0x48, 0x7F, 0, 0, 0, 0, 0]),
            Err(FrameError::UnknownType(0x7F))
        );
        let too_big = (MAX_PAYLOAD + 1) as u32;
        let mut header = vec![0x45, 0x48, 3, 0];
        header.extend_from_slice(&too_big.to_be_bytes());
        assert_eq!(
            FrameDecoder::new().feed(&header),
            Err(FrameError::PayloadTooLarge(MAX_PAYLOAD + 1))
        );
    }

    #[test]
    fn refuses_to_encode_an_oversized_payload() {
        let frame = Frame::new(FrameType::Input, vec![0; MAX_PAYLOAD + 1]);
        assert_eq!(
            encode_frame(&frame),
            Err(FrameError::PayloadTooLarge(MAX_PAYLOAD + 1))
        );
    }

    #[test]
    fn waits_for_the_rest_of_a_partial_frame() {
        let encoded = encode_frame(&Frame::new(FrameType::Input, vec![7; 10])).unwrap();
        let mut decoder = FrameDecoder::new();
        assert!(decoder.feed(&encoded[..12]).unwrap().is_empty());
        assert_eq!(decoder.feed(&encoded[12..]).unwrap().len(), 1);
    }
}
