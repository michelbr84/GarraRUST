# Superpowers Configuration — GarraRUST

## Project Context

GarraRUST is a multi-crate Rust workspace (edition 2024, rust-version 1.92) with 16 crates.
The primary binary is `garraia-cli`; the HTTP/WS gateway lives in `garraia-gateway`.
Mobile client in Flutter, desktop app in Tauri v2.

## Build & Test Commands

```bash
# Build
cargo build                          # entire workspace
cargo build -p <crate>               # single crate

# Test
cargo test --workspace               # all tests
cargo test -p <crate>                # single crate
cargo test -p <crate> <test_name>    # single test

# Lint
cargo clippy --workspace
cargo fmt --check

# Flutter
cd apps/garraia-mobile && flutter test
```

## TDD Specifics

- Use `#[test]` or `#[tokio::test]` — Axum 0.8 uses native AFIT, no `#[async_trait]`
- Never use `unwrap()` in production code — only in tests
- SQL queries via `params!` macro — never concatenate strings
- Integration tests in `garraia-gateway` start their own ephemeral HTTP/WS servers on random ports

## Git Worktree Guidelines

- Default branch: `main`
- Worktree naming: `feature/<description>` or `fix/<description>`
- Always run `cargo check -p <crate>` before commits
- Conventional Commits format: `feat:`, `fix:`, `chore:`, `refactor:`, `test:`, `docs:`

## Security Rules (must enforce during code review)

1. Never commit `.env`, credentials, or tokens
2. Never expose secrets in logs
3. Never force push to `main`
4. Never use `unwrap()` in production code
5. Never concatenate strings in SQL queries

## Crate Dependency Order (for planning tasks)

```
garraia-common (base)
├── garraia-config
├── garraia-db
├── garraia-security
├── garraia-agents (depends on config, db, security)
├── garraia-channels (depends on agents, config)
├── garraia-plugins (depends on common)
├── garraia-media
├── garraia-voice
├── garraia-tools
├── garraia-skills
├── garraia-runtime (depends on agents, channels)
├── garraia-glob
├── garraia-gateway (depends on most crates)
└── garraia-cli (depends on gateway)
```

## Initial Compilation Note

The `garraia-gateway` crate depends on many workspace crates; initial compilation can take ~45s.
No external services (databases, Redis, etc.) are required — SQLite is bundled via `rusqlite` with the `bundled` feature.
