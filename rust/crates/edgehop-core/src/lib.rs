//! The EdgeHop core process (`docs/IPC.md`): a headless helper a UI drives over stdin/stdout.
//!
//! * [`ipc`]: newline-delimited JSON server (requests, responses, events).
//! * [`params`]: strict parameter parsing.
//! * [`backend`]: what a platform must provide (sessions, permissions, discovery).
//! * [`service`]: every IPC method on top of a [`backend::Backend`].
//! * [`stub`]: placeholder backend used until the real network and input layers exist.
//!
//! Everything here is platform-independent and unit-tested on any machine.

pub mod backend;
pub mod ipc;
pub mod params;

/// Version reported by `core.hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// IPC protocol version reported by `core.hello`.
pub const IPC_VERSION: i64 = 1;
