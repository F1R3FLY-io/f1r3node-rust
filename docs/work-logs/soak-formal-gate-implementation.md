# Soak formal gate implementation

## Authority and scope

The user approved the proposal on 2026-09-19: `proceed with your proposal -- approved`.

The approval covers workflow implementation, regression checks, provenance records, and the local-only finalized-floor obligation revision. Protection-rule changes remain a separate approval step.

The implementation starts from `f5ed8c198f18fb7b44fddbb0e8c3fe2185465087`. No commit, push, workflow dispatch, ruleset update, or node campaign occurs in this work.

The restored [CLAIM-SOAK-GATE-001 specification](../claims/soak-formal-gate.md) records the current obligations. The claim remains pending.

## Current repository observations

The effective `dev` rules require multiple CI checks, but not `TLA+ invariant check`. The older statement that only `Lint` is required is historical.

The classic branch-protection endpoint returns 404. Active repository rulesets still apply, so that response does not mean `dev` has no protection.

Hosted run `35469963983` succeeded at `f8623a54ee7edb272b71c7993379b39818412351`. It predates this implementation and cannot verify it.

These observations do not change a ruleset or establish success-only enforcement. Final verification must repeat the effective-rule query.

## Implementation

`slashing-tests.yml` adds a `Formal verification gate` job with an `always()` condition. It depends on the existing TLA and Rocq jobs.

`scripts/ci/check-formal-gate.sh` rejects every prerequisite result except exact `success`. It also produces and verifies canonical JSON provenance receipts.

Each producer checks out the GitHub-supplied tested SHA. It records the workflow-control hash from the GitHub-supplied workflow SHA and checks the local workflow bytes.

The input record binds repository, event, run, attempt, tested SHA, workflow SHA, workflow reference, and tracked verification sources.

A producer creates its success receipt only after verification. Input drift, untracked verification inputs, and a changed checkout cause refusal.

The independent gate job downloads receipts named for the current run and attempt. It reconstructs their expected contents from its own checkout.

The consumer requires byte-exact canonical JSON. It rejects missing, malformed, changed, duplicate-key, oversized, and symlink receipts.

Provenance uploads run even after failure. A partial input record without a successful job and receipt cannot pass the gate.

New attempts require receipts from every prerequisite in that attempt. Rerun all jobs instead of reusing earlier successful receipts.

## Limits

The local tests use synthetic Git responses and isolated fixture files. They do not establish hosted checkout or artifact-service behavior.

Independent job comparison is not signed supply-chain attestation. Trusted runners, GitHub context, Git objects, reviewed workflow code, and tooling remain assumptions.

A malicious workflow can alter its own checks. Administrative control of workflow changes and the required-check source remains necessary.

An always-running dependency gate does not turn whole-run cancellation into success. Actual protection behavior still needs verification after separate authorization.

The existing driver-binding job remains separate. No runtime assertions, model controls, theorem counts, or production-driver containment change.

## Local checks

- Shell syntax checks pass for both new scripts.
- Positive receipt round trips and workflow wiring checks pass.
- Forty-seven exact-exit refusal controls pass.
- The shared formal regression passes for all 71 registered negative controls and event routing.
- Active language-server checks report no diagnostics for the two scripts and workflow.

The first shared-regression command exceeded the 120-second tool limit. Its empty log remains retained as an incomplete attempt.

A retry with a 600-second limit completed successfully. No assertion or expected count changed.

The [implementation evidence](../casper/cbc-evidence/runs/soak-formal-gate-implementation-20260919-01/report.json) records source identities and these results.

## Claim001 renewal

The workflow change invalidates its earlier Claim001 binding. Claim001 now has pending status and binding, while Claims002 through008 retain their separate discharges.

The ordinary eight-claim audit exits 0 and reports Claim001 pending. The strict audit exits 4, as required.

The dependent formal-area documentation contract also returns to pending. Earlier accepted reports and their source identities remain unchanged.

The README and plan still describe the earlier accepted source snapshot. Their current consistency review must wait for renewed workflow acceptance.

This pending state must block campaign qualification. A generic artifact status cannot substitute for the strict source-bound claim audit.

## Finalized-floor decision

The approved revision keeps finalized-floor verification local-only. This scope decision supersedes the historical demand to add its Rocq build to this workflow.

The decision does not discharge a finalized-floor correctness claim. The existing local verification script and proof sources remain unchanged.

## Remaining steps

1. Review and commit the implementation with separate Git authorization.
2. Publish the commit with separate push authorization.
3. Inspect fresh hosted prerequisite results, receipts, and the final gate result.
4. Independently reconstruct the hosted run and source identities.
5. Renew Claim001 and recheck the dependent documentation contract.
6. Obtain protection-rule authorization after the gate is available on the protected baseline.
7. Preserve existing required checks when adding the new gate from the expected GitHub Actions integration.
8. Verify actual rejection of missing, skipped, canceled, stale, and failing results.
9. Record acceptance before discharging CLAIM-SOAK-GATE-001.

Local evidence is under `target/soak-formal-gate-20260919-01/`. Preserve the incomplete attempt and successful retry before cleanup.
