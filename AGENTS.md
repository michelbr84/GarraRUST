# AGENTS.md

## Cursor Cloud specific instructions

### Project overview

GarraIA is a multi-crate Rust workspace (`edition = "2024"`, `rust-version = "1.92"`). The main binary is `garraia-cli`; the HTTP/WS gateway lives in `garraia-gateway`.

### Build, lint, and test

- **Build all**: `cargo build` (from workspace root)
- **Build a single crate**: `cargo build -p garraia-gateway`
- **Test a single crate**: `cargo test -p garraia-gateway`
- **Test all**: `cargo test --workspace`
- **Lint**: `cargo clippy --workspace` (clippy is available via the installed toolchain)
- **Format check**: `cargo fmt --check`

### Notes

- The workspace uses `edition = "2024"` and declares `rust-version = "1.92"` (GAR-441 — bumped to track real transitive MSRV floor of wasmtime 44 sub-crates after GAR-454 bump). The VM ships with Rust 1.93.
- No external services (databases, Redis, etc.) are required for building or running tests; SQLite is bundled via `rusqlite` with the `bundled` feature.
- The gateway integration tests start their own ephemeral HTTP/WS servers on random ports — no manual server startup is needed.
- The `garraia-gateway` crate depends on many workspace crates; initial compilation can take ~45 s.
