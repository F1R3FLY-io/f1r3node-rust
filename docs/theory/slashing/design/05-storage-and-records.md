# 05 · Storage & Records

## 5.1 Two indices, two questions

The slashing subsystem persists evidence in two places that answer
two different questions:

| Index                      | Question it answers                                  | Sub-component  |
|----------------------------|------------------------------------------------------|----------------|
| `BlockDagStorage`          | *Is this block hash known to be invalid?*            | §3.2.2 storage |
| `EquivocationTrackerStore` | *Has this validator equivocated at this seq number?* | §3.2.2 storage |

The two are **redundant by design**: a proposer reading invalid
latest messages from the DAG and a proposer reading equivocation
records from the tracker should converge on the same offender set
(modulo bug fix #3, which makes the *non-equivocation* slashable
variants reach the tracker too).

## 5.2 The DAG store — `BlockDagStorage`

### 5.2.1 Two operations relevant to slashing

| Operation                            | Effect                                                              |
|--------------------------------------|---------------------------------------------------------------------|
| `insert(block, invalid: Bool)`       | Adds a block to the DAG; sets its invalid flag.                     |
| `access_equivocations_tracker { f }` | Invokes `f` on the tracker store under a global semaphore (atomic). |

The semaphore lives at `BlockDagKeyValueStorage.scala:262`
(`accessEquivocationsTracker { lock.withPermit(f(...)) }`,
allocated via `MetricsSemaphore.single[F]` at line 350). The
`EquivocationTrackerStore` itself has *no* concurrency primitives.

### 5.2.2 The lock-free entry points (Rust regression)

The Rust port additionally exposes lock-free direct methods:

```rust
pub fn equivocation_records(&self) -> Vec<EquivocationRecord>
pub fn insert_equivocation_record(&self, v: Validator, base: u64, hashes: Set<Hash>)
pub fn update_equivocation_record(&self, v: Validator, base: u64, h: Hash)
```

These bypass the lock and are the source of bug #2 (§09 / Diagram 09).
The pre-fix `MultiParentCasperImpl.handle_invalid_block` calls these
directly; the post-fix re-routes them through the existing
`access_equivocations_tracker` wrapper to restore atomicity.

## 5.3 The tracker store — `EquivocationTrackerStore`

### 5.3.1 Shape

```
type EquivocationTrackerStore = KV-store of
    (Validator × SeqNum) → Set[BlockHash]
```

The Scala uses `Set[BlockHash]` (unordered hash-set); the Rust uses
`BTreeSet<BlockHash>` (ordered). Iteration order differs but
element membership is identical, hence bisimilar at the value level.

### 5.3.2 Two operations: `insert` and `update`

```
function insert_equivocation_record(v: Validator, base: SeqNum, witnesses: Set[Hash]):
    if (v, base) ∉ store:
        store[(v, base)] := witnesses

function update_equivocation_record(v: Validator, base: SeqNum, h: Hash):
    let current ← store.get((v, base))
    store[(v, base)] := current ∪ {h}
```

The `insert` operation is **idempotent** when called with the same
empty set (just adds an empty record if absent). The `update`
operation is **monotone** under set union: it only ever *adds*
hashes, never removes (theorem T-4, `EquivocationRecord.v`
`record_monotone_update`).

### 5.3.3 Why two operations?

The two-operation API exists because the dispatcher inserts the
*record key* eagerly (when the equivocation is first detected) and
adds the *witness hashes* lazily (when they are surfaced by later
verification passes). This separation lets the dispatcher commit
the record key under the lock without needing to enumerate witnesses
upfront.

## 5.4 The race condition (bug #2 / T-9.2)

[![Diagram 09 — Tracker race & locking fix](../diagrams/09-seq-tracker-race-and-fix.svg)](../diagrams/09-seq-tracker-race-and-fix.svg)

### 5.4.1 The pre-fix race trace

Two threads `T1` and `T2` independently process two distinct
equivocating blocks `b₁` and `b₂` by validator A at the same
sequence number. Both reach the
`update_equivocation_record(A, sn − 1, ...)` path concurrently.

> **Phase a — `handle_invalid_block` (idempotent ∅-insert).**
> Both threads insert an empty record at `(A, sn − 1)`. This is
> NOT the source of the race — both threads write the same value.
>
> **Phase b — `update_equivocation_record` (lossy RMW).**
> Both threads read the same pre-image (∅) before either writes,
> compute `newSet := ∅ ∪ {b_i.hash}` = `{b_i.hash}`, and put back
> a single-element set. The second put **overwrites** the first;
> one of `b₁.hash` and `b₂.hash` is **lost**.

### 5.4.2 The post-fix locked trace

The post-fix re-routes both threads through
`access_equivocations_tracker { closure }`, which acquires a global
semaphore before running the closure. The closure runs as one
atomic step:

```
T1 → access_equivocations_tracker { tracker →
       let view ← tracker.equivocation_records()
       tracker.update_equivocation_record(A, sn-1, b₁.hash)
     }   -- lock RELEASED

T2 → access_equivocations_tracker { tracker →
       let view ← tracker.equivocation_records()      -- now sees {b₁.hash}
       tracker.update_equivocation_record(A, sn-1, b₂.hash)  -- {b₁.hash, b₂.hash}
     }
```

Now the final state is `{b₁.hash, b₂.hash}` — both witnesses
preserved.

### 5.4.3 Theorem T-9.2 — Atomic no-overwrite

**Statement.** *(`t_9_2_atomic_no_overwrite`,
`BugFixAtomicTracker.v:43`; n-thread `t_9_2_atomic_n_threads_arbitrary`
at line 130.)*

```
∀ s k h, incl(hashes_at_key(s, k),
              hashes_at_key(atomic_record_or_update(s, k, h), k))

-- lifted to schedules:
∀ ops s k, incl(hashes_at_key(s, k),
                hashes_at_key(apply_schedule(s, ops), k))
```

In English: under the lock, T-4 (record monotonicity) holds for
arbitrary thread schedules. Every hash present before the operation
is also present after. The proof is by case analysis on `has_key`,
composing `t_4_record_monotone_update` and
`t_4_record_monotone_insert_cond`.

### 5.4.4 TLA+ corroboration

The model `MC_ConcurrentTracker.tla` checks the invariant
`Inv_NoOverwrite` over interleaved schedules. With `Locked = FALSE`
(pre-fix), the invariant violates trivially — TLC produces a
counter-example schedule of length 4 in under one second. With
`Locked = TRUE` (post-fix), the invariant is exhausted across all
thread interleavings of length ≤ 8 in under a minute. Both runs are
reported in verification §10.

## 5.5 Why a tracker store, not a flat append-only log?

| Design alternative                             | Tradeoff                                                                                                 |
|------------------------------------------------|----------------------------------------------------------------------------------------------------------|
| Per-`(V, baseSeq)` keyed store (chosen)        | O(1) lookup for "has this validator equivocated at this seq?"; record monotonicity is set-union per key. |
| Flat append-only log of equivocation events    | Trivial to write; O(n) lookup per event; harder to enforce monotonicity per-key.                         |
| Per-validator ordered map of (seq → witnesses) | Adds a layer; equivalent expressivity; more complex serialization.                                       |
| In-memory only (no persistence)                | Loses evidence across node restarts; *no longer audit-grade*.                                            |

The keyed store is the simplest design that supports O(1) lookup,
per-key monotonicity, and persistence — the three properties the
slash pipeline depends on.

## 5.6 Persistence layout

Both indices use the same low-level KV-store substrate
(`KeyValueTypedStore`). Serialization is via prost-protobuf.

```
BlockDagStorage:
  block-hash → SerializedBlockMetadata { invalid: bool, parents: ..., justifications: ... }

EquivocationTrackerStore:
  (validator-bytes, seq-num-le-u64) → SerializedSet<BlockHash>
```

The on-disk format is byte-equivalent between Rust and Scala
**modulo iteration order** (the spec defines bisimilarity at the value level,
not byte-level — see §10).

## 5.7 What this layer does *not* do

- **No deletion.** Once an `EquivocationRecord` is written, it is
  never removed. (Bug fix #2 preserves this; bug fix #3 extends the
  invariant to non-equivocation slashable variants.)
- **No GC.** The tracker store grows with the number of distinct
  `(validator, baseSeq)` keys that have ever equivocated. In
  practice this is bounded by the number of unique
  `(validator, seqNum)` pairs in the active validator set's
  history.
- **No notification.** The tracker store is *read* by the proposer;
  there is no event queue or observer. This is consistent with the
  rest of the architecture's *pull, not push* convention (cf. the
  fork-choice layer in §07).

---

**Next:** [§06 — Proposing & effect](06-proposing-and-effect.md)
