# 02 · Adversarial search — damage optimization and deep-threat sweeps

> *“If you know the enemy and know yourself, you need not fear the
> result of a hundred battles.”* — Sun Tzu, *The Art of War*, c. 5th
> century BCE, trans. Lionel Giles [Tzu10].

This chapter explains the methodology's **adversarial search** layer:
once the attack tree is built
([`01-stride-and-attack-trees.md`](./01-stride-and-attack-trees.md)),
the next question is *how to search the adversary's strategy space
efficiently*. The slashing development uses **objective-guided
search** [Gol89] over bounded adversarial parameter spaces, scored by
attack-objective functions.

Organization:

- [§1 — The adversary's strategy space](#1--the-adversarys-strategy-space)
- [§2 — Objective-guided search](#2--objective-guided-search--why-it-is-better-than-uniform-sampling)
- [§3 — Damage optimizer](#3--damage-optimizer--algorithm-and-output)
- [§4 — Deep-threat model](#4--deep-threat-model--multi-objective-sweeps)
- [§5 — Adversarial timing game](#5--adversarial-timing-game)
- [§6 — Pitfalls](#6--pitfalls)
- [§7 — Related work](#7--related-work)

---

## 1 · The adversary's strategy space

The slashing adversary's strategy is a tuple:

```
σ ≜ ⟨equivocators, neglect_edges, visibility, reports,
     stake_distribution, gossip_schedule, validator_churn⟩
```

The strategy space is the Cartesian product of:

| Coordinate           | Domain                                          | Cardinality (n=4, depth=4) |
|----------------------|-------------------------------------------------|----------------------------|
| `equivocators`       | `Subsets(Validators)`                           | `2⁴ = 16`                  |
| `neglect_edges`      | `Subsets(Validators × Validators ∖ diag)`       | `2¹² = 4 096`              |
| `visibility`         | `Maps(Validator → Subsets(Equivocators))`       | `2¹⁶ = 65 536`             |
| `reports`            | `Subsets(Equivocators × Time)`                  | `2³² ≈ 4 × 10⁹`            |
| `stake_distribution` | `Vectors(Validators → ℤ⁺)` bounded to `1..S`    | `S⁴ = 625` (S=5)           |
| `gossip_schedule`    | `Permutations(Edges)`                           | `factorial(12) ≈ 5 × 10⁸`  |
| `validator_churn`    | `Sequences(Validator → {Bond, Unbond, Rebond})` | `unbounded`                |

Product: `≈ 10²⁴` even at modest bounds. Uniform random sampling
visits a negligible fraction; **uniform sampling is the wrong
algorithm**.

### 1.1 What the adversary actually wants

The adversary does not want a uniformly random strategy; it wants
one that **maximizes an objective**:

| Adversary objective       | Definition                                                       |
|---------------------------|------------------------------------------------------------------|
| Honest validators slashed | `|closure(σ)| − |equivocators(σ)|`                               |
| Quorum drop               | `quorum_required − active_after(σ)`                              |
| Accountability gap        | `|full_visibility_closure(σ)| − |partial_visibility_closure(σ)|` |
| Delay                     | `max_rounds_to_closure(σ)`                                       |
| Damage ratio              | `slashed_stake(σ) / direct_offender_stake(σ)`                    |

The methodology uses these objectives as the **scoring function** for
the search.

---

## 2 · Objective-guided search — why it is better than uniform sampling

Objective-guided search [Gol89] is the algorithmic family that
includes genetic algorithms, simulated annealing, MCMC, and
*novelty search* [LS11]. The core idea is:

> *Sample strategies in proportion to their objective score, not
> uniformly.*

For the slashing strategy space, the speedup over uniform sampling
is dramatic. A particular adversarial witness — say, the
`n=4, depth=3` chain attack that amplifies a single direct offender's
damage by a factor of 4 (Sage finding #4) — has probability
≈ `10⁻²³` under uniform sampling. Under damage-objective-guided
search, the same witness emerges within `O(10⁴)` strategy evaluations.

### 2.1 The algorithm template

```
algorithm objective_guided_search(
        budget : ℕ, n_init : ℕ, objective : Strategy → ℝ
    ) → List(Strategy):
    let population ← random_sample(n_init)
    let scored     ← [(σ, objective(σ)) for σ ∈ population]
    let best       ← top_k(scored, k = n_init / 4)

    repeat budget times:
        for each (σ, _) in best:
            let σ' ← perturb(σ)              (* mutate one coordinate *)
            scored.append((σ', objective(σ')))
        best ← top_k(scored, k = n_init / 4)

    return [σ for (σ, _) in best]
```

The `perturb` function is the **innovation engine**: it generates a
neighbor of a high-scoring strategy by mutating one coordinate at a
time. Local moves preserve most of the strategy's structure; the
neighborhood is small enough that the perturbed strategy is also
likely to score well.

### 2.2 Frontier vs. damage objective

The slashing development uses two complementary objective functions:

1. **Damage objective** — directly maximize slashed honest stake.
2. **Novelty objective** — maximize *behavioral diversity* (Lehman
   & Stanley [LS11]). The novelty objective ignores the damage score
   and instead picks strategies whose *behaviors* (observable
   classification, closure depth, accountability gap) are distant
   from previously-seen behaviors.

The novelty objective is crucial because the damage objective often
gets stuck in a local optimum (one particular attack shape that
slashes 1 honest validator). Novelty search escapes the optimum and
finds genuinely different attacks.

---

## 3 · Damage optimizer — algorithm and output

The Sage script
[`formal/sage/slashing/damage_optimizer.sage`](../../../../../formal/sage/slashing/damage_optimizer.sage)
implements damage-objective-guided search. The output structure is:

```json
{
  "kind": "damage_optimizer_witness",
  "n": 4,
  "stakes": [3, 3, 3, 3],
  "fault_budget": 3,
  "equivocators": [3],
  "edges": [[0, 1], [1, 2], [2, 3]],
  "closure": [0, 1, 2, 3],
  "slashed_stake_total": 12,
  "direct_offender_stake": 3,
  "extra_slashed_stake": 9,
  "amplification_factor": 4,
  "depth": 3
}
```

### 3.1 Reading the witness

This is **Sage finding #4** (see
[`formal/sage/slashing/FINDINGS.md`](../../../../../formal/sage/slashing/FINDINGS.md)):
a chain of neglect edges amplifies a single direct offender's
damage by a factor of 4 in the worst case. The attack is a 4-node
chain `0 ← 1 ← 2 ← 3` where node 3 is the direct offender and the
other three nodes each fail to acknowledge node 3 in their
justifications.

### 3.2 How this becomes formal evidence

The witness flows through:

1. **Classification** under the threat-model vocabulary —
   classified `candidate_boundary` (the BFT bound is what limits the
   chain length, not the chain shape itself).
2. **Promotion** to the Rocq theorem
   `two_level_closure_depth_bound`
   (`formal/rocq/slashing/theories/TwoLevelSlashing.v`), which proves
   the chain depth is bounded by `n − 1`.
3. **TLA⁺ corroboration** via `TwoLevelSlashing.tla`
   `Inv_ClosureTermination` and `Inv_BFTBound`.
4. **Rust regression** at
   [`casper/tests/slashing/prop_t_11_neglect_closure.rs`](../../../../../casper/tests/slashing/prop_t_11_neglect_closure.rs).

The witness is therefore present in **four artifacts** that protect
the system from a regression that would re-introduce the amplification
beyond the BFT bound.

---

## 4 · Deep-threat model — multi-objective sweeps

The Sage script
[`formal/sage/slashing/deep_threat_model.sage`](../../../../../formal/sage/slashing/deep_threat_model.sage)
is the methodology's most extensive adversarial search. It runs
multi-objective optimization across the entire strategy space:

```
algorithm deep_threat_sweep(bounds : SearchBounds) → MultiObjFrontier:
    let frontier ← []
    for each objective in [damage, quorum_drop, gap, delay, ratio]:
        for each adversary_class in [
                rational, byzantine, censorship, withholding,
                long_range, bribery, partition
            ]:
            let local_results ← objective_guided_search(
                budget    = bounds.budget,
                n_init    = bounds.n_init,
                objective = objective,
                constraint = adversary_class.constraints,
            )
            for each σ in local_results:
                if σ is Pareto-dominant in frontier:
                    frontier.append(σ)
    return frontier
```

### 4.1 The Pareto-frontier output

A strategy `σ₁` Pareto-dominates `σ₂` if `σ₁` scores at least as
well as `σ₂` on every objective and strictly better on at least one.
The deep-threat sweep produces the **frontier** — the set of
strategies that no other strategy dominates.

The frontier is what the auditor reads first when evaluating
adversary strength; every frontier point is a *concrete*
adversarial strategy with a *measurable* impact. The methodology
requires every frontier point to be:

1. Classified under the threat-model vocabulary.
2. Either reproduced on the Rust path (with a status from the
   traceability ledger) or documented as a model-only artifact.
3. Promoted to a Rocq theorem (if it strengthens an invariant) or
   a regression test (if it represents a concrete defense).

---

## 5 · Adversarial timing game

The `adversarial_timing_game.sage` model
([`formal/sage/slashing/adversarial_timing_game.sage`](../../../../../formal/sage/slashing/adversarial_timing_game.sage))
extends the damage objective to **timing**: when does the adversary
release evidence, when does it propose, when does it withhold?

This is a separate model because timing introduces a new dimension
(`when`) on top of the existing dimensions (`what`, `where`). The
methodology factors timing out for the same reason TLA⁺ models are
factored: state-space cost.

### 5.1 The timing dimensions

| Dimension             | What the adversary controls                                                        |
|-----------------------|------------------------------------------------------------------------------------|
| Evidence release time | When the offending block is broadcast (immediate, delayed by `k` rounds, withheld) |
| Report timing         | When a report is submitted that shrinks accountability closure                     |
| Proposer slot timing  | When the adversary's proposer slot arrives (controlled via stake bonding)          |
| Slash deploy timing   | When the SlashDeploy is included in a block (immediate, next epoch, withheld)      |

The Sage model enumerates small combinations of these and finds
worst-case timing for each adversary objective.

---

## 6 · Pitfalls

### 6.1 Pitfall: optimizing the wrong objective

A search that maximizes `slashed_stake_total` finds attacks where
the *adversary* is heavily slashed. The adversary doesn't care about
that; it cares about *honest* validators being slashed. The
objective must be carefully formulated.

**Mitigation**: every objective in this development is documented
with an explicit *“the adversary's gain is X”* description, and is
peer-reviewed before being added to the deep-threat sweep.

### 6.2 Pitfall: search budget exhausted on noise

A small search budget on a high-dimensional space wastes evaluations
on noise. Conversely, an overly large budget wastes wall-clock time
on diminishing returns.

**Mitigation**: every search in this development is **budgeted**
explicitly (`--budget N`); the CI smoke uses `N = 10³`, nightly uses
`N = 10⁶`. The budget is chosen empirically from the convergence
curve.

### 6.3 Pitfall: the search forgets

A search run that does not persist its frontier loses information
between runs. The methodology uses **persistent corpora**: every
frontier is appended to `formal/sage/slashing/FINDINGS.md` and
replayed by `casper/tests/slashing/minimal_counterexample_catalog_replay.rs`.

**Mitigation**: same as the Hypothesis persistent-corpus pattern;
see [`../randomized-search/02-stateful-hypothesis.md §4.3`](../randomized-search/02-stateful-hypothesis.md).

### 6.4 Pitfall: model is more adversarial than the protocol

A Sage search may permit adversary capabilities the Rust path does
not. For example, the Sage model may allow validators to send blocks
out-of-order; the Rust path enforces sequence-number monotonicity
before the detector even sees the block.

**Mitigation**: every Sage adversarial model has a **constraint
predicate** mirroring the production preconditions; constraints are
applied before the search begins. See `corpus_generator.sage`'s
constraint section for the canonical pattern.

---

## 7 · Related work

- **Genetic algorithms / objective-guided search**: Goldberg [Gol89].
- **Novelty search**: Lehman & Stanley [LS11].
- **Pareto-optimal multi-objective optimization**: Deb [Deb01].
- **Adversarial example search in machine learning** (parallel
  literature): Szegedy *et al.* [Sze14].
- **Game-theoretic threat modeling**: Khouzani *et al.* [Khouz19].
- **Adversarial schedule synthesis in distributed systems**: Yang
  *et al.* [Yang11] (MoDist).

DOIs in [`../references.md`](../references.md).

---

## 8 · Next chapter

[`03-economic-game-theoretic.md`](./03-economic-game-theoretic.md) —
the **rational adversary** layer. Where this chapter searched the
adversary's strategy space mechanically, the next chapter asks
*why* the adversary would play that strategy — incentives,
bribery, long-range attacks, nothing-at-stake.
