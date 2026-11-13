#!/usr/bin/env bash
# Issue #1862 — the installer must finish an UPGRADE while the gateway is
# running, and must not pretend the running process picked the new version.
#
# Three contracts, one file:
#   (a) the upgrade path: `cp` over a RUNNING executable is refused by the
#       kernel with ETXTBSY ("Text file busy") — reproduced right here as a
#       precondition — and install_binary's staged rename completes the same
#       upgrade over the same live process: new bytes on disk, exec bit set,
#       the running process untouched, no staging leftovers;
#   (b) a clean install keeps working (regression) and the sudo branch of
#       install_binary_atomically stages/chmods/renames under `sudo` the
#       same way;
#   (c) the upgrade notice: bootstrap_phase returns early with the restart
#       instruction when a gateway is already running (no wizard, no
#       `garraia start` over a live service), and the probe is disabled by
#       GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE=1 so harnesses are not hostage
#       to the machine they run on.
#
# Strategy mirrors the sibling suites: source `install.sh` with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() never runs, and sandbox every
# assertion in a mktemp directory. `sudo` is replaced by a logging stub that
# still EXECUTES the command, so the privilege branch is observable without
# touching /usr/local/bin.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"
stub="${repo_root}/tests/install_sh/fixtures/garraia-stub.sh"

case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "upgrade_etxtbsy.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
        exit 0
        ;;
esac

chmod +x "${stub}"

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

assert_rc_zero() {
    local label="$1" sandbox="$2"
    if [ "$(cat "${sandbox}/rc" 2>/dev/null)" = "0" ]; then
        pass "${label}"
    else
        fail "${label} — rc=$(cat "${sandbox}/rc" 2>/dev/null || echo 'subshell died before writing rc')"
    fi
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

# No staging leftovers next to the freshly installed binary: only the binary
# and the `garra` alias (created by install_garra_alias) may be there.
assert_no_staging_leftovers() {
    local label="$1" bindir="$2" strays
    strays="$(ls "${bindir}" 2>/dev/null | grep -v -e '^garraia$' -e '^garra$' || true)"
    if [ -z "${strays}" ]; then
        pass "${label}"
    else
        fail "${label} — leftovers in ${bindir}: ${strays}"
    fi
}

kill_and_wait() {
    local pid="$1"
    kill "${pid}" 2>/dev/null || true
    wait "${pid}" 2>/dev/null || true
}

# Source install.sh in library mode and run install_binary against a sandbox
# bin/ directory. GARRAIA_TMPDIR stands in for the download step; the artifact
# is the garraia-stub fixture, so the installed bytes are recognisable.
run_install_binary() {
    local sandbox="$1"
    (
        set +e
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        export GARRAIA_INSTALL_DIR="${sandbox}/bin"
        GARRAIA_TMPDIR="$(mktemp -d)"
        cp "${stub}" "${GARRAIA_TMPDIR}/garraia-test-x86_64"
        ARTIFACT="garraia-test-x86_64"
        VERSION="vTEST-1862"
        OS_NAME="linux"
        ARCH_NAME="x86_64"
        install_binary >"${sandbox}/out.log" 2>"${sandbox}/err.log"
        echo "$?" >"${sandbox}/rc"
        rm -rf "${GARRAIA_TMPDIR}"
    )
}

# ---- case (a): upgrade over a RUNNING binary ---------------------------------
#
# The old binary being replaced must be a REAL executable: the kernel's
# deny_write_access applies to a file being EXECUTED. A script run as
# `sh script.sh` is only opened for reading by the interpreter, so `cp` over
# it would succeed and prove nothing.
#
# The executable copied into the sandbox is the HARNESS's own bash, resolved
# from /proc/$$/exe ($$ = the bash running this suite): a real ELF on every
# Linux box that ignores the argv[0] basename. Two other candidates fail:
#   - copying /bin/sleep breaks on distros whose coreutils ships as a single
#     multicall binary (the CI runner did: the copy dispatches on argv[0]
#     and prints "coreutils: unknown program 'garraia'" instead of sleeping);
#   - `cp /proc/self/exe` is a trap: /proc/self resolves to the process that
#     OPENS the link — cp copies ITSELF, not the harness.
case_a_upgrade_while_running() {
    echo ""
    echo "== case (a) upgrade over a running binary -> staged rename, no Text file busy =="
    local harness_exe
    harness_exe="$(readlink -f "/proc/$$/exe" 2>/dev/null || true)"
    if [ -z "${harness_exe}" ] || [ ! -x "${harness_exe}" ]; then
        skip "case (a): cannot resolve the harness executable via /proc/$$/exe"
        return 0
    fi
    local dir
    dir="$(mktemp -d)"
    mkdir -p "${dir}/bin"
    cp "${harness_exe}" "${dir}/bin/garraia"
    chmod +x "${dir}/bin/garraia"
    # TWO commands, never one: `bash -c 'sleep 60'` exec-replaces itself with
    # sleep (the deny_write_access moves off the copied file and the
    # precondition cp would succeed); with a trailing command bash forks and
    # STAYS as the executing garraia file, which is what holds ETXTBSY.
    "${dir}/bin/garraia" -c 'sleep 60; exit' &
    local pid=$!
    sleep 0.5

    # Precondition: this environment reproduces ETXTBSY at all. If a plain cp
    # over the running binary succeeds here, the case cannot prove anything
    # and must say so instead of passing vacuously.
    if cp "${stub}" "${dir}/bin/garraia" 2>/dev/null; then
        fail "case (a): precondition — cp over a RUNNING binary succeeded; this system cannot reproduce 'Text file busy', so the case proves nothing here"
        kill_and_wait "${pid}"
        rm -rf "${dir}"
        return 0
    fi
    pass "case (a): precondition — cp over the running binary is refused (Text file busy reproduced)"

    run_install_binary "${dir}"

    assert_rc_zero "case (a): install_binary returns 0 over the live process" "${dir}"
    if [ -f "${dir}/bin/garraia" ] && [ ! -L "${dir}/bin/garraia" ] \
        && grep -qF "GARRAIA_STUB_LOG" "${dir}/bin/garraia"; then
        pass "case (a): bin/garraia now holds the new artifact bytes"
    else
        fail "case (a): bin/garraia does not hold the new artifact: $(ls -ld "${dir}/bin/garraia" 2>&1)"
    fi
    if [ -x "${dir}/bin/garraia" ]; then
        pass "case (a): the new binary keeps its exec bit"
    else
        fail "case (a): the new binary lost its exec bit"
    fi
    if kill -0 "${pid}" 2>/dev/null; then
        pass "case (a): the running process survived the upgrade untouched"
    else
        fail "case (a): the running process died during the install"
    fi
    assert_no_staging_leftovers "case (a): no staging leftovers" "${dir}/bin"
    assert_contains "case (a): reports the installed version" \
        "GarraIA vTEST-1862 installed to ${dir}/bin/garraia" "${dir}/out.log"

    kill_and_wait "${pid}"
    rm -rf "${dir}"
}

# ---- case (b): clean install regression --------------------------------------
case_b_clean_install() {
    echo ""
    echo "== case (b) clean install -> same staged path, no regression =="
    local dir
    dir="$(mktemp -d)"
    run_install_binary "${dir}"

    assert_rc_zero "case (b): install_binary returns 0 on a fresh directory" "${dir}"
    if [ -f "${dir}/bin/garraia" ] && grep -qF "GARRAIA_STUB_LOG" "${dir}/bin/garraia"; then
        pass "case (b): bin/garraia installed"
    else
        fail "case (b): bin/garraia missing: $(ls -l "${dir}/bin" 2>&1)"
    fi
    if [ -x "${dir}/bin/garraia" ]; then
        pass "case (b): exec bit set"
    else
        fail "case (b): exec bit missing"
    fi
    assert_no_staging_leftovers "case (b): no staging leftovers" "${dir}/bin"
    assert_contains "case (b): alias created" \
        "Alias ${dir}/bin/garra -> garraia" "${dir}/out.log"
    rm -rf "${dir}"
}

# ---- case (c): sudo branch stages the same way --------------------------------
# sudo is stubbed to LOG and EXECUTE the command, so the privilege branch is
# exercised without touching /usr/local/bin.
case_c_sudo_branch() {
    echo ""
    echo "== case (c) sudo branch -> cp/chmod/mv all under sudo =="
    local dir
    dir="$(mktemp -d)"
    mkdir -p "${dir}/bin" "${dir}/download"
    cp "${stub}" "${dir}/download/artifact"
    (
        set +e
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        # Execute the command and record it — no privilege change in the test.
        # shellcheck disable=SC2317
        sudo() {
            printf 'sudo %s\n' "$*" >>"${dir}/sudo.log"
            "$@"
        }
        install_binary_atomically "${dir}/download/artifact" "${dir}/bin/garraia" 1 \
            >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (c): install_binary_atomically returns 0 under sudo stub" "${dir}"
    if [ -f "${dir}/bin/garraia" ] && [ -x "${dir}/bin/garraia" ] \
        && grep -qF "GARRAIA_STUB_LOG" "${dir}/bin/garraia"; then
        pass "case (c): destination holds the new executable bytes"
    else
        fail "case (c): destination wrong: $(ls -l "${dir}/bin" 2>&1)"
    fi
    local steps
    steps="$(grep -c -e '^sudo cp ' -e '^sudo chmod ' -e '^sudo mv ' "${dir}/sudo.log" 2>/dev/null || true)"
    if [ "${steps}" = "3" ]; then
        pass "case (c): cp, chmod and mv all went through sudo"
    else
        fail "case (c): expected 3 sudo steps (cp/chmod/mv), got ${steps}: $(cat "${dir}/sudo.log" 2>/dev/null)"
    fi
    assert_no_staging_leftovers "case (c): no staging leftovers" "${dir}/bin"
    rm -rf "${dir}"
}

# ---- case (d): bootstrap_phase prints the restart notice on a live gateway ----
# A PATH-stubbed `systemctl` answers the user-unit probe, so the case does not
# depend on the machine running a real gateway.
case_d_restart_notice() {
    echo ""
    echo "== case (d) gateway running -> restart notice, no wizard, no start =="
    local dir stubbin
    dir="$(mktemp -d)"
    mkdir -p "${dir}/bin" "${dir}/stubbin"
    stubbin="${dir}/stubbin"
    cat >"${stubbin}/systemctl" <<STUB
#!/bin/sh
# Answers only "user unit is-active" with success; everything else is no unit.
if [ "\${1:-}" = "--user" ] && [ "\${2:-}" = "is-active" ]; then
    exit 0
fi
exit 3
STUB
    chmod +x "${stubbin}/systemctl"

    GARRAIA_STUB_LOG="${dir}/bin/stub.log"
    export GARRAIA_STUB_LOG
    (
        set +e
        # stubbin FIRST: the PATH-stubbed systemctl shadows any real one.
        PATH="${stubbin}:${PATH}"
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        INSTALL_PATH="${stub}"
        # resolve_version sets VERSION in production before the install;
        # pre-populate it the same way the sibling suites pre-populate
        # INSTALL_PATH (install.sh runs with `set -u`).
        VERSION="vTEST-1862"
        bootstrap_phase >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (d): bootstrap_phase returns 0" "${dir}"
    assert_contains "case (d): prints the upgrade note" \
        "still running with the previous version" "${dir}/out.log"
    assert_contains "case (d): names the installed version" \
        "the binary on disk is now vTEST-1862" "${dir}/out.log"
    assert_contains "case (d): user unit gets the user-unit restart command" \
        "systemctl --user restart garraia.service" "${dir}/out.log"
    assert_absent "case (d): under a unit, stop&&start would orphan the daemon — never offered" \
        "garraia stop && garraia start" "${dir}/out.log"
    if [ -e "${dir}/bin/stub.log" ]; then
        fail "case (d): the wizard/start ran over a live gateway: $(cat "${dir}/bin/stub.log")"
    else
        pass "case (d): neither init nor start ran (no second gateway over a live service)"
    fi
    rm -rf "${dir}"
}

# ---- case (d2): SYSTEM unit active -> sudo systemctl, not the user command ----
# deploy/systemd/garraia.service installs to /etc/systemd/system (system unit,
# User=garraia) — the reviewer-confirmed B1 gap: the user-unit command would
# be wrong there, and stop&&start would orphan the daemon.
case_d2_system_unit_notice() {
    echo ""
    echo "== case (d2) system unit running -> sudo systemctl restart =="
    local dir stubbin
    dir="$(mktemp -d)"
    mkdir -p "${dir}/stubbin"
    stubbin="${dir}/stubbin"
    cat >"${stubbin}/systemctl" <<'STUB'
#!/bin/sh
# Answers only the SYSTEM-unit is-active probe (no --user prefix).
if [ "${1:-}" = "is-active" ]; then
    exit 0
fi
exit 3
STUB
    chmod +x "${stubbin}/systemctl"

    (
        set +e
        PATH="${stubbin}:${PATH}"
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        INSTALL_PATH="${stub}"
        VERSION="vTEST-1862"
        if gateway_running && print_restart_notice; then
            :
        fi >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (d2): probe and notice return 0" "${dir}"
    assert_contains "case (d2): system unit gets the sudo restart command" \
        "sudo systemctl restart garraia.service" "${dir}/out.log"
    assert_absent "case (d2): no user-unit command" \
        "systemctl --user restart garraia.service" "${dir}/out.log"
    assert_absent "case (d2): under a unit, stop&&start is never offered" \
        "garraia stop && garraia start" "${dir}/out.log"
    rm -rf "${dir}"
}

# ---- case (e): no gateway running -> no notice, normal bootstrap -------------
case_e_no_notice_when_idle() {
    echo ""
    echo "== case (e) no gateway running -> no notice, skips respected =="
    local dir stubbin
    dir="$(mktemp -d)"
    mkdir -p "${dir}/stubbin"
    stubbin="${dir}/stubbin"
    # A systemd that exists but knows no active unit: gateway_running must
    # return false WITHOUT falling through to pgrep.
    cat >"${stubbin}/systemctl" <<'STUB'
#!/bin/sh
exit 3
STUB
    chmod +x "${stubbin}/systemctl"

    GARRAIA_STUB_LOG="${dir}/stub.log"
    export GARRAIA_STUB_LOG
    (
        set +e
        PATH="${stubbin}:${PATH}"
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        INSTALL_PATH="${stub}"
        VERSION="vTEST-1862"
        GARRAIA_SKIP_INIT=1
        GARRAIA_SKIP_START=1
        bootstrap_phase >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (e): bootstrap_phase returns 0" "${dir}"
    assert_absent "case (e): no upgrade note" "still running with the previous version" "${dir}/out.log"
    assert_contains "case (e): legacy next steps" "Next steps:" "${dir}/out.log"
    assert_contains "case (e): skips respected, wizard never announced" \
        "garraia init    # interactive setup wizard" "${dir}/out.log"
    rm -rf "${dir}"
}

# ---- case (f): the kill-switch disables the probe -----------------------------
case_f_kill_switch() {
    echo ""
    echo "== case (f) GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE=1 -> probe disabled =="
    local dir stubbin
    dir="$(mktemp -d)"
    mkdir -p "${dir}/stubbin"
    stubbin="${dir}/stubbin"
    cat >"${stubbin}/systemctl" <<'STUB'
#!/bin/sh
if [ "${1:-}" = "--user" ] && [ "${2:-}" = "is-active" ]; then
    exit 0
fi
exit 3
STUB
    chmod +x "${stubbin}/systemctl"

    GARRAIA_STUB_LOG="${dir}/stub.log"
    export GARRAIA_STUB_LOG
    (
        set +e
        PATH="${stubbin}:${PATH}"
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        INSTALL_PATH="${stub}"
        VERSION="vTEST-1862"
        GARRAIA_SKIP_RUNNING_GATEWAY_NOTICE=1
        GARRAIA_SKIP_INIT=1
        GARRAIA_SKIP_START=1
        bootstrap_phase >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (f): bootstrap_phase returns 0" "${dir}"
    assert_absent "case (f): no upgrade note despite the active stub" \
        "still running with the previous version" "${dir}/out.log"
    assert_contains "case (f): normal branch taken" "Next steps:" "${dir}/out.log"
    rm -rf "${dir}"
}

# ---- case (g): pgrep fallback where systemd is absent ------------------------
case_g_pgrep_fallback() {
    echo ""
    echo "== case (g) no systemctl, pgrep matches -> notice fires =="
    local dir stubbin
    dir="$(mktemp -d)"
    mkdir -p "${dir}/stubbin"
    stubbin="${dir}/stubbin"
    printf '#!/bin/sh\nexit 0\n' >"${stubbin}/pgrep"
    chmod +x "${stubbin}/pgrep"

    (
        set +e
        # ONLY the stub dir: on merged-usr systems /bin IS /usr/bin, so adding
        # them back would expose the real systemctl and command -v would take
        # the systemd branch instead of the pgrep fallback this case proves.
        # Everything the probe needs below is a builtin or lives in stubbin.
        PATH="${stubbin}"
        export GARRAIA_INSTALL_SH_LIBRARY=1
        # shellcheck disable=SC1090
        . "${install_sh}"
        set +e
        INSTALL_PATH="${stub}"
        VERSION="vTEST-1862"
        if gateway_running && print_restart_notice; then
            echo "__gateway_running__=yes"
        else
            echo "__gateway_running__=no"
        fi >"${dir}/out.log" 2>"${dir}/err.log"
        echo "$?" >"${dir}/rc"
    )
    assert_rc_zero "case (g): gateway_running probe returns 0" "${dir}"
    assert_contains "case (g): pgrep fallback detects the running gateway" \
        "__gateway_running__=yes" "${dir}/out.log"
    # No unit can hold the gateway here, so stop&&start IS the right command.
    assert_contains "case (g): bare process gets stop&&start" \
        "garraia stop && garraia start" "${dir}/out.log"
    rm -rf "${dir}"
}

case_a_upgrade_while_running
case_b_clean_install
case_c_sudo_branch
case_d_restart_notice
case_d2_system_unit_notice
case_e_no_notice_when_idle
case_f_kill_switch
case_g_pgrep_fallback

echo ""
echo "==============================================="
echo "upgrade_etxtbsy.sh — pass: ${results_pass}, fail: ${results_fail}"
echo "==============================================="
if [ "${results_fail}" -ne 0 ]; then
    exit 1
fi
