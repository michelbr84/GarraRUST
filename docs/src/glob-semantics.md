# Glob Semantics for GarraRUST

This document defines the glob matching semantics for the GarraRUST file matching engine.

## Overview

GarraRUST uses glob patterns for:
- File watching (detecting changes)
- Repository traversal (finding files)
- Ignore patterns (.garraignore, .gitignore compatibility)

## Default Engine: Picomatch

GarraRUST defaults to **picomatch** for glob matching because:
- More predictable behavior (no backtracking)
- Faster performance for complex patterns
- Better security (avoids ReDoS attacks)
- POSIX-compliant matching

## Pattern Reference

### Basic Patterns

| Pattern | Meaning | Example |
|---------|---------|---------|
| `*` | Matches anything except `/` | `*.rs` matches `main.rs` but not `src/main.rs` |
| `**` | Matches anything including `/` | `**/*.rs` matches any Rust file |
| `?` | Matches single character | `file?.txt` matches `file1.txt` |
| `[abc]` | Matches character class | `[abc].rs` matches `a.rs`, `b.rs`, `c.rs` |
| `[!abc]` | Negated character class | `[!abc].rs` matches `x.rs` |

### Special Characters

| Pattern | Meaning |
|---------|---------|
| `\` | Escape character |
| `!` | Negate pattern (at start) |

### Path Separators

- **Unix/Linux/macOS**: `/` (forward slash)
- **Windows**: Both `/` and `\` are treated as path separators

## .garraignore Syntax

The `.garraignore` file uses the same syntax as `.gitignore` with some additions:

### Basic Rules

```
# Comment line
*.log           # Ignore all .log files
build/          # Ignore build directory
src/*.tmp       # Ignore .tmp files in src/
**/.DS_Store   # Ignore all .DS_Store files
```

### Pattern Precedence

1. ** negation patterns** (starting with `!`) have highest priority
2. **More specific patterns** take precedence over less specific
3. **Order matters** for patterns at the same level

### Special Patterns

```
# Ignore everything in node_modules
node_modules/

# But not the package.json
!node_modules/package.json

# Ignore all log files except error.log
*.log
!error.log

# Ignore all .env files in any directory
**/.env
```

## Compatibility

### .gitignore Compatibility

GarraRUST's `.garraignore` is designed to be compatible with `.gitignore`:

- Same basic pattern syntax
- Same precedence rules
- Same special patterns (`**`, `?`, `[]`)

### Additions for GarraRUST

| Pattern | Description |
|---------|-------------|
| `# glob: <pattern>` | Explicit glob mode |
| `# regex: <pattern>` | Regex mode (advanced) |

## Performance Considerations

### Guardrails

To prevent performance issues:

1. **Max depth**: Default 20 directory levels
2. **Max files**: Default 10,000 files per traversal
3. **Timeout**: Default 30 seconds per operation
4. **Pattern complexity limit**: Max 100 pattern terms

### Security

- **ReDoS protection**: Picomatch prevents catastrophic backtracking
- **Path traversal**: Block `../` patterns that escape workspace
- **Symlink loops**: Detect and skip circular symlinks

## API Usage

```rust
use garraia_glob::{GlobMatcher, MatchOptions};

// Create matcher
let matcher = GlobMatcher::new(
    vec!["*.rs".to_string(), "src/**".to_string()],
    MatchOptions::default(),
)?;

// Check if path matches
if matcher.matches("src/main.rs") {
    println!("Matched!");
}

// Iterate matches
for path in matcher.iter_walk(".")? {
    println!("{}", path.display());
}
```

## Examples

### Common Patterns

```
# Rust project
target/
*.rs.bk
Cargo.lock

# Node project
node_modules/
dist/
*.log

# IDE
.vscode/
.idea/
*.swp

# OS
.DS_Store
Thumbs.db
```

### Complex Patterns

```
# Ignore all .txt except README
*.txt
!README.txt

# Ignore all in vendor except specific
vendor/*
!vendor/special/

# Ignore all log files in any subdirectory
**/*.log
```

## Testing

Run tests with:

```bash
cargo test -p garraia-glob
```

## References

- [picomatch](https://github.com/micromatch/picomatch) - The underlying matcher
- [gitignore man page](https://git-scm.com/docs/gitignore) - Pattern reference
