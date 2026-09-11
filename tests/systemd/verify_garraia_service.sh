#!/usr/bin/env bash
set -euo pipefail

# Regression guard for #1147: StartLimitIntervalSec/StartLimitBurst only take
# effect in the [Unit] section (systemd.unit(5)) — systemd silently ignores
# them in [Service], which is exactly how the gateway's crash-loop circuit
# breaker went missing without anyone noticing. `systemd-analyze verify`
# prints "Unknown key name '<key>' in section '<section>', ignoring." for any
# misplaced key; this test fails the build if that ever shows up again.
#
# The unit's ExecStart binary is never installed in CI, so `verify` always
# reports it missing — that specific complaint is expected and ignored here.
# Any "ignoring" line, though, means a directive landed in the wrong section.

UNIT_FILE="deploy/systemd/garraia.service"

if ! command -v systemd-analyze >/dev/null 2>&1; then
    echo "SKIP: systemd-analyze not available in this environment" >&2
    exit 0
fi

output="$(systemd-analyze verify "$UNIT_FILE" 2>&1 || true)"

if echo "$output" | grep -qi "ignoring"; then
    echo "FAIL: systemd-analyze reported an ignored/misplaced key in $UNIT_FILE:" >&2
    echo "$output" >&2
    exit 1
fi

echo "OK: no ignored keys in $UNIT_FILE"
