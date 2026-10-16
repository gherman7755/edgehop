//! The EdgeHop TCP link on top of `edgehop-proto` (`docs/PROTOCOL.md`): a server (the computer being controlled)
//! and a client (the computer whose keyboard and mouse are shared). Every connection starts with an encrypted
//! handshake keyed by the shared token, and everything after it is encrypted and authenticated.
//!
//! Plain `std` threads and blocking sockets: no async runtime.

mod client;
mod conn;
mod error;
mod handshake;
mod server;

pub use client::{ClientConfig, ClientHandler, LinkClient};
pub use error::LinkError;
pub use server::{LinkServer, ServerConfig, ServerHandler};
