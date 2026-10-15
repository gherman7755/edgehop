//! Framed connection helpers shared by the server and the client.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use edgehop_proto::{encode_frame, Frame, FrameDecoder, FrameError};

/// Locks a mutex, ignoring poisoning: the data is plain state and the caller is shutting a connection down.
pub(crate) fn polling() -> bool {
    std::env::var_os("EDGEHOP_POLL").is_some()
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Monotonic milliseconds, the clock of ping payloads.
pub(crate) fn now_millis() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    u64::try_from(START.get_or_init(Instant::now).elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Serialises writes from several threads onto one socket.
pub(crate) struct FrameWriter {
    stream: Mutex<TcpStream>,
}

impl FrameWriter {
    pub(crate) fn new(stream: TcpStream) -> Self {
        Self {
            stream: Mutex::new(stream),
        }
    }

    pub(crate) fn send(&self, frame: &Frame) -> io::Result<()> {
        let bytes =
            encode_frame(frame).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        lock(&self.stream).write_all(&bytes)
    }

    pub(crate) fn shutdown(&self) {
        // EXPERIMENT: EDGEHOP_SHUTDOWN_WRITE=1 sends only the FIN instead of shutting both directions.
        let how = if std::env::var_os("EDGEHOP_SHUTDOWN_WRITE").is_some() {
            Shutdown::Write
        } else {
            Shutdown::Both
        };
        let _ = lock(&self.stream).shutdown(how);
    }
}

#[derive(Debug)]
pub(crate) enum ReadError {
    /// The peer closed the connection.
    Closed,
    /// The deadline passed.
    TimedOut,
    Io(io::Error),
    /// The peer broke the framing: the connection must be dropped.
    Frame(FrameError),
}

pub(crate) struct FrameReader {
    stream: TcpStream,
    decoder: FrameDecoder,
    queue: VecDeque<Frame>,
}

impl FrameReader {
    pub(crate) fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            decoder: FrameDecoder::new(),
            queue: VecDeque::new(),
        }
    }

    /// EXPERIMENT: like `next_frame(None)` but wakes every 200 ms to check `closing`, so a blocked read can
    /// never outlive a close even if the OS does not abort it.
    pub(crate) fn next_frame_polling(
        &mut self,
        closing: &std::sync::atomic::AtomicBool,
    ) -> Result<Frame, ReadError> {
        loop {
            match self.next_frame(Some(Instant::now() + Duration::from_millis(200))) {
                Err(ReadError::TimedOut) => {
                    if closing.load(std::sync::atomic::Ordering::SeqCst) {
                        return Err(ReadError::Closed);
                    }
                }
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
                Ok(n) => self
                    .queue
                    .extend(self.decoder.feed(&buffer[..n]).map_err(ReadError::Frame)?),
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
