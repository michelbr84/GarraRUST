#!/usr/bin/env bash
# Issue #1328 — unit tests for `install_garra_alias` in `install.sh`.
#
# Strategy mirrors bootstrap_phase.sh: source `install.sh` with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() never runs, point INSTALL_DIR at a
# sandbox that holds the garraia-stub.sh fixture as `garraia`, call the
# function, and assert on the filesystem plus stdout/stderr. `sudo` is
# replaced by a logging function so the privilege branch is observable
# without ever touching /usr/local/bin.
#
# Mirrored one-for-one by tests/install_ps1/garra_alias.ps1 (CLAUDE.md 16):
# the same cases, with the garra.cmd shim standing in for the symlink.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"
stub="${repo_root}/tests/install_sh/fixtures/garraia-stub.sh"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "garra_alias.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
        exit 0
        ;;
esac

# ---- assertion helpers -------------------------------------------------------

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

skip() {
    echo "  SKIP: $1"
}

assert_contains() {
    local label="$1" needle="$2" file="$3"
    if [ -f "${file}" ] && grep -qF -- "${needle}" "${file}"; then
        pass "${label}"
    else
        fail "${label} — expected '${needle}' in ${file}; got:"
        sed 's/^/    /' "${file}" >&2 2>/dev/null || true
    fi
}

assert_absent() {
    local label="$1" needle="$2" file="$3"
    if [ -f "${file}" ] && grep -qF -- "${needle}" "${file}"; then
        fail "${label} — '${needle}' should NOT appear in ${file}; got:"
        sed 's/^/    /' "${file}" >&2 || true
    else
        pass "${label}"
    fi
}

assert_symlink_to_garraia() {
    local label="$1" link="$2"
    if [ -L "${link}" ] && [ "$(readlink "${link}")" = "garraia" ]; then
        pass "${label}"
    else
        fail "${label} — expected a symlink to 'garraia', got: $(ls -ld "${link}" 2>&1)"
    fi
}

# ---- fixtures ----------------------------------------------------------------

# A fresh sandbox whose bin/ holds the stub as `garraia`. Prints the sandbox.
new_sandbox() {
    local dir
    dir="$(mktemp -d)"
    mkdir -p "${dir}/bin"
    cp "${stub}" "${dir}/bin/garraia"
    chmod +x "${dir}/bin/garraia"
    printf '%s\n' "${dir}"
}

# Source install.sh in library mode and run one function against an explicit
# INSTALL_DIR. stdout -> <sandbox>/out.log, stderr -> <sandbox>/err.log, the
# return code -> <sandbox>/rc, and every `sudo` call -> <sandbox>/sudo.log
# (the stub records the command line and does NOT execute it).
#
#   run_in_library <sandbox> <install_dir> <function> [env assignments...]
run_in_library() {
    local sandbox="$1" install_dir="$2" fn="$3"
    shift 3
    (
        set +e
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        # Invoked indirectly by install_garra_alias / install_binary when
        # install_needs_sudo is true; records the command line, runs nothing.
        # shellcheck disable=SC2329
        sudo() { printf 'sudo %s\n' "$*" >>"${sandbox}/sudo.log"; }
        INSTALL_DIR="${install_dir}"
        # Read by the sourced functions (warning text, wrappers), not here.
        # shellcheck disable=SC2034
        INSTALL_PATH="${INSTALL_DIR}/garraia"
        for kv in "$@"; do
            # Caller-supplied VAR=value pairs for OS_NAME, PREFIX, etc.
            eval "${kv}"
        done
        "${fn}" >"${sandbox}/out.log" 2>"${sandbox}/err.log"
        echo "$?" >"${sandbox}/rc"
    )
}

assert_rc_zero() {
    local label="$1" sandbox="$2"
    if [ "$(cat "${sandbox}/rc")" = "0" ]; then
        pass "${label}"
    else
        fail "${label} — rc=$(cat "${sandbox}/rc")"
    fi
}

# ---- case (a): nothing there -> relative link, no sudo, no warning ------------
case_a_creates_link() {
    echo ""
    echo "== case (a) no garra -> creates a relative symlink =="
    local dir
    dir="$(new_sandbox)"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias

    assert_rc_zero "case (a): returns 0" "${dir}"
    assert_symlink_to_garraia "case (a): bin/garra -> garraia (relative)" "${dir}/bin/garra"
    assert_contains "case (a): reports the alias" "Alias ${dir}/bin/garra -> garraia" "${dir}/out.log"
    assert_absent "case (a): no warning" "warning:" "${dir}/err.log"
    if [ -e "${dir}/sudo.log" ]; then
        fail "case (a): sudo must not be used outside /usr/local/bin — got: $(cat "${dir}/sudo.log")"
    else
        pass "case (a): sudo not used for a user-writable directory"
    fi

    # `garra --version` through the link runs the binary the link points at.
    GARRAIA_STUB_LOG="${dir}/stub.log" "${dir}/bin/garra" --version
    assert_contains "case (a): garra --version executes garraia" "--version" "${dir}/stub.log"
}

# ---- case (b): stale symlink -> repointed --------------------------------------
case_b_repoints_stale_link() {
    echo ""
    echo "== case (b) stale symlink -> repointed at garraia =="
    local dir
    dir="$(new_sandbox)"
    ln -s /nonexistent/old-layout/garraia "${dir}/bin/garra"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias

    assert_rc_zero "case (b): returns 0" "${dir}"
    assert_symlink_to_garraia "case (b): dangling link repointed" "${dir}/bin/garra"
    assert_contains "case (b): says it repointed" "Repointed alias" "${dir}/out.log"
    assert_absent "case (b): no warning" "warning:" "${dir}/err.log"

    # A hand-made ABSOLUTE link to the right binary becomes the relative one,
    # so the pair survives a later move of the directory.
    local dir2
    dir2="$(new_sandbox)"
    ln -s "${dir2}/bin/garraia" "${dir2}/bin/garra"
    run_in_library "${dir2}" "${dir2}/bin" install_garra_alias
    assert_symlink_to_garraia "case (b): absolute link normalized to relative" "${dir2}/bin/garra"
}

# ---- case (c): real file -> untouched, warning, still rc 0 -------------------
case_c_keeps_real_file() {
    echo ""
    echo "== case (c) real file named garra -> kept, warning =="
    local dir
    dir="$(new_sandbox)"
    printf '#!/bin/sh\necho from-source-build\n' >"${dir}/bin/garra"
    chmod +x "${dir}/bin/garra"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias

    assert_rc_zero "case (c): returns 0 (a warning never aborts the install)" "${dir}"
    if [ -f "${dir}/bin/garra" ] && [ ! -L "${dir}/bin/garra" ]; then
        pass "case (c): the file is still a regular file"
    else
        fail "case (c): the file was replaced: $(ls -ld "${dir}/bin/garra")"
    fi
    assert_contains "case (c): content untouched" "from-source-build" "${dir}/bin/garra"
    assert_contains "case (c): warns it was left alone" \
        "already exists and was not created by this installer - left untouched" "${dir}/err.log"
    assert_contains "case (c): names the installed binary" \
        "The installed binary is ${dir}/bin/garraia" "${dir}/err.log"
    assert_absent "case (c): does not claim an alias" "Alias" "${dir}/out.log"
}

# ---- case (d): the link is relative, so moving the directory keeps it valid ---
case_d_survives_move() {
    echo ""
    echo "== case (d) relative link survives moving the directory =="
    local dir
    dir="$(new_sandbox)"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias
    mv "${dir}/bin" "${dir}/moved"

    if [ -e "${dir}/moved/garra" ]; then
        pass "case (d): link still resolves after the move"
    else
        fail "case (d): link dangles after the move: $(ls -l "${dir}/moved" 2>&1)"
    fi
    GARRAIA_STUB_LOG="${dir}/stub.log" "${dir}/moved/garra" --version
    assert_contains "case (d): garra --version still executes garraia" "--version" "${dir}/stub.log"
}

# ---- case (e): re-running is idempotent ---------------------------------------
case_e_idempotent() {
    echo ""
    echo "== case (e) running twice is idempotent =="
    local dir
    dir="$(new_sandbox)"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias
    run_in_library "${dir}" "${dir}/bin" install_garra_alias

    assert_rc_zero "case (e): second run returns 0" "${dir}"
    assert_symlink_to_garraia "case (e): still a relative symlink" "${dir}/bin/garra"
    assert_absent "case (e): no warning on the second run" "warning:" "${dir}/err.log"
}

# ---- case (f): ln fails -> warning, never an abort ----------------------------
case_f_ln_failure_is_a_warning() {
    echo ""
    echo "== case (f) ln fails -> warning, rc 0 =="
    if [ "$(id -u)" -eq 0 ]; then
        skip "case (f): running as root, a read-only directory would not stop ln"
        return 0
    fi
    local dir
    dir="$(new_sandbox)"
    chmod 555 "${dir}/bin"
    run_in_library "${dir}" "${dir}/bin" install_garra_alias
    chmod 755 "${dir}/bin"

    assert_rc_zero "case (f): returns 0 even though ln failed" "${dir}"
    assert_contains "case (f): warns and points at garraia" \
        "could not create the 'garra' alias" "${dir}/err.log"
    if [ -e "${dir}/bin/garra" ] || [ -L "${dir}/bin/garra" ]; then
        fail "case (f): a link appeared in a read-only directory"
    else
        pass "case (f): no link was created"
    fi
}

# ---- case (g): /usr/local/bin as non-root -> the SAME sudo branch as the copy --
case_g_sudo_branch() {
    echo ""
    echo "== case (g) /usr/local/bin as non-root -> sudo ln, like the binary =="
    if [ "$(id -u)" -eq 0 ]; then
        skip "case (g): running as root, install_needs_sudo is false by design"
        return 0
    fi
    if [ -e /usr/local/bin/garra ] && [ ! -L /usr/local/bin/garra ]; then
        skip "case (g): this machine has a real /usr/local/bin/garra; the keep-and-warn branch would fire first"
        return 0
    fi
    local dir
    dir="$(mktemp -d)"
    run_in_library "${dir}" "/usr/local/bin" install_garra_alias

    assert_rc_zero "case (g): returns 0" "${dir}"
    assert_contains "case (g): link goes through sudo" \
        "sudo ln -sfn garraia /usr/local/bin/garra" "${dir}/sudo.log"
    assert_absent "case (g): no warning" "warning:" "${dir}/err.log"
}

# ---- case (h): Termux -> alias lands in $PREFIX/bin, no sudo ------------------
case_h_termux_prefix() {
    echo ""
    echo "== case (h) Termux -> alias in \$PREFIX/bin without sudo =="
    local dir
    dir="$(mktemp -d)"
    mkdir -p "${dir}/prefix/bin"
    cp "${stub}" "${dir}/prefix/bin/garraia"
    chmod +x "${dir}/prefix/bin/garraia"
    run_in_library "${dir}" "${dir}/prefix/bin" install_garra_alias \
        "OS_NAME=android" "PREFIX=${dir}/prefix"

    assert_rc_zero "case (h): returns 0" "${dir}"
    assert_symlink_to_garraia "case (h): \$PREFIX/bin/garra -> garraia" "${dir}/prefix/bin/garra"
    if [ -e "${dir}/sudo.log" ]; then
        fail "case (h): Termux has no sudo — got: $(cat "${dir}/sudo.log")"
    else
        pass "case (h): sudo not used on Termux"
    fi
}

# ---- case (i): install_binary wires the alias in -----------------------------
case_i_install_binary_wiring() {
    echo ""
    echo "== case (i) install_binary creates the alias right after the binary =="
    local dir
    dir="$(mktemp -d)"
    mkdir -p "${dir}/tmp"
    cp "${stub}" "${dir}/tmp/garraia-linux-x86_64"
    run_in_library "${dir}" "${dir}/bin" install_binary \
        "OS_NAME=linux" "VERSION=v0.0.0-test" \
        "GARRAIA_TMPDIR=${dir}/tmp" "ARTIFACT=garraia-linux-x86_64" \
        "GARRAIA_INSTALL_DIR=${dir}/bin"

    assert_rc_zero "case (i): install_binary returns 0" "${dir}"
    if [ -x "${dir}/bin/garraia" ]; then
        pass "case (i): garraia installed"
    else
        fail "case (i): garraia missing from ${dir}/bin"
    fi
    assert_symlink_to_garraia "case (i): garra alias created alongside" "${dir}/bin/garra"
    assert_contains "case (i): summary names the binary" "installed to ${dir}/bin/garraia" "${dir}/out.log"
    assert_contains "case (i): summary names the alias" "Alias ${dir}/bin/garra -> garraia" "${dir}/out.log"
}

# ---- case (j): the next-steps summary cites both names -----------------------
case_j_next_steps_cite_both() {
    echo ""
    echo "== case (j) next steps mention the alias =="
    local dir
    dir="$(mktemp -d)"
    run_in_library "${dir}" "${dir}/bin" print_next_steps_legacy "OS_NAME=linux"

    assert_contains "case (j): still prints Next steps" "Next steps:" "${dir}/out.log"
    assert_contains "case (j): explains the alias" \
        "'garra' is an alias for 'garraia'" "${dir}/out.log"
}

case_a_creates_link
case_b_repoints_stale_link
case_c_keeps_real_file
case_d_survives_move
case_e_idempotent
case_f_ln_failure_is_a_warning
case_g_sudo_branch
case_h_termux_prefix
case_i_install_binary_wiring
case_j_next_steps_cite_both

echo ""
echo "==============================================="
echo "garra_alias.sh — pass: ${results_pass}, fail: ${results_fail}"
echo "==============================================="
if [ "${results_fail}" -ne 0 ]; then
    exit 1
fi
