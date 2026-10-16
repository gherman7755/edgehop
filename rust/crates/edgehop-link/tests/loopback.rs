//! Real TCP on 127.0.0.1 (random ports): server and client against each other and against hand-written raw
//! peers that misbehave.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use edgehop_link::{
    ClientConfig, ClientHandler, LinkClient, LinkError, LinkServer, ServerConfig, ServerHandler,
};
use edgehop_proto::{
    encode_frame, with_length, ControlMessage, Frame, FrameDecoder, FrameType, Handshake,
    HelloMessage, InputEvent, MessageReader, Modifiers, MonitorInfo, MouseButton, Opener, Role,
    Sealer,
};

const TOKEN: &str = "loopback-secret-1";

#[derive(Default)]
struct Recorder {
    connected: Mutex<Vec<String>>,
    peer_keymaps: Mutex<Vec<Option<String>>>,
    inputs: Mutex<Vec<InputEvent>>,
    controls: Mutex<Vec<ControlMessage>>,
    disconnects: AtomicUsize,
}

impl ServerHandler for Recorder {
    fn on_peer_connected(&self, hello: &HelloMessage) {
        self.connected.lock().unwrap().push(hello.name.clone());
        self.peer_keymaps.lock().unwrap().push(hello.keymap.clone());
    }
    fn on_input(&self, event: InputEvent) {
        self.inputs.lock().unwrap().push(event);
    }
    fn on_control(&self, message: ControlMessage) {
        self.controls.lock().unwrap().push(message);
    }
    fn on_peer_disconnected(&self) {
        self.disconnects.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Default)]
struct ClientRecorder {
    pongs: Mutex<Vec<f64>>,
    closed: AtomicBool,
}

impl ClientHandler for ClientRecorder {
    fn on_pong(&self, rtt_millis: f64) {
        self.pongs.lock().unwrap().push(rtt_millis);
    }
    fn on_closed(&self) {
        self.closed.store(true, Ordering::SeqCst);
    }
}

fn wait_until(what: &str, mut condition: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if condition() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("timed out waiting for: {what}");
}

fn start_server(auth_timeout: Duration) -> (LinkServer, Arc<Recorder>) {
    let recorder = Arc::new(Recorder::default());
    let mut config = ServerConfig::new(TOKEN, "srv");
    config.port = 0;
    config.bind = "127.0.0.1".parse().unwrap();
    config.auth_timeout = auth_timeout;
    config.monitors = vec![MonitorInfo {
        id: "d1".into(),
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
    }];
    let server = LinkServer::start(config, recorder.clone()).unwrap();
    (server, recorder)
}

fn connect(
    port: u16,
    token: &str,
    name: &str,
) -> Result<(LinkClient, HelloMessage, Arc<ClientRecorder>), LinkError> {
    let handler = Arc::new(ClientRecorder::default());
    let mut config = ClientConfig::new(token, name);
    config.timeout = Duration::from_secs(5);
    let (client, hello) = LinkClient::connect("127.0.0.1", port, &config, handler.clone())?;
    Ok((client, hello, handler))
}

/// A hand-written peer on a raw socket, speaking the encrypted protocol by hand so that tests can misbehave.
struct RawPeer {
    stream: TcpStream,
    sealer: Sealer,
    opener: Opener,
    decoder: FrameDecoder,
    /// Everything this peer has read from the socket, before decryption.
    wire: Vec<u8>,
}

impl RawPeer {
    /// Sends a frame, encrypted.
    fn send(&mut self, frame: &Frame) {
        let sealed = self.sealer.seal(&encode_frame(frame).unwrap()).unwrap();
        self.stream.write_all(&sealed).unwrap();
    }

    /// Sends bytes as they are (damaged, for instance).
    fn send_raw(&mut self, bytes: &[u8]) {
        self.stream.write_all(bytes).unwrap();
    }

    fn next_frame(&mut self) -> Option<Frame> {
        let mut buffer = [0u8; 4096];
        loop {
            let n = self.stream.read(&mut buffer).ok()?;
            if n == 0 {
                return None;
            }
            self.wire.extend_from_slice(&buffer[..n]);
            let plaintext = self.opener.open(&buffer[..n]).ok()?;
            if let Some(frame) = self.decoder.feed(&plaintext).unwrap().into_iter().next() {
                return Some(frame);
            }
        }
    }
}

/// Reads one whole length-prefixed handshake message from a raw socket.
fn read_message(stream: &mut TcpStream, reader: &mut MessageReader) -> Option<Vec<u8>> {
    let mut buffer = [0u8; 4096];
    loop {
        if let Some(message) = reader.next_message() {
            return Some(message);
        }
        let n = stream.read(&mut buffer).ok()?;
        if n == 0 {
            return None;
        }
        reader.push(&buffer[..n]);
    }
}

/// Does the encrypted handshake by hand with `token` and sends `hello` as the payload of the last message.
/// `None` if the server hung up instead (what it does when the token is wrong).
fn raw_handshake(port: u16, token: &str, hello: &HelloMessage) -> Option<RawPeer> {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut handshake = Handshake::new(Role::Initiator, token).unwrap();
    stream
        .write_all(&with_length(&handshake.write(&[]).unwrap()))
        .unwrap();
    let mut reader = MessageReader::new();
    let second = read_message(&mut stream, &mut reader)?;
    handshake.read(&second).unwrap();
    stream
        .write_all(&with_length(
            &handshake.write(&hello.to_frame().payload).unwrap(),
        ))
        .unwrap();
    let (sealer, opener) = handshake.into_transport().unwrap();
    Some(RawPeer {
        stream,
        sealer,
        opener,
        decoder: FrameDecoder::new(),
        wire: Vec::new(),
    })
}

/// A raw peer that has been accepted (the receiver's answer has been read).
fn raw_authenticated(port: u16) -> RawPeer {
    let mut peer = raw_handshake(port, TOKEN, &HelloMessage::new("raw").with_keymap())
        .expect("the handshake with the right token");
    let answer = peer.next_frame().expect("the receiver's answer");
    assert_eq!(
        HelloMessage::from_frame(&answer).unwrap().accepted,
        Some(true)
    );
    peer
}

#[test]
fn authenticates_and_delivers_input_in_order() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (client, hello, _) = connect(server.port(), TOKEN, "cli").unwrap();
    assert_eq!(hello.name, "srv");
    assert_eq!(hello.monitors.as_ref().unwrap()[0].width, 1512.0);
    wait_until("peer connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    assert_eq!(recorder.connected.lock().unwrap()[0], "cli");

    let sent = [
        InputEvent::MouseMoveDelta { dx: 3, dy: -4 },
        InputEvent::MouseButton {
            button: MouseButton::Left,
            is_down: true,
        },
        InputEvent::Scroll {
            horizontal: 0,
            vertical: 120,
        },
        InputEvent::Key {
            code: 53,
            is_down: true,
            modifiers: Modifiers::COMMAND,
        },
    ];
    for event in &sent {
        client.send_input(event).unwrap();
    }
    wait_until("all input delivered", || {
        recorder.inputs.lock().unwrap().len() == sent.len()
    });
    assert_eq!(*recorder.inputs.lock().unwrap(), sent);
}

#[test]
fn rejects_a_wrong_token_and_delivers_nothing() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let error = connect(server.port(), "not-the-token-xx", "cli")
        .err()
        .expect("must be rejected");
    assert_eq!(error, LinkError::Rejected);
    assert!(recorder.connected.lock().unwrap().is_empty());
    assert_eq!(
        recorder.disconnects.load(Ordering::SeqCst),
        0,
        "an unauthenticated peer is not a peer"
    );
    // The server is still usable.
    assert!(connect(server.port(), TOKEN, "cli").is_ok());
}

#[test]
fn both_sides_learn_whether_the_other_speaks_hid_key_codes() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (_client, hello, _) = connect(server.port(), TOKEN, "cli").unwrap();
    assert!(hello.speaks_hid_keys(), "the receiver announces it");
    wait_until("the receiver to see the controller", || {
        !recorder.peer_keymaps.lock().unwrap().is_empty()
    });
    assert_eq!(
        recorder.peer_keymaps.lock().unwrap()[0].as_deref(),
        Some("hid"),
        "the controller announces it"
    );
}

#[test]
fn an_older_peer_that_announces_nothing_is_recognised() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut config = ClientConfig::new(TOKEN, "old");
    config.announce_keymap = false;
    let (_client, _) = LinkClient::connect(
        "127.0.0.1",
        server.port(),
        &config,
        Arc::new(ClientRecorder::default()),
    )
    .unwrap();
    wait_until("the receiver to see the controller", || {
        !recorder.peer_keymaps.lock().unwrap().is_empty()
    });
    assert_eq!(recorder.peer_keymaps.lock().unwrap()[0], None);
}

/// A refused or finished peer must free the slot before it is told, or an immediate retry is dropped.
#[test]
fn an_immediate_retry_after_a_refusal_or_a_disconnect_is_accepted() {
    let (server, _) = start_server(Duration::from_secs(5));
    for round in 0..200 {
        let refused = connect(server.port(), "not-the-token-xx", "cli");
        assert_eq!(refused.err(), Some(LinkError::Rejected), "round {round}");
        let (client, _, _) = connect(server.port(), TOKEN, "cli")
            .unwrap_or_else(|e| panic!("retry after a refusal, round {round}: {e}"));
        drop(client);
        let (again, _, _) = connect(server.port(), TOKEN, "cli")
            .unwrap_or_else(|e| panic!("retry after a disconnect, round {round}: {e}"));
        drop(again);
    }
}

#[test]
fn answers_ping_with_the_same_payload() {
    let (server, _) = start_server(Duration::from_secs(5));
    let (client, _, handler) = connect(server.port(), TOKEN, "cli").unwrap();
    client.send_ping().unwrap();
    wait_until("pong", || !handler.pongs.lock().unwrap().is_empty());
    let rtt = handler.pongs.lock().unwrap()[0];
    assert!((0.0..1000.0).contains(&rtt), "rtt was {rtt}");
}

#[test]
fn delivers_control_messages() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (client, _, _) = connect(server.port(), TOKEN, "cli").unwrap();
    client.send_control(ControlMessage::ReleaseAll).unwrap();
    wait_until("control", || recorder.controls.lock().unwrap().len() == 1);
    assert_eq!(
        recorder.controls.lock().unwrap()[0],
        ControlMessage::ReleaseAll
    );
}

#[test]
fn reports_a_disconnect_and_accepts_a_new_peer() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (first, _, _) = connect(server.port(), TOKEN, "one").unwrap();
    wait_until("first connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    first.close();
    wait_until("disconnect reported", || {
        recorder.disconnects.load(Ordering::SeqCst) == 1
    });

    let (_second, _, _) = connect(server.port(), TOKEN, "two").unwrap();
    wait_until("second connected", || {
        recorder.connected.lock().unwrap().len() == 2
    });
    assert_eq!(
        *recorder.connected.lock().unwrap(),
        vec!["one".to_string(), "two".to_string()]
    );
}

#[test]
fn refuses_a_second_concurrent_peer() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (_first, _, _) = connect(server.port(), TOKEN, "one").unwrap();
    wait_until("first connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });

    let second = connect(server.port(), TOKEN, "two");
    assert!(
        matches!(second, Err(LinkError::Connection(_))),
        "got {:?}",
        second.err()
    );
    assert_eq!(recorder.connected.lock().unwrap().len(), 1);
}

#[test]
fn drops_a_peer_that_never_finishes_the_handshake_after_the_timeout() {
    let (server, recorder) = start_server(Duration::from_millis(300));
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    // Say nothing: the server must hang up.
    let started = Instant::now();
    let mut buffer = [0u8; 16];
    assert_eq!(
        stream.read(&mut buffer).unwrap_or(0),
        0,
        "connection should be closed by the server"
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(recorder.connected.lock().unwrap().is_empty());
    assert!(
        connect(server.port(), TOKEN, "cli").is_ok(),
        "the slot is free again"
    );
}

#[test]
fn a_peer_that_does_not_speak_the_handshake_is_dropped() {
    // What an EdgeHop from before the encrypted link sends first is a hello frame, starting with "EH".
    let (server, recorder) = start_server(Duration::from_millis(400));
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream
        .write_all(b"EH\x01\x00\x00\x00\x00\x10{\"version\":1}")
        .unwrap();
    let mut buffer = [0u8; 64];
    assert_eq!(
        stream.read(&mut buffer).unwrap_or(0),
        0,
        "it must hang up without answering"
    );
    assert!(recorder.connected.lock().unwrap().is_empty());
}

#[test]
fn a_wrong_token_gets_no_answer_at_all() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut peer = raw_handshake(server.port(), "not-the-token-xx", &HelloMessage::new("raw"))
        .expect("the first two messages do not depend on the token");
    assert!(peer.next_frame().is_none(), "the server hung up");
    assert!(peer.wire.is_empty(), "nothing was written back");
    assert!(recorder.connected.lock().unwrap().is_empty());
}

#[test]
fn nothing_readable_crosses_the_wire() {
    let (server, _) = start_server(Duration::from_secs(5));
    let mut peer = raw_handshake(server.port(), TOKEN, &HelloMessage::new("raw")).unwrap();
    assert!(peer.next_frame().is_some());
    // The receiver's answer carries its name ("srv") and its monitors: none of it is in the clear.
    let wire = String::from_utf8_lossy(&peer.wire).to_string();
    assert!(
        !wire.contains("srv") && !wire.contains("monitors") && !wire.contains("accepted"),
        "{wire:?}"
    );
    assert!(!peer.wire.is_empty());
}

#[test]
fn a_damaged_message_drops_the_peer() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut peer = raw_authenticated(server.port());
    wait_until("peer connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    let mut sealed = peer
        .sealer
        .seal(&encode_frame(&Frame::new(FrameType::Ping, vec![0; 8])).unwrap())
        .unwrap();
    *sealed.last_mut().unwrap() ^= 1;
    peer.send_raw(&sealed);
    wait_until("the peer to be dropped", || {
        recorder.disconnects.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn a_frame_larger_than_one_encrypted_message_arrives_whole() {
    let (server, _) = start_server(Duration::from_secs(5));
    let mut peer = raw_authenticated(server.port());
    // A layout frame is ignored by the receiver, but it has to be decrypted and reassembled from three messages for
    // the ping behind it to be understood.
    peer.send(&Frame::new(FrameType::Layout, vec![7; 150_000]));
    peer.send(&Frame::new(FrameType::Ping, 42u64.to_be_bytes().to_vec()));
    let pong = peer.next_frame().expect("the pong behind the big frame");
    assert_eq!(pong.frame_type, FrameType::Pong);
    assert_eq!(pong.payload, 42u64.to_be_bytes().to_vec());
}

#[test]
fn a_client_recognises_a_receiver_from_before_the_encrypted_link() {
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // The old receiver speaks first: a hello frame with a challenge.
            let _ = stream.write_all(&encode_frame(&HelloMessage::new("old").to_frame()).unwrap());
            std::thread::sleep(Duration::from_millis(500));
        }
    });
    let error = connect(port, TOKEN, "cli").err().expect("must fail");
    match error {
        LinkError::Protocol(message) => assert!(message.contains("older"), "{message}"),
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn drops_a_peer_that_breaks_the_framing_and_then_serves_the_next_one() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut peer = raw_authenticated(server.port());
    wait_until("raw peer connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    // A well-encrypted message whose plaintext breaks the framing (bad magic).
    let sealed = peer
        .sealer
        .seal(&[0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0])
        .unwrap();
    peer.send_raw(&sealed);
    wait_until("disconnect reported", || {
        recorder.disconnects.load(Ordering::SeqCst) == 1
    });

    let (client, _, _) = connect(server.port(), TOKEN, "cli").unwrap();
    client
        .send_input(&InputEvent::MouseMoveDelta { dx: 1, dy: 1 })
        .unwrap();
    wait_until("input after the violation", || {
        recorder.inputs.lock().unwrap().len() == 1
    });
}

#[test]
fn ignores_a_malformed_input_record_but_keeps_the_connection() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut peer = raw_authenticated(server.port());
    peer.send(&Frame::new(FrameType::Input, vec![99; 16])); // unknown kind
    let good = InputEvent::MouseMoveDelta { dx: 5, dy: 6 };
    peer.send(&Frame::new(FrameType::Input, good.encode().to_vec()));
    wait_until("the valid record after the bad one", || {
        recorder.inputs.lock().unwrap().len() == 1
    });
    assert_eq!(recorder.inputs.lock().unwrap()[0], good);
    assert_eq!(recorder.disconnects.load(Ordering::SeqCst), 0);
}

#[test]
fn stopping_the_server_closes_the_connection() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let (_client, _, handler) = connect(server.port(), TOKEN, "cli").unwrap();
    wait_until("connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    server.stop();
    wait_until("client sees the close", || {
        handler.closed.load(Ordering::SeqCst)
    });
    wait_until("server reports the disconnect", || {
        recorder.disconnects.load(Ordering::SeqCst) == 1
    });
    server.stop(); // idempotent
}

#[test]
fn closing_the_client_is_reported_to_its_handler() {
    let (server, _) = start_server(Duration::from_secs(5));
    let (client, _, handler) = connect(server.port(), TOKEN, "cli").unwrap();
    client.close();
    assert!(handler.closed.load(Ordering::SeqCst));
    client.close(); // idempotent
}

#[test]
fn an_unreachable_port_fails_with_a_readable_message() {
    // Port 1 on loopback is closed everywhere we run.
    let error = connect(1, TOKEN, "cli").err().expect("must fail");
    match error {
        LinkError::Connection(message) => {
            assert!(message.contains("could not reach 127.0.0.1:1"), "{message}")
        }
        other => panic!("unexpected {other:?}"),
    }
}
