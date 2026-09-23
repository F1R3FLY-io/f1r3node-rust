# CbC Evidence: formal/tlaplus/casper_soak/profiles/merge_accounting/verification-plan.jsonc

The bounded checks passed. Human binding acceptance remains pending.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/merge_accounting/verification-plan.jsonc",
    "id": "formal-tlaplus-casper-soak-profiles-merge-accounting-verification-plan-jsonc",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "ecf4584bdb69ec856a6ea01deb2768facf05e2c9e813391cc41396e38dd78fed"
  },
  "claim": "docs/claims/casper-soak-merge-accounting.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-005"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-merge-accounting.md": "647dd6edee35c5bbf2c7f81e01d4f4787099f4bf0a93c5cd3c854d7b6c177459"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-merge-accounting-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/report.json",
    "sha256": "0becc7da8667efdc75f1619be1b8a014fd718543f1c7d4265ceb2d0caffd9647"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-19T06:31:09Z"
}

```
