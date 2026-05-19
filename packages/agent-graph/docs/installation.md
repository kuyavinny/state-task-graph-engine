# Installation

## Prerequisites

- **Rust** 1.70+ (for building from source)
- **Cargo** (included with Rust)

## Build from Source

```bash
git clone https://github.com/kuyavinny/agent-system-os.git
cd agent-system-os
cargo install --path packages/agent-graph
```

This installs the `stage` binary to `~/.cargo/bin/`. Make sure `~/.cargo/bin` is on your `PATH`.

## Verify Installation

```bash
stage --version
stage --help
```

## Build in Release Mode

For optimized performance:

```bash
cargo build --release -p agent-graph
```

The binary is at `target/release/stage`. You can copy it to any directory on your `PATH`:

```bash
cp target/release/stage /usr/local/bin/stage
```

## Cross-Compilation

To build for a different target:

```bash
# Add the target
rustup target add x86_64-unknown-linux-musl

# Build
cargo build --release -p agent-graph --target x86_64-unknown-linux-musl
```

Common targets:
- `x86_64-unknown-linux-musl` — Static Linux binary (no glibc dependency)
- `aarch64-unknown-linux-gnu` — ARM64 Linux
- `x86_64-apple-darwin` — macOS Intel
- `aarch64-apple-darwin` — macOS Apple Silicon

## Running Tests

```bash
# All tests
cargo test -p agent-graph

# Unit tests only
cargo test -p agent-graph --lib

# Integration tests only
cargo test -p agent-graph --test init
cargo test -p agent-graph --test reconcile
cargo test -p agent-graph --test validate

# With output
cargo test -- --nocapture
```

## Linting

```bash
cargo clippy -p agent-graph --all-targets -- -D warnings
cargo fmt --all -- --check
```