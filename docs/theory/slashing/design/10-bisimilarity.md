# 10 · Bisimilarity (Rust ↔ Scala)

## 10.1 The headline claim

The F1R3FLY Rust port of the slashing subsystem is **observationally
equivalent** to the Scala original *modulo* the nine documented
bug-fix deltas (§09) and a small set of structural conventions
(α-equivalence on rho-calculus names, iteration order on `BTreeSet`
vs `Set`).

In formal terms (theorem T-15, see verification §8):

```
Rust(S) ≈ₓ Scala(S)
```

where `≈ₓ` is *weak barbed equivalence* over the five-component
projection `x = {bonds, records, slashedSet, coopVault, forkChoice}`.

> **What this means in plain English.** A node operator running
> a Rust validator and a node operator running a Scala validator,
> given the same sequence of input events (block messages, deploys,
> network conditions), observe **the same on-chain bond map, the
> same equivocation records, the same slashed set, the same Coop
> vault balance, and the same fork-choice latest messages** — at
> every state.

[![Diagram 10 — Specification ↔ Rocq ↔ TLA+ ↔ Rust correspondence](../diagrams/10-component-formal-correspondence.svg)](../diagrams/10-component-formal-correspondence.svg)

## 10.2 Why bisimilarity matters

The audit-grade certification of the migration depends on
bisimilarity. Without it:

- A regression in the Rust port could ship undetected.
- The Scala upstream's existing audit reports (e.g. by RChain
  pre-fork) would not transfer to the Rust port.
- Cross-implementation interoperability would require independent
  re-audit.

With it:

- Every property the Scala port satisfies, the Rust port satisfies
  (modulo the documented widening at #9).
- The eight Scala-inherited bugs and their fixes are common to both
  implementations; fixing them in the spec automatically fixes them
  in both ports.
- The Rust regression at #2 is the *only* place we need to worry
  about Rust-specific test coverage.

## 10.3 The relation R

The bisimulation relation is defined in spec §9.1:

```
R = { (sR, sS) | sR.BondMap = sS.BondMap
              ∧ sR.EqRecords ≡ sS.EqRecords           [mutual containment, modulo iter order]
              ∧ sR.SlashedSet ≡ sS.SlashedSet         [mutual containment (slashed_bisim)]
              ∧ sR.CoopVaultBalance = sS.CoopVaultBalance
              ∧ sR.ForkChoiceLatestMessages ≡ sS.ForkChoiceLatestMessages   [forkchoice_bisim] }
```

Note the mix of `=` and `≡`:

- `BondMap` is a *function* (validator → ℕ); pointwise equal lookups
  for all keys ⟹ strict equality.
- `CoopVaultBalance` is a *number*; strict equality.
- `EqRecords`, `SlashedSet`, `ForkChoiceLatestMessages` are
  *sets / lists*; mutual containment because iteration order differs
  between Rust's `BTreeSet` and Scala's `Set`.

## 10.4 The five sub-bisimulations

Each component projection has its own sub-bisimulation in
`Bisimulation.v`:

| Sub-bisimulation       | File:line              | Reflexive | Symmetric | Transitive              |
|------------------------|------------------------|-----------|-----------|-------------------------|
| `bonds_bisim`          | `Bisimulation.v:30`    | ✓         | ✓         | ✓ (`Bisimulation.v:55`) |
| `records_bisim_strong` | `Bisimulation.v` §7    | ✓         | ✓         | ✓                       |
| `slashed_bisim`        | `Bisimulation.v:39-40` | ✓         | ✓         | ✓                       |
| `vault_bisim`          | (definitional `=`)     | ✓         | ✓         | ✓ (`eq_trans`)          |
| `forkchoice_bisim`     | `Bisimulation.v` §9    | ✓         | ✓         | ✓                       |

All five component relations carry reflexivity, symmetry, and
transitivity proofs. T-14 is therefore a full weak-barbed equivalence
over the five observable projections.

## 10.5 Theorem T-13 (split into a/b/c)

After the prior renaming pass, T-13 is split into three
sub-theorems, one per projection:

- **T-13a (Bonds projection).** *(`t_13_bm_slash_preserves_bonds_bisim`,
  `Bisimulation.v:77`.)* If `bonds_bisim(b₁, b₂)`, then
  `bonds_bisim(bm_slash(b₁, v), bm_slash(b₂, v))`.

- **T-13b (Records projection).** *(`records_bisim_strong_preserved_update`,
  `Bisimulation.v` §8.)* If `records_bisim_strong(s₁, s₂)`, then for
  every key `k` and hash `h`, applying the same update to both stores
  preserves the full strong record bisimulation.

- **T-13c (Fork-choice projection).** *(`forkchoice_bisim_preserves_filter`,
  `Bisimulation.v` §9.)* If `forkchoice_bisim(lm₁, lm₂)` and
  `bonds_bisim(b₁, b₂)`, then per-bond filtering preserves the
  bisimulation.

## 10.6 Theorem T-14 (Weak barbed equivalence)

**Statement.** *(`weak_barbed_equiv` (relation, `Bisimulation.v:367`),
`weak_barbed_equiv_refl` (`Bisimulation.v:376`),
`weak_barbed_equiv_sym` (`Bisimulation.v:388`), and
`weak_barbed_equiv_trans`.)* The five-component relation
`weak_barbed_equiv` is reflexive, symmetric, and transitive.

```
weak_barbed_equiv(b₁,b₂, rs₁,rs₂, sl₁,sl₂, v₁,v₂, lm₁,lm₂)
  := bonds_bisim(b₁,b₂)
   ∧ records_bisim_strong(rs₁,rs₂)
   ∧ slashed_bisim(sl₁,sl₂)
   ∧ vault_bisim(v₁,v₂)
   ∧ forkchoice_bisim(lm₁,lm₂)
```

## 10.7 Theorem T-15 (split into a/b)

- **T-15a (Pipeline composition).** *(`t_15_pipeline_step_preserves_R`,
  `MainTheorem.v:335`.)* Define a pipeline step as the composition

  ```
  pipeline_step(b, rs, sl, v, lm, offender, baseSeq, h)
    := (bm_slash(b, offender),
        update_record(rs, (offender, baseSeq), h),
        offender :: sl,
        v + bm_lookup(b, offender),
        filter_slashed(lm, bm_slash(b, offender)))
  ```

  Then under the strong bisimulation R, applying `pipeline_step`
  consistently on both sides preserves all five components.

- **T-15b (Composed bisimulation closure).** *(`main_bisimilarity_theorem`,
  `MainTheorem.v:232`.)* For every component triple, the slash
  transition preserves component-wise R-equivalence.

## 10.8 What T-15 lets you conclude

If a Rust node and a Scala node start in R-related states and
process the same input events:

1. They produce the same on-chain bond map (T-13a).
2. They produce the same equivocation records (modulo iter order, T-13b).
3. They produce the same slashed set (modulo iter order, slashed_bisim refl/sym).
4. They produce the same Coop vault balance (vault_bisim refl/sym).
5. They produce the same fork-choice latest-message map (T-13c).

Compose the per-component preservation across the pipeline (T-15a)
and you get end-to-end behavioral equivalence (T-15b /
`main_bisimilarity_theorem`).

## 10.9 What "modulo" means in T-15

The bisimilarity claim is **modulo**:

- **α-equivalence on Rholang names.** A standard equivalence on
  rho-calculus terms, justified in [MR05a]. Two names that differ
  only in their underlying byte representation but share the same
  binding structure are considered equivalent.

- **Iteration order on `BTreeSet` (Rust) vs `Set` (Scala).** Rust's
  ordered set and Scala's hash set produce different iteration
  orders but agree on element membership. The bisimilarity is
  *value-level*, not byte-level on-disk equality.

- **Seven Scala-inherited bug-fix deltas (T-9.1, T-9.3–T-9.8) and
  one Rust-introduced regression fix (T-9.2)** — all of which restore
  Rust↔Scala convergence (§09).

- **The deliberate Rust-side widening at bug #9 (T-9.9)** which
  admits self-correcting blocks Scala rejects. This is the **only
  intentional divergence** in the bisimilarity claim.

- **An authenticated PKI identity layer** (out of scope; T-15 holds
  modulo this assumption).

## 10.10 Why "weak" barbed equivalence?

In process-calculus theory [Mil89, Mil99, San98], *strong*
bisimulation requires matching internal `τ` steps as well as
observable actions. *Weak* bisimulation matches only observable
actions, treating arbitrary numbers of internal `τ` steps as
equivalent.

In the slashing subsystem:

- **Internal `τ` steps**: tracker reads, snapshot construction,
  proof-checking, network gossip, replay verification. None of these
  produce observable on-chain effects.
- **Observable actions**: bond mutations, record insertions,
  slashed-set changes, vault balance changes, fork-choice latest-
  message updates.

Two implementations that differ in *how* they internally arrive at
the same observable post-state are weakly bisimilar — even if
their internal step counts differ. This is exactly the right
notion for an *audit*: the auditor cares about what an observer
sees, not how many internal heartbeats the implementation needed.

## 10.11 Bisimilarity proof structure

The proof of T-15 is **componentwise**:

```
T-13a (bonds)         ┐
T-13b (records)       ├─→ Conjunction → T-14 (weak_barbed_equiv refl/sym)
T-13c (fork-choice)   │
slashed_bisim refl    │
vault_bisim refl      ┘

T-14 + per-component preservation under pipeline_step → T-15a
T-15a + composition over multi-step traces           → T-15b (main_bisimilarity_theorem)
```

Each leaf theorem is proven by direct unfolding and case analysis.
The composition is by structural induction on the pipeline trace.
The full chain is mechanically checked in `MainTheorem.v` with zero
`Admitted`.

## 10.12 What if a new bug is found?

If a tenth bug is discovered (post-fix), the maintenance procedure is:

1. **Document it** as bug #10 in spec §10 with the same structure
   as #1–#9: cause, pre-fix behavior, post-fix behavior, theorem
   T-9.10, bisimilarity impact, worked example, diagram.
2. **Mechanize the fix** in a new `BugFix*.v` module under
   `formal/rocq/slashing/theories/`.
3. **Add the bug to the §10.0 bug-class table** with origin
   classification (Scala-inherited / Rust-introduced / deliberate
   widening).
4. **Update T-15's "modulo" clause** in spec §13 to reflect the
   tenth delta.
5. **Re-run TLA+ model-checking** for any TLA+ invariant the new
   fix touches.

The procedure is *additive*: existing proofs remain valid; the new
bug-fix module attaches to the existing pipeline at the right
component layer.

---

**Next:** [§11 — Worked examples](11-worked-examples.md)
