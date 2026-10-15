//! Regression test for a hang found on Windows: closing a client (or stopping a server) joined a thread that was
//! blocked in `read`, and on Windows `shutdown` from another thread does not reliably wake it. About one run in
//! three of the loopback tests hung for 15 minutes; this test reproduced it within a few dozen rounds.
//!
//! It hammers the pattern (start a server, connect, send, drop the client, drop the server) from several threads.
//! A watchdog turns a hang into a clear failure after 60 s, naming the stage each worker is stuck in.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use edgehop_link::{
    ClientConfig, ClientHandler, LinkClient, LinkServer, ServerConfig, ServerHandler,
};
use edgehop_proto::InputEvent;

const TOKEN: &str = "stress-secret-12345";
// Few rounds on purpose: the hang showed up within the first few rounds per worker on Windows, and every round
// opens a connection, so thousands of them in a few seconds exhaust the ephemeral ports (the OS then answers
// "Can't assign requested address") and make repeated local runs fail for the wrong reason.
const THREADS: usize = 4;
const ITERATIONS: usize = 25;

struct Count(AtomicUsize);

impl ServerHandler for Count {
    fn on_input(&self, _event: InputEvent) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

struct Quiet;

impl ClientHandler for Quiet {}

const STAGES: [&str; 7] = [
    "idle",
    "start server",
    "connect",
    "send",
    "wait delivery",
    "drop client",
    "drop server",
];

fn one_round(stage: &AtomicUsize) {
    stage.store(1, Ordering::SeqCst);
    let counter = Arc::new(Count(AtomicUsize::new(0)));
    let mut config = ServerConfig::new(TOKEN, "srv");
    config.port = 0;
    config.bind = "127.0.0.1".parse().unwrap();
    let server = LinkServer::start(config, counter.clone()).unwrap();

    stage.store(2, Ordering::SeqCst);
    let (client, _) = LinkClient::connect(
        "127.0.0.1",
        server.port(),
        &ClientConfig::new(TOKEN, "cli"),
        Arc::new(Quiet),
    )
    .unwrap();

    stage.store(3, Ordering::SeqCst);
    client
        .send_input(&InputEvent::MouseMoveDelta { dx: 1, dy: 1 })
        .unwrap();

    stage.store(4, Ordering::SeqCst);
    let end = Instant::now() + Duration::from_secs(5);
    while counter.0.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < end, "input never delivered");
        thread::sleep(Duration::from_millis(1));
    }

    stage.store(5, Ordering::SeqCst);
    drop(client);
    stage.store(6, Ordering::SeqCst);
    drop(server);
    stage.store(0, Ordering::SeqCst);
}

#[test]
fn closing_connections_never_hangs() {
    let stages: Arc<Vec<AtomicUsize>> =
        Arc::new((0..THREADS).map(|_| AtomicUsize::new(0)).collect());
    let rounds: Arc<Vec<AtomicUsize>> =
        Arc::new((0..THREADS).map(|_| AtomicUsize::new(0)).collect());

    let (watch_stages, watch_rounds) = (stages.clone(), rounds.clone());
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(60));
        let report: Vec<String> = (0..THREADS)
            .map(|t| {
                format!(
                    "worker {t}: round {} stuck in '{}'",
                    watch_rounds[t].load(Ordering::SeqCst),
                    STAGES[watch_stages[t].load(Ordering::SeqCst)]
                )
            })
            .collect();
        eprintln!(
            "HANG: closing a connection blocked for 60 s: {}",
            report.join(" | ")
        );
        std::process::exit(86);
    });

    let workers: Vec<_> = (0..THREADS)
        .map(|t| {
            let (stages, rounds) = (stages.clone(), rounds.clone());
            thread::spawn(move || {
                for i in 0..ITERATIONS {
                    rounds[t].store(i, Ordering::SeqCst);
                    one_round(&stages[t]);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    eprintln!(
        "{} connect/send/close rounds without a hang",
        THREADS * ITERATIONS
    );
}
