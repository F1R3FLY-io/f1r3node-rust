# CbC Evidence: scripts/casper-soak/tests/carrier_index.rs

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/carrier_index.rs",
    "id": "scripts-casper-soak-tests-carrier-index-rs",
    "commit": "f7b0cb32f4d4ac8ac28996e99e75e0cd8cdb68c6",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "e2dcbd5c5cd457b01234586b2b5e806a4fc73727aaba8c94d5b5f63d39c9cdc9"
  },
  "claim": "docs/claims/casper-soak-carrier-index.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-008"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-carrier-index.md": "4aab7367b93766df504bcd679670b8d4bcfeda3d4cd5ea3a2f6b2f39ca86a236"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-carrier-profile",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json",
    "sha256": "06e5c0d15a9eef13c09f5e28c5b553a082685be7dc44f305fed5380e0c7860e7"
  },
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
  "verified_at": "2026-09-19T17:37:20Z",
  "pending": [],
  "previous_ledger": {
    "git_revision": "f7b0cb32f4d4ac8ac28996e99e75e0cd8cdb68c6",
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-tests-carrier-index-rs.md",
    "sha256": "0c9c2071ea795917de23f8513be8f10053f324866fb804505f67e71088752455"
  }
}
```
