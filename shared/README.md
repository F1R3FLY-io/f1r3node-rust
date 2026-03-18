# shared

Common utilities and storage primitives shared across the Rust workspace.

## Responsibilities

- Key-value store traits and LMDB-backed implementations
- Event publishing helpers and event stream types
- Compression helpers
- DAG and collection utilities
- Metrics helpers such as `MetricsSemaphore`
- Shared gRPC server wrappers

## Build

```bash
cargo build -p shared
cargo build --release -p shared
```

## Test

This crate currently relies mostly on downstream integration coverage:

```bash
cargo test -p shared
```

## Key Source Areas

| Path | Purpose |
| --- | --- |
| `src/rust/store/` | Store traits and LMDB-backed implementations |
| `src/rust/shared/` | Events, printers, recent hash filters, helpers |
| `src/rust/grpc/` | Shared gRPC server helpers |
| `src/rust/dag/` | DAG utility functions |
| `src/rust/metrics_semaphore.rs` | Metrics-aware concurrency primitive |
