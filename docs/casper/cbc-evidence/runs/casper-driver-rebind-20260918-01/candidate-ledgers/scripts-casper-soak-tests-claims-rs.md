# Pending repair binding: scripts/casper-soak/tests/claims.rs

This candidate does not replace the canonical acceptance record. The repaired source requires binding acceptance.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/claims.rs",
    "id": "scripts-casper-soak-tests-claims-rs",
    "commit": "a94c5655ee2284e228667a02e357a6ef6cd10ebf",
    "commit_is_base": true,
    "sha256": "8813eba982f56f4fd45e339e61f5ee4b70c29e67bc28b1b2e85c0633b5eeb7d2"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "1f0aea4e775ab73ffa4c8a355c6bcfd9b4c330c8d0f803c1c6321dceb0b5aa81"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "bounded-refutation-and-binding-candidate",
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/report.json",
    "sha256": "0a4a23bba01053c5674c487ca7dded80d573c25eb2f0c51904d6c3096f67e581"
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
  "verified_at": "2026-09-18T17:55:05Z"
}
```
