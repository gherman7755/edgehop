//! A minimal controller for interop checks: connects to a receiver (Swift or Rust), authenticates, prints what
//! the receiver says, pings it a few times and sends `release_all`. It never sends input events.
//!
//!     probe HOST PORT TOKEN

use std::sync::{Arc, Mutex};
use std::thread::sleep;
use std::time::Duration;

use edgehop_link::{ClientConfig, ClientHandler, LinkClient};
use edgehop_proto::{ControlMessage, MonitorInfo};

#[derive(Default)]
struct Pongs(Mutex<Vec<f64>>);

impl ClientHandler for Pongs {
    fn on_pong(&self, rtt_millis: f64) {
        self.0.lock().unwrap().push(rtt_millis);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, host, port, token] = args.as_slice() else {
        eprintln!("usage: probe HOST PORT TOKEN");
        std::process::exit(2);
    };
    let port: u16 = port.parse().expect("PORT must be a number");

    let mut config = ClientConfig::new(token.as_str(), "rust-probe");
    config.monitors = vec![MonitorInfo {
        id: "probe".into(),
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1080.0,
    }];
    let pongs = Arc::new(Pongs::default());
    let (client, hello) = match LinkClient::connect(host, port, &config, pongs.clone()) {
        Ok(connected) => connected,
        Err(error) => {
            eprintln!("probe: connection failed: {error}");
            std::process::exit(1);
        }
    };
    let monitors = hello.monitors.as_ref().map_or(0, Vec::len);
    println!("connected to {:?} ({monitors} monitor(s))", hello.name);

    for _ in 0..3 {
        client.send_ping().expect("ping");
        sleep(Duration::from_millis(100));
    }
    client
        .send_control(ControlMessage::ReleaseAll)
        .expect("control");
    sleep(Duration::from_millis(200));

    let rtts = pongs.0.lock().unwrap().clone();
    println!("pongs: {}", rtts.len());
    client.close();
    if rtts.len() != 3 {
        eprintln!("probe: expected 3 pongs, got {}", rtts.len());
        std::process::exit(1);
    }
    println!("probe ok");
}
