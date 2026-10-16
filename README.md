# EdgeHop

Share one mouse and keyboard between computers: push the cursor through a screen edge and it continues on the
other machine. A from-scratch learning project in Swift (macOS first, Windows planned).

It is an independent implementation. Its design was informed by studying how such tools behave (event
capture, injection, transport, edge switching); it shares concepts, not code or wire format, with any other app.

**Status:** prototype. The link is authenticated but **not encrypted yet** (see Milestones), so use it on a
network you trust. No license has been chosen yet (all rights reserved by default).

## Layout

| Path | What |
|---|---|
| `Sources/EdgeHopCore/InputEvent.swift` | Platform-neutral input event + fixed 16-byte wire record |
| `Sources/EdgeHopCore/Frame.swift` | `"EH"` frame header (type, flags, big-endian length) and a streaming decoder |
| `Sources/EdgeHopCore/Layout.swift` | Monitors, edges, `deriveEdges`, `EdgeDetector`, `VirtualCursor` |
| `Sources/EdgeHopCore/EventTap.swift` | Active session tap on its own thread; re-enable on disable; own-event marker |
| `Sources/EdgeHopCore/EventInjector.swift` | Replay: warp-or-relative moves, 120-unit scroll, `flagsChanged` modifiers, `releaseAll` |
| `Sources/EdgeHopCore/Recording.swift` | Text recording format: `<millis> <wire record hex>` per line |
| `Sources/edgehop-tap/` | CLI that prints (and with `--out` records) captured events; log-only, never swallows |
| `Sources/EdgeHopCore/Sessions.swift` | `ReceiverSession` / `ControllerSession`: everything the CLI and the app do, behind simple callbacks |
| `Sources/EdgeHopCore/Discovery.swift`, `TokenStore.swift` | Bonjour browse (`_edgehop._tcp`) and Keychain token storage |
| `Sources/EdgeHopApp/` | The macOS desktop app (SwiftUI window + menu bar item) |
| `scripts/package-app.sh` | Builds `dist/EdgeHop.app` (ad-hoc signed) |
| `docs/PROTOCOL.md` | Wire protocol, for other implementations (e.g. Windows) |
| `docs/IPC.md` | The local UI-to-core API (newline-delimited JSON over stdin/stdout) |
| `Sources/EdgeHopIPC/`, `Sources/edgehop-core/` | The core as a headless helper process implementing that API |
| `scripts/ipc-smoke.mjs` | End-to-end check of a core from a real (Node) client; works against the Swift and the Rust core |
| `scripts/ipc-conformance.mjs` | Sends the same requests to two cores (Swift, Rust) and requires identical responses |
| `rust/` | Rust workspace for the Windows core. `crates/edgehop-proto`: wire protocol (frames, records, handshake). `crates/edgehop-core`: the core process (IPC server, service, backend trait, stub backend) |
| `rust/crates/edgehop-link` | The Rust TCP link: token-authenticated server and client on `edgehop-proto` (std threads, no async runtime) |
| `rust/crates/edgehop-input`, `rust/crates/edgehop-win` | Input injection logic shared by every platform (`OsInput` trait, tested with a fake) and the Windows layer (displays, `SendInput`, mouse hook and raw input); see `docs/WINDOWS.md` |
| `rust/crates/edgehop-switch` | Edge switching in Rust: a port of the Swift `SwitchController`, replayed against scenarios recorded from the Swift one |
| `scripts/check-link-interop.sh` | Swift and Rust links must interoperate over real TCP, both directions |
| `scripts/golden-vectors.sh`, `scripts/check-rust-compat.sh` | Cross-language wire checks between the Swift core and the Rust crate |
| `Sources/EdgeHopCore/Link.swift` | M4 plaintext TCP link: `LinkServer`/`LinkClient`, token challenge-response, hello/ping/input frames, `PanicChord` |
| `Sources/edgehop-link/` | `token`, `serve`, `connect HOST`: mirror mouse input to another Mac |
| `scripts/deploy.sh` | Build release `edgehop-link` and `scp` it to the other Mac |
| `Sources/edgehop-inject/` | `selftest` (feedback-guard check) and `replay FILE` (replays a recording) |
| `Tests/EdgeHopCoreTests/` | Codec, framing and edge/virtual-cursor tests |

## Run

```bash
swift build
swift run edgehop-tap --seconds 20 --out /tmp/rec.txt     # record 20 s of mouse movement
swift run edgehop-inject selftest                         # automated feedback-guard check (moves the cursor ~2 s)
swift run edgehop-inject replay /tmp/rec.txt --speed 2    # replay; refuses clicks/keys/scroll without --allow-input
```
macOS will ask for **Accessibility** and **Input Monitoring** for the app that runs the command
(Terminal, iTerm, the Claude app...). `edgehop-tap` only logs; it exits on its own after `--seconds`.

Tests: with only the Command Line Tools installed, SwiftPM does not pass the Swift Testing macro
plugin to the compiler, so give it explicitly:

```bash
swift test -Xswiftc -plugin-path -Xswiftc /Library/Developer/CommandLineTools/usr/lib/swift/host/plugins/testing
```
(Not needed with full Xcode.)

## Desktop app

```bash
scripts/package-app.sh          # builds dist/EdgeHop.app
open dist/EdgeHop.app
```
Same app on both Macs: on the controlled one choose **Be controlled** and press Start (it shows a token);
on the other choose **Control another computer**, pick the Mac found nearby (or type its address), paste the
token, set which side the other Mac is on, Connect. Close the window and it keeps running from the menu bar.
The Log section has a "Verbose diagnostics" switch and a Copy button: paste it when something misbehaves.
Because the app is ad-hoc signed, re-grant Accessibility / Input Monitoring after each rebuild.
The Command Line Tools have no SwiftUI macro plugin: do not use `@State` (keep view state in `AppModel`).

## Core helper (IPC)

The core can run as a headless helper that any UI drives (planned: an Electron UI on macOS and Windows, with a
Rust core on Windows speaking the same API). See `docs/IPC.md`.

```bash
swift build --product edgehop-core
node scripts/ipc-smoke.mjs                       # spawns the helper and exercises the protocol (~10 s)
printf '{"id":1,"method":"core.hello"}\n' | .build/debug/edgehop-core
```
If the UI dies, the helper's stdin closes and it stops every session and gives the cursor back.

## Rust core (Windows)

A second implementation of the core, in Rust, so the same UI can drive Windows. `edgehop-proto` is the wire
protocol and `edgehop-core` is the process speaking `docs/IPC.md`; both are platform-independent logic, tested on
any machine, and type-check for Windows.

```bash
export PATH=/opt/homebrew/opt/rustup/bin:$PATH      # Homebrew's rustup is keg-only
cd rust && cargo test                               # unit tests + byte-compatibility with the Swift core
cargo clippy --all-targets -- -D warnings
cargo check --target x86_64-pc-windows-msvc         # type-checks for Windows from any machine
cd .. && scripts/check-rust-compat.sh               # Swift must accept everything Rust writes
scripts/golden-vectors.sh gen > rust/crates/edgehop-proto/tests/golden.json   # regenerate if the protocol changes
scripts/golden-vectors.sh scenarios > rust/crates/edgehop-switch/tests/scenarios.json   # regenerate if the Swift switching logic changes

(cd rust && cargo build -p edgehop-core)
node scripts/ipc-smoke.mjs rust/target/debug/edgehop-core --stub              # on Windows: --platform windows --loopback
node scripts/ipc-conformance.mjs .build/debug/edgehop-core rust/target/debug/edgehop-core
scripts/check-link-interop.sh                       # Rust controller <-> Swift receiver and Swift controller <-> Rust receiver
```
The golden vectors in `edgehop-proto` are produced by the Swift implementation, and the conformance script
requires the two cores to answer 43 requests (errors included) identically. The edge-switching port is checked the
same way: Swift runs 22 scripted and pseudo-random scenarios (2,420 steps) through its real `SwitchController` and
records every action, and the Rust controller must produce the identical trace.

On Windows the Rust core runs real sessions: `edgehop-core`'s `SessionBackend` joins the TCP link, the input layer
and the edge switching on top of a small `Platform` trait (`edgehop-input`), implemented for Windows by
`edgehop-win`. Its tests run a receiver and a controller against each other over real TCP with a fake operating
system, and the Windows CI job does the same through the IPC with the real hook and `SendInput`. On other platforms
the Rust core still uses the stub (starting a session returns an `internal` error); the Swift core is the macOS one.

## Two-Mac test (M4)

```bash
# On this Mac: token + copy the binary (needs ssh access to the other Mac)
export EDGEHOP_TOKEN=$(swift run -q edgehop-link token); echo "$EDGEHOP_TOKEN"
scripts/deploy.sh user@other-mac.local

# Other Mac (receiver): grant Accessibility + Input Monitoring to its Terminal first
EDGEHOP_TOKEN=<token> ~/edgehop/edgehop-link serve

# This Mac (sender)
swift run edgehop-link connect other-mac.local
```
Switching mode (M5): `swift run edgehop-link connect other-mac.local --switch --side right --seconds 60`
(`--side` = where the other Mac sits relative to this Mac's MAIN display; that edge must have no other
display next to it). Push the cursor through the edge: a "Tink" plays and the other Mac takes over; push back
through its facing edge to return ("Pop"). Redeploy first: the receiver must know `release_all`.

Move the mouse on this Mac: the other Mac's cursor should follow (relative motion), clicks and scroll too.
Panic chord on either Mac: **Ctrl + Option + Cmd + Esc**. Both sides also auto-exit (`--seconds`).
Everything is unencrypted: do it on a trusted network, with keys off.

## Milestones

1. ✅ **M1 capture:** event tap + record codec (`edgehop-tap`).
2. ✅ **M2 model:** frame codec, layout, edge detection, virtual cursor (unit-tested).
3. ✅ **M3 loopback injection:** `edgehop-inject selftest|replay` + recordings. Verified on a real Mac:
   98 injected moves, 98 recognised by the tap as ours (marker), 0 delivered to `onEvent`
   (an earlier run showed 11 extra events while the mouse was being touched; re-run was clean).
   Only moves are covered by the selftest (clicks/keys/scroll go through the same `post()` that sets the marker).
4. ✅ **M4 TCP link:** verified on two Macs (192.168.x LAN): auth, ping (7-10 ms typical, Wi-Fi spikes
   40-180 ms also seen with plain ICMP), cursor mirroring. The first two-Mac run found a real bug: the
   receiver dropped relative moves until it had seen an absolute move and posted clicks at (0,0)
   (fixed with `PointerState`, regression tests + selftest phase).
5. 🟨 **M5 switching:** implemented (`SwitchController`, 11 state-machine tests; `connect --switch --side S`).
   The cursor leaves the main display through side S, local input is swallowed while remote, only the
   *applied* delta is sent (virtual cursor clamps), it returns through the remote's facing edge with a
   `release_all` control frame. Panic chord, link loss, Ctrl+C/SIGTERM and `--seconds` always hand control
   back (and re-couple the cursor). Needs one two-Mac run to mark done. Not done yet: hiding the local cursor
   (the original uses a private CGS call), drags across the edge, multi-display edges, dead corners in the CLI.
6. 🟨 **Core IPC (UI to core):** implemented for macOS (`edgehop-core`, `docs/IPC.md`, 14 unit tests and the
   Node smoke test). Rust core for Windows: wire protocol crate (27 tests, byte-compatible with Swift in both
   directions) and the core process speaking the IPC (28 tests, stub session backend, identical to the Swift core
   on 43 conformance requests); TCP link crate (13 loopback tests, interoperates with the Swift link both ways).
   Windows input layer (injection verified on a real Windows runner); edge-switching controller ported to Rust and
   proven identical to the Swift one on recorded scenarios. Session backend joining them (`SessionBackend`, tested end to end
   over loopback with fakes) and its Windows platform adapter. Next: confirm it on two real computers, then the
   Electron UI. Still missing in the Rust core: discovery, keyboard, encryption.
7. ⬜ **M6 security:** Noise XX (X25519, ChaChaPoly, SHA256) with pinned static keys + 6-digit SAS pairing.
   Use a vetted Noise implementation or test vectors; do not hand-roll without vectors.
8. ⬜ **M7 keymap:** neutral key table (HID usage or VK) <-> macOS keycodes, modifier handling.
9. ⬜ **M8 polish:** `PeerHello` with monitor geometry, layout sync with a revision, sleep-gap detection,
   reconnect, ping/pong latency, tap re-enable stress test.

## Releases and CI

* `main` is protected: changes land through pull requests only (no direct pushes, no force pushes, also for admins).
* `.github/workflows/ci.yml` runs on every pull request: the Swift build and tests and the Swift IPC smoke test on
  macOS, the cross-language wire check and the Swift/Rust conformance check, and the Rust format, clippy and tests
  on Linux and on **Windows** (where the Rust core is also built and smoke-tested).
* Releases are DMGs for Apple Silicon Macs, published as pre-releases (latest: `v0.1.0`). To cut one: bump
  `CFBundleShortVersionString` in `Resources/Info.plist`, merge that, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
  `.github/workflows/release.yml` builds the DMG on a macOS runner and publishes it with a checksum. Running that
  workflow by hand (Actions tab) builds the DMG as a workflow artifact without publishing.

## Safety notes

* An active tap that swallows events can lock out your own input if there is a bug. Keep the log-only
  tool for experiments, add the panic chord first, and keep a second input device handy.
* Only use `releaseAll()` for keys/buttons this process pressed.
* Injected events carry `EventTap.marker` (`0x45484F50`, "EHOP"); the tap ignores them.
