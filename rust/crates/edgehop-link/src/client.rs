//! The controlling side: connects to a receiver, authenticates and sends input.

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use edgehop_proto::{
    ping_payload, proof, rtt_millis, ControlMessage, Frame, FrameType, HelloMessage, InputEvent,
    MonitorInfo,
};

use crate::conn::{lock, now_millis, FrameReader, FrameWriter, ReadError};
use crate::error::LinkError;

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
}

impl ClientConfig {
    pub fn new(token: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            name: name.into(),
            monitors: Vec::new(),
            timeout: Duration::from_secs(8),
        }
    }
}

pub struct LinkClient {
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
        let stream = open(host, port, config.timeout, deadline)?;
        let _ = stream.set_nodelay(true);
        let write_half = stream
            .try_clone()
            .map_err(|e| LinkError::Connection(e.to_string()))?;
        let mut reader = FrameReader::new(stream);

        // 1. the receiver's challenge
        let challenge = next(&mut reader, deadline, config.timeout)?;
        let nonce = HelloMessage::from_frame(&challenge)
            .and_then(|hello| hello.nonce)
            .and_then(|text| STANDARD.decode(text).ok())
            .ok_or_else(|| LinkError::Protocol("expected a challenge".into()))?;

        // 2. our proof
        let writer = Arc::new(FrameWriter::new(write_half));
        let mut answer = HelloMessage::new(config.name.clone());
        answer.proof = Some(STANDARD.encode(proof(&config.token, &nonce)));
        answer.monitors = Some(config.monitors.clone());
        writer
            .send(&answer.to_frame())
            .map_err(|e| LinkError::Connection(e.to_string()))?;

        // 3. accepted or not
        let verdict = next(&mut reader, deadline, config.timeout)?;
        let hello = HelloMessage::from_frame(&verdict)
            .filter(|hello| hello.accepted.is_some())
            .ok_or_else(|| LinkError::Protocol("expected an answer to the proof".into()))?;
        if hello.accepted != Some(true) {
            writer.shutdown();
            return Err(LinkError::Rejected);
        }

        let thread_writer = Arc::clone(&writer);
        let handle = thread::Builder::new()
            .name("edgehop-link-client".into())
            .spawn(move || read_loop(reader, &thread_writer, handler.as_ref()))
            .map_err(|e| LinkError::Connection(e.to_string()))?;
        Ok((
            Self {
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
        eprintln!("[DEBUG-7c1e] client.close: shutting down");
        self.writer.shutdown();
        eprintln!("[DEBUG-7c1e] client.close: shut down, joining reader");
        if let Some(handle) = lock(&self.reader).take() {
            // `on_closed` runs on the reader thread and may drop the client: never join ourselves.
            if handle.thread().id() != thread::current().id() {
                let _ = handle.join();
                eprintln!("[DEBUG-7c1e] client.close: reader joined");
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

fn next(
    reader: &mut FrameReader,
    deadline: Instant,
    timeout: Duration,
) -> Result<Frame, LinkError> {
    reader.next_frame(Some(deadline)).map_err(|e| match e {
        ReadError::TimedOut => LinkError::Connection(format!(
            "connected, but the other computer did not answer the handshake within {} s \
             (is it running EdgeHop, on the same port and version?)",
            timeout.as_secs()
        )),
        ReadError::Closed => {
            LinkError::Connection("the other computer closed the connection".into())
        }
        ReadError::Io(e) => LinkError::Connection(e.to_string()),
        ReadError::Frame(e) => LinkError::Protocol(e.to_string()),
    })
}

fn read_loop(mut reader: FrameReader, writer: &FrameWriter, handler: &dyn ClientHandler) {
    loop {
        let frame = match reader.next_frame(None) {
            Ok(frame) => frame,
            Err(error) => {
                eprintln!("[DEBUG-7c1e] client.read_loop ends: {error:?}");
                break;
            }
        };
        if frame.frame_type == FrameType::Pong {
            if let Some(rtt) = rtt_millis(&frame.payload, now_millis()) {
                handler.on_pong(rtt);
            }
        }
    }
    writer.shutdown();
    handler.on_closed();
}
