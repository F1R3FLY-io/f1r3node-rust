# 04 · Concurrency interleaving with Loom

> *“If you have concurrency, you have a memory model. If you ignore
> the memory model, the memory model will not ignore you.”* —
> Hans Boehm, in conversation, paraphrased.

This chapter explains the role of Loom [Loom] in the slashing
methodology. Loom is a Rust testing framework that **exhaustively
explores the permitted thread interleavings** of a concurrent
program under the C11/C++11 memory model [BOSSW11]. Where TLA⁺
explores an abstract scheduler at protocol granularity, Loom
explores the *actual* Rust scheduler at atomic-operation
granularity.

Organization:

- [§1 — Why Loom is necessary](#1--why-loom-is-necessary)
- [§2 — Loom's algorithm in literate form](#2--looms-algorithm-in-literate-form)
- [§3 — The three slashing Loom tests](#3--the-three-slashing-loom-tests)
- [§4 — Coupling with TLA⁺ — the same race, two abstractions](#4--coupling-with-tla--the-same-race-two-abstractions)
- [§5 — Pitfalls](#5--pitfalls)
- [§6 — Related work](#6--related-work)

---

## 1 · Why Loom is necessary

A concurrent Rust program is correct **iff** every interleaving of
its threads, permitted by the memory model, produces a result the
program's contract allows. Standard testing (cargo test) executes
*one* arbitrary interleaving per run. The probability of hitting an
interleaving that exposes a particular race is generally **far below
the noise floor** of CI runs.

The slashing port encountered this directly with Bug #2 (the
lock-free tracker race; see
[`../../design/09-bug-fixes-and-rationale.md §9.3`](../../design/09-bug-fixes-and-rationale.md)).
Standard tests passed millions of times without exposing the lost
update. The race requires:

1. Two threads, `t_A` and `t_B`, both inserting into
   `tracker[v]` (validator `v`) at the same time.
2. `t_A.read` and `t_B.read` to observe the same value (the empty
   set, or the same prior insertions).
3. `t_A.write` and `t_B.write` to occur in either order; the second
   write overwrites the first.

In a 2-thread, 2-operation interleaving space of size `4! / 2 = 12`,
the racy interleaving is **one** specific ordering. With more
threads or more operations the space grows combinatorially, and
random sampling becomes hopeless. Loom **exhausts** the space.

### 1.1 What Loom replaces

Loom replaces the standard library's `std::sync` types with its own
instrumented versions. When you call `loom::sync::Arc<Mutex<T>>` in
a test, every load, store, and atomic operation is recorded by
Loom's scheduler. The scheduler then re-runs the test under each
permitted interleaving.

The standard library types and the Loom types are wire-compatible
(same surface API), so production code is unmodified — only the
*test* swaps `std::sync` for `loom::sync` via `cfg(loom)`:

```rust
#[cfg(loom)]
use loom::sync::{Arc, Mutex};
#[cfg(not(loom))]
use std::sync::{Arc, Mutex};
```

### 1.2 What Loom does *not* do

- Loom does not detect data races; it detects **semantic violations
  under any permitted interleaving**. (Use Miri or ThreadSanitizer
  for data-race detection per se.)
- Loom does not explore unbounded thread counts; the slashing tests
  use 3 and 4 threads (the smallest values that exhibit the race).
- Loom does not explore network or message-passing models; it works
  on shared-memory only. (TLA⁺ covers the message-passing model.)

---

## 2 · Loom's algorithm in literate form

The conceptual algorithm Loom executes is:

```
algorithm loom_explore(test_body : Fn() → Result):
    let executions  ← []
    let visited     ← ∅
    push initial_schedule onto executions
    while executions not empty:
        let schedule ← pop(executions)
        let outcome  ← run_test_with_schedule(test_body, schedule)
        if outcome = panic ∨ outcome = invariant_violation:
            return Failure(schedule, outcome)
        for each branch_point in observed_branch_points(schedule):
            let alternatives ← enumerate_alternatives(branch_point)
            for each alt in alternatives:
                let new_schedule ← extend(schedule, alt)
                if new_schedule ∉ visited:
                    push new_schedule onto executions
                    insert new_schedule into visited
    return Success(visited.size)
```

Loom prunes the search by **partial-order reduction**: two interleavings
that differ only in the order of independent operations (operations on
disjoint memory locations) are treated as equivalent. This is the
single most important reason Loom is tractable for tests with non-
trivial thread counts.

### 2.1 What counts as a "branch point"

Every:

- atomic load / store / RMW (read-modify-write)
- mutex acquire / release
- channel send / receive
- thread spawn / join

is a branch point. Loom enumerates the orderings the C11/C++11
memory model permits at each point and explores each.

### 2.2 The cost surface

| Threads × ops | Schedules (unreduced) | Schedules (reduced) | Loom wall time     |
|---------------|-----------------------|---------------------|--------------------|
| 2 × 2         | 24                    | ≈ 12                | < 1 s              |
| 3 × 3         | 362 880               | ≈ 1 000–10 000      | seconds            |
| 4 × 4         | ≈ 2 × 10¹³            | ≈ 10⁵–10⁶           | minutes            |
| 4 × 6         | ≈ 6 × 10²³            | ≈ 10⁸               | hours (CI-bounded) |

The slashing tests use 3- and 4-thread variants because Bug #2
manifests at any thread count `≥ 2`; higher thread counts increase
confidence but yield diminishing returns.

---

## 3 · The three slashing Loom tests

The Loom tests are in
[`casper/tests/slashing/loom_t_9_2_*.rs`](../../../../../casper/tests/slashing/).
There are three:

| File                          | Threads | What it exercises                                                          |
|-------------------------------|---------|----------------------------------------------------------------------------|
| `loom_t_9_2_atomic_record.rs` | 2       | Atomic record insertion under contention (the canonical Bug #2 reproducer) |
| `loom_t_9_2_n_threads_3.rs`   | 3       | 3-thread variant — confirms the fix scales to small thread counts          |
| `loom_t_9_2_n_threads_4.rs`   | 4       | 4-thread variant — confirms no new race emerges at higher contention       |

### 3.1 The canonical test in literate form

```
loom_test loom_t_9_2_atomic_record:
    setup:
        let shared ← Arc::new(EquivocationsTracker::new())
        (* the tracker uses an internal Mutex; pre-fix used a Try/Read+Write *)

    threads:
        thread_a:
            spawn { shared.clone().insert("v0", base_seq = 0, hash = 0xAAAA) }
        thread_b:
            spawn { shared.clone().insert("v0", base_seq = 0, hash = 0xBBBB) }

    join_all

    invariant (checked after both threads complete):
        let final_record ← shared.get(validator = "v0", base_seq = 0)
        assert final_record.hashes = {0xAAAA, 0xBBBB}
        (* both hashes must survive — neither thread's write may be lost *)
```

### 3.2 What the test does in pre-fix vs. post-fix code

| Code path                               | Loom exploration                                                                                                                              |
|-----------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------|
| **Pre-fix** (lock-free read-then-write) | Loom finds the schedule `t_A.read; t_B.read; t_A.write; t_B.write`, sees `final_record.hashes = {0xBBBB}`, panics with the assertion failure. |
| **Post-fix** (atomic RMW via Mutex)     | Loom explores all schedules, every one preserves both hashes, all assertions pass.                                                            |

This is the **same race** as the TLA⁺ counterexample in
[`../formal-methods/02-model-checking-tla.md §5`](../formal-methods/02-model-checking-tla.md);
two different abstractions are giving the same answer, which is the
methodology's gold-standard evidence pattern (see
[`../pipeline/03-evidence-stacking.md`](../pipeline/03-evidence-stacking.md)).

---

## 4 · Coupling with TLA⁺ — the same race, two abstractions

The fact that *both* Loom and TLA⁺ surface Bug #2 with essentially
the same 4-step trace is not a coincidence; it is **methodology
working as intended**. The two tools have complementary epistemics:

| Aspect                   | TLA⁺                                      | Loom                                            |
|--------------------------|-------------------------------------------|-------------------------------------------------|
| Abstraction level        | Protocol — "two threads call `insert`"    | Memory model — "two `load` / `store` orderings" |
| State space              | Finite, bounded by parameters             | Schedule space, bounded by thread count         |
| Failure manifestation    | Invariant violation in trace              | Rust panic / assertion failure in test          |
| Re-execution cost        | TLC run (seconds)                         | Loom run (sub-second to minutes)                |
| What it tells you        | "The protocol abstraction admits the bug" | "The actual Rust code admits the bug"           |
| What it doesn't tell you | What instruction sequence triggers it     | Whether the abstraction is faithful             |

A failure in **only one** of the two is informative:

- TLA⁺ fails, Loom passes → the protocol abstraction is wrong (or
  the Rust code accidentally provides stronger guarantees than the
  protocol requires).
- TLA⁺ passes, Loom fails → the Rust code has a defect the protocol
  abstraction did not foresee (the abstraction needs refinement).

A failure in **both** is the strongest signal: the protocol is
correct in capturing the bug, the Rust code is buggy in the way the
protocol predicted, and both artifacts will surface a regression if
either is later weakened.

---

## 5 · Pitfalls

### 5.1 Pitfall: Loom + non-Loom imports

A test that imports `std::sync::Mutex` instead of `loom::sync::Mutex`
under `cfg(loom)` runs against the production lock and Loom cannot
observe its operations. The test passes spuriously.

**Mitigation**: use the `cfg`-gated import pattern from
[§1.1](#11--what-loom-replaces); the slashing development standardizes
on it.

### 5.2 Pitfall: blocking operations inside Loom

A test that calls a blocking system call (`std::thread::sleep`, real
network I/O) under Loom's scheduler deadlocks or violates the
scheduler's invariants.

**Mitigation**: every Loom test in this development is **purely
synchronous** with respect to system resources; the only Loom-aware
operations are atomic ones on `loom::sync` types and `loom::thread::yield_now()`.

### 5.3 Pitfall: state-space explosion in Loom

A test with too many threads or too long an operation sequence
exhausts Loom's schedule budget and fails to complete.

**Mitigation**: every Loom test in this development has explicit
thread count and operation count that keeps the schedule space at
`≤ 10⁶`. The 4-thread variant is the maximum; higher thread counts
require deliberate planning.

### 5.4 Pitfall: forgetting `loom::model { … }` wrapper

A Loom test that does not wrap its body in `loom::model { … }` runs
exactly once, defeating the entire purpose of using Loom.

**Mitigation**: every Loom test file in
[`casper/tests/slashing/loom_t_*.rs`](../../../../../casper/tests/slashing/)
follows the pattern:

```rust
#[test]
#[cfg(loom)]
fn loom_t_9_2_atomic_record() {
    loom::model(|| {
        // … test body …
    });
}
```

Without the `loom::model` wrapper, the test compiles and passes but
provides no concurrency coverage. The slashing CI runs Loom tests
under `RUSTFLAGS="--cfg loom" cargo test --release` to enable Loom
specifically.

---

## 6 · Related work

- **Loom**: tokio-rs Loom [Loom].
- **C11/C++11 memory model**: Boehm *et al.* [BOSSW11].
- **Stateless model checking** (the algorithm Loom uses): Godefroid
  [God97], Flanagan & Godefroid [FG05].
- **Partial-order reduction**: Peled [Pel93].
- **ThreadSanitizer** (the complementary tool for data races):
  Serebryany & Iskhodzhanov [SI09].
- **CDSChecker** (academic stateless model checker for C/C++):
  Norris & Demsky [ND13].

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`../differential-and-metamorphic/01-differential-rust-vs-scala.md`](../differential-and-metamorphic/01-differential-rust-vs-scala.md)
— the **differential testing** arm of the methodology. Where the
arms covered so far operate on the system in isolation, differential
testing compares two implementations of the same abstract protocol —
and treats every disagreement as evidence.
