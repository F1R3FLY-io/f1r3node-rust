# Case study #2 — Lock-free tracker access (Rust regression)

## 1 · Summary

The Rust port replaced the Scala atomic
`accessEquivocationsTracker { … }` with a lock-free
read-modify-write pattern. Under concurrent insertion of equivocation
records for the same `(validator, base_seq_num)`, two threads could
both observe the empty record state, both compute `Set::new()` ∪
`{their hash}`, and both write — the second write overwriting the
first. Post-fix, the access wraps the read-modify-write window in an
atomic `access_equivocations_tracker { … }` semaphore.

## 2 · Discovery technique

**Primary**: TLA⁺ `ConcurrentTracker.tla` with `Locked ∈ BOOLEAN`
toggle. With `Locked = FALSE`, TLC immediately produces the 4-step
trace that violates `Inv_NoOverwrite`. With `Locked = TRUE`, every
invariant passes.

**Corroborating**:

- **Loom** tests
  [`casper/tests/slashing/loom_t_9_2_atomic_record.rs`](../../../../../casper/tests/slashing/loom_t_9_2_atomic_record.rs)
  reproduce the same race in actual Rust under the C11 memory
  model; pre-fix the test fails on a specific schedule, post-fix
  all schedules pass.
- **Sage** `tracker_race_model.sage` enumerates 2-, 3-, and 4-thread
  schedules combinatorially and emits the canonical lost-update
  witness.

Three independent analyses converge on the same 4-step trace; this
is the canonical example of methodology **evidence stacking**
(see [`../pipeline/03-evidence-stacking.md`](../pipeline/03-evidence-stacking.md)).

## 3 · Witness reproduction

Deterministic reproduction (single-threaded harness emulating the
race):

```
cargo test -p casper --test mod -- slashing::pre_fix_bug_2
```

Loom reproduction (under C11 memory model):

```
RUSTFLAGS="--cfg loom" cargo test --release loom_t_9_2_atomic_record
```

TLA⁺ reproduction (TLC):

```
tlc -workers 12 MC_ConcurrentTracker.tla       # passes (Locked=TRUE default)
tlc -workers 12 MC_ConcurrentTracker_pre_fix.tla  # fails Inv_NoOverwrite
```

## 4 · Classification trace

```
threat_class       = permitted_bug_fix (Rust regression vs. Scala atomic)
ledger_status      = confirmed_fixed_bug
action             = Keep pre_fix_bug_2.rs + loom_t_9_2_*.rs
                     regressions; TLA+ MC_ConcurrentTracker_pre_fix
                     remains as the pre-fix evidence file
```

## 5 · Evidence stack

| Layer            | Artifact                                                                                                                                                                               |
|------------------|----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Rocq theorem     | `t_9_2_atomic_no_overwrite` (`BugFixAtomicTracker.v:43`), n-thread `t_9_2_atomic_n_threads_arbitrary` (line 130)                                                                       |
| TLA⁺ invariant   | `Inv_NoOverwrite` (`ConcurrentTracker.tla`); both `Locked = TRUE` (pass) and `Locked = FALSE` (fail) runs are recorded                                                                 |
| Loom test        | `loom_t_9_2_atomic_record.rs`, `loom_t_9_2_n_threads_3.rs`, `loom_t_9_2_n_threads_4.rs`                                                                                                |
| Sage witness     | `tracker_race_model.sage` output JSON                                                                                                                                                  |
| Rust regression  | `pre_fix_bug_2.rs`                                                                                                                                                                     |
| Storage contract | [`block-storage/src/rust/dag/equivocations_access.rs`](../../../../../block-storage/src/rust/dag/equivocations_access.rs) defines `EquivocationsAccess` trait with atomic-RMW contract |
| Bug-fix manifest | [`../../design/09-bug-fixes-and-rationale.md §9.3`](../../design/09-bug-fixes-and-rationale.md)                                                                                        |
| Diagram          | [Diagram 09 — Tracker race & locking fix](../../diagrams/09-seq-tracker-race-and-fix.svg)                                                                                              |

**Stack depth: 5** (Rocq + TLA⁺ + Loom + Sage + Rust regression).

## 6 · Lessons for the methodology

1. **Lock-free is a *design constraint*, not a *correctness
   assumption***. The Rust port took an unsound shortcut by
   replacing the atomic with a read-then-write pair; the bug was
   inevitable under the C11 memory model. The methodology requires
   every concurrent code path to have a Loom test *before* the lock-
   free claim is admitted.
2. **The TLA⁺ `Locked ∈ BOOLEAN` toggle is the canonical *“model
   the bug, not just the fix”* pattern**. A model that only ever
   passes is weak evidence; a model that *also fails on the unfixed
   version* corroborates that it has contact with the real defect.
3. **Three independent confirmations are the gold standard**. Sage
   (combinatorial), TLA⁺ (symbolic), Loom (operational) — three
   different tools, the same 4-step trace. The probability of three
   tools sharing the same bug is negligible.
