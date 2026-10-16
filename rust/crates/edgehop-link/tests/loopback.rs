//! Real TCP on 127.0.0.1 (random ports): server and client against each other and against hand-written raw
//! peers that misbehave.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use edgehop_link::{
    ClientConfig, ClientHandler, LinkClient, LinkError, LinkServer, ServerConfig, ServerHandler,
};
use edgehop_proto::{
    encode_frame, proof, ControlMessage, Frame, FrameDecoder, FrameType, HelloMessage, InputEvent,
    Modifiers, MonitorInfo, MouseButton,
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

/// Reads frames from a raw socket until one is complete.
fn read_frame(stream: &mut TcpStream, decoder: &mut FrameDecoder) -> Option<Frame> {
    let mut buffer = [0u8; 4096];
    loop {
        let n = stream.read(&mut buffer).ok()?;
        if n == 0 {
            return None;
        }
        if let Some(frame) = decoder.feed(&buffer[..n]).unwrap().into_iter().next() {
            return Some(frame);
        }
    }
}

/// Authenticates by hand and returns the raw, authenticated socket.
fn raw_authenticated(port: u16) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut decoder = FrameDecoder::new();
    let challenge = read_frame(&mut stream, &mut decoder).expect("challenge");
    let nonce = STANDARD
        .decode(HelloMessage::from_frame(&challenge).unwrap().nonce.unwrap())
        .unwrap();
    let mut answer = HelloMessage::new("raw");
    answer.proof = Some(STANDARD.encode(proof(TOKEN, &nonce)));
    stream
        .write_all(&encode_frame(&answer.to_frame()).unwrap())
        .unwrap();
    let verdict = read_frame(&mut stream, &mut decoder).expect("verdict");
    assert_eq!(
        HelloMessage::from_frame(&verdict).unwrap().accepted,
        Some(true)
    );
    stream
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
fn drops_an_unauthenticated_peer_after_the_timeout() {
    let (server, recorder) = start_server(Duration::from_millis(300));
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut decoder = FrameDecoder::new();
    assert!(
        read_frame(&mut stream, &mut decoder).is_some(),
        "the challenge comes first"
    );
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
fn drops_a_first_frame_that_is_not_a_hello() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut stream = TcpStream::connect(("127.0.0.1", server.port())).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut decoder = FrameDecoder::new();
    read_frame(&mut stream, &mut decoder).expect("challenge");
    stream
        .write_all(&encode_frame(&Frame::new(FrameType::Ping, vec![0; 8])).unwrap())
        .unwrap();
    let mut buffer = [0u8; 256];
    // The server answers with a refusal and closes: either way we reach end of stream.
    let mut closed = false;
    for _ in 0..4 {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => {
                closed = true;
                break;
            }
            Ok(_) => {}
        }
    }
    assert!(closed);
    assert!(recorder.connected.lock().unwrap().is_empty());
}

#[test]
fn drops_a_peer_that_breaks_the_framing_and_then_serves_the_next_one() {
    let (server, recorder) = start_server(Duration::from_secs(5));
    let mut stream = raw_authenticated(server.port());
    wait_until("raw peer connected", || {
        recorder.connected.lock().unwrap().len() == 1
    });
    stream
        .write_all(&[0xde, 0xad, 0xbe, 0xef, 0, 0, 0, 0, 0, 0])
        .unwrap(); // bad magic
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
    let mut stream = raw_authenticated(server.port());
    stream
        .write_all(&encode_frame(&Frame::new(FrameType::Input, vec![99; 16])).unwrap())
        .unwrap(); // unknown kind
    let good = InputEvent::MouseMoveDelta { dx: 5, dy: 6 };
    stream
        .write_all(&encode_frame(&Frame::new(FrameType::Input, good.encode().to_vec())).unwrap())
        .unwrap();
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
