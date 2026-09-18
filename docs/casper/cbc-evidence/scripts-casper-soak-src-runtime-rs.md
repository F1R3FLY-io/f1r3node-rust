# CbC Evidence: scripts/casper-soak/src/runtime.rs

Current driver and terminal-transition fixtures pass. Semantic claim discharge remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/runtime.rs",
    "id": "scripts-casper-soak-src-runtime-rs",
    "commit": null,
    "sha256": "8d2c4da61749d33491c6328bfc4c11c6e2e7bf07f0ac6b5345542d4b707e9d43"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "29709b0b86bcf1bc287582fa15410af0cc4d7381736c3d23576903db5ca66661"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {
    "kind": "source-bound-fixture-verification",
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-checks-20260918-01/report.json",
    "sha256": "d0c11f742ae19e833df893cf7a77f18e7e5912a97ad39fcb03795ce147bd15b0"
  },
  "previous_binding_evidence": {
    "kind": "partial-driver-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-binding-gates-20260918-01/report.json",
    "sha256": "f07c3f7955d7a098b3ce4e646d96bd4472d8f99f56bc43689693c2fb9ecb6039"
  },
  "previous_evidence": {
    "kind": "language-migration-verification",
    "ref": "docs/casper/cbc-evidence/runs/casper-rust-migration-20260917-01/report.json",
    "sha256": "6696a0659bb58fa72b27542a414ef4598c3a649bc638136d810d1796c98b2555"
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
