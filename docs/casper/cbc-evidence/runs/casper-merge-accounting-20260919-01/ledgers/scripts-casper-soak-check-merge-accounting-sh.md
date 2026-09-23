# CbC Evidence: scripts/casper-soak/check-merge-accounting.sh

The bounded checks passed. Human binding acceptance remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/check-merge-accounting.sh",
    "id": "scripts-casper-soak-check-merge-accounting-sh",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "3fa17e17890784848c2126d8ca6b04c4b4a56b4628c718dabf615034c47a7af6"
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
