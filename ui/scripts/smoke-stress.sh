#!/usr/bin/env bash
# Starts the packaged app in smoke mode many times, a few at a time, and reports every run that does not end by
# itself within 30 s (a hang at exit once showed up in about one run in a hundred, only under load).
#   cd ui && npm run pack && scripts/smoke-stress.sh [RUNS] [PARALLEL]      (macOS)
set -uo pipefail
cd "$(dirname "$0")/.."
RUNS=${1:-100}; PARALLEL=${2:-6}
APP=release/mac-arm64/EdgeHop.app/Contents/MacOS/EdgeHop
[ -x "$APP" ] || { echo "no packaged app at $APP: run npm run pack first" >&2; exit 2; }
export APP
one() {
  id=$1; ud=$(mktemp -d); log=$(mktemp)
  EDGEHOP_SMOKE=1 EDGEHOP_USER_DATA=$ud "$APP" > "$log" 2>&1 &
  pid=$!; waited=0
  while kill -0 $pid 2>/dev/null && [ $waited -lt 60 ]; do sleep 0.5; waited=$((waited+1)); done
  if kill -0 $pid 2>/dev/null; then echo "run $id: HUNG"; cat "$log"; kill -9 $pid 2>/dev/null; else grep -q "SMOKE OK" "$log" || { echo "run $id: no SMOKE OK"; cat "$log"; }; fi
  wait $pid 2>/dev/null; rm -rf "$ud" "$log"
}
export -f one
output=$(seq 1 "$RUNS" | xargs -P "$PARALLEL" -I{} bash -c 'one {}')
if [ -n "$output" ]; then echo "$output"; echo "FAILED"; exit 1; fi
echo "all $RUNS runs ended by themselves"
