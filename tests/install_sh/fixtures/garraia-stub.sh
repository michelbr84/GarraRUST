#!/bin/sh
# Plan 0127 §M1.3 fixture — stands in for the real `garraia` binary
# during `tests/install_sh/bootstrap_phase.sh` runs.
#
# Behavior: append `<subcommand> <args...>` to ${GARRAIA_STUB_LOG} and
# exit 0. The test asserts on the log contents to verify that
# `bootstrap_phase` invoked the right subcommands in the right order.
#
# When `GARRAIA_STUB_FAIL_INIT=1` is set, the stub exits non-zero on
# the `init` subcommand so the test can exercise the
# "wizard exited non-zero" fall-through branch.
set -eu

: "${GARRAIA_STUB_LOG:?GARRAIA_STUB_LOG must be set by the test runner}"

sub="${1:-no-subcommand}"
shift || true
# Format: `<sub> <space-joined-args>`. One line per invocation.
printf '%s' "${sub}" >>"${GARRAIA_STUB_LOG}"
for a in "$@"; do
    printf ' %s' "${a}" >>"${GARRAIA_STUB_LOG}"
done
printf '\n' >>"${GARRAIA_STUB_LOG}"

if [ "${sub}" = "init" ] && [ "${GARRAIA_STUB_FAIL_INIT:-}" = "1" ]; then
    exit 23
fi
exit 0
