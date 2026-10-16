//! EdgeHop wire protocol (`docs/PROTOCOL.md`), shared by every platform core.
//!
//! Pure logic only: no sockets, no operating-system calls, so it is unit-tested on any machine and its bytes
//! are checked against the Swift implementation (`tests/golden.json`).

mod control;
mod event;
mod frame;
mod hello;
mod keymap;
mod noise;
mod ping;

pub use control::ControlMessage;
pub use event::{InputEvent, Modifiers, MouseButton, RecordError, RECORD_SIZE};
pub use frame::{
    encode_frame, Frame, FrameDecoder, FrameError, FrameType, HEADER_SIZE, MAGIC, MAX_PAYLOAD,
};
pub use hello::{HelloMessage, MonitorInfo};
pub use keymap::{
    hid_to_windows, is_panic_chord, modifier_for, windows_key_to_hid, windows_to_hid, WinKey,
    HID_ESCAPE, KEYMAP_NAME,
};
pub use noise::{
    looks_like_older_edgehop, psk_from_token, with_length, Handshake, MessageReader, NoiseError,
    Opener, Role, Sealer, MAX_CHUNK, MAX_MESSAGE, PATTERN as NOISE_PATTERN,
    PROLOGUE as NOISE_PROLOGUE,
};
pub use ping::{ping_payload, rtt_millis};

/// Default TCP port of a receiver.
pub const DEFAULT_PORT: u16 = 47474;
/// Bonjour / mDNS service type a receiver advertises.
pub const SERVICE_TYPE: &str = "_edgehop._tcp";
/// Version carried in `hello` messages.
pub const PROTOCOL_VERSION: i32 = 2;
