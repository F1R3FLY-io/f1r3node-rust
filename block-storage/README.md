# block-storage

Persistence layer for blocks, deploys, DAG metadata, and finality state.

## Responsibilities

- Store block bodies and metadata
- Persist deploy data and related indexes
- Maintain block DAG and equivocation tracking structures
- Keep last-finalized-state storage implementations

## Build

```bash
cargo build -p block-storage
cargo build --release -p block-storage
```

## Test

```bash
cargo test -p block-storage
cargo test -p block-storage --release
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rust/key_value_block_store.rs` | Main block store |
| `src/rust/dag/` | DAG metadata and equivocation stores |
| `src/rust/deploy/` | Deploy storage |
| `src/rust/finality/` | Finality state backends |
| `src/rust/casperbuffer/` | Casper buffer storage |
| `tests/` | Storage and DAG integration tests |
