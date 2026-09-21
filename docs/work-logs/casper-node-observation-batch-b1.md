# Casper Node Observation: Batch B1 Implementation

---
handoff_status: ready
next_steps:
  - Commit the Batch B1 change with separate user consent.
  - Run source-bound verification and obtain explicit acceptance of CLAIM-CASPER-NODE-OBSERVATION-002.
  - Prepare the Batch B2 final file list, limits, reference algorithm, and tests for separate approval.
---

## Authorization

The user confirmed the nine-file Batch B1 scope, the pending claim, and the four high-weight mandatory tags on 2026-09-21. The [Batch B review](../plans/casper-node-observation-batch-b.md) defines that scope.

The branch is `feature/casper-node-observation` at `799e2136adc6e0100b289945d9a5a6851e81c91f`. Batch B2 and Batch C remain unapproved.

## Implemented files

| File | Change |
| --- | --- |
| `shared/src/rust/store/soak_snapshot.rs` | Bounded LMDB reader. It opens one read transaction per environment, records transaction identities, checks raw lengths before copying, checks length prefixes before decoding, and rechecks every environment at validation. |
| `shared/src/rust/store/mod.rs` | Registers the reader module. |
| `shared/tests/soak_snapshot.rs` | Eleven tests for limits, unsupported backends, raw length, budgets, malformed prefixes, environment changes, restored values, unchanged bytes, one-transaction-per-thread rules, and separate environments. |
| `block-storage/src/rust/dag/soak_snapshot.rs` | Capture limits, detached snapshot data, availability states, canonical encoding, SHA-256 digest, and independent scratch construction. |
| `block-storage/src/rust/dag/mod.rs` | Registers the snapshot module. |
| `block-storage/src/rust/dag/block_dag_key_value_storage.rs` | Adds a checked capture boundary that holds the global read guard and the metadata read guard with a finite wait. |
| `block-storage/src/rust/dag/block_metadata_store.rs` | Adds a checked constructor, a narrow typed-store accessor, and a guarded state copy. The existing constructor is unchanged. |
| `block-storage/src/rust/key_value_block_store.rs` | Adds observer-only bounded block decoding with compressed, decompressed, and expansion limits. The production decoder is unchanged. |
| `block-storage/tests/soak_snapshot.rs` | Fourteen tests over two LMDB environments for copy fidelity, parent order, availability, finalization and cache writes without generation change, block environment changes, mutation survival, scratch independence, unchanged production bytes, incomplete rows, and bounded decoding. |

No dependency, lockfile, storage format, or production write path changed.

## Source findings applied

The stored key and value bytes carry an eight-byte little-endian length prefix from the store codec. The reader checks the raw length before it copies a value, and it checks the prefix before it returns the payload.

LMDB reports the last committed transaction identity through environment information. The reader requires that identity to equal the read transaction identity at open and again at validation.

LMDB thread-local reader slots allow one read transaction per thread per environment. The reader shares one transaction across stores of one environment. A shared test shows that a second same-thread open fails with an explicit error.

The block-storage tests write from a separate thread while a capture holds its transactions. Those writes touch persisted metadata, a cache row, and the block environment without changing the insertion generation, and each capture is rejected.

## Local results

| Check | Result |
| --- | --- |
| `cargo test -p shared` | 102 library tests and 11 capture tests passed. |
| `cargo test -p block-storage` | 68 library tests, 8 buffer transition tests, 38 DAG storage tests in two targets, 1 carrier property test, 3 loom tests, and 14 capture tests passed. |
| `cargo clippy -p shared -p block-storage --all-targets -- -D warnings` | Passed. |
| `cargo fmt --check -p shared -p block-storage` | Passed. |
| `cargo check --workspace` | Passed after the module additions. |

The [Batch B1 report](../cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/report.json) records the source digests and these results.

## Claim and records

`CLAIM-CASPER-NODE-OBSERVATION-002` is registered in the [claim file](../claims/casper-node-authority-snapshot.md) with status pending.

Pending records exist for the four tagged files and for the DAG storage file. The DAG storage record does not discharge or waive its existing `CLAIM-FINALITY-002` obligations.

## Boundaries

The tests use LMDB environments on the local filesystem and a test executable. No node, live adapter, campaign image, or consensus result was qualified.

The assistant did not commit, push, switch branches, merge, publish images, or repin candidates.
