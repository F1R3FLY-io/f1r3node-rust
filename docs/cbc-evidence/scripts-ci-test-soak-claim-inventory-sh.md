# CbC Evidence: Soak Claim Inventory

**Claim:** [CLAIM-SOAK-GATE-001](../claims/soak-formal-gate.md)

**Status:** Pending. Cycle G0/B3 validates inventory records. It does not discharge the gate implementation or any production claim.

**Base revision:** `43af06dab41ddf81b8dd1fcc5cf9e1303027aab7`

**Candidate identity:** The [manifest](soak-g0-b3-2026-09-08/manifest.jsonc) binds the inventory, checker, workflow, and retained evidence.

## RED

The [checker](../../scripts/ci/test-soak-claim-inventory.sh) found no candidate-bound inventory for the required claims.

```text
FAIL: Required soak claims lack a candidate-bound inventory.
```

The [RED log](soak-g0-b3-2026-09-08/red.log) records exit code 1. This is an inventory-contract failure, not a missing verifier or a production finalization failure.

The public interface is the inventory file. The correction adds the missing records, not a new consensus transition or resource model.

## GREEN

The [inventory](../claims/soak-claim-inventory.jsonc) identifies four required claims and their current input digests. Each claim maps to implementation symbols, baseline checks, evidence identities, and pending obligations.

The check records identify commands, configurations, verifier versions or known gaps, assumptions, finite bounds, production tests, and CI jobs.

The inventory retains the declared status of each existing claim. It does not extend the historical settled-probe discharge to the current candidate.

All nine prevention gates remain pending. The acceptance identity remains explicitly unknown.

The [GREEN log](soak-g0-b3-2026-09-08/green.log) records the valid inventory. The existing `Lint` job now runs this check without running a formal verifier.

Ten altered-record checks confirm rejection of missing claims, missing checks, missing evidence, and stale production, specification, or model digests. They also reject skipped checks, false discharge, incomplete identity, and false acceptance.

The [rejection log](soak-g0-b3-2026-09-08/rejection-checks.log) records those results. These checks validate the inventory contract, not GitHub enforcement or formal truth.

The checker validates this pending inventory schema only. It is not a general claim-discharge or release-acceptance evaluator.

## Hosted B2 confirmation

The [hosted observation](soak-g0-b3-2026-09-08/hosted-observation.jsonc) records these separate identities:

| Field | Value |
| --- | --- |
| PR head | `43af06dab41ddf81b8dd1fcc5cf9e1303027aab7` |
| Actual synthetic checkout | `bad72c4c299b73ede9abb1b5adc0f70fbdfb8e7b` |
| Run and attempt | `34228660038`, attempt `1` |
| Formal job | `102069244605` |
| Artifact | `10056856375` |

The formal job completed successfully in 33 seconds, including setup and upload. The two positive configurations passed, and both controls produced their exact expected invariant violations.

The downloaded archive digest matched the artifact API digest. The evidence directory retains all four TLC logs and the selected API responses.

The actual checkout supplied the same model, configuration, gate, and workflow bytes as the local baseline. The observation records their SHA-256 digests.

This result does not establish success of every CI job. The workflow-control SHA was not independently retained, and this is not a tested node image identity.

The earlier formal job for PR head `3fee1ca8a` also passed. The retained evidence uses the later merged candidate rather than treating ancestry as acceptance.

## Enforcement and remaining obligations

The visible `dev` rules require `Lint`, but they omit `TLA+ invariant check`. The classic required-check endpoint returned HTTP 403.

Required-check enforcement therefore remains unconfirmed. A maintainer must establish that missing, skipped, canceled, or stale formal results cannot satisfy acceptance.

The slashing workflow's Rocq job does not run `finalized_floor`. The inventory records the separate settled-probe command and this CI gap.

Historical B1 and B2 manifests still match commits `7034e2168` and `3fee1ca8a`, respectively. Their historical source digests must not be compared with changed candidate files as new proof.

No Rust source, model, consensus decision, soak workload, or finalization limit changed in this cycle. No soak was dispatched or canceled.

The B1 and B2 regression tests, workflow security checks, release workflow tests, and release-gate tests passed. The full Rust and Rocq suites were not rerun locally.

## Maintenance procedure

1. When a digest fails, inspect the changed input and its claim obligations.
2. Reverify affected evidence before claiming that the changed candidate satisfies an obligation.
3. Keep superseded evidence marked as historical.
4. Update the inventory after review.
5. Run `bash scripts/ci/test-soak-claim-inventory.sh`.

A digest update does not reverify a model or production behavior. The checker cannot detect a dishonest evidence statement with internally consistent digests.

## Ledger record

```json
{
  "artifact": {
    "path": "scripts/ci/test-soak-claim-inventory.sh",
    "commit": null,
    "base_commit": "43af06dab41ddf81b8dd1fcc5cf9e1303027aab7",
    "id": "scripts-ci-test-soak-claim-inventory-sh"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "inventory-contract-tests+hosted-baseline-observation",
    "ref": "docs/cbc-evidence/soak-g0-b3-2026-09-08/manifest.jsonc",
    "counterexample": "docs/cbc-evidence/soak-g0-b3-2026-09-08/red.log",
    "detail": "Inventory validation is not formal discharge. Hosted baseline verification does not establish required-check enforcement or soak acceptance."
  },
  "waiver": null,
  "verified_at": null
}
```
