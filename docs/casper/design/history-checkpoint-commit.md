# History checkpoint commit

**Status.** Decided by the user on 2026-10-06 as contention reduction, not as a fix for issue #24. Maintainer approval is pending.

Step 1 is implemented and held for soak evidence in draft PR #653 (label `awaiting-soak-evidence`). Step 2 is deferred. Tracker: EPIC-021, TASK-021-10.

**Scope.** `rspace++/src/rspace/history` at `fix/issue-24-root-cause-fix` (`59d7c4d23`, PR #620). The change reduces lock and fsync contention in the history checkpoint.

## 1. Context

Every checkpoint makes the new state durable with two LMDB write transactions on one LMDB environment. The databases `rspace-history` and `rspace-roots` share the environment `rspace/history` (`casper/src/rust/storage/rnode_key_value_store_manager.rs`). LMDB allows one writer for each environment.

1. `RadixHistory::process` calls `RadixTreeImpl::commit`, which writes the new radix nodes. This is the first transaction and the first fsync.
2. `do_checkpoint` then takes the roots mutex and calls `RootRepository::commit`, which writes the root tag and `current-root`. This is the second transaction and the second fsync.

The roots mutex is one `Arc<Mutex<RootRepository>>` that every repository instance shares, so it serializes the root commit of every runtime. The full run of soak 37343966570 measured about 1.2 ms for each checkpoint root commit.

## 1a. Relation to issue #24

This change does not fix issue #24. The full-run analysis of soak 37343966570 is in [issue #24, comment 6016872961](https://github.com/F1R3FLY-io/f1r3node-rust/issues/24#issuecomment-6016872961).

- AUC is the area under the receiver operating characteristic curve. Here it is the probability that a failing iteration has the higher value. A value of 0.5 means that a measure does not separate the two groups.
- The checkpoint root commit has an AUC of 0.60, and the roots lock wait has an AUC of 0.57.
- No per-node stage separates failing iterations from passing ones. The finalization tail is in the finality rounds and in blocks that wait for missing parents.

An earlier version of this record used a lead from the first 7 sessions of that soak: 5 to 25 ms for each root commit. The full run does not support that lead.

## 2. Decision 1: one transaction for nodes and root (contention reduction)

Fix F1 moves the root commit out of the roots mutex. The commit then still needs the LMDB writer lock of `rspace/history`, which the node write also needs, so F1 only moves the wait. Fix F2 writes the nodes and the root in one transaction. It removes one fsync and one writer-lock acquisition from each checkpoint. It also takes the root commit out of the roots mutex.

The change has two steps.

**Step 1: a checkpoint writer for the most common caller.**

- `History::stage(actions)` returns the new history and the pending node writes. It does not write to the store. `RadixHistory::process` calls `stage` and then writes the nodes, so the existing callers and tests keep their behavior.
- `RadixTreeImpl::pending_writes` runs the collision check on `cache_w` and returns the absent pairs. `commit` calls it and then writes.
- A new interface `CheckpointWriter::write(nodes, root)` has the production adapter `KvCheckpointWriter`. That adapter writes the nodes and the root record through `batched_put`, in one transaction when the stores share an environment.
- `roots_store::root_record_kvs(root)` is the one encoding of the root record.
- `do_checkpoint` stages under the current-history mutex, releases the mutex, and then calls the writer. The checkpoint no longer takes the roots mutex.
- No trait signature changes for callers in `casper` or `rspace`.
- The unused checkpoint roots-lock site, the unused roots-lock-wait histogram, and their names in `scripts/bench/extend-issue24-metrics.sh` are removed.

**Step 2: one checkpoint commit module (deferred).** Do step 2 only if the soak of step 1 shows a measurable gain.

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

**Open question.** A design review reported a possible defect. A node that `process` saves can leave `cache_r` before the commit. A reload of that node then fails with "Missing node".

**Result.** On 2026-10-06 the TDD cycle B7 checked it and did not observe it. With the read cache bound set to one entry for one run, a checkpoint of 20,000 inserts and all history tests passed. `make_actions` keeps the nodes that it builds in memory, so it does not load a node saved in the same call through `cache_r`. This result is evidence for the tested workloads, not a proof.

## 4. Decision 3: test the write shape at the interface (C3 folded into Decision 1)

- A real-LMDB test reads `env.info().last_txn_id` of `rspace/history` before and after one checkpoint and asserts a difference of exactly 1. The code before step 1 gave 2.
- An in-memory test uses a recording `CheckpointWriter`. It asserts one write for each non-empty checkpoint and no write for an empty checkpoint. It also asserts that the written root equals the new history root.
- A one-thread test asserts that no write occurs while the roots mutex is held, with `try_lock` on the mutex inside the recording writer. It uses no timing. Step 2 deletes this test with the mutex.
- A failing writer leaves the history store and `current-root` unchanged.
- The ignored timing probe `roots_lock_contention_probe` and its helpers are deleted. The two LMDB shape tests stay, and the `record_root` test uses the `last_txn_id` difference.
- A counting wrapper around `KeyValueStore` is not used, because `batched_put` detects LMDB stores with a downcast and a wrapper forces its fallback.

## 5. Verification

The unit and LMDB tests of Decision 3 run in every PR. The soak comparison uses a soak of `dev` after PR #622 and PR #620 merge, and a soak of this branch. The soak of this branch must show lower root commit and roots lock times, and a sustained finalization p95 that is not worse. PR #653 stays a draft until that evidence exists. This change does not decide issue #24.

## 6. Related

- Discovery file (local, not tracked): `docs/discoveries/architecture-review-2026-10-06T114619Z.md`
- [Glossary: Finalization latency p95](../../Glossary.md#finalization-latency-p95)
- `docs/ToDos.md`, EPIC-021: TASK-021-5, TASK-021-8, TASK-021-10
