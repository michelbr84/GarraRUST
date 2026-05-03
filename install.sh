#!/bin/sh
# GarraIA installer — https://github.com/michelbr84/GarraRUST
# Usage: curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
set -eu

REPO="michelbr84/GarraRUST"
BINARY="garraia"

main() {
    detect_platform
    get_latest_version
    download_and_verify
    install_binary
    echo ""
    echo "GarraIA $VERSION installed to $INSTALL_PATH"
    echo ""
    echo "Next steps:"
    echo "  garraia init    # interactive setup wizard"
    echo "  garraia start   # start the gateway"
}

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  OS_NAME="linux" ;;
        Darwin) OS_NAME="macos" ;;
        *)      error "Unsupported OS: $OS. Only Linux and macOS are supported." ;;
    esac

    case "$ARCH" in
        x86_64|amd64)   ARCH_NAME="x86_64" ;;
        aarch64|arm64)   ARCH_NAME="aarch64" ;;
        *)               error "Unsupported architecture: $ARCH" ;;
    esac

    ARTIFACT="${BINARY}-${OS_NAME}-${ARCH_NAME}"
    echo "Detected platform: ${OS_NAME}-${ARCH_NAME}"
}

get_latest_version() {
    # Try /releases/latest first (only returns non-prerelease)
    VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name"' \
        | head -1 \
        | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

    # Fall back to the newest release (including pre-releases)
    if [ -z "$VERSION" ]; then
        VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" 2>/dev/null \
            | grep '"tag_name"' \
            | head -1 \
            | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')
    fi

    if [ -z "$VERSION" ]; then
        error "Failed to fetch latest release version"
    fi

    echo "Latest version: $VERSION"
}

download_and_verify() {
    BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"
    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT

    echo "Downloading ${ARTIFACT}..."
    curl -fsSL "${BASE_URL}/${ARTIFACT}" -o "${TMPDIR}/${ARTIFACT}"
    curl -fsSL "${BASE_URL}/${ARTIFACT}.sha256" -o "${TMPDIR}/${ARTIFACT}.sha256"

    echo "Verifying checksum..."
    cd "$TMPDIR"
    if command -v sha256sum >/dev/null 2>&1; then
        if ! sha256sum -c "${ARTIFACT}.sha256" >/dev/null 2>&1; then
            error "Checksum verification failed"
        fi
    elif command -v shasum >/dev/null 2>&1; then
        if ! shasum -a 256 -c "${ARTIFACT}.sha256" >/dev/null 2>&1; then
            error "Checksum verification failed"
        fi
    else
        echo "warning: No checksum tool found, skipping verification"
    fi
    cd - >/dev/null
    echo "Checksum verified."
}

install_binary() {
    # Priority: $GARRAIA_INSTALL_DIR > ~/.local/bin (if in PATH) > /usr/local/bin
    if [ -n "${GARRAIA_INSTALL_DIR:-}" ]; then
        INSTALL_DIR="$GARRAIA_INSTALL_DIR"
    elif echo "$PATH" | tr ':' '\n' | grep -qx "$HOME/.local/bin"; then
        INSTALL_DIR="$HOME/.local/bin"
    else
        INSTALL_DIR="/usr/local/bin"
    fi

    mkdir -p "$INSTALL_DIR"
    INSTALL_PATH="${INSTALL_DIR}/${BINARY}"

    if [ "$INSTALL_DIR" = "/usr/local/bin" ] && [ "$(id -u)" -ne 0 ]; then
        echo "Installing to ${INSTALL_DIR} (requires sudo)..."
        sudo cp "${TMPDIR}/${ARTIFACT}" "$INSTALL_PATH"
        sudo chmod +x "$INSTALL_PATH"
    else
        cp "${TMPDIR}/${ARTIFACT}" "$INSTALL_PATH"
        chmod +x "$INSTALL_PATH"
    fi
}

error() {
    echo "error: $1" >&2
    exit 1
}

main
