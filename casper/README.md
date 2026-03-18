# casper

Consensus and block-processing engine for the Rust node.

## Responsibilities

- Genesis creation and validator ceremony flows
- Block processing, validation, and finalization
- Safety and synchrony checks
- Approved block, block retrieval, and last-finalized-state recovery logic
- Casper-facing APIs used by the node crate

## Build

```bash
cargo build -p casper
cargo build --release -p casper
```

## Test

```bash
cargo test -p casper
cargo test -p casper --release
```

To expose the crate's test helpers to other workspace members:

```bash
cargo test -p casper --features test-utils
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rust/engine/` | Node state machine, LFS recovery, genesis startup |
| `src/rust/blocks/` | Block processing |
| `src/rust/finality/` | Finalization logic |
| `src/rust/safety/` | Safety oracle and clique logic |
| `src/rust/api/` | Block and DAG API helpers |
| `src/rust/util/` | Genesis, deploy, DAG, and RSpace utilities |
| `src/main/resources/` | Contracts and runtime resources used by genesis and system deploys |

## Dependencies

This crate coordinates closely with:

- `block-storage` for persistence
- `comm` for network messaging
- `models` for protocol structures
- `rholang` and `rspace++` for contract execution and tuple space state
