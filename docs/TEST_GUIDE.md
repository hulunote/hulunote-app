# Test Guide

This document describes how to run tests for `hulunote-app`.

## 1) Unit Tests (host)

Run the standard Rust unit tests (fast feedback; covers pure Rust logic):

```bash
cargo test
```

## 2) WASM tests (wasm-bindgen-test)

WASM tests are a **separate test suite** from host tests.

- `cargo test --target wasm32-unknown-unknown` does **NOT** run host tests.
- `cargo test` (host) does **NOT** run wasm32 tests.

To avoid missing coverage, run **both** suites when validating changes.

Some tests are WASM-only (e.g., localStorage round-trips, DOM/contenteditable/Selection behavior). These require:
- `wasm32-unknown-unknown` target
- `wasm-bindgen-test-runner`
- A working WebDriver + browser

### 2.1 Install prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.108
```

### 2.2 Run WASM tests

```bash
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown
```

### 2.3 Recommended validation (avoid missing coverage)

For changes that may affect user behavior (especially DOM/editor behavior), run:

```bash
# Host suite
cargo test

# Browser suite
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown
```

### 2.3 WebDriver notes

- On macOS, the runner may default to Safari. You may need to enable Safari WebDriver:

```bash
safaridriver --enable
```

- If you use Chrome, **ChromeDriver major version must match Chrome major version**.
  - If you cannot upgrade Chrome, download a matching ChromeDriver and set:

```bash
export CHROMEDRIVER=/path/to/chromedriver
```

- `webdriver.json` can be used to provide additional capabilities to the runner.
  Keep this file local (do not commit it) because it is environment-specific.

## 3) Linting

```bash
cargo clippy --all-targets --all-features -- -D warnings
```

## 4) Formatting

```bash
cargo fmt --all
```
