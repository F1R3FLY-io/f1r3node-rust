# Merge Recovery Validation Plan

## Purpose

This branch validates the corrections developed on `fix/merge-recovery-finalization` without applying those corrections to `dev`. The assertions are intentionally written against the corrected behavior and are expected to fail until the corresponding production changes are implemented.

## Baseline and provenance

- Validation baseline: `origin/dev` at `e71dd897`
- Preserved source branch tip: `backup/test-asi-chain-validations-f584e9e` at `f584e9e9`
- Source pull request: [F1R3FLY-io/f1r3node-rust#114](https://github.com/F1R3FLY-io/f1r3node-rust/pull/114)
- Primary defect: [F1R3FLY-io/f1r3node-rust#71](https://github.com/F1R3FLY-io/f1r3node-rust/issues/71)
- Historical RCA identifier: `RCA-asi-devnet-finality-halt`

## Validation matrix

| Area | Corrected behavior | Validation |
| --- | --- | --- |
| Finalized state | A later multi-parent merge cannot remove an effect already present at a finalized cut | Multi-validator single-cell convergence and finalized-state monotonicity |
| Merge base | Multi-parent merge is based on a node-deterministic finalized floor | Parent post-state invariance and deep-DAG soak |
| Merge scope | Scope remains bounded as DAG height grows and never silently drops parent effects | Deep-DAG scope and 400-block soak |
| Mergeable state | Missing locally materialized mergeable entries are reconstructed from their source blocks | Missing-mergeable reconstruction regression |
| Single-value cells | Concurrent writes keep one deterministic value and recover the rejected writes | Two- and three-writer convergence regressions |
| Produce-only overfill | A number cell never commits more than one datum after a produce-only conflict | Single-value number-cell integration assertions |
| Integer arithmetic | Merge arithmetic rejects terminal overflow instead of laundering it through wrapping state | Integer-add overflow unit validation |
| Deploy ordering | Distinct deploy chains never compare equal | Strict-total-order unit validation |
| Recovery | Merge-rejected deploys remain observable, retryable, and cannot be double-applied | Rejected-buffer and deploy-status lifecycle tests |
| Finalization cleanup | Included and rejected deploys are purged only when terminal; recoverable work remains available | Finalizer cleanup tests |
| Finalizer effect | A finalized candidate ahead of the persisted LFB still invokes the finalization effect | Finalizer effect regression |
| Counter deploys | Concurrent counter updates recover and finalization continues | Counter and map-cell integration scenarios |
| Invalid evidence | Invalid justifications from an obsolete validator epoch are not slash-obligating | Stale-invalid-justification unit validation |
| Slash matching | An unrelated slash deploy cannot excuse a current invalid justification | Matching-slash unit validation |
| Slash replay | Slash replay is derived from block-visible evidence and is node deterministic | Slashing regression suite |
| Rebonding | Unbonded observation order cannot pollute evidence and create divergent neglect verdicts after rebonding | Equivocation detector convergence validation |
| Deploy leadership | Only the deterministic inclusion leader packages ordinary user deploys while unresolved user work is in the frontier | Multi-validator packaging validation |
| Fork choice | Invalid bounds and arithmetic return typed failures without nondeterministic fallback | Fork-choice boundary validations |

## CI tiers

### Per-change

Normal Rust unit and integration jobs execute the validation modules.

**Re-enablement (this fast-follow PR):** the five expected-red annotations
introduced on 2026-07-14 are removed — every validation assertion runs
unconditionally in per-change CI again. This PR merges only when the
remaining corrections for `three_writers_converge_under_load`,
`unresolved_user_frontier_has_one_deploy_inclusion_leader`,
`recovery_cycle_rejected_deploy_retries_while_source_is_visible`,
`bridge_query_survives_multi_parent_merge`, and
`stale_diff_application_corrupts_merged_state` have landed (issue #71) and
its CI is green. Known failures are once again not ignored, inverted, or
marked as expected failures.

### Daily soak

Monday through Thursday at 22:00 Pacific, Oracle Cloud runs a minimum 24-hour integration soak. A newly scheduled or manually dispatched run cancels the prior soak run.

### Weekend soak

Friday at 22:00 Pacific, Oracle Cloud runs a minimum 72-hour integration soak. The same replacement policy prevents duplicate weekend runs.

The schedule uses paired UTC cron entries and an `America/Los_Angeles` runtime gate so the start time remains 22:00 across daylight-saving transitions.

## Overnight workload

The long-running job repeatedly executes the trusted, pinned system-integration load scenario against both Docker and subprocess providers. Each iteration creates a multi-validator shard, applies sustained concurrent deploy load, verifies deploy finalization, checks LFB convergence, checks node liveness, and archives the resulting logs. Failures are accumulated without shortening the requested soak duration; the job exits nonzero after the deadline if any iteration failed.

## Exit criteria

A correction is considered validated when its named test changes from red to green without weakening the assertion. The validation branch is complete when every matrix row has an executable assertion, CI invokes the complete suite, and the daily/weekend soak workflows can be manually dispatched and scheduled.
