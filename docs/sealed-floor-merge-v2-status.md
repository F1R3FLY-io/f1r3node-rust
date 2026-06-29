# Sealed-Floor Merge v2 — Status

Branch: `feat/sealed-floor-merge-v2` (off `staging`). Companion to the reference-branch PR
[F1R3FLY-io/f1r3node-rust#77](https://github.com/F1R3FLY-io/f1r3node-rust/pull/77)
(`feat/floor-sealed-merge`), which this branch rebuilds cleanly per the sealed-floor design.

## Where we are

This branch rebases the multi-parent merge on a **node-deterministic finalized floor** with
**record-driven recovery**, replacing the reference branch's main_parent-tip base + gas-cell
recovery (which let finalized state regress). BASE + RECOVERY + a first single-value-cell keep-one
are landed and the unit/integration suite is green.

The green-gate proxy (`casper/tests/batch2/map_cell_convergence_spec.rs`) drives concurrent
single-value-cell writes (the same shape as the PoS bonds map and the
`test_user_contract_concurrency` / `test_validator_lifecycle` integration tests). Its two
remaining failure modes under load are now **root-caused to specific functions** — see "Remaining".

## What we accomplished

**Foundation / BASE (§3a)**
- `finalized_floor` (`casper/src/rust/finality/floor.rs`): a per-block, **justification-derived,
  node-deterministic** finalized cut (parent-floor inheritance + per-parent advancement +
  sound-base selection), plus a node-deterministic fault-tolerance witness.
- Merge base = `floor.post_state`; scope = `closure(parents) \ closure(floor)`.

**Recovery (§3b)**
- `canonical_won_sigs` (`interpreter_util.rs`): record-driven done-detection — a deploy is
  re-proposable unless its **latest canonical disposition** (winner in `body.deploys`, loser in
  `body.rejected_deploys`) across the **full merge scope** is a WIN. Proposer (`block_creator`)
  and validator (`repeat_deploy`) gate on the **same record** → **InvalidRepeatDeploy eliminated**,
  double-apply closed. Walks all parents (not just the main-parent chain) so a deploy already won
  on a co-parent is caught.

**Merge (§3c, first pass)**
- Single-value-cell keep-one (`dag_merger.rs`): a chain-level available-multiset pass that rejects
  concurrent writers of one non-foldable cell to recovery (the "bolt-on"), replacing the
  over-rejecting rejection-expansion.

**Determinism / robustness**
- Deterministic slash-deploy replay (block-derived invalid-blocks map).
- On-demand mergeable-entry recompute (`ensure_scope_mergeable_present`) to heal a cross-node
  merge-validity fork for LFS-imported blocks.

**Test infrastructure**
- Green-gate proxy with production-like heartbeat (`TestNode.allow_empty_blocks`), a **cross-node
  node-identity assertion** (`finalized_keys_all_nodes` reads every node, not just node 0), and a
  **deterministic single-value-cell datum-count check** (`m_datum_count` via `get_data`) that turns
  a flaky cross-node peek into a precise "`@"m"` holds N datums at block #B" failure.

## Remaining (known)

### Two pinpointed fixes for the proxy's under-load failure modes
Both root-caused by code reading; independent of each other and of the keep-one bolt-on.

1. **Multi-datum single-value cell** (cross-node "node-identity" divergence) —
   `ChannelChange::combine` (`rspace++/src/rspace/merger/channel_change.rs`) **unions** added/removed
   instead of **netting** them, so a dependent-chain intermediate datum (added by one change,
   removed by another) orphans onto the floor base → two datums on a single-value cell, which a peek
   read then samples non-deterministically across nodes. **Fix:** net the intermediate —
   `combined.added = (A.added ∪ B.added) \ (A.removed ∪ B.removed)` (symmetric for removed).
   Latent in the reference branch too: its main_parent-**tip** base bakes the dependent writers into
   the base, so they never reach `combine`; v2's **floor** base puts them in scope as chains.

2. **Convergence-missing** (a write silently lost) — deploys are evicted from the pending pool on
   block-**accept** (`multi_parent_casper_impl.rs`), but recovery only re-proposes merge-**rejected**
   deploys. An accepted-but-orphaned, never-rejected deploy is therefore in neither the pool nor the
   recovery buffer → lost. **Fix:** evict on **finalize** (not accept), or extend the recovery net to
   accepted-but-orphaned deploys.

### Larger design item
- **§3c proper** — fix `EventLogIndex::combine` multiplicity so `resolve_conflicts` natively rejects
  N−1 of N concurrent single-cell writers, then **delete the keep-one bolt-on**. This is the
  conflict-detection side and does **not** subsume fix (1) above (dependent chains are not conflicts).

### Still to port from the reference branch (PR #77)
The reference carries independent keepers not yet ported. The full per-commit
PRESERVE / REBUILD / DROP / IN-STAGING / MIXED inventory is in
[`docs/port-manifest.md`](./port-manifest.md). Highlights:

- LMD-GHOST main-parent selection (`22299115`).
- Genesis-sourced FT threshold (`97767045`) — required for a node-identical floor.
- **LFS state-sync hardening (`ad0081d7`)** — networking resilience (join-all, deadline budget,
  byzantine reason).
- Full-bonds finality denominator (`f0decf13`, a safety fix); multi-value IntegerAdd reject
  (`16b2e980`); DAG-ancestry finality agreement (`e349dc4e`); fresh-joiner latest-message fix
  (`7ed761ab`); active/live-committee weighting (`e7efb39d`, `4f63cb82` committee half);
  total-order rejection tiebreak (`9d1f5d1d`); bonds-equality parent-filter removal
  (`3499b39e`, part).
