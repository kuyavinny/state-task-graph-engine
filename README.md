# agent-system-os

A modular system for building and orchestrating LLM-powered agents.

## Packages

| Package | Description |
|---------|-------------|
| [`packages/agent-graph`](packages/agent-graph/) | State & task graph engine for agents. Manages DAG-based task graphs with state-machine enforcement, optimistic concurrency, lease-based claiming, and bounded LLM context views. CLI binary: `stage` |
| `packages/agent-adapter` | _Planned_ — Agent adapter layer |

## Quick Start

```bash
# Clone the monorepo
git clone https://github.com/kuyavinny/agent-system-os.git
cd agent-system-os

# Build and install the CLI
cargo install --path packages/agent-graph

# Verify
stage --version
stage --help
```

## Workspace Commands

```bash
# Build all packages
cargo build --workspace

# Test all packages
cargo test --workspace

# Lint all packages
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Repository Structure

```
agent-system-os/
├── Cargo.toml              # Workspace root
├── packages/
│   └── agent-graph/        # state-task graph engine
│       ├── Cargo.toml
│       ├── src/
│       ├── tests/
│       ├── docs/
│       ├── fixtures/
│       └── README.md
└── .gitignore
```

## License

MIT