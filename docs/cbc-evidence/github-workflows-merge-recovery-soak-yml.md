# CbC Evidence: .github/workflows/merge-recovery-soak.yml

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)

These claims cover the harness and profiles only. The node is the system under test, not a proof artifact.

```json
{
  "artifact": {
    "path": ".github/workflows/merge-recovery-soak.yml",
    "commit": "40e2d2d7c409d9f644c04221286bd95a187d1434",
    "id": "github-workflows-merge-recovery-soak-yml",
    "sha256": "1eae728e3f73ff9086296225ce5b900efdb469a66b1a8b1af784f580cdaf1a76"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "a280729b02c810dda2a4386444581919eeae9046a365bbcc4a391c54bea6cd7b"
  },
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "scaffold",
    "ref": null,
    "counterexample": null,
    "detail": "No verifier, fixture, or soak has run for these claims."
  },
  "tiers": {
    "refutation": "pending",
    "construction": "not-applicable",
    "construction_assumptions": null,
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "scaffold_base_commit": "40e2d2d7c409d9f644c04221286bd95a187d1434",
  "waiver": null,
  "verified_at": null,
  "scope": "harness-and-profiles-only"
}
```
