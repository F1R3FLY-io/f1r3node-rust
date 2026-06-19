# 11 · Tracker race models

## 1 · Family motivation

This family models the lock-free vs. locked tracker access (Bug #2).
Sage's `Permutations` library lets the model enumerate every
two- and three-thread schedule of read-modify-write operations and
classify each schedule as lost-update or atomic. The witness is then
the canonical input to the TLA⁺ `ConcurrentTracker.tla` model and
the Loom `loom_t_9_2_*.rs` tests.

## 2 · The model

| Model                                                                                    | Searches                                                                       |
|------------------------------------------------------------------------------------------|--------------------------------------------------------------------------------|
| [`tracker_race_model.sage`](../../../../../formal/sage/slashing/tracker_race_model.sage) | Finite tracker schedules; minimal lost-update witness; atomic-RMW preservation |

## 3 · Representative witness

```json
{
  "kind": "tracker_race_witness",
  "threads": 2,
  "operations_per_thread": 1,
  "operation_kind": "insert_hash",
  "validator": "v0",
  "thread_hashes": {"t_A": "0xAAAA", "t_B": "0xBBBB"},
  "schedule_kind": "lock_free",
  "schedule": ["t_A.read", "t_B.read", "t_A.write", "t_B.write"],
  "final_tracker_state": {"v0": ["0xBBBB"]},
  "expected_atomic_state": {"v0": ["0xAAAA", "0xBBBB"]},
  "lost_hashes": ["0xAAAA"],
  "is_lost_update": true,
  "fixed_by_bug": "Bug #2"
}
```

Reading: the canonical 2-thread lost-update schedule. Both threads
read the empty tracker, both write — the second write overwrites
the first. The result has only `0xBBBB`; `0xAAAA` is lost. The
post-fix atomic-RMW schedule would have `{0xAAAA, 0xBBBB}`.

## 4 · Promotion targets

| Witness shape                | Rocq theorem                       | TLA⁺ model                                                         | Rust regression                                                                         |
|------------------------------|------------------------------------|--------------------------------------------------------------------|-----------------------------------------------------------------------------------------|
| Lost-update under lock-free  | T-9.2 (atomic record bisimilarity) | `ConcurrentTracker.tla` `Inv_NoOverwrite` (`Locked = FALSE` fails) | `loom_t_9_2_atomic_record.rs`, `loom_t_9_2_n_threads_3.rs`, `loom_t_9_2_n_threads_4.rs` |
| Atomic-RMW preservation      | (same theorem; post-fix branch)    | `ConcurrentTracker.tla` `Inv_NoOverwrite` (`Locked = TRUE` passes) | (same Loom tests pass)                                                                  |
| EquivocationsAccess contract | (informal; design §05)             | (subsumed)                                                         | `block-storage/src/rust/dag/equivocations_access.rs` integration tests                  |

## 5 · Related findings

In [`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md):

- (No standalone finding number; the tracker-race witness is the
  motivating example for Bug #2 and is referenced throughout the
  bisimilarity discussion.)

## 6 · Methodology note

This model is the canonical example of **cross-tool corroboration**:
the same race appears in Sage (combinatorial enumeration), in TLA⁺
(symbolic model checking under the `Locked` toggle), and in Loom
(actual Rust under the C11 memory model). Three independent
analyses, the same answer. See
[`../pipeline/03-evidence-stacking.md`](../pipeline/03-evidence-stacking.md)
for the evidence-stacking interpretation.
