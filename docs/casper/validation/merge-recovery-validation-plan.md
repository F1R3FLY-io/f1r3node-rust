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
| Recovery authorization | A rejected source authorizes retry only after every visible source occurrence is tombstoned | Exact-source reducer examples, observation-order properties, and TLA⁺ negative control |
| Recovery expiry | Rejected history never extends the ordinary deploy lifespan; expired records leave both local stores | Exact-boundary unit tests and TLA⁺ expiry-bypass counterexample |
| Recovery custody | The source carrier owner packages its rejected-buffer work. Different owners can retry independent carriers concurrently. | Owner-custody regressions, Loom concurrency tests, and TLA⁺ source-distinct retry model |
| Recovery packaging | A retry selected by the exact-source reducer survives self-chain filtering while ordinary self-chain duplicates remain excluded | Unit partition test, D3 vault-conflict end-to-end test, and TLA⁺ packaging negative control |
| Recovery liveness | An unavailable carrier owner delays only that owner's retry. Ordinary heartbeat leadership continues to rotate. | TLA⁺ temporal properties, Loom owner-independence test, and heartbeat system-integration scenario |
| Approved-state bootstrap | A late node reconstructs every historical root from the immutable consensus context serialized by that block, never from its current approved tip or local shard configuration | `ApprovedStateReplay` safe/unsafe models, axiom-free `BootstrapReplayContext`, exact replay unit regressions, and the late-checkpoint epoch-change integration scenario |
| Local validation faults | Storage, unavailable-block, unavailable-root, and busy failures remain inconclusive local outcomes: certification preserves the exact block hash or state root; genesis-rooted absence remains local while truncated-history absence remains a dependency; the buffer retains custody; same-artifact requests deduplicate; distinct validators recover independently; no path creates slash evidence or releases an ordinary descendant from the wrong artifact | Concurrent `LocalValidationRecovery` safe model exhausted by TLC and bounded independently by Apalache; immediate-requeue, identity-collapse, drop, and false-invalidity controls under both checkers; axiom-free typed `LocalFaultDeferral`; exhaustive Loom request/release races; Rust typed-round-trip, idempotent-ack, certificate-sidecar, block/state retry, and descendant-gating regressions; forbidden-log assertions |
| Consensus inputs | Missing parent bodies, visible disposition bodies, or finalized metadata stop local proposal processing instead of selecting a fallback disposition | Fail-closed reducer, ancestry, and leader tests |
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
`unresolved_user_frontier_fresh_admission_is_bounded_and_disjoint (reconciled with the bounded fresh-admission semantics)`,
`recovery_cycle_rejected_deploy_retries_while_source_is_visible`,
`bridge_query_survives_multi_parent_merge`, and
`stale_diff_application_corrupts_merged_state` have landed (issue #71) and
its CI is green. Known failures are once again not ignored, inverted, or
marked as expected failures.

### Daily soak

Monday through Thursday at 19:30 Pacific, Oracle Cloud runs a 22-hour integration soak. A newly scheduled or manually dispatched run cancels the prior soak run.

The duration is 22 hours rather than 24 deliberately. Soak runs share a concurrency group with `cancel-in-progress`, so a 24-hour soak on a 24-hour cadence is cancelled by the following night's launch shortly *before* it finishes — losing its final iteration and its entire artifact upload. Ending at 22 hours leaves the run time to report before the next one starts.

Two conditions can shorten or skip a daily soak:

- **Nothing new to test.** If no commits landed on the branch under test since the previous window, the run is skipped rather than re-soaking already-soaked code. The check fails *open*: any API error or unparseable response runs the soak, because a silently skipped soak is the failure this workflow exists to prevent.
- **The branch moved.** A soak pins one SHA at checkout and tests that image for its whole run, so once the branch advances it is measuring history. After a floor of 8 hours, the soak stops at the next iteration boundary when the tip changes, recording `early_exit_reason=target_advanced` in its summary. The floor stops an early merge from reducing a night to a token soak; before it, merges are ignored.

### Weekend soak

Friday at 19:30 Pacific, Oracle Cloud runs a 60-hour integration soak, finishing at 07:30 Pacific on Monday. The same replacement policy prevents duplicate weekend runs.

Neither shortening condition applies to the weekend run. It always launches, and merge-triggered exit is disabled, because its numbers are the week-over-week benchmark baseline and are only comparable if every run covers the same span.

The schedule uses paired UTC cron entries and an `America/Los_Angeles` runtime gate so the start time remains 19:30 across daylight-saving transitions. The gate matches on hour *and* minute, and 19:30 Pacific falls on the following UTC day, so the cron entries read 02:30 and 03:30 UTC.

## Overnight workload

The long-running job repeatedly executes the trusted, pinned system-integration load scenario against both Docker and subprocess providers. Each iteration creates a multi-validator shard, applies sustained concurrent deploy load, verifies deploy finalization, checks LFB convergence, checks node liveness, and archives the resulting logs. Failures are accumulated without shortening the requested soak duration; the job exits nonzero after the deadline if any iteration failed.

## Exit criteria

A correction is considered validated when its named test changes from red to green without weakening the assertion. The validation branch is complete when every matrix row has an executable assertion, CI invokes the complete suite, and the daily/weekend soak workflows can be manually dispatched and scheduled.

For deploy recovery, completion additionally requires all validators and the
read-only node to report the same exact source block hash for the deploy. The
test must also observe continued LFB progress after one carrier owner is paused,
zero `ContainsExpiredDeploy` proposal failures, bounded occurrence/tombstone
cardinality for one deploy signature, and no host-protection breach. A timeout,
an increased RSS ceiling, or agreement on status without agreement on source
hash is not a passing substitute.

For approved-state bootstrap, completion additionally requires a validator to
join after the approved checkpoint has advanced beyond the funded epoch
transition, reconstruct every declared root, enter `Running`, and continue to a
later finalized block with zero `UnknownRootError`, `InvalidTransaction`,
invalid-block recording, replay retry, or local-validation-fault entries. A
recovery request that merely suppresses those logs without reconstructing the
same roots is not a passing substitute.
