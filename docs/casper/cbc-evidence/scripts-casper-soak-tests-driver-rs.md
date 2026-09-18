# CbC Evidence: scripts/casper-soak/tests/driver.rs

The terminal-entry regression passes. The complete current fixture suite remains unverified, and the harness claim remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/driver.rs",
    "id": "scripts-casper-soak-tests-driver-rs",
    "commit": null,
    "sha256": "cf7b7d06c8363394a5d1e2b66b4189e390192c961dcf7ea8c974cc74d963c499"
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
