# State & Task Graph Engine

A Rust CLI for managing DAG-based task graphs for LLM agents. Provides strict state machine enforcement, optimistic concurrency via graph revision, task claiming with lease recovery, and bounded context views to prevent LLM context bloat.

## Quick Start

```bash
# Initialize a new task graph
stg init

# Check graph status
stg status

# Get next available task
stg next
```

## Development

```bash
cargo build
cargo test
cargo clippy -- -D warnings
cargo fmt -- --check
```

## Architecture

See `.rpiv/artifacts/` for PRD, technical spec, and implementation plan.