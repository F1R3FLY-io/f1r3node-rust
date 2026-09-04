---
doc_type: work-log
discovered_by: claude-session-03abbe11
date: 2026-08-22
relevance: [ISSUE-294, ISSUE-317, ISSUE-104, fix/key-contention-base-bias]
evidence:
  - CI run 32549479790, job 96974947890 (amd64-subprocess), artifact integration-logs-amd64-subprocess
  - dev CI runs 32544160782 and 32540926176 (arm64-subprocess), same failure class
---

## Finding

The `test_web_api.py` shared-shard finalization timeouts (`rejection_count` 18 to 24, `dev` fails about one run in two) are not same-key contention between concurrent deploys. Every merge rejects the deploy on the unavailable-split path with `reject: numeric cell would overfill` (`dag_merger.rs`, `numeric_cell_would_overfill`), and the conflict map is empty (`conflictMap entries with conflicts=0`).

The mechanism has two parts.

1. **Test fixture.** `_deploy_and_wait` in `test_web_api.py` deploys `@{2000 + i}!({i})` with one deployer key. Sixteen tests in the shared shard call it, so `@2000`, `@2001`, and `@2002` receive repeated bare integer produces across the suite.
2. **Single-number cell rule.** `numeric_cell_would_overfill` returns true when the base holds exactly one integer datum and the chain adds one more. A base with two or more datums is not "a single number", so it accepts any further produce. The **second** write to a channel is the only hazardous one.

In the failing run, worker `gw0` ran `test_prepare_deploy` (count=3, first write to all three channels), then `test_last_finalized_block` (second write to `@2000`, landed), then `test_get_block` (third write to `@2000`), then `test_get_blocks` (count=3). `test_get_blocks` made the second write to `@2001` and `@2002`. Both deploys were rejected in every merge by every validator for six consecutive recovery rounds. The `@2000` deploy landed because that channel already held two datums. Result: 2 rejected, 1 landed, which matches the log exactly.

The second write lands only when its carrier block is on the main-parent lineage, because the base then already contains the effect and no merge re-applies it. Whether the carrier becomes the base depends on proposer rotation (remedy ladder Option A). That is why the failure is probabilistic and why it appears on different tests in different runs (`test_last_finalized_block` failed on `dev` run 32540926176).

## Why loss priority cannot help

All three phase-1 adjudication sites are loss-aware, including the claim order at `dependency_ordered_branch_items`. They reorder rivals inside one merge. Here there is no rival chain. The chain is stale against every possible base because the first datum is long finalized. Reordering changes nothing.

## Implications

1. **Invariant inconsistency in the node.** The single-number cell protection (the #104 purse double-produce defense) is enforced when the chain is merged as a sibling and not enforced when the chain is on the base lineage. A cell that lands two datums through the base lineage then loses its protection for every later write. The rule should have one answer: either a second bare integer produce onto a one-number cell is always rejected (then PLAY or the base path must reject it too), or the protection applies only among concurrent writers in one merge (then the `base == 1` branch must count producers in this merge, not the base). This is a maintainer decision and belongs on the close-out branch.
2. **Test fixture hazard in `system-integration`.** `_deploy_and_wait` should use a fresh channel per call (for example a per-test nonce) so that the shared suite stops depending on landing order. This is a cross-repo change and needs its own session.
3. **Scope of #317 / C1.** This failure is a base-bias shape, and Option C1 (declare the carrier as `parents[0]`) would land the deploy. C1 would also make the two-datum cell the committed state, which defeats the #104 protection. Pick the invariant answer in item 1 before you pick the C1 remedy.
4. The failure did not start with PR #299 or #312. It reproduces on `dev` at `8b895792f` and `39edf1855`.

## Reproduction recipe

Deploy `@2000!(0)` twice from one key in a three-validator shard with a gap long enough for the first to finalize. Watch `f1r3.trace.unavail` for `reject: numeric cell would overfill` with `base=1 current=1 added=1` on every validator. The second deploy lands only when its carrier becomes the main parent.
