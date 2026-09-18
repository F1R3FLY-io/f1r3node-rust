# CbC Evidence: scripts/casper-soak/src/manifest.rs

The language migration passes its recorded checks. The complete harness claim remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/manifest.rs",
    "id": "scripts-casper-soak-src-manifest-rs",
    "commit": null,
    "sha256": "d9606d542cfb9bc058f691196244ae2e6055e6c84c02b5c2581ee07e70c03000"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "97bab1392457a21bd0a673cce2f66f8c212cfe2d5a1bae134aea5e92e320c9c4"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {
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
