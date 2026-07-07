# 01 · Property-based testing with proptest

> *“Beware of bugs in the above code; I have only proved it correct,
> not tried it.”* — Donald Knuth, in a letter to Peter van Emde Boas,
> 1977.

This chapter explains the role of proptest in the slashing
methodology. proptest is the Rust port of the QuickCheck idea
[CH00, Hug00]: write a *property* — a statement about the system
that should hold for all inputs of a certain shape — and let the
framework sample inputs randomly, with **shrinking** to minimize any
counterexample.

Organization:

- [§1 — When proptest is the right tool](#1--when-proptest-is-the-right-tool)
- [§2 — Anatomy of a slashing proptest property](#2--anatomy-of-a-slashing-proptest-property)
- [§3 — The 27 slashing proptest properties](#3--the-27-slashing-proptest-properties)
- [§4 — Shrinking and minimal counterexamples](#4--shrinking-and-minimal-counterexamples)
- [§5 — Common anti-patterns](#5--common-anti-patterns)
- [§6 — Related work](#6--related-work)

---

## 1 · When proptest is the right tool

proptest occupies the **cheap, fast, in-Rust** slot in the
methodology:

| Strength                                        | Cost                                                 |
|-------------------------------------------------|------------------------------------------------------|
| Sub-second feedback on a focused property       | Cannot prove absence of counterexamples; only sample |
| Native Rust — no language boundary to cross     | Random sampling misses sparse witnesses              |
| Built-in shrinking minimizes any counterexample | Stateful behavior is awkward to express              |
| Replays a failing seed deterministically        | Bounded by `cases` parameter (typically 256–2048)    |

The methodology uses proptest for:

1. **Single-step properties** — a property about one harness step
   (*“slashing zeros the bond”*, T-7).
2. **Bisimilarity at small bounds** — Rust ≈ oracle on `n ≤ 8`,
   `depth ≤ 10` (T-13a/b/c, T-14, T-15).
3. **Idempotence and monotonicity** — slashing twice equals slashing
   once (T-Idem); record set is non-decreasing (T-5).
4. **Soundness, completeness on small inputs** — sampling confirms
   theorems for small cases before the Rocq proof generalizes them
   (T-1, T-2).

It is **not** used for:

- Stateful multi-step traces — use Hypothesis instead (see
  [`02-stateful-hypothesis.md`](./02-stateful-hypothesis.md)).
- Byte-level parser stress — use libFuzzer (see
  [`03-coverage-guided-fuzzing.md`](./03-coverage-guided-fuzzing.md)).
- Concurrency interleavings — use Loom (see
  [`04-concurrency-interleaving-loom.md`](./04-concurrency-interleaving-loom.md)).
- Establishing absence of counterexamples — use Rocq.

---

## 2 · Anatomy of a slashing proptest property

The 27 proptest properties in the slashing test suite share a common
structure. The example below
([`casper/tests/slashing/prop_t_1_detection_sound.rs`](../../../../../casper/tests/slashing/prop_t_1_detection_sound.rs))
is the simplest of them.

### 2.1 The property in literate form

```
property t_1_honest_validator_never_recorded:
    sample validator_count ← uniform(2, 8)
    sample depth          ← uniform(1, 10)

    let H ← new SlashingTestHarness(validator_count, initial_bond = 100)
    for each seq in 0 .. depth:
        for each i in 0 .. validator_count:
            let v ← "v{i}"
            let h ← H.sign_block(v, seq)         (* honest single block per seq *)
            let s ← H.dispatch(h)
            assert s = Status::Valid              (* honest blocks must classify Valid *)

    for each i in 0 .. validator_count:
        let v ← "v{i}"
        for each base_seq in 0 .. depth:
            assert ¬ H.has_record(v, base_seq)    (* honest validator must have no record *)
        assert H.is_active(v)                     (* honest validator must remain active *)
        assert H.bond(v) > 0                      (* honest validator must retain bond *)
```

### 2.2 The Rust translation

```rust
proptest! {
    #![proptest_config(ProptestConfig { cases: 256, .. ProptestConfig::default() })]

    #[test]
    fn t_1_honest_validator_never_recorded(
        validator_count in 2usize..8,
        depth in 1u64..10,
    ) {
        let mut harness = SlashingTestHarness::new(validator_count, 100);
        for seq in 0..depth {
            for i in 0..validator_count {
                let v = format!("v{}", i);
                let hash = harness.sign_block(&v, seq);
                let status = harness.dispatch(hash);
                prop_assert_eq!(status, Status::Valid,
                    "honest block must classify Valid");
            }
        }
        // … (record / active / bond assertions)
    }
}
```

### 2.3 The four roles of the harness in a proptest

A proptest in this development always interacts with the
`SlashingTestHarness` (see
[`casper/tests/slashing/harness.rs`](../../../../../casper/tests/slashing/harness.rs)):

| Role                | API surface                                                                                         |
|---------------------|-----------------------------------------------------------------------------------------------------|
| **Setup**           | `SlashingTestHarness::new(validator_count, initial_bond)`                                           |
| **Action**          | `sign_block(v, seq)`, `equivocate_block(v, seq)`, `try_bond(v, stake)`, …                           |
| **Observation**     | `dispatch(hash) → Status`, `has_record(v, base_seq) → bool`, `bond(v) → i64`, `is_active(v) → bool` |
| **Invariant check** | `prop_assert!`, `prop_assert_eq!`                                                                   |

This separation — pure setup / action / observation / assertion —
is the reason proptest properties in this development reliably
produce minimal counterexamples; the framework can shrink each axis
independently.

---

## 3 · The 27 slashing proptest properties

The full enumeration, with theorem citation, is in
[`../../design/14-test-plan.md`](../../design/14-test-plan.md);
the table below is a representative subset to illustrate the
*coverage shape*.

| File                                      | Theorem   | Property                                                                 |
|-------------------------------------------|-----------|--------------------------------------------------------------------------|
| `prop_t_1_detection_sound.rs`             | T-1       | Honest validators never recorded                                         |
| `prop_t_2_detection_complete.rs`          | T-2       | Every real equivocator is eventually recorded                            |
| `prop_t_3_slashable_taxonomy.rs`          | T-3       | `is_slashable(s)` ⇔ `s ∈ {17 slashable variants}`                        |
| `prop_t_4_record_uniqueness.rs`           | T-4       | At most one record per `(v, base_seq)`                                   |
| `prop_t_5_record_monotonicity.rs`         | T-5       | Record set is non-decreasing in any trace                                |
| `prop_t_6_neglect_detection.rs`           | T-6       | Neglected equivocations detected at all valid views                      |
| `prop_t_7_slash_zeros_bond.rs`            | T-7       | `slash(v)` ⇒ `bond(v) = 0`                                               |
| `prop_t_9_1_ignorable_safety.rs`          | T-9.1     | Ignorable equivocations record (Bug #1 post-fix invariant)               |
| `prop_t_9_3_catchall_records.rs`          | T-9.3     | Every slashable variant records (Bug #3 post-fix invariant)              |
| `prop_t_9_4_transfer_failure.rs`          | T-9.4     | Failed `transfer` leaves bonds untouched (Bug #4)                        |
| `prop_t_9_5_active_has_positive_bond.rs`  | T-9.5     | Active set ⊆ {validators with bond > 0} (Bug #5)                         |
| `prop_t_9_6_self_regression.rs`           | T-9.6     | Self-regression detected as equivocation (Bug #6)                        |
| `prop_t_9_7_seqnum_density.rs`            | T-9.7     | Density check is closed under `seq ↦ seq + 1` (Bug #7)                   |
| `prop_t_9_8_unbonded_proposer.rs`         | T-9.8     | `prepare_slashing_deploys` rejects unbonded proposers (Bug #8)           |
| `prop_t_9_10_withdraw_safety.rs`          | T-9.10    | Failed `transfer` preserves `withdrawers` (Bug #10)                      |
| `prop_t_9_11_detector_*.rs` (three files) | T-9.11    | Detector totality, permutation invariance, bisim under complete pointers |
| `prop_t_11_neglect_closure.rs`            | T-11      | Two-level closure converges in ≤ `n − 1` rounds                          |
| `prop_t_12_quorum_preservation.rs`        | T-12      | Closure preserves BFT quorum under `f < n/3`                             |
| `prop_t_13a_bonds_bisim.rs`               | T-13a     | Bond map ≈ between harness and oracle                                    |
| `prop_t_13b_records_bisim.rs`             | T-13b     | Record set ≈ between harness and oracle                                  |
| `prop_t_13c_forkchoice_bisim.rs`          | T-13c     | Fork-choice projection ≈ between harness and oracle                      |
| `prop_t_14_weak_barbed_equiv.rs`          | T-14      | Weak barbed bisimilarity Rust ↔ oracle on bounded traces                 |
| `prop_t_15_bisim_under_workload.rs`       | T-15      | Bisimilarity holds under a random workload                               |
| `prop_t_auth_check.rs`                    | T-Auth    | System auth-token guard                                                  |
| `prop_t_idem_slash_idempotence.rs`        | T-Idem    | Slashing twice = slashing once                                           |
| `prop_t_invariants_under_workload.rs`     | composite | All invariants under a random workload                                   |
| `prop_t_triple_bisim_*.rs` (three files)  | T-15a/b   | Triple-bisim harness ↔ oracle ↔ production adapter                       |

### 3.1 The coverage shape

Three observations about this catalog:

1. **One theorem ⇒ one property file**. Every theorem in
   [`../../slashing-verification.md`](../../slashing-verification.md)
   has a corresponding proptest file; the proptest is the *executable
   sanity check* of the theorem on small bounds.
2. **One bug ⇒ one post-fix property file**. The `prop_t_9_N_*.rs`
   files match the bug-fix numbering in
   [`../../design/09-bug-fixes-and-rationale.md`](../../design/09-bug-fixes-and-rationale.md);
   the property is the *positive* statement of *“the bug cannot
   recur”*.
3. **Triple coverage on bisimilarity**. Bisimilarity is the headline
   claim and the most subtle one; it is checked at three levels
   (T-13a/b/c per projection, T-14 weak-barbed, T-15a/b under
   workload).

---

## 4 · Shrinking and minimal counterexamples

The single most valuable feature of proptest is **shrinking**:
when a property fails on input `x`, the framework iteratively
replaces `x` with smaller candidates `x'`, re-runs the test, and
records the smallest still-failing input. The minimized
counterexample is what gets reported, typically printed verbatim in
the failure message:

```
Test failed: prop_assert_eq! failed: status == Status::Valid …
minimal failing input: validator_count = 2, depth = 1
```

The methodology relies on shrinking for two reasons:

1. **Debugging speed** — a failing input `(validator_count=7,
   depth=8)` is much harder to reason about than the shrunk
   `(validator_count=2, depth=1)`.
2. **Regression test seed** — the shrunk input goes directly into a
   `pre_fix_bug_N.rs` regression file as the deterministic seed for
   future runs.

### 4.1 The shrinking pseudocode

```
algorithm proptest_shrink(failing_input : I, prop : I → Bool) → I:
    let candidate ← failing_input
    loop:
        let smaller_candidates ← generate_smaller(candidate)
        let still_failing ← { c ∈ smaller_candidates | ¬ prop(c) }
        if still_failing = ∅:
            return candidate           (* candidate is locally minimal *)
        candidate ← any(still_failing)
```

This is greedy descent, not global minimization; in practice it
yields very small counterexamples within a few hundred attempts.

### 4.2 What shrinking *cannot* do

Shrinking minimizes the **input**, not the **trace**. For multi-step
properties — *“in some history of N actions, the system reaches a
bad state”* — proptest shrinks `N` and the individual actions, but
the strategy is shallow. The Hypothesis framework
([`02-stateful-hypothesis.md`](./02-stateful-hypothesis.md))
shrinks at the action-sequence level natively and is preferred for
those properties.

---

## 5 · Common anti-patterns

### 5.1 Anti-pattern: testing the implementation, not the property

**Symptom**: the property reads as a paraphrase of the Rust code
under test:

```rust
prop_assert_eq!(harness.dispatch(h), harness.expected_status(h));
```

This is a tautology unless `expected_status` is a *different*
implementation. The methodology requires the property to be a
**statement about the behavior**, expressible in mathematical English
without referring to the Rust function being tested.

### 5.2 Anti-pattern: silent precondition

**Symptom**: the property assumes the input is *“reasonable”* (e.g.
all validators have positive bond) without asserting it.

```rust
proptest! {
    fn p(v in any::<String>()) { /* assumes v ∈ Validators, no check */ }
}
```

If the property holds vacuously on inputs that violate the implicit
precondition, the test is meaningless. The methodology requires
preconditions to be **explicit** via `prop_assume!` (which causes
the framework to discard the case and try another), not implicit.

### 5.3 Anti-pattern: stateful test masquerading as a property

**Symptom**: a property creates a long sequence of harness operations
and checks an invariant at the end.

```rust
fn property(actions in vec(any_action(), 10..20)) {
    for a in actions { harness.apply(a); }
    prop_assert!(invariant(harness));
}
```

This is **stateful testing wearing the proptest skin**. proptest does
not shrink the action sequence intelligently — it cuts the vector
length blindly, often missing the minimal counterexample. The
methodology requires multi-step stateful tests to use Hypothesis
(via `hypothesis_*.rs` files).

### 5.4 Anti-pattern: assertion outside the harness contract

**Symptom**: the property accesses an internal field of the harness
or a private function.

```rust
let internal = harness.equivocation_tracker.lock().unwrap();
prop_assert!(internal.contains_key(&v));   /* private field */
```

This breaks the harness's observation contract (see
[§2.3](#23--the-four-roles-of-the-harness-in-a-proptest)). The
methodology requires every assertion to use the documented
observation API — `has_record`, `bond`, `is_active`, etc. — so the
test survives internal refactors.

---

## 6 · Related work

- **QuickCheck**: Claessen & Hughes [CH00], Hughes [Hug00].
- **proptest**: Rust port of QuickCheck by Jason Lingle; see the
  proptest book at [https://proptest-rs.github.io/proptest/](https://proptest-rs.github.io/proptest/).
- **Shrinking strategies**: Hughes [Hug16] surveys the state of
  shrinking in property-based testing.
- **Integration with theorem proving**: see Dybjer *et al.* [DyHaTa03]
  for QuickChick-style integration of QuickCheck with Coq, which
  inspires the harness/oracle bridge used here.

DOIs in [`../references.md`](../references.md).

---

## 7 · Next chapter

[`02-stateful-hypothesis.md`](./02-stateful-hypothesis.md) — Python-
based stateful property testing with built-in trace shrinking.
Where proptest shrinks inputs to one-step properties, Hypothesis
shrinks multi-step lifecycle traces — essential for the
adversarial-campaign and multi-epoch scenarios.
