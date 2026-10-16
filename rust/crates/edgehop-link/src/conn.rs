//! Framed connection helpers shared by the server and the client.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use edgehop_proto::{encode_frame, Frame, FrameDecoder, FrameError, Opener, Sealer};

/// Locks a mutex, ignoring poisoning: the data is plain state and the caller is shutting a connection down.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Monotonic milliseconds, the clock of ping payloads.
pub(crate) fn now_millis() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Serialises writes from several threads onto one socket, encrypting each frame on the way (the handshake has
/// already happened).
pub(crate) struct FrameWriter {
    /// `None` once shut down: the write half's handle is then closed, so the socket is fully released as soon as the
    /// reading side lets go of its own handle.
    stream: Mutex<Option<TcpStream>>,
    sealer: Mutex<Sealer>,
}

impl FrameWriter {
    pub(crate) fn new(stream: TcpStream, sealer: Sealer) -> Self {
        Self {
            stream: Mutex::new(Some(stream)),
            sealer: Mutex::new(sealer),
        }
    }

    pub(crate) fn send(&self, frame: &Frame) -> io::Result<()> {
        let bytes =
            encode_frame(frame).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        // The stream lock is held while sealing, so the order on the wire is the order of the nonces.
        let mut guard = lock(&self.stream);
        let Some(stream) = guard.as_mut() else {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        };
        let sealed = lock(&self.sealer)
            .seal(&bytes)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        stream.write_all(&sealed)
    }

    /// Closes the connection. The shutdown is not trusted to be enough on its own: on Windows it has been seen to
    /// fail with "not connected" and then no FIN reached the other side, so the handle is closed too, and the
    /// reader's handle goes with its thread within one wake-up.
    pub(crate) fn shutdown(&self) {
        if let Some(stream) = lock(&self.stream).take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

/// How often a blocked read wakes up to check whether it should give up.
const WAKE_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub(crate) enum ReadError {
    /// The peer closed the connection.
    Closed,
    /// The deadline passed.
    TimedOut,
    Io(io::Error),
    /// The peer broke the framing: the connection must be dropped.
    Frame(FrameError),
    /// A message did not verify (damaged, replayed or forged): the connection must be dropped.
    Crypto,
}

pub(crate) struct FrameReader {
    stream: TcpStream,
    opener: Opener,
    decoder: FrameDecoder,
    queue: VecDeque<Frame>,
}

impl FrameReader {
    /// `opener` may already hold bytes that arrived behind the last handshake message.
    pub(crate) fn new(stream: TcpStream, opener: Opener) -> Self {
        Self {
            stream,
            opener,
            decoder: FrameDecoder::new(),
            queue: VecDeque::new(),
        }
    }

    /// The next frame, waiting for as long as the connection lives, but returning `Closed` soon after `closing`
    /// is set.
    ///
    /// A plain blocking read cannot be relied on to end when another thread calls `shutdown` on the socket:
    /// on Windows the read can stay blocked forever, which made `close()` hang (found with a stress test on a
    /// Windows runner). So the read wakes every `WAKE_INTERVAL` and looks at the flag itself.
    pub(crate) fn next_frame_until_closed(
        &mut self,
        closing: &AtomicBool,
    ) -> Result<Frame, ReadError> {
        loop {
            match self.next_frame(Some(Instant::now() + WAKE_INTERVAL)) {
                Err(ReadError::TimedOut) if !closing.load(Ordering::SeqCst) => {}
                Err(ReadError::TimedOut) => return Err(ReadError::Closed),
                other => return other,
            }
        }
    }

    /// The next frame, waiting until `deadline` (forever if `None`).
    pub(crate) fn next_frame(&mut self, deadline: Option<Instant>) -> Result<Frame, ReadError> {
        let mut buffer = [0u8; 16 * 1024];
        loop {
            if let Some(frame) = self.queue.pop_front() {
                return Ok(frame);
            }
            // Bytes the handshake read ahead are decrypted before anything new is read.
            let ahead = self.opener.open(&[]).map_err(|_| ReadError::Crypto)?;
            if !ahead.is_empty() {
                self.queue
                    .extend(self.decoder.feed(&ahead).map_err(ReadError::Frame)?);
                continue;
            }
            let timeout = match deadline {
                None => None,
                Some(deadline) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Err(ReadError::TimedOut);
                    }
                    Some(remaining.max(Duration::from_millis(1)))
                }
            };
            self.stream
                .set_read_timeout(timeout)
                .map_err(ReadError::Io)?;
            match self.stream.read(&mut buffer) {
                Ok(0) => return Err(ReadError::Closed),
                Ok(n) => {
                    let plaintext = self
                        .opener
                        .open(&buffer[..n])
                        .map_err(|_| ReadError::Crypto)?;
                    self.queue
                        .extend(self.decoder.feed(&plaintext).map_err(ReadError::Frame)?);
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // Unix reports an expired read timeout as WouldBlock, Windows as TimedOut.
                Err(e)
                    if matches!(
                        e.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    return Err(ReadError::TimedOut)
                }
                Err(e) => return Err(ReadError::Io(e)),
            }
        }
    }
}
