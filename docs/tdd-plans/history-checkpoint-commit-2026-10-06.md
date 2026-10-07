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
    done: true
    cycle_log:
      - date: 2026-10-06
        tracer: true
        test: "history::roots_lock_contention_tests::checkpoint_makes_the_new_state_durable_in_one_history_environment_transaction"
        red: "left: 2, right: 1. The current code makes two write transactions on rspace/history for each checkpoint."
        green: "History::stage returns the new radix nodes without a write. RadixTreeImpl::pending_writes keeps the collision check, and commit uses it. CheckpointWriter with KvCheckpointWriter writes the nodes and roots_store::root_record_kvs through batched_put. do_checkpoint stages under the current-history mutex and writes after it releases the mutex. The checkpoint no longer takes the roots mutex."
        files:
          - rspace++/src/rspace/history/checkpoint_writer.rs
          - rspace++/src/rspace/history/history.rs
          - rspace++/src/rspace/history/instances/radix_history.rs
          - rspace++/src/rspace/history/radix_tree.rs
          - rspace++/src/rspace/history/roots_store.rs
          - rspace++/src/rspace/history/history_repository_impl.rs
          - rspace++/src/rspace/history/history_repository.rs
          - rspace++/tests/history/roots_lock_contention_tests.rs
          - rspace++/tests/history/history_repository_tests.rs
        suite: "rspace++ integration tests 150 passed (4 ignored), library tests 113 passed, clippy -D warnings clean, cargo check --workspace --all-targets clean. The casper test suite did not run."
        observations:
          - "Two existing tests asserted the old shape and changed with the behavior. lock_site_metrics_count_each_call_site_separately now expects no roots-repository checkpoint site and 3 roots lock calls. checkpoint_attribution_preserves_roots_and_records_only_executed_stages no longer expects the roots-lock-wait stage."
          - "create_empty_repository now uses RootsStoreInstances over an in-memory key-value store instead of the custom InmemRootsStore, because the one-transaction writer needs the roots as a key-value store. InmemRootsStore and root_repository() were deleted."
          - "The root-commit histogram now times the combined node and root write."
        refactors_done_2026_10_06:
          - "Removed HISTORY_CHECKPOINT_ROOTS_LOCK_WAIT_TIME_METRIC and ROOTS_LOCK_CHECKPOINT_SITE, and their names in scripts/bench/extend-issue24-metrics.sh and its test."
          - "Deleted the ignored roots_lock_contention_probe and its helpers. The record_root shape test now asserts a last_txn_id difference of 1."
  - id: B2
    statement: "After a checkpoint, a repository reopened on the same store starts at the new root and reads the checkpointed data"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::roots_lock_contention_tests::repository_reopened_after_a_checkpoint_starts_at_the_new_root_and_reads_its_data"
        red: "Not RED. GREEN before any production change, as the conformance audit expected."
        mutation_check: "With KvCheckpointWriter changed to write no root record, the test failed: the reopened repository started at the empty root. The writer was restored."
        green: "No production change."
        files:
          - rspace++/tests/history/roots_lock_contention_tests.rs
        suite: "rspace++ integration tests 151 passed (4 ignored), clippy -D warnings clean."
  - id: B4
    statement: "A failed durable write leaves the store at the previous root, and no root is recorded without its nodes"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::history_repository_tests::failed_checkpoint_write_leaves_the_store_at_the_previous_root"
        red: "Not RED. GREEN after B1, because the checkpoint writes only through the CheckpointWriter."
        mutation_check: "With RadixHistory::stage changed to also write its nodes to the history store, the test failed at the history store comparison. stage was restored."
        green: "No production change. The test helper create_empty_repository now delegates to empty_repository_over(history, roots, writer), so a test can inject a writer at the declared boundary."
        files:
          - rspace++/tests/history/history_repository_tests.rs
        suite: "rspace++ integration tests 152 passed (4 ignored), clippy -D warnings clean."
        observations:
          - "HistoryRepository::checkpoint returns no Result, so a failed write panics through expect. The design keeps the caller signatures. A change to a Result return is a separate decision for step 2."
  - id: B5
    statement: "A checkpoint that meets a stored node with a different value fails with the collision error, and a stored node with an equal value is accepted"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::history_repository_tests::checkpoint_rejects_a_stored_node_with_a_different_value_and_accepts_an_equal_one"
        red: "Not RED. GREEN after B1, because RadixTreeImpl::pending_writes kept the collision check of commit."
        mutation_check: "With the collision check in pending_writes disabled, the conflicting checkpoint succeeded and the test failed. The check was restored."
        green: "No production change. A helper repository_over_in_memory_stores returns a repository and its history store."
        files:
          - rspace++/tests/history/history_repository_tests.rs
        suite: "rspace++ integration tests 153 passed (4 ignored), clippy -D warnings clean."
        observations:
          - "The test learns the root node key from a reference checkpoint and seeds that key into fresh stores. It never calls an internal method."
  - id: B3
    statement: "The durable write of a checkpoint never occurs while the roots mutex is held"
    priority: should
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::history_repository_tests::checkpoint_writes_its_state_while_the_roots_mutex_is_free"
        red: "Not RED. GREEN after B1, because do_checkpoint no longer takes the roots mutex."
        mutation_check: "With do_checkpoint holding the roots mutex around the write, the recorded flags were [true, true] and the test failed. do_checkpoint was restored."
        green: "No production change. A RecordingCheckpointWriter at the declared boundary checks the roots mutex with try_lock on one thread, without timing."
        files:
          - rspace++/tests/history/history_repository_tests.rs
        suite: "rspace++ integration tests 154 passed (4 ignored), clippy -D warnings clean."
        observations:
          - "Step 2 deletes the roots mutex. Delete this test with it, because it then passes for no reason."
  - id: B6
    statement: "A checkpoint with no actions writes nothing and keeps the current root"
    priority: should
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::history_repository_tests::empty_checkpoint_writes_nothing_and_keeps_the_current_root"
        red: "Not RED. GREEN before change, because checkpoint returns a no-op clone for an empty action list."
        mutation_check: "With the empty checkpoint also calling the writer, the test failed. checkpoint was restored."
        green: "No production change."
        refactor: "While GREEN, the B3 and B6 tests now share repository_with_recording_writer() instead of a duplicated setup."
        files:
          - rspace++/tests/history/history_repository_tests.rs
        suite: "rspace++ integration tests 155 passed (4 ignored), clippy -D warnings clean."
  - id: B7
    statement: "A large checkpoint completes without a missing-node error when the read cache holds only one entry"
    priority: should
    deep_module: false
    done: true
    cycle_log:
      - date: 2026-10-06
        regression_guard: true
        test: "history::roots_lock_contention_tests::checkpoint_larger_than_the_read_cache_completes_and_reads_back_its_data"
        red: "Not RED. One LMDB checkpoint of 20,000 inserts completes and reads back its data."
        cache_bound_check: "The read cache bound is a private constant, so the test cannot set it through the interface. With READ_CACHE_MAX_ITEMS set to 1 and READ_CACHE_TRIM_TARGET set to 0 for one run, this test and all 50 history tests passed. The constants were restored."
        result: "The eviction defect of the design record is not observed. make_actions keeps the nodes that it builds in memory and does not load a node saved in the same process call through cache_r. This is evidence for the tested workloads, not a proof."
        green: "No production change."
        files:
          - rspace++/tests/history/roots_lock_contention_tests.rs
        suite: "rspace++ integration tests 156 passed (4 ignored), clippy -D warnings clean."
  - id: B8
    statement: "Opening a repository on a new store records the empty root through the single checkpoint commit module (step 2)"
    priority: consider
    deep_module: true
    done: false
    cycle_log: []
---

# TDD Plan -- history checkpoint commit (TASK-021-10)

This plan implements step 1 of the one-transaction checkpoint commit. The change is contention reduction. It does not fix the issue #24 [finalization latency p95](../Glossary.md#finalization-latency-p95) tail (issue #24 comment 6016872961). Every behavior is tested through the `HistoryRepository` interface. The source is candidate C1 of the discovery file, and candidate C3 supplies the write-shape tests. The design record `docs/casper/design/history-checkpoint-commit.md` holds the reasons and the rejected alternatives.

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

- 2026-10-06, B1 (tracer bullet): GREEN. One checkpoint now makes one write transaction on `rspace/history`, down from two. See the B1 cycle log in the frontmatter.
- 2026-10-06, B2 (regression guard): GREEN before change. A mutation check that drops the root record makes it fail.
- 2026-10-06, B4 (regression guard): GREEN after B1. A mutation check that writes nodes outside the writer makes it fail.
- 2026-10-06, B5 (regression guard): GREEN after B1. A mutation check that disables the collision check makes it fail. All must behaviors are done.
- 2026-10-06, B3 (regression guard): GREEN after B1. A mutation check that holds the roots mutex during the write makes it fail.
- 2026-10-06, B6 (regression guard): GREEN before change. A mutation check that writes on an empty checkpoint makes it fail.
- 2026-10-06, B7: GREEN. The eviction defect is not observed, even with the read cache bounded to one entry for one run. Step 1 is complete. B8 belongs to step 2.
- 2026-10-06, cleanup: the unused roots-lock-wait metric, the checkpoint roots-lock site, and the ignored timing probe were deleted. The full soak showed that this change is contention reduction. The plan stops at step 1 until a soak of this branch is compared with a soak of dev.
