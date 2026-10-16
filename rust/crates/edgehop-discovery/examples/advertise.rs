//! Announces a receiver for interop checks (the Swift side browses for it): `advertise NAME PORT SECONDS`.

use std::io::Write;
use std::thread::sleep;
use std::time::Duration;

use edgehop_discovery::MdnsDiscovery;
use edgehop_input::Discovery;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, name, port, seconds] = args.as_slice() else {
        eprintln!("usage: advertise NAME PORT SECONDS");
        std::process::exit(2);
    };
    let port: u16 = port.parse().expect("PORT must be a number");
    let seconds: u64 = seconds.parse().expect("SECONDS must be a number");
    let discovery = MdnsDiscovery::new();
    let _announcement = discovery.advertise(name, port).unwrap_or_else(|e| {
        eprintln!("advertise: {e}");
        std::process::exit(1);
    });
    println!("advertising {name:?} on port {port}");
    let _ = std::io::stdout().flush();
    sleep(Duration::from_secs(seconds));
}
