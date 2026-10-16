//! `edgehop-core`: the core as a helper process (docs/IPC.md), newline-delimited JSON on stdin/stdout.
//! stdout belongs to the protocol; anything for humans goes to stderr. When the UI goes away stdin closes,
//! every session is stopped and the process exits.
//!
//! Windows runs real sessions on `edgehop-win`; other platforms have no platform layer in this core yet and use
//! the stub (the Swift core is the macOS one).

use std::io;

use edgehop_core::ipc::{serve, Emitter};
use edgehop_core::service::CoreService;
use edgehop_core::VERSION;

#[cfg(windows)]
fn backend() -> impl edgehop_core::backend::Backend {
    edgehop_core::session::SessionBackend::new(edgehop_win::WinPlatform::new())
}

#[cfg(not(windows))]
fn backend() -> impl edgehop_core::backend::Backend {
    edgehop_core::stub::StubBackend::new()
}

fn main() {
    let emitter = Emitter::new(io::stdout());
    let service = CoreService::new(backend(), emitter.clone());
    eprintln!("edgehop-core {VERSION} ready");
    serve(io::stdin().lock(), &emitter, &service);
}
