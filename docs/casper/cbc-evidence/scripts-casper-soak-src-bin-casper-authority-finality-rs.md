# CbC Evidence: scripts/casper-soak/src/bin/casper-authority-finality.rs

The bounded model and controlled fixtures pass. Human binding acceptance remains pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/bin/casper-authority-finality.rs",
    "id": "scripts-casper-soak-src-bin-casper-authority-finality-rs",
    "commit": "d871a2df83a69dbb0ddaafffef609612cf8388ea",
    "commit_is_base": true,
    "sha256": "93caa3d4c8909d8c8befe4ec3a1b551ca470d75fe39bf14ba7a45100310d1718"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "9340a152acec453ce2acf2e68b5238e06e4201476d036e2e883213560549e4f4"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-authority-finality-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-finality-20260918-01/report.json",
    "sha256": "aee066e144f2e725373d479a097c9b185b71621a47997862e0858f6b97cdc777"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending-review"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-18T14:19:14Z"
}
```
