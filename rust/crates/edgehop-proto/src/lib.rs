//! EdgeHop wire protocol (`docs/PROTOCOL.md`), shared by every platform core.
//!
//! Pure logic only: no sockets, no operating-system calls, so it is unit-tested on any machine and its bytes
//! are checked against the Swift implementation (`tests/golden.json`).

mod auth;
mod event;
mod frame;
mod hello;

pub use auth::{make_nonce, proof, verify, NONCE_LEN, PROOF_LEN};
pub use event::{InputEvent, Modifiers, MouseButton, RecordError, RECORD_SIZE};
pub use frame::{
    encode_frame, Frame, FrameDecoder, FrameError, FrameType, HEADER_SIZE, MAGIC, MAX_PAYLOAD,
};
pub use hello::{HelloMessage, MonitorInfo};

/// Default TCP port of a receiver.
pub const DEFAULT_PORT: u16 = 47474;
/// Bonjour / mDNS service type a receiver advertises.
pub const SERVICE_TYPE: &str = "_edgehop._tcp";
/// Version carried in `hello` messages.
pub const PROTOCOL_VERSION: i32 = 1;
