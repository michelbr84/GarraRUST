#!/usr/bin/env bash
# Unit tests for the network-free pieces of `resolve_version` in `install.sh`.
#
# Context: unauthenticated api.github.com allows 60 requests/hour **per IP**.
# Cloud pods (RunPod etc.) share their egress IP across many users, so the
# installer's original API-based version lookup 429'd/403'd routinely. The
# fix resolves the tag from the `github.com/<repo>/releases/latest` redirect
# URL instead, keeping the API as fallback. These tests pin the URL → tag
# parser; the network path is exercised by the live installer.
#
# Strategy mirrors checksum_format.sh: source install.sh with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() does not run, then invoke the
# functions directly.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "resolve_version.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
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

echo "== extract_tag_from_release_url =="

# --- the canonical shape github.com redirects /releases/latest to -----------
got="$(printf '%s' "https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.0" \
    | extract_tag_from_release_url)"
assert_eq "canonical tag URL" "v0.3.0" "${got}"

# --- query string after the tag must not leak into the version --------------
got="$(printf '%s' "https://github.com/michelbr84/GarraRUST/releases/tag/v0.3.0?foo=bar" \
    | extract_tag_from_release_url)"
assert_eq "query string stripped" "v0.3.0" "${got}"

# --- prerelease-style tags survive intact ------------------------------------
got="$(printf '%s' "https://github.com/michelbr84/GarraRUST/releases/tag/v0.1.0-beta.1" \
    | extract_tag_from_release_url)"
assert_eq "prerelease tag kept whole" "v0.1.0-beta.1" "${got}"

# --- repo with no full release: redirect lands on /releases (no /tag/) ------
got="$(printf '%s' "https://github.com/michelbr84/GarraRUST/releases" \
    | extract_tag_from_release_url || true)"
assert_eq "no-tag URL -> empty (API fallback takes over)" "" "${got}"

# --- non-redirected URL (rate-limited HTML page, error body, etc.) ----------
got="$(printf '%s' "https://github.com/michelbr84/GarraRUST/releases/latest" \
    | extract_tag_from_release_url || true)"
assert_eq "unredirected /latest -> empty" "" "${got}"

echo ""
echo "== resolve_version (pin short-circuit) =="

# A pinned GARRAIA_VERSION must never touch the network. Poison curl via PATH
# so any accidental network call fails the test loudly.
work="$(mktemp -d)"
trap 'rm -rf -- "${work}"' EXIT
cat > "${work}/curl" <<'EOF'
#!/bin/sh
echo "curl must not be invoked when GARRAIA_VERSION is pinned" >&2
exit 97
EOF
chmod +x "${work}/curl"

got="$(PATH="${work}:${PATH}" GARRAIA_VERSION="v9.9.9" sh -c "
    export GARRAIA_INSTALL_SH_LIBRARY=1
    . '${install_sh}'
    resolve_version >/dev/null
    printf '%s' \"\${VERSION}\"
")"
assert_eq "pinned version bypasses network" "v9.9.9" "${got}"

echo ""
echo "resolve_version.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
