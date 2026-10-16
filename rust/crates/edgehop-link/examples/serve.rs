//! A minimal receiver for interop checks: authenticates controllers (Swift or Rust) and prints what arrives.
//! It injects nothing.
//!
//!     serve PORT TOKEN SECONDS

use std::io::Write;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;

use edgehop_link::{LinkServer, ServerConfig, ServerHandler};
use edgehop_proto::{ControlMessage, HelloMessage, InputEvent, MonitorInfo};

fn say(line: &str) {
    println!("{line}");
    let _ = std::io::stdout().flush();
}

struct Printer;

impl ServerHandler for Printer {
    fn on_peer_connected(&self, hello: &HelloMessage) {
        let monitors = hello.monitors.as_ref().map_or(0, Vec::len);
        say(&format!(
            "peer connected: {} ({monitors} monitor(s)), keymap {}",
            hello.name,
            hello.keymap.as_deref().unwrap_or("none")
        ));
    }
    fn on_input(&self, event: InputEvent) {
        say(&format!("input {event:?}"));
    }
    fn on_control(&self, message: ControlMessage) {
        say(&format!("control {message:?}"));
    }
    fn on_peer_disconnected(&self) {
        say("peer disconnected");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, port, token, seconds] = args.as_slice() else {
        eprintln!("usage: serve PORT TOKEN SECONDS");
        std::process::exit(2);
    };
    let mut config = ServerConfig::new(token.as_str(), "rust-serve");
    config.port = port.parse().expect("PORT must be a number");
    config.monitors = vec![MonitorInfo {
        id: "serve".into(),
        x: 0.0,
        y: 0.0,
        width: 1280.0,
        height: 800.0,
    }];
    let server = match LinkServer::start(config, Arc::new(Printer)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("serve: cannot listen: {error}");
            std::process::exit(1);
        }
    };
    say(&format!("listening on {}", server.port()));
    sleep(Duration::from_secs(
        seconds.parse().expect("SECONDS must be a number"),
    ));
    server.stop();
}
