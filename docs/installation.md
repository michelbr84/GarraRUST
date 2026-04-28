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
sudo cp target/release/garraia /usr/local/bin/

# Or use cargo install
cargo install --path crates/garraia-cli
```

## Initial Setup

### 1. Initialize

```bash
garraia init
```

This wizard will:
- Create config directory (`~/.garraia/`)
- Ask for your LLM provider choice
- Store API keys in encrypted vault

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
COPY target/release/garraia /usr/local/bin/

ENTRYPOINT ["garraia"]
CMD ["start"]
```

## Pre-compiled Binaries

Download from [GitHub Releases](https://github.com/michelbr84/GarraRUST/releases):

| Platform | Architecture | Filename |
|----------|--------------|----------|
| Linux | x86_64 | garraia-linux-x86_64 |
| Linux | ARM64 | garraia-linux-arm64 |
| macOS | x86_64 | garraia-macos-x86_64 |
| macOS | ARM64 | garraia-macos-arm64 |
| Windows | x86_64 | garraia-windows-x86_64.exe |

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
