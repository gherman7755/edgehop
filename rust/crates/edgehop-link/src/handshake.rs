//! The encrypted handshake on a socket (`edgehop_proto::noise` has the cryptography; this moves its messages
//! across a `TcpStream` with deadlines).

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use edgehop_proto::{
    looks_like_older_edgehop, with_length, Handshake, MessageReader, NoiseError, Opener, Role,
    Sealer,
};

/// How often a blocked read wakes up to look at the stop flag (a plain blocking read cannot be relied on to end
/// when another thread shuts the socket down on Windows).
const WAKE_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub(crate) enum HandshakeError {
    /// The other computer closed the connection.
    Closed,
    TimedOut,
    Io(io::Error),
    /// The other computer sent the hello frame of an EdgeHop from before the encrypted link.
    OlderEdgeHop,
    /// A message did not verify: most often a wrong token.
    Noise,
    /// The server is stopping.
    Stopped,
}

impl From<NoiseError> for HandshakeError {
    fn from(_: NoiseError) -> Self {
        Self::Noise
    }
}

fn next_message(
    stream: &mut TcpStream,
    reader: &mut MessageReader,
    deadline: Instant,
    stop: Option<&AtomicBool>,
    expect_older_hello: bool,
    first_read: &mut bool,
) -> Result<Vec<u8>, HandshakeError> {
    let mut buffer = [0u8; 4096];
    loop {
        if let Some(message) = reader.next_message() {
            return Ok(message);
        }
        if stop.is_some_and(|stop| stop.load(Ordering::SeqCst)) {
            return Err(HandshakeError::Stopped);
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(HandshakeError::TimedOut);
        }
        stream
            .set_read_timeout(Some(
                remaining.min(WAKE_INTERVAL).max(Duration::from_millis(1)),
            ))
            .map_err(HandshakeError::Io)?;
        match stream.read(&mut buffer) {
            Ok(0) => return Err(HandshakeError::Closed),
            Ok(n) => {
                if expect_older_hello
                    && *first_read
                    && n >= 2
                    && looks_like_older_edgehop([buffer[0], buffer[1]])
                {
                    return Err(HandshakeError::OlderEdgeHop);
                }
                *first_read = false;
                reader.push(&buffer[..n]);
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            // Unix reports an expired read timeout as WouldBlock, Windows as TimedOut.
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(HandshakeError::Io(e)),
        }
    }
}

/// The controller's side: sends message 1, reads message 2, sends message 3 carrying `hello`. Whether the receiver
/// accepted the token is learned from the first encrypted frame (or from its absence).
pub(crate) fn initiate(
    stream: &mut TcpStream,
    token: &str,
    hello: &[u8],
    deadline: Instant,
) -> Result<(Sealer, Opener), HandshakeError> {
    let mut handshake = Handshake::new(Role::Initiator, token)?;
    stream
        .write_all(&with_length(&handshake.write(&[])?))
        .map_err(HandshakeError::Io)?;
    let mut reader = MessageReader::new();
    let mut first_read = true;
    let second = next_message(stream, &mut reader, deadline, None, true, &mut first_read)?;
    handshake.read(&second)?;
    stream
        .write_all(&with_length(&handshake.write(hello)?))
        .map_err(HandshakeError::Io)?;
    let (sealer, mut opener) = handshake.into_transport()?;
    opener.feed_leftover(reader.pending());
    Ok((sealer, opener))
}

/// The receiver's side: reads message 1, sends message 2, reads message 3. Returns the channel and the controller's
/// hello (the payload of message 3), which only a computer that knows the token can have produced.
pub(crate) fn respond(
    stream: &mut TcpStream,
    token: &str,
    deadline: Instant,
    stop: &AtomicBool,
) -> Result<(Sealer, Opener, Vec<u8>), HandshakeError> {
    let mut handshake = Handshake::new(Role::Responder, token)?;
    let mut reader = MessageReader::new();
    let mut first_read = true;
    let first = next_message(
        stream,
        &mut reader,
        deadline,
        Some(stop),
        false,
        &mut first_read,
    )?;
    handshake.read(&first)?;
    stream
        .write_all(&with_length(&handshake.write(&[])?))
        .map_err(HandshakeError::Io)?;
    let third = next_message(
        stream,
        &mut reader,
        deadline,
        Some(stop),
        false,
        &mut first_read,
    )?;
    let hello = handshake.read(&third)?;
    let (sealer, mut opener) = handshake.into_transport()?;
    opener.feed_leftover(reader.pending());
    Ok((sealer, opener, hello))
}
