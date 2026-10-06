# History checkpoint commit

**Status.** Decided by the user on 2026-10-06 in an architecture review. Maintainer approval is pending. Tracker: EPIC-021, TASK-021-10.

**Scope.** `rspace++/src/rspace/history` at `fix/issue-24-root-cause-fix` (`59d7c4d23`, PR #620), for the issue #24 sustained-phase finalization tail.

## 1. Context

Every checkpoint makes the new state durable with two LMDB write transactions on one LMDB environment. The databases `rspace-history` and `rspace-roots` share the environment `rspace/history` (`casper/src/rust/storage/rnode_key_value_store_manager.rs`). LMDB allows one writer for each environment.

1. `RadixHistory::process` calls `RadixTreeImpl::commit`, which writes the new radix nodes. This is the first transaction and the first fsync.
2. `do_checkpoint` then takes the roots mutex and calls `RootRepository::commit`, which writes the root tag and `current-root`. This is the second transaction and the second fsync.

The roots mutex is one `Arc<Mutex<RootRepository>>` that every repository instance shares, so it serializes the root commit of every runtime. Soak 37343966570 measured 5 to 25 ms of roots-mutex hold time for each checkpoint root commit.

## 2. Decision 1: one transaction for nodes and root (fix F2)

Fix F1 moves the root commit out of the roots mutex. The commit then still needs the LMDB writer lock of `rspace/history`, which the node write also needs, so F1 only moves the wait. Fix F2 writes the nodes and the root in one transaction. It removes one fsync and one writer-lock acquisition from each checkpoint. It also takes the root commit out of the roots mutex.

The change has two steps.

**Step 1: a checkpoint writer for the most common caller.**

- `History::stage(actions)` returns the new history and the pending node writes. It does not write to the store. `process` stays as a default method that calls `stage` and then writes, so the existing callers and tests keep their behavior.
- `RadixTreeImpl::take_pending_writes` drains `cache_w`, runs the collision check, and returns the absent pairs. `commit` calls it and then writes.
- A new interface `CheckpointWriter::write(nodes, root)` has the production adapter `KvCheckpointWriter`. That adapter writes the nodes and the root record through `batched_put`, in one transaction when the stores share an environment.
- `roots_store::root_record_kvs(root)` is the one encoding of the root record.
- `do_checkpoint` stages under the current-history mutex, releases the mutex, and then calls the writer. The checkpoint no longer takes the roots mutex.
- No trait signature changes for callers in `casper` or `rspace`.

**Step 2: one checkpoint commit module.**

- `CheckpointCommit { open, commit, contains_root }` replaces `RootRepository`. `open` contains the empty-root bootstrap.
- The global roots mutex is deleted. LMDB serializes the writers.
- The collision check moves into the write transaction (`batched_write` with an insert-or-match mode), so no window remains between the check and the write.

**Rejected alternative.** A flexible `StateCommit` interface with sync policies (group commit) and LFS import coverage. Three of its variants have no caller. Group commit would let finalized-looking state disappear after a crash.

## 3. Decision 2: no immutable-History refactor for issue #24 (C2 rejected)

**Proposal.** Make a `History` value read-only with a per-call write buffer, so that readers do not wait for a checkpoint fsync behind the current-history mutex.

**Reason for rejection.** In production, readers and checkpoints do not share a current-history mutex.

- `RuntimeManager.history_repo` is set once (`casper/src/rust/util/rholang/runtime_manager.rs`). The block index, the deploy-chain index, the DAG merger, and `spawn` read it. Nothing checkpoints it.
- Play and replay checkpoint the repository of a spawned space. `spawn` creates that repository with `reset()`, which makes a new mutex. The DAG merger checkpoints a repository that `reset` also created.
- So no production path holds the shared mutex across `process` or its fsync. Readers wait only for other readers, for microseconds.

**Consequence.** A future review must not propose this refactor as an issue #24 fix. The part that the checkpoint commit needs, `stage` instead of `commit`, is in Decision 1. A smaller change can split the read access from the `process` serialization. Do it only if the reader-site and reset-site wait counters of TASK-021-8 show measurable waits.

**Open question.** A design review reported a possible defect. A node that `process` saves can leave `cache_r` before the commit. A reload of that node then fails with "Missing node". This is not verified. Step 1 must check it with a test that bounds `cache_r` to one entry during a large `process`.

## 4. Decision 3: test the write shape at the interface (C3 folded into Decision 1)

- A real-LMDB test reads `env.info().last_txn_id` of `rspace/history` before and after one checkpoint and asserts a difference of exactly 1. The current code gives 2.
- An in-memory test uses a recording `CheckpointWriter`. It asserts one write for each non-empty checkpoint and no write for an empty checkpoint. It also asserts that the written root equals the new history root.
- A one-thread test asserts that no write occurs while the roots mutex is held, with `try_lock` on the mutex inside the recording writer. It uses no timing. Step 2 deletes this test with the mutex.
- A failing writer leaves the history store and `current-root` unchanged.
- The ignored timing probe `roots_lock_contention_probe` and its helpers are deleted. The two LMDB shape tests stay, and the `record_root` test uses the `last_txn_id` difference.
- A counting wrapper around `KeyValueStore` is not used, because `batched_put` detects LMDB stores with a downcast and a wrapper forces its fallback.

## 5. Verification

The unit and LMDB tests of Decision 3 run in every PR. A soak of the change must show a lower checkpoint root commit time. Its sustained finalization p95 must be unchanged or lower. The comparison uses the controls of TASK-021-8. Issue #24 closes only when the failure rate and the finalization p95 meet the decision rules of TASK-021-8.

## 6. Related

- Discovery file (local, not tracked): `docs/discoveries/architecture-review-2026-10-06T114619Z.md`
- [Glossary: Finalization latency p95](../../Glossary.md#finalization-latency-p95)
- `docs/ToDos.md`, EPIC-021: TASK-021-5, TASK-021-8, TASK-021-10
