# Fork-Choice ("Ghosting") — Verification & Hardening Dossier

> **Status:** casper's LMD-GHOST fork-choice logic is **formally verified end-to-end**
> — Rocq (axiom-free capstones), TLA⁺ (determinism + LCA models with dual
> counterexamples), Z3/Sage/Wolfram cross-witnesses, and Rust regression tests. The
> headline finding is **honest**: unlike the finalized-floor (which had three real
> safety bugs), the ghosting logic is **largely correct** — the prime fork suspect
> (tie-break non-determinism) is **refuted** — with three **low-severity** robustness
> seams (B1/B2/B3) fixed full-stack and one **reframe** (validators do not recompute
> fork choice). Verification is **local-only** — no Rocq/TLA⁺/Z3/Sage/Wolfram step is
> wired into CI.

This document is written so the design and its verification can be reconstructed from
scratch. It records the feature, the bug-hunt outcome, the fixes, the formal artifacts
(with theorem anchors), the invariant catalog, and exactly how to re-run every check.

> **Companion docs.** [`fork-choice-specification.md`](./fork-choice-specification.md)
> is the normative contract (RFC-2119); [`fork-choice-glossary.md`](./fork-choice-glossary.md)
> defines every symbol/term used here (before first use) and gives literate-programming
> pseudocode for the score accumulation and the heaviest-subtree ranking. Rendered
> PlantUML diagrams live in [`diagrams/`](./diagrams/) and are embedded in §8.

---

## 1. The feature

The **fork-choice estimator** selects the canonical tip(s) of the block DAG. For a DAG
`d` and a frozen `latest_messages` map:

- **filter** — remove slashed/invalid validators' latest messages (T-10) so they add
  zero weight;
- **LCA** — depth-filter latest messages (`> top − LATEST_MESSAGE_MAX_DEPTH`) and take
  their lowest universal common ancestor as the scoring base;
- **score** — `score(b) = Σ { w(v,b) : b ⪯ lm(v) }`, the cumulative bonded weight of
  the validators whose latest message supports `b`, accumulated to the LCA;
- **rank** — descend into the maximum-score child at each level (GHOST), tie-broken by
  `(score desc, hash asc)`, to a fixpoint; the head is the main tip;
- **truncate** — keep ≤ `max_number_of_parents` tips (head preserved) and drop
  secondary parents deeper than `max_parent_depth`.

The main tip feeds the sealed/finalized-floor merge (`snapshot.rs`, proposer-side); the
validator only **bound-checks** declared parents (`validate.rs`), it does not recompute
fork choice.

### Verified call graph

| Step | Code |
|---|---|
| collect + T-10 filter | `estimator.rs:71-91` (`invalid_latest_messages_from_hashes` + `retain`) |
| LCA (+ 1000-depth filter) | `estimator.rs:96-97,172-201`; `dag_operations.rs` (`lowest_universal_common_ancestor_many`) |
| scores | `estimator.rs:203-276` (`build_scores_map`); `proto_util.rs:160-193` (`weight_from_validator_by_dag`) |
| rank | `estimator.rs:278-345` (`rank_forkchoices` + `replace_block_hash_with_children` + `still_same`); `list_ops.rs:44-67` (total tie-break) |
| depth + count cut | `estimator.rs:108-116,121-170` (`filter_deep_parents`, `take`) |
| consumers | `snapshot.rs:317-337` (main-parent, proposer — **two stages**: the ghost head `:317-323` + `(is_main DESC, hash ASC)` sort `:325-331`, then the deploy-support promotion `:332` → `:124-185`); `validate.rs:922` (bound check, **not** recompute) |

Key source files:

| Concern | File |
|---|---|
| Estimator (score, rank, LCA, bounds) | `casper/src/rust/estimator.rs` |
| Tie-break (total order) | `shared/src/rust/shared/list_ops.rs` |
| Per-validator weight | `casper/src/rust/util/proto_util.rs` |
| LCA | `casper/src/rust/util/dag_operations.rs` |
| Main-parent selection | `casper/src/rust/engine/multi_parent_casper/snapshot.rs` |
| Validator bound check | `casper/src/rust/validate.rs` |

---

## 2. The bug-hunt outcome — mostly refuted, three seams fixed

Front-loading the fork-suspects (as the finalized-floor did with H1/H2/H3), but
reporting the **honest** result: each prime suspect is **refuted with code evidence**.

- **S1 — tie-break non-determinism → fork (the #1 suspect): REFUTED.**
  `sort_by_with_decreasing_order` (`list_ops.rs:55-64`) is a **total** order — score
  descending, then `item_a.cmp(item_b)` (block-hash ascending, `BlockHash = Bytes`,
  lexicographic) — and `rank_forkchoices` deduplicates to **distinct** hashes before
  sorting (`estimator.rs:291-295`). So the sorted output (and thus the ranked head) is
  a pure function of the scored tip set; the `HashSet`/`HashMap` iteration order can
  never leak into it. Confirmed as a test (`tie_break_is_total_shuffle_invariant_on_distinct`)
  and axiom-free in Rocq (`TieBreak.sort_total_order` / `output_indep_of_input_perm`).
- **Weight non-determinism (the A9 `f32` analog): REFUTED.** Weights are exact **`i64`**
  read from the **main parent's on-chain `weight_map`** (`proto_util.rs:171-190`) —
  block-structural, identical across nodes, no floating point.
- **LCA determinism + the `LATEST_MESSAGE_MAX_DEPTH = 1000` cliff (the 256/512 analog):
  REFUTED as a fork.** Under an identical DAG the structural top height is identical, so
  the depth filter is a pure function of the DAG (deterministic). Its *effect* — keeping
  one stale validator from dragging the LCA to genesis — is intended and bounded; a
  message below the resulting LCA has zero ranking influence.
- **`rank_forkchoices` termination: HOLDS.** Each non-`still_same` step advances to a
  strictly higher-numbered child, bounded by the DAG height (`Rank.rank_terminates`).

**Three low-severity robustness seams** (each fixed full-stack in §3; each below the
convergence-test envelope, so a green gate would miss them):

| ID | Sev | Evidence | Finding |
|---|---|---|---|
| **B1** | Med | `proto_util.rs:168,176` `.expect(...)` | Panics on the fork-choice BFS when a traversed block or its main parent is momentarily absent (sync/prune window). |
| **B2** | Low | `estimator.rs:120-127` `take(.. as usize)`; `casper.rs:56` | The `-1` config "unlimited" sentinel reaches the estimator and only worked via `-1 as usize` wrap; two sentinels (`-1`, `i32::MAX`) silently conflated. |
| **B3** | Low | `estimator.rs:267-268` (now `checked_add`; was `+=`) | Unchecked score accumulation; wraps only above `i64::MAX` (a supply-cap violation). |
| **B4** | — | `estimator.rs:144-150` | Already hardened (P2-8 typed `Err` on empty tips); modeled, not re-fixed. |

---

## 3. The fixes (Phase 2)

- **B1 — typed error, not panic.** `weight_from_validator_by_dag` now returns
  `Err(KvStoreError::KeyNotFound(..))` (mirroring `snapshot.rs`'s sibling case) when a
  block or its main parent is absent, and propagates it. Regression:
  `weight_from_validator_missing_parent_is_typed_err` (flipped from the Phase-0
  `#[should_panic]` repro).
- **B2 — cast-safe, explicit sentinels.** `tips_with_latest_messages` treats **both**
  "unlimited" sentinels explicitly (`self.max_number_of_parents < 0`, and `== i32::MAX`)
  → take all; a genuine positive cap truncates. Behaviour-preserving; removes the
  two's-complement reliance.
- **B3 — checked accumulation.** `build_scores_map` uses `checked_add` → typed `Err` on
  overflow (only reachable on a supply-cap violation).

Verified: `cargo check -p casper --all-targets` clean; fork-choice bisim (5) + uc_16
slashed-parent + `three_writers_converge_under_load` all green (B2/B3 behaviour-
preserving on the propose hot path).

---

## 4. Invariant catalog → artifact map

Every catalog item maps to a concrete artifact — no "assumed"/"prose-only" row.

| ID | Property | Mechanized / checked in |
|---|---|---|
| **T-DET** | same `(DAG, latest_messages)` ⟹ identical `(tips, lca, main_parent)` (S1) | Rocq `MainTheorem.fork_choice_determinism_correct`; TLA⁺ `ForkChoice.Inv_Deterministic`; Rust `fork_choice_determinism_correct` (example: the flipping DAG returns `[b8, b7]` for every latest-message map order) + `fork_choice_determinism_over_subsets` (proptest: identical tips across rebuilds of any latest-message sub-relation) (`prop_estimator_determinism.rs`) |
| **T-TOTAL** | tie-break is a total order on distinct hashes ⟹ unique argmax | Rocq `TieBreak.sort_total_order`, `output_indep_of_input_perm`; Z3 `tiebreak_total_order.py`; Rust `tie_break_is_total_shuffle_invariant_on_distinct` |
| **T-SCORE** | score accumulation additive/order-independent | Rocq `Score.score_perm_invariant`, `score_eq_support_sum`; Z3 `score_supply_cap_bitvec.py`; Sage `forkchoice_algebra.sage`; Rust `score_perm_invariant` (proptest: permuting the latest-message order never changes the tips — the score monoid's only observable consequence, `prop_estimator_determinism.rs`) |
| **weight purity** | weights are block-structural `i64` (no `f32`, no node-local view) | Rocq `Score.weight_is_pure` + `GuardBridge.weight_block_structural` |
| **T-FILTER** | slashed/invalid latest messages add zero weight (S2) | Rocq `Filter.invalid_excluded` (cites slashing `ForkChoice.v`); Rust `filter_t10_invalid_latest_message_excluded` (the real `Estimator` + a DAG-flagged invalid latest message: excluded ⇒ tips identical to omitting it, `prop_estimator_determinism.rs`) |
| **T-GHOST** | ranking returns the heaviest-subtree leaf | Rocq `Rank.rank_selects_heaviest`, `rank_head_is_argmax`; Wolfram `ghost_heaviest_subtree.wl`; Rust **proptest** `ghost_ranked_tips_are_heaviest_subtree_argmax` (`prop_ghost_argmax.rs`: on RANDOM weighted fork DAGs the real `Estimator`'s ranked tips equal the branches ordered heaviest-subtree-first `(score DESC, hash ASC)`, led by the GHOST argmax, checked against an independent stake-sum oracle) |
| **T-TERM** | `rank_forkchoices` terminates | Rocq `Rank.rank_terminates`; Wolfram (measure monotone) |
| **T-LCA** | LCA is a common ancestor (modeled from the fold; termination proved); depth filter deterministic (S6) | Rocq `Lca.lca_is_common_ancestor` (+ `lcua_many_common_ancestor`, `reduce_converges`, `lca_is_lowest` — §6.1), `lca_depth_filter_deterministic`, `lca_empty_is_genesis`; TLA⁺ `ForkChoiceScan.Inv_LcaDeterministic`; Rust proptests `reduce_converges`, `lca_is_common_ancestor`, `lcua_many_is_max`, `lca_single_and_genesis_boundary` (`prop_lca.rs`: the real `DagOperations::lowest_universal_common_ancestor_many` on random DAGs converges, is a common ancestor of every input, and is the lowest such on trees) |
| **T-BOUND** | count/depth/`i32::MAX` truncations keep the head, never panic (S3/S4) | Rocq `Bound.head_preserved`, `take_never_drops_head`, `empty_tips_typed_err`; Rust `filter_deep_parents` tests |
| **B1** | missing metadata ⟹ typed error, not panic (S4) | Rust `weight_from_validator_missing_parent_is_typed_err`; bridged by `GuardBridge.weight_block_structural` |
| **B3** | score overflow ⟹ typed error, not wrap (S5) | Rust (`checked_add`); Z3 `score_supply_cap_bitvec.py` |
| **T-MP** | main-parent selection is a **deterministic pure function** of `(dag, parents, last_finalized_block)` — **not** the GHOST argmax (see §6.2) | Rocq `GuardBridge.main_parent_pipeline_deterministic` (+ `main_parent_pipeline_permutation`, `dbetter_strict_total_order`, `dbest_hash_perm_invariant`); the old "= ghost head" claim is **refuted by computation** in `GuardBridge.pipeline_head_may_differ_from_ghost`. Rust: `main_parent_is_ghost_head_deterministic` (`prop_ghost_argmax.rs`: `tips[0]` is the heaviest branch, stable across every latest-message map order — this is the ESTIMATOR, still true) + `deploy_support_*` proptests in `snapshot.rs`'s in-module `mod tests` (the real `prefer_deploy_support_main_parent`: permutation-preserving, unique-argmax, strict-total-order) |
| **T-VALID (reframed)** | honest proposer's parents pass `Validate::parents` (not a recompute) | Rocq `GuardBridge.honest_forkchoice_parents_validate`, derived against the real predicate — `parents` (`validate.rs:945`), which bound-checks count/depth/progress and never recomputes the estimator (§6) |
| **T-WF (bridge)** | block validation ⟹ acyclic, height-monotone DAG (the premise the proofs derive) | Rocq `GuardBridge.validation_implies_wf_dag` |
| **T-ROOT (bridge)** | block validation ⟹ single-rooted DAG (exactly one parentless block = the approved genesis); `single_root` DERIVED, not assumed | Rocq `GuardBridge.validation_implies_single_root` (models `justification_follows` rejecting every other parentless block as `InvalidParents` — `validate.rs:1159-1162`; genesis admitted via the signed approved-block path `Validate::approved_block`, `initializing.rs:441`) |
| **capstone** | all of the above, axiom-free | Rocq `MainTheorem.fork_choice_{determinism,ghost,bound,bridge}_correct` |

---

## 5. Formal artifacts

### 5.1 Rocq (`formal/rocq/fork_choice/`) — axiom-free

Rocq/Coq 9.1.1, Stdlib-only. Every headline result is checked with `Print Assumptions`
⇒ *"Closed under the global context"* (no `Axiom`, `Parameter`, or `Admitted`). A
`Recovery`-analog is **deliberately omitted** — fork choice is a stateless re-derivation
each round (no effect-application/idempotence concern).

| Module | Depends on | Key results |
|---|---|---|
| `Foundation.v` | — | DAG, block numbers, parents/children, ancestry, `wf_dag`, height bound |
| `Score.v` | Foundation | `weight_is_pure`, `score_perm_invariant`, `score_eq_support_sum` |
| `Filter.v` | Foundation | `invalid_excluded`, `valid_preserved`, `filter_idempotent` (T-10 reuse) |
| `TieBreak.v` | Foundation | **S1 proof**: `sort_total_order`, `output_indep_of_input_perm`, `sort_argmax_unique`, `sort_is_permutation` |
| `Lca.v` | Foundation | `lcua_many` fold + `reduce_converges` (lex-measure termination); `lca_is_common_ancestor` (from the fold, no circular premise), `lca_is_lowest`, `lca_depth_filter_deterministic`, `lca_empty_is_genesis` |
| `Rank.v` | Foundation, Score, TieBreak | `rank_terminates`, `rank_selects_heaviest`, `still_same_fixpoint` |
| `Bound.v` | Foundation, Rank | `head_preserved`, `take_never_drops_head`, `cast_usize_safe`, `empty_tips_typed_err` |
| `GuardBridge.v` | Foundation, Rank, Bound, Filter, Lca | the Rust-enforced seams: `validation_implies_wf_dag`, `validation_implies_single_root` (approved-genesis pin ⟹ `single_root`, DERIVED), `weight_block_structural`, `honest_forkchoice_parents_validate`; the **two-stage** main-parent pipeline (§6.2) — `ghost_sort_first_deterministic` (stage 1; formerly `main_parent_first_deterministic`, kept as a deprecated alias), `dbetter_strict_total_order`, `dbest_hash_perm_invariant`, `main_parent_pipeline_{permutation,deterministic}`, `pipeline_head_may_differ_from_ghost`; and `lca_is_common_ancestor_validated` (the capstone-facing LCA property with `single_root` DERIVED) |
| `MainTheorem.v` | all | capstones `fork_choice_{determinism,ghost,bound,bridge}_correct` |

Build (memory-capped, 32 GB envelope):

```bash
cd formal/rocq/fork_choice && coq_makefile -f _CoqProject -o Makefile
systemd-run --user --scope -p MemoryMax=16G -p CPUQuota=1800% -p TasksMax=200 \
  make -C formal/rocq/fork_choice -j1
```

### 5.2 TLA⁺ (`formal/tlaplus/fork_choice/`)

- **`ForkChoice.tla`** — determinism + heaviest-subtree, with the `TotalTieBreak` knob.
  `MC_ForkChoice.cfg` (`TotalTieBreak = TRUE`, the code) **passes** (`Inv_Deterministic`,
  `Inv_HeaviestSubtree`); `MC_ForkChoice_nontotal.cfg` (`TotalTieBreak = FALSE`, the
  score-only fork) **reproduces** `Inv_Deterministic is violated` — the exact fork the
  total tie-break prevents.
- **`ForkChoiceScan.tla`** — the LCA depth-filter layer, with the `NodeLocalTop` knob.
  `MC_ForkChoiceScan.cfg` (`NodeLocalTop = 0`, structural top) **passes**
  (`Inv_LcaDeterministic`); `MC_ForkChoiceScan_bug.cfg` (`NodeLocalTop = 1`, node-local
  top) **reproduces** `Inv_LcaDeterministic is violated`.

Run under the bounded envelope (never tmpfs/`auto`):

```bash
source scripts/lib/tlc-run.sh
FC=formal/tlaplus/fork_choice
tlc_run "$(tlc_metadir fc_det)"  "$FC/MC_ForkChoice.cfg"          "$FC/ForkChoice.tla"       # PASS
tlc_run "$(tlc_metadir fc_nt)"   "$FC/MC_ForkChoice_nontotal.cfg" "$FC/ForkChoice.tla"       # counterexample
```

### 5.3 Z3 / Sage / Wolfram cross-witnesses

- **Z3** `formal/z3/fork_choice/tiebreak_total_order.py` — the `(score desc, hash asc)`
  relation is irreflexive/asymmetric/transitive/total on distinct hashes, argmax unique
  (the S1 witness). `score_supply_cap_bitvec.py` — BitVec-64 score add is assoc/comm; no
  overflow while every prefix sum ≤ cap; the wrap exists above the cap (motivating B3).
- **Sage** `formal/sage/fork_choice/forkchoice_algebra.sage` — score commutative monoid
  (permutation identity), tie-break order-embedding key strictly monotone ⇒ total order
  ⇒ unique argmax. Prints `ALL PASS`.
- **Wolfram** `formal/wolfram/fork_choice/ghost_heaviest_subtree.wl` — greedy heaviest-
  child descent reaches the heaviest-subtree leaf; the descent measure is strictly
  monotone (termination); the `LATEST_MESSAGE_MAX_DEPTH` cap bounds the scored band
  deterministically. Validated via the licensed Wolfram MCP evaluator (CLI license-bind
  is skip-tolerant).

### 5.4 Rust tests

The property-based fork-choice suite lives in `casper/tests/fork_choice/`, wired into the
gate's `[8/8]` Rust tier (each test carries its Rocq citation in a doc-comment):

- **`prop_filter_deep_parents`** — the concrete `Estimator::filter_deep_parents` conforms to
  `GuardBridge.within_depth` / `prop_filter`: every retained secondary parent is within depth
  (soundness), the main parent is retained first, nothing within depth is dropped
  (completeness), and the retained set equals `{main} ∪ prop_filter(secondaries)`.
- **`prop_estimator_determinism`** — `Estimator::tips_with_latest_messages` is invariant under
  permuted latest-message input / `HashMap` seeds (`MainTheorem.fork_choice_determinism`); the
  `build_scores_map` score monoid is permutation-invariant (`Score.score_perm_invariant`); and
  the T-10 invalid-latest-message `retain` excludes invalid tips (`Filter`).
- **`prop_lca`** — `lowest_universal_common_ancestor_many` over random DAGs: the fold converges
  (`Lca.reduce_converges`), the result is a common ancestor of every input
  (`Lca.lca_is_common_ancestor`), and no common ancestor is higher (`Lca.lcua_many_is_max`, over
  single-parent trees per the `Lca.v` §7 `single_parent_spine` residual).
- **`prop_bound`** — the B2/B3/B4 seams on `tips_with_latest_messages`: `usize`-cast sentinels +
  positive caps (B2), typed `Err` on score overflow (B3) and empty ranked tips (B4), and `take`
  never drops the head.
- **Tie-break** — `shared` `list_ops::sort_by_with_decreasing_order` proptest
  (`TieBreak.sort_total_order`): permutation-invariant, output is a permutation of the input, and
  the argmax is unique — the S1 linchpin the estimator's ranking depends on.
- **`Validate::parents` depth horizon** — `casper/tests/batch2/validate_test.rs::
  parent_validation_enforces_max_parent_depth_horizon` is the **receive-side** realization of
  the C12 bridge (§6): with a finite `max_parent_depth`, an honest within-horizon parent
  accepts, a too-deep parent is `InvalidParents`, and `depth_buffer` extends the horizon —
  extending the abstract `honest_forkchoice_parents_validate` past `prop_filter` to the real
  validator predicate.

Plus `casper` `proto_util.rs` (`weight_from_validator_missing_parent_is_typed_err`), the
existing bisimulation suite (`casper/tests/slashing/{prop_t_13c_forkchoice_bisim, uc_16,
uc_17}`), and `three_writers_converge_under_load` exercise the estimator end-to-end.

---

## 6. The T-VALID reframe (design boundary, not a bug)

The intuitive property "a validator recomputes the same fork-choice the proposer used"
is **false to the code** and must not be asserted. `validate.rs` contains no estimator
recomputation; `Validate::parents` (`:945`) checks only parent count, depth, and
progress, and `snapshot.rs:315` states it: *"validators replay declared parents, not
fork-choice."* The `ghost_main_parent` is computed **proposer-side** only. The honest
bridge (the finalized-floor GuardBridge lesson: bridge the seam the Rust *enforces*, do
not assume a premise it doesn't) is therefore:

- `GuardBridge.honest_forkchoice_parents_validate` — an honest proposer's depth-filtered
  parent list satisfies the validator's acceptance predicate; plus
- observer-level **T-DET** — every node agrees on the canonical tips for a given DAG.

Consensus safety of parents is anchored by the finalized-floor committee/bonds
validation, not by re-running the estimator.

### 6.1 The LCA is modeled from the fold — RESOLVED

`Lca.lca_is_common_ancestor` now proves the fork-choice-relevant LCA property — the
computed LCA is a common ancestor of every depth-filtered latest message — from a
**concrete model** of the `DagOperations::lowest_universal_common_ancestor_many` fold
(`lcua_many`, a `BTreeSet`-faithful dedup fold), with **no** conditioning on "assume it
is a common ancestor". The fold's **termination is proved** (`reduce_converges`, via a
lexicographic `(max numof, count-at-max)` measure — not assumed), so the theorem is
**non-vacuous** on the wide multi-validator DAGs the LCUA-many is actually run on
(confirmed by `vm_compute` on a concrete DAG). Lowest-ness (`lca_is_lowest`) now
**derives** — no longer assumes — the LCA's maximality (`lcua_many_is_max`), and concludes
the strictly stronger `anc_of d c (lcua_many d ms)` (every common ancestor is *below* the
LUCA), on the faithful main-parent-tree model.

The circular `lcua_common` hypothesis and the fold-termination premise — the two things
that constituted the residual — are **gone**, and two further premises were **discharged**
in the P0–P3 FV sweep:

- **`common_ancestor … root` is now derived (C4).** `descends_from_root` proves, by
  well-founded induction on `numof`, that under `wf_dag` + `single_root` every real block
  descends from the genesis; `common_ancestor_root` lifts that to "genesis is a common
  ancestor of any real tip set". The premise is dropped from `lcua_many_common_ancestor`,
  `lca_is_common_ancestor`, and the `fork_choice_ghost_correct` capstone conjunct — all
  still discharged, now internally.
- **`lca_is_lowest` maximality is now derived (C2).** The old statement *took* the maximal-
  ity `(∀c', common_ancestor c' → numof c' ≤ numof (lcua_many …))` as a hypothesis — i.e.
  assumed the fold's survivor already was the LCA. It is now the **theorem** `lcua_many_is_max`,
  proved via a `below_all` fold invariant. **Correction (disclosed residual):** the earlier
  `spine_linear` premise is *provably insufficient* for maximality — a well-formed single-
  genesis DAG with a *straddling* old parent satisfies `spine_linear` yet makes the fold
  over-descend past the true common ancestor (making the old `lca_is_lowest` **vacuous**
  there). The derivation instead uses `single_parent_spine` (each block's `blk_parents` is
  exactly its main parent — the main-parent tree the LUCA descends) + `NoDup ms` (the
  deduplicated `BTreeSet` input); both are faithful and both are necessary (a duplicate or a
  multi-parent straddle each break lowest-ness). Non-vacuity is witnessed by an axiom-free
  `Example` on a concrete tree.

- **`single_root` is now derived, not assumed (T-ROOT).** It was formerly a bare premise of
  the `Lca` common-ancestor results *and* the `fork_choice_ghost_correct` capstone. It is now
  **DERIVED from block validation** by `GuardBridge.validation_implies_single_root`. The
  strengthened `validated_block` requires a parentless (main-parent = `None`) block's hash to
  equal the genesis hash — faithfully modeling `validate.rs::justification_follows`
  (:1135-1139), which runs *before* `Validate.parents` and rejects **every** empty-parents
  block as `InvalidParents`; the unique genesis is admitted out-of-band via the
  signature-authenticated approved-block path (`initializing.rs:832-840`, bypassing the
  pipeline). `MainTheorem.fork_choice_ghost_correct` clause (d) therefore takes the
  Rust-enforced validation predicate `(∀ b ∈ d, validated_block genesis_hash d b)` and
  discharges `single_root` internally via `lca_is_common_ancestor_validated`. `genesis_hash`
  is a Section `Variable` (a **parameter** of the closed term), never an axiom.

One FAITHFUL DAG-shape premise remains on the ghost capstone: `all_real` (the tips resolve
to real blocks). It is **necessary, not a deferral**: a DAG with two distinct parentless
blocks makes fully-unconditional common-ancestor *false* — the Rust `BTreeSet` fold exhibits
the same — and `single_root` (now derived) rules exactly that out. The internal `Lca` lemmas
(`lca_is_common_ancestor`, `lca_is_lowest`, `lcua_many_is_max`, `descends_from_root`,
`common_ancestor_root`) still *state* `single_root` as a premise, but it is now
**dischargeable** from validation via the bridge (and is so discharged at the capstone). No
lemma is `Admitted` or axiomatized; the one added hypothesis (`wf_dag` on `reduce_converges`)
is mathematically required. Rocq assumes exactly what the DAG / Rust guarantees, as premises,
never axioms — every headline result is `Print Assumptions` Closed.

> **Recommended Rust hardening (FV finding).** `validate.rs::block_number` (:698-721) only
> forces `num == 0` for a parentless block; it does **not** assert that block's hash equals
> the approved-genesis hash. The single-root guarantee currently rests on
> `justification_follows` (:1135-1139) rejecting every *other* parentless block *before*
> `Validate.parents`, plus genesis entering via the signed approved-block path
> (`initializing.rs:832-840`). An explicit approved-genesis-hash assertion for parentless
> blocks in `block_number` would make the pin local and defense-in-depth; the FV models the
> `justification_follows` rejection that already exists, so the proof is faithful to shipped
> behavior.

---

### 6.2 The main parent is **not** the GHOST head — REMODELED

A second design boundary of the same shape as §6's T-VALID reframe: the model claimed
something **stronger than the code does**, and the fix is to weaken the model to the true
statement — not to weaken the code.

**The drift.** Seam (3) formerly read *"`snapshot.rs` (:136-142) orders parents by
(is_main DESC, hash ASC); that is a TOTAL order, so the ordering is deterministic"*, and
modeled the main parent as the **GHOST head**. Both the line citation and the claim are
now stale: `snapshot.rs` builds the parent list in **two** stages.

| Stage | Code | What it does |
|---|---|---|
| 1a | `snapshot.rs:317-323` | `ghost_main_parent = estimator.tips_with_latest_messages(..).tips.into_iter().next()` — the GHOST head |
| 1b | `snapshot.rs:429-435` | `sort_by` on `(is_main DESC, hash ASC)` — **this is the sort the model cited**, now at `:429-435`, not `:136-142` |
| 2 | `snapshot.rs:436` → `:128-189` | `prefer_deploy_support_main_parent` scores each parent **branch** by unfinalized user-deploy support and, if a best exists, `remove(best_idx)` + `insert(0, _)` (`:177-178`) — **promoting it over the ghost head** |

So *"the block's main parent = the GHOST argmax"* is **false for the proposer**. This is
not an opinion: `GuardBridge.pipeline_head_may_differ_from_ghost` **refutes it by
computation** (ghost head `0`, parents `{0,1}`, branch `1` carrying a deploy ⟹ the
pipeline yields `[1;0]`, main parent `1 ≠ 0`).

**What is *not* affected.** `Rank.rank_selects_heaviest` / `rank_head_is_argmax` /
**T-GHOST** are untouched — the `Estimator` still returns the heaviest subtree. Only the
**consumer** of `tips[0]` re-orders afterwards.

**The true (weaker) statement, now proved axiom-free.** The main parent is a
**deterministic pure function of `(dag, parents, last_finalized_block)`**:

| Claim | Theorem | Content |
|---|---|---|
| (a) permutation | `main_parent_pipeline_permutation` (via `promote_permutation`) | `remove(i)` + `insert(0,_)` is a permutation, so the parent **multiset** is preserved — no parent lost or duplicated |
| (b) strict total order | `dbetter_strict_total_order` | `better_deploy_branch_score` (`:48-70`) is irreflexive, asymmetric, transitive, and total on distinct hashes — lex on `(deploy_sig_count, latest_deploy_block_number, root_block_number)` then **hash ASC** (the `:68` comparison is reversed, so the *smaller* hash wins) |
| (c) determinism | `dbest_hash_perm_invariant`, `main_parent_pipeline_deterministic` | from (b) the argmax is **unique**, so *which* branch is promoted is scan-order independent; and the whole pipeline output is invariant under input permutation |
| (d) soundness | documented link to `finalized_floor/theories/Selection.v:196` `T_PS` | `T_PS` proves floor safety for an **unconstrained parent oracle** (`∀ parents`), so a reordered list is already inside the modeled domain; with (a) the promotion changes only the **order**, never the **set** |

> **Why the composition is load-bearing.** Stage 2 **alone is not** permutation-invariant:
> when *no* branch scores it returns `parents` **unchanged** (`:163-165`), so its head is
> then simply whatever came first. Determinism rests on stage 1 **canonicalizing** the list
> before stage 2 runs. That is precisely why the model composes the two stages rather than
> asserting stage 2 is order-invariant on its own — and why the Rust proptests below
> assert argmax-invariance **conditioned on a scored branch existing**, plus whole-pipeline
> invariance for the composed pipeline.

**Had (b) failed**, the promotion would have been a genuine consensus non-determinism
bug (two honest proposers scanning the same parents in different `HashSet` orders could
promote different branches). It holds, so this is a **model-update** obligation, not a
safety regression.

**Naming.** The old theorem is still true **of stage 1 alone** and is retained, renamed
`ghost_sort_first_deterministic`; the former name `main_parent_first_deterministic` is
kept as a **deprecated alias** so `MainTheorem.fork_choice_bridge_correct` clause (c)
compiles unchanged. Recommended follow-up: retarget `MainTheorem.v:36/:184` at
`ghost_sort_first_deterministic` and add `main_parent_pipeline_deterministic` as a fifth
bridge clause.

---

## 7. Verification status

Run the whole suite with `scripts/check-fork-choice-ALL.sh` (Rocq authoritative;
TLA⁺/Z3/Sage/Wolfram fail-soft; PlantUML render check). Target result: **ALL GATES OK**.

| Layer | Result |
|---|---|
| Rust build | `cargo check -p casper --all-targets` clean |
| Rust unit/regression | tie-break totality, B1 typed-error, fork-choice bisim (5), uc_16, convergence — all pass |
| Rocq | full dev builds `-j1`; **13 headline results axiom-free** — 4 capstones + `validation_implies_wf_dag`, `validation_implies_single_root` (approved-genesis pin ⟹ `single_root`, T-ROOT), `honest_forkchoice_parents_validate`, `sort_total_order`, `reduce_converges`, `lca_is_lowest`, and the P0–P3 derivations `lcua_many_is_max` (C2), `descends_from_root` + `common_ancestor_root` (C4) |
| Rocq kernel (coqchk) | **independent kernel re-check** of `ForkChoice.MainTheorem` + all deps ⇒ "Modules were successfully checked" (C3 — the trust root under the `Print Assumptions` claim) |
| TLA⁺ | `MC_ForkChoice.cfg` + `MC_ForkChoiceScan.cfg` pass; both bug cfgs reproduce their counterexample |
| Apalache (unbounded) | **`IndInv = TypeOK ∧ Inv_Deterministic ∧ Inv_HeaviestSubtree` proved INDUCTIVE** (BASE `Init ⊨ IndInv` + STEP `Next` preserves `IndInv`) on `ForkChoice_apalache.tla` — over **all of ℤ scores** (native SMT `Int`, strictly beyond TLC's `MaxScore=2`); non-vacuous (`TotalTieBreak=FALSE` ⇒ STEP CTI = the S1 fork). Horizon-free: holds on every reachable state at any trajectory length (C9). Fail-soft. |
| Rust proptest | **C12** proposer-side `prop_filter_deep_parents` (4/4: `Estimator::filter_deep_parents` ⊨ `GuardBridge.within_depth`/`prop_filter` — soundness + main-parent-retention + completeness + `retained == {main} ∪ prop_filter(secondaries)`) **+ receive-side** `Validate::parents` depth-horizon (accept within / reject beyond / buffer extends); `prop_estimator_determinism` (permutation-invariant tips + score-monoid + T-10 filter); `prop_ghost_argmax` (**T-GHOST** ranked tips = heaviest-subtree argmax on random weighted forks + **T-MP** main-parent `tips[0]` deterministic + heaviest); `prop_lca` (LUCA converges / common-ancestor / maximal); `prop_bound` (B2/B3/B4 sentinel/overflow/empty seams); tie-break `sort_by_with_decreasing_order` (perm-invariant + permutation + argmax-unique) — all pass |
| Z3 | tie-break total order (5/5) + score supply-cap BitVec (4/4) |
| Sage | fork-choice algebra ⇒ `ALL PASS` |
| Wolfram | GHOST heaviest-subtree / termination / LCA-bound — via the licensed MCP evaluator |
| Diagrams | 6 PlantUML diagrams render clean (populated SVG, no stderr) |

**Coverage matrix (§4).** Every catalog item maps to a concrete Rocq/TLA⁺/Z3/Sage/Wolfram
artifact or a Rust test — no "prose-only" row. The DAG well-formedness the proofs rest on is
*derived* from block validation (`GuardBridge.validation_implies_wf_dag`), the LCA
common-ancestor property is *modeled from the fold* with its termination *proved*
(`reduce_converges`), the genesis-common-ancestor fact is *derived* (`common_ancestor_root`,
C4) rather than assumed, the LCA's maximality is *derived* (`lcua_many_is_max`, C2)
rather than taken as a premise (§6.1), and **`single_root` is now *derived* from block
validation** (`GuardBridge.validation_implies_single_root`, T-ROOT) rather than assumed —
the ghost capstone takes the Rust-enforced `validated_block` predicate and discharges
single-rootedness internally (modeling `validate.rs::justification_follows`). The one
remaining `all_real` premise (and `single_parent_spine` + `NoDup ms` on the lowest-ness
results — the faithful, sufficient replacement for the provably-insufficient `spine_linear`)
are counterexample-necessary DAG-shape bridges — Rocq assumes exactly what the DAG
guarantees, as premises, never axioms (every headline result is `Print Assumptions` Closed,
and the whole library is re-checked by `coqchk`) — the finalized-floor GuardBridge
discipline.

**Policy:** all of the above run **locally**. Do **not** add any Rocq / TLA⁺ / Z3 / Sage
/ Wolfram step to `.github/workflows/*` (an earlier formal-CI workflow was deliberately
removed).

---

## 8. Diagrams

Six PlantUML diagrams (sources + rendered SVGs in [`diagrams/`](./diagrams/); render with
`plantuml -tsvg`, checked by `scripts/check-fork-choice-ALL.sh` step **[6/6]**). Each is
fully coloured with an in-diagram legend.

### 8.1 Component correspondence — spec ↔ Rocq ↔ TLA⁺ ↔ Z3/Sage ↔ Rust

[![Diagram 1 — every fork-choice component (ranking/tie-break, scoring, T-10 filter, LCA, ranking fixpoint, bounds, main-parent/validation bridge) annotated with its spec concern, Rocq module, TLA⁺ model, Z3/Sage/Wolfram witness, and Rust file, with the axiom-free MainTheorem capstone on top](./diagrams/01-component-correspondence.svg)](./diagrams/01-component-correspondence.svg)

### 8.2 The `Estimator::tips` pipeline (deterministic fork-choice)

[![Diagram 2 — sequence: collect latest messages → T-10 filter → LCA (with the 1000-depth filter) → build_scores_map → rank_forkchoices (total tie-break) → filter_deep_parents → take, each a pure function of the DAG](./diagrams/02-seq-tips-pipeline.svg)](./diagrams/02-seq-tips-pipeline.svg)

### 8.3 GHOST heaviest-subtree selection

[![Diagram 3 — a worked weighted DAG: subtree-score accumulation and the per-level heaviest-child descent 0→1→3 to the heaviest-subtree leaf](./diagrams/03-ghost-heaviest-subtree.svg)](./diagrams/03-ghost-heaviest-subtree.svg)

### 8.4 Tie-break totality — why fork-choice cannot fork (S1)

[![Diagram 4 — the total order (score desc, hash asc) makes the argmax unique so iteration order is washed out, versus the score-only bug where equal-score tips leave a non-deterministic choice (the TLA⁺ counterexample)](./diagrams/04-tiebreak-total-order.svg)](./diagrams/04-tiebreak-total-order.svg)

### 8.5 Fork-choice evaluation flow + the hardened seams

[![Diagram 5 — activity: collect → filter → LCA → build_scores (checked_add, B3) → rank → empty?→typed Err (B4) / else filter_deep + cast-safe parent cap (B2) → ForkChoice tips](./diagrams/05-activity-estimator-flow.svg)](./diagrams/05-activity-estimator-flow.svg)

### 8.6 `rank_forkchoices` fixpoint (termination)

[![Diagram 6 — state: expand to scored children, advancing one level (strictly-increasing block-number measure) until the still_same fixpoint; the no-scored-children arm keeps self; empty tips is a typed Err](./diagrams/06-state-rank-fixpoint.svg)](./diagrams/06-state-rank-fixpoint.svg)

---

## 9. References

DOIs are given where they exist and have been verified; whitepapers without a DOI carry
a stable identifier.

1. Y. Sompolinsky, A. Zohar. **Secure High-Rate Transaction Processing in Bitcoin
   (GHOST).** *Financial Cryptography and Data Security 2015.*
   DOI [10.1007/978-3-662-47854-7_32](https://doi.org/10.1007/978-3-662-47854-7_32).
   *(The greedy heaviest-observed-subtree rule the estimator specializes — §2, T-GHOST.)*
2. V. Buterin et al. **Combining GHOST and Casper (Gasper).** arXiv:2003.03052, 2020.
   (No DOI; stable id `arXiv:2003.03052`.) *(LMD-GHOST + finality — the fork-choice /
   finalized-floor split.)*
3. V. Buterin, V. Griffith. **Casper the Friendly Finality Gadget.** arXiv:1710.09437,
   2017. (No DOI; stable id `arXiv:1710.09437`.) *(Finality gadget layered over
   fork choice.)*
4. L. Lamport. **The Part-Time Parliament.** *ACM TOCS* 16(2), 1998.
   DOI [10.1145/279227.279229](https://doi.org/10.1145/279227.279229).
   *(Quorum agreement — the bonded-weight majority behind scoring.)*
5. M. Castro, B. Liskov. **Practical Byzantine Fault Tolerance and Proactive Recovery.**
   *ACM TOCS* 20(4), 2002.
   DOI [10.1145/571637.571640](https://doi.org/10.1145/571637.571640).
   *(BFT quorum intersection — why weighted fork choice is stable.)*
6. M. J. Fischer, N. A. Lynch, M. S. Paterson. **Impossibility of Distributed Consensus
   with One Faulty Process.** *J. ACM* 32(2), 1985.
   DOI [10.1145/3149.214121](https://doi.org/10.1145/3149.214121).
   *(Why fork-choice liveness/termination is proved, not assumed — T-TERM.)*
7. L. Lamport. **The Temporal Logic of Actions.** *ACM TOPLAS* 16(3), 1994.
   DOI [10.1145/177492.177726](https://doi.org/10.1145/177492.177726).
   *(TLA — the basis of `ForkChoice.tla` / `ForkChoiceScan.tla`.)*
8. Y. Bertot, P. Castéran. **Interactive Theorem Proving and Program Development:
   Coq'Art.** Springer, 2004.
   DOI [10.1007/978-3-662-07964-5](https://doi.org/10.1007/978-3-662-07964-5).
   *(The Coq/Rocq calculus in which the axiom-free capstones are mechanized, §5.1.)*
