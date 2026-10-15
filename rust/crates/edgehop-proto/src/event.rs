use thiserror::Error;

/// Mouse buttons as carried on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    Back = 3,
    Forward = 4,
}

impl MouseButton {
    fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Left),
            1 => Some(Self::Right),
            2 => Some(Self::Middle),
            3 => Some(Self::Back),
            4 => Some(Self::Forward),
            _ => None,
        }
    }
}

/// Modifier bits of a key event: shift, control, option (Alt) and command (Windows / Meta key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Modifiers(pub u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const SHIFT: Self = Self(1 << 0);
    pub const CONTROL: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const COMMAND: Self = Self(1 << 3);

    /// True if every bit of `other` is set.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// Platform-neutral input event, the unit that travels between computers.
///
/// Scroll values are 1/120 of a wheel notch. `Key::code` is currently a macOS virtual keycode on the wire
/// (a neutral key map is a later milestone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputEvent {
    MouseMoveAbsolute {
        x: i32,
        y: i32,
    },
    MouseMoveDelta {
        dx: i32,
        dy: i32,
    },
    MouseButton {
        button: MouseButton,
        is_down: bool,
    },
    Scroll {
        horizontal: i32,
        vertical: i32,
    },
    Key {
        code: u16,
        is_down: bool,
        modifiers: Modifiers,
    },
}

/// Size in bytes of one input record.
pub const RECORD_SIZE: usize = 16;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RecordError {
    #[error("input record must be {RECORD_SIZE} bytes, got {0}")]
    WrongLength(usize),
    #[error("unknown input record kind {0}")]
    UnknownKind(u8),
    #[error("unknown mouse button {0}")]
    UnknownButton(u8),
}

// Layout (big-endian numbers):
//   0      kind  (0 moveAbs, 1 moveDelta, 2 button, 3 scroll, 4 key)
//   1      flag  (button/key: 1 = down)
//   2      aux   (button: MouseButton; key: modifiers)
//   3      reserved
//   4..8   a     (x / dx / horizontal scroll / key code)
//   8..12  b     (y / dy / vertical scroll)
//   12..16 reserved
impl InputEvent {
    pub fn encode(&self) -> [u8; RECORD_SIZE] {
        fn put(record: &mut [u8; RECORD_SIZE], offset: usize, value: i32) {
            record[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        let mut b = [0u8; RECORD_SIZE];
        match *self {
            Self::MouseMoveAbsolute { x, y } => {
                put(&mut b, 4, x);
                put(&mut b, 8, y);
            }
            Self::MouseMoveDelta { dx, dy } => {
                b[0] = 1;
                put(&mut b, 4, dx);
                put(&mut b, 8, dy);
            }
            Self::MouseButton { button, is_down } => {
                b[0] = 2;
                b[1] = u8::from(is_down);
                b[2] = button as u8;
            }
            Self::Scroll {
                horizontal,
                vertical,
            } => {
                b[0] = 3;
                put(&mut b, 4, horizontal);
                put(&mut b, 8, vertical);
            }
            Self::Key {
                code,
                is_down,
                modifiers,
            } => {
                b[0] = 4;
                b[1] = u8::from(is_down);
                b[2] = modifiers.0;
                put(&mut b, 4, i32::from(code));
            }
        }
        b
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, RecordError> {
        if bytes.len() != RECORD_SIZE {
            return Err(RecordError::WrongLength(bytes.len()));
        }
        let int32 = |offset: usize| {
            i32::from_be_bytes([
                bytes[offset],
                bytes[offset + 1],
                bytes[offset + 2],
                bytes[offset + 3],
            ])
        };
        match bytes[0] {
            0 => Ok(Self::MouseMoveAbsolute {
                x: int32(4),
                y: int32(8),
            }),
            1 => Ok(Self::MouseMoveDelta {
                dx: int32(4),
                dy: int32(8),
            }),
            2 => {
                let button =
                    MouseButton::from_wire(bytes[2]).ok_or(RecordError::UnknownButton(bytes[2]))?;
                Ok(Self::MouseButton {
                    button,
                    is_down: bytes[1] != 0,
                })
            }
            3 => Ok(Self::Scroll {
                horizontal: int32(4),
                vertical: int32(8),
            }),
            // Truncating to 16 bits matches the Swift decoder.
            4 => Ok(Self::Key {
                code: int32(4) as u16,
                is_down: bytes[1] != 0,
                modifiers: Modifiers(bytes[2]),
            }),
            other => Err(RecordError::UnknownKind(other)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn samples() -> Vec<InputEvent> {
        vec![
            InputEvent::MouseMoveAbsolute { x: 1919, y: -3 },
            InputEvent::MouseMoveDelta { dx: -7, dy: 12 },
            InputEvent::MouseButton {
                button: MouseButton::Left,
                is_down: true,
            },
            InputEvent::MouseButton {
                button: MouseButton::Forward,
                is_down: false,
            },
            InputEvent::Scroll {
                horizontal: -120,
                vertical: 360,
            },
            InputEvent::Key {
                code: 53,
                is_down: true,
                modifiers: Modifiers::COMMAND | Modifiers::SHIFT,
            },
            InputEvent::MouseMoveAbsolute {
                x: i32::MIN,
                y: i32::MAX,
            },
        ]
    }

    #[test]
    fn round_trips_every_kind() {
        for event in samples() {
            let record = event.encode();
            assert_eq!(record.len(), RECORD_SIZE);
            assert_eq!(InputEvent::decode(&record), Ok(event));
        }
    }

    #[test]
    fn numbers_are_big_endian() {
        let record = InputEvent::MouseMoveAbsolute {
            x: 0x0102_0304,
            y: 5,
        }
        .encode();
        assert_eq!(&record[4..8], &[1, 2, 3, 4]);
        assert_eq!(&record[8..12], &[0, 0, 0, 5]);
    }

    #[test]
    fn rejects_malformed_records() {
        assert_eq!(
            InputEvent::decode(&[1, 2, 3]),
            Err(RecordError::WrongLength(3))
        );
        let mut bad = InputEvent::MouseMoveDelta { dx: 0, dy: 0 }.encode();
        bad[0] = 99;
        assert_eq!(InputEvent::decode(&bad), Err(RecordError::UnknownKind(99)));
        let mut button = InputEvent::MouseButton {
            button: MouseButton::Left,
            is_down: true,
        }
        .encode();
        button[2] = 9;
        assert_eq!(
            InputEvent::decode(&button),
            Err(RecordError::UnknownButton(9))
        );
    }

    #[test]
    fn modifiers_combine_and_test_bits() {
        let both = Modifiers::CONTROL | Modifiers::OPTION;
        assert!(both.contains(Modifiers::CONTROL));
        assert!(both.contains(Modifiers::CONTROL | Modifiers::OPTION));
        assert!(!both.contains(Modifiers::COMMAND));
        assert!(Modifiers::NONE.contains(Modifiers::NONE));
    }

    #[test]
    fn key_code_is_truncated_to_sixteen_bits_like_the_swift_decoder() {
        let mut record = InputEvent::Key {
            code: 0,
            is_down: true,
            modifiers: Modifiers::NONE,
        }
        .encode();
        record[4..8].copy_from_slice(&0x0001_0035_i32.to_be_bytes());
        assert_eq!(
            InputEvent::decode(&record),
            Ok(InputEvent::Key {
                code: 0x35,
                is_down: true,
                modifiers: Modifiers::NONE
            })
        );
    }
}
