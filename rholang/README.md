# rholang

Rholang interpreter and CLI crate for the Rust node.

## Responsibilities

- Parse and evaluate Rholang contracts
- Provide the `rholang-cli` binary for local execution
- Support contract execution used by `casper` and `node`
- Ship sample contracts and tutorials under `rholang/examples/`

## Build

```bash
cargo build -p rholang
cargo build --release -p rholang
cargo build --release --bin rholang-cli
```

## Test

```bash
cargo test -p rholang
cargo test -p rholang --release
```

## CLI Usage

Run from the repository root:

```bash
cargo run --bin rholang-cli -- rholang/examples/stdout.rho
```

Run from the crate directory:

```bash
cd rholang
cargo run --bin rholang-cli -- examples/stdout.rho
```

Show help:

```bash
cargo run --bin rholang-cli -- --help
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rholang_cli.rs` | CLI entry point |
| `src/lib.rs` | Library entry point |
| `examples/` | Runnable example contracts |
| `tests/` | Interpreter, matcher, accounting, and integration tests |

## Current Notes

- The crate depends on the external `rholang-parser` Git dependency.
- Some advanced language behavior is still under active refinement. Prefer running the release test suite for realistic performance and recursion behavior.
