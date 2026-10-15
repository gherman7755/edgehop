use thiserror::Error;

/// Why a client could not connect.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum LinkError {
    /// The other computer answered but refused the token.
    #[error("the other computer rejected the token")]
    Rejected,
    /// The other computer is not an EdgeHop receiver, or sent something invalid during the handshake.
    #[error("protocol error: {0}")]
    Protocol(String),
    /// Could not reach it, or the connection closed or timed out. The text is meant to be shown to a user.
    #[error("{0}")]
    Connection(String),
}
