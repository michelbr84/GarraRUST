#!/bin/sh
# GarraIA installer — https://github.com/michelbr84/GarraRUST
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
#
# If raw.githubusercontent.com answers 429 (per-IP rate limit — common on
# cloud pods whose egress IP is shared by many users), the same script is
# published through two alternative channels:
#   curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh
#   curl -fsSL https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh | sh
#
# Plan 0127 (PR-B, 2026-05-14): after install_binary the installer
# auto-runs `garraia init` and `garraia start` when a TTY is available.
# In true non-interactive contexts (docker build, pure CI: /dev/tty cannot
# be opened, or CI is set -- see has_usable_tty) it prints the legacy
# "Next steps" message and exits 0 instead.
#
# Optional environment variables:
#   GARRAIA_VERSION         Pin a specific release tag (e.g. v0.1.0-beta).
#                           When set, the GitHub API is not queried.
#                           Caveat: pins <= v0.2.0 will 404 on Apple Silicon
#                           (macOS arm64). Pre-v0.2.1 releases published
#                           `garraia-macos-arm64`; v0.2.1+ aligned with
#                           `std::env::consts::ARCH` and emit
#                           `garraia-macos-aarch64`. This installer only
#                           constructs the `aarch64` asset name. Use
#                           v0.2.1+ on M-series Macs.
#   GARRAIA_INSTALL_DIR     Override install directory. Must NOT be a
#                           system path (/bin, /sbin, /usr/bin, /usr/sbin, /etc).
#
# Android (Termux): when run inside Termux ($TERMUX_VERSION set, or a
# *com.termux* $PREFIX), the installer picks the garraia-android-aarch64
# asset (bionic, built by release.yml's build-android-arm64 job) and
# installs to $PREFIX/bin — no sudo on Android. Requires official Termux
# builds (F-Droid/GitHub). See docs/adr/0016.
#
# It also drops a second file next to it, $PREFIX/bin/garra-mcp-server: a
# wrapper that exports LD_PRELOAD=$PREFIX/lib/libtermux-exec.so before
# exec'ing `garraia mcp-server`. External MCP hosts spawn their servers with
# a filtered environment (`env -i PATH=... HOME=...`), which strips
# LD_PRELOAD — and without it the ELF exec fails inside Termux before the
# binary ever runs, so no amount of code in `garraia` itself can recover.
# Issue #909. This wrapper deliberately has NO install.ps1 mirror (rule 16):
# it exists only to work around Termux's exec path, which has no Windows
# analogue. See install.ps1's matching note.
#
# A third file, $PREFIX/bin/garra-mcp-server-linker, execs the ELF through
# /system/bin/linker64 instead. Issue #920 showed the LD_PRELOAD wrapper still
# loses when the inner exec is the thing that fails; the loader maps the binary
# with no shim and no LD_PRELOAD at all. It shares the rule 16 waiver above:
# both wrappers are Termux exec-path workarounds with no Windows analogue.
#
# On every platform the installer also leaves $INSTALL_DIR/garra, a relative
# symlink to `garraia` (issue #1328). The CLI's own hints, the wiki and the
# README quick start all say `garra ...` -- the `[[bin]]` name a cargo build
# produces -- while the published asset is `garraia`. Both names work after
# an install. install.ps1 mirrors this with a `garra.cmd` shim (rule 16).
#
#   GARRAIA_SKIP_INIT=1     Skip the auto-run of `garraia init`.
#   GARRAIA_SKIP_START=1    Skip the auto-run of `garraia start`.
#                           Both set together → installer prints next-steps
#                           and exits like the pre-PR-B behavior.
#   GARRAIA_BOOTSTRAP_LOCAL=0
#                           Forwarded to `garraia init` — suppresses the
#                           GPU/Ollama/Qwen3 prompts even on a machine
#                           with a working `nvidia-smi`. See plan 0126.
#
# Command-line flags (usable through a pipe via `sh -s -- <flags>`):
#   --skip-setup            Same as GARRAIA_SKIP_INIT=1 GARRAIA_SKIP_START=1.
#   --skip-init             Same as GARRAIA_SKIP_INIT=1.
#   --skip-start            Same as GARRAIA_SKIP_START=1.
#   --no-local              Same as GARRAIA_BOOTSTRAP_LOCAL=0.
#   --version <tag>         Same as GARRAIA_VERSION=<tag>.
#   --install-dir <dir>     Same as GARRAIA_INSTALL_DIR=<dir>.
#   -h, --help              Print usage and exit 0.
# An env var already set by the caller wins over the matching flag.
#
#   GARRAIA_INSTALL_SH_LIBRARY=1
#                           Test-only. When set, the script returns
#                           instead of calling main(), so its functions
#                           can be sourced for unit testing. See
#                           `tests/install_sh/bootstrap_phase.sh`.
set -eu

REPO="michelbr84/GarraRUST"
BINARY="garraia"
# The short name: what a cargo build produces and what every `garra ...` hint
# in the CLI, the wiki and the README refers to. Installed as a symlink to
# ${BINARY}, never as a second copy (see install_garra_alias).
ALIAS="garra"

# Every GitHub fetch goes through this wrapper. curl treats HTTP 408/429/5xx
# as transient under --retry (curl >= 7.66), which matters on cloud pods:
# their shared egress IPs exhaust GitHub's per-IP rate limits constantly.
curl_gh() {
    curl -fsSL --retry 5 --retry-delay 2 "$@"
}

# Parse CLI flags. Every flag is an alias for an environment variable that
# already existed, so `sh install.sh --skip-setup` and
# `GARRAIA_SKIP_INIT=1 GARRAIA_SKIP_START=1 sh install.sh` are equivalent.
#
# Flags matter because `curl … | sh` gives no way to set an env var for the
# piped shell; `curl … | sh -s -- --skip-setup` does. That is exactly the
# invocation form third-party launchers use (Ollama's `ollama launch` runs
# the analogous `… | bash -s -- --skip-setup` for other agents), so a
# flagless installer cannot be driven by one.
#
# An explicit env var already set by the caller wins over its flag, so
# existing automation keeps its behaviour.
parse_args() {
    while [ "$#" -gt 0 ]; do
        case "$1" in
            --skip-setup)
                GARRAIA_SKIP_INIT="${GARRAIA_SKIP_INIT:-1}"
                GARRAIA_SKIP_START="${GARRAIA_SKIP_START:-1}"
                export GARRAIA_SKIP_INIT GARRAIA_SKIP_START
                ;;
            --skip-init)
                GARRAIA_SKIP_INIT="${GARRAIA_SKIP_INIT:-1}"; export GARRAIA_SKIP_INIT ;;
            --skip-start)
                GARRAIA_SKIP_START="${GARRAIA_SKIP_START:-1}"; export GARRAIA_SKIP_START ;;
            --no-local|--skip-local)
                GARRAIA_BOOTSTRAP_LOCAL="${GARRAIA_BOOTSTRAP_LOCAL:-0}"
                export GARRAIA_BOOTSTRAP_LOCAL
                ;;
            --version)
                [ "$#" -ge 2 ] || error "--version requires a release tag (e.g. --version v0.3.4)"
                GARRAIA_VERSION="${GARRAIA_VERSION:-$2}"; export GARRAIA_VERSION; shift ;;
            --version=*)
                GARRAIA_VERSION="${GARRAIA_VERSION:-${1#--version=}}"; export GARRAIA_VERSION ;;
            --install-dir)
                [ "$#" -ge 2 ] || error "--install-dir requires a path"
                GARRAIA_INSTALL_DIR="${GARRAIA_INSTALL_DIR:-$2}"; export GARRAIA_INSTALL_DIR; shift ;;
            --install-dir=*)
                GARRAIA_INSTALL_DIR="${GARRAIA_INSTALL_DIR:-${1#--install-dir=}}"
                export GARRAIA_INSTALL_DIR ;;
            -h|--help)
                usage; exit 0 ;;
            --)
                shift; break ;;
            *)
                error "unknown option: $1 (try --help)" ;;
        esac
        shift
    done
}

usage() {
    cat <<'USAGE'
GarraIA installer

Usage:
  curl -fsSL https://garraia.org/install.sh | sh
  curl -fsSL https://garraia.org/install.sh | sh -s -- [options]
  sh install.sh [options]

Options:
  --skip-setup          Install only: skip both `garraia init` and `garraia start`.
  --skip-init           Skip the interactive setup wizard.
  --skip-start          Skip starting the gateway.
  --no-local            Skip the GPU/Ollama/local-model prompts in the wizard.
  --version <tag>       Pin a release tag (e.g. v0.3.4) instead of latest.
  --install-dir <dir>   Install into <dir> instead of the autodetected location.
  -h, --help            Show this help.

Every option mirrors an environment variable (GARRAIA_SKIP_INIT,
GARRAIA_SKIP_START, GARRAIA_BOOTSTRAP_LOCAL, GARRAIA_VERSION,
GARRAIA_INSTALL_DIR). An environment variable already set by the caller
takes precedence over the corresponding flag.
USAGE
}

main() {
    parse_args "$@"
    detect_platform
    check_glibc
    resolve_version
    download_and_verify
    install_binary
    bootstrap_phase
}

# Minimum glibc the prebuilt Linux binaries link against. Must stay in sync
# with the `runs-on` of build-linux-x86_64 in .github/workflows/release.yml
# (ubuntu-22.04 => glibc 2.35) and the note in docs/installation.md.
MIN_GLIBC="2.35"

# Fail fast with an actionable message instead of the loader's cryptic
# "version `GLIBC_2.39' not found" after the download (the v0.3.2/v0.3.3
# failure mode: binaries were built on ubuntu-latest/24.04). Silently
# skipped when the local libc version cannot be determined.
check_glibc() {
    [ "${OS_NAME}" = "linux" ] || return 0

    if [ -e "/lib/ld-musl-${ARCH}.so.1" ]; then
        error "musl libc detected (Alpine?). Prebuilt GarraIA binaries require glibc >= ${MIN_GLIBC}. Build from source instead: cargo install --git https://github.com/${REPO} garraia"
    fi

    # glibc's ldd prints "ldd (<vendor blurb>) X.YY" on line 1.
    GLIBC_VERSION="$(ldd --version 2>/dev/null | sed -n '1s/.* //p')" || true
    case "${GLIBC_VERSION}" in
        [0-9]*.[0-9]*) ;;
        *) return 0 ;;
    esac

    LOWEST="$(printf '%s\n%s\n' "${MIN_GLIBC}" "${GLIBC_VERSION}" | sort -V | head -n1)"
    if [ "${LOWEST}" != "${MIN_GLIBC}" ]; then
        error "GarraIA prebuilt binaries require glibc >= ${MIN_GLIBC} (Ubuntu 22.04+, Debian 12+); this system has glibc ${GLIBC_VERSION}. Upgrade the distro or build from source: cargo install --git https://github.com/${REPO} garraia"
    fi
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    # Termux tem precedência sobre o uname: o Android reporta `uname -s` =
    # Linux, e sem este ramo o installer baixaria o asset glibc linux e
    # morreria no loader. $TERMUX_VERSION é exportado por todo shell do
    # Termux; o case em $PREFIX cobre ambientes em que só o prefixo sobrevive
    # (su, alguns forks). Fora do Termux o fluxo linux/macos é o de sempre.
    OS_IS_TERMUX=0
    if [ -n "${TERMUX_VERSION:-}" ]; then
        OS_IS_TERMUX=1
    fi
    case "${PREFIX:-}" in
        *com.termux*) OS_IS_TERMUX=1 ;;
    esac

    if [ "${OS_IS_TERMUX}" -eq 1 ]; then
        OS_NAME="android"
    else
        case "${OS}" in
            Linux)  OS_NAME="linux" ;;
            Darwin) OS_NAME="macos" ;;
            *)      error "Unsupported OS: ${OS}. Only Linux, macOS and Android (Termux) are supported." ;;
        esac
    fi

    case "${ARCH}" in
        x86_64|amd64)  ARCH_NAME="x86_64" ;;
        aarch64|arm64) ARCH_NAME="aarch64" ;;
        *)             error "Unsupported architecture: ${ARCH}" ;;
    esac

    # release.yml emits `garraia-{linux,macos,android}-aarch64` from v0.2.1
    # onwards (aligned with `std::env::consts::OS`/`ARCH` consumed by
    # garraia-cli's update command). No remapping needed.
    ARTIFACT="${BINARY}-${OS_NAME}-${ARCH_NAME}"
    echo "Detected platform: ${OS_NAME}-${ARCH_NAME}"
}

resolve_version() {
    if [ -n "${GARRAIA_VERSION:-}" ]; then
        VERSION="${GARRAIA_VERSION}"
        echo "Using pinned version: ${VERSION}"
        return
    fi

    # Preferred path: follow the web redirect of /releases/latest and read the
    # tag out of the final URL. Unlike api.github.com (60 unauthenticated
    # requests/hour per IP — exhausted immediately on shared cloud-pod egress
    # IPs), the github.com web endpoint tolerates far more traffic.
    VERSION="$(resolve_version_from_redirect || true)"

    # Fallback: the REST API — /releases/latest first (404s when only
    # prereleases exist), then the most recent non-draft release.
    if [ -z "${VERSION}" ]; then
        VERSION="$(github_api "https://api.github.com/repos/${REPO}/releases/latest" \
            | extract_tag_name || true)"
    fi

    if [ -z "${VERSION}" ]; then
        VERSION="$(github_api "https://api.github.com/repos/${REPO}/releases" \
            | tr ',' '\n' \
            | extract_first_non_draft_tag || true)"
    fi

    if [ -z "${VERSION}" ]; then
        rate_limit_hint
        error "Failed to resolve latest release. Set GARRAIA_VERSION=vX.Y.Z to pin."
    fi

    echo "Latest version: ${VERSION}"
}

# HEAD-follow the /releases/latest redirect (no body download) and print the
# tag embedded in the final URL. Prints nothing when the redirect does not
# land on a /releases/tag/<tag> URL (e.g. a repo with no full release yet),
# letting resolve_version fall back to the API.
resolve_version_from_redirect() {
    curl_gh -I -o /dev/null -w '%{url_effective}' \
        "https://github.com/${REPO}/releases/latest" \
        | extract_tag_from_release_url
}

# `…/releases/tag/v0.3.0[?query]` → `v0.3.0`; anything else → empty output.
# Kept standalone so it can be unit-tested via GARRAIA_INSTALL_SH_LIBRARY=1
# (see tests/install_sh/resolve_version.sh).
extract_tag_from_release_url() {
    sed -n 's|.*/releases/tag/\([^/?]*\).*|\1|p' | head -1
}

github_api() {
    # Surface curl error code while keeping stderr quiet in success path.
    curl_gh -H "Accept: application/vnd.github+json" "$1"
}

extract_tag_name() {
    grep '"tag_name"' \
        | head -1 \
        | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/'
}

# Filter out lines that declare draft:true, then pick the first tag_name.
# Works on the normalized one-key-per-line stream produced by `tr ',' '\n'`.
extract_first_non_draft_tag() {
    awk '
        /"draft": *true/        { skip_until_next_release = 1; next }
        /"id": *[0-9]+/         { skip_until_next_release = 0 }
        /"tag_name":/ {
            if (!skip_until_next_release) {
                match($0, /"tag_name": *"[^"]*"/)
                tag = substr($0, RSTART, RLENGTH)
                sub(/"tag_name": *"/, "", tag)
                sub(/"$/, "", tag)
                print tag
                exit
            }
        }
    '
}

download_and_verify() {
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
    GARRAIA_TMPDIR="$(mktemp -d 2>/dev/null || mktemp -d -t garraia-install)"
    [ -d "${GARRAIA_TMPDIR}" ] || error "Failed to create temp directory."
    trap 'rm -rf -- "${GARRAIA_TMPDIR}"' EXIT INT TERM

    echo "Downloading ${ARTIFACT} from ${VERSION}..."
    if ! curl_gh "${BASE_URL}/${ARTIFACT}" -o "${GARRAIA_TMPDIR}/${ARTIFACT}"; then
        rate_limit_hint
        error "Failed to download ${ARTIFACT} from ${BASE_URL}. The release may not include this platform yet."
    fi

    echo "Downloading SHA256SUMS..."
    if ! curl_gh "${BASE_URL}/SHA256SUMS" -o "${GARRAIA_TMPDIR}/SHA256SUMS"; then
        rate_limit_hint
        error "Failed to download SHA256SUMS. Cannot verify binary integrity."
    fi

    echo "Verifying checksum..."
    if command -v sha256sum >/dev/null 2>&1; then
        SHA_TOOL="sha256sum -c -"
    elif command -v shasum >/dev/null 2>&1; then
        SHA_TOOL="shasum -a 256 -c -"
    else
        error "No checksum tool (sha256sum or shasum) available. Refusing to install unverified binary."
    fi

    (
        cd "${GARRAIA_TMPDIR}"
        # Extract just the line matching our artifact from SHA256SUMS and pipe it
        # into `<tool> -c -`. SHA256SUMS may be emitted in GNU *text mode*
        # (`<hash>  <filename>`, two spaces) or *binary mode*
        # (`<hash> *<filename>`, one space + asterisk — what `sha256sum -b`
        # and several Windows/Tauri toolchains produce; e.g. the published
        # v0.2.1 release). `sha256sum -c` understands both, but the previous
        # two-space-only grep silently matched nothing on binary-mode files,
        # piping an empty stream into the tool and aborting with a misleading
        # "Checksum verification failed". `select_checksum_line` normalizes
        # CR endings and accepts either separator.
        expected_line="$(select_checksum_line "${ARTIFACT}" SHA256SUMS || true)"
        if [ -z "${expected_line}" ]; then
            error "Checksum verification failed for ${ARTIFACT}: no entry found in SHA256SUMS."
        fi
        if ! printf '%s\n' "${expected_line}" | ${SHA_TOOL} >/dev/null 2>&1; then
            error "Checksum verification failed for ${ARTIFACT}."
        fi
    )
    echo "Checksum verified."
}

# Select the SHA256SUMS line for ${1} from file ${2}, tolerating both the
# two-space text-mode separator and the one-space + `*` binary-mode separator,
# plus stray CR line endings from Windows-generated checksum files. Prints the
# (CR-stripped) matching line to stdout, or nothing if absent. Kept as a
# standalone function so it can be unit-tested via GARRAIA_INSTALL_SH_LIBRARY=1
# (see tests/install_sh/checksum_format.sh).
select_checksum_line() {
    artifact="$1"
    sums_file="$2"
    # The character immediately before the filename is a space (text mode, the
    # second of two) or `*` (binary mode). Anchor the filename to end-of-line so
    # `garraia-linux-x86_64` never matches `garraia-linux-x86_64.sha256` or a
    # longer sibling name.
    tr -d '\r' < "${sums_file}" \
        | grep -E "[ *]${artifact}\$" \
        | head -1
}

install_binary() {
    # Priority:
    #   Android/Termux: $GARRAIA_INSTALL_DIR > $PREFIX/bin
    #   Other OS:       $GARRAIA_INSTALL_DIR > ~/.local/bin (if in PATH) > /usr/local/bin
    if [ -n "${GARRAIA_INSTALL_DIR:-}" ]; then
        case "${GARRAIA_INSTALL_DIR}" in
            /bin*|/sbin*|/usr/bin*|/usr/sbin*|/etc*)
                error "GARRAIA_INSTALL_DIR refuses to write to system path: ${GARRAIA_INSTALL_DIR}"
                ;;
        esac
        INSTALL_DIR="${GARRAIA_INSTALL_DIR}"
    elif [ "${OS_NAME:-}" = "android" ]; then
        # Termux: $PREFIX/bin está no PATH, é gravável sem root e é onde os
        # pacotes do Termux instalam. /usr/local/bin seria criado FORA do
        # PATH (o rootfs do Termux é gravável) — uma armadilha silenciosa.
        INSTALL_DIR="${PREFIX}/bin"
    elif echo "${PATH}" | tr ':' '\n' | grep -qx "${HOME}/.local/bin"; then
        INSTALL_DIR="${HOME}/.local/bin"
    else
        INSTALL_DIR="/usr/local/bin"
    fi

    mkdir -p "${INSTALL_DIR}"
    INSTALL_PATH="${INSTALL_DIR}/${BINARY}"

    if install_needs_sudo; then
        echo "Installing to ${INSTALL_DIR} (requires sudo to copy ${ARTIFACT} → ${INSTALL_PATH})..."
        sudo cp "${GARRAIA_TMPDIR}/${ARTIFACT}" "${INSTALL_PATH}"
        sudo chmod +x "${INSTALL_PATH}"
    else
        cp "${GARRAIA_TMPDIR}/${ARTIFACT}" "${INSTALL_PATH}"
        chmod +x "${INSTALL_PATH}"
    fi

    echo ""
    echo "GarraIA ${VERSION} installed to ${INSTALL_PATH}"

    install_garra_alias
    install_termux_mcp_wrapper
    install_termux_mcp_linker_wrapper
}

# Does writing into ${INSTALL_DIR} need sudo? Only /usr/local/bin as a
# non-root user. Shared by install_binary and install_garra_alias so the
# binary and its alias can never disagree about privileges: a directory that
# needed sudo for the copy needs it for the link too.
install_needs_sudo() {
    [ "${INSTALL_DIR}" = "/usr/local/bin" ] && [ "$(id -u)" -ne 0 ]
}

# Issue #1328. Leaves ${INSTALL_DIR}/garra -> garraia next to the binary.
#
# Why: the CLI prints ~113 hints of the form "run `garra start`", the wiki's
# first steps say `garra init`, and the README quick start uses `garra` too --
# that is the `[[bin]]` name a cargo build produces. The published asset, and
# therefore ${INSTALL_PATH}, is `garraia` (rule 15: the raw asset names are
# frozen because `garra update` resolves them by exact name). Both names are
# legitimate; only the installer left the user without the one every document
# uses. Fix the reality, not the 113 strings.
#
# A symlink, not a copy: `garra update` resolves the link first (update.rs
# canonicalizes `current_exe()` -- on macOS `_NSGetExecutablePath` may hand
# back the link itself, not the file; Linux reads `/proc/self/exe`, which the
# kernel already resolves) and swaps the one binary it points at, so the pair
# cannot drift; a second copy would drift away from the first on the next
# update. Binaries that predate that canonicalization (v0.4.3 and older) wrote
# the new version over the link on macOS -- docs/installation.md tells those
# users to run `garraia update` once. RELATIVE (`garraia`, not
# `${INSTALL_DIR}/garraia`) so the pair survives the directory being moved or
# $HOME being mounted elsewhere.
#
# Decision table -- mirrored one-for-one by Install-GarraAlias in install.ps1:
#   absent                  -> create the link
#   symlink (any target)    -> repoint it at garraia (a stale alias from an
#                              older layout, or a hand-made absolute one)
#   regular file / other    -> keep it and warn. A real file named `garra` is
#                              most likely a from-source build the user copied
#                              by hand; clobbering it would destroy their work.
#
# Every failure here is a warning, never an abort: the binary is already in
# place and usable as `garraia`, and a missing alias must not undo an install.
install_garra_alias() {
    alias_path="${INSTALL_DIR}/${ALIAS}"

    # `-e` follows symlinks, so a dangling link fails it and falls through to
    # the `ln -sfn` below, which replaces it; only a real file gets protected.
    if [ -e "${alias_path}" ] && [ ! -L "${alias_path}" ]; then
        warn "${alias_path} already exists and was not created by this installer - left untouched."
        warn "  The installed binary is ${INSTALL_PATH}; use 'garraia' or replace 'garra' yourself."
        return 0
    fi

    if [ -L "${alias_path}" ]; then
        alias_verb="Repointed alias"
    else
        alias_verb="Alias"
    fi

    # `-n` treats an existing link as the thing to replace even when it points
    # at a directory (GNU, BSD/macOS and Termux coreutils all accept it).
    if install_needs_sudo; then
        if ! sudo ln -sfn "${BINARY}" "${alias_path}"; then
            warn "could not create the 'garra' alias at ${alias_path}; 'garraia' still works."
            return 0
        fi
    elif ! ln -sfn "${BINARY}" "${alias_path}"; then
        warn "could not create the 'garra' alias at ${alias_path}; 'garraia' still works."
        return 0
    fi

    echo "${alias_verb} ${alias_path} -> ${BINARY}"
}

# Android-only (issue #909). Writes $INSTALL_DIR/garra-mcp-server.
#
# Why a wrapper and not a fix inside the binary: an external MCP host spawns
# its servers with a filtered environment (`env -i PATH=... HOME=...` — good
# security hygiene), which drops LD_PRELOAD. On Android the ELF exec resolves
# through the termux-exec shim, so without LD_PRELOAD the exec itself fails
# and `garraia` never starts. There is no point inside the process at which it
# could repair that; the export has to happen in the parent.
#
# The shebang is an absolute Termux path on purpose: /usr/bin/env does not
# exist in Termux, which is the same class of breakage this works around.
# The guard keeps the wrapper working even when termux-exec is not installed
# (`pkg install termux-exec`) — it just degrades to a plain exec.
install_termux_mcp_wrapper() {
    [ "${OS_NAME:-}" = "android" ] || return 0

    termux_prefix="${PREFIX:-/data/data/com.termux/files/usr}"
    wrapper_path="${INSTALL_DIR}/garra-mcp-server"

    cat > "${wrapper_path}" <<WRAPPER
#!${termux_prefix}/bin/sh
# Generated by GarraIA's install.sh — do not edit; reinstalling overwrites it.
# Termux exec shim (issue #909, ADR 0016): MCP hosts filter the environment
# and drop LD_PRELOAD, after which exec'ing an ELF fails inside Termux.
PREFIX="\${PREFIX:-${termux_prefix}}"
if [ -f "\${PREFIX}/lib/libtermux-exec.so" ] && [ -z "\${LD_PRELOAD:-}" ]; then
    LD_PRELOAD="\${PREFIX}/lib/libtermux-exec.so"
    export LD_PRELOAD
fi
exec "${INSTALL_PATH}" mcp-server "\$@"
WRAPPER

    chmod +x "${wrapper_path}"

    echo "MCP stdio wrapper installed to ${wrapper_path}"
    echo "  Point external MCP hosts at this path instead of 'garraia mcp-server'."
}

# Android-only (issue #920). Writes $INSTALL_DIR/garra-mcp-server-linker.
#
# The wrapper above still loses a case. Issue #920 reported two *distinct*
# failures under a fully filtered environment (`env -i PATH=... HOME=...`):
#
#   A. the host cannot exec the wrapper *script* at all
#      ("timeout: failed to run command 'garra-mcp-server': Permission denied")
#   B. the script runs and the inner exec of the ELF fails
#      ("garra-mcp-server: 8: exec: .../garraia: Permission denied")
#
# This second wrapper fixes B: the Android dynamic loader maps the ELF
# directly, so neither the termux-exec shim nor LD_PRELOAD is on the path.
# It cannot fix A, because it is itself a script. For A the host has to invoke
# the loader itself, with no wrapper in between:
#
#   command: /system/bin/linker64
#   args:    ["<INSTALL_PATH>", "mcp-server"]
#
# That recipe is the documented answer (docs/cli-mcp-server.md) and is what
# `garraia doctor` prints. This file exists so the common case is one path.
install_termux_mcp_linker_wrapper() {
    [ "${OS_NAME:-}" = "android" ] || return 0

    termux_prefix="${PREFIX:-/data/data/com.termux/files/usr}"
    linker_wrapper_path="${INSTALL_DIR}/garra-mcp-server-linker"

    cat > "${linker_wrapper_path}" <<LINKERWRAPPER
#!${termux_prefix}/bin/sh
# Generated by GarraIA's install.sh — do not edit; reinstalling overwrites it.
# Android loader fallback (issue #920, ADR 0016): linker64 maps the ELF with no
# termux-exec shim and no LD_PRELOAD, so it survives an MCP host that filters
# the environment down to PATH and HOME.
for garra_linker in /system/bin/linker64 /apex/com.android.runtime/bin/linker64; do
    if [ -x "\$garra_linker" ]; then
        exec "\$garra_linker" "${INSTALL_PATH}" mcp-server "\$@"
    fi
done
# No loader found — degrade to a plain exec instead of doing nothing.
exec "${INSTALL_PATH}" mcp-server "\$@"
LINKERWRAPPER

    chmod +x "${linker_wrapper_path}"

    echo "MCP stdio loader fallback installed to ${linker_wrapper_path}"
    echo "  Use it when the host filters the environment and the plain wrapper fails."
}

# Is there a terminal a human can answer the wizard on?
#
# `[ -r /dev/tty ]` is NOT that test: it only reads the permission bits, and
# /dev/tty is crw-rw-rw- on every Linux box -- including a container or a CI
# runner with no controlling terminal, where open(2) then fails with ENXIO.
# That is how the v0.4.4 clean-install smoke run printed
# "/dev/tty: No such device or address" followed by a bogus
# "Wizard exited non-zero": the probe said yes, the redirect in front of
# `garraia init` said no, and the wizard never even started.
#
# So open the device, exactly as the `</dev/tty` redirects below will, in a
# subshell: a failed redirect then reports through the subshell's stderr,
# which we discard, and cannot abort the `set -e` caller. Read-only on
# purpose -- `<>` and `>` create a missing path, and as root in a chroot with
# a writable /dev that would leave a regular file named /dev/tty behind. The
# `-c` guard rejects anything that is not a character device, so a stray
# regular file there cannot pass for a terminal and feed the wizard EOF.
# (The old `-w` half never meant anything: the node is 0666 on every
# platform this script supports.)
#
# A non-empty CI short-circuits to "no", as Test-InteractiveSession does in
# install.ps1 (rule 16): a CI job that hands the installer a pty still has
# nobody to type into it, and the wizard would block the job forever.
has_usable_tty() {
    [ -z "${CI:-}" ] || return 1
    [ -c /dev/tty ] || return 1
    (: </dev/tty) 2>/dev/null
}

# Plan 0127 — interactive bootstrap after install_binary.
#
# Decision logic:
#   * both GARRAIA_SKIP_INIT=1 and GARRAIA_SKIP_START=1 → print legacy
#     "Next steps" hint and return (preserves prior behavior).
#   * no usable terminal (see has_usable_tty: /dev/tty cannot be opened,
#     or CI is set) → true non-interactive context (docker build, pure CI,
#     no controlling terminal). Print the same legacy hint and exit 0;
#     never hang waiting for input, never attempt the wizard.
#   * otherwise → run `garraia init </dev/tty` unless GARRAIA_SKIP_INIT=1,
#     then `exec garraia start </dev/tty` unless GARRAIA_SKIP_START=1.
#     `exec` is intentional — it replaces the installer shell so Ctrl-C
#     is delivered directly to `garraia start` (the user expects this
#     because we explicitly tell them "Press Ctrl+C to stop").
#
# `INSTALL_PATH` is set by install_binary(); tests pre-populate it
# before calling bootstrap_phase via the library guard.
bootstrap_phase() {
    if [ "${GARRAIA_SKIP_INIT:-}" = "1" ] && [ "${GARRAIA_SKIP_START:-}" = "1" ]; then
        print_next_steps_legacy
        return 0
    fi

    if ! has_usable_tty; then
        echo ""
        if [ -n "${CI:-}" ]; then
            echo "Non-interactive install (CI environment detected) — skipping wizard + start."
        else
            echo "Non-interactive install (no /dev/tty available) — skipping wizard + start."
        fi
        print_next_steps_legacy
        return 0
    fi

    if [ "${GARRAIA_SKIP_INIT:-}" != "1" ]; then
        echo ""
        echo "Running interactive setup wizard..."
        if ! "${INSTALL_PATH}" init </dev/tty; then
            echo ""
            echo "Wizard exited non-zero — your config may need manual edits."
            print_next_steps_legacy
            return 0
        fi
    fi

    if [ "${GARRAIA_SKIP_START:-}" != "1" ]; then
        echo ""
        echo "Starting GarraIA in the foreground. Press Ctrl+C to stop."
        echo "  To run later in background: garraia start -d"
        echo "  Either way, 'garraia status' and 'garraia stop' manage the process."
        print_termux_notice
        exec "${INSTALL_PATH}" start </dev/tty
    fi

    print_next_steps_legacy
}

print_next_steps_legacy() {
    echo ""
    echo "Next steps:"
    echo "  garraia init    # interactive setup wizard"
    echo "  garraia start   # start the gateway"
    echo "  'garra' is an alias for 'garraia' - either name works."
    print_termux_notice
}

# Android-only advisory (Garra Mobile Fase 0, ADR 0016): o Android mata
# processos de longa duração em background (phantom process killer) e
# congela apps agressivamente por bateria. O gateway funciona melhor no
# foreground da sessão Termux em que foi iniciado.
print_termux_notice() {
    [ "${OS_NAME:-}" = "android" ] || return 0
    echo ""
    echo "Termux: Android may kill background processes (phantom process"
    echo "  killer, battery optimization). If the gateway dies in background,"
    echo "  keep the Termux session open, run 'termux-wake-lock' and disable"
    echo "  battery optimization for Termux."
    echo "  For MCP servers run 'pkg install termux-exec' and point external"
    echo "  hosts at ${INSTALL_DIR:-\$PREFIX/bin}/garra-mcp-server."
    echo "  If the host filters the environment and that still fails, point it"
    echo "  straight at the loader (issue #920):"
    echo "    command: /system/bin/linker64"
    echo "    args:    [\"${INSTALL_PATH:-\$PREFIX/bin/garraia}\", \"mcp-server\"]"
}

error() {
    echo "error: $1" >&2
    exit 1
}

# Non-fatal counterpart of error(): report and carry on. Used where the
# install already succeeded and only a nicety failed (the `garra` alias).
warn() {
    echo "warning: $1" >&2
}

# Printed when a GitHub fetch fails even after curl's retries. On shared
# cloud pods (RunPod etc.) HTTP 429 here usually means the pod's egress IP —
# shared by many users — exhausted GitHub's per-IP quota, not that anything
# is wrong on this machine.
rate_limit_hint() {
    echo "" >&2
    echo "If the failure above was HTTP 429 (rate limit): this machine's shared" >&2
    echo "egress IP has exhausted GitHub's per-IP quota. You can:" >&2
    echo "  * retry in a few minutes, or" >&2
    echo "  * fetch the installer from the release CDN instead:" >&2
    echo "      curl -fsSL https://github.com/${REPO}/releases/latest/download/install.sh | sh" >&2
    echo "  * or from the jsDelivr mirror:" >&2
    echo "      curl -fsSL https://cdn.jsdelivr.net/gh/${REPO}@main/install.sh | sh" >&2
}

# Library mode (plan 0127 §M1.3): when sourced by the test runner with
# GARRAIA_INSTALL_SH_LIBRARY=1, return before main() runs so each
# function can be invoked in isolation. Misuse (setting the env var on
# a non-sourced execution) errors out — the variable is test-only.
if [ "${GARRAIA_INSTALL_SH_LIBRARY:-}" = "1" ]; then
    return 0
fi

main "$@"
