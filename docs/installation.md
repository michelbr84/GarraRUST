# Installation Guide

This guide covers installing GarraIA on various platforms.

## Prerequisites

- **Rust 1.92+** (if building from source)
- **FFmpeg** (for voice mode)
- **OpenSSL** (for some features)

## Quick Install

### Linux/macOS

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
```

### Windows

Download the pre-compiled binary from [GitHub Releases](https://github.com/michelbr84/GarraRUST/releases).

## Build from Source

### Prerequisites

```bash
# Install Rust 1.92+
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup update stable

# Install FFmpeg (for voice mode)
# Ubuntu/Debian:
sudo apt install ffmpeg
# macOS:
brew install ffmpeg
# Windows:
# Download from https://ffmpeg.org/download.html
```

### Build

```bash
# Clone the repository
git clone https://github.com/michelbr84/GarraRUST.git
cd GarraRUST

# Build release
cargo build --release

# Or with plugin support
cargo build --release --features plugins
```

### Install

```bash
# Copy to PATH
sudo cp target/release/garra /usr/local/bin/

# Or use cargo install
cargo install --path crates/garraia-cli
```

## Initial Setup

### 1. Initialize

```bash
garraia init
```

This wizard will (plan 0126):

- Detect the environment (OS, root, RunPod hints, systemd, NVIDIA GPU via
  `nvidia-smi`, Ollama install/running state, and whether the well-known
  ports `3888`, `8080`, `11434`, `7860`, `9090` are free) and print a
  one-line summary.
- **Preserve an existing `config.yml`**: if one is already present, the
  wizard prompts you to **backup-and-overwrite** (renames the old file
  to `config.yml.bak-YYYYMMDD-HHMMSS`), **merge/update** (keeps your
  values and only adds missing keys), or cancel. Non-interactive runs
  (e.g. `garraia init` in CI) print the legacy hint and exit 0 without
  touching `config.yml`.
- Offer a provider mode:
  - **Local-first** (Ollama on this GPU + cloud fallback) — default
    when an NVIDIA GPU is detected and `GARRAIA_BOOTSTRAP_LOCAL` is not
    set to `0`.
  - **Cloud-first** (OpenRouter primary + Ollama fallback).
  - **Cloud-only** (OpenRouter — default for CPU/no-GPU machines).
- On GPU machines (and only after explicit confirmation), install
  Ollama via the official upstream script and pull
  `hf.co/MaziyarPanahi/Qwen3-14B-GGUF:Q4_K_M`. NVIDIA drivers and CUDA
  are **never** installed by the wizard — if `nvidia-smi` works, the
  wizard assumes the GPU runtime is already usable.
- Offer to enable voice (Chatterbox TTS @ `:7860` + faster-whisper STT
  @ `:9090`). Endpoints are written into `config.yml`; install
  instructions for both servers are printed for copy-paste (auto-install
  of those Python stacks is deferred — see [voice.md](voice.md)).
- Configure the Telegram channel as before.
- Store API keys and bot tokens in the encrypted vault.
- Pick server-friendly defaults: `gateway.host: 0.0.0.0` when running
  as root or inside a RunPod pod; `127.0.0.1` otherwise. `PORT` env
  var (Runpod LB Serverless) is honored.

Skip toggles:

- `GARRAIA_BOOTSTRAP_LOCAL=0` — suppress the GPU/local-stack prompts
  even when a GPU is present (useful when you want to use the GPU for
  something else and run Garra in cloud-only mode).
- `GARRAIA_SKIP_INIT=1` — when running via the `curl | sh` installer
  (plan 0127, PR-B), skip the auto-run of `garraia init` and leave
  configuration for later. The installer falls back to printing
  next-steps and exits 0.
- `GARRAIA_SKIP_START=1` — same flow but skips the foreground
  `garraia start` after `garraia init` completes. Both toggles set
  together is equivalent to the pre-PR-B installer behavior.

### 2. Configure

Edit `~/.garraia/config.yml`:

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  main:
    provider: openai
    model: gpt-4o
    api_key: "sk-..."  # or use vault

channels:
  telegram:
    enabled: true
    bot_token: "YOUR_BOT_TOKEN"
```

### 3. Start

```bash
# Start in foreground
garraia start

# Or as daemon
garraia start --daemon

# With voice mode
garraia start --with-voice
```

## Docker Installation

### Using Docker Compose

```bash
# Clone and start
git clone https://github.com/michelbr84/GarraRUST.git
cd GarraRUST
docker-compose up -d
```

### Manual Docker

```dockerfile
FROM rust:1.92-bookworm

RUN apt-get update && apt-get install -y ffmpeg libssl3

# Build and copy binary
COPY target/release/garra /usr/local/bin/

ENTRYPOINT ["garraia"]
CMD ["start"]
```

## Pre-compiled Binaries

Download from [GitHub Releases](https://github.com/michelbr84/GarraRUST/releases):

| Platform | Architecture | Filename |
|----------|--------------|----------|
| Linux | x86_64 | garraia-linux-x86_64 |
| Linux | aarch64 (ARM64) | garraia-linux-aarch64 |
| macOS | x86_64 | garraia-macos-x86_64 |
| macOS | aarch64 (Apple Silicon) | garraia-macos-aarch64 |
| Windows | x86_64 | garraia-windows-x86_64.exe |

> From `v0.2.1` (2026-05-14) aarch64 binaries match Rust's `std::env::consts::ARCH`,
> so `garraia update` selects the right asset automatically. Each binary ships with
> a sibling `<name>.sha256` for verification.

## Verification

Check installation:

```bash
garraia --version
```

Run health check:

```bash
curl http://127.0.0.1:3888/api/health
```

## Troubleshooting

### Port already in use

```bash
# Find what's using the port
lsof -i :3888

# Use a different port
garraia start --port 3889
```

### Permission denied

```bash
# Make executable
chmod +x garraia
```

### Database issues

```bash
# Remove database and start fresh
rm -rf ~/.garraia/data/
garraia start
```

### Update GarraIA

```bash
garraia update

# If update fails, rollback
garraia rollback
```
