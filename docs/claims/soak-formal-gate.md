# Soak formal gate evidence

```yaml
claim_id: CLAIM-SOAK-GATE-001
status: pending
artifacts:
  - .github/workflows/slashing-tests.yml
  - scripts/ci/check-formal-gate.sh
  - scripts/ci/test-check-formal-gate.sh
  - scripts/ci/check-tla-invariants.sh
  - scripts/ci/test-check-tla-invariants.sh
  - scripts/ci/check-formal-invariants.sh
phase: repository-governance
binding: pending
hosted_verification: passed
hosted_revision: 191e184be556c1f190748143377ab369586c53b6
hosted_run: 35473280388
required_check_enforcement: pending
```

## Authority and history

The [historical specification](https://github.com/F1R3FLY-io/f1r3node-rust/blob/2388a8eedf33d07018f0630bced51a6e054ba439/docs/claims/soak-formal-gate.md) and [prerequisite record](../cbc-evidence/prerequisites/pr432-formal-gate.md) retain earlier results and open obligations.

The user approved the proposed implementation on 2026-09-19: `proceed with your proposal -- approved`.

That approval covers workflow implementation and an explicit local-only revision of the finalized-floor obligation. It does not authorize protection-rule changes.

The retired 256-file digest inventory is not restored. Earlier pending records remain historical evidence, not fresh results for this implementation.

## Required behavior

The existing TLA gate must reject missing configurations, incomplete exploration, incorrect control verdicts, timeouts, and verifier failures.

The `Formal verification gate` job must require exact `success` results from both TLA and Rocq prerequisite jobs. Missing, skipped, canceled, or other results cannot pass.

Each prerequisite records its run, attempt, event, repository, tested commit, workflow commit, workflow reference, and verification source hashes.

The workflow-control hash must match the workflow bytes at the GitHub-supplied workflow commit. The tested checkout must match the GitHub-supplied tested commit.

A successful prerequisite writes a receipt only after verification succeeds. Its tracked verification sources must remain unchanged between input capture and receipt creation.

The separate gate job reconstructs the expected identities and hashes from its own checkout. It requires byte-exact canonical receipts for the same run and attempt.

Missing, malformed, changed, duplicate-key, oversized, or symlink receipts must not pass. Receipts from earlier attempts cannot substitute for current evidence.

Rerun all prerequisite jobs when a new attempt needs new receipts. Reusing a successful receipt from an earlier attempt is intentionally unsupported.

## Trust boundary

This mechanism compares independent job checkouts. It is not a signed supply-chain attestation and does not prove that a hostile workflow is trustworthy.

It assumes trusted GitHub context, runners, checkout tooling, Git objects, and the reviewed workflow and verifier sources.

Hosted review must independently match the run metadata, tested commit, workflow commit, and artifact identities. A local synthetic fixture cannot establish that binding.

Repository protection must require the reviewed gate from the expected GitHub Actions integration. Protection against unauthorized workflow changes remains an administrative policy obligation.

An `always()` condition prevents dependency failure from silently skipping the gate. Whole-run cancellation remains a non-passing outcome, not successful verification.

## Finalized-floor obligation revision

Finalized-floor verification remains local-only under its [documented policy](../casper/theory/finalized-floor/finalized-floor-verification.md).

The historical requirement to add finalized-floor Rocq verification to this workflow is superseded by the approved local-only scope decision.

This decision does not discharge finalized-floor correctness claims. It does not change local proof requirements, node code, or Rocq sources.

The existing CI Rocq suite still checks slashing, fork choice, and rspace guards. Its assertions and theorem counts remain unchanged.

## Completion gates

- [x] Review local refusal tests and workflow wiring checks.
- [x] Verify a successful hosted run and independently reconstruct its source and run identities.
- [x] Preserve unsuccessful hosted outcomes and artifact retrieval evidence.
- [x] Renew Claim001 for the changed workflow and recheck the formal-area documentation contract.
- [ ] Obtain separate approval before changing repository protection rules.
- [ ] Activate the required check only after it is available on the protected baseline.
- [ ] Verify rejection of skipped, canceled, stale, missing, and failing results under the actual protection configuration.
- [ ] Record current enforcement evidence and obtain final claim acceptance.

The [implementation log](../work-logs/soak-formal-gate-implementation.md) records the initial checks. The [hosted renewal log](../work-logs/soak-formal-gate-hosted-renewal.md) records verified execution and retained outcomes.

The protected `dev` baseline does not yet contain the new gate. This claim remains pending until baseline availability, separate protection authorization, enforcement tests, and acceptance are complete.
