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
| `scripts/ipc-smoke.mjs` | End-to-end check of the helper from a real (Node) client |
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
   Node smoke test). Next: Rust core for Windows against the same API, then the Electron UI.
7. ⬜ **M6 security:** Noise XX (X25519, ChaChaPoly, SHA256) with pinned static keys + 6-digit SAS pairing.
   Use a vetted Noise implementation or test vectors; do not hand-roll without vectors.
8. ⬜ **M7 keymap:** neutral key table (HID usage or VK) <-> macOS keycodes, modifier handling.
9. ⬜ **M8 polish:** `PeerHello` with monitor geometry, layout sync with a revision, sleep-gap detection,
   reconnect, ping/pong latency, tap re-enable stress test.

## Safety notes

* An active tap that swallows events can lock out your own input if there is a bug. Keep the log-only
  tool for experiments, add the panic chord first, and keep a second input device handy.
* Only use `releaseAll()` for keys/buttons this process pressed.
* Injected events carry `EventTap.marker` (`0x45484F50`, "EHOP"); the tap ignores them.
