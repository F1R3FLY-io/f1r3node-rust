# CbC Evidence: scripts/casper-soak/src/profiles/carrier_index.rs

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

The maintainer accepted the PR #436 review remediation of this file on 2026-10-03 (PR #441, comment 5972145164) at revision `1aea3f6b4`.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/carrier_index.rs",
    "id": "scripts-casper-soak-src-profiles-carrier-index-rs",
    "commit": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "4deffb17e398490678d4c5ecc09eb486c9110c4cbbf541da5a65cf2669cdfa62",
    "sha256_before_review_remediation": "997181af0574612162ad8167f1989c86d49d005e54a77a1da9f0a1bdb0409d57"
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
    "kind": "accepted-review-remediation",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-review-fixes-acceptance-20261003-01/report.json",
    "sha256": "5c1f90f31886844d58b9c57992ced1905eea30b62555c31d7429ee5dea0ae633"
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
  "verified_at": "2026-10-03T18:30:00Z",
  "pending": [],
  "previous_ledger": {
    "git_revision": "f7b0cb32f4d4ac8ac28996e99e75e0cd8cdb68c6",
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-src-profiles-carrier-index-rs.md",
    "sha256": "4551620a3a83a8d0aaf5779a91145f684a039acf5655f61c9ab85a1672d58dae"
  },
  "evidence_before_review_remediation": {
    "kind": "accepted-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json",
    "sha256": "06e5c0d15a9eef13c09f5e28c5b553a082685be7dc44f305fed5380e0c7860e7"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/441#issuecomment-5972145164",
    "revision": "1aea3f6b4905533e3891b8b497aacb6198ef08e9",
    "reviewed_at": "2026-10-03T18:24:22Z"
  }
}
```
