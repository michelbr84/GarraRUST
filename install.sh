#!/bin/sh
# GarraIA installer — https://github.com/michelbr84/GarraRUST
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
#
# Plan 0127 (PR-B, 2026-05-14): after install_binary the installer
# auto-runs `garraia init` and `garraia start` when a TTY is available.
# In true non-interactive contexts (docker build, pure CI) it prints
# the legacy "Next steps" message and exits 0 instead.
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
#   GARRAIA_SKIP_INIT=1     Skip the auto-run of `garraia init`.
#   GARRAIA_SKIP_START=1    Skip the auto-run of `garraia start`.
#                           Both set together → installer prints next-steps
#                           and exits like the pre-PR-B behavior.
#   GARRAIA_BOOTSTRAP_LOCAL=0
#                           Forwarded to `garraia init` — suppresses the
#                           GPU/Ollama/Qwen3 prompts even on a machine
#                           with a working `nvidia-smi`. See plan 0126.
#
#   GARRAIA_INSTALL_SH_LIBRARY=1
#                           Test-only. When set, the script returns
#                           instead of calling main(), so its functions
#                           can be sourced for unit testing. See
#                           `tests/install_sh/bootstrap_phase.sh`.
set -eu

REPO="michelbr84/GarraRUST"
BINARY="garraia"

main() {
    detect_platform
    resolve_version
    download_and_verify
    install_binary
    bootstrap_phase
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "${OS}" in
        Linux)  OS_NAME="linux" ;;
        Darwin) OS_NAME="macos" ;;
        *)      error "Unsupported OS: ${OS}. Only Linux and macOS are supported." ;;
    esac

    case "${ARCH}" in
        x86_64|amd64)  ARCH_NAME="x86_64" ;;
        aarch64|arm64) ARCH_NAME="aarch64" ;;
        *)             error "Unsupported architecture: ${ARCH}" ;;
    esac

    # release.yml emits `garraia-{linux,macos}-aarch64` from v0.2.1 onwards
    # (aligned with `std::env::consts::ARCH` consumed by garraia-cli's
    # update command). No remapping needed.
    ARTIFACT="${BINARY}-${OS_NAME}-${ARCH_NAME}"
    echo "Detected platform: ${OS_NAME}-${ARCH_NAME}"
}

resolve_version() {
    if [ -n "${GARRAIA_VERSION:-}" ]; then
        VERSION="${GARRAIA_VERSION}"
        echo "Using pinned version: ${VERSION}"
        return
    fi

    # Try /releases/latest first (returns 404 when only prereleases exist).
    VERSION="$(github_api "https://api.github.com/repos/${REPO}/releases/latest" \
        | extract_tag_name || true)"

    # Fall back to the most recent non-draft release (includes prereleases).
    if [ -z "${VERSION}" ]; then
        VERSION="$(github_api "https://api.github.com/repos/${REPO}/releases" \
            | tr ',' '\n' \
            | extract_first_non_draft_tag || true)"
    fi

    if [ -z "${VERSION}" ]; then
        error "Failed to resolve latest release. Set GARRAIA_VERSION=vX.Y.Z to pin."
    fi

    echo "Latest version: ${VERSION}"
}

github_api() {
    # Surface curl error code while keeping stderr quiet in success path.
    curl -fsSL -H "Accept: application/vnd.github+json" "$1"
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
    if ! curl -fsSL "${BASE_URL}/${ARTIFACT}" -o "${GARRAIA_TMPDIR}/${ARTIFACT}"; then
        error "Failed to download ${ARTIFACT} from ${BASE_URL}. The release may not include this platform yet."
    fi

    echo "Downloading SHA256SUMS..."
    if ! curl -fsSL "${BASE_URL}/SHA256SUMS" -o "${GARRAIA_TMPDIR}/SHA256SUMS"; then
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
        # into `<tool> -c -`. The expected format is `<hash>  <filename>`.
        if ! grep "  ${ARTIFACT}\$" SHA256SUMS | ${SHA_TOOL} >/dev/null 2>&1; then
            error "Checksum verification failed for ${ARTIFACT}."
        fi
    )
    echo "Checksum verified."
}

install_binary() {
    # Priority: $GARRAIA_INSTALL_DIR > ~/.local/bin (if in PATH) > /usr/local/bin
    if [ -n "${GARRAIA_INSTALL_DIR:-}" ]; then
        case "${GARRAIA_INSTALL_DIR}" in
            /bin*|/sbin*|/usr/bin*|/usr/sbin*|/etc*)
                error "GARRAIA_INSTALL_DIR refuses to write to system path: ${GARRAIA_INSTALL_DIR}"
                ;;
        esac
        INSTALL_DIR="${GARRAIA_INSTALL_DIR}"
    elif echo "${PATH}" | tr ':' '\n' | grep -qx "${HOME}/.local/bin"; then
        INSTALL_DIR="${HOME}/.local/bin"
    else
        INSTALL_DIR="/usr/local/bin"
    fi

    mkdir -p "${INSTALL_DIR}"
    INSTALL_PATH="${INSTALL_DIR}/${BINARY}"

    if [ "${INSTALL_DIR}" = "/usr/local/bin" ] && [ "$(id -u)" -ne 0 ]; then
        echo "Installing to ${INSTALL_DIR} (requires sudo to copy ${ARTIFACT} → ${INSTALL_PATH})..."
        sudo cp "${GARRAIA_TMPDIR}/${ARTIFACT}" "${INSTALL_PATH}"
        sudo chmod +x "${INSTALL_PATH}"
    else
        cp "${GARRAIA_TMPDIR}/${ARTIFACT}" "${INSTALL_PATH}"
        chmod +x "${INSTALL_PATH}"
    fi

    echo ""
    echo "GarraIA ${VERSION} installed to ${INSTALL_PATH}"
}

# Plan 0127 — interactive bootstrap after install_binary.
#
# Decision logic:
#   * both GARRAIA_SKIP_INIT=1 and GARRAIA_SKIP_START=1 → print legacy
#     "Next steps" hint and return (preserves prior behavior).
#   * /dev/tty not readable → true non-interactive context (docker build,
#     pure CI, no controlling terminal). Print the same legacy hint and
#     exit 0; never hang waiting for input.
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

    if [ ! -r /dev/tty ] || [ ! -w /dev/tty ]; then
        echo ""
        echo "Non-interactive install (no /dev/tty available) — skipping wizard + start."
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
        exec "${INSTALL_PATH}" start </dev/tty
    fi

    print_next_steps_legacy
}

print_next_steps_legacy() {
    echo ""
    echo "Next steps:"
    echo "  garraia init    # interactive setup wizard"
    echo "  garraia start   # start the gateway"
}

error() {
    echo "error: $1" >&2
    exit 1
}

# Library mode (plan 0127 §M1.3): when sourced by the test runner with
# GARRAIA_INSTALL_SH_LIBRARY=1, return before main() runs so each
# function can be invoked in isolation. Misuse (setting the env var on
# a non-sourced execution) errors out — the variable is test-only.
if [ "${GARRAIA_INSTALL_SH_LIBRARY:-}" = "1" ]; then
    return 0
fi

main
