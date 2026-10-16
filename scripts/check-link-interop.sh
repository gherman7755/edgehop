#!/usr/bin/env bash
# The Swift and Rust links must interoperate over real TCP (token handshake, monitors, ping/pong, control):
#   1. a Rust controller (examples/probe) connects to the Swift receiver (edgehop-link serve)
#   2. the Swift controller (edgehop-link connect) connects to a Rust receiver (examples/serve)
# No input events are sent and nothing is injected. Needs the Swift tools built: `swift build`.
#   scripts/check-link-interop.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"

TOKEN="ABCDEFGHJKLMNPQRSTUV"
SWIFT_PORT=47997
RUST_PORT=47996
work="$(mktemp -d)"
pids=()
cleanup() { for pid in "${pids[@]:-}"; do kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; done; rm -rf "$work"; }
trap cleanup EXIT

swift build --product edgehop-link -q 2>&1 | grep -v "ld: warning" || true
cargo build -q --manifest-path rust/Cargo.toml -p edgehop-link --examples

echo "== 1. Rust controller -> Swift receiver =="
EDGEHOP_TOKEN="$TOKEN" .build/debug/edgehop-link serve --port "$SWIFT_PORT" --seconds 30 > "$work/swift-serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 50); do grep -q "Listening on port" "$work/swift-serve.log" 2>/dev/null && break; sleep 0.2; done
rust/target/debug/examples/probe 127.0.0.1 "$SWIFT_PORT" "$TOKEN" | tee "$work/probe.out"
grep -q "probe ok" "$work/probe.out"
grep -q "keymap hid" "$work/probe.out" || { echo "the Swift receiver did not announce HID key codes"; exit 1; }
sleep 0.5
grep -q "peer connected: rust-probe, keymap hid" "$work/swift-serve.log" || { echo "Swift receiver never saw the Rust peer"; cat "$work/swift-serve.log"; exit 1; }

echo "== 1b. a wrong token is rejected by the Swift receiver =="
if rust/target/debug/examples/probe 127.0.0.1 "$SWIFT_PORT" "WRONGTOKENWRONGTOKEN" 2> "$work/wrong.err"; then
  echo "the Swift receiver accepted a wrong token"; exit 1
fi
grep -q "rejected the token" "$work/wrong.err"; cat "$work/wrong.err"

echo "== 2. Swift controller -> Rust receiver =="
rust/target/debug/examples/serve "$RUST_PORT" "$TOKEN" 20 > "$work/rust-serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 50); do grep -q "listening on" "$work/rust-serve.log" 2>/dev/null && break; sleep 0.2; done
# Without Accessibility/Input Monitoring the Swift tool exits right after connecting; with them it mirrors your
# mouse into this receiver, which only prints. Either way the handshake is what is checked.
EDGEHOP_TOKEN="$TOKEN" .build/debug/edgehop-link connect 127.0.0.1 --port "$RUST_PORT" --seconds 3 > "$work/swift-connect.log" 2>&1 || true
sleep 0.5
cat "$work/swift-connect.log" | head -3
grep -q "peer connected: .*keymap hid" "$work/rust-serve.log" || { echo "Rust receiver never saw the Swift peer"; cat "$work/rust-serve.log"; exit 1; }
grep "peer connected" "$work/rust-serve.log"

echo
echo "INTEROP OK"
