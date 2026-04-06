# GarraIA WASM Plugin SDK

Build plugins for GarraIA using Rust and WebAssembly.

## Overview

GarraIA plugins are compiled to WASM (WebAssembly) and run inside a sandboxed Wasmtime runtime. Each plugin gets its own isolated execution environment with capability-based permissions.

## Quick Start

### 1. Create a new plugin project

```bash
cargo new --lib my-garraia-plugin
cd my-garraia-plugin
```

### 2. Configure `Cargo.toml`

```toml
[package]
name = "my-garraia-plugin"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

### 3. Implement the plugin

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct Output {
    success: bool,
    data: String,
}

fn main() {
    // Read input from stdin
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).unwrap();
    
    // Process
    let result = Output {
        success: true,
        data: format!("processed: {}", input.trim()),
    };
    
    // Write output to stdout
    println!("{}", serde_json::to_string(&result).unwrap());
}
```

### 4. Compile to WASM

```bash
# Add the WASI target
rustup target add wasm32-wasip1

# Build
cargo build --target wasm32-wasip1 --release
```

### 5. Create the plugin manifest

Create `plugin.toml` in your plugin directory:

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "My custom GarraIA plugin"

[permissions]
filesystem = false
network = []
env_vars = []

[limits]
timeout_secs = 30
max_memory_mb = 64
max_output_bytes = 1048576
```

### 6. Install the plugin

Copy your plugin directory to `~/.garraia/plugins/my-plugin/`:

```
~/.garraia/plugins/my-plugin/
  plugin.toml
  my-plugin.wasm  (or plugin.wasm)
```

## Plugin Manifest Format

The `plugin.toml` file describes your plugin:

```toml
[plugin]
name = "plugin-name"          # Required: alphanumeric + hyphens
version = "1.0.0"             # Required: semver
description = "What it does"  # Required

[permissions]
filesystem = false                       # Enable filesystem access
filesystem_read_paths = ["./data"]       # Read-only directories (relative to plugin root)
filesystem_write_paths = ["./output"]    # Read-write directories
network = ["api.example.com"]            # Allowlisted network domains
env_vars = ["MY_API_KEY"]               # Environment variables to pass through

[limits]
timeout_secs = 30        # Max execution time (default: 30)
max_memory_mb = 64       # Max memory usage in MiB (default: 64)
max_output_bytes = 1048576  # Max stdout/stderr bytes (default: 1MB)
```

## Available Host Functions

Plugins can call these host functions to interact with GarraIA:

| Function | Description |
|----------|-------------|
| `send_message` | Send a chat message to a channel/session |
| `read_file` | Read a file from the host filesystem (scoped) |
| `http_request` | Make an HTTP request (allowlisted domains only) |
| `log` | Write a log entry to the host's tracing system |
| `get_config` | Read plugin configuration values |
| `set_state` | Store persistent plugin state |
| `get_state` | Retrieve previously stored plugin state |

### `send_message`

Send a message to a chat session or channel.

```json
{
  "target": "session-id-or-channel",
  "content": "Hello from my plugin!",
  "metadata": { "plugin": "my-plugin" }
}
```

### `read_file`

Read a file within the plugin's allowed filesystem scope.

```json
{
  "path": "data/config.json",
  "max_bytes": 1048576
}
```

Returns:
```json
{
  "content": "file contents...",
  "size": 1234,
  "truncated": false
}
```

### `http_request`

Make an HTTP request to an allowlisted domain.

```json
{
  "method": "POST",
  "url": "https://api.example.com/translate",
  "headers": { "Content-Type": "application/json" },
  "body": "{\"text\":\"hello\",\"target\":\"es\"}",
  "timeout_secs": 10
}
```

### `log`

Write a structured log entry.

```json
{
  "level": 2,
  "message": "Processing completed successfully"
}
```

Levels: 0=Trace, 1=Debug, 2=Info, 3=Warn, 4=Error

## Example Plugins

### Translator Plugin

```rust
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Deserialize)]
struct TranslateInput {
    text: String,
    target_lang: String,
}

#[derive(Serialize)]
struct TranslateOutput {
    success: bool,
    translated: String,
    source_lang: String,
    target_lang: String,
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    
    let req: TranslateInput = serde_json::from_str(&input).unwrap();
    
    // In a real plugin, call a translation API via http_request
    let output = TranslateOutput {
        success: true,
        translated: format!("[{}] {}", req.target_lang, req.text),
        source_lang: "auto".into(),
        target_lang: req.target_lang,
    };
    
    println!("{}", serde_json::to_string(&output).unwrap());
}
```

### Code Formatter Plugin

```rust
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Deserialize)]
struct FormatInput {
    code: String,
    language: String,
}

#[derive(Serialize)]
struct FormatOutput {
    success: bool,
    formatted: String,
    changes: usize,
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    
    let req: FormatInput = serde_json::from_str(&input).unwrap();
    
    // Simple indentation fixer as example
    let formatted = req.code
        .lines()
        .map(|line| line.trim_start().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    
    let changes = if formatted != req.code { 1 } else { 0 };
    
    let output = FormatOutput {
        success: true,
        formatted,
        changes,
    };
    
    println!("{}", serde_json::to_string(&output).unwrap());
}
```

### Summarizer Plugin

```rust
use serde::{Deserialize, Serialize};
use std::io::Read;

#[derive(Deserialize)]
struct SummarizeInput {
    text: String,
    max_sentences: Option<usize>,
}

#[derive(Serialize)]
struct SummarizeOutput {
    success: bool,
    summary: String,
    original_length: usize,
    summary_length: usize,
}

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    
    let req: SummarizeInput = serde_json::from_str(&input).unwrap();
    let max = req.max_sentences.unwrap_or(3);
    
    // Simple extractive summary: take first N sentences
    let sentences: Vec<&str> = req.text.split(". ").collect();
    let summary = sentences.iter()
        .take(max)
        .copied()
        .collect::<Vec<_>>()
        .join(". ");
    
    let output = SummarizeOutput {
        success: true,
        original_length: req.text.len(),
        summary_length: summary.len(),
        summary,
    };
    
    println!("{}", serde_json::to_string(&output).unwrap());
}
```

## Security Model

Plugins run in a sandboxed environment with these protections:

- **Memory isolation**: Each plugin has its own WASM linear memory, capped by `max_memory_mb`
- **Execution timeout**: Enforced via Wasmtime epoch interruption
- **Filesystem scoping**: Only explicitly allowed directories are accessible
- **Network filtering**: DNS resolution checks prevent SSRF (private IPs blocked)
- **Output limiting**: stdout/stderr are capped to prevent memory exhaustion

## Publishing

### Package your plugin

```bash
# Directory structure
my-plugin/
  plugin.toml
  my-plugin.wasm
  README.md (optional)
```

### Using `garraia plugin publish`

```bash
# Build and package
cargo build --target wasm32-wasip1 --release
cp target/wasm32-wasip1/release/my_plugin.wasm ./my-plugin.wasm

# Publish (future: plugin registry)
garraia plugin publish ./my-plugin/
```

## Template: Cargo Generate

```bash
# Future: cargo-generate template
cargo generate --git https://github.com/michelbr84/garraia-plugin-template
```

## API Reference

### Plugin Installation API

```http
POST /api/plugins/install
Content-Type: application/json

{
  "url": "https://example.com/my-plugin-manifest.json"
}
```

### List Installed Plugins

```http
GET /api/plugins
```

### Plugin Details

```http
GET /api/plugins/{id}
```

### Toggle Plugin

```http
POST /api/plugins/{id}/toggle
Content-Type: application/json

{ "enabled": true }
```

### Uninstall Plugin

```http
DELETE /api/plugins/{id}
```
