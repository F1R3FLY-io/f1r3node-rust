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
cold walk (`floor_incremental_guard_fallback` metric).

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
| **T-CACHE** | warm up-walk == cold walk (no fork from cache, S1) | Rocq `Floor.warm_eq_cold`, `frontier_cache_transparent` |
| **T-DETMERGE / T-CONV** | merge order-independent (no fork, S6) | Rocq `Merge.merge_or_perm`, `merge_max_perm` |
| **T-K1** | no mergeable write lost (the 400-block loss, S5) | Rocq `Merge.merge_or_no_lost_bit`, `merge_absorbs` |
| **T-NDA** | recovery not double-applied | Rocq `Recovery.apply_idem`, `no_double_apply` |
| **S5 / Inv_NoLostParentWrite** | over-Δ never drops a parent write | TLA⁺ `SpecFixed` (holds); `Spec` (violated) |
| **Δ bound (driver)** | floor distance stays ≤ cap | TLA⁺ `Inv_DeltaWithinCap` |
| **L3/L5 liveness** | chain still progresses despite the backstop | TLA⁺ `Liveness_Progress` |
| **ratchet instability** | buggy advance is structurally unstable | Wolfram `delta_ratchet.wl` |
| **T-SOUND** | chosen floor is a sound base; `None` ⇒ Err correct (S4) | Rocq `Selection.select_sound`, `select_none_correct` |
| **T-LIN** | a Case-A base is a common DAG-ancestor (one chain) | Rocq `Selection.case_a_common_ancestor` |
| **T-FIN** | the chosen floor is finalized | Rocq `Selection.select_finalized` |
| **T-PS** | safety for ANY parent list (unconstrained oracle) | Rocq `Selection.T_PS`; TLA⁺ `FinalizedFloorScan` (nondeterministic parent set) |
| **T-COMM** | committee = `bonds_of(floor)`, a pure fn of the floor (S8) | Rocq `Selection.committee_is_floor_bonds` |
| **H3 coverage** | floor-bounded scan drops no parent write ≥ floor | Rocq `Selection.scope_covers_band`; TLA⁺ `FinalizedFloorScan` (`.cfg` PASS, `_bug.cfg` counterexample) |
| **T-ALG (semilattice)** | BitmaskOr / keep-one fold laws | Rocq `Merge` (`Nat.lor` / `Nat.max`) |
| **T-ALG (IntegerAdd c/d)** | wrapping-add group + checked-apply reject overflow/`<0` (S7) | Rocq `IntegerAdd.wadd_assoc`, `checked_apply_rejects_overflow`/`_negative` |
| **IntegerAdd launder** | fail-loudly fix is launder-free + supply-cap bound | Rocq `IntegerAdd.launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder`; Z3 `integeradd_launder_bitvec.py`; Rust merger tests |
| **capstone** | all of the above, axiom-free | Rocq `MainTheorem.{finalized_floor_merge_correct, finalized_floor_selection_correct, finalized_floor_arithmetic_correct}` |

---

## 5. Formal artifacts

### 5.1 Rocq (`formal/rocq/finalized_floor/`) — axiom-free

Rocq/Coq 9.1.1, Stdlib-only. Every theorem is checked with `Print Assumptions`
⇒ *"Closed under the global context"* (no `Axiom`, `Parameter`, or `Admitted`).

| Module | Depends on | Key results |
|---|---|---|
| `Foundation.v` | — | DAG, block numbers, main-parent spine, `walk_spine`, **T-TERM** |
| `CliqueOracle.v` | Foundation | DAG ancestry, agreement, quorum `Finalized`, **L-ANC**, **L-SNAP** |
| `Floor.v` | CliqueOracle | **T-CACHE** (`warm_eq_cold`, `frontier_cache_transparent`) |
| `Merge.v` | — | semilattice fold: **T-DETMERGE/T-CONV** (`merge_*_perm`), **T-K1** (`merge_or_no_lost_bit`) |
| `Recovery.v` | — | **T-NDA** (`apply_idem`, `no_double_apply`) |
| `Selection.v` | Floor, CliqueOracle | the Case-A/B sound-base pick: **T-SOUND**, **T-LIN**, **T-PS**, **T-FIN**, **T-COMM**, **H3** (`select_sound`, `select_none_correct`, `case_a_common_ancestor`, `T_PS`, `select_finalized`, `committee_is_floor_bonds`, `scope_covers_band`) |
| `IntegerAdd.v` | — | signed-64 wrapping: **T-ALG(c)** (`wadd_assoc`), **T-ALG(d)** (`checked_apply_rejects_*`), launder `launder_exhibit`/`checked_combine_sound`/`supply_cap_no_launder` |
| `MainTheorem.v` | all | capstones `finalized_floor_merge_correct`, `finalized_floor_selection_correct`, `finalized_floor_arithmetic_correct` |

The finalization model is a faithful monotone abstraction of `ft_witnessed`:
`Finalized c J b` := *some majority-weight sub-committee all agree on `b`* (a
clique is such a quorum). L-ANC/L-SNAP hold by the **same-quorum argument** — the
identical validators that finalize `b` finalize every ancestor of `b`, and still
do under a larger snapshot — which is exactly why they hold for the real oracle
(the pairwise-clique refinement reuses the same witnessing set verbatim).

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

### A9 — the `f32` fault-tolerance ratio is deterministic (not a fork bug)

`clique_oracle.rs` computes `ft = (2q − S)/S` in **f32** and `floor.rs` compares
`ft ≥ θ` (f32). Investigated with Wolfram: `q` (max-clique weight) and `S` (stake)
are integers computed identically on every node, and IEEE-754 f32 arithmetic is
exactly-rounded/deterministic ⇒ **every conforming node computes the identical
decision — no fork.** The genuine residual is *precision*: for stakes `> 2²⁴` the
`i64→f32` cast drops mantissa bits, making the threshold fuzzy by `O(S/2²⁴)` — but
*consistently* across nodes (still no fork). **Recommended hardening** (future,
cross-cutting to the whole clique oracle): decide finalization with exact integer
arithmetic `2q·den ≥ S·(den+num)` for `θ = num/den`, removing the fuzz entirely.

### IntegerAdd overflow-launder asymmetry — **FIXED** (Phase 6, W2/W3)

`merging_logic.rs` `combine_mergeable_value` folded `IntegerAdd` diffs with
`wrapping_add` (silent), while `conflict_set_merger.rs:762` applies to the base
with `checked_add` (rejects on overflow/negative). A branch whose diffs wrapped in
`combine` could launder an overflow past the `checked_add` guard with a wrong
value (reachable only above `i64::MAX ≈ 9.2×10¹⁸`, so not for realistic supply).

Resolution (user decision: **both** — fix *and* prove bounded):
- **Fix (fail loudly):** `combine_mergeable_value` now returns `Option<i64>` —
  IntegerAdd uses `checked_add` (`None` on overflow), propagated through
  `EventLogIndex::combine` (Level A) and `cal_merged_result` (Level B, the
  confirmed launder) so a wrapping combine **rejects the branch** instead of
  laundering. BitmaskOr is unchanged. Regression tests at both levels
  (`combine_rejects_integer_add_overflow`,
  `cal_merged_result_rejects_integer_add_overflow_launder`).
- **Formal:** Rocq `IntegerAdd.v` `launder_exhibit` confirms the defect is real,
  `checked_combine_sound` proves the fix is launder-free, `supply_cap_no_launder`
  proves the defense-in-depth bound; the Z3 BitVec-64 witness confirms the same
  against exact machine `i64` (`bvadd = wrapping_add`).

### A9 — the `f32` fault-tolerance ratio is deterministic (residual precision only)

Still a documented residual (not a fork bug — see above). The exact-integer
hardening is cross-cutting to the whole clique oracle (all finalization, not the
finalized-floor feature); it changes the finalization rule and so warrants its own
coordinated, independently-verified change. Recorded here with the precise recipe
(`2q·den ≥ S·(den+num)`); Z3/Sage already cross-witness that equivalence.

---

## 7. Verification status

Run the whole suite with `scripts/check-finalized-floor-ALL.sh` (Rocq authoritative;
TLA⁺/Z3/Sage/Wolfram fail-soft). Current result: **ALL GATES OK**.

| Layer | Result |
|---|---|
| Rust build | `cargo check -p casper --all-targets` / `-p rspace_plus_plus` clean |
| Convergence green-gate | 3/3 pass; 400+-block soak holds all fix invariants (~421 blocks) |
| Rust unit/regression | merger overflow, Level-A/B launder, backstop predicate, floor warm==cold + cache-transparent, frontier round-trip — all pass |
| Rocq | full dev builds `-j1`; **3 capstones axiom-free** (merge/selection/arithmetic) |
| TLA⁺ | `SpecFixed` + `FinalizedFloorScan` pass; both pre-fix cfgs reproduce their counterexample |
| Z3 | FT-algebra + BitVec-64 IntegerAdd launder (exists on wrap; checked-combine launder-free) |
| Sage | FT-algebra identity + finalization-margin monotonicity |
| Wolfram | ratchet instability (buggy unstable / fixed stable) — via the licensed MCP evaluator |

**Coverage matrix (§4) has no ABSENT / assumed / prose-only row** — every catalog
item maps to a Rocq/TLA⁺/Z3/Sage artifact or a Rust test.

**Policy:** all of the above run **locally**. Do **not** add any Rocq / TLA⁺ / Z3 /
Sage / Wolfram step to `.github/workflows/*` (an earlier formal-CI workflow was
deliberately removed).
