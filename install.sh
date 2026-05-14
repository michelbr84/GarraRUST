#!/bin/sh
# GarraIA installer — https://github.com/michelbr84/GarraRUST
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
#
# Optional environment variables:
#   GARRAIA_VERSION       Pin a specific release tag (e.g. v0.1.0-beta). When set,
#                         the GitHub API is not queried.
#   GARRAIA_INSTALL_DIR   Override install directory. Must NOT be a system path
#                         (/bin, /sbin, /usr/bin, /usr/sbin, /etc).
set -eu

REPO="michelbr84/GarraRUST"
BINARY="garraia"

main() {
    detect_platform
    resolve_version
    download_and_verify
    install_binary
    echo ""
    echo "GarraIA ${VERSION} installed to ${INSTALL_PATH}"
    echo ""
    echo "Next steps:"
    echo "  garraia init    # interactive setup wizard"
    echo "  garraia start   # start the gateway"
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

    # Release workflow names the arm64 binary "arm64", not "aarch64".
    case "${ARCH_NAME}" in
        aarch64) ASSET_ARCH="arm64" ;;
        *)       ASSET_ARCH="${ARCH_NAME}" ;;
    esac

    ARTIFACT="${BINARY}-${OS_NAME}-${ASSET_ARCH}"
    echo "Detected platform: ${OS_NAME}-${ASSET_ARCH}"
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
}

error() {
    echo "error: $1" >&2
    exit 1
}

main
