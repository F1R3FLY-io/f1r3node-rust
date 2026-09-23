# Pending repair binding: scripts/ci/check-casper-soak-bindings.sh

This candidate does not replace the canonical acceptance record. The repaired source requires binding acceptance.

```json
{
  "artifact": {
    "path": "scripts/ci/check-casper-soak-bindings.sh",
    "id": "scripts-ci-check-casper-soak-bindings-sh",
    "commit": "a94c5655ee2284e228667a02e357a6ef6cd10ebf",
    "commit_is_base": true,
    "sha256": "39652b782a2474c82ec7f1678c185f3adcbc0388426cc65d45c85dc74e6904bf"
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
