#!/usr/bin/env bash
# Contract tests for the install URLs documented inside `install.sh`.
#
# Regression guard for the garraia.org Windows-installer outage: the site
# served `/install.sh` but had no route for `/install.ps1`, so the SPA
# fallback answered the PowerShell one-liner with HTML and HTTP 200. The
# endpoint fix lives in the website repo, but the canonical URL and the
# mirror list are documented HERE, and the two must not drift apart.
#
# These assertions are deliberately about *text in the script*, not about
# the network: CI must stay hermetic. The live-endpoint probe lives in
# `.github/workflows/install-endpoints.yml`, which is scheduled, not a gate.
#
# Mirrors tests/install_ps1/documented_urls.ps1 one-for-one (CLAUDE.md §16:
# install.sh and install.ps1 stay at behavioral parity, and so do their
# suites).
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"
install_ps1="${repo_root}/install.ps1"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "documented_urls.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
        exit 0
        ;;
esac

results_pass=0
results_fail=0

pass() {
    echo "  PASS: $1"
    results_pass=$((results_pass + 1))
}

fail() {
    echo "  FAIL: $1" >&2
    results_fail=$((results_fail + 1))
}

assert_contains() {
    label="$1"
    haystack="$2"
    needle="$3"
    case "${haystack}" in
        *"${needle}"*) pass "${label}" ;;
        *) fail "${label} — não encontrou: ${needle}" ;;
    esac
}

assert_not_contains() {
    label="$1"
    haystack="$2"
    needle="$3"
    case "${haystack}" in
        *"${needle}"*) fail "${label} — encontrou o proibido: ${needle}" ;;
        *) pass "${label}" ;;
    esac
}

sh_body="$(cat "${install_sh}")"
ps1_body="$(cat "${install_ps1}")"

echo "documented_urls.sh"

# --- canonical one-liners ---------------------------------------------------
assert_contains "install.sh documenta a URL canônica garraia.org" \
    "${sh_body}" "https://garraia.org/install.sh"
assert_contains "install.ps1 documenta a URL canônica garraia.org" \
    "${ps1_body}" "https://garraia.org/install.ps1"

# --- mirrors stay listed ----------------------------------------------------
for mirror in \
    "https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh" \
    "https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh" \
    "https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh"
do
    assert_contains "install.sh lista o espelho ${mirror##*/} (${mirror%%/install.sh})" \
        "${sh_body}" "${mirror}"
done

for mirror in \
    "https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1" \
    "https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1" \
    "https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.ps1"
do
    assert_contains "install.ps1 lista o espelho ${mirror%%/install.ps1}" \
        "${ps1_body}" "${mirror}"
done

# --- the Windows/Unix invocation split --------------------------------------
# `curl -fsSL` no PowerShell resolve para `Invoke-WebRequest`, que não aceita
# essas flags. install.ps1 nunca pode ensinar essa forma como comando Windows.
ps1_commands="$(grep -n 'curl -fsSL' "${install_ps1}" || true)"
if [ -z "${ps1_commands}" ]; then
    pass "install.ps1 não contém nenhuma linha 'curl -fsSL'"
else
    # A única menção tolerada é a que explica o análogo shell, em comentário.
    offending="$(printf '%s\n' "${ps1_commands}" | grep -v 'análogo\|analogue\|analogo' || true)"
    if [ -z "${offending}" ]; then
        pass "install.ps1 só cita 'curl -fsSL' como análogo em comentário"
    else
        fail "install.ps1 ensina 'curl -fsSL' como comando Windows: ${offending}"
    fi
fi

assert_contains "install.ps1 documenta a forma irm | iex" \
    "${ps1_body}" "irm https://garraia.org/install.ps1 | iex"
assert_not_contains "install.sh não ensina irm/iex" \
    "${sh_body}" "| iex"

# --- the http/https scheme is never downgraded ------------------------------
assert_not_contains "install.sh não usa http:// em nenhuma URL de download" \
    "${sh_body}" "http://raw.githubusercontent.com"
assert_not_contains "install.ps1 não usa http:// em nenhuma URL de download" \
    "${ps1_body}" "http://raw.githubusercontent.com"

echo "documented_urls.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
