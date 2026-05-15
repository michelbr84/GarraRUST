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

# Source the script in library mode and run bootstrap_phase under a
# controlled env. Output goes to ${log_dir}/output.log; the stub's
# subcommand trace goes to ${log_dir}/stub.log.
#
# On a GitHub-hosted runner (or any environment with no controlling
# tty) `[ -r /dev/tty ]` correctly fails — but that masks the
# init/start branches we want to test. When that happens we wrap the
# subshell with `script -qec` to allocate a pty so /dev/tty is
# readable inside the inner shell, restoring coverage of cases (c),
# (d), and (e). The wrap is skipped when the parent shell already has
# /dev/tty (developer running tests in a terminal) so the run stays
# fast.
run_bootstrap_in_subshell() {
    local log_dir="$1"
    # Materialize the inner script to a temp file — keeps quoting
    # simple and works inside `script -qec "bash <path>"`.
    local runner="${log_dir}/run-inner.sh"
    cat >"${runner}" <<INNER
#!/usr/bin/env bash
set +e
export GARRAIA_INSTALL_SH_LIBRARY=1
export GARRAIA_STUB_LOG="${log_dir}/stub.log"
# shellcheck disable=SC1090
. "${install_sh}"
INSTALL_PATH="${stub}"
bootstrap_phase
echo "__bootstrap_phase_rc__=\$?" >>"${log_dir}/output.log"
INNER
    chmod +x "${runner}"

    # GitHub-hosted runners present /dev/tty as a character device
    # file (so `[ -r /dev/tty ]` returns true) but the actual `</dev/tty`
    # redirect fails at exec time because no controlling terminal is
    # attached. Probe via a real read-with-timeout instead of the
    # superficial readability check.
    if dd if=/dev/tty bs=0 count=0 status=none </dev/tty >/dev/null 2>&1; then
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

# ---- case (a): no /dev/tty → next-steps + exit 0 ----------------------------
# We can't make /dev/tty unreadable on a real terminal, but the
# bootstrap_phase logic checks `[ -r /dev/tty ]`. Under `</dev/null`
# AND with no controlling tty (set sid / setsid not always available),
# Bash still sees /dev/tty if there's any. Instead we cover this
# branch by overriding the readability test via a Bash trick:
# we wrap the section in a subshell where /dev/tty is bind-replaced
# with a closed fd. Since that needs root on Linux, we instead drive
# this case by running install.sh as a child with stdin from /dev/null,
# inside a `script -q -c 'setsid ...'`-style setup ONLY when those
# tools are present. Otherwise we run the equivalent assertion against
# both-skip mode (case b), which already exercises the same
# next-steps-and-exit code path.
case_a_no_tty() {
    echo ""
    echo "== case (a) no /dev/tty → next-steps + exit 0 =="
    local log_dir
    log_dir="$(mktemp -d)"

    # If `setsid` is available, use it to drop the controlling tty
    # so /dev/tty becomes unavailable to the child.
    if command -v setsid >/dev/null 2>&1; then
        setsid bash -c '
            set +e
            export GARRAIA_INSTALL_SH_LIBRARY=1
            export GARRAIA_STUB_LOG="'"${log_dir}"'/stub.log"
            . "'"${install_sh}"'"
            INSTALL_PATH="'"${stub}"'"
            bootstrap_phase
            echo "__rc__=$?" >>"'"${log_dir}"'/output.log"
        ' </dev/null >"${log_dir}/output.log" 2>&1 || true

        # On real Linux runners (CI), setsid + </dev/null makes
        # /dev/tty unreadable, so `[ ! -r /dev/tty ]` fires and we print
        # the explicit non-interactive notice. Under WSL/MinGW the
        # readability check sometimes succeeds even though the
        # subsequent `</dev/tty` redirect fails — in that case the
        # wizard runs and exits non-zero, and we fall through to
        # `print_next_steps_legacy` instead. Both outcomes prove the
        # installer does NOT hang or `exec garraia start`, which is
        # what this case ultimately guarantees. Accept either.
        if grep -q "no /dev/tty available" "${log_dir}/output.log"; then
            pass "case (a): prints non-interactive notice"
        elif grep -q "Wizard exited non-zero" "${log_dir}/output.log"; then
            pass "case (a): /dev/tty unusable → wizard fall-through (WSL/MinGW quirk)"
        else
            fail "case (a): neither non-interactive notice nor wizard fall-through observed"
            sed 's/^/    /' "${log_dir}/output.log" >&2 || true
        fi
        assert_log_contains "case (a): prints legacy Next steps" \
            "Next steps:" "${log_dir}/output.log"
        assert_log_contains "case (a): exits 0" "__rc__=0" "${log_dir}/output.log"
        # The stub log may or may not exist depending on which sub-path
        # fired; assert that `start` was never invoked either way.
        if [ -f "${log_dir}/stub.log" ]; then
            assert_log_absent "case (a): start never invoked" \
                "start" "${log_dir}/stub.log"
        else
            pass "case (a): stub log never created"
        fi
    else
        echo "  SKIP: setsid not available — relying on case (b) for next-steps coverage"
    fi
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

case_a_no_tty
case_b_both_skip
case_c_skip_init_only
case_d_skip_start_only
case_e_default
case_f_init_fails

echo ""
echo "==============================================="
echo "bootstrap_phase.sh — pass: ${results_pass}, fail: ${results_fail}"
echo "==============================================="
if [ "${results_fail}" -ne 0 ]; then
    exit 1
fi
