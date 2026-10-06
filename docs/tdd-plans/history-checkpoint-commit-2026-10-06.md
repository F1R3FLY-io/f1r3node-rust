---
kind: tdd-plan
scope: rspace++/src/rspace/history (checkpoint commit, TASK-021-10)
produced_by: /tdd
produced_at: 2026-10-06T12:40:00Z
source_discovery: docs/discoveries/architecture-review-2026-10-06T114619Z.md
source_candidate: C1
design: docs/casper/design/history-checkpoint-commit.md
task: TASK-021-10
glossary: docs/Glossary.md
test_runner: "other:cargo test --release -p rspace_plus_plus --test mod history"
system_boundaries:
  - durable-store-writer
  - filesystem-tempdir-lmdb
  - in-memory-key-value-store
conformance_audit:
  status: warnings
  notes:
    - "B1 asserts the number of LMDB write transactions. A count is usually an implementation detail. Here the count is the performance characteristic of the interface that issue #24 is about, so the plan keeps it. It reads the count from LMDB (last_txn_id), not from a mock."
    - "B3 asserts that the roots mutex is free during the durable write. The mutex is internal to the repository. The test must use only the recording adapter at the durable-store-writer boundary and the repository interface. Step 2 deletes the mutex, and B3 is deleted with it."
    - "B2, B5, and B6 can already pass before the change, because they describe behavior that the current code has. Treat them as regression guards: write each test, run it on the current code, and record GREEN-before-change in the cycle log. Do not change production code to make them RED."
    - "The design record names internal methods (stage, take_pending_writes, root_record_kvs). No behavior names them, and no test may call them directly."
behaviors:
  - id: B1
    statement: "A checkpoint with actions makes the new state durable in one write transaction on the history environment"
    priority: must
    deep_module: true
    done: false
    cycle_log: []
  - id: B2
    statement: "After a checkpoint, a repository reopened on the same store starts at the new root and reads the checkpointed data"
    priority: must
    deep_module: false
    done: false
    cycle_log: []
  - id: B4
    statement: "A failed durable write leaves the store at the previous root, and no root is recorded without its nodes"
    priority: must
    deep_module: false
    done: false
    cycle_log: []
  - id: B5
    statement: "A checkpoint that meets a stored node with a different value fails with the collision error, and a stored node with an equal value is accepted"
    priority: must
    deep_module: false
    done: false
    cycle_log: []
  - id: B3
    statement: "The durable write of a checkpoint never occurs while the roots mutex is held"
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B6
    statement: "A checkpoint with no actions writes nothing and keeps the current root"
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B7
    statement: "A large checkpoint completes without a missing-node error when the read cache holds only one entry"
    priority: should
    deep_module: false
    done: false
    cycle_log: []
  - id: B8
    statement: "Opening a repository on a new store records the empty root through the single checkpoint commit module (step 2)"
    priority: consider
    deep_module: true
    done: false
    cycle_log: []
---

# TDD Plan -- history checkpoint commit (TASK-021-10)

This plan implements step 1 of fix F2 for the issue #24 [finalization latency p95](../Glossary.md#finalization-latency-p95) tail. Every behavior is tested through the `HistoryRepository` interface. The source is candidate C1 of the discovery file, and candidate C3 supplies the write-shape tests. The design record `docs/casper/design/history-checkpoint-commit.md` holds the reasons and the rejected alternatives.

## Public Interface

- **Name:** `HistoryRepository` (`rspace++/src/rspace/history/history_repository.rs`). The glossary has no entry for it yet. The discovery file proposes the terms checkpoint, history root, and root commit.
- **Signature surface:** `checkpoint`, `reset`, `root`, `contains_root`, `record_root`, and the existing LMDB and in-memory constructors.
- **Invariants:**
  - A recorded root always has all of its nodes in the history store.
  - `current-root` names the root of the last durable checkpoint.
  - Node keys are content hashes, so one key never has two values.
- **Error modes:** a node collision fails the checkpoint. A store error fails the checkpoint, and the store keeps the previous root.
- **Performance characteristics:** one write transaction and one fsync on the history environment for each checkpoint with actions. The durable write does not hold the roots mutex.

## System Boundaries (Mocking Allowed)

- **Durable store writer** (`CheckpointWriter`): a recording adapter or a failing adapter stands in for the durable write. It is used for B3, B4, and B6.
- **Filesystem, tempdir LMDB:** a real LMDB environment in a temporary directory. It is never mocked. It is used for B1, B2, and B5.
- **In-memory key-value store:** the existing in-memory `KeyValueStore` is the local substitute for LMDB. It is used for B5 and B7.

The radix tree, the history value, the roots store, and the root repository are internal collaborators. Tests do not mock them and do not call them directly.

## Order of cycles

1. B1 is the tracer bullet. It goes RED on the current code, because a checkpoint advances `last_txn_id` by 2. GREEN needs the `CheckpointWriter` path.
2. B2, B4, and B5 follow in that order. B2 and B5 are regression guards (see the conformance notes).
3. B3, B6, and B7 come next. B7 confirms or rejects the eviction defect in the design record, section 3.
4. B8 belongs to step 2 and waits for soak evidence of step 1.

## Cycle log

No cycles have run yet. Run `/tdd` to start the B1 tracer bullet, or `/loop /tdd` to walk the plan.
