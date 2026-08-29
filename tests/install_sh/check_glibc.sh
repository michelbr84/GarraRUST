#!/usr/bin/env bash
# Unit tests for `check_glibc` in `install.sh`.
#
# Regression guard for the v0.3.2 install failure: binaries built on
# ubuntu-latest (24.04, glibc 2.39) aborted at runtime on older systems with
# the loader's cryptic "version `GLIBC_2.39' not found". `check_glibc` must
# fail fast with an actionable message when the local glibc is older than
# MIN_GLIBC, and stay silent when the version is new enough or undeterminable.
#
# Strategy mirrors checksum_format.sh: source install.sh with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() does not run, then invoke
# `check_glibc` in subshells where `ldd` is shadowed by a function.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "check_glibc.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
        exit 0
        ;;
esac

export GARRAIA_INSTALL_SH_LIBRARY=1
# shellcheck source=/dev/null
. "${install_sh}"

results_pass=0
results_fail=0

pass() { echo "  PASS: $1"; results_pass=$((results_pass + 1)); }
fail() { echo "  FAIL: $1" >&2; results_fail=$((results_fail + 1)); }

# run_case <label> <expected_exit> <ldd_line1> [expect_stderr_substring]
# Runs check_glibc in a subshell with `ldd` shadowed to print <ldd_line1>.
run_case() {
    local label="$1" expected_exit="$2" ldd_line="$3" want_substr="${4:-}"
    local stderr_file rc
    stderr_file="$(mktemp)"
    rc=0
    (
        OS_NAME="linux"
        ARCH="test-arch-without-musl-loader"
        eval "ldd() { printf '%s\n' \"${ldd_line}\"; }"
        check_glibc
    ) 2>"${stderr_file}" || rc=$?
    if [ "${rc}" -ne "${expected_exit}" ]; then
        fail "${label} — expected exit ${expected_exit} got ${rc}"
    elif [ -n "${want_substr}" ] && ! grep -q "${want_substr}" "${stderr_file}"; then
        fail "${label} — stderr missing [${want_substr}]: $(cat "${stderr_file}")"
    else
        pass "${label}"
    fi
    rm -f -- "${stderr_file}"
}

echo "== check_glibc =="

# --- older than MIN_GLIBC: fail fast with actionable message ----------------
run_case "glibc 2.31 rejected" 1 \
    "ldd (Ubuntu GLIBC 2.31-0ubuntu9.16) 2.31" "glibc >= ${MIN_GLIBC}"
run_case "glibc 2.17 rejected" 1 \
    "ldd (GNU libc) 2.17" "build from source"

# --- at or above MIN_GLIBC: pass silently -----------------------------------
run_case "glibc ${MIN_GLIBC} (baseline) accepted" 0 \
    "ldd (Ubuntu GLIBC 2.35-0ubuntu3.8) ${MIN_GLIBC}"
run_case "glibc 2.39 accepted" 0 \
    "ldd (Ubuntu GLIBC 2.39-0ubuntu8.4) 2.39"

# --- version undeterminable: skip the check, do not block -------------------
run_case "unparseable ldd output skipped" 0 \
    "not a libc version banner"

# --- non-Linux platforms never run the probe --------------------------------
rc=0
( OS_NAME="macos"; ARCH="x86_64"; check_glibc ) || rc=$?
if [ "${rc}" -eq 0 ]; then
    pass "macos skipped"
else
    fail "macos skipped — expected exit 0 got ${rc}"
fi

echo ""
echo "check_glibc.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
