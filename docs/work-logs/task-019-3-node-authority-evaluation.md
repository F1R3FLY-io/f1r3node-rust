# TASK-019-3: Node authority evaluation

```yaml
handoff_status: review
base_revision: 4c0c0dbe7
claimed_by: codex-batch-b2-20260923
claim: docs/claims/casper-node-authority-evaluation.md
implementation_authorized: true
acceptance: pending
```

The user requested B2 completion on 2026-09-23.
The request authorizes implementation after the completed planning steps.
PR #447 review 5294038948 accepted the predecessor claims at `4c0c0dbe7`.
B2 acceptance remains separate.

## Work sequence

1. Register the pending claim and mandatory verification records.
2. Implement bounded evaluation work and independent reference decisions.
3. Attach the observer through engine installation and record existing finalizer outcomes.
4. Connect the authenticated authority request to detached capture and evaluation.
5. Run the focused suites, regressions, and source-bound evidence checks.
6. Record remaining acceptance requirements without changing predecessor claim statuses.

## Registration

The claim and seven new mandatory records precede production edits.
The records contain no evidence of claim acceptance.

## Implementation

The implementation follows the 27-file boundary in the batch plan.
The observer attaches once through engine installation.
Ordinary startup keeps an empty attachment cell and creates no observer controller.

The authority request retains the authenticated socket envelope.
It captures once, then evaluates exact, original, and reference results over detached data.
The exact decision uses adopted integer threshold parameters.
The original result preserves floating-point bits.

Live hooks distinguish a derivation, an effect attempt, and an effect return.
A captured finalized row produces a separate persisted observation.
The bounded queue records loss without blocking a finalizer.
Live coverage includes finalizer contexts created after attachment.

The generic work meter preserves ordinary calls through its no-op specialization.
Observer evaluation shares aggregate work limits across separate counter sets.
Budget errors retain partial counters.
The controller rejects overlapping evaluations and requests from a replaced instance.

The reference uses immutable captured metadata and independent traversals.
It enumerates validator subsets instead of using the production clique search.
It derives floors without captured optimization caches when complete ancestry is available.

## Verification status

The trait method, node test initializer, and observer test compilation issues are resolved.
Workspace formatting passes.
Clippy passes with warnings denied across all targets in Casper, node, block-storage, and shared.
The final focused regression checks pass.
The full Casper run exceeded its 30-minute limit and remains incomplete.

The [area review](../../formal/tlaplus/node_observation/README.md#batch-b2-applicability-review) records each property, domain, test boundary, and pending construction requirement.
Tests do not discharge the B2 claim.
Named maintainer acceptance must identify the final source manifest and these remaining requirements.

## Final verification package

The [report](../cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/report.json) records the final source manifest and test results.
External commit d11acabcb contains the B2 implementation.
This package also covers five subsequent work-meter corrections in the working tree.

The corrections charge collection scans and ordering operations at their operation sites.
The ordinary path retains its original sorting implementation.
The bounded observer path stops sorting when its work budget fails.
The ordering test checks all 4,096 six-element inputs over four values.

| Check | Result |
| --- | --- |
| Repository commit checks | Formatting, workspace clippy, chart tests, and dependency checks pass. |
| Casper | 377 unit tests and 15 B2 tests pass. |
| Block storage | All 186 tests pass. |
| Shared storage | 102 unit tests and 19 snapshot tests pass. |
| Node binding | 257 unit tests and 20 interface tests pass. One existing test remains ignored. |
| Existing model gate | 15 positive configurations pass. All 78 negative controls produce their expected violations. |
| Final source check | All recorded source hashes remain unchanged across the final checks. |
| Strict CbC audit | Exit 4. All 13 current B2 records remain pending. |

The final focused checks contain 976 passing test executions.
The model gate checks the existing session and capture models.
It does not establish B2 attachment, work-meter, or independent-reference correctness.

The earlier full Casper run exceeded 1,800 seconds without a reported test failure.
That run preceded the final work-meter correction.
Its output remains supplemental evidence and does not establish a complete regression pass.
The timed-out test left a child process, which was identified by its output file and stopped.

The first model attempt used a TLC build that requires Java 11.
The host has Java 8.
A compatible cached TLC build completed the model gate.
An optional Java 17 download returned HTTP 403, and no downloaded runtime ran.

## Review handoff

TASK-019-3 has review status because Step 7 remains incomplete.
The implementation and focused checks are complete.
The B2 applicability review, construction obligations, semantic binding gaps, and named maintainer acceptance remain unresolved.

The source and claim digest audit passes for all 13 mandatory records.
The records retain links to previous acceptance where applicable.
No B2 record is discharged, and no waiver is recorded.

The full Casper suite needs a longer run before anyone can claim complete regression coverage.
Display and restore limitations remain explicit.
The observer does not qualify a live authority profile.
