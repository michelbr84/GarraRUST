# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

GarraIA is a multi-crate Rust workspace (`edition = "2024"`, `rust-version = "1.95"`). The main binary is `garraia-cli` (the binary itself is called `garra`); the HTTP/WS gateway lives in `garraia-gateway`.

### Build, lint, and test

- **Build all**: `cargo build --workspace --exclude garraia-desktop`
- **Build a single crate**: `cargo build -p garraia-gateway`
- **Test a single crate**: `cargo test -p garraia-gateway`
- **Test all**: `cargo test --workspace --exclude garraia-desktop`
- **Lint**: `cargo clippy --workspace --exclude garraia-desktop --all-targets -- -D warnings`
- **Format check**: `cargo fmt --all -- --check`

**Always pass `--exclude garraia-desktop`.** Its `build.rs` needs the
GTK/glib system libraries and a Windows sidecar binary that a clean machine
does not have, so a plain `--workspace` fails on something unrelated to your
change. CI excludes it for the same reason (`.github/workflows/ci.yml`);
desktop is built separately via `scripts/build-installer.ps1`.

Note the package name: the CLI crate is `garraia` (binary `garra`), **not**
`garraia-cli` — `cargo test -p garraia-cli` fails with "did not match any
packages".

### Notes

- The workspace uses `edition = "2024"` and declares `rust-version = "1.95"` (bumped from 1.94; the reason is recorded inline in `Cargo.toml` next to the pin). The VM ships with a new enough toolchain.
- No external services (databases, Redis, etc.) are required for building or running tests; SQLite is bundled via `rusqlite` with the `bundled` feature.
- The gateway integration tests start their own ephemeral HTTP/WS servers on random ports — no manual server startup is needed.
- The `garraia-gateway` crate depends on many workspace crates; initial compilation can take ~45 s.
