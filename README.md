# hulunote-app

A modern web client for [hulunote](https://github.com/hulunote/hulunote), an open-source outliner-style note-taking service.

## Overview

This client is built with [Leptos](https://leptos.dev/) and [Rust/UI](https://www.rust-ui.com/). It connects to the [hulunote-rust](https://github.com/hulunote/hulunote-rust) backend API.

## Features

### Notes
- Create and organize notes as an outline (nested blocks)
- Drag and drop to reorder blocks
- Daily notes for journaling and quick capture

### Linking
- Bidirectional links between pages
- Backlinks (see what links to the current page)

### Navigation
- Workspaces to separate personal/work/projects
- Fast switching between workspaces, notes, and blocks

### Search
- Full-text search across your workspace

### Import / Export
- Import and export notes (Markdown, JSON)

### Integrations
- MCP support (connect AI tools to your notes)

### Settings
- Customize the app to fit your workflow

## Getting Started

### Prerequisites

```bash
# Install Rust
rustup install stable

# Add WASM target
rustup target add wasm32-unknown-unknown

# Install Trunk
cargo install trunk

# Install Tailwind CSS CLI (Trunk will invoke this to compile Tailwind)
brew install tailwindcss
```

### Development

```bash
# Start dev server(with auto-rebuild)
trunk serve
```

### Tests

Run **both** suites when validating changes (host unit tests + browser-based WASM tests).

```bash
# Unit tests (host)
cargo test

# WASM tests (browser)
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
cargo test --target wasm32-unknown-unknown
```

Prereqs + WebDriver setup are documented in:
- [docs/TEST_GUIDE.md](./docs/TEST_GUIDE.md)

### Production Build

```bash
trunk build --release
```

### Environment Configuration

The app reads configuration from `window.ENV` in the browser. To customize the API URL:

```html
<script>
  window.ENV = {
    API_URL: "http://your-backend-url:6689"
  };
</script>
```

Or set the default in `src/lib.rs` via the `get_api_url()` function.


## Project Structure

```
hulunote-app/
├── src/
│   └── lib.rs         # Main app (components, API client, state)
├── index.html          # Entry HTML (Trunk + Tailwind pipeline)
├── trunk.toml         # Trunk build configuration
├── tailwind.config.js # Tailwind CSS configuration
├── public/
│   └── style.css      # Global styles with Tailwind directives
├── Cargo.toml         # Dependencies and WASM config
└── docs/              # Documentation
    ├── PRODUCT.md       # Product overview
    ├── API_REFERENCE.md
    ├── LEPTOS_GUIDE.md  # Leptos development guide
    └── TEST_GUIDE.md    # How to run unit/WASM tests
```

## Documentation

- [User Manual](./docs/USER_MANUAL.md)
- [Product Overview](./docs/PRODUCT.md)
- [API Contract](./docs/API_REFERENCE.md)
- [Leptos Development Guide](./docs/LEPTOS_GUIDE.md)
- [Test Guide](./docs/TEST_GUIDE.md)
- [Rust/UI Guide](./docs/RUST_UI_GUIDE.md)

## Desktop Build

To build for desktop, you have several options:

### Option 1: Tauri (Recommended)

Tauri can wrap the WASM application for native desktop deployment.

```bash
# Install Tauri CLI
cargo install tauri-cli

# Build for desktop
cargo tauri build
```

### Option 2: Web Desktop Wrappers

For a more lightweight desktop experience, consider:
- [nativefier](https://github.com/nativefier/nativefier) - Wrap the web app as a desktop app
- [Electron](https://www.electronjs.org/) - Create a desktop wrapper

## License

MIT
