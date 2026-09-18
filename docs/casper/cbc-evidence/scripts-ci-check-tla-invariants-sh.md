# CbC Evidence: scripts/ci/check-tla-invariants.sh

The user accepted the bounded H01–H10 binding review. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

Profile verification, node soaks, post-merge work, and inherited containment limits remain separate. Earlier ledger bytes remain in the linked archive.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "id": "scripts-ci-check-tla-invariants-sh",
    "commit": "946743a7740e5dd3c0816263c3347e501f2d50d7",
    "sha256": "41849481aeafbf7b98ec690fc44d209139c8b1749cd99e8de43ebb5ffc876baf",
    "commit_is_base": false
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "c711fdc42bfed34ce28a6cd5da407e27d2e4745037b0eb250d8e760bee24a071"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/report.json",
    "sha256": "c510e4e393daf52f5a3d2e3affa908897ab0a574484de807939354d5ad8c5ec1"
  },
  "previous_ledger": null,
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "phase_status": {
    "pre_pr216_merge": "discharged",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-18T04:31:12Z"
}
```
