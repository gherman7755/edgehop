//! The receiving side: accepts one authenticated controller at a time and delivers its input.

use std::io;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use edgehop_proto::{
    make_nonce, verify, ControlMessage, Frame, FrameType, HelloMessage, InputEvent, MonitorInfo,
    DEFAULT_PORT,
};

use crate::conn::{lock, FrameReader, FrameWriter};

/// Callbacks run on the connection's thread: keep them quick (injection is fine, blocking I/O is not).
pub trait ServerHandler: Send + Sync + 'static {
    /// The peer passed authentication. `hello` carries its name and monitors.
    fn on_peer_connected(&self, _hello: &HelloMessage) {}
    fn on_input(&self, _event: InputEvent) {}
    fn on_control(&self, _message: ControlMessage) {}
    /// An authenticated peer is gone (closed, broke the protocol or the server stopped).
    fn on_peer_disconnected(&self) {}
}

#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub token: String,
    /// Shown to the controller (the computer name).
    pub name: String,
    /// 0 picks a free port.
    pub port: u16,
    pub bind: IpAddr,
    /// Advertised to the controller after authentication.
    pub monitors: Vec<MonitorInfo>,
    /// How long a new connection may take to authenticate before it is dropped.
    pub auth_timeout: Duration,
}

impl ServerConfig {
    pub fn new(token: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            name: name.into(),
            port: DEFAULT_PORT,
            bind: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            monitors: Vec::new(),
            auth_timeout: Duration::from_secs(5),
        }
    }
}

struct Shared {
    config: ServerConfig,
    handler: Arc<dyn ServerHandler>,
    stop: AtomicBool,
    next_id: AtomicU64,
    /// The one connection being served: its id and a handle to shut it down from `stop`.
    active: Mutex<Option<(u64, TcpStream)>>,
}

impl Shared {
    fn release(&self, id: u64) {
        let mut active = lock(&self.active);
        if active.as_ref().is_some_and(|(current, _)| *current == id) {
            *active = None;
        }
    }
}

pub struct LinkServer {
    port: u16,
    wake: SocketAddr,
    shared: Arc<Shared>,
    accept: Mutex<Option<JoinHandle<()>>>,
}

impl LinkServer {
    /// Binds and starts accepting. Returns once the socket is listening.
    pub fn start(config: ServerConfig, handler: Arc<dyn ServerHandler>) -> io::Result<Self> {
        let listener = TcpListener::bind((config.bind, config.port))?;
        let port = listener.local_addr()?.port();
        let wake_ip = match config.bind {
            ip if !ip.is_unspecified() => ip,
            IpAddr::V4(_) => IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(_) => IpAddr::V6(Ipv6Addr::LOCALHOST),
        };
        let shared = Arc::new(Shared {
            config,
            handler,
            stop: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            active: Mutex::new(None),
        });
        let accept_shared = Arc::clone(&shared);
        let accept = thread::Builder::new()
            .name("edgehop-link-accept".into())
            .spawn(move || accept_loop(&listener, &accept_shared))?;
        Ok(Self {
            port,
            wake: SocketAddr::new(wake_ip, port),
            shared,
            accept: Mutex::new(Some(accept)),
        })
    }

    /// The port actually listened on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Stops accepting and drops the current peer. Idempotent.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // `accept` blocks: a throw-away connection to ourselves wakes it so it can see the stop flag.
        let _ = TcpStream::connect_timeout(&self.wake, Duration::from_secs(1));
        if let Some(handle) = lock(&self.accept).take() {
            let _ = handle.join();
        }
        if let Some((_, stream)) = lock(&self.shared.active).as_ref() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

impl Drop for LinkServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept_loop(listener: &TcpListener, shared: &Arc<Shared>) {
    for incoming in listener.incoming() {
        if shared.stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = incoming else { continue };
        let id = shared.next_id.fetch_add(1, Ordering::SeqCst);
        {
            let mut active = lock(&shared.active);
            if active.is_some() {
                continue; // one peer at a time: the new connection is dropped
            }
            let Ok(handle) = stream.try_clone() else {
                continue;
            };
            *active = Some((id, handle));
        }
        let peer_shared = Arc::clone(shared);
        let spawned = thread::Builder::new()
            .name("edgehop-link-peer".into())
            .spawn(move || {
                serve_peer(&peer_shared, stream);
                peer_shared.release(id);
            });
        if spawned.is_err() {
            shared.release(id);
        }
    }
}

fn serve_peer(shared: &Shared, stream: TcpStream) {
    let _ = stream.set_nodelay(true); // small input records: never wait for Nagle
    let Ok(write_half) = stream.try_clone() else {
        return;
    };
    let writer = FrameWriter::new(write_half);
    let mut reader = FrameReader::new(stream);
    let config = &shared.config;

    let Ok(nonce) = make_nonce() else { return };
    let mut challenge = HelloMessage::new(config.name.clone());
    challenge.nonce = Some(STANDARD.encode(nonce));
    if writer.send(&challenge.to_frame()).is_err() {
        return;
    }

    // The whole handshake shares one deadline, so a peer cannot stretch it by dribbling bytes.
    let deadline = Instant::now() + config.auth_timeout;
    let Ok(first) = reader.next_frame(Some(deadline)) else {
        return;
    };
    let Some(hello) = authenticate(&config.token, &nonce, &first) else {
        let mut refusal = HelloMessage::new(config.name.clone());
        refusal.accepted = Some(false);
        let _ = writer.send(&refusal.to_frame());
        writer.shutdown();
        return;
    };
    let mut accepted = HelloMessage::new(config.name.clone());
    accepted.accepted = Some(true);
    accepted.monitors = Some(config.monitors.clone());
    if writer.send(&accepted.to_frame()).is_err() {
        return;
    }

    shared.handler.on_peer_connected(&hello);
    while let Ok(frame) = reader.next_frame(None) {
        match frame.frame_type {
            FrameType::Input => {
                // A malformed record is ignored, like in the Swift core.
                if let Ok(event) = InputEvent::decode(&frame.payload) {
                    shared.handler.on_input(event);
                }
            }
            FrameType::Control => {
                if let Some(message) = ControlMessage::from_frame(&frame) {
                    shared.handler.on_control(message);
                }
            }
            FrameType::Ping => {
                if writer
                    .send(&Frame::new(FrameType::Pong, frame.payload))
                    .is_err()
                {
                    break;
                }
            }
            FrameType::Hello | FrameType::Layout | FrameType::Pong => {}
        }
    }
    shared.handler.on_peer_disconnected();
    writer.shutdown();
}

/// The controller's hello if it carries a valid proof for `nonce`.
fn authenticate(token: &str, nonce: &[u8], first: &Frame) -> Option<HelloMessage> {
    let hello = HelloMessage::from_frame(first)?;
    let proof = STANDARD.decode(hello.proof.as_deref()?).ok()?;
    verify(token, nonce, &proof).then_some(hello)
}
