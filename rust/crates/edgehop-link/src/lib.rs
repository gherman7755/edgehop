//! The EdgeHop TCP link on top of `edgehop-proto` (`docs/PROTOCOL.md`): a token-authenticated server (the
//! computer being controlled) and client (the computer whose keyboard and mouse are shared).
//!
//! Plain `std` threads and blocking sockets: no async runtime. Not encrypted yet (authentication only).

mod client;
mod conn;
mod error;
mod server;

pub use client::{ClientConfig, ClientHandler, LinkClient};
pub use error::LinkError;
pub use server::{LinkServer, ServerConfig, ServerHandler};
