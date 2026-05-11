# 02 · Metamorphic relations

> *“If you can't change the world, change the input.”* — Adapted
> from the metamorphic-testing literature, Chen *et al.* [CTH98].

This chapter explains **metamorphic testing** in the slashing
methodology. Metamorphic testing [CTH98, SCY18] is a technique for
deriving testable properties when an *oracle* is unavailable or
expensive: instead of asking *“is this the right answer?”*, the
engineer asks *“if I transform the input in a known way, does the
output transform predictably?”*. The transformation is the
**metamorphic relation**.

Organization:

- [§1 — Why metamorphic testing](#1--why-metamorphic-testing-in-a-system-with-a-rocq-oracle)
- [§2 — The slashing metamorphic relations](#2--the-slashing-metamorphic-relations)
- [§3 — Literate walkthrough of permutation invariance](#3--literate-walkthrough-of-permutation-invariance)
- [§4 — Pitfalls](#4--pitfalls)
- [§5 — Related work](#5--related-work)

---

## 1 · Why metamorphic testing in a system *with* a Rocq oracle?

The slashing development has a Rocq oracle, so the oracle-absent
motivation for metamorphic testing does not apply. Why is the
technique still used?

Three reasons:

1. **Cheap regression coverage of *operations the oracle is
   indifferent to***. A metamorphic relation like *“permuting the
   insertion order of records leaves the record set unchanged”*
   is trivially true in the oracle (records are a set), but it is a
   non-trivial property of the Rust implementation (which uses
   ordered maps internally). The metamorphic test catches a class
   of regressions where the implementation accidentally exposes
   the iteration order.
2. **Confidence in the oracle's contract**. A metamorphic relation
   that holds for both the oracle and the Rust implementation
   corroborates that both share the same operational contract. A
   relation that holds in only one is informative — it surfaces a
   semantic difference the bisimilarity proof should have prevented.
3. **Shrinking-friendly properties**. Metamorphic relations are
   typically *universally quantified over transformations* (e.g.
   *“for all permutations …”*) and are well-suited to proptest's
   shrinking. They are cheaper than full triple-bisim runs.

---

## 2 · The slashing metamorphic relations

The slashing development encodes seven metamorphic relations, each as
a proptest property:

| Relation                            | Statement                                                                            | File                                                                       |
|-------------------------------------|--------------------------------------------------------------------------------------|----------------------------------------------------------------------------|
| **Permutation invariance**          | Record set is invariant under insertion-order permutations                            | `prop_t_9_11_detector_permutation_invariance.rs`                            |
| **Idempotence**                     | `slash(slash(v)) = slash(v)`                                                          | `prop_t_idem_slash_idempotence.rs`, `uc_24_slash_idempotence_trace.rs`     |
| **Monotonicity**                    | Record set never shrinks across the trace                                             | `prop_t_5_record_monotonicity.rs`                                           |
| **Replay determinism**              | Replaying a fixed trace produces the same final state                                 | `replay_determinism.rs`, `uc_14_replay_after_crash.rs`                     |
| **Validator renaming equivariance** | Renaming all validators consistently does not change observable outcomes              | `prop_t_9_11_detector_bisim_under_complete_pointers.rs` (subsumes)         |
| **Detector totality**               | Detector terminates on every well-formed view                                         | `prop_t_9_11_detector_totality.rs`                                          |
| **Frontier monotonicity**           | Closure depth is monotone in the input edge set                                       | `frontier_monotonicity_merge_basis.rs`                                     |

Each relation is small, mechanically checkable, and catches a
distinct refactor risk.

---

## 3 · Literate walkthrough of permutation invariance

The permutation-invariance property is the simplest and most-cited
metamorphic relation in this development.

### 3.1 The relation in mathematical form

Let `R = {(v₁, s₁, h₁), (v₂, s₂, h₂), …, (vₖ, sₖ, hₖ)}` be a multiset
of equivocation records, and let `π : 1..k → 1..k` be any permutation.
Define `insert_in_order(R, π)` as the harness state obtained by
inserting the records in the order `π(1), π(2), …, π(k)`.

> **Permutation invariance**:
> `∀ π. insert_in_order(R, π).records = insert_in_order(R, id).records`

In English: *the final record set is independent of the order in
which the records were inserted*.

### 3.2 The property in literate form

```
property permutation_invariance:
    sample n_records ← uniform(2, 10)
    sample records   ← unique_records(n_records)
    sample π         ← random_permutation(n_records)

    let H_id  ← new SlashingTestHarness(…)
    let H_π   ← new SlashingTestHarness(…)

    for i in 0 .. n_records:
        H_id.insert(records[i])
    for i in 0 .. n_records:
        H_π.insert(records[π(i)])

    assert H_id.all_records() = H_π.all_records()
```

The Rust translation lives in
[`casper/tests/slashing/prop_t_9_11_detector_permutation_invariance.rs`](../../../../../casper/tests/slashing/prop_t_9_11_detector_permutation_invariance.rs).

### 3.3 What this catches that a bisimilarity test would not

A bisimilarity test compares the implementation against the oracle
on **a specific** trace. A metamorphic test compares the
implementation against *itself* under a transformation.

Suppose a refactor introduces an off-by-one in the iteration order
inside the tracker (e.g. uses `Vec` instead of `BTreeSet`). The
record-set is still *eventually* correct, so a bisimilarity test
that compares final states passes. But the **intermediate** order
matters for the public observation API; a metamorphic test that
inserts in permuted order surfaces the disagreement.

This is the canonical *“regression caught only by metamorphic
testing”* pattern from the literature [SCY18 §4].

---

## 4 · Pitfalls

### 4.1 Pitfall: the relation does not hold in the oracle

A metamorphic relation that the oracle violates is **not** a property
of the system; promoting it to a proptest produces a flaky test.

**Mitigation**: every metamorphic relation in this development is
*derived from* a Rocq theorem (typically a `Theorem … _equivariance`
or `_commutes_with_`). The Rocq theorem is the oracle's guarantee
that the relation holds.

### 4.2 Pitfall: the transformation breaks an invariant

Some transformations break the system's preconditions — e.g.,
permuting records may put a "neglect closure" record before its
"direct offender" record, violating a temporal ordering the
implementation assumes.

**Mitigation**: every metamorphic property in this development has
explicit `prop_assume!` guards on the transformation; sequences that
break preconditions are discarded.

### 4.3 Pitfall: the relation is too weak

A metamorphic relation that is trivially true (e.g. the identity
transformation preserves the output) catches no regressions.

**Mitigation**: every relation in this development corresponds to a
*meaningful* algebraic property — commutativity, associativity,
idempotence, distributivity — verified to non-trivially constrain
the implementation.

---

## 5 · Related work

- **Metamorphic testing**: Chen *et al.* [CTH98] introduce the
  technique; Segura *et al.* [SCY18] survey the state of the art.
- **Algebraic specification**: Guttag & Horning [GH78] are the
  intellectual ancestors of the *“algebraic property of the
  implementation”* framing used here.
- **Equivariance in machine learning**: a parallel use of the same
  concept, e.g. group-equivariant convolutional networks [CW16].

DOIs in [`../references.md`](../references.md).

---

## 6 · Next chapter

[`03-triple-bisimilarity.md`](./03-triple-bisimilarity.md) — the
**triple-bisim driver** that runs the Rust harness, the Rocq oracle
mirror, and the production adapter against the same trace and
asserts triple agreement.
