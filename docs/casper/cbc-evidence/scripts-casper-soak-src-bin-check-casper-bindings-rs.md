# CbC Evidence: scripts/casper-soak/src/bin/check-casper-bindings.rs

Focused checks pass. The complete fixture suite and harness claim remain pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/bin/check-casper-bindings.rs",
    "id": "scripts-casper-soak-src-bin-check-casper-bindings-rs",
    "commit": null,
    "sha256": "aca91276eb0fafacb34193082abc4c02de5e1562f1a6eeccf3d5f51f0a057859"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "8a0ccde7e3752aecdcd5d92527b6ddceefd733b551b1ee4bea9f2893802c9d27"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {
    "kind": "partial-driver-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-binding-gates-20260918-01/report.json",
    "sha256": "f07c3f7955d7a098b3ce4e646d96bd4472d8f99f56bc43689693c2fb9ecb6039"
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
  "waiver": null,
  "verified_at": null
}
```
