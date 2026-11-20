# RSpace History GC — bounded safety model

Companion issue: [#1098](https://github.com/F1R3FLY-io/f1r3node-rust/issues/1098).
Predecessor issue (type-1/type-2 content-addressed races): [#1061](https://github.com/F1R3FLY-io/f1r3node-rust/issues/1061).

Mechanizes the "no live state references a deleted node" invariant for the
batched mark-and-sweep garbage collector in
`rspace++/src/rspace/history/history_repository_impl.rs::atomic_prune_via_store_batched`,
coordinated with writers via `prune_guard.rs`. TLC explores every
interleaving of one writer's `do_checkpoint` (stage → pin → write) with the
sweep's phases (begin_pass → quiesce-snapshot → mark → batch* → finish),
including the specific interleavings that produced the pre-fix defects.

## Model ↔ code

| Model operator / variable | Rust code |
|---|---|
| `WriterStage` | `HistoryRepositoryImpl::do_checkpoint` — the `stage` + `pending_writes` call, captures `writerSnapshot` (the store contents the content-addressed `pending_writes` will skip against) |
| `WriterPin` | `PruneGuard::pin` from `do_checkpoint` |
| `WriterCommit` | `batched_put` inside `checkpoint_writer` — writes `subtree \ writerSnapshot` atomically (content-addressed skipping) |
| `SweepBeginPass` | `PruneGuard::begin_pass` |
| `SweepQuiesceAndSnapshot` | Initial exclusive-held pin snapshot in `prune_history_atomic` (the brief quiesce that forces in-flight writers to commit before we read pins) |
| `SweepMarkFromSeed` | Lock-free initial DFS in `atomic_prune_via_store_batched` |
| `SweepBatch` | One iteration of the per-page loop: scan page, acquire exclusive, re-snapshot pins, walk new pins, delete |
| `pinned` (per-hash generation) | `PruneGuard::Pinned.generations` |
| Knobs `IncludeMutex`, `IncludePin`, `IncludeBatchRemark` | The three protections that together close the three defect classes. Each pre-fix config unsets one. |

## Configurations

| Config | Knobs | Expected | Shows |
|---|---|---|---|
| `MC_HistoryGC` | all `TRUE` | **clean** (160 distinct states) | Post-fix. Every interleaving of writer and batched sweep preserves the committed writer's subtree and keeps every live-root subtree intact. |
| `MC_HistoryGC_no_mutex_pre_1061` | `IncludeMutex = FALSE` | `Inv_Safety` violated | #1061 race type 1 (content-addressed): sweep deletes a hash the writer's `pending_writes` just skipped; the writer's commit then writes only `subtree \ snapshot`, which omits the deleted hash, so the committed state references a missing node. |
| `MC_HistoryGC_no_pin_pre_1061` | `IncludePin = FALSE` | `Inv_Safety` violated | #1061 race type 2 (not-yet-in-DAG root): the writer commits but never pins, the DAG retention set doesn't name the new root yet, and the sweep's mark walk misses it. Delete sweeps the writer's new subtree. |
| `MC_HistoryGC_no_remark_pre_1098` | `IncludeBatchRemark = FALSE` | `Inv_Safety` violated | #1098 regression: the sweep's batched algorithm doesn't re-walk new pins between delete batches. A writer that commits between the initial snapshot and a later batch pinned its new root, but the batch's scan sees the writer's nodes without the corresponding mark — the delete list includes the writer-committed subtree. This is the specific regression the `IncludeBatchRemark` branch in `SweepBatch` exists to prevent. |

The pre-fix configs are the formal-side counterexamples; each is a
reproduction of one defect class, not a spec bug.

## Running locally

```bash
cd formal/tlaplus/history_gc
java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_HistoryGC.cfg MC_HistoryGC.tla

java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_HistoryGC_no_mutex_pre_1061.cfg MC_HistoryGC_no_mutex_pre_1061.tla

java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_HistoryGC_no_pin_pre_1061.cfg MC_HistoryGC_no_pin_pre_1061.tla

java -jar ~/.tla/tla2tools.jar -workers auto \
  -config MC_HistoryGC_no_remark_pre_1098.cfg MC_HistoryGC_no_remark_pre_1098.tla
```

The post-fix config completes in under a second (~160 reachable states
over the depth-2 subtree universe); each pre-fix config halts with a
`Inv_Safety` counterexample within the first dozen states.

## Scope and limitations

What the model captures:

1. **Mutual exclusion between writers and the sweep's exclusive-lock
   windows.** TLC explores every order of `AcquireShared` / `AcquireExclusive`
   that the `PruneGuard` RwLock permits.
2. **Content-addressed skip semantics.** `WriterCommit` writes
   `subtree \ writerSnapshot` rather than the full subtree, so a deletion
   of a skipped-by-stage hash produces a dangling pointer at commit.
3. **The per-batch LMDB scan cursor.** The scan picks an arbitrary non-empty
   subset of `store \ sweepScanned`, so TLC explores every page ordering
   the real `scan_page` could emit — including the orderings in which a
   writer-committed-after-scan-start node appears in a later batch's page.
4. **Pin timing across phases.** Pins are visible to re-snapshotting even
   when they were set after the initial snapshot; the invariant covers both
   "pin visible, write landed" and "pin visible, write pending" (the latter
   is why the mutex-based quiesce in the fix matters).

What the model deliberately omits (covered by the Rust test suite instead):

- Age-based pin eviction and the `PIN_RETENTION_PASSES` constant
  — covered by `prune_guard::tests::begin_pass_evicts_pins_older_than_retention`
  and the end-to-end test
  `prune_history_atomic::pin_is_evicted_after_retention_window_and_its_subtree_is_swept`.
- The LRU cap on `PINNED_ROOTS_CAP` — covered by the dedicated unit tests
  in `prune_guard.rs`.
- Multi-pass behaviour across `begin_pass` ticks — single-pass is enough
  to exhibit the three defect classes.
- Observability counters (`pins_initial`, `batches`, etc.) — purely
  diagnostic; not a safety property.

## Implementation obligations

What the bounded model gates for the Rust side (following the project's
"spec must expose defects" rule):

1. **The sweep lock pairing stays in `do_checkpoint` and
   `atomic_prune_via_store_batched`.** Removing either side of the
   `enter_write` / `enter_sweep` dance reproduces the `no_mutex` counterexample.
2. **Writer pins its new root before the DAG catches up.** Dropping
   `self.prune_guard.pin(new_root.clone())` from `do_checkpoint`
   reproduces the `no_pin` counterexample.
3. **The per-batch re-snapshot and DFS on new pins stays in the batched
   sweep.** Dropping the `walk_and_mark(&new_pins, ...)` call inside the
   batch loop reproduces the `no_remark` counterexample.
