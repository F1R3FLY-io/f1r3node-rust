# CbC Evidence: scripts/casper-soak/tests/claims.rs

The synthetic audit fixtures pass. These inputs do not discharge the real claim. Semantic discharge remains pending.

```json
{
  "artifact": {"path": "scripts/casper-soak/tests/claims.rs", "id": "scripts-casper-soak-tests-claims-rs", "commit": null, "sha256": "bacfc23b9b87dbab93d1eb172753087481419382f9dd29101e8b61e7cdc55f57"},
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": ["CLAIM-CASPER-SOAK-001"],
  "claim_digests": {"docs/claims/casper-soak-harness.md": "29709b0b86bcf1bc287582fa15410af0cc4d7381736c3d23576903db5ca66661"},
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {"kind": "source-bound-fixture-verification", "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-checks-20260918-01/report.json", "sha256": "d0c11f742ae19e833df893cf7a77f18e7e5912a97ad39fcb03795ce147bd15b0"},
  "tiers": {"refutation": "bounded-safety-pass", "construction": "not-applicable", "binding": "pending"},
  "phase_status": {"pre_pr216_merge": "pending", "post_pr216_merge": "blocked"},
  "waiver": null,
  "verified_at": null
}
```
