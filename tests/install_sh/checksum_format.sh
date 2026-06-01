#!/usr/bin/env bash
# Unit tests for `select_checksum_line` in `install.sh`.
#
# Regression guard for the v0.2.1 install failure: the published
# SHA256SUMS used GNU *binary mode* (`<hash> *<name>`, one space + asterisk)
# while the installer only matched *text mode* (`<hash>  <name>`, two
# spaces). The two-space-only grep matched nothing, piped an empty stream
# into `sha256sum -c`, and aborted with "Checksum verification failed".
#
# Strategy mirrors bootstrap_phase.sh: source install.sh with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() does not run, then invoke
# `select_checksum_line` directly against fixture SHA256SUMS bodies.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "checksum_format.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
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

# assert_eq <label> <expected> <actual>
assert_eq() {
    local label="$1" expected="$2" actual="$3"
    if [ "${expected}" = "${actual}" ]; then
        pass "${label}"
    else
        fail "${label} — expected [${expected}] got [${actual}]"
    fi
}

work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT

ARTIFACT="garraia-linux-x86_64"
HASH="ff848350f7979a3eabf2dc2a992ca0ea0c507107ceef12eeef5175002844a920"

echo "== select_checksum_line =="

# --- text mode: "<hash>  <name>" (two spaces) -------------------------------
printf '%s  %s\n' "${HASH}" "${ARTIFACT}" > "${work}/text.sums"
printf '%s  %s\n' "deadbeef" "garraia-macos-x86_64" >> "${work}/text.sums"
got="$(select_checksum_line "${ARTIFACT}" "${work}/text.sums" || true)"
assert_eq "text mode matches" "${HASH}  ${ARTIFACT}" "${got}"

# --- binary mode: "<hash> *<name>" (one space + asterisk) — the v0.2.1 case --
printf '%s *%s\n' "${HASH}" "${ARTIFACT}" > "${work}/binary.sums"
printf '%s *%s\n' "deadbeef" "garraia-macos-aarch64" >> "${work}/binary.sums"
got="$(select_checksum_line "${ARTIFACT}" "${work}/binary.sums" || true)"
assert_eq "binary mode matches" "${HASH} *${ARTIFACT}" "${got}"

# --- binary mode with CR line endings (Windows-generated) -------------------
printf '%s *%s\r\n' "${HASH}" "${ARTIFACT}" > "${work}/crlf.sums"
got="$(select_checksum_line "${ARTIFACT}" "${work}/crlf.sums" || true)"
assert_eq "CRLF stripped" "${HASH} *${ARTIFACT}" "${got}"

# --- no false match against a longer sibling name ---------------------------
printf '%s *%s.sha256\n' "${HASH}" "${ARTIFACT}" > "${work}/sibling.sums"
got="$(select_checksum_line "${ARTIFACT}" "${work}/sibling.sums" || true)"
assert_eq "anchored to EOL (no .sha256 match)" "" "${got}"

# --- absent artifact yields empty -------------------------------------------
printf '%s *garraia-windows-x86_64.exe\n' "${HASH}" > "${work}/absent.sums"
got="$(select_checksum_line "${ARTIFACT}" "${work}/absent.sums" || true)"
assert_eq "absent artifact -> empty" "" "${got}"

echo ""
echo "checksum_format.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
