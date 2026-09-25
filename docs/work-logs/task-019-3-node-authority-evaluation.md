# TASK-019-3: Node authority evaluation

```yaml
handoff_status: ready
base_revision: 4c0c0dbe7
claimed_by: codex-batch-b2-20260923
claim: docs/claims/casper-node-authority-evaluation.md
implementation_authorized: true
acceptance: accepted
acceptance_record: https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948
acceptance_revision: 237e43d723b9867985cd47fdfe312fd8d06352e8
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

## Applicability review and construction cycle on 2026-09-23

The user requested the applicability review and the construction obligations after the implementation landed at `d11acabcb` and the source-bound package at `d69e12151`.

### Applicability review

The area README now classifies all 15 B2 properties with a class, a refutation, a construction, a binding, and a decision column. C1 and C13 propose a bounded-by-design classification with a named finite domain. The other 13 properties are unbounded and require construction.

C7 and C8 inherit the accepted session and capture theorems from claims 001 and 002. Their extension to the authority operation awaits acceptance.

### Construction tier

A new project `formal/rocq/node_authority` exports 15 theorems. It models once-only attachment, installation coverage, the nonblocking event ledger, the shared sticky work budget, checked overflow, and the digest guard on result comparison.

The build passed in the resource-limited container, `coqchk` reported that the modules were successfully checked, and all 15 assumption sets reported `Closed under the global context`. The formal gate registers the project and requires 15 closed sets. The accepted `node_observation` project is unchanged.

C3 and C5 have complete construction. C4, C9, and C12 have partial construction with a named remaining part. C2, C6, C10, C11, C14, and C15 keep pending construction with Rust tests only.

### Binding tier

The bindings manifest gains a claim 003 section that maps every property to named tests, theorems, and gaps. All test references resolve to source declarations.

### Records and package

The five project files and the formal gate script join the claim 003 inventory with mandatory tags. The package `casper-node-authority-b2-d69e12151-02` records the tiers reached and names the source-bound package as its previous cycle. Every claim 003 record cites the new package. The formal gate script record keeps the acceptance of claims 001 and 002 beside its current digest.

The task stops at the acceptance gate. The named maintainer must review each applicability decision and accept claim 003 by naming the revision, the claim ID, and the package.

## Acceptance on 2026-09-23

The named maintainer `jltatbeach` accepted claim 003 by editing PR #447 review 5294038948 in place. The appended text names the claim, revision `237e43d72`, and the package `casper-node-authority-b2-d69e12151-02`, and accepts C1 and C13 as bounded by design, the construction gaps and remaining parts as recorded, and the C7 and C8 extension.

The review was verified against origin at 2026-09-23T21:01:02Z. Revision `237e43d72` is the first that contains the package and the `NodeAuthority` project.

This recording sets the claim file to accepted, marks every B2 decision as accepted in the area README, and discharges all 19 claim 003 records with the review as acceptance evidence. The six records that also carry the acceptance of claims 001 and 002 keep both acceptances. The strict audit over the claim 003 inventory now exits 0.

The area README, the project README, and the claim file changed in this recording. Each affected record keeps the acceptance-time digest beside the current one. The package directory is unchanged. TASK-019-3 is complete.

