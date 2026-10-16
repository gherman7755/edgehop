#!/usr/bin/env bash
# Runs the app in smoke mode (`run-smoke.sh COMMAND [ARGS...]`, with EDGEHOP_SMOKE=1 in the environment) and passes if
# it printed "SMOKE OK", whatever its exit status: the app's exit watchdog may have had to end it (status 137 on macOS),
# and what the smoke test is about is that the app started, showed the running core and answered. If the app never ends
# (the watchdog failed), this never ends either, and the step's own timeout fails it.
log=$(mktemp)
"$@" 2>&1 | tee "$log"
if grep -q "SMOKE OK" "$log"; then
	echo "smoke test passed"
	exit 0
fi
echo "no SMOKE OK in the output" >&2
exit 1
