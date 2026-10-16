//! The controlling side: connects to a receiver, authenticates and sends input.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use edgehop_proto::{
    ping_payload, rtt_millis, ControlMessage, Frame, FrameType, HelloMessage, InputEvent,
    MonitorInfo,
};

use crate::conn::{lock, now_millis, FrameReader, FrameWriter, ReadError};
use crate::error::LinkError;
use crate::handshake::{initiate, HandshakeError};

/// Callbacks run on the reader thread.
pub trait ClientHandler: Send + Sync + 'static {
    /// Round-trip time of a ping, in milliseconds.
    fn on_pong(&self, _rtt_millis: f64) {}
    /// The connection ended (the other computer closed it, the network dropped, or `close` was called).
    fn on_closed(&self) {}
}

#[derive(Debug, Clone)]
pub struct ClientConfig {
    pub token: String,
    /// Shown to the receiver (the computer name).
    pub name: String,
    pub monitors: Vec<MonitorInfo>,
    /// Limit for reaching the receiver and for the whole handshake.
    pub timeout: Duration,
    /// Announce HID key codes in the handshake (`false` only to play an older computer in tests).
    pub announce_keymap: bool,
}

impl ClientConfig {
    pub fn new(token: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            name: name.into(),
            monitors: Vec::new(),
            timeout: Duration::from_secs(8),
            announce_keymap: true,
        }
    }
}

pub struct LinkClient {
    closing: Arc<AtomicBool>,
    writer: Arc<FrameWriter>,
    reader: Mutex<Option<JoinHandle<()>>>,
}

impl LinkClient {
    /// Connects and authenticates (blocking). Returns the client and the receiver's hello (its name and
    /// monitors).
    pub fn connect(
        host: &str,
        port: u16,
        config: &ClientConfig,
        handler: Arc<dyn ClientHandler>,
    ) -> Result<(Self, HelloMessage), LinkError> {
        let deadline = Instant::now() + config.timeout;
        let mut stream = open(host, port, config.timeout, deadline)?;
        let _ = stream.set_nodelay(true);
        let write_half = stream
            .try_clone()
            .map_err(|e| LinkError::Connection(e.to_string()))?;

        // The encrypted handshake: it only gets through if both computers know the token. Our hello (name,
        // monitors, key map) is the payload of its last message.
        let mut hello = HelloMessage::new(config.name.clone());
        if config.announce_keymap {
            hello = hello.with_keymap();
        }
        hello.monitors = Some(config.monitors.clone());
        let (sealer, opener) = initiate(
            &mut stream,
            &config.token,
            &hello.to_frame().payload,
            deadline,
        )
        .map_err(|e| handshake_error(e, config.timeout))?;
        let writer = Arc::new(FrameWriter::new(write_half, sealer));
        let mut reader = FrameReader::new(stream, opener);

        // The receiver answers with the first encrypted frame; a receiver that could not verify our last
        // handshake message (a different token) just hangs up instead.
        let answer = match reader.next_frame(Some(deadline)) {
            Ok(frame) => frame,
            Err(ReadError::Closed) => {
                writer.shutdown();
                return Err(LinkError::Rejected);
            }
            Err(error) => return Err(read_error(error, config.timeout)),
        };
        let hello = HelloMessage::from_frame(&answer)
            .filter(|hello| hello.accepted == Some(true))
            .ok_or_else(|| LinkError::Protocol("expected the receiver's answer".into()))?;

        let closing = Arc::new(AtomicBool::new(false));
        let thread_closing = Arc::clone(&closing);
        let thread_writer = Arc::clone(&writer);
        let handle = thread::Builder::new()
            .name("edgehop-link-client".into())
            .spawn(move || read_loop(reader, &thread_writer, handler.as_ref(), &thread_closing))
            .map_err(|e| LinkError::Connection(e.to_string()))?;
        Ok((
            Self {
                closing,
                writer,
                reader: Mutex::new(Some(handle)),
            },
            hello,
        ))
    }

    pub fn send_input(&self, event: &InputEvent) -> io::Result<()> {
        self.writer
            .send(&Frame::new(FrameType::Input, event.encode().to_vec()))
    }

    pub fn send_control(&self, message: ControlMessage) -> io::Result<()> {
        self.writer.send(&message.to_frame())
    }

    /// Sends a ping; the answer arrives through [`ClientHandler::on_pong`].
    pub fn send_ping(&self) -> io::Result<()> {
        self.writer.send(&Frame::new(
            FrameType::Ping,
            ping_payload(now_millis()).to_vec(),
        ))
    }

    /// Closes the connection. Idempotent.
    pub fn close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.writer.shutdown();
        if let Some(handle) = lock(&self.reader).take() {
            // `on_closed` runs on the reader thread and may drop the client: never join ourselves.
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
            }
        }
    }
}

impl Drop for LinkClient {
    fn drop(&mut self) {
        self.close();
    }
}

fn open(
    host: &str,
    port: u16,
    timeout: Duration,
    deadline: Instant,
) -> Result<TcpStream, LinkError> {
    let addresses: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|e| LinkError::Connection(format!("cannot find {host}: {e}")))?
        .collect();
    let mut last = String::from("no address");
    for address in addresses {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match TcpStream::connect_timeout(&address, remaining) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = e.to_string(),
        }
    }
    Err(LinkError::Connection(format!(
        "could not reach {host}:{port} within {} s ({last})",
        timeout.as_secs()
    )))
}

fn handshake_error(error: HandshakeError, timeout: Duration) -> LinkError {
    match error {
        HandshakeError::TimedOut => LinkError::Connection(format!(
            "connected, but the other computer did not answer the handshake within {} s \
             (is it running EdgeHop, on the same port and version?)",
            timeout.as_secs()
        )),
        HandshakeError::Closed => {
            LinkError::Connection("the other computer closed the connection".into())
        }
        HandshakeError::Io(e) => LinkError::Connection(e.to_string()),
        HandshakeError::OlderEdgeHop => LinkError::Protocol(
            "the other computer runs an older, unencrypted EdgeHop: update it there".into(),
        ),
        HandshakeError::Noise => LinkError::Protocol("the encrypted handshake failed".into()),
        HandshakeError::Stopped => LinkError::Connection("stopped".into()),
    }
}

fn read_error(error: ReadError, timeout: Duration) -> LinkError {
    match error {
        ReadError::TimedOut => LinkError::Connection(format!(
            "connected, but the other computer did not answer within {} s \
             (is it running EdgeHop, on the same port and version?)",
            timeout.as_secs()
        )),
        ReadError::Closed => {
            LinkError::Connection("the other computer closed the connection".into())
        }
        ReadError::Io(e) => LinkError::Connection(e.to_string()),
        ReadError::Frame(e) => LinkError::Protocol(e.to_string()),
        ReadError::Crypto => {
            LinkError::Protocol("the encrypted link failed to verify a message".into())
        }
    }
}

fn read_loop(
    mut reader: FrameReader,
    writer: &FrameWriter,
    handler: &dyn ClientHandler,
    closing: &AtomicBool,
) {
    while let Ok(frame) = reader.next_frame_until_closed(closing) {
        if frame.frame_type == FrameType::Pong {
            if let Some(rtt) = rtt_millis(&frame.payload, now_millis()) {
                handler.on_pong(rtt);
            }
        }
    }
    writer.shutdown();
    handler.on_closed();
}
