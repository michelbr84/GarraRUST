# Installation Guide

This guide covers installing GarraIA on various platforms.

## Prerequisites

- **Rust 1.95+** (if building from source)
- **FFmpeg** (for voice mode)
- **Linux (prebuilt binaries):** glibc ≥ 2.35 — Ubuntu 22.04+, Debian 12+.
  Older distros and musl-based systems (Alpine) must build from source.
  (Since v0.3.4 OpenSSL is statically vendored into the binaries: no system
  `libssl` required.)

## Quick Install

### Linux/macOS

```bash
curl -fsSL https://garraia.org/install.sh | sh
```

> **Minimum glibc (Linux):** the release binaries are built on Ubuntu 22.04 and
> require glibc ≥ 2.35. The installer probes `ldd --version` and aborts early
> with instructions if the system is older (the raw loader error would be
> ``version `GLIBC_2.xx' not found``). Keep this note in sync with the
> `build-linux-x86_64` runner in `.github/workflows/release.yml` and
> `MIN_GLIBC` in `install.sh`.

The same script (auto-synced) is published through alternative channels —
the release CDN is the most robust against per-IP rate limits (**HTTP 429**
is common on cloud pods whose egress IP is shared by many users):

```bash
# Official mirror — GitHub release CDN (no aggressive per-IP limits):
curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh

# Repository main branch (raw) and community CDN mirror:
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | sh
curl -fsSL https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh | sh
```

### Windows

```powershell
irm https://garraia.org/install.ps1 | iex
```

`install.ps1` is the Windows sibling of `install.sh` and behaves the same way:
it detects the platform, resolves the latest release, downloads the binary,
verifies it against the release's `SHA256SUMS`, installs it as `garraia.exe`,
adds it to your **user** PATH, and then chains into `garraia init` and
`garraia start`. It never needs administrator rights.

Same mirrors as the shell installer:

```powershell
# Repository main branch (raw) and community CDN mirror:
irm https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.ps1 | iex
irm https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.ps1 | iex

# GitHub release CDN (no aggressive per-IP limits) - see the caveat below:
irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
```

> **Release-CDN caveat.** `install.ps1` landed after the `v0.3.3` tag, so
> `releases/latest/download/install.ps1` returns 404 until the next release is
> cut. `release.yml` already uploads it, so this resolves itself from `v0.3.4`
> onward. Until then, use `garraia.org`, raw, or jsDelivr on Windows. The
> shell installer is unaffected -- `install.sh` has shipped as a release asset
> for several versions.
>
> The `Install endpoints` workflow probes every URL on this page daily, so a
> broken mirror surfaces without anyone having to notice by hand.

**Passing options.** `irm | iex` evaluates the script with no arguments -- there
is no PowerShell equivalent of `sh -s --`. To pass flags, turn the downloaded
text into a scriptblock and invoke that:

```powershell
& ([scriptblock]::Create((irm https://garraia.org/install.ps1))) -SkipSetup
```

Environment variables work in both forms and are usually simpler in automation:

```powershell
$env:GARRAIA_SKIP_INIT=1; $env:GARRAIA_SKIP_START=1
irm https://garraia.org/install.ps1 | iex
```

| Flag | Environment variable | Effect |
|------|----------------------|--------|
| `-SkipSetup` | `GARRAIA_SKIP_INIT=1` + `GARRAIA_SKIP_START=1` | Install only. |
| `-SkipInit` | `GARRAIA_SKIP_INIT=1` | Skip the setup wizard. |
| `-SkipStart` | `GARRAIA_SKIP_START=1` | Skip starting the gateway. |
| `-NoLocal` | `GARRAIA_BOOTSTRAP_LOCAL=0` | Skip the GPU/Ollama prompts. |
| `-Version <tag>` | `GARRAIA_VERSION=<tag>` | Pin a release instead of latest. |
| `-InstallDir <dir>` | `GARRAIA_INSTALL_DIR=<dir>` | Install elsewhere. |
| `-NoModifyPath` | `GARRAIA_NO_PATH=1` | Do not touch the user PATH. |

An environment variable set by the caller always wins over the matching flag,
so existing automation keeps its behavior.

**Where it installs.** `%LOCALAPPDATA%\Programs\GarraIA` by default -- a
per-user location, so no UAC prompt. The installer adds that directory to your
user PATH *and* to the current session, so `garraia` works immediately in the
terminal you ran it from; other terminals that were already open need a
restart. Windows system directories are refused outright.

**SmartScreen.** The published binaries and installers are **not code-signed**
(there is no certificate configured for this project). Windows SmartScreen will
show *"Windows protected your PC"* the first time you run the desktop installer,
and you need *More info -> Run anyway*. Windows Defender may also flag a
low-reputation unsigned binary. This is expected, not a sign of tampering --
verify the `SHA256SUMS` entry if you want certainty. Removing the warning
requires an OV/EV code-signing certificate, which is tracked separately.

**Windows ARM64.** From `v0.3.4` a native ARM64 build
(`garraia-windows-aarch64.exe`, best-effort) is published, and the installer
selects it automatically on ARM64 hosts. Pinning `GARRAIA_VERSION` to anything
older than `v0.3.4` on ARM64 fails with a 404 -- those releases only served
ARM64 via the x86_64 binary under Windows 11's x64 emulation, which you can
still download by hand if you need an older version.

**Desktop app (optional).** Releases also carry
`garraia-desktop-windows-x86_64.msi` (and, when the bundler produces it, a NSIS
`-setup.exe`) -- a tray application that bundles the CLI as a sidecar. It is
built best-effort, so a given release may ship without it. The MSI is not a
prerequisite for the CLI; the one-liner above is the supported path.

### Android (Termux)

From `v0.3.6` the installer has an Android branch. Run it **inside the
[Termux](https://github.com/termux/termux-app#installation) app** — the F-Droid
or GitHub build; the Play Store fork is a different app that removed the
`RUN_COMMAND` permission and is not supported.

```bash
pkg install curl termux-exec
curl -fsSL https://garraia.org/install.sh | bash
garraia doctor    # platform, dirs, config, providers, daemon + a Termux block
garraia chat      # a cloud provider, or --url http://PC-ON-LAN:8080
```

`uname -s` reports `Linux` inside Termux, so the installer detects Android
through `$TERMUX_VERSION` (exported by every Termux shell) or a `*com.termux*`
`$PREFIX`. On that branch it:

- downloads `garraia-android-aarch64` (bionic, `aarch64-linux-android`, API
  21+) instead of the glibc `garraia-linux-aarch64`, which does **not** run on
  Android;
- skips the glibc preflight — Termux has no glibc loader to interrogate;
- installs into `$PREFIX/bin`, which is on `PATH` and writable without `sudo`
  (`/usr/local/bin` would be created *outside* `PATH`, a silent trap);
- writes `$PREFIX/bin/garra-mcp-server`, a wrapper for external MCP hosts (see
  [MCP servers under Termux](#mcp-servers-under-termux) below).

`garra update` resolves the same asset from inside Termux, so self-update works
normally.

**Keep the session alive.** Android's phantom process killer and battery
optimization stop long-running background processes. Run `termux-wake-lock`,
exempt Termux from battery optimization, and prefer keeping the gateway in the
foreground of the Termux session that started it. `garraia start -d` works, but
Android may still reap it; a real "always on" needs the companion app on the
v1 rung of [ADR 0016](adr/0016-mobile-termux-local-first.md).

**Static musl builds are deliberately not shipped.** A static musl binary
breaks DNS on Android: there is no `/etc/resolv.conf`, musl's internal resolver
never reaches `dnsproxyd`, and `LD_PRELOAD` cannot intercept a static binary.

### Garra Desktop on Linux

From `v0.3.5` releases also carry the desktop app (parrot overlay + chat bar)
for Linux, built best-effort by the Tauri bundler:

- `garraia-desktop-linux-x86_64.deb` — `sudo apt install ./garraia-desktop-linux-x86_64.deb`
- `garraia-desktop-linux-x86_64.AppImage` — `chmod +x` and run; needs `libfuse2`
  on Ubuntu 22.04+ (`sudo apt install libfuse2`), or run with
  `--appimage-extract-and-run`

These are **different packages from the CLI ones**: `garraia-linux-x86_64.deb`
installs only the terminal CLI (`/usr/bin/garraia`); the desktop `.deb`
installs the tray app **plus** the same CLI as its bundled sidecar, also at
`/usr/bin/garraia`. Because both own that path, the desktop package declares
`Provides/Conflicts/Replaces: garraia` — installing it **replaces** the CLI
package (you keep the `garraia` command either way). Pick one:

| You want | Install |
|---|---|
| Terminal only | `garraia-linux-x86_64.deb` (or the one-liner above) |
| Parrot + chat bar + CLI | `garraia-desktop-linux-x86_64.deb` |

**Wayland caveat.** On Wayland sessions (Ubuntu's default) the window manager
does not honor always-on-top/skip-taskbar for regular windows, so the parrot
and the chat bar behave as normal windows. X11 sessions behave as documented.
The app itself works on both.

## Build from Source

### Prerequisites

```bash
# Install Rust 1.95+
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

### Build on Termux (fallback)

Building on the device is a **fallback**, not the supported path — the release
asset above is. Use it when you need an unreleased commit, or on a device the
published binary does not cover.

```bash
pkg install rust clang pkg-config openssl ca-certificates git

# Termux ships its own CA bundle; without this, TLS fails with a generic error
# during the build (crates.io fetches) rather than anything that names certs.
export SSL_CERT_FILE=$PREFIX/etc/tls/cert.pem

# LTO is the difference between "slow" and "impossible" here. The workspace
# release profile sets `lto = true` + `codegen-units = 1`, and on a mid-range
# phone the linker gets OOM-killed -- the symptom is a bare `Signal 9`, with no
# error message pointing at memory. Override both:
CARGO_PROFILE_RELEASE_LTO=false \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
  cargo build --release --package garraia

install -m 755 target/release/garra "$PREFIX/bin/garraia"
```

Budget for it: on a device with under 4 GB of RAM expect **30-60 minutes** and
set up swap first. Even with LTO off, the link step is the peak. Keep the
session in the foreground (`termux-wake-lock`) — a build backgrounded long
enough gets killed by the phantom process killer, which also reads as an
unexplained `Signal 9`.

Note that a source build does **not** produce `$PREFIX/bin/garra-mcp-server`;
that wrapper is written by `install.sh`. See
[MCP servers under Termux](#mcp-servers-under-termux) for how to create it by
hand.

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
  - **Cloud-first** (cloud provider primary + Ollama fallback).
  - **Cloud-only** (default for CPU/no-GPU machines).
- On the cloud branch, let you pick the provider — **OpenRouter**
  (recommended default), **OpenAI**, or **Anthropic** — and prompt for
  that provider's API key (each preset names its own env var, default
  model, and key-creation URL).
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

`garraia init` writes this for you. To edit it by hand, first find the file the
gateway actually reads — the startup banner prints both the directory (`Config`)
and the filename (`File`).

The config directory is resolved in this order:

1. `$GARRAIA_CONFIG_DIR`, when set — the supported way to keep everything under
   a single directory of your choosing.
2. `~/.config/garraia` (XDG), when it exists. **This is the default for new
   installs.**
3. `~/.garraia`, when it exists and the XDG path does not (legacy).

Within that directory, `config.yml` wins over `config.toml`; if both exist the
TOML file is silently ignored.

```yaml
gateway:
  host: "127.0.0.1"
  port: 3888

llm:
  main:
    provider: openai
    model: gpt-4o
    api_key: "sk-..."

channels:
  telegram:
    enabled: true
    bot_token: "YOUR_BOT_TOKEN"
```

The API key is resolved per provider in the order **credential vault → this
file → environment variable** (`OPENAI_API_KEY`, `OPENROUTER_API_KEY`, …). If
none of the three yields a key, the provider is skipped at startup and the
gateway comes up unable to answer.

> **The credential vault needs a passphrase on every start.** If you store keys
> in the vault, the gateway can only open it when `GARRAIA_VAULT_PASSPHRASE` is
> present in *its* environment — the wizard cannot arrange that for you. Export
> it from your shell profile or a systemd `EnvironmentFile`. This is why
> `garraia init` now defaults to writing the key into `config.yml`, which it
> creates with mode `0600`.

Run `garraia config check` to verify: it reports which file is in force and
fails with an explicit error for any provider whose key resolves nowhere.

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
# Runtime-only image: build the binary first with `cargo build --release -p garraia`
# (Rust 1.95+). The repo's own Dockerfile does the multi-stage build for you.
FROM debian:trixie-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates ffmpeg \
    && apt-get clean

# cargo names the binary `garra`
COPY target/release/garra /usr/local/bin/garra

ENTRYPOINT ["garra"]
CMD ["start", "--host", "0.0.0.0"]
```

## Pre-compiled Binaries

Download from [GitHub Releases](https://github.com/michelbr84/GarraRUST/releases):

| Platform | Architecture | Bare binary | Archive |
|----------|--------------|-------------|---------|
| Linux | x86_64 | `garraia-linux-x86_64` | `garraia-linux-x86_64.tar.gz` |
| Linux | aarch64 (ARM64) | `garraia-linux-aarch64` | `garraia-linux-aarch64.tar.gz` |
| macOS | x86_64 | `garraia-macos-x86_64` | `garraia-macos-x86_64.tar.gz` |
| macOS | aarch64 (Apple Silicon) | `garraia-macos-aarch64` | `garraia-macos-aarch64.tar.gz` |
| Windows | x86_64 | `garraia-windows-x86_64.exe` | `garraia-windows-x86_64.zip` |
| Windows | aarch64 (ARM64, from `v0.3.4`) | `garraia-windows-aarch64.exe` | `garraia-windows-aarch64.zip` |
| Android (Termux) | aarch64 (from `v0.3.6`) | `garraia-android-aarch64` | `garraia-android-aarch64.tar.gz` |

Plus, on Windows only, the desktop installers
`garraia-desktop-windows-x86_64.msi` and
`garraia-desktop-windows-x86_64-setup.exe` (both best-effort -- a release may
ship without them).

### Linux packages (`.deb` / `.rpm` / AppImage)

From `v0.3.4` releases also carry Linux packages (all best-effort, packaged by
the `package-linux` job from the same binaries as above -- see ADR 0015):

```bash
# Debian / Ubuntu (needs glibc >= 2.35, i.e. Ubuntu 22.04+ / Debian 12+):
sudo apt install ./garraia-linux-x86_64.deb

# Fedora / RHEL / openSUSE:
sudo rpm -i garraia-linux-x86_64.rpm

# Portable AppImage (x86_64 only for now):
chmod +x garraia-linux-x86_64.AppImage
./garraia-linux-x86_64.AppImage --version
```

Both packages install `/usr/bin/garraia` plus `LICENSE`/`README.md` under
`/usr/share/doc/garraia/`. aarch64 variants (`garraia-linux-aarch64.deb` /
`.rpm`) exist whenever the best-effort aarch64 binary was built.

**`garraia update` under a package install.** The self-updater swaps the
binary file in place, and a package installs it root-owned in `/usr/bin` -- so
run `sudo garraia update`, or simply download and install the next release's
package. The packages are not GPG-signed (there is no hosted apt/dnf
repository); verify downloads against the release's `SHA256SUMS`.

**Bare binary or archive?** They contain the same program. The archive adds
`LICENSE` and `README.md` and unpacks to a single directory whose executable is
named plainly `garraia` / `garraia.exe`, which is what you want for a manual
install or for repackaging. The bare binaries exist because `garraia update`
and both installers resolve assets by exact name -- they are the compatibility
surface and will not be renamed or removed.

> From `v0.2.1` (2026-05-14) aarch64 binaries match Rust's `std::env::consts::ARCH`,
> so `garraia update` selects the right asset automatically. Every asset ships with
> a sibling `<name>.sha256`, and all of them are listed in `SHA256SUMS`.

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

### `curl | sh` fails with HTTP 429

`raw.githubusercontent.com` (which serves `install.sh`) and `api.github.com`
enforce **per-IP** rate limits. Cloud pods (RunPod etc.) share their egress IP
across many users, so a brand-new pod can be over the quota before you run
anything. Retry in a few minutes, or use one of the alternative channels:

```bash
# Official mirror — GitHub release CDN:
curl -fsSL https://github.com/michelbr84/GarraRUST/releases/latest/download/install.sh | sh

# Community CDN mirror of main:
curl -fsSL https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.sh | sh
```

On Windows the same mirrors serve `install.ps1`:

```powershell
irm https://github.com/michelbr84/GarraRUST/releases/latest/download/install.ps1 | iex
irm https://cdn.jsdelivr.net/gh/michelbr84/GarraRUST@main/install.ps1 | iex
```

Inside the installer itself, downloads retry automatically and the release
tag is resolved from the `github.com` redirect (not the API), so the 429
surface is limited to that very first fetch of the script.

### Termux: the downloaded binary does not run

The symptom is that the installer downloads, the checksum verifies, and running
`garraia` fails anyway. That means an `install.sh` **older than v0.3.6** ran: it
had no Android branch, so `uname -s` reporting `Linux` made it fetch the glibc
`garraia-linux-aarch64`, which Android's bionic loader cannot run.

`https://garraia.org/install.sh` is a static copy in a separate repository,
synced on a daily cron and published manually — so it can lag behind `main` by
a day or more. If you hit this, fetch from a channel that tracks the source
directly:

```bash
curl -fsSL https://raw.githubusercontent.com/michelbr84/GarraRUST/main/install.sh | bash
```

Confirm before running: `grep -c TERMUX_VERSION install.sh` must be non-zero.

### Termux: TLS, `SSL_CERT_FILE` and certificates

**`SSL_CERT_FILE` does not affect the CLI's HTTPS traffic.** `garra` talks to
LLM providers, GitHub and every other endpoint through `reqwest` with the
`rustls-tls` feature, whose roots are the webpki (Mozilla) set **compiled into
the binary**. No system trust store is read, so nothing to point at and nothing
to configure — TLS works on a fresh Termux install with no setup.

Two real exceptions:

- **Postgres.** The `sqlx` driver uses native roots, so TLS to a Postgres
  server does read the system store. Export
  `SSL_CERT_FILE=$PREFIX/etc/tls/cert.pem` (`pkg install ca-certificates`).
  `garraia doctor` raises this only when a `GARRAIA_*_DATABASE_URL` is set.
- **Building from source.** `cargo` fetching crates.io is a separate process
  with its own trust store — see the build section above.

If you are debugging a `rustls-platform-verifier` panic (`Expect
rustls-platform-verifier to be initialized`, `android.rs:90`): that crate needs
a JNI Context that does not exist outside the JVM, and it is **not** in the
`garra` binary. Verify with
`cargo tree -p garraia --target aarch64-linux-android --invert rustls-platform-verifier`,
which reports "did not match any packages"; CI asserts it stays that way. A
panic with that message comes from a different binary on the device.

### MCP servers under Termux

Two distinct failures, with distinct fixes.

**1. Garra as an MCP *server*, spawned by an external host.** MCP hosts spawn
their servers with a filtered environment (`env -i PATH=… HOME=…`), which drops
`LD_PRELOAD`. On Android the ELF exec resolves through the termux-exec shim, so
without it the exec fails before `garraia` runs at all — nothing inside the
binary can recover from that. Point the host at the wrapper `install.sh`
installs:

```json
{ "mcpServers": { "garra": { "command": "/data/data/com.termux/files/usr/bin/garra-mcp-server" } } }
```

Equivalent, if you would rather configure the host directly:

```json
{ "mcpServers": { "garra": {
    "command": "garraia", "args": ["mcp-server"],
    "env": { "LD_PRELOAD": "/data/data/com.termux/files/usr/lib/libtermux-exec.so" }
} } }
```

After a source build the wrapper does not exist; recreate it with:

```bash
cat > "$PREFIX/bin/garra-mcp-server" <<EOF
#!$PREFIX/bin/sh
# \$PREFIX is baked in as a default on purpose: the host that runs this
# wrapper is the one filtering the environment, so PREFIX may be unset too.
PREFIX="\${PREFIX:-$PREFIX}"
if [ -f "\${PREFIX}/lib/libtermux-exec.so" ] && [ -z "\${LD_PRELOAD:-}" ]; then
    LD_PRELOAD="\${PREFIX}/lib/libtermux-exec.so"
    export LD_PRELOAD
fi
exec "$PREFIX/bin/garraia" mcp-server "\$@"
EOF
chmod +x "$PREFIX/bin/garra-mcp-server"
```

**2. Garra as an MCP *client*, spawning `npx`/`python` servers.** Packages
installed manually through npm/pip carry `/usr/bin/env` shebangs, a path Termux
does not have, and the child dies with
`env: 'node': No such file or directory`. Run `pkg install termux-exec` — from
`v0.3.7` the gateway injects `LD_PRELOAD` into MCP children on Android by
itself, but the shim still has to be installed. For a binary you installed by
hand, `termux-fix-shebang <file>` rewrites the shebang in place.

`garraia doctor` checks the shim, the wrapper, `$PREFIX/bin` on `PATH` and the
trust store in one pass, and prints the next step for whatever is missing.

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
