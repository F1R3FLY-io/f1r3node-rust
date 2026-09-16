# Ratified Casper Conformance and Soak Plan

**Status.** Planning draft. No implementation or claim discharge is complete.

**Branch.** `formal/soak-casper-consensus`

**Epic.** [EPIC-017](../ToDos.md#epic-017-ratified-casper-conformance-and-soak-evidence)

**Baseline.** `a2fe60c7255bf4ba035d41fb65b6d6f1c0f02632`, the checked-out `dev` baseline on 2026-09-16.

## Authority and scope

The [16 September 2026 meeting record](https://github.com/F1R3FLY-io/f1r3node-rust/pull/390#pullrequestreview-5227717933) controls the selected dispositions.

The PR #216 comments record the [integration decisions](https://github.com/F1R3FLY-io/f1r3node-rust/pull/216#issuecomment-5703891094) and [harness requirements](https://github.com/F1R3FLY-io/f1r3node-rust/pull/216#issuecomment-5703891358).

Current `dev` remains the default Casper authority. A branch proposal does not independently authorize protocol changes.

This branch prepares formal claims, production conformance tests, and separate soak profiles. It does not activate deferred policies or approve a protocol release.

Correct by Construction (CbC) work must distinguish a model result from production conformance. A passing soak does not discharge an unbounded theorem.

## Source inventory

These revisions were observed on 2026-09-16. Every PR was open. None was merged into this branch during preparation.

| Source | Observed revision | Role |
| --- | --- | --- |
| [PR #216](https://github.com/F1R3FLY-io/f1r3node-rust/pull/216) | `619beb4a4a7ad3f8967d4586daf0f5c552bd150e` | Candidate accounting and Casper implementation, subject to the ratified dispositions |
| [PR #390](https://github.com/F1R3FLY-io/f1r3node-rust/pull/390) | `ce266ddcd8369107441dd9fc9b8e9ff6719146f1` | Historical comparison ledger and published meeting review |
| Local ratification commit | `f26c975234752f516ea45be7c64005336b75d8c5` | Updated ledger on `chore/rectify-cost-accounting-casper-specs`, not yet at the observed remote PR head |
| [PR #430](https://github.com/F1R3FLY-io/f1r3node-rust/pull/430) | `dedb3add172098efcbc63d72b2ebdc612f2fddcd` | Phlo-bound deploy storage model |
| [PR #431](https://github.com/F1R3FLY-io/f1r3node-rust/pull/431) | `0e176e486a028d10704b96add6e5eb50525682cb` | Disk-protected soak driver and fixtures |
| [PR #432](https://github.com/F1R3FLY-io/f1r3node-rust/pull/432) | `e0380392bcc66d9774e403edb8a08415798c1e0e` | Soak models, registered negative controls, and bounded CI tier |
| [PR #433](https://github.com/F1R3FLY-io/f1r3node-rust/pull/433) | `65f7f6daa832c0acb6fddf2b462db1b9d5461729` | Machine/medium separation and CbC verification tiers |

The stack order is `#430 -> #431 -> #432 -> #433`. Preserve that order when integrating reviewed prerequisites.

PR #431 names B44 containment as open. A prototype containment launcher is not evidence that the normal soak workflow provides containment.

The source PRs report verification results. Those reports are inherited evidence, not test results from this branch.

## Decisions and acceptance obligations

| Decision | Required disposition | Evidence or activation condition | Task |
| --- | --- | --- | --- |
| D-01 | Use one Casper protocol-7 authority chain and fresh genesis. Keep reusable accounting authority version 8 separate. | FIPS approval, ceremony/adoption/reception agreement, unsupported-version refusal | TASK-017-11 |
| D-02 | Preserve upstream committee and stake provenance, exact justifications, duplicate rejection, and validator signatures. Do not require certificate sidecars. | Integration tests must pass before removing coupled certificate code. Cover replay, settlement, restart, dependencies, and finalization. | TASK-017-5 |
| D-03 | Preserve GHOST, electorate, stake provenance, depth rules, truncation, and progress. Bound LCA traversal at the finalized floor. | Compare bounded and reference traversals on identical admissible DAGs. A changed valid head fails the equivalence claim. | TASK-017-5 |
| D-04 | Preserve containment, inclusive threshold, strict majority precondition, disagreement rules, budgets, and divergence telemetry. Reject the second state certificate. | Test missing-history holds, exact boundaries, state retention, and verified failed-body settlement. Do not authorize universal certified advancement. | TASK-017-5 |
| D-05 | Require atomic publication, stale-result refusal, restart atomicity, durable terminal eviction authority, and distinguishable FT projections. Keep single-flight finalization. | Crash and concurrency tests must precede optional parallel evaluation. The complete ledger architecture remains optional. | TASK-017-6 |
| D-06 | Preserve all-eligible stale recovery, leader-only convergence, readiness lanes, and frontier follow. Adopt intents, coalescing, and permit revalidation. | Test rotating stale leadership, frontier removal, and progress-clock replacement as deferred experiments only. | TASK-017-7 |
| D-07 | Preserve exact occurrences, tombstones, reason joins, carrier custody, lifespan, causal closure, retry pacing, and one-parent coverage. | Compare collective coverage and leader-free custody separately. Lease expiry must not bypass the floor gate or validity rules. | TASK-017-7 |
| D-08 | Select additive composition conditionally. Preserve exact independent multiplicity, causal closure, local evidence authentication, admission alignment, and checked arithmetic. | FIPS approval, fresh genesis, compatibility analysis, differential tests, and accounting conservation must precede activation. | TASK-017-8 |
| D-09 | Preserve upstream slashing authorization, recovery, epoch protection, inactive economic neglect, and Rust-to-Scala bisimilarity. | Canonical reconstruction remains supplementary until differential tests prove complete coverage. | TASK-017-9 |
| D-10 | Preserve the dedicated carrier store, watermark, fallback, retention, and narrow fast path. Select a protocol-7 typed lookup identity. | The FIP defines the envelope commitment. Keep CLAIM-FINALITY-002 pending until differential and soak gates pass. | TASK-017-10 |
| D-11 | Preserve all five practice rules, gate replacement governance, negative controls, mandatory scope, and the slashing proof anchor. | Use truthful 2,000/10,000/100,000-or-more property tiers. Require claim scripts in workflows and the pending scan benchmark. | TASK-017-2, TASK-017-4, TASK-017-13 |
| D-12 | Require both signed `phloLimit` and `phloPrice`. Preserve prepayment, refund, exhaustion, minimum-price validation, tags, and APIs. | Token accounting cannot replace either field. Multi-wallet funding needs a normative mapping before activation. | TASK-017-11 |

The selected D-12 disposition rejects field removal. Do not implement the earlier proposed replacement with a token-only client maximum.

## Existing epic review

The review covered all twelve active epic blocks and the completed-epic index. Existing task states are historical records, not proof of current implementation status.

| Existing work | Relationship | Disposition for this branch |
| --- | --- | --- |
| EPIC-010 | Soak metrics, reporting, runner lifecycle, and dashboard | Reuse the evidence infrastructure. Do not duplicate reporting or silently close its tasks. |
| EPIC-012, TASK-012-19 through TASK-012-21 | Frontier instrumentation and bounded work | Share measured work counters and baseline fixtures. Do not infer a bottleneck from total latency. |
| EPIC-012, TASK-012-22 and TASK-012-23 | Exhaustive formal profiles | Preserve timeout, violation, tool error, and success as distinct outcomes. |
| EPIC-013 | Exact-candidate release evidence | Produce compatible evidence, but do not dispatch promotion or change release authority. |
| EPIC-015 | Casper test-node congruence | Audit the available production-shaped fixture boundary before adding a second helper tree. |
| EPIC-016 | Contention, retry coverage, protected cells, and concurrency | Reuse applicable regressions. Reconcile stale C1 and wire-policy assumptions against D-03, D-07, and D-08. |
| EPIC-003 through EPIC-009 | Migration and external testbed work | Leave unrelated claims and states unchanged. |
| EPIC-014 | Continuous test net | Do not make this branch depend on test-net provisioning for deterministic conformance. |
| Completed EPIC-011 | Exhaustive TLA+ baseline | Retain its distinction between bounded completion and unresolved exhaustive coverage. |

The shared parser rejects the repository's existing `review` task status in strict mode. Compatibility mode preserves those records during inspection.

Do not relabel those tasks merely to satisfy the parser. TASK-017-1 records this tooling mismatch for a separate resolution.

No `docs/handoffs/` directory exists at this baseline. Relevant work logs include the issue-24 fast path and the 60-hour preflight handoff.

## Integration sequence

1. Review this epic and its authority map.
2. Define the claim inventory and reference oracles before changing production code.
3. Integrate reviewed prerequisites in stack order with separate Git approval.
4. Bind all evidence to exact node, model, harness, and configuration revisions.
5. Run deterministic baseline conformance before comparative soak profiles.
6. Exercise deferred policies only in isolated experimental builds or test fixtures.
7. Submit evidence for review before any deferred policy becomes an activation candidate.

PR #390 documentation publication and PR #216 implementation alignment remain separate prerequisites. Neither requires adopting PR #216 wholesale.

Protocol-changing comparisons need separate fresh-genesis shards. Do not mix incompatible block formats or node-local validity switches within a shard.

Missing candidate implementations produce a blocked profile, not a skipped success. Ordinary `dev` baseline tests do not wait for protocol-7 activation.

## CbC claim inventory to prepare

TASK-017-2 must create or refine claims before the related code changes. The following identifiers are proposed, not discharged.

| Proposed claim group | Scope | Required evidence |
| --- | --- | --- |
| `CLAIM-CASPER-SOAK-001` | Evidence provenance and experiment isolation | Manifest validation, missing-artifact failures, baseline/candidate identity checks |
| `CLAIM-CASPER-SOAK-002` | D-02 through D-04 authority, floor bounds, and preservation | Cross-view model, negative controls, exact-threshold tests, LCA oracle, production bridge |
| `CLAIM-CASPER-SOAK-003` | D-05 publication and lifecycle eviction | Crash/restart model, stale-result control, interleaving tests, durable-state bridge |
| `CLAIM-CASPER-SOAK-004` | D-06 and D-07 recovery experiments | Explicit scheduling assumptions, paused validators, custody controls, retry/expiry measurements |
| `CLAIM-CASPER-SOAK-005` | D-08 execution and merge accounting | Multiplicity and causal-closure proofs, authenticated evidence tests, conservation bridge |
| `CLAIM-CASPER-SOAK-006` | D-09 slashing equivalence | Objective-evidence oracle, rebond and forged-evidence controls, recovery coverage |
| `CLAIM-CASPER-SOAK-007` | D-01 and D-12 version and Phlo requirements | Version lifecycle controls, retained-field tests, replay and funding-boundary tests |
| Existing `CLAIM-FINALITY-002` | D-10 carrier index | Forced-path differential, watermark and crash controls, measured absence-path work bound |

Reuse existing claims when their statements match the ratified requirement. Do not discharge an old claim against a different property.

Each claim must name authenticated inputs, outputs, assumptions, boundedness, negative controls, production functions, and required evidence tiers.

### Verification tiers

The [PR #433 tier proposal](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/cbc-verification-tiers.md) separates three forms of evidence:

- Refutation: TLA+ with TLC checks a stated finite model and expected counterexamples.
- Construction: Rocq and kernel checks establish unbounded claims with an explicit assumption set.
- Binding: production tests connect the actual implementation to the model or theorem.

Soak infrastructure remains separate from Casper semantics. Shell-driver claims use model checks and fixtures, not an invented Rust construction theorem.

The [PR #433 architecture note](https://github.com/F1R3FLY-io/f1r3node-rust/blob/65f7f6daa832c0acb6fddf2b462db1b9d5461729/docs/artifacts/f1r3fly-consensus-neutral-sm.md) separates execution, DAG substrate, Casper, and shared infrastructure.

Preserve that separation in claim ownership. Do not add RGB, Casanova, or Cordial implementation to this branch.

### Scope and tooling findings

The `/cbc identify` baseline probe found mandatory attributes on these relevant artifacts:

- `casper/src/rust/finality/floor.rs`
- `casper/src/rust/blocks/proposer/block_creator.rs`
- `casper/src/rust/validate.rs`
- `block-storage/src/rust/dag/carrier_index.rs`
- `block-storage/src/rust/dag/block_dag_key_value_storage.rs`
- `node/src/rust/instances/heartbeat_proposer.rs`

The probe found no `cbc` attribute on `estimator.rs`, `dag_operations.rs`, the soak driver, or the TLA+ gate script. Review their scope before modifying them.

Existing waivers and discharge records are not automatic evidence for new claims. The status command reports multiple entries for `interpreter_util.rs`, which needs a record audit.

The repository has no local `scripts/cbc.sh`. The shared CbC skill supplies the driver. Do not add a copied framework implementation to this repository.

Java, the TLC jar, and Z3 are present. Rocq and Coq are not on the current PATH. Tool presence does not establish a verified run.

### Known specification conflicts

The draft [CbC repair plan](../casper/design/cbc-repair-plan.md) still requires a state-retention certificate and rotating recovery behavior.

Those statements conflict with D-02, D-04, and D-06. TASK-017-2 must reconcile them before they become new claim assumptions.

The [cross-view leader claim](../claims/consensus-cross-view-determinism.md) needs an explicit lane scope. It must not reintroduce leader-only stale recovery.

D-11 keeps expected violations outside the positive configuration list. PR #432 adds a separate expected-violation registry, which is compatible with that distinction.

The PR #432 description states a 15-minute bounded workflow tier. PR #433 describes a two-minute refutation tier. Reconcile these budgets before registering new checks.

## Evidence profiles

| Profile | Scenarios and required observations |
| --- | --- |
| Authority and finality | Missing dependencies, replay, settlement, restart, committee provenance, inclusive threshold equality, strict majority, divergent local views, retained finalized effects |
| Fork choice | Identical admissible DAGs, dense heartbeats, deep ancestry, bounded versus reference LCA, unchanged head, explicit traversal counters |
| Publication | Crash before and after publication, stale evaluations, terminal verdict persistence, no torn block/root/effect tuple, distinguishable FT projections |
| Recovery | Frontier follow on/off, all-eligible/rotating recovery, clock variants, paused/delayed validators, empty/deploy workloads, one-parent/collective coverage, leader-free custody |
| Merge | Independent identical outputs, repeated observations of one identity, inconsistent identity evidence, causal rejection, failed execution, admission rejection, overflow, conservation |
| Slashing | Merge-lost slash, both evidence orders, same-key rebond, stale epochs, missing dependencies, forged deploys, duplicates, restart |
| Carrier index | Forced index/reference paths, domain collisions, watermark/pruning boundaries, read failures, missing bodies, forks, valid/invalid/approved carriers, publication crashes |
| Version and Phlo | Ceremony/adoption/reception, unsupported versions, both signed cost fields, minimum price, prepayment/refund/exhaustion, replay, undefined multi-wallet policy refusal |

The recovery report must include completion and expiry rates, duplicates, custody consistency, recovery time, blocks per height, latency, and resource use.

Do not derive a causal explanation from latency alone. Record traversal work, index probes, ancestor reads, merge work, and replay work separately.

## Completion gate

Each result must include revisions, configuration, seed, run identifier, tool version, bounds, assumptions, artifact digests, and terminal outcome.

Timeout, cancellation, missing evidence, and tool failure are not passing results. Infrastructure failure does not erase an earlier product failure.

Use `/cbc identify`, `/cbc verify`, and `/cbc discharge --strict` for the applicable artifact scope. Record unavailable verifiers as gaps, not mock proof.

A clean documentation diff does not discharge planned runtime claims. Final closure requires model results, construction evidence where applicable, production bridges, and completed soak evidence.

Do not mark the epic complete or close related issues while required claims remain pending or refuted.
