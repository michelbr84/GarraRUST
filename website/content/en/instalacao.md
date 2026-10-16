# Installation

The commands below are the ones published in the project's README; see the
[full README installation section](https://github.com/michelbr84/GarraRUST#quick-start)
for every flag, mirror and platform note.

## Install via script (Linux, macOS)

```bash
curl -fsSL https://garraia.org/install.sh | sh
```

The installer downloads the binary for your platform, verifies it against the
release checksums, then chains into `init` and `start`.

## Install via script (Windows)

```powershell
irm https://garraia.org/install.ps1 | iex
```

The Windows sibling of `install.sh`, at behavioral parity with it.

## Build from source

Requires Rust 1.96+ (the MSRV declared in `Cargo.toml`).

```bash
cargo build --release -p garraia
./target/release/garra init     # interactive setup — pick your LLM provider
./target/release/garra start
```

Running the bare `garra` binary opens the chat REPL.

For the deeper, step-by-step guide (platforms, packages, verification), the
full installation document is currently available in Portuguese:
[Instalação](/instalacao/getting-started).
