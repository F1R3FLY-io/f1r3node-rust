# Casper CbC Evidence: scripts/run-merge-recovery-soak.sh

The current driver, terminal-transition, legacy, and disk fixtures pass. Semantic discharge remains pending. The separate shared ledger remains unchanged.

```json
{
  "artifact": {"path": "scripts/run-merge-recovery-soak.sh", "id": "scripts-run-merge-recovery-soak-sh", "commit": null, "sha256": "7f4ba9b1c9078c984251317a4cb5032772a092645bfb83d07f6950c0b43542b2"},
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
