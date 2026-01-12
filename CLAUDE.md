# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

tanu-allure is an Allure reporter plugin for [tanu](https://github.com/tanu-rs/tanu), a Rust HTTP testing framework. It generates Allure-compatible JSON reports with test steps, HTTP call traces, and execution history tracking.

## Build Commands

```bash
# Build entire workspace (library + example)
cargo build --workspace --all-targets --all-features

# Run tests with nextest (used in CI)
cargo nextest run --retries 3 --no-tests=pass

# Run a single test
cargo test test_name

# Lint with clippy (must pass in CI)
cargo clippy --workspace --all-targets --all-features -- --deny clippy::all

# Format check
cargo fmt --check
```

## Running the Example

```bash
# Run example test against httpbin.org
cargo run --manifest-path example/Cargo.toml test --reporters allure,list

# Generate HTML report from results
allure generate allure-results --clean -o allure-report
```

## Architecture

### Core Components

- **`src/adapter.rs`** - `AllureReporter` implementation of `tanu_core::Reporter` trait. Buffers test events (checks, HTTP calls) and outputs JSON on test completion.

- **`src/models.rs`** - Allure JSON schema types (`TestResult`, `Step`, `Status`, `Label`, etc.) and history tracking types. Contains `generate_history_id()` for deterministic test identification using SHA-256.

### Event Flow

1. Test execution triggers `on_check()` and `on_http_call()` callbacks
2. Events are buffered per test in an `IndexMap` keyed by `(project, module, test_name)`
3. `on_end()` converts buffered events to `TestResult` and writes `{uuid}-result.json`
4. `on_summary()` writes `history/history.json` for trend tracking across runs

### Key Implementation Details

- Sensitive headers (Authorization, Cookie, X-API-Key, etc.) are automatically masked
- History ID uses SHA-256 of `project::module::test_name` + non-excluded parameters
- Test status mapping: `Ok` → Passed, `ErrorReturned` → Failed, `Panicked` → Broken
- History retains up to 20 runs per test (`MAX_HISTORY_ITEMS`)

## Allure JSON Schema Reference

Rust implementation: `src/models.rs`

- [Test results](https://allurereport.org/docs/how-it-works-test-result-file/)
- [Container](https://allurereport.org/docs/how-it-works-container-file/)
- [Categories](https://allurereport.org/docs/how-it-works-categories-file/)
- [Environment](https://allurereport.org/docs/how-it-works-environment-file/)
- [Executor](https://allurereport.org/docs/how-it-works-executor-file/)
- [History](https://allurereport.org/docs/how-it-works-history-files/)

## Versioning

Update version in two places:

1. `Cargo.toml` - the `version` field
2. `README.md` - the version in the Installation section

```bash
# Example: bumping from 0.5.1 to 0.6.0
# Cargo.toml:  version = "0.5.1"  →  version = "0.6.0"
# README.md:   tanu-allure = "0.5"  →  tanu-allure = "0.6"
```

The example uses a path dependency (`path = ".."`), so no version update is needed there.

To release:
1. Update versions in `Cargo.toml` and `README.md`
2. Commit and push to main
3. Create a GitHub release with a tag matching the version (e.g., `v0.6.0`)
4. The `release.yaml` workflow automatically publishes to crates.io

## CI Workflows

- **test.yml** - Runs on PRs and pushes: build, clippy, nextest
- **publish-report.yml** - Weekly: runs tanu integration tests, generates Allure v2/v3 reports, publishes to GitHub Pages
- **release.yaml** - On GitHub release: publishes to crates.io
