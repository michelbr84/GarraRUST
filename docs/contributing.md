# Contributing to GarraIA

Thank you for your interest in contributing to GarraIA!

## Getting Started

### Prerequisites

- Rust 1.92+
- Git
- FFmpeg (for voice features)

### Setup

1. Fork the repository
2. Clone your fork:
   ```bash
   git clone https://github.com/YOUR_USERNAME/GarraRUST.git
   cd GarraRUST
   ```

3. Build:
   ```bash
   cargo build --workspace
   ```

4. Run tests:
   ```bash
   cargo test --workspace
   ```

## Development Workflow

### 1. Create a Branch

```bash
git checkout -b feature/your-feature
# or
git checkout -b fix/your-fix
```

### 2. Make Changes

Follow the coding standards:
- Run `cargo fmt` before committing
- Run `cargo clippy` to catch issues
- Add tests for new features

### 3. Commit

Use conventional commits:

```bash
git commit -m "feat: add new LLM provider"
git commit -m "fix: resolve memory leak"
git commit -m "docs: update configuration guide"
```

### 4. Submit PR

1. Push your branch
2. Open a Pull Request
3. Fill out the PR template
4. Wait for review

## Project Structure

```
crates/
├── garraia-cli/       # CLI entry point
├── garraia-gateway/   # HTTP/WebSocket gateway
├── garraia-config/    # Configuration
├── garraia-channels/  # Messaging channels
├── garraia-agents/    # LLM providers
├── garraia-voice/     # Voice pipeline
├── garraia-runtime/   # State machine
├── garraia-db/        # Memory/SQLite
├── garraia-plugins/   # WASM plugins
├── garraia-media/     # Media processing
├── garraia-security/  # Vault/auth
├── garraia-skills/    # Skills system
├── garraia-tools/     # Tool traits
└── garraia-common/    # Shared types
```

## Coding Standards

### Rust Guidelines

- Use `cargo fmt` for formatting
- Follow Rust API guidelines
- Use meaningful names
- Add documentation for public APIs

### Testing

- Unit tests in `src/`
- Integration tests in `tests/`
- Run full test suite before PR

### Documentation

- Update README if needed
- Add doc comments to public functions
- Update docs/ for user-facing changes

## Finding Issues

- [Good first issues](https://github.com/michelbr84/GarraRUST/issues?q=label%3Agood-first-issue+is%3Aopen)
- [Help wanted](https://github.com/michelbr84/GarraRUST/issues?q=label%3Ahelp-wanted+is%3Aopen)
- [Linear Roadmap](https://linear.app/chatgpt25/project/garraia-complete-roadmap-2026-ac242025/overview)

## Commands

### Build

```bash
# Debug build
cargo build

# Release build
cargo build --release

# With plugins
cargo build --release --features plugins
```

### Test

```bash
# All tests
cargo test --workspace

# Single crate
cargo test -p garraia-gateway

# With output
cargo test --workspace -- --nocapture
```

### Lint

```bash
cargo fmt --check
cargo clippy --workspace
cargo deny check
```

### Run

```bash
cargo run --package garraia-cli -- init
cargo run --package garraia-cli -- start
```

## Communication

- [Discord](https://discord.gg/aEXGq5cS) - Chat with contributors
- [GitHub Issues](https://github.com/michelbr84/GarraRUST/issues) - Bug reports
- [Linear](https://linear.app/) - Project tracking

## License

By contributing, you agree that your contributions will be licensed under MIT.
