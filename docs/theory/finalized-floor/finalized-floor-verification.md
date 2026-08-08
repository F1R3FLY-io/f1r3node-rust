# Finalized-Floor Multi-Parent Merge — Verification & Bug-Fix Dossier

> **Status:** the ~400-block "fails under load" bug is **found** (formally, three
> ways), **fixed** (full-stack), and the fix is **verified** by Rocq (axiom-free
> capstone), TLA⁺ (before/after model checking), Wolfram (structural instability),
> and an empirical 400+-block soak. Verification is **local-only** — no Rocq/TLA⁺/
> Wolfram step is wired into CI.

This document is written so the design and its verification can be reconstructed
from scratch. It records the feature, the bug's root cause, the fix, the formal
artifacts (with theorem anchors), the invariant catalog, the additional findings,
and exactly how to re-run every check.

> **Companion docs.** [`finalized-floor-specification.md`](./finalized-floor-specification.md)
> is the normative contract (the *what must hold*, RFC-2119);
> [`finalized-floor-glossary.md`](./finalized-floor-glossary.md) defines every
> symbol/term used here (before first use) and gives literate-programming pseudocode
> for the warm up-walk and the launder-free combine. Rendered PlantUML diagrams live
> in [`diagrams/`](./diagrams/) and are embedded in §8.

---

## 1. The feature

The **finalized-floor multi-parent merge** chooses the base a new block's merge
builds on. For a block `B` with parents `P₁…Pₖ`:

- **`floor(B)`** = the highest ancestor of `B`'s parents that the clique oracle
  certifies **finalized**, evaluated over `B`'s *frozen justification snapshot*
  `just(B)` (never a node-local live view). Every input is a signed/structural
  fact of `B`, so **every honest node derives the same floor** — this is the
  linear-finality analogue of RChain's per-message fringe.
- **merge base** = `floor(B).post_state`.
- **merge scope** = the unfinalized band `closure(parents) \ closure(floor)` — the
  blocks whose writes the merge must fold onto the base.

`floor(B)` is the maximum of two candidate sources (both pure functions of `B`):

1. **Inheritance** — each parent's own floor (a child's cut never drops below a
   parent's, so a race sealed at some cut is never re-litigated).
2. **Advancement** — per parent, the highest main-chain ancestor with
   `ft_witnessed ≥ θ` over `just(B)` (genesis is finalized by definition).

Key source files:

| Concern | File |
|---|---|
| Floor derivation, frontier walk | `casper/src/rust/finality/floor.rs` |
| Clique oracle (`ft_witnessed`) | `casper/src/rust/safety/clique_oracle.rs` |
| Merge driver, scope, backstop | `casper/src/rust/util/rholang/interpreter_util.rs` (`compute_parents_post_state`) |
| Merge write-algebra | `rspace++/src/rspace/merger/merging_logic.rs`, `casper/src/rust/merging/conflict_set_merger.rs` |
| Floor / frontier cache (LMDB) | `block-storage/src/rust/dag/block_dag_key_value_storage.rs` |

---

## 2. The bug — a silent merge-scope cliff driven by a quadratic floor walk

Two coupled defects. The first loses data; the second drives the system into the
first under load.

### H1 (SAFETY) — silent lossy fallback

`compute_parents_post_state` capped the merge with
`MAX_FLOOR_DISTANCE_BLOCKS = 256` and `MAX_PARENT_MERGE_SCOPE_BLOCKS = 512`. When
the finalization lag

```
Δ = num(maxParent) − num(floor)
```

exceeded the cap, it **silently returned the single highest-numbered parent's
post-state with empty rejected-sets** — discarding every other parent's committed
writes. Deterministic (no fork), but committed writes vanished (safety **S5** /
`¬T-K1` / `¬T-NDA`), and a dropped co-parent's deploys could be simultaneously
marked non-re-proposable → **permanently stranded**.

### H2 (DRIVER) — uncached Θ(Δ²·V) floor walk

`parent_frontier` re-walked the main chain **uncached** on every merge, running
the max-clique `ft_witnessed` oracle (each an O(V) per-validator ancestry BFS) at
**every** step — Θ(Δ·V) oracle calls per parent, Θ(Δ²·V) cumulatively. As Δ grew,
propose latency grew super-linearly → finalization lagged → Δ grew: a
**positive-feedback ratchet** that pushes Δ across the 256 cliff under genuine
load (concurrency + propagation delay). `DEEP_WALK_WARN_THRESHOLD = 256` only
warned.

### H3 (COMPOUND) — unbounded ancestor scan

The merge-scope ancestor collection was bounded by a shard config
(`max_parent_depth`) that **degenerated to an unbounded O(chain) scan** at its
default (`≤0` or `i32::MAX`), and could even cut *above* the floor (dropping the
band in between). It compounded H2's per-merge cost.

### Why the green-gate missed it

The convergence gate `three_writers_converge_under_load = run_convergence(3,3,21)`
is ≈ **35 blocks** — far below the 256 cliff. The 400-block observation is a
soak / shard run under real concurrency, where the ratchet has room to build.

### The ratchet, quantitatively

Model the finality lag as a difference equation `Δₙ₊₁ = f(Δₙ)` where a propose
step advances the tip and a finalize step advances the floor, and finalize
throughput falls as propose cost `∝ Δ²` rises. `formal/wolfram/finalized_floor/
delta_ratchet.wl` shows — parameter-free, over the reals — that with the buggy
Θ(Δ²) advance the feedback slope exceeds 1 at **every** equilibrium (unstable:
Δ runs away), whereas the fixed **O(1)** advance has zero feedback (Δ stable).

```
 Δ  ▲                              buggy: propose cost ∝ Δ²  → finalize starves
256 ┤· · · · · · · · · · · ·╱····  cliff (silent write-loss fires here)
    │                    ╱⟋   ← runaway (slope > 1 at every fixed point)
    │              ╱⟋⟋
    │        ╱⟋⟋
  k ┤─────────────────────────────  fixed: O(1) advance → Δ bounded (flat)
    └────────────────────────────▶ block height
```

---

## 3. The fix

### 3.1 H2 — persist a per-block frontier + incremental up-walk

Cache `F(X) := parent_frontier(X, just(X))` — the highest witnessed-finalized
block on `X`'s main spine over `X`'s **own** snapshot — in a new `frontier-index`
LMDB store mirroring `floor-index`. `F(X)` is a pure function of the block (it is
exactly the `parents[0]` advancement candidate `derive_floor` already computes),
so caching is free; `floor_of_block` persists it, `derive_floor` now returns
`(floor, F(B))`.

`parent_frontier(parent, J)` (where `J = just(child) ⊇ just(parent)`) becomes:

- **Warm** (`incremental_frontier`): read the cached pivot `F(parent)`; verify it
  still finalizes over the larger `J` (**L-SNAP** guard) and that the committee is
  constant across the band (**L-ANC** guard); then **up-walk** the spine from the
  pivot toward `parent`, advancing while each block stays finalized. The band is
  collected with cheap `main_parent` hops (no oracle calls); only the up-walk
  itself calls the oracle — `O(advance)` calls, **amortized O(1)** (advance sums
  telescope to the spine length).
- **Cold** (`cold_parent_frontier`): the original top-down walk (cache miss, guard
  trip, pivot off-spine, or genesis).

```
 spine (bottom→top):   genesis ── … ── F(parent) ── … ── parent
 flags over just(B):    true    true    TRUE(pivot)  ?…?     ?
 warm up-walk:                          └──advance──▶ stop at first non-final
 cold down-walk:        first finalized from the top ◀──────┘   (== warm, by L-ANC)
```

**Determinism linchpin (why the cache never forks):**

- **L-ANC** (ancestor-monotone): `Finalized(C,J) ∧ C' ancestor of C ⟹ Finalized(C',J)`.
  The *same* quorum that finalizes `C` finalizes every ancestor (each member has
  `C`, hence `C'`, in its past). ⟹ finalized blocks are a downward-closed prefix
  of the spine, so "highest finalized" is well-defined and the up-walk may stop at
  the first non-finalized block.
- **L-SNAP** (snapshot-monotone): `just(B) ⊇ just(P) ⟹ Finalized(C,just(B)) ≥
  Finalized(C,just(P))`. ⟹ the pivot stays valid over the child's larger snapshot.

Together: **warm-walk result == cold-walk result** (transparent cache).
Residual: a bonding event inside the band can break L-ANC's constant-committee
premise; the warm path detects this (committee comparison) and falls back to the
cold walk (`floor_incremental_guard_fallback` metric). This guard is **not merely a
runtime check** — Rocq `GuardBridge.chain_adj_AdjDC` proves that a *constant*
committee across the band **derives** `Floor.frontier_cache_transparent`'s `AdjDC`
premise from L-ANC, so the guard is exactly the condition under which warm == cold
(the "Rocq assumes what Rust enforces" seam is closed; tested by
`guard_trip_committee_change_falls_back_to_cold`).

### 3.2 H1 — deterministic backstop, never a silent lossy substitution

The over-cap path now returns `Err(CasperError…)` keyed on the **deterministic**
`floor_distance` Δ only. On **propose** the `Err` parks the round (retried once
finality advances and Δ shrinks); on **validate** an over-Δ block is
deterministically invalid — both sides compute the same Δ, so **no fork**. The
scope-size test `|visible_blocks| > 512` is **demoted to a metric** — it is *not*
node-deterministic (branch width differs across views), so it must never gate
admission. The lossy `put_cached_parents_post_state` was deleted. This also fixes
the `canonical_won_sigs` stranding.

### 3.3 H3 — floor-bounded ancestor scan

The floor is now derived **before** the ancestor scan, which is bounded at the
floor height (`meta.block_number ≥ floor_block_number`) — `O(Δ)`, and never cuts
above the floor.

### 3.4 Metrics

Renamed `MERGE_SCOPE_TOO_LARGE_FALLBACK_FIRED` → `MERGE_SCOPE_BACKSTOP_ERROR`;
added `floor_distance`, `merge_scope_size`, `floor_walk_oracle_calls`,
`floor_frontier_advance`, frontier `cache_hit`/`cache_miss`, and
`floor_incremental_guard_fallback`.

Net effect: per-merge cost **Θ(Δ²·V) → amortized O(1) oracle + O(Δ) cheap reads**;
the ratchet collapses and over-cap is safe.

---

## 4. Invariant catalog → artifact map

| ID | Property | Mechanized / checked in |
|---|---|---|
| **T-TERM** | spine walk terminates | Rocq `Foundation.spine_walk_terminates` |
| **T-MONO / L-ANC** | ancestor-monotone finalization (no floor regress, S2) | Rocq `CliqueOracle.L_ANC`, `L_ANC_mainparent` |
| **L-SNAP** | snapshot-monotone finalization | Rocq `CliqueOracle.L_SNAP`, `L_ANC_SNAP` |
| **C1 — θ-exact refinement** | the node's REAL θ-decision (`ft_exact_ge`, not the strict-majority θ=0 proxy) is ancestor- and snapshot-monotone, and every θ-finalized block (θ ∈ (0,1), positive stake) is strict-majority `Finalized` — so T-CACHE's no-fork rests on the test the node runs | Rocq `CliqueOracle.L_ANC_ft`, `L_SNAP_ft`, `L_ANC_SNAP_ft`, **`Finalized_ft_refines_Finalized`** (side-conditions `0<num`, `0<cweight c` disclosed + necessary — VACUOUS at θ≤0, see C1′), `is_quorum_ft_mono_weight`/`Finalized_ft_enlarge` (via `FtExact.ft_exact_mono_q`); capstone conjuncts of `finalized_floor_thetaexact_advance_correct` |
| **C1′ — θ≤0 coverage (hard gate)** | the num>0 refinement is VACUOUS at the DEFAULT θ=0 and the negative-θ sentinels; the node's REAL decision ALSO applies a θ-INDEPENDENT hard majority gate (`2·agreeing > S`, clique_oracle.rs:79-81), which ALONE yields strict-majority `Finalized` for **ALL** num, and T-CACHE holds directly over `Finalized_ft` for all num via `L_ANC_ft` — so θ≤0 is covered both ways | Rocq `CliqueOracle.hard_gate`, `hard_gate_iff_Finalized`, `Finalized_ft_hg`, **`Finalized_ft_hg_refines_Finalized`** (ALL num — no `0<num`, no positive-stake side-condition), `L_ANC_ft_hg`/`L_SNAP_ft_hg`; **`GuardBridge.BridgeFt.guard_constant_committee_transparent_ft`** + `upgo_finalized_ft` (θ-exact cache transparency, all num); Z3 `ft_exact_no_overflow.py` (the θ≤0 GAP `sat` + hard-gate closure `unsat`); capstone conjunct C1′ of `finalized_floor_thetaexact_advance_correct` |
| **C5 — snapshot advancement** | growth modeled as latest-message ADVANCEMENT (each binding → a DAG-descendant), not just preservation; L-SNAP holds for it, and preservation ⇒ advancement so the old L-SNAP is subsumed | Rocq `CliqueOracle.snap_advances`, `agrees_snap_advance_mono`, **`L_SNAP_advance`**, `L_ANC_SNAP_advance`, `L_SNAP_advance_ft`, `snap_extends_snap_advances`, `L_SNAP_of_extends` (original L-SNAP re-derived) |
| **T-CACHE** | warm up-walk == cold walk (no fork from cache, S1) | Rocq `Floor.frontier_cache_transparent` (takes `AdjDC`) **+ `GuardBridge.chain_adj_AdjDC` / `guard_constant_committee_transparent`** — the committee-constancy guard *derives* `AdjDC` from L-ANC, so the seam is bridged, not assumed; Rust test `guard_trip_committee_change_falls_back_to_cold` |
| **T-DETMERGE / T-CONV** | merge order-independent (no fork, S6) | Rocq `Merge.merge_or_perm`, `merge_max_perm`; Rust proptests `bitmask_or_is_commutative`, `integer_add_is_commutative` (`rspace++/…/merging_logic.rs` — the fold operands commute ⇒ order-independent) + `multiple_branches_should_merge_number_channels` (`casper/tests/merging/merge_number_channel_spec.rs`, concurrent IntegerAdd branches merge deterministically) |
| **T-K1** | no mergeable write lost (the 400-block loss, S5) | Rocq `Merge.merge_or_no_lost_bit`, `merge_absorbs`; Rust proptests `bitmask_or_dominates_each_input` (the BitmaskOr result carries every bit set in either input — no mergeable bit dropped) + `bitmask_or_is_idempotent` (`rspace++/…/merging_logic.rs`) |
| **T-NDA** | recovery not double-applied | Rocq `Recovery.apply_idem`, `no_double_apply`; Rust test `recovery_effect_is_applied_at_most_once` (`casper/tests/finalized_floor/recovery_no_double_apply.rs`) — drives the production `interpreter_util::canonical_won_sigs` recovery record: an effect is not canonically-won before it is applied, is won exactly once after a block includes it, and the recovery filter `apply(apply(s)) == apply(s)` (drops the won effect, never re-proposes it) |
| **S5 / Inv_NoLostParentWrite** | over-Δ never drops a parent write | TLA⁺ `SpecFixed` (holds); `Spec` (violated) |
| **Δ bound (driver)** | floor distance stays ≤ cap | TLA⁺ `Inv_DeltaWithinCap` |
| **L3/L5 liveness** | chain still progresses despite the backstop | TLA⁺ `Liveness_Progress` |
| **ratchet instability** | buggy advance is structurally unstable | Wolfram `delta_ratchet.wl` |
| **T-SOUND** | chosen floor is a sound base; `None` ⇒ Err correct (S4) | Rocq `Selection.select_sound`, `select_none_correct` |
| **T-LIN** | a Case-A base is a common DAG-ancestor (one chain) | Rocq `Selection.case_a_common_ancestor`; Rust test `derive_floor_case_a_floor_is_common_ancestor_of_all_parents` (the Case-A floor is `is_dag_ancestor` of every parent) |
| **T-FIN** | the chosen floor is finalized | Rocq **`GuardBridge.upgo_finalized`** (the warm up-walk's result is `Finalized` — discharges the premise unconditionally) + `Selection.select_finalized` (a floor drawn from finalized candidates is finalized); Rust test `derive_floor_result_is_finalized_over_justifications` (the result clears `CliqueOracle::ft_witnessed_exact` over the justification snapshot) |
| **T-PS** | safety for ANY parent list (unconstrained oracle) | Rocq `Selection.T_PS`; TLA⁺ `FinalizedFloorScan` (nondeterministic parent set); Rust test `derive_floor_incompatible_fork_errors` |
| **T-COMM** | committee = `bonds_of(floor)`, a pure fn of the floor (S8) | Rocq `Selection.committee_is_floor_bonds` |
| **Case-B** | Case-A fails but every other candidate is compatible ⇒ the dominating finalized tip is a sound base | Rocq `Selection.case_b_compatible`; Rust test `derive_floor_case_b_selects_dominating_finalized_tip` |
| **maximality / T-DET** | the chosen floor is the HIGHEST sound candidate, a pure function of (parents, candidates) ⇒ every node picks the same floor | Rocq `Selection.select_highest_sound`; Rust example `derive_floor_selects_highest_sound_finalized_candidate` (inheritance + advancement: lagging inherited cuts `{g@0, t@1}` lose to the newly-finalized higher candidate `c@2`) + Rust **proptest** `derive_floor_selects_highest_sound_candidate_over_chain` (`finality::floor` lib tests: on a RANDOM single-validator chain, the candidate multiset `{inherited b_i} ∪ {frontier b_k}` is all Case-A sound and `derive_floor` returns its block-number MAXIMUM — inheritance or advancement, whichever is higher) |
| **H3 coverage** | floor-bounded scan drops no parent write ≥ floor | Rocq `Selection.scope_covers_band`; TLA⁺ `FinalizedFloorScan` (`.cfg` PASS, `_bug.cfg` counterexample) |
| **T-ALG (semilattice)** | BitmaskOr / keep-one fold laws | Rocq `Merge` (`Nat.lor` / `Nat.max`); Rust proptests `bitmask_or_is_associative`, `bitmask_or_is_commutative`, `bitmask_or_is_idempotent` (`rspace++/…/merging_logic.rs` — the join semilattice laws for the shipped `combine_mergeable_value`) |
| **T-ALG (IntegerAdd c/d)** | wrapping-add group + checked-apply reject overflow/`<0` (S7) | Rocq `IntegerAdd.wadd_assoc`, `checked_apply_rejects_overflow`/`_negative`; Rust proptests `integer_add_is_commutative`, `integer_add_is_associative`, `integer_add_overflow_returns_none` (`≡ i64::checked_add`) + unit `integer_add_rejects_overflow_and_underflow` (`rspace++/…/merging_logic.rs`) |
| **IntegerAdd launder** | fail-loudly at BOTH combine **and terminal apply**; the diff (`end−prev`) stays wrapping — it is the group inverse that recovers the true delta; supply-cap bound | Rocq `IntegerAdd.launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder`; Z3 `integeradd_launder_bitvec.py`; Rust `combine_mergeable_value` (combine, `checked_add`), `calculate_number_channel_merge` (terminal apply, `checked_add`+`≥0`); tests `cal_merged_result_rejects_integer_add_true_launder_wraps_nonnegative`, `merge_integer_add_overflow_is_rejected`, `diff_integer_add_recovers_wrapped_delta` |
| **A9 exact-integer FT** | finalization decides `2·q·den ⋛ S·(den+num)` in i128 (`≥` floor / `>` LFB), not the fuzzy f32 ratio — precise + node-identical | Rocq `MainTheorem.finalized_floor_ftexact_correct` (`FtExact.v`); Z3 `ft_exact_no_overflow.py`; Sage `ft_algebra.sage`; Rust `clique_oracle.ft_decides_exact`/`ft_witnessed_exact`; test `ft_decides_exact_tests` |
| **ancestry precondition (GAP-2/GAP-4)** | `CliqueOracle.v`/`Selection.v` model DAG ancestry ABSTRACTLY (`anc_of`); the trusted realization `is_dag_ancestor` (`block_dag_key_value_storage.rs`, used by `floor.rs`) computes EXACTLY that relation. Its block-number prune is sound under strict per-edge monotonicity (`wf_dag`: `block_number = 1 + max parent`), which block validation enforces — **not** the global contiguity (`max−min==len`) that `block_metadata_store.rs` demoted to a `warn!` (GAP-4: a strictly stronger, separate diagnostic the prune never needed) | Rust property test `is_dag_ancestor_matches_reflexive_transitive_closure_over_parents` (`block-storage`, `--features test-internals`): on random well-formed DAGs, `is_dag_ancestor` (with the prune) ≡ the reflexive-transitive closure over parents |
| **capstone** | all of the above, axiom-free | Rocq `MainTheorem.{finalized_floor_merge_correct, finalized_floor_selection_correct, finalized_floor_arithmetic_correct, finalized_floor_phase7_correct, finalized_floor_ftexact_correct, finalized_floor_thetaexact_advance_correct}` (the last bundles C1/C5) |

---

## 5. Formal artifacts

### 5.1 Rocq (`formal/rocq/finalized_floor/`) — axiom-free

Rocq/Coq 9.1.1, Stdlib-only. Every theorem is checked with `Print Assumptions`
⇒ *"Closed under the global context"* (no `Axiom`, `Parameter`, or `Admitted`),
**and independently re-verified by the trusted kernel via `coqchk`** (C3): the
`scripts/check-finalized-floor-ALL.sh` gate runs `coqchk -Q theories FinalizedFloor
FinalizedFloor.MainTheorem` ⇒ *"Modules were successfully checked"*, closing the
gap between the elaborator that *built* the `.vo` and the kernel that *checks* it —
the trust root under every capstone.

| Module | Depends on | Key results |
|---|---|---|
| `Foundation.v` | — | DAG, block numbers, main-parent spine, `walk_spine`, **T-TERM** |
| `CliqueOracle.v` | Foundation, FtExact | DAG ancestry, agreement, quorum `Finalized` (quorums carry **`NoDup (map fst Q)`** — distinct validators, matching the code's `WeightMap = HashMap<V,i64>` keys), **L-ANC**, **L-SNAP**; **C1 θ-exact bridge** — `Finalized_ft` via `FtExact.ft_exact_ge` (the REAL node θ-decision), `L_ANC_ft`/`L_SNAP_ft` (quorum-opaque re-proofs), `Finalized_ft_refines_Finalized` (θ-finalized ⇒ strict-majority `Finalized` for θ>0); **C1′ §9 θ≤0 hard gate** — `hard_gate`/`hard_gate_iff_Finalized`, `Finalized_ft_hg`, **`Finalized_ft_hg_refines_Finalized`** (the θ-independent `2·agreeing>S` gate ⇒ strict-majority `Finalized` for ALL num, closing the θ≤0 seam), `L_ANC_ft_hg`/`L_SNAP_ft_hg`; **C5 advancement** — `snap_advances`, `L_SNAP_advance`, `snap_extends_snap_advances` (preservation ⇒ advancement; original L-SNAP is the corollary) |
| `Floor.v` | CliqueOracle | **T-CACHE** (`warm_eq_cold`, `frontier_cache_transparent`) — takes `AdjDC` as a hypothesis |
| `GuardBridge.v` | Foundation, CliqueOracle, Floor | **guard ⇒ AdjDC** (`chain_adj_AdjDC`): under a *constant* committee, finalization along the spine is downward-closed, so the Rust committee-constancy guard *derives* Floor.v's `AdjDC` premise (no longer assumed); `guard_constant_committee_transparent` (warm == cold with AdjDC derived); **T-FIN** (`upgo_finalized`: the warm up-walk's result is `Finalized`). **Section `BridgeFt` (C1′ θ≤0)** — the SAME construction over the θ-exact `Finalized_ft` via `L_ANC_ft`: `chain_adj_AdjDC_ft`, **`guard_constant_committee_transparent_ft`** (T-CACHE for ALL num, no `0<num` bridge), `upgo_finalized_ft` |
| `Merge.v` | — | semilattice fold: **T-DETMERGE/T-CONV** (`merge_*_perm`), **T-K1** (`merge_or_no_lost_bit`) |
| `Recovery.v` | — | **T-NDA** (`apply_idem`, `no_double_apply`) |
| `Selection.v` | Floor, CliqueOracle | the Case-A/B sound-base pick: **T-SOUND**, **T-LIN**, **T-PS**, **T-FIN**, **T-COMM**, **H3**, **Case-B**, **maximality** (`select_sound`, `select_none_correct`, `case_a_common_ancestor`, `T_PS`, `select_finalized`, `committee_is_floor_bonds`, `scope_covers_band`, `case_b_compatible`, `select_highest_sound`) |
| `IntegerAdd.v` | — | signed-64 wrapping: **T-ALG(c)** (`wadd_assoc`), **T-ALG(d)** (`checked_apply_rejects_*`), launder `launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder` |
| `FtExact.v` | — | **A9 exact-integer FT** (`ft_exact_iff_ratio`/`_strict`, `ft_exact_mono_q`, `ft_exact_no_overflow`): the exact test `2q·den ≥ S(den+num)` IS the f32 ratio test cleared of denominators, monotone in `q`, overflow-free in i128 |
| `MainTheorem.v` | all | capstones `finalized_floor_merge_correct`, `finalized_floor_selection_correct`, `finalized_floor_arithmetic_correct`, `finalized_floor_phase7_correct`, `finalized_floor_ftexact_correct`, and **`finalized_floor_thetaexact_advance_correct`** (C1/C5/C1′ bundle: `L_ANC_ft` ∧ `L_SNAP_ft` ∧ `Finalized_ft_refines_Finalized` [θ>0] ∧ `L_SNAP_advance` ∧ `snap_extends_snap_advances` ∧ `Finalized_ft_hg_refines_Finalized` [θ≤0 hard gate]) |

The finalization model is a faithful monotone abstraction of `ft_witnessed`:
`Finalized c J b` := *some majority-weight sub-committee all agree on `b`* (a
clique is such a quorum). L-ANC/L-SNAP hold by the **same-quorum argument** — the
identical validators that finalize `b` finalize every ancestor of `b`, and still
do under a larger snapshot — which is exactly why they hold for the real oracle
(the pairwise-clique refinement reuses the same witnessing set verbatim).

**C1 — the strict-majority `Finalized` is no longer a proxy.** The node's REAL
finalization decision is the θ-exact test `FtExact.ft_exact_ge` (θ = num/den =
ppm/1e6), not the hard-coded strict-majority (θ=0) corner. `Finalized_ft` is
`Finalized` with its quorum weight-condition replaced by that exact test; because
L-ANC/L-SNAP are **quorum-opaque** (they carry the *same* `Q` through, never
inspecting its weight bound) they re-prove verbatim as `L_ANC_ft`/`L_SNAP_ft`. The
bridge `Finalized_ft_refines_Finalized` then shows every θ-finalized block (θ ∈
(0,1) over a positive-stake committee) is also strict-majority `Finalized`, so it
inherits T-CACHE and every downstream capstone with no re-proof — T-CACHE's no-fork
guarantee now rests on the decision the node actually runs. **Disclosed side-
condition** (faithful, necessary; documented in `CliqueOracle.v` §7): the strict
bridge needs `0 < num` (θ>0 is *strictly* above majority) and `0 < cweight c`
(positive committee stake) — a zero-stake committee's empty quorum vacuously
"finalizes" everything under the exact test yet fails strict majority, so the
side-condition is the minimal faithful fix, not a weakening.

**C1′ — θ≤0 is covered by the θ-INDEPENDENT hard gate.** The `0<num` side-condition
means `Finalized_ft_refines_Finalized` is **vacuous at the DEFAULT θ=0** (and the
negative-θ sentinels), so the strict-majority proxy would appear unsupported there.
But the node does **not** finalize on the θ-test alone: `ft_decides_exact`
(clique_oracle.rs:79-81) first applies a θ-independent **hard majority gate**
`if 2·agreeing ≤ S return false`, where `agreeing` is the TOTAL agreeing weight and
the θ-tested clique weight `q = max_clique_weight` is a sub-part (`q ≤ agreeing`;
the call sites pass them separately). `CliqueOracle.v` §9 models this as `hard_gate`
(a strict-majority *agreeing* set, provably `Finalized` — `hard_gate_iff_Finalized`)
and the node's real decision as `Finalized_ft_hg := Finalized_ft ∧ hard_gate`; the
capstone-checked **`Finalized_ft_hg_refines_Finalized`** shows the hard gate ALONE
yields strict-majority `Finalized` for **ALL num** — no `0<num`, no positive-stake
side-condition — so θ≤0 inherits every downstream result. Independently, T-CACHE
holds directly over `Finalized_ft` for all num via `GuardBridge.BridgeFt`
(`guard_constant_committee_transparent_ft`, built on `L_ANC_ft` which needs no sign
of `num`), so **cache transparency was never gated on `0<num`** either. The Z3
`ft_exact_no_overflow.py` exhibits the seam and its closure: the θ-test alone at
θ≤0 can finalize with `2q ≤ S` (a `sat` GAP), while the real decision (θ-test ∧
`2·agreeing>S`) always carries a strict-majority agreeing set (`unsat` refutation of
any counterexample over the full `−den ≤ num ≤ den` range). **C5 — snapshot
growth is modeled as latest-message ADVANCEMENT** (`snap_advances`: each binding
moves forward to a DAG-descendant), strictly more faithful than the preservation-
only `snap_extends`; `L_SNAP_advance` re-proves L-SNAP for it (via
`agrees_snap_advance_mono`/`anc_of_trans`), and `snap_extends_snap_advances`
(preservation ⇒ advancement, `anc_refl`) makes the original `L_SNAP` its
reflexive-descendant corollary — nothing existing is weakened.

Build (memory-capped, per the 32 GB envelope):

```bash
cd formal/rocq/finalized_floor && coq_makefile -f _CoqProject -o Makefile
systemd-run --user --scope -p MemoryMax=16G -p CPUQuota=1800% -p TasksMax=200 \
  make -C formal/rocq/finalized_floor -j1
```

### 5.2 TLA⁺ (`formal/tlaplus/finalized_floor/`)

`FinalizedFloor.tla` carries **both** models:

- **Pre-fix** `Spec` + `MC_FinalizedFloor_pre_fix.cfg`: unguarded propose with the
  silent single-parent fallback. TLC **discovers the counterexample** —
  `Inv_NoLostParentWrite` violated (`parentKeys = {1,2}`, `mergeKeys = {1}`) once
  Δ crosses the cap. This is the formal reproduction of the write-loss.
- **Post-fix** `SpecFixed` + `MC_FinalizedFloor.cfg`: the backstop as a park-guard
  (no lossy merge; scope-gate demoted). TLC **passes**: `Inv_NoLostParentWrite`,
  `Inv_DeltaWithinCap`, `Inv_FloorMonotone`, and the temporal `Liveness_Progress`
  all hold.

`FinalizedFloorScan.tla` models the **H3 scan + T-PS** that `FinalizedFloor.tla`
abstracts away: per-block writes collected by the floor-bounded scan over an
**unconstrained** parent set (any nonempty subset). `MC_FinalizedFloorScan.cfg`
(`BadCut = 0`, the fix) **passes** — `Inv_NoParentWriteDropped` + `Inv_BandCovered`
hold for every parent set (H3 is now model-checked, not assumed; T-PS holds);
`MC_FinalizedFloorScan_bug.cfg` (`BadCut = 1`, the old cut-above-floor bound) is
**violated** (the H3 counterexample).

Run under the bounded envelope:

```bash
source scripts/lib/tlc-run.sh
FF=formal/tlaplus/finalized_floor
tlc_run "$(tlc_metadir ff_post)" "$FF/MC_FinalizedFloor.cfg"         "$FF/FinalizedFloor.tla"   # PASS
tlc_run "$(tlc_metadir ff_pre)"  "$FF/MC_FinalizedFloor_pre_fix.cfg" "$FF/FinalizedFloor.tla"   # counterexample (exit 12)
```

### 5.3 Wolfram (`formal/wolfram/finalized_floor/delta_ratchet.wl`)

Models the Δ difference equation and proves — over the reals, parameter-free —
that the buggy Θ(Δ²) advance is **structurally unstable** (feedback slope > 1 at
every equilibrium) while the fixed O(1) advance has zero feedback. Run with
`wolfram -script delta_ratchet.wl` (or the `math` kernel).

### 5.4 Empirical soak (`casper/tests/batch2/map_cell_convergence_spec.rs`)

`finalized_floor_400_block_soak` (`#[ignore]`) runs `run_convergence(3, 100, 20)`
≈ 421 blocks — an order of magnitude past the green-gate and well past the old
256/512 cliff. Every merge exercises the warm up-walk; a backstop `Err` would
surface as a panic. Across the full run the fix-relevant invariants held with
**zero** violations: no Δ-backstop fired, no fork (cross-node LFB + finalized-key
identity every round), no finalized write lost (FS-monotonicity), single-datum
cell (keep-one collapsed). Run:

```bash
cargo test -p casper --test mod --release -- finalized_floor_400_block_soak --ignored
```

---

## 6. Additional findings (investigated during verification)

### A10 — recovery throughput is bounded by the deploy-lifespan window

The soak's *terminal* full-convergence check initially failed: under **sustained**
single-cell N-writer overload the keep-one recovery backlog grows ~`(N−1)`/round
while recovery drains ~`1`/round, so old losers **expire** (deploy_lifespan)
before recovery. This is a **capacity bound**, not a merge fault — the merge held
every per-round invariant for the whole run. It is fundamental to keep-one on a
single cell (you cannot finalize `N` conflicting whole-cell writes/round when only
one survives per merge). The soak therefore asserts the fix-relevant invariants
every round and gates only the terminal full-convergence behind
`require_full_convergence` (the graded gates keep it `true`; the soak passes
`false`). *Not a consensus bug; a hotspot the application must avoid.*

### A9 — the fault-tolerance decision is now EXACT-INTEGER — **RESOLVED**

`clique_oracle.rs` previously computed `ft = (2q − S)/S` in **f32** and finalized on
`ft ≥ θ` (f32). That decision was deterministic (IEEE-754 f32 is exactly-rounded, so
every conforming node computed the identical verdict — no fork) but *imprecise*: for
stakes `> 2²⁴` the `i64→f32` cast drops mantissa bits, making the threshold fuzzy by
`O(S/2²⁴)`. The fix replaces the finalization **decision** with an **exact-integer**
test over `i128`, `θ = num/den = ppm / 1_000_000`:

- **`≥` (floor path):** `2·q·den ≥ S·(den + num)` — `clique_oracle.rs`
  `ft_decides_exact` / `ft_witnessed_exact`, routed through `floor.rs`'s three decision
  sites; **`>` (LFB finalizer):** the strict twin via `finalizer.rs`
  `compute_decision_with_cache(…, strict=true)` — the finalizer's strict-`>` clearance
  is preserved. The early `agreeing ≤ S/2 ⇒ not finalized` becomes exact `2·agreeing ≤ S`.
  The `f32` `ft` value is kept only for display/metadata (`fault_tolerance_value`); **no
  decision is re-derived from it**. θ is threaded as the exact on-chain **ppm** (i64),
  converted once at `initializing.rs` (never the lossy f32).
- **i128 rationale:** `2·q·den ≤ ~2⁸⁴` and `S·(den+num) ≤ ~2⁸⁴` for `S ≤ i64::MAX`,
  `den = 10⁶`, both far below `2¹²⁷` — no overflow.
- **Formal (axiom-free):** Rocq `FtExact.v` — `ft_exact_iff_ratio` (the exact test IS
  `(2q−S)/S ≥ num/den` cleared of its positive denominators, *unconditional*),
  `ft_exact_iff_ratio_strict`, `ft_exact_mono_q` (monotone in `q`, given `den ≥ 0` — the
  one honest side-condition, faithful since `den = 10⁶`), `ft_exact_no_overflow` (the
  i128 envelope). Z3 `ft_exact_no_overflow.py` and Sage `ft_algebra.sage` cross-witness
  the same (i128 no-overflow, exact≡ratio for `≥` and `>`, and that the f32 residual is
  real — `2²⁴` and `2²⁴+1` collide under `i64→f32`). Tests: `ft_decides_exact_tests`
  (small-stake agreement with f32, the large-stake boundary tie, `2·agreeing = S`, and
  no overflow at i64::MAX-scale stake).
- **Activation:** the exact decision changes which blocks finalize *at the margin* — a
  consensus-observable change — so it activates **atomically with the unreleased
  `staging-into-dev-merge` branch** (all validators run the branch binary together; no
  mixed-version window), the same discipline as the IntegerAdd apply change (below). No
  on-chain activation parameter is introduced.

### IntegerAdd overflow-launder — **FIXED at both chokepoints** (Phase 6 W3 combine; Phase 7 W7.1 terminal apply)

The IntegerAdd number-channel pipeline is a **wrapping group** `ℤ/2⁶⁴`, and it has
exactly TWO points where an overflow must be caught — the *combine* (summing a
branch's diffs) and the *terminal apply* (writing `base + Σdiffs` to consensus
state). The per-deploy *diff* is deliberately **not** one of them.

**The defect.** Two silent-`wrapping_add` sites could commit an overflowed value.
These must be distinguished from the *rejection decision*: `conflict_set_merger.rs`
`cal_merged_result` already used `checked_add` + `≥0` to decide *which* branches to
drop — it is **not** a write. The two writes that still wrapped were
`rspace++/…/merging_logic.rs` `combine_mergeable_value` (folds a branch's IntegerAdd
diffs) and `rholang/…/rholang_merging_logic.rs` `calculate_number_channel_merge`
(the terminal apply that actually PERSISTS the merged value via `dag_merger`). A sum
that wraps to a **non-negative** value (e.g. `i64::MAX + i64::MAX + 2 ≡ 0 (mod 2⁶⁴)`)
would sail through the `≥0` gate carrying a wrong value.
**Reachability (corrected):** the terminal apply is a consensus-state write reached
whenever `base + Σdiffs` crosses `i64::MAX` — e.g. near-`i64::MAX` genesis vault
balances — and was a fragile *non-local* invariant (safe only because the combine
gate happens to keep sums in range), not "unreachable for realistic supply".

**Resolution** (user decision: **both** — fix *and* prove bounded):
- **Fix — combine (Phase 6 W3):** `combine_mergeable_value` returns `Option<i64>`;
  IntegerAdd uses `checked_add` (`None` on overflow), propagated through
  `EventLogIndex::combine` (Level A) and `cal_merged_result` (Level B) so a wrapping
  combine **rejects the branch**. BitmaskOr unchanged.
- **Fix — terminal apply (Phase 7 W7.1):** `calculate_number_channel_merge` IntegerAdd
  now uses `checked_add` + a `≥0` guard → `Err(HistoryError::MergeError)` on overflow
  OR a negative balance, never `wrapping_add`. A defense-in-depth backstop (the
  combine gate rejects overflow upstream, so this can only reject an already-wrong
  value: for `base ≥ 0` any positive overflow wraps *negative*, caught by `≥0`). The
  `Err` propagates through `dag_merger::merge → interpreter_util.rs`, aborting the
  merge deterministically.
- **Deliberately NOT changed — the diff `end − prev`:** `calculate_num_channel_diff`
  stays `wrapping_sub`. It is the exact **group inverse** of the wrapping add that
  language-level execution (`reduce.rs` `GInt +`, intended 64-bit semantics) used to
  produce `end`, so it faithfully **recovers the deploy's true intended delta** even
  when execution overflowed and stored a wrapped `end`. `end − prev` can legitimately
  exceed `i64` range for such a deploy; a `checked_sub` there would hard-error the
  LIVE per-block diff path and crash block processing on a deploy that must instead be
  **gracefully rejected at merge time**. (The casper `…_got_overflow` integration test
  refuted a checked-diff design; regression `diff_integer_add_recovers_wrapped_delta`
  locks the invariant in.)
- **Formal:** Rocq `IntegerAdd.v` — `launder_exhibit` confirms the defect is real,
  `checked_combine_sound` proves the fix is launder-free (an accepted combine returns
  the true sum, in range), `supply_cap_no_launder` proves the defense-in-depth bound
  (while partial sums stay ≤ Cap, wrapping = checked); the Z3 BitVec-64 witness
  `integeradd_launder_bitvec.py` confirms the same against exact machine `i64`
  (`bvadd = wrapping_add`). In the model, `checked_apply` corresponds to the terminal
  apply (Site 1) and `wadd`/`wsum` to the wrapping execution + the diff group.

**Activation.** The reject-vs-wrap change at the combine + terminal apply activates
*atomically* with the unreleased finalized-floor feature (the
`dag_merger::merge → conflict_set_merger → calculate_number_channel_merge` chain is
part of it) — no separate flag, no mixed-version window. The diff
(`calculate_num_channel_diff`) is on the already-live execution/replay path but is
left wrapping (above), so its behavior is unchanged and there is nothing to gate.

### A9 — exact-integer fault tolerance — **RESOLVED (see the A9 block above)**

The precision residual is closed: finalization now decides with the exact-integer test
`2·q·den ≥ S·(den + num)` (i128), across the floor and LFB-finalizer paths, activated
atomically with the branch. See the resolved **A9** block above for the full change, the
axiom-free `FtExact.v` + Z3 + Sage witnesses, the tests, and the activation note. The
change was scoped across the whole clique oracle (all finalization), independently
verified — the exact discipline the prior revision recommended.

---

## 7. Verification status

Run the whole suite with `scripts/check-finalized-floor-ALL.sh` (Rocq authoritative;
TLA⁺/Z3/Sage/Wolfram fail-soft). Current result: **ALL GATES OK**.

| Layer | Result |
|---|---|
| Rust build | `cargo check -p casper --all-targets` / `-p rspace_plus_plus` clean |
| Convergence green-gate | 3/3 pass; 400+-block soak holds all fix invariants (~421 blocks) |
| Rust unit/regression | combine + terminal-apply launder (`checked_add`), discriminating true-launder (sum wraps non-negative), wrapping-group diff recovery, guard-trip cold-fallback, Case-B dominating-tip, incompatible-fork `Err`, backstop predicate, floor warm==cold + cache-transparent, frontier round-trip — all pass |
| Rocq | full dev builds `-j1`; **14 headline results axiom-free** — 6 capstones (merge / selection / arithmetic / phase7 / **A9 ftexact** / **G2 ftprovenance**) + the 3 GuardBridge lemmas (guard⇒AdjDC `chain_adj_AdjDC`, `guard_constant_committee_transparent`, `upgo_finalized`) + the C1/C5 results (`finalized_floor_thetaexact_advance_correct`, `Finalized_ft_refines_Finalized`, `snap_extends_snap_advances`) + the **C1′ θ≤0 hard-gate** results (`Finalized_ft_hg_refines_Finalized`, `guard_constant_committee_transparent_ft`) |
| Rocq kernel (coqchk) | **independent kernel re-check** of `FinalizedFloor.MainTheorem` + all deps ⇒ "Modules were successfully checked" (C3) |
| TLA⁺ | `SpecFixed` + `FinalizedFloorScan` pass; both pre-fix cfgs reproduce their counterexample |
| Z3 | FT-algebra + BitVec-64 IntegerAdd launder (exists on wrap; checked-combine launder-free) + **G2 `ft_ppm_roundtrip`** (FPA Float32/64 RNE: `to_ppm` monotone/range, ½ppm round-trip, exact-decision display-invariance) |
| Sage | FT-algebra identity + finalization-margin monotonicity |
| Wolfram | ratchet instability (buggy unstable / fixed stable) — via the licensed MCP evaluator |
| Loom (concurrency) | **C10** `loom_frontier_floor_cache` — the write-once `floor_index`/`frontier_index` memoization observes no torn/regressed value on any interleaving (the concurrent realization of the sequentially-proved T-CACHE; real guarantee = idempotence + LMDB single-key MVCC) |
| Rust proptest | **G2** `prop_ft_ppm_provenance` (`reconcile==onchain`, real `to_ppm` round-trip/range, genesis embed↔read) + **P1** `prop_bonds_from_floor` (proposer-derive ≡ validator-derive committee PLAY≡REPLAY; accept-rule = set-equality) — 11 pass |

**Coverage matrix (§4).** After the Phase-7 strengthening every catalog item maps to
a concrete Rocq/TLA⁺/Z3/Sage artifact or Rust test — including the two seams the
earlier revision had *asserted* rather than mechanized (an honest correction: that
"no assumed row" claim was premature). Both are now closed: the frontier-cache guard
is **bridged in Rocq** (`GuardBridge.chain_adj_AdjDC` derives Floor.v's `AdjDC`
premise from the committee-constancy predicate the Rust guard enforces), and T-FIN is
**unconditional** (`GuardBridge.upgo_finalized`), no longer merely `Forall Fin cands →`.
The one documented **residual** is A9's `f32` finalization *precision* (§6.A9) — not a
fork (the decision is bit-identical across conforming nodes) but a cross-cutting
exact-integer hardening tracked for the whole clique oracle, outside this feature.
**G2 (θ_ppm provenance)** additionally closed a *latent* overflow-envelope gap: the A9
`ft_exact_no_overflow` bound assumed `0 ≤ num ≤ den`, but the node's own
`ft_decides_exact` validates `num ∈ [−den, den]` (documented negative-θ "finalize on
any majority clique" sentinels); the hypothesis is now widened to `[−den, den]`
(i128 envelope still holds), and `FtProvenance.reconcile_is_onchain` proves the θ_ppm a
node finalizes with is always the *on-chain* value (local config unconditionally
overridden), so it cannot fork the exact decision.

**Policy:** all of the above run **locally**. Do **not** add any Rocq / TLA⁺ / Z3 /
Sage / Wolfram step to `.github/workflows/*` (an earlier formal-CI workflow was
deliberately removed).

### 7.1 Scope disclosure — what the capstones prove (and what they do NOT)

The finalized-floor capstones prove **DETERMINISM** of floor derivation and the
floor-anchored merge: that every honest node derives the **same** finalized floor,
the frontier **cache** is transparent (warm up-walk == cold down-walk), the
multi-parent **merge** is order-independent, and no mergeable **write** is lost.
Concretely they establish floor/cache determinism (`frontier_cache_transparent`,
`guard_constant_committee_transparent`(`_ft`)), monotone finalization (L-ANC /
L-SNAP, and their θ-exact and advancement variants), sound selection
(`select_sound`, `select_highest_sound`), and the arithmetic hardening (A9/G2).

They do **NOT** prove **CBC finalization safety** — the *quorum-intersection /
agreement* property that two conflicting blocks can never both finalize. **No such
theorem exists in this development**, and the disclosure is deliberate rather than
implied by the "no-fork" labels:

- `Finalized` here is a **faithful monotone abstraction** of the clique oracle (a
  majority-weight agreeing sub-committee), used to carry L-ANC/L-SNAP through the
  cache proof. It is **not** a proof that the finalization rule is *safe* — the
  monotonicity lemmas are quorum-**opaque** (they reuse the same witnessing set),
  which is exactly why they never need, and never establish, quorum intersection.
- The `2·cweight Q > cweight c` majority bound and the θ-exact test bound the weight
  of a *single* quorum; safety would need that **two** majority quorums must
  **share** an honest validator. That combinatorial lemma is **out of scope** here.
- **Groundwork laid (Tier-2):** the quorum abstractions now carry
  `NoDup (map fst Q)` (distinct validators, matching the code's
  `WeightMap = HashMap<V,i64>` keys). Distinct-validator quorums are precisely the
  hypothesis a future quorum-intersection lemma
  (`2·|Q₁| > S ∧ 2·|Q₂| > S ⇒ Q₁ ∩ Q₂ ≠ ∅`) would build on; it would additionally
  require committee well-formedness (`NoDup (map fst c)`) and a disjoint-weight
  bound, and is tracked as a separate effort, not asserted here.

Merge/floor determinism is a **necessary** condition for safety (a non-deterministic
floor or merge would fork outright) but not a **sufficient** one; CBC agreement lives
in the finalization rule and is stated here as an explicit **non-goal**. The same
disclosure applies to the merge-algebra dossier (`merge-algebra-verification.md`
§7.1), whose capstones likewise prove determinism, not quorum intersection.

---

## 8. Diagrams

Six PlantUML diagrams (sources + rendered SVGs in [`diagrams/`](./diagrams/); render
with `plantuml -tsvg`, checked by `scripts/check-finalized-floor-ALL.sh` step
**[6/6]**). Each is fully coloured with an in-diagram legend. Click any figure for the
full-resolution SVG.

### 8.1 Component correspondence — spec ↔ Rocq ↔ TLA⁺ ↔ Z3/Sage ↔ Rust

[![Diagram 1 — every finalized-floor component (floor derivation, clique oracle, merge write-algebra, merge driver/backstop, recovery, LMDB cache) annotated with its spec concern, Rocq module, TLA⁺ model, Z3/Sage witness, and Rust file, with the axiom-free MainTheorem capstone on top](./diagrams/01-component-correspondence.svg)](./diagrams/01-component-correspondence.svg)

*Provenance: the §4 catalog ↔ §5 artifact map, made visual.*

### 8.2 Warm up-walk vs cold down-walk (T-CACHE)

[![Diagram 2 — sequence: the warm incremental_frontier (read cached pivot → committee-constancy + L-SNAP guards → O(advance) up-walk) versus the cold top-down walk, with the L-ANC note that makes the two results identical](./diagrams/02-seq-warm-vs-cold-walk.svg)](./diagrams/02-seq-warm-vs-cold-walk.svg)

*Provenance: §3.1; Rocq `Floor.frontier_cache_transparent` + `GuardBridge`.*

### 8.3 The Δ-ratchet — buggy runaway vs fixed bounded

[![Diagram 3 — the positive-feedback ratchet: the buggy Θ(Δ²·V) floor walk starves finalization and drives Δ across the 256 cliff into silent write-loss, versus the fixed amortized-O(1) up-walk that keeps Δ bounded with a deterministic over-cap Err](./diagrams/03-delta-ratchet.svg)](./diagrams/03-delta-ratchet.svg)

*Provenance: §2; Wolfram `delta_ratchet.wl` (feedback slope > 1 buggy / 0 fixed).*

### 8.4 IntegerAdd overflow-launder and the fail-loudly fix

[![Diagram 4 — the launder (a combine that wraps to a non-negative value passes the ≥0 apply gate carrying a wrong value) versus the fix (checked_add at both the combine and the terminal apply; the per-deploy diff stays the wrapping group inverse that recovers the true delta)](./diagrams/04-integeradd-launder-and-fix.svg)](./diagrams/04-integeradd-launder-and-fix.svg)

*Provenance: §6; Rocq `IntegerAdd.v`; Z3 `integeradd_launder_bitvec.py`.*

### 8.5 Merge flow — floor derive → floor-bounded scan → Δ-backstop

[![Diagram 5 — activity: compute_parents_post_state derives the floor, computes Δ, and either parks (propose) / rejects (validate) deterministically when over the cap, or runs the H3 floor-bounded scan and the checked merge fold to produce the post-state](./diagrams/05-activity-merge-flow.svg)](./diagrams/05-activity-merge-flow.svg)

*Provenance: §3.2/§3.3; Rocq `Selection.scope_covers_band`; TLA⁺ `FinalizedFloorScan`.*

### 8.6 Finalization downward-closure + the warm-path guards

[![Diagram 6 — state: the warm up-walk advancing over a downward-closed finalized prefix (L-ANC), with the committee-constancy and L-SNAP guard transitions that divert to the cold-walk fallback yielding the identical frontier](./diagrams/06-state-finalization-guards.svg)](./diagrams/06-state-finalization-guards.svg)

*Provenance: §3.1; Rocq `GuardBridge.chain_adj_AdjDC` (guard ⇒ AdjDC).*

---

### 8.7 A9 — exact-integer fault-tolerance decision vs the f32 fuzzy threshold

[![Diagram 7 — the legacy f32 path casts q, S to f32 (lossy above 2²⁴) and compares the fuzzy ratio, versus the exact-integer i128 cut 2·q·den ⋛ S·(den+num) that is bit-identical on every node](./diagrams/07-a9-exact-vs-f32-decision.svg)](./diagrams/07-a9-exact-vs-f32-decision.svg)

*Provenance: §6.A9; Rocq `FtExact.v`; Z3 `ft_exact_no_overflow.py`; Sage `ft_algebra.sage`.*

---

## 9. References

Foundational literature for the consensus, finality, arithmetic, and
formal-methods claims in this dossier. DOIs are given where they exist and have
been verified; whitepapers without a DOI carry a stable identifier.

1. L. Lamport. **The Part-Time Parliament.** *ACM Trans. Comput. Syst.* 16(2),
   1998. DOI [10.1145/279227.279229](https://doi.org/10.1145/279227.279229).
   *(Quorum-based agreement — the majority-weight sub-committee underlying the
   clique oracle's `Finalized`.)*
2. M. Castro, B. Liskov. **Practical Byzantine Fault Tolerance and Proactive
   Recovery.** *ACM Trans. Comput. Syst.* 20(4), 2002.
   DOI [10.1145/571637.571640](https://doi.org/10.1145/571637.571640).
   *(BFT quorum intersection — why a finalized cut is stable, §1, L-ANC.)*
3. M. J. Fischer, N. A. Lynch, M. S. Paterson. **Impossibility of Distributed
   Consensus with One Faulty Process.** *J. ACM* 32(2), 1985.
   DOI [10.1145/3149.214121](https://doi.org/10.1145/3149.214121).
   *(Why liveness is guarded, not assumed — §2's ratchet is a liveness failure.)*
4. C. Dwork, N. Lynch, L. Stockmeyer. **Consensus in the Presence of Partial
   Synchrony.** *J. ACM* 35(2), 1988.
   DOI [10.1145/42282.42283](https://doi.org/10.1145/42282.42283).
   *(Partial-synchrony model behind the propose/finalize progress assumption,
   TLA⁺ `Liveness_Progress`.)*
5. L. Lamport. **The Temporal Logic of Actions.** *ACM Trans. Program. Lang.
   Syst.* 16(3), 1994.
   DOI [10.1145/177492.177726](https://doi.org/10.1145/177492.177726).
   *(TLA — the basis of the `FinalizedFloor.tla` / `FinalizedFloorScan.tla` models.)*
6. Y. Bertot, P. Castéran. **Interactive Theorem Proving and Program Development:
   Coq'Art.** Springer, 2004.
   DOI [10.1007/978-3-662-07964-5](https://doi.org/10.1007/978-3-662-07964-5).
   *(The Coq/Rocq calculus in which the axiom-free capstones are mechanized, §5.1.)*
7. IEEE. **IEEE Standard for Floating-Point Arithmetic (IEEE 754-2019).** 2019.
   DOI [10.1109/IEEESTD.2019.8766229](https://doi.org/10.1109/IEEESTD.2019.8766229).
   *(Exactly-rounded, deterministic `f32` — the A9 determinism argument, §6.A9.)*
8. V. Zamfir et al. **Introducing the Minimal CBC Casper Family of Consensus
   Protocols.** CBC Casper whitepaper, 2018. (No DOI; stable source:
   `https://github.com/cbc-casper/cbc-casper-paper`.)
   *(Correct-by-construction safety oracle and the linear-finality fringe that
   `floor(B)` specializes, §1.)*
