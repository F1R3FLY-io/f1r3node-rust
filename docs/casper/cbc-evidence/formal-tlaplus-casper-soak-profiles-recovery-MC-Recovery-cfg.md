# CbC Evidence: formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery.cfg

The bounded model and controlled fixtures pass. Human binding acceptance remains pending. The shared audit remains blocked by driver source changes. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-recovery-MC-Recovery-cfg",
    "commit": "a94c5655ee2284e228667a02e357a6ef6cd10ebf",
    "commit_is_base": true,
    "sha256": "2da0ff4b0aff2b575d7f54ca0e66f540ea7075d1140aea696b7bbc3063c2357b"
  },
  "claim": "docs/claims/casper-soak-recovery.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-004"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-recovery.md": "3b38680bbf99b7156609b3b05c2e21590fbecd5149a006def590e4e49895ce6d"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-recovery-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-recovery-20260918-01/report.json",
    "sha256": "5111cc7c456ed80cb47da37fd17978bf350c29b86d72441819bb6856e9c6ba87"
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
  "verified_at": "2026-09-18T16:28:47Z"
}
```
