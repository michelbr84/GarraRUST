#!/bin/sh
# SC2030/SC2031 are expected here, not bugs: every case builds a throwaway
# environment inside a subshell (env vars + shadowed uname) and the export's
# scope ending with that subshell is the isolation the test relies on.
# shellcheck disable=SC2030,SC2031
# Unit tests for `detect_platform` (Termux branch) and the Android paths of
# `install_binary` / `print_next_steps_legacy` in `install.sh`.
#
# Garra Mobile Fase 0 (ADR 0016): inside Termux the installer must pick the
# `garraia-android-aarch64` asset. Android reports `uname -s` = Linux, so
# without the Termux branch the installer would download the glibc linux
# binary and die in the loader. Detection is $TERMUX_VERSION (exported by
# every Termux shell) or a *com.termux* $PREFIX.
#
# Strategy mirrors check_glibc.sh: source install.sh with
# GARRAIA_INSTALL_SH_LIBRARY=1 so main() does not run, then invoke
# `detect_platform` in subshells where `uname` is shadowed by a function
# printing the case's OS/arch and the Termux environment is simulated.
# Written in POSIX sh (like checksum_format.sh) so it stays in the
# static-analysis step of the installer job, not just `bash -n`.
set -eu

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
install_sh="${repo_root}/install.sh"

export GARRAIA_INSTALL_SH_LIBRARY=1
# shellcheck source=/dev/null
. "${install_sh}"

results_pass=0
results_fail=0

pass() { echo "  PASS: $1"; results_pass=$((results_pass + 1)); }
fail() { echo "  FAIL: $1" >&2; results_fail=$((results_fail + 1)); }

# run_platform_case <label> <stub_os> <stub_arch> <termux_version> <prefix>
#                   <want_os_name> <want_artifact>
# Runs detect_platform in a subshell with `uname` shadowed to print the
# given OS/arch and the Termux environment variables set to the given
# values (empty string = unset/empty). Expects success; asserts OS_NAME and
# ARTIFACT.
run_platform_case() {
    label="$1"
    stub_os="$2"
    stub_arch="$3"
    termux_version="$4"
    prefix="$5"
    want_os_name="$6"
    want_artifact="$7"
    out_file="$(mktemp)"
    rc=0
    (
        export TERMUX_VERSION="${termux_version}"
        export PREFIX="${prefix}"
        eval "uname() { case \"\$1\" in -s) printf '%s\n' '${stub_os}' ;; -m) printf '%s\n' '${stub_arch}' ;; esac; }"
        detect_platform
        echo "OS_NAME=${OS_NAME}"
        echo "ARTIFACT=${ARTIFACT}"
    ) >"${out_file}" 2>&1 || rc=$?
    if [ "${rc}" -ne 0 ]; then
        fail "${label} — expected exit 0 got ${rc}: $(cat "${out_file}")"
    elif ! grep -qx "OS_NAME=${want_os_name}" "${out_file}"; then
        fail "${label} — expected OS_NAME=${want_os_name}: $(cat "${out_file}")"
    elif ! grep -qx "ARTIFACT=${want_artifact}" "${out_file}"; then
        fail "${label} — expected ARTIFACT=${want_artifact}: $(cat "${out_file}")"
    else
        pass "${label}"
    fi
    rm -f "${out_file}"
}

# run_error_case <label> <stub_os> <stub_arch> <termux_version> <prefix>
#                <expect_stderr_substring>
# Same subshell setup, but expects detect_platform to fail (exit 1).
run_error_case() {
    label="$1"
    stub_os="$2"
    stub_arch="$3"
    termux_version="$4"
    prefix="$5"
    want_substr="$6"
    out_file="$(mktemp)"
    rc=0
    (
        export TERMUX_VERSION="${termux_version}"
        export PREFIX="${prefix}"
        eval "uname() { case \"\$1\" in -s) printf '%s\n' '${stub_os}' ;; -m) printf '%s\n' '${stub_arch}' ;; esac; }"
        detect_platform
    ) >"${out_file}" 2>&1 || rc=$?
    if [ "${rc}" -ne 1 ]; then
        fail "${label} — expected exit 1 got ${rc}"
    elif ! grep -q "${want_substr}" "${out_file}"; then
        fail "${label} — output missing [${want_substr}]: $(cat "${out_file}")"
    else
        pass "${label}"
    fi
    rm -f "${out_file}"
}

echo "== detect_platform: Termux branch =="

# --- Termux detection wins over uname ----------------------------------------
run_platform_case "termux via TERMUX_VERSION" \
    Linux aarch64 "0.119.0" "" \
    android "garraia-android-aarch64"
run_platform_case "termux via *com.termux* PREFIX" \
    Linux aarch64 "" "/data/data/com.termux/files/usr" \
    android "garraia-android-aarch64"
run_platform_case "termux precedence over foreign uname" \
    SunOS aarch64 "0.119.0" "/data/data/com.termux/files/usr" \
    android "garraia-android-aarch64"

# --- non-Termux flow unchanged ------------------------------------------------
run_platform_case "plain linux x86_64" \
    Linux x86_64 "" "/usr" \
    linux "garraia-linux-x86_64"
run_platform_case "plain linux aarch64" \
    Linux aarch64 "" "/usr" \
    linux "garraia-linux-aarch64"
run_platform_case "plain macos aarch64" \
    Darwin arm64 "" "/usr" \
    macos "garraia-macos-aarch64"

# --- errors keep failing outside Termux ----------------------------------------
run_error_case "unsupported OS rejected" \
    SunOS x86_64 "" "" \
    "Unsupported OS: SunOS"
run_error_case "unsupported arch rejected" \
    Linux riscv64 "" "" \
    "Unsupported architecture: riscv64"

echo ""
echo "== detect_platform: install_binary on Termux =="

tmp_root="$(mktemp -d)"
fake_artifact="${tmp_root}/garraia-android-aarch64"
printf '#!/bin/sh\nexit 0\n' > "${fake_artifact}"

# Default install dir on Termux is $PREFIX/bin (in PATH, writable, no sudo).
fake_prefix="${tmp_root}/prefix"
rc=0
(
    export OS_NAME="android"
    export PREFIX="${fake_prefix}"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR=""
    export VERSION="vTEST"
    install_binary
) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -ne 0 ]; then
    fail "install_binary default \$PREFIX/bin — exit ${rc}"
elif [ -x "${fake_prefix}/bin/garraia" ]; then
    pass "install_binary default \$PREFIX/bin"
else
    fail "install_binary default \$PREFIX/bin — binary not at ${fake_prefix}/bin/garraia"
fi

# $GARRAIA_INSTALL_DIR still wins over the Termux default.
custom_dir="${tmp_root}/custom"
rc=0
(
    export OS_NAME="android"
    export PREFIX="${fake_prefix}"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR="${custom_dir}"
    export VERSION="vTEST"
    install_binary
) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -ne 0 ]; then
    fail "install_binary honors GARRAIA_INSTALL_DIR on android — exit ${rc}"
elif [ -x "${custom_dir}/garraia" ]; then
    pass "install_binary honors GARRAIA_INSTALL_DIR on android"
else
    fail "install_binary honors GARRAIA_INSTALL_DIR on android — binary not at ${custom_dir}/garraia"
fi

# The system-path refusal gate is intact on Android too.
gate_out="$(mktemp)"
rc=0
(
    export OS_NAME="android"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR="/usr/bin"
    export VERSION="vTEST"
    install_binary
) >"${gate_out}" 2>&1 || rc=$?
if [ "${rc}" -ne 1 ]; then
    fail "install_binary refuses /usr/bin on android — exit ${rc}"
elif grep -q "refuses to write to system path" "${gate_out}"; then
    pass "install_binary refuses /usr/bin on android"
else
    fail "install_binary refuses /usr/bin on android — output: $(cat "${gate_out}")"
fi
rm -f "${gate_out}"

echo ""
echo "== detect_platform: garra-mcp-server wrapper (issue #909) =="

# The wrapper is what makes `garraia mcp-server` reachable from an external MCP
# host: hosts spawn with a filtered environment, LD_PRELOAD is dropped, and the
# ELF exec then fails inside Termux before the binary runs. Nothing inside
# `garraia` can recover from that, so the contract lives here.
wrapper_dir="${tmp_root}/wrapper"
rc=0
(
    export OS_NAME="android"
    export PREFIX="${fake_prefix}"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR="${wrapper_dir}"
    export VERSION="vTEST"
    install_binary
) >/dev/null 2>&1 || rc=$?
wrapper="${wrapper_dir}/garra-mcp-server"
if [ "${rc}" -ne 0 ]; then
    fail "install_binary writes the MCP wrapper on android — exit ${rc}"
elif [ -x "${wrapper}" ]; then
    pass "install_binary writes the MCP wrapper on android"
else
    fail "install_binary writes the MCP wrapper on android — missing ${wrapper}"
fi

# Shebang must be an absolute Termux path: /usr/bin/env does not exist in
# Termux, which is the same breakage the wrapper works around.
if [ -f "${wrapper}" ] && head -1 "${wrapper}" | grep -q "^#!${fake_prefix}/bin/sh$"; then
    pass "MCP wrapper shebang is an absolute Termux path"
else
    fail "MCP wrapper shebang is an absolute Termux path — got: $(head -1 "${wrapper}" 2>/dev/null)"
fi

# The three load-bearing lines: the shim export, the guard that keeps the
# wrapper usable without termux-exec, and the exec into the real binary.
if grep -q 'libtermux-exec.so' "${wrapper}" 2>/dev/null; then
    pass "MCP wrapper exports the termux-exec shim"
else
    fail "MCP wrapper exports the termux-exec shim"
fi
if grep -q 'exec "'"${wrapper_dir}"'/garraia" mcp-server' "${wrapper}" 2>/dev/null; then
    pass "MCP wrapper execs the installed binary"
else
    fail "MCP wrapper execs the installed binary — got: $(grep '^exec' "${wrapper}" 2>/dev/null)"
fi

# A caller-supplied LD_PRELOAD is never clobbered by the wrapper.
if grep -q 'LD_PRELOAD:-' "${wrapper}" 2>/dev/null; then
    pass "MCP wrapper leaves an inherited LD_PRELOAD alone"
else
    fail "MCP wrapper leaves an inherited LD_PRELOAD alone"
fi

# Reinstalling must not append or fail — the installer is re-run on upgrades.
rc=0
(
    export OS_NAME="android"
    export PREFIX="${fake_prefix}"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR="${wrapper_dir}"
    export VERSION="vTEST"
    install_binary
) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ "$(grep -c '^exec ' "${wrapper}")" -eq 1 ]; then
    pass "MCP wrapper install is idempotent"
else
    fail "MCP wrapper install is idempotent — exit ${rc}, exec lines $(grep -c '^exec ' "${wrapper}")"
fi

# Off Android the wrapper must not exist: it is meaningless anywhere the exec
# path is not Termux's, and a stray file on PATH is worse than none.
linux_dir="${tmp_root}/linux"
rc=0
(
    export OS_NAME="linux"
    export ARTIFACT="garraia-android-aarch64"
    export GARRAIA_TMPDIR="${tmp_root}"
    export GARRAIA_INSTALL_DIR="${linux_dir}"
    export VERSION="vTEST"
    install_binary
) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -eq 0 ] && [ ! -e "${linux_dir}/garra-mcp-server" ]; then
    pass "MCP wrapper is not installed off android"
else
    fail "MCP wrapper is not installed off android — exit ${rc}"
fi

echo ""
echo "== detect_platform: garra-mcp-server-linker wrapper (issue #920) =="

# Issue #920 reported two distinct failures under `env -i PATH=... HOME=...`,
# and the LD_PRELOAD wrapper above only covers one of them:
#
#   A. the host cannot exec the wrapper *script* ("failed to run command")
#   B. the script runs and the inner exec of the ELF fails
#
# This second wrapper covers B by handing the ELF to the Android loader, which
# needs neither termux-exec nor LD_PRELOAD. A is unfixable from a script and is
# answered by documentation (`command: /system/bin/linker64`) instead.
#
# install_binary already ran three times above (twice into wrapper_dir for the
# idempotency check, once into linux_dir), so the artifacts to assert on exist.
linker_wrapper="${wrapper_dir}/garra-mcp-server-linker"
if [ -x "${linker_wrapper}" ]; then
    pass "install_binary writes the loader wrapper on android"
else
    fail "install_binary writes the loader wrapper on android — missing ${linker_wrapper}"
fi

# Same shebang contract as the sibling wrapper: /usr/bin/env is absent in Termux.
if [ -f "${linker_wrapper}" ] && head -1 "${linker_wrapper}" | grep -q "^#!${fake_prefix}/bin/sh$"; then
    pass "loader wrapper shebang is an absolute Termux path"
else
    fail "loader wrapper shebang is an absolute Termux path — got: $(head -1 "${linker_wrapper}" 2>/dev/null)"
fi

# Both loader paths are tried: /system/bin/linker64 exists on every Android,
# and the apex path is the one Android 10+ actually resolves through.
if grep -q '/system/bin/linker64' "${linker_wrapper}" 2>/dev/null &&
    grep -q '/apex/com.android.runtime/bin/linker64' "${linker_wrapper}" 2>/dev/null; then
    pass "loader wrapper tries both linker64 locations"
else
    fail "loader wrapper tries both linker64 locations"
fi

# The load-bearing line: the loader is the argv[0], the installed binary its
# first argument. Getting this backwards is the whole bug the issue reported.
# shellcheck disable=SC2016  # $garra_linker is literal in the generated wrapper
if grep -q 'exec "$garra_linker" "'"${wrapper_dir}"'/garraia" mcp-server' "${linker_wrapper}" 2>/dev/null; then
    pass "loader wrapper execs the installed binary through the loader"
else
    fail "loader wrapper execs the installed binary through the loader — got: $(grep 'garra_linker' "${linker_wrapper}" 2>/dev/null | tail -1)"
fi

# It must not export LD_PRELOAD: the entire point is that the loader path does
# not need the shim. If this ever starts matching, the two wrappers collapsed
# into one and the fallback for failure mode B is gone.
if grep -q 'libtermux-exec.so' "${linker_wrapper}" 2>/dev/null; then
    fail "loader wrapper does not depend on the termux-exec shim"
else
    pass "loader wrapper does not depend on the termux-exec shim"
fi

# Reinstall is idempotent, and the only column-0 exec is the degrade path —
# the loader exec lives inside the `for` loop. Same assertion shape as the
# sibling wrapper so a future edit that merges the two files trips both.
if [ "$(grep -c '^exec ' "${linker_wrapper}")" -eq 1 ]; then
    pass "loader wrapper install is idempotent"
else
    fail "loader wrapper install is idempotent — exec lines $(grep -c '^exec ' "${linker_wrapper}")"
fi

# Off Android it must not exist, for the same reason as the sibling wrapper.
if [ ! -e "${linux_dir}/garra-mcp-server-linker" ]; then
    pass "loader wrapper is not installed off android"
else
    fail "loader wrapper is not installed off android"
fi

echo ""
echo "== detect_platform: android notices and glibc skip =="

# The Termux advisory only prints on android.
notice_out="$(OS_NAME="android"; print_next_steps_legacy)"
case "${notice_out}" in
    *"phantom process"*) pass "next-steps prints Termux advisory on android" ;;
    *) fail "next-steps prints Termux advisory on android" ;;
esac
notice_out="$(OS_NAME="linux"; print_next_steps_legacy)"
case "${notice_out}" in
    *"phantom process"*) fail "next-steps stays silent on linux" ;;
    *) pass "next-steps stays silent on linux" ;;
esac

# check_glibc is a linux-only probe: on android it must return 0 before
# touching ldd — Termux has no glibc/musl loader to interrogate.
rc=0
( export OS_NAME="android" ARCH="aarch64"; check_glibc ) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
    pass "check_glibc skipped on android"
else
    fail "check_glibc skipped on android — exit ${rc}"
fi

rm -r "${tmp_root}"

echo ""
echo "detect_platform.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
