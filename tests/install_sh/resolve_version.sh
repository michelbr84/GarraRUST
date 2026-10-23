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
echo "== extract_first_stable_tag (last-resort fallback skips prereleases) =="

# The installer's third tier parses `GET /releases` and must pick the newest
# STABLE release: never a draft, never a prerelease. The function reads the raw
# JSON (no `tr ',' '\n'`, which would split commas inside string values) and is
# FIELD-ORDER-INDEPENDENT: a release is classified by all three of its
# tag_name/draft/prerelease in whatever order they serialize. The inputs below
# mirror the real payload shape — asset objects with their own `"id"`/`"name"`
# noise — and deliberately include reordered and brace-laden cases.

# stable_tag <json>  → feeds the raw JSON straight to the installer function
stable_tag() { printf '%s' "$1" | extract_first_stable_tag || true; }

# --- prerelease on top, stable right below → the stable wins -----------------
json_rc_then_stable='[{"url":"https://api.github.com/repos/x/y/releases/1","id":111,"author":{"login":"michelbr84","id":999},"node_id":"RE_1","tag_name":"v0.4.10-rc1","name":"Release v0.4.10-rc1","draft":false,"prerelease":true,"assets":[{"id":501,"name":"garraia-linux-x86_64","state":"uploaded"},{"id":502,"name":"garraia-linux-x86_64.sha256"}]},{"url":"https://api.github.com/repos/x/y/releases/2","id":222,"author":{"login":"michelbr84","id":999},"node_id":"RE_2","tag_name":"v0.4.9","name":"Release v0.4.9","draft":false,"prerelease":false,"assets":[{"id":601,"name":"garraia-linux-x86_64"}]},{"url":"https://api.github.com/repos/x/y/releases/3","id":333,"tag_name":"v0.4.8","draft":false,"prerelease":false,"assets":[]}]'
assert_eq "prerelease on top is skipped, picks newest stable" "v0.4.9" "$(stable_tag "${json_rc_then_stable}")"

# --- the GitHub web pretty-print (spaces after colons) -----------------------
json_spaced='[{"id": 111, "tag_name": "v0.5.0-beta.2", "draft": false, "prerelease": true, "assets": [{"id": 7, "name": "garraia-linux-x86_64"}]}, {"id": 222, "tag_name": "v0.4.9", "draft": false, "prerelease": false, "assets": []}]'
assert_eq "whitespace-tolerant: beta skipped, stable chosen" "v0.4.9" "$(stable_tag "${json_spaced}")"

# --- FIELD ORDER REVERSED: prerelease/draft serialized BEFORE tag_name -------
# The whole point of the brace-depth scanner: if GitHub ever moves the flags
# ahead of tag_name, a prerelease must still be skipped (a "decide on next
# tag_name" scanner would leak it).
json_reversed='[{"url":"u1","prerelease":true,"draft":false,"id":111,"author":{"login":"m","id":999},"name":"R","tag_name":"v0.4.10-rc1","assets":[{"id":501,"name":"a"}]},{"url":"u2","prerelease":false,"draft":false,"id":222,"name":"R2","tag_name":"v0.4.9","assets":[]}]'
assert_eq "reversed order (flags before tag_name), compact" "v0.4.9" "$(stable_tag "${json_reversed}")"

# --- same reordering, but indented the way the live API serves it ------------
json_reversed_indented="$(cat <<'JSON'
[
  {
    "url": "u1",
    "prerelease": true,
    "draft": false,
    "author": { "login": "m", "id": 999 },
    "name": "Release rc1",
    "tag_name": "v0.4.10-rc1",
    "assets": [ { "id": 1, "name": "garraia-linux-x86_64" } ]
  },
  {
    "url": "u2",
    "prerelease": false,
    "draft": false,
    "name": "Release 0.4.9",
    "tag_name": "v0.4.9",
    "assets": [ { "id": 2, "name": "garraia-linux-x86_64" } ]
  }
]
JSON
)"
assert_eq "reversed order (flags before tag_name), indented" "v0.4.9" "$(stable_tag "${json_reversed_indented}")"

# --- a release body with braces, escaped quotes and the literal text
#     "prerelease": true must NOT fool the scanner (braces are inside a string;
#     the escaped quotes mean the key-like text never matches the real key).
# The ${{ }} below is literal on purpose: the body must stay unexpanded.
# shellcheck disable=SC2016
json_tricky_body='[{"url":"u1","tag_name":"v0.4.10-rc1","draft":false,"prerelease":true,"assets":[{"id":1,"name":"a"}]},{"url":"u2","tag_name":"v0.4.9","draft":false,"prerelease":false,"body":"uses ${{ matrix.os }} and a closing } brace and \"prerelease\": true as text","assets":[{"id":2,"name":"b"}]}]'
assert_eq "body braces + escaped quotes do not corrupt boundaries" "v0.4.9" "$(stable_tag "${json_tricky_body}")"

# --- stable already on top → returned as-is ----------------------------------
json_stable_top='[{"id":10,"tag_name":"v0.4.9","draft":false,"prerelease":false,"assets":[{"id":1,"name":"garraia-linux-x86_64"}]},{"id":20,"tag_name":"v0.4.8","draft":false,"prerelease":false,"assets":[]}]'
assert_eq "stable on top is returned" "v0.4.9" "$(stable_tag "${json_stable_top}")"

# --- a draft on top (authenticated reuse), draft before tag_name too ---------
json_draft_then_stable='[{"url":"u1","draft":true,"prerelease":false,"tag_name":"v0.5.0-wip","assets":[]},{"url":"u2","draft":false,"prerelease":false,"tag_name":"v0.4.9","assets":[]}]'
assert_eq "draft on top is skipped" "v0.4.9" "$(stable_tag "${json_draft_then_stable}")"

# --- only prereleases exist → empty, so resolve_version errors loudly ---------
json_only_pre='[{"id":10,"tag_name":"v0.5.0-rc1","draft":false,"prerelease":true,"assets":[]},{"id":20,"tag_name":"v0.5.0-rc2","draft":false,"prerelease":true,"assets":[]}]'
assert_eq "all-prerelease list yields no stable tag" "" "$(stable_tag "${json_only_pre}")"

# --- a single stable release (closes at the array end) -----------------------
json_single='[{"id":10,"tag_name":"v0.4.9","draft":false,"prerelease":false,"assets":[{"id":1,"name":"garraia-linux-x86_64"}]}]'
assert_eq "single stable release" "v0.4.9" "$(stable_tag "${json_single}")"

echo ""
echo "resolve_version.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
