# Test Guide

This document summarizes test architecture, validation method, and key commands for `hulunote-app`.

Testing principle: tests exist to expose real business-logic bugs, not to force green results; keep assertions in test/spec, keep non-semantic helpers linear and action-oriented, avoid self-healing branches/exceptions, prefer state-driven synchronization, and avoid regex in tests by default.

## Test Architecture

- **Host Rust tests**: fast feedback for pure logic and non-browser behavior.
- **WASM/browser tests**: DOM/runtime behavior via `wasm-bindgen-test`.
- **Playwright E2E scripts**: workflow-level browser checks in a real UI session.

Host and wasm tests are separate suites and must be run independently.

## Validation Method

Use this order for reliable feedback:

```bash
# Host tests
cargo test

# WASM/browser tests
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
  cargo test --target wasm32-unknown-unknown

# E2E (Playwright)
npx playwright test
```

## WASM Test Prerequisites

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

`wasm32` browser tests run through `wasm-bindgen-test-runner`, and that runner requires a browser + WebDriver.

### WASM policy

- For wasm/editor behavior changes, run wasm tests explicitly.
- Preferred command:
  - `CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --target wasm32-unknown-unknown`
- If wasm tests are not feasible in current environment, state the blocker and provide the exact command for local verification.

### WebDriver notes

Install browser drivers as needed:

```bash
# Safari (macOS)
safaridriver --enable

# ChromeDriver (macOS/Homebrew)
brew install chromedriver
```

Use `webdriver.json` in repo root to provide local WebDriver capabilities overrides.

## Playwright E2E Scripts

Install dependencies:

```bash
npm install
```

Required environment variables (when login session is not already cached):

```bash
export HULUNOTE_E2E_EMAIL='your-email@example.com'
export HULUNOTE_E2E_PASSWORD='your-password'
```

Example command:

```bash
npm run test:e2e
```

Detailed E2E principles:

- `docs/E2E_GUIDE.md`

## Playwright MCP Manual Verification

When an E2E case fails but behavior appears correct in a real browser, verify with Playwright MCP before changing production code.

Recommended checks:

- Open the app in MCP browser and reproduce the same workflow manually.
- Inspect outline row count / depth changes after `Enter`, `Tab`, `Shift+Tab`, drag, and soft break.
- Compare MCP-observed behavior with test assertions to locate flaky waits/selectors.

This is a verification path for test reliability, not a replacement for automated E2E coverage.
```
