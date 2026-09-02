#!/bin/sh
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
        TERMUX_VERSION="${termux_version}"
        PREFIX="${prefix}"
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
        TERMUX_VERSION="${termux_version}"
        PREFIX="${prefix}"
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
    OS_NAME="android"
    PREFIX="${fake_prefix}"
    ARTIFACT="garraia-android-aarch64"
    GARRAIA_TMPDIR="${tmp_root}"
    GARRAIA_INSTALL_DIR=""
    VERSION="vTEST"
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
    OS_NAME="android"
    PREFIX="${fake_prefix}"
    ARTIFACT="garraia-android-aarch64"
    GARRAIA_TMPDIR="${tmp_root}"
    GARRAIA_INSTALL_DIR="${custom_dir}"
    VERSION="vTEST"
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
    OS_NAME="android"
    ARTIFACT="garraia-android-aarch64"
    GARRAIA_TMPDIR="${tmp_root}"
    GARRAIA_INSTALL_DIR="/usr/bin"
    VERSION="vTEST"
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
( OS_NAME="android"; ARCH="aarch64"; check_glibc ) >/dev/null 2>&1 || rc=$?
if [ "${rc}" -eq 0 ]; then
    pass "check_glibc skipped on android"
else
    fail "check_glibc skipped on android — exit ${rc}"
fi

rm -r "${tmp_root}"

echo ""
echo "detect_platform.sh: ${results_pass} passed, ${results_fail} failed"
[ "${results_fail}" -eq 0 ]
