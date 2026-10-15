//! `edgehop-core`: the core as a helper process (docs/IPC.md), newline-delimited JSON on stdin/stdout.
//! stdout belongs to the protocol; anything for humans goes to stderr. When the UI goes away stdin closes,
//! every session is stopped and the process exits.

use std::io;

use edgehop_core::ipc::{serve, Emitter};
use edgehop_core::service::CoreService;
use edgehop_core::stub::StubBackend;
use edgehop_core::VERSION;

fn main() {
    let emitter = Emitter::new(io::stdout());
    let service = CoreService::new(StubBackend::new(), emitter.clone());
    eprintln!("edgehop-core {VERSION} ready");
    serve(io::stdin().lock(), &emitter, &service);
}
