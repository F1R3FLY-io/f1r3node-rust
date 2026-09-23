# Casper Node Authority Snapshot Capture

```yaml
claim_id: CLAIM-CASPER-NODE-OBSERVATION-002
status: pending
adapter: null
scope: batch-b1-bounded-detached-capture
artifacts:
  - shared/src/rust/store/soak_snapshot.rs
  - shared/src/rust/store/mod.rs
  - shared/tests/soak_snapshot.rs
  - block-storage/src/rust/dag/soak_snapshot.rs
  - block-storage/src/rust/dag/mod.rs
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs
  - block-storage/src/rust/dag/block_metadata_store.rs
  - block-storage/src/rust/key_value_block_store.rs
  - block-storage/tests/soak_snapshot.rs
  - formal/tlaplus/node_observation/BoundedCapture.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_admission_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_admission_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_deadline_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_deadline_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_order_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_order_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_open_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_open_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_validation_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_validation_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_generation_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_generation_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_incomplete_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_incomplete_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_bytes_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_bytes_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_release_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_release_unsafe.cfg
  - formal/tlaplus/node_observation/MC_BoundedCapture_write_unsafe.tla
  - formal/tlaplus/node_observation/MC_BoundedCapture_write_unsafe.cfg
  - formal/tlaplus/node_observation/README.md
  - formal/tlaplus/node_observation/verification-plan.json
  - formal/tlaplus/node_observation/bindings.json
  - scripts/ci/check-tla-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
  - scripts/ci/check-formal-invariants.sh
  - formal/rocq/node_observation/_CoqProject
  - formal/rocq/node_observation/theories/BoundedCapture.v
  - formal/rocq/node_observation/theories/MainTheorem.v
refutation: pending
construction: pending
binding: pending
soak: pending
```

## Authorization and scope

The user confirmed the nine-file Batch B1 scope on 2026-09-21. The user also ratified the four high-weight mandatory tags that the [Batch B review](../plans/casper-node-observation-batch-b.md) proposed.

Implementation starts from `feature/casper-node-observation` at `799e2136adc6e0100b289945d9a5a6851e81c91f`. That revision contains Batch A and its shutdown correction.

This claim covers bounded, detached capture of DAG storage data. It adds no socket operation, evaluator, finalizer observer, or live capability declaration.

The claim does not establish authority equivalence, live profile qualification, or node correctness. Batch B2 and Batch C keep their separate approval gates.

## Required properties

1. Capture requires explicit limits for value bytes, total bytes, records, operations, blocks, validators, edges, block decode expansion, capture work, and lock acquisition. Invalid limits are rejected before any guard or store access.
2. Capture acquires the DAG global read guard and the metadata read guard with a finite acquisition limit. A timeout rejects the capture.
3. Capture opens one read transaction for each participating LMDB environment. The DAG environment and the block environment are separate participants.
4. Each transaction identity is recorded with its environment path. The identity at open must equal the last committed transaction identity before open.
5. Raw values are length-checked before they are copied, and length-prefixed encodings are checked before typed decoding. Block bodies are checked against compressed, decompressed, and expansion limits before any decode buffer is allocated.
6. The in-memory DAG state is copied while the existing guards are held. Capture never calls the finalizer, the snapshot builder, or a production cache writer.
7. After all reads, capture rechecks every participating environment. A changed transaction identity rejects the capture, including a value that was changed and then restored.
8. A changed insertion generation also rejects the capture. An unchanged generation does not establish consistency on its own.
9. A held block with no metadata row or no requested body row is incomplete evidence and rejects the capture. A requested block outside the held DAG is recorded as not held. An absent floor or frontier cache row is recorded as absent.
10. Capture releases all production guards and transactions before the caller serializes or evaluates the snapshot. Production store bytes are unchanged by successful and rejected captures.
11. The canonical encoding sorts unordered collections, preserves ordered parent lists, and covers schema, scope, limits, coverage, generation, transaction identities, availability states, and cache seeds. The digest is SHA-256 over that encoding.
12. Scratch construction allocates new metadata, block, floor, and frontier stores from captured rows and rejects construction failure. Two scratch views share no mutable store with each other or with production. Lifecycle and carrier stores are excluded and their empty substitutes are declared as such.
13. A non-LMDB backend is rejected as unsupported. Capture never substitutes independent reads for a missing transaction identity check.

## Trust and evidence boundaries

The pinned LMDB library, its transaction identity counter, and its reader slot rules are trusted inputs. The shared tests exercise the rule that one thread cannot hold two read transactions on one environment.

A consistent read does not make separate node writes atomic. A snapshot can expose an intermediate publication state between two node writes.

Native I/O latency remains a separate assumption. Operation bounds and lock timeouts do not prove an operating-system scheduling deadline.

The capture covers a complete held DAG within explicit limits. A larger DAG is unsupported until a separately specified bounded-window contract exists.

The observer-only block decoder does not change the production decoder. Observer limits are not block-validity rules.

The DAG storage file retains its existing `CLAIM-FINALITY-002` carrier obligations. This claim adds a read-only capture boundary and changes no production write path.

## Local implementation evidence

The [Batch B1 report](../cbc-evidence/runs/casper-node-snapshot-batch-b1-799e2136a-01/report.json) identifies the source bytes and local checks.

The shared capture suite passed 11 tests. The block-storage capture suite passed 14 tests. Existing shared, DAG, carrier, block-store, and loom regressions passed.

Both suites use LMDB environments on the local filesystem. No running node, live adapter, campaign image, or consensus result was qualified.

Unit and integration tests supply evidence but do not discharge this claim. Source-bound verification and explicit acceptance remain pending.

## Construction and binding evidence

The TASK-019-4 handoff cycle on 2026-09-23 added the `BoundedCapture` Rocq module. Its theorems cover interval consistency over monotone transaction clocks, generation stability, charge budgets, overflow, length prefixes, guard order, and detachment.

The formal gate requires 14 closed assumption sets for the `NodeObservation.MainTheorem` module. Properties 1 through 8 and 10 have recorded theorems. Properties 9, 11, and 12 have none, and property 13 has a proposed bounded-by-design classification.

The capture oracle test runs production capture against a hand-translated `BoundedCapture` oracle over 14 scenarios. Eight Kani harnesses cover the length prefix, limit comparison, checked totals, atomic charging, and decode-limit validation.

Property 8 keeps a recorded binding gap. No deterministic generation-rejection test exists on this branch.

The [applicability review](../../formal/tlaplus/node_observation/README.md#applicability-per-property) lists every property's class, evidence, and decision status. Named maintainer review of each decision and acceptance of this claim remain pending.

## Verification requirements

The user confirmed model reconciliation on the node branch. The expanded inventory registers the bounded capture model and its ten negative controls.

These controls test selected abstract transitions. They do not supply construction proofs, complete Rust correspondence, or named maintainer acceptance.

The node package records gate registration evidence separately from the harness claim record. A node claim edit does not renew the harness claim.

Retain failing controls for invalid limits, unsupported backends, raw length, malformed prefixes, record and operation budgets, environment changes, restored values, incomplete rows, oversized block lengths, and scratch independence.

Verify that production store bytes are unchanged after successful and rejected captures. Verify that source and dependency inventories contain no unrelated changes.

This work does not change the harness claims, approve a campaign, publish images, or merge a pull request.
