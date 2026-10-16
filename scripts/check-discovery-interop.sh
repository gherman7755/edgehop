#!/usr/bin/env bash
# Discovery must work between the Swift (Bonjour) and the Rust (mDNS) cores on a real network interface:
#   1. a Rust receiver announces itself; the Swift side finds it and connects to it by name (SRV + A records)
#   2. a Swift receiver announces itself; the Rust side finds it and connects to the address it announced
# Needs multicast on this machine, so it is not part of CI. Needs the Swift tool built: `swift build`.
#   scripts/check-discovery-interop.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"

TOKEN="ABCDEFGHJKLMNPQRSTUV"
work="$(mktemp -d)"
pids=()
cleanup() { for pid in "${pids[@]:-}"; do kill "$pid" 2>/dev/null || true; wait "$pid" 2>/dev/null || true; done; rm -rf "$work"; }
trap cleanup EXIT

swift build --product edgehop-link -q 2>&1 | grep -v "ld: warning" || true
cargo build -q --manifest-path rust/Cargo.toml -p edgehop-discovery -p edgehop-link --examples

echo "== 1. Rust announces, Swift finds it and connects by name =="
# A Rust receiver (examples/serve) is the thing to connect to; the announcement is made by examples/advertise on its port.
rust/target/debug/examples/serve 47994 "$TOKEN" 25 > "$work/rust-serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 50); do grep -q "listening on" "$work/rust-serve.log" 2>/dev/null && break; sleep 0.2; done
rust/target/debug/examples/advertise rust-disco-test 47994 25 > "$work/advertise.log" 2>&1 &
pids+=($!)
EDGEHOP_TOKEN="$TOKEN" .build/debug/edgehop-link browse --seconds 15 --connect rust-disco-test | tee "$work/swift-browse.out"
grep -q 'connected to "rust-serve"' "$work/swift-browse.out" || { echo "Swift never connected to the Rust receiver by name"; cat "$work/rust-serve.log"; exit 1; }

echo "== 2. Swift announces, Rust finds it and connects =="
EDGEHOP_TOKEN="$TOKEN" .build/debug/edgehop-link serve --port 47993 --seconds 25 > "$work/swift-serve.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 50); do grep -q "Listening on port\|listening on port" "$work/swift-serve.log" 2>/dev/null && break; sleep 0.2; done
name="$(scutil --get ComputerName 2>/dev/null || hostname)"
rust/target/debug/examples/browse 15 --connect "$name" "$TOKEN" | tee "$work/rust-browse.out"
grep -q "connected to" "$work/rust-browse.out" || { echo "Rust never connected to the Swift receiver"; cat "$work/swift-serve.log"; exit 1; }

echo
echo "DISCOVERY INTEROP OK"
