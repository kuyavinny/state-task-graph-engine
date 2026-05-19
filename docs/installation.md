# Installation

## Prerequisites

- **Rust** 1.70+ (for building from source)
- **Cargo** (included with Rust)

## Build from Source

```bash
git clone https://github.com/kuyavinny/state-task-graph-engine.git
cd state-task-graph-engine
cargo install --path .
```

This installs the `stg` binary to `~/.cargo/bin/`. Make sure `~/.cargo/bin` is on your `PATH`.

## Verify Installation

```bash
stg --version
stg --help
```

## Build in Release Mode

For optimized performance:

```bash
cargo build --release
```

The binary is at `target/release/stg`. You can copy it to any directory on your `PATH`:

```bash
cp target/release/stg /usr/local/bin/stg
```

## Cross-Compilation

To build for a different target:

```bash
# Add the target
rustup target add x86_64-unknown-linux-musl

# Build
cargo build --release --target x86_64-unknown-linux-musl
```

Common targets:
- `x86_64-unknown-linux-musl` — Static Linux binary (no glibc dependency)
- `aarch64-unknown-linux-gnu` — ARM64 Linux
- `x86_64-apple-darwin` — macOS Intel
- `aarch64-apple-darwin` — macOS Apple Silicon

## Running Tests

```bash
# All tests
cargo test

# Unit tests only
cargo test --lib

# Integration tests only
cargo test --test init
cargo test --test reconcile
cargo test --test validate

# With output
cargo test -- --nocapture
```

## Linting

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt -- --check
```