#!/usr/bin/env bash
# Build the Swift cross-language tool (the real EdgeHopCore sources + scripts/golden/main.swift) and either
# regenerate the golden vectors the Rust crate tests against, or verify a file of Rust vectors.
#   scripts/golden-vectors.sh gen > rust/crates/edgehop-proto/tests/golden.json
#   scripts/golden-vectors.sh verify rust-vectors.json
set -euo pipefail
cd "$(dirname "$0")/.."

tool="$(mktemp -d)/golden"
swiftc -O -module-name Golden -o "$tool" Sources/EdgeHopCore/*.swift scripts/golden/main.swift 2>/dev/null
"$tool" "$@"
