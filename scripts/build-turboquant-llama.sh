#!/usr/bin/env bash
set -euo pipefail

# ─── Build llama.cpp with TurboQuant+ KV cache support ──────────────────────
#
# This script clones the TurboQuant+ fork of llama.cpp, builds it,
# and places the binaries in services/llama-turboquant/.
#
# Usage:
#   ./scripts/build-turboquant-llama.sh [--cuda] [--hip] [--metal] [--cpu]
#
# Flags:
#   --cuda    Build with NVIDIA CUDA support (requires CUDA toolkit)
#   --hip     Build with AMD ROCm/HIP support
#   --metal   Build with Apple Metal support (macOS only)
#   --cpu     Build CPU-only (default if no flag given)
#
# Requirements:
#   - cmake >= 3.14
#   - C/C++ compiler (gcc, clang, or MSVC)
#   - git
#   - (Optional) CUDA toolkit, ROCm, or Xcode CLI tools

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
REPO_URL="https://github.com/TheTom/llama-cpp-turboquant.git"
BRANCH="feature/turboquant-kv-cache"
BUILD_DIR="$PROJECT_ROOT/services/llama-turboquant"
SRC_DIR="$BUILD_DIR/src"

# Parse flags
BACKEND="cpu"
for arg in "$@"; do
    case "$arg" in
        --cuda)  BACKEND="cuda" ;;
        --hip)   BACKEND="hip" ;;
        --metal) BACKEND="metal" ;;
        --cpu)   BACKEND="cpu" ;;
        *)       echo "Unknown flag: $arg"; exit 1 ;;
    esac
done

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Building llama.cpp with TurboQuant+ ($BACKEND backend)    ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ─── Clone or update ────────────────────────────────────────────────────────

if [ -d "$SRC_DIR/.git" ]; then
    echo "→ Updating existing clone..."
    cd "$SRC_DIR"
    git fetch origin
    git checkout "$BRANCH"
    git pull origin "$BRANCH"
else
    echo "→ Cloning $REPO_URL ($BRANCH)..."
    mkdir -p "$BUILD_DIR"
    git clone --branch "$BRANCH" --single-branch "$REPO_URL" "$SRC_DIR"
fi

cd "$SRC_DIR"

# ─── Configure cmake ───────────────────────────────────────────────────────

CMAKE_ARGS="-DCMAKE_BUILD_TYPE=Release"

case "$BACKEND" in
    cuda)
        CMAKE_ARGS="$CMAKE_ARGS -DGGML_CUDA=ON"
        echo "→ Backend: NVIDIA CUDA"
        ;;
    hip)
        CMAKE_ARGS="$CMAKE_ARGS -DGGML_HIP=ON -DGGML_CUDA_FA_ALL_QUANTS=ON"
        echo "→ Backend: AMD ROCm/HIP"
        ;;
    metal)
        CMAKE_ARGS="$CMAKE_ARGS -DGGML_METAL=ON -DGGML_METAL_EMBED_LIBRARY=ON"
        echo "→ Backend: Apple Metal"
        ;;
    cpu)
        echo "→ Backend: CPU only"
        ;;
esac

# ─── Build ──────────────────────────────────────────────────────────────────

echo "→ Configuring with cmake..."
cmake -B build $CMAKE_ARGS

echo "→ Building (this may take a few minutes)..."
cmake --build build -j "$(nproc 2>/dev/null || sysctl -n hw.logicalcpu 2>/dev/null || echo 4)"

# ─── Copy binaries ─────────────────────────────────────────────────────────

BIN_DIR="$BUILD_DIR/bin"
mkdir -p "$BIN_DIR"

echo "→ Copying binaries to $BIN_DIR..."
cp -f build/bin/llama-server "$BIN_DIR/" 2>/dev/null || true
cp -f build/bin/llama-cli "$BIN_DIR/" 2>/dev/null || true
# Windows
cp -f build/bin/Release/llama-server.exe "$BIN_DIR/" 2>/dev/null || true
cp -f build/bin/Release/llama-cli.exe "$BIN_DIR/" 2>/dev/null || true

# ─── Verify ────────────────────────────────────────────────────────────────

echo ""
echo "→ Verifying TurboQuant+ types..."

SERVER_BIN="$BIN_DIR/llama-server"
if [ -f "$BIN_DIR/llama-server.exe" ]; then
    SERVER_BIN="$BIN_DIR/llama-server.exe"
fi

if [ -f "$SERVER_BIN" ]; then
    if "$SERVER_BIN" --help 2>&1 | grep -q "turbo"; then
        echo "  ✅ TurboQuant+ types (turbo3, turbo4) are available!"
    else
        echo "  ⚠️  Warning: turbo types not found in --help output"
    fi
else
    echo "  ⚠️  Warning: llama-server binary not found at $SERVER_BIN"
fi

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  Build complete!                                            ║"
echo "║                                                             ║"
echo "║  Start server:                                              ║"
echo "║  $BIN_DIR/llama-server \\                                    ║"
echo "║    -m models/your-model.gguf \\                              ║"
echo "║    --cache-type-k turbo3 --cache-type-v turbo2 \\            ║"
echo "║    -ngl 99 -c 32768 -fa on \\                                ║"
echo "║    --host 0.0.0.0 --port 8080                               ║"
echo "╚══════════════════════════════════════════════════════════════╝"
