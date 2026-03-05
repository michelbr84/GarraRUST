# Garra Desktop

Animated Clippy-style AI assistant overlay for the GarraIA ecosystem.
Built with [Tauri v2](https://tauri.app/) — transparent always-on-top window, system tray, global hotkey.

## Features

- **Animated parrot** — idle, thinking, and talking sprite animations
- **Speech bubble** — shows GarraIA responses in real time
- **Global hotkey** `Alt+G` — toggle input bar from anywhere on the desktop
- **WebSocket connection** to `garraia-gateway` on `ws://localhost:3888/ws/parrot`
- **System tray** with contextual menu (Open, Restart Gateway, Toggle Voice, Open Logs, Autostart, Quit)
- **Start with OS** — optional autostart via system login entry
- **Adaptive positioning** — bottom-right corner, adjusts for screen resolution and taskbar

## Requirements

- [GarraIA gateway](../../README.md) running on port `3888` (`cargo run -p garraia -- start`)
- Rust + [Tauri CLI](https://tauri.app/start/prerequisites/) (`cargo install tauri-cli`)

## Development

```bash
# From the workspace root
cargo tauri dev --manifest-path crates/garraia-desktop/src-tauri/Cargo.toml

# Or from the crate directory
cd crates/garraia-desktop/src-tauri
cargo tauri dev
```

The gateway must be running first:

```bash
cargo run -p garraia -- start
```

## Build (production)

```bash
cargo tauri build --manifest-path crates/garraia-desktop/src-tauri/Cargo.toml
```

Output bundles are placed in `target/release/bundle/`:

| OS      | Format   | Location                          |
|---------|----------|-----------------------------------|
| Windows | MSI      | `bundle/msi/*.msi`                |
| macOS   | DMG      | `bundle/dmg/*.dmg`                |
| Linux   | AppImage | `bundle/appimage/*.AppImage`      |

## Architecture

```
crates/garraia-desktop/
├── src-tauri/          # Rust backend (Tauri v2)
│   ├── src/
│   │   ├── lib.rs      # App setup, global shortcut (Alt+G)
│   │   ├── main.rs     # Binary entry point
│   │   ├── overlay.rs  # Transparent window creation + toggle
│   │   └── tray.rs     # System tray icon + menu
│   ├── capabilities/
│   │   └── default.json
│   ├── icons/
│   └── tauri.conf.json
└── ui/                 # WebView frontend (HTML/CSS/JS)
    ├── index.html
    ├── parrot.css
    ├── parrot.js       # Animation engine + WebSocket client
    └── assets/
        └── parrot-sprite.png   # 1280x600px sprite sheet (3 rows x 8 frames)
```

## Sprite sheet layout

`assets/parrot-sprite.png` — 1280 x 600 px, frames 160 x 200 px each:

| Row | State    | Frames |
|-----|----------|--------|
| 0   | idle     | 4      |
| 1   | thinking | 6      |
| 2   | talking  | 8      |

## WebSocket protocol

All messages are JSON.

**Client → Server**
```json
{ "type": "message", "text": "What is the weather?" }
```

**Server → Client**
```json
{ "type": "connected" }
{ "type": "thinking" }
{ "type": "response", "text": "..." }
{ "type": "error", "message": "..." }
```

The desktop always uses the fixed session ID `parrot-desktop` so conversation history
persists across gateway restarts and overlay reconnections.

## Tray menu

| Item              | Action                                               |
|-------------------|------------------------------------------------------|
| Open Garra        | Show/hide the overlay window                         |
| Restart Gateway   | Spawn `garraia start` (binary must be in PATH)       |
| Toggle Voice      | Emit voice-toggle event to overlay (future feature)  |
| Open Logs         | Open the app log directory in the file manager       |
| Start with OS     | Toggle autostart at login (checkmark indicates ON)   |
| Quit Garra        | Exit the application                                 |
