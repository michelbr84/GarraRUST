#!/usr/bin/env bash
# Plan 0127 §M1.3 — unit tests for `bootstrap_phase` in `install.sh`.
#
# Strategy: source `install.sh` with `GARRAIA_INSTALL_SH_LIBRARY=1`
# so its `main()` does not run. Then pre-populate `INSTALL_PATH` with
# the `garraia-stub.sh` fixture and invoke `bootstrap_phase` directly,
# asserting on the stub's log to verify which subcommands ran.
#
# We use plain Bash assertions rather than bats-core to keep test
# infrastructure minimal — matches the project's existing
# `tests/e2e_*.sh` style.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"
stub="${repo_root}/tests/install_sh/fixtures/garraia-stub.sh"

chmod +x "${stub}"

# install.sh is Linux/macOS only — `detect_platform` errors out on
# Windows. The exec-on-/dev/tty paths in `bootstrap_phase` also rely
# on a real POSIX tty, which git-bash on Windows does not expose.
# Skip the whole test runner outside Linux/macOS so we don't emit
# false negatives on a developer's Windows checkout. CI runs on Ubuntu
# and exercises the full matrix.
case "$(uname -s)" in
    Linux|Darwin) : ;;
    *)
        echo "bootstrap_phase.sh: skipping on $(uname -s) — install.sh is Linux/macOS only."
        exit 0
        ;;
esac

# ---- assertion helpers -------------------------------------------------------

# All tests collect output into ${log_dir}/test.log so a single grep
# can verify the expected lines (and absence of forbidden ones).
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

assert_log_contains() {
    local label="$1" needle="$2" log="$3"
    if grep -qF "${needle}" "${log}"; then
        pass "${label}"
    else
        fail "${label} — expected '${needle}' in ${log}; got:"
        sed 's/^/    /' "${log}" >&2 || true
    fi
}

assert_log_absent() {
    local label="$1" needle="$2" log="$3"
    if grep -qF "${needle}" "${log}" 2>/dev/null; then
        fail "${label} — '${needle}' should NOT appear in ${log}; got:"
        sed 's/^/    /' "${log}" >&2 || true
    else
        pass "${label}"
    fi
}

# Can this process open its controlling terminal? The same probe as
# `has_usable_tty` in install.sh, minus the CI short-circuit (the
# harness decides about CI itself). `[ -r /dev/tty ]` would lie here
# exactly as it lied in install.sh: GitHub-hosted runners and containers
# ship a crw-rw-rw- /dev/tty that fails to open with ENXIO.
harness_has_tty() {
    (: </dev/tty) 2>/dev/null
}

# Run "$@" with no controlling terminal, so /dev/tty exists (0666) but
# open(2) on it fails with ENXIO -- the container/CI condition behind
# the v0.4.4 smoke failure. `setsid -w` is util-linux (every CI runner);
# python3's os.setsid() covers macOS, which ships no setsid(1). Returns
# 127 when neither is available so the caller can SKIP instead of
# asserting against a process that still owns a terminal.
run_without_ctty() {
    if command -v setsid >/dev/null 2>&1; then
        setsid -w "$@"
    elif command -v python3 >/dev/null 2>&1; then
        # setsid(2) refuses a process-group leader (EPERM), so fork first
        # in that case -- what setsid(1) does -- and hand back the exit code.
        python3 -c '
import os, sys
if os.getpgrp() == os.getpid():
    pid = os.fork()
    if pid:
        _, status = os.waitpid(pid, 0)
        sys.exit(os.WEXITSTATUS(status) if os.WIFEXITED(status) else 1)
os.setsid()
os.execvp(sys.argv[1], sys.argv[1:])
' "$@"
    else
        return 127
    fi
}

# Source the script in library mode and run bootstrap_phase under a
# controlled env. Output goes to ${log_dir}/output.log; the stub's
# subcommand trace goes to ${log_dir}/stub.log.
#
# On a GitHub-hosted runner (or any environment with no controlling
# tty) the installer correctly refuses the wizard — but that masks the
# init/start branches we want to test. When that happens we wrap the
# subshell with `script -qec` to allocate a pty so /dev/tty is
# openable inside the inner shell, restoring coverage of cases (c),
# (d), (e) and (f). The wrap is skipped when the parent shell already
# has a terminal (developer running tests in a terminal) so the run
# stays fast.
#
# CI is unset inside the runner: GitHub Actions exports CI=true, and
# install.sh treats a non-empty CI as "nobody at the keyboard", which
# would send every case down the non-interactive path. A case that
# wants CI set (case g) passes it through BOOTSTRAP_TEST_CI.
run_bootstrap_in_subshell() {
    local log_dir="$1"
    # Materialize the inner script to a temp file — keeps quoting
    # simple and works inside `script -qec "bash <path>"`.
    local runner="${log_dir}/run-inner.sh"
    cat >"${runner}" <<INNER
#!/usr/bin/env bash
set +e
unset CI
if [ -n "\${BOOTSTRAP_TEST_CI:-}" ]; then
    export CI="\${BOOTSTRAP_TEST_CI}"
fi
if (: </dev/tty) 2>/dev/null; then
    echo "__runner_tty__=ok"
else
    echo "__runner_tty__=fails"
fi
export GARRAIA_INSTALL_SH_LIBRARY=1
export GARRAIA_STUB_LOG="${log_dir}/stub.log"
# shellcheck disable=SC1090
. "${install_sh}"
INSTALL_PATH="${stub}"
bootstrap_phase
# To stdout, not appended to output.log: under the \`script\` wrap the
# pty relay holds output.log open at its own offset and overwrites
# anything appended to the file behind its back.
echo "__bootstrap_phase_rc__=\$?"
INNER
    chmod +x "${runner}"

    if harness_has_tty; then
        bash "${runner}" >"${log_dir}/output.log" 2>&1 || true
    elif command -v script >/dev/null 2>&1; then
        # `script -qec 'cmd' /dev/null`: -q suppresses the banner,
        # -e forwards the wrapped command's exit code, -c runs the
        # command and exits. Allocates a pty so /dev/tty is real
        # inside the inner shell.
        script -qec "bash ${runner}" /dev/null \
            >"${log_dir}/output.log" 2>&1 || true
    else
        echo "WARNING: no working /dev/tty and no \`script\` command — cases (c)/(d)/(e) may fail" >&2
        bash "${runner}" >"${log_dir}/output.log" 2>&1 || true
    fi
}

# ---- case (a): no usable /dev/tty → non-interactive path, cleanly ----------
# Regression for the v0.4.4 clean-install smoke run. In a container (and on
# a GitHub-hosted runner) /dev/tty is crw-rw-rw- but open(2) fails with
# ENXIO; the old `[ -r /dev/tty ]` probe read only the permission bits,
# said "interactive", and the installer printed
#     main: line 631: /dev/tty: No such device or address
#     Wizard exited non-zero — your config may need manual edits.
# for a wizard that never ran. This case used to accept that output as a
# "WSL/MinGW quirk"; it was the bug.
#
# The child runs with no controlling terminal (run_without_ctty) and stdin
# from /dev/null, which recreates that exact condition on a laptop and on a
# CI runner alike. It reports the precondition first, so the case cannot
# pass vacuously on a host where /dev/tty still opens. It runs under `sh`
# (dash on Debian/Ubuntu — what `curl | sh` actually uses) and under bash.
#
# CI is unset: on GitHub Actions CI=true would otherwise take the
# non-interactive branch through the CI short-circuit (case g) and hide a
# regression in the tty probe itself.
case_a_no_tty() {
    echo ""
    echo "== case (a) no usable /dev/tty → non-interactive path, no wizard =="
    local shell_name
    for shell_name in sh bash; do
        if ! command -v "${shell_name}" >/dev/null 2>&1; then
            echo "  SKIP: ${shell_name} not installed"
            continue
        fi
        local log_dir
        log_dir="$(mktemp -d)"
        local inner="${log_dir}/run-no-ctty.sh"
        cat >"${inner}" <<INNER
set +e
unset CI
if [ -c /dev/tty ]; then echo "__tty_node__=present"; else echo "__tty_node__=absent"; fi
if (: </dev/tty) 2>/dev/null; then echo "__tty_open__=ok"; else echo "__tty_open__=fails"; fi
GARRAIA_INSTALL_SH_LIBRARY=1
export GARRAIA_INSTALL_SH_LIBRARY
GARRAIA_STUB_LOG="${log_dir}/stub.log"
export GARRAIA_STUB_LOG
. "${install_sh}"
INSTALL_PATH="${stub}"
bootstrap_phase
echo "__rc__=\$?"
INNER

        local out="${log_dir}/output.log"
        local rc=0
        run_without_ctty "${shell_name}" "${inner}" </dev/null >"${out}" 2>&1 || rc=$?
        if [ "${rc}" -eq 127 ] && [ ! -s "${out}" ]; then
            echo "  SKIP: neither setsid nor python3 available — cannot drop the controlling terminal"
            return 0
        fi

        local label="case (a) [${shell_name}]"
        if grep -qF "__tty_open__=fails" "${out}"; then
            pass "${label}: precondition — /dev/tty cannot be opened in the child"
        else
            fail "${label}: precondition — /dev/tty still opens with no controlling terminal; this case proves nothing here"
            sed 's/^/    /' "${out}" >&2 || true
            continue
        fi
        # The node being present is what made the old probe lie; without
        # it the case still checks the clean path but cannot tell the
        # probes apart.
        if grep -qF "__tty_node__=present" "${out}"; then
            pass "${label}: precondition — /dev/tty node present (the ENXIO condition)"
        else
            echo "  NOTE: ${label}: /dev/tty node absent — cannot discriminate the permission-bits probe"
        fi

        assert_log_contains "${label}: prints the non-interactive notice" \
            "Non-interactive install (no /dev/tty available)" "${out}"
        assert_log_absent "${label}: never announces the wizard" \
            "Running interactive setup wizard" "${out}"
        assert_log_absent "${label}: no misleading 'wizard exited non-zero'" \
            "Wizard exited non-zero" "${out}"
        # Every shell phrases a failed redirect as '<path>: <strerror>'
        # (bash: 'line N: /dev/tty: ...', dash: 'cannot open /dev/tty: ...',
        # busybox: "can't open /dev/tty: ..."). Our own notice never has
        # the colon right after the path.
        assert_log_absent "${label}: no shell redirect error on /dev/tty" \
            "/dev/tty:" "${out}"
        assert_log_absent "${label}: never tries to start in the foreground" \
            "Starting GarraIA in the foreground" "${out}"
        assert_log_contains "${label}: prints legacy Next steps" \
            "Next steps:" "${out}"
        assert_log_contains "${label}: exits 0" "__rc__=0" "${out}"
        if [ -f "${log_dir}/stub.log" ]; then
            fail "${label}: the binary was invoked without a terminal"
            sed 's/^/    /' "${log_dir}/stub.log" >&2 || true
        else
            pass "${label}: binary never invoked (stub log never created)"
        fi
    done
}

# ---- case (b): both skips → next-steps, no subcommand --------------------
case_b_both_skip() {
    echo ""
    echo "== case (b) both skips → next-steps, no init/start =="
    local log_dir
    log_dir="$(mktemp -d)"
    GARRAIA_SKIP_INIT=1 GARRAIA_SKIP_START=1 run_bootstrap_in_subshell "${log_dir}"

    assert_log_contains "case (b): prints legacy Next steps" \
        "Next steps:" "${log_dir}/output.log"
    if [ -f "${log_dir}/stub.log" ]; then
        assert_log_absent "case (b): stub NOT invoked" "init" "${log_dir}/stub.log"
    else
        pass "case (b): stub log never created (no invocation)"
    fi
}

# ---- case (c): GARRAIA_SKIP_INIT=1 alone → start invoked, init skipped ---
case_c_skip_init_only() {
    echo ""
    echo "== case (c) skip init only → start invoked =="
    local log_dir
    log_dir="$(mktemp -d)"
    GARRAIA_SKIP_INIT=1 run_bootstrap_in_subshell "${log_dir}"
    # `exec` replaces the subshell so this case actually exec's the
    # stub. The stub's log will record one "start" entry.
    if [ -f "${log_dir}/stub.log" ]; then
        assert_log_contains "case (c): start invoked" "start" "${log_dir}/stub.log"
        assert_log_absent "case (c): init NOT invoked" "init" "${log_dir}/stub.log"
    else
        fail "case (c): stub log missing — bootstrap_phase did not invoke start"
        echo "    --- output.log ---" >&2
        sed 's/^/    /' "${log_dir}/output.log" >&2 || true
        echo "    --- end output.log ---" >&2
    fi
}

# ---- case (d): GARRAIA_SKIP_START=1 alone → init invoked, start skipped --
case_d_skip_start_only() {
    echo ""
    echo "== case (d) skip start only → init invoked =="
    local log_dir
    log_dir="$(mktemp -d)"
    GARRAIA_SKIP_START=1 run_bootstrap_in_subshell "${log_dir}"

    if [ -f "${log_dir}/stub.log" ]; then
        assert_log_contains "case (d): init invoked" "init" "${log_dir}/stub.log"
        assert_log_absent "case (d): start NOT invoked" "start" "${log_dir}/stub.log"
    else
        fail "case (d): stub log missing — bootstrap_phase did not invoke init"
    fi
    assert_log_contains "case (d): prints legacy Next steps after init" \
        "Next steps:" "${log_dir}/output.log"
}

# ---- case (e): default → init then start ---------------------------------
case_e_default() {
    echo ""
    echo "== case (e) default → init then start =="
    local log_dir
    log_dir="$(mktemp -d)"
    run_bootstrap_in_subshell "${log_dir}"

    if [ -f "${log_dir}/stub.log" ]; then
        assert_log_contains "case (e): init invoked first" \
            "init" "${log_dir}/stub.log"
        assert_log_contains "case (e): start invoked after init" \
            "start" "${log_dir}/stub.log"
        # Order: init must appear on a line before start.
        if head -1 "${log_dir}/stub.log" | grep -q "^init"; then
            pass "case (e): init is the first subcommand"
        else
            fail "case (e): init was not the first subcommand"
            cat "${log_dir}/stub.log" >&2 || true
        fi
    else
        fail "case (e): stub log missing — bootstrap_phase invoked nothing"
    fi
}

# ---- case (f): init fails non-zero → fall through to next-steps ----------
case_f_init_fails() {
    echo ""
    echo "== case (f) init fails → next-steps, no start =="
    local log_dir
    log_dir="$(mktemp -d)"
    GARRAIA_STUB_FAIL_INIT=1 run_bootstrap_in_subshell "${log_dir}"

    if [ -f "${log_dir}/stub.log" ]; then
        assert_log_contains "case (f): init invoked once" \
            "init" "${log_dir}/stub.log"
        assert_log_absent "case (f): start NOT invoked after init failure" \
            "start" "${log_dir}/stub.log"
    fi
    assert_log_contains "case (f): falls through to legacy Next steps" \
        "Next steps:" "${log_dir}/output.log"
}

# ---- case (g): CI set, terminal usable → non-interactive, no wizard --------
# Parity with install.ps1 (rule 16): Test-InteractiveSession returns false
# whenever CI is set. A CI job can hand the installer a pty (`docker run
# -t`, `script`), but nobody will type into it, so the wizard would block
# the job until its timeout. Driven through run_bootstrap_in_subshell's pty
# wrap so the terminal really IS openable and only the CI check can keep
# the wizard from running. Mirrored by the "CI is set" block in
# tests/install_ps1/bootstrap_phase.ps1.
case_g_ci_with_tty() {
    echo ""
    echo "== case (g) CI set, terminal usable → non-interactive, no wizard =="
    local log_dir
    log_dir="$(mktemp -d)"
    BOOTSTRAP_TEST_CI=true run_bootstrap_in_subshell "${log_dir}"
    local out="${log_dir}/output.log"

    if grep -qF "__runner_tty__=ok" "${out}"; then
        pass "case (g): precondition — /dev/tty opens inside the runner"
    else
        fail "case (g): precondition — no usable terminal inside the runner (need a tty or \`script\`)"
        sed 's/^/    /' "${out}" >&2 || true
        return 0
    fi
    assert_log_contains "case (g): says CI is why it stopped" \
        "Non-interactive install (CI environment detected)" "${out}"
    assert_log_absent "case (g): never announces the wizard" \
        "Running interactive setup wizard" "${out}"
    assert_log_contains "case (g): prints legacy Next steps" \
        "Next steps:" "${out}"
    assert_log_contains "case (g): exits 0" "__bootstrap_phase_rc__=0" "${out}"
    if [ -f "${log_dir}/stub.log" ]; then
        fail "case (g): the binary was invoked under CI"
        sed 's/^/    /' "${log_dir}/stub.log" >&2 || true
    else
        pass "case (g): binary never invoked (stub log never created)"
    fi
}

case_a_no_tty
case_b_both_skip
case_c_skip_init_only
case_d_skip_start_only
case_e_default
case_f_init_fails
case_g_ci_with_tty

echo ""
echo "==============================================="
echo "bootstrap_phase.sh — pass: ${results_pass}, fail: ${results_fail}"
echo "==============================================="
if [ "${results_fail}" -ne 0 ]; then
    exit 1
fi
