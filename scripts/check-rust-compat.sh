#!/usr/bin/env bash
# Prove the Swift core accepts what the Rust crate writes (the other direction of the golden test).
#   scripts/check-rust-compat.sh
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"

vectors="$(mktemp)"
(cd rust && cargo run -q -p edgehop-proto --example emit_vectors) > "$vectors"
scripts/golden-vectors.sh verify "$vectors"
