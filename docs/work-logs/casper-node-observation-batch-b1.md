# Casper Node Observation: Batch B1 Implementation

---
handoff_status: paused
next_steps:
  - Review the source-bound evidence for the completed B1 corrections before claim acceptance.
  - Run source-bound verification and obtain explicit acceptance of CLAIM-CASPER-NODE-OBSERVATION-002.
  - Prepare the Batch B2 final file list, limits, reference algorithm, and tests for separate approval.
  - Complete the Batch C writer review and publication contract before requesting implementation approval.
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

## Continuation review on 2026-09-21

The user requested continuation on this branch after another writer supplied the initial B1 implementation.

The checkout was clean at `2ccc4ae0ac1045232c247ecd76925e13fabd0ade`. The local branch was two commits ahead of its remote.

Commit `a38d44185c73923a769bd204ed5f6ba0e110ca39` contains the initial B1 implementation. Commit `2ccc4ae0ac1045232c247ecd76925e13fabd0ade` contains the separate dependency update.

Both commits existed before this continuation. This continuation made no commit or push.

### Corrections and retained failures

The reader tests reproduced five failures against the existing implementation:

- Failed scans did not retain consumed record, byte, or operation budgets.
- Empty scans did not consume an operation.
- Scans did not apply the per-buffer limit to keys.
- Lookups encoded oversized keys without a length check.
- Reader construction inspected backends before checking the supplied store count.

The reader now checks these limits before the applicable allocation or store operation. Failed scans retain their consumed budgets.

The reader now checks record budgets before copying lookup values. Checked arithmetic rejects counter overflow.

Scans decode each row directly into the result map. Scans no longer allocate an intermediate copy of every raw row.

The byte counter retains its existing accounting scope. Lookup records count raw values, and scanned records count raw keys and values.

The operation counter includes empty scans and the final cursor step. The operation limit also bounds the supplied store count.

A separate decoder test reproduced acceptance of incomplete decompression. Zero-filled buffer bytes completed a valid Protocol Buffers field despite the incorrect declared length.

The observer decoder now requires the decompressor's returned byte count to equal the declared length. The production decoder remains unchanged.

### Current verification

The [continuation report](../cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/report.json) binds the tested working-tree sources and retained evidence.

| Check | Result |
| --- | --- |
| Initial reader regression run | Five failures and twelve passes reproduced the reader defects. |
| Initial decoder regression run | One failure reproduced the decompression defect. |
| `cargo test --locked -p shared -p block-storage` | All 290 test executions passed, including 17 reader tests and 15 capture tests. |
| `cargo clippy --locked -p shared -p block-storage --all-targets -- -D warnings` | Passed. |
| `cargo fmt --check -p shared -p block-storage` | Passed. |
| `cargo check --locked --workspace` | Passed. |
| Active language-server checks for four changed Rust files | Three checks completed without findings. One check timed out. |
| Changed-artifact CbC gate | Exit 4. All three changed mandatory artifacts remain pending. |

The total includes the DAG tests that two test targets execute. It is not a count of unique behaviors.

The STE Check passed against the unchanged legacy baseline. No human STE Review or full ASD-STE100 conformance is claimed.

The tests ran natively on Linux/aarch64. No isolated rebuild, live node, hosted proof, campaign, or enforcement test ran.

The retained evidence includes failing source snapshots, logs, final source hashes, and post-run copies of the two snapshot test executables.

The failing executables were not retained separately. The source snapshots and logs identify those failures, not the later executable copies.

### Remaining B1 work at the hardening checkpoint

These source findings remained open at that checkpoint. The completion review below records the subsequent corrections.

- Metadata decoding still precedes parent-count checks. Nested justifications and electorate maps need limits before allocation.
- Capture work does not yet account for every collection traversal and output copy.
- Scratch construction creates metadata and cache stores but does not create a block store from captured bodies.
- Canonical encoding does not independently encode the public duplicate hash and parent fields that detached blocks expose.

The canonical identity review must cover every field that scratch construction and evaluation consume. It must also check duration encoding and mutable snapshot inputs.

Batch B2 remains dependent on corrected capture bounds, independent reference evaluation, adopted configuration, and an approved final file list.

Batch C still requires a complete writer inventory and a publication consistency contract. This continuation does not approve either batch.

`CLAIM-CASPER-NODE-OBSERVATION-002` remains pending. No live capability, harness guard, candidate pin, or cloud budget changed.

## B1 completion review

The user requested completion of the four B1 corrections. The user also requested a minimum-code review in the final epic task.

The tested working-tree sources derive from `6198821283b960c3aafae1e8c35966b883dfd2f3`. Source manifests identify the tested bytes.

Another writer committed `566a21223830eb6594467615eb45fe3e4ccb3c2e` during this work. That commit added EPIC-019 and changed only `docs/ToDos.md`.

The initial evidence check rejected the changed HEAD. Review confirmed that the commit changed no Rust source, dependency, or claim file.

Per-command checkout revisions were not recorded. The evidence retains both revisions and does not attribute all tests to either commit.

The [completion report](../cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/report.json) binds the source files and retained evidence.

### Corrections

- Metadata wire checks now bound nested byte strings, parents, justifications, and electorate maps before typed decoding.
- Capture checks requested body counts before acquiring guards. Scans have per-store record limits and decreasing read budgets.
- Work accounting includes state traversal, copied bytes, decoded input, canonical output, and digest input. Checked arithmetic rejects overflow.
- Canonical output has a byte limit. The encoder checks each append before extending its buffer.
- Scratch block stores use captured encoded bodies. Each scratch view receives independent block, metadata, floor, and frontier stores.
- Snapshot schema 2 removes duplicate block hashes and parent lists. Metadata supplies those fields.
- The canonical identity includes all detached inputs, usage, work, and the complete duration. Captured data exposes no mutable access.
- The approved-block store joins the declared exclusions. Empty lifecycle, carrier, and approved-block stores do not represent durable observations.

Work units count logical visits and bytes. They do not count processor instructions, allocator overhead, or elapsed time.

The work limit applies to capture. Scratch construction uses bounded retained data, but it is not a metered B2 evaluator.

Metadata wire checks follow the pinned bincode schema. The existing codec and compression libraries remain trusted dependencies.

### Verification

Four new regressions failed before implementation. They covered nested metadata bounds, canonical work, duration truncation, and requested-body limits.

The final run passed 301 test executions, including 19 reader tests and 24 capture tests. This count includes repeated DAG tests across targets.

Exact-boundary tests cover work and canonical bytes. Additional tests cover malformed metadata, decreasing budgets, and independent scratch block mutations.

Later review moved aggregate parent-budget checks before typed decoding. The new regression initially used an edge limit below the five child edges in the fixture.

The corrected test requires aggregate parent rejection before a later malformed field can reach decoding. The evidence retains both failed runs with the incorrect fixture limit.

A positive compile check accepted read-only access. A negative compile check rejected mutable snapshot access with `E0596`.

Clippy, formatting, and the workspace build passed. All four final language-server checks completed without findings.

The tests ran natively on Linux/aarch64. No allocation instrumentation, independent rebuild, formal proof, hosted verification, live node, or campaign ran.

The evidence retains red sources, logs, source archives, and final executable copies. Final executable copies do not identify the earlier failing executable.

### Remaining gates

The four implementation findings have local corrections and regression coverage. The claim and all verification tiers remain pending.

The changed-artifact CbC gate returned exit 4. All four changed mandatory artifacts remain pending.

The STE Check passed against the unchanged baseline. No human STE Review or full ASD-STE100 conformance is claimed.

The final node-epic task, [`TASK-019-6`](../ToDos.md#epic-019-casper-node-observation-interface), now requires a review of the minimum necessary codebase changes.

This direct requirement replaces the initial linked proposal for the separate harness epic. Task ownership and completion status remain unchanged.

Batch B2 and Batch C remain unapproved. This continuation made no commit or push.
