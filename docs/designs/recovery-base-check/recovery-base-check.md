# Recovery Base-Check — preventing content-twins from re-executed merge losers

## Summary

When the multi-parent merge drops a deploy (a "loser"), it is placed in the
per-node recovery buffer so the block creator can re-propose it and let its
effect land in a later block. Re-proposing is correct **only when the deploy's
effect is not already in the execution base** the new block builds on. When it
*is* already there, re-executing the deploy a second time re-creates its
per-deploy cells, which corrupts single-value (`IntegerAdd`) cells:

- the deploy's per-deploy number cell goes multi-value, tripping the
  single-value invariant in `RhoRuntime::get_number_channel` (the
  `number channel holds N values` panic), and/or
- the deploy's gas/pre-charge accounting is applied twice, surfacing later as a
  `Deploy refund failed: Insufficient funds` platform error.

Both are the same root cause — recovery re-executing a deploy whose effect is
already in the base — with two surface symptoms.

This design adds a **base-check at re-proposal time**: before a recovered deploy
is re-executed, the proposer verifies that the deploy's effect is **not** present
in the block's actual pre-state, and skips it otherwise. The check reads the real
pre-state, so it is correct for both merged and fast-pathed bases, where the
buffer-membership and ancestry paper-trail are blind.

## Background

A user deploy that conflicts on a single-value cell (e.g. concurrent writes to
one map/number channel) is resolved by cost-optimal keep-one in the merge: one
write is kept, the rest are rejected and recorded in the recovery buffer. Their
owners re-propose them so the dropped writes can re-land on top of the winner.

A deploy can be **kept on one branch and rejected on another at the same time**
(the "flip"): a merge keeps it by absorbing its effect from a *parent's* body,
while a sibling merge rejects it into the buffer. Two consequences matter:

1. A merge that **keeps** a deploy does so by inheriting its effect from a parent
   — the deploy is **not** in that merge block's own `body.deploys`, so
   `handle_valid_block`'s purge (which removes `body.deploys` from the buffer)
   never removes it. The kept deploy lingers in the buffer.
2. The block creator's pre-state is frequently computed via the **fast path** in
   `compute_parents_post_state` (one parent covers all others), returning a
   covering parent's post-state directly with no merge — and therefore no
   `applied_user` set describing what that base contains.

So a recovered deploy can be re-proposed into a block whose base **already holds
its effect** (because the base descends from a branch that kept it), and the
existing signals do not catch it.

## Why existing gates miss it

- **Buffer membership** (`KeyValueRejectedDeployBuffer`) is event-based
  (added-on-reject, removed-on-`body.deploys`-accept). It cannot express "applied
  on branch A, rejected on branch B": the flip leaves the deploy in the buffer
  even though its effect is in the base on the branch being built.
- **Ancestry paper-trail** (`Validate::is_live_in_ancestry`) keys on inclusion
  vs. rejection *hosts*. Under the flip, every inclusion host of the deploy is
  also a rejection host, so it reports "not live" and admits re-execution — even
  though the effect is in the base via a *different* branch the base descends
  from.
- **`applied_user`** (the merge's kept-set) would answer the question for a
  merged base, but the fast path produces no merge and hence no `applied_user`.

The only signal immune to all three is the **actual contents of the pre-state**.

## The fix

`block_creator::create` already computes the block's pre-state hash
(`compute_parents_post_state`) before executing the body. Between that point and
the deploy-execution checkpoint, the proposer filters the recovered deploys:

> For each recovered deploy, if any of its **own per-deploy cells** is already
> present in the pre-state, the deploy has already executed in this base's
> lineage — skip it. Otherwise it is a genuine loser on this base — execute it.

This restores a base-check that earlier work had removed, but feeds it the
correct signal (pre-state contents) rather than the paper-trail.

### Identifying the deploy's own cells (value-agnostic)

The collision cell is a sig-derived unforgeable channel: a deploy's random seed
is `Blake2b512Random::create_from_bytes(&deploy.sig)` (`Tools::rng`), so the
**channel name** is a deterministic function of the signature and is identical
across re-executions in any block. The cell's **value**, however, is *not* stable
across executions (gas/PoS amounts and map contents depend on the base), so
matching the produced datum by content is unreliable — the deploy's recorded
produce never equals the base datum produced against a different base.

The check is therefore **value-agnostic and keyed on channel presence**, but
restricted to the deploy's **own created cells** to avoid false positives on
shared state:

1. Resolve the block that executed the deploy via the deploy index
   (`lookup_by_deploy_id`); it carries the deploy's `deploy_log` and a per-index
   mergeable-channel map.
2. Take the deploy's number channels for that inclusion
   (`load_mergeable_channels(origin)[deploy_index]`).
3. For each such channel, read the **origin block's pre-state**: if the channel
   was empty there, this deploy *created* it (its sig-unique per-deploy cell — gas
   cell or `new`-site cell). Channels that pre-existed (shared PoS/vault state the
   pre-charge reads) carry data unrelated to this deploy and are excluded.
4. For each created channel, read the **block's actual base**: if it is
   non-empty, the deploy's own cell is already there → the deploy ran in this
   base's lineage → skip.

Every deploy is pre-charged, so it always has at least one such created cell; the
check is never empty and is not tied to any contract or channel layout. The
restriction to created cells is what keeps it false-positive-free: shared cells
(which the pre-charge merely reads) pre-exist in the origin pre-state and are
skipped.

On any error during the check (e.g. a missing mergeable entry), the deploy is
**skipped** conservatively — never re-executed at the risk of a twin — and stays
in the buffer for a later attempt.

## Implementation

- `casper/src/rust/util/rholang/interpreter_util.rs`:
  `recovered_deploy_effect_in_base(dag, block_store, runtime_manager, base_state, sig)`
  — the base-check above. Pure reads (DAG lookup, mergeable map, two history
  readers); no replay.
- `casper/src/rust/blocks/proposer/block_creator.rs`:
  `prepare_user_deploys` exposes the round's recovered sigs; `create` filters the
  selected deploys against the freshly-computed pre-state before
  `compute_deploys_checkpoint`.

## Consensus and determinism

The check is applied **proposer-side**: it only causes the proposer to omit a
deploy whose effect is already present, never to include or alter one. There is
no "missing deploy" validation rule, so an honest proposer's skip is never
rejected by peers. A block that *did* re-execute an in-base deploy is already
rejected by every validator today, because the validation replay
(`validate_block_checkpoint`) re-runs the body and hits the same
single-value-cell error.

All inputs are node-deterministic: the pre-state is a function of
(parents, justifications); the deploy's channels are content-derived from a
stored block; the channel name is a function of the signature alone. So every
node computes the same result.

## Validation

- `fs_seal_must_preserve_both_concurrent_single_value_cell_writes` (the
  concurrent single-value-cell stress test), run 10×: **0 twins, 0 refund
  failures** (both previously occurred on essentially every run). The cell
  remains single-value across all runs.
- `recovery_base_check_skips_only_when_effect_is_in_base` (new focused test):
  exercises both directions on a real executed deploy — base = the deploy's own
  post-state ⇒ skip; base = the pre-state it built on ⇒ execute. Passes
  deterministically.

## Scope and follow-ups

This design fixes the **recovery content-twin / gas-refund** class. Two related
items are intentionally **out of scope**:

- **Validator-side hardening.** The validation replay already rejects a block
  that re-executes an in-base deploy (as a replay error). A symmetric,
  pre-replay base-check that yields a clean `InvalidRepeatDeploy`-class verdict is
  optional defense-in-depth and can be added later.
- **Buffer hygiene (purge-on-keep).** A merge-kept deploy lingers in the buffer
  because it is absorbed from a parent's body rather than the merge's own
  `body.deploys`. Purging it would require surfacing the merge's `applied_user`
  into `handle_valid_block`. With this base-check in place it is unnecessary for
  correctness (a lingering deploy is simply skipped on each check), so it is
  deferred as a bounded-size optimization.

Separately, the **finalized-state convergence regression** (a concurrent
single-value-cell write the merge drops being re-litigated and lost from the
sealed finalized state by floor-lag) is a distinct issue in the merge/seal layer,
not the recovery path, and is not addressed here.
