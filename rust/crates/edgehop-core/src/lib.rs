//! The EdgeHop core process (`docs/IPC.md`): a headless helper a UI drives over stdin/stdout.
//!
//! * [`ipc`]: newline-delimited JSON server (requests, responses, events).
//! * [`params`]: strict parameter parsing.
//! * [`backend`]: what a platform must provide (sessions, permissions, discovery).
//! * [`service`]: every IPC method on top of a [`backend::Backend`].
//! * [`session`]: the real backend, joining the link, the input layer and edge switching on any [`edgehop_input::Platform`].
//! * [`stub`]: placeholder backend for builds without a platform.
//!
//! Everything here is platform-independent and unit-tested on any machine.

pub mod backend;
pub mod ipc;
pub mod params;
pub mod service;
pub mod session;
pub mod stub;

/// Version reported by `core.hello`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// IPC protocol version reported by `core.hello`.
pub const IPC_VERSION: i64 = 1;
