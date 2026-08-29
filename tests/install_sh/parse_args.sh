#!/usr/bin/env bash
# Unit tests for `parse_args` in `install.sh`.
#
# `parse_args` exists so the installer can be driven through a pipe:
# `curl … | sh` cannot carry environment variables into the piped shell, but
# `curl … | sh -s -- --skip-setup` can. That is the invocation form
# third-party launchers use (Ollama's `ollama launch` runs the analogous
# `… | bash -s -- --skip-setup` for other agents), so the flags are a
# contract, not a convenience.
#
# Strategy mirrors check_glibc.sh: source install.sh with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() never runs, then invoke `parse_args`
# in subshells and inspect the environment it produced.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"

export GARRAIA_INSTALL_SH_LIBRARY=1
# shellcheck source=/dev/null
. "${install_sh}"

results_pass=0
results_fail=0

pass() { echo "  PASS: $1"; results_pass=$((results_pass + 1)); }
fail() { echo "  FAIL: $1" >&2; results_fail=$((results_fail + 1)); }

# expect_env <label> <var> <expected> [flags...]
# Runs parse_args with <flags> in a clean subshell and compares $<var>.
expect_env() {
    local label="$1" var="$2" expected="$3"; shift 3
    local actual
    actual="$(
        unset GARRAIA_SKIP_INIT GARRAIA_SKIP_START GARRAIA_BOOTSTRAP_LOCAL \
              GARRAIA_VERSION GARRAIA_INSTALL_DIR || true
        parse_args "$@" >/dev/null 2>&1
        eval "printf '%s' \"\${${var}:-<unset>}\""
    )"
    if [ "${actual}" = "${expected}" ]; then
        pass "${label} (${var}=${actual})"
    else
        fail "${label}: expected ${var}=${expected}, got ${actual}"
    fi
}

echo "parse_args.sh: flag → environment mapping"

expect_env "--skip-setup sets SKIP_INIT"   GARRAIA_SKIP_INIT  1 --skip-setup
expect_env "--skip-setup sets SKIP_START"  GARRAIA_SKIP_START 1 --skip-setup
expect_env "--skip-init alone"             GARRAIA_SKIP_INIT  1 --skip-init
expect_env "--skip-init leaves START"      GARRAIA_SKIP_START "<unset>" --skip-init
expect_env "--skip-start alone"            GARRAIA_SKIP_START 1 --skip-start
expect_env "--no-local"                    GARRAIA_BOOTSTRAP_LOCAL 0 --no-local
expect_env "--skip-local alias"            GARRAIA_BOOTSTRAP_LOCAL 0 --skip-local

echo "parse_args.sh: flags taking a value"

expect_env "--version <tag>"        GARRAIA_VERSION v0.3.4 --version v0.3.4
expect_env "--version=<tag>"        GARRAIA_VERSION v0.3.4 --version=v0.3.4
expect_env "--install-dir <dir>"    GARRAIA_INSTALL_DIR /opt/bin --install-dir /opt/bin
expect_env "--install-dir=<dir>"    GARRAIA_INSTALL_DIR /opt/bin --install-dir=/opt/bin

echo "parse_args.sh: combinations and no-ops"

expect_env "no flags leaves SKIP_INIT unset" GARRAIA_SKIP_INIT "<unset>"
expect_env "combined flags (init)"  GARRAIA_SKIP_INIT 1        --skip-setup --version v1.2.3
expect_env "combined flags (ver)"   GARRAIA_VERSION   v1.2.3   --skip-setup --version v1.2.3

echo "parse_args.sh: an explicit env var wins over its flag"

# Regression guard: existing automation sets the env var directly; a flag
# must never clobber a caller's explicit choice.
actual="$(
    GARRAIA_VERSION=v9.9.9 parse_args --version v0.0.1 >/dev/null 2>&1
    printf '%s' "${GARRAIA_VERSION}"
)"
if [ "${actual}" = "v9.9.9" ]; then
    pass "env GARRAIA_VERSION beats --version (${actual})"
else
    fail "env precedence: expected v9.9.9, got ${actual}"
fi

echo "parse_args.sh: errors and help"

# --help must exit 0 and describe --skip-setup.
help_out="$(parse_args --help 2>&1)" && help_rc=0 || help_rc=$?
if [ "${help_rc}" -eq 0 ] && printf '%s' "${help_out}" | grep -q -- "--skip-setup"; then
    pass "--help exits 0 and documents --skip-setup"
else
    fail "--help: rc=${help_rc}, output missing --skip-setup"
fi

# An unknown flag must fail loudly rather than being silently ignored —
# otherwise a typo'd `--skip-setpu` would run the interactive wizard inside
# a launcher that cannot answer prompts.
if ( parse_args --definitely-not-a-flag ) >/dev/null 2>&1; then
    fail "unknown flag should have exited non-zero"
else
    pass "unknown flag exits non-zero"
fi

# A value-taking flag with no value must not silently swallow the next stage.
if ( parse_args --version ) >/dev/null 2>&1; then
    fail "--version with no value should have exited non-zero"
else
    pass "--version with no value exits non-zero"
fi

echo
echo "parse_args.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
