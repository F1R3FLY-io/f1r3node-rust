# CbC Evidence: scripts/casper-soak/src/authority_execution.rs

Executable binding changes require renewed verification and acceptance. Previous acceptance remains bound to its historical sources.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/authority_execution.rs",
    "id": "scripts-casper-soak-src-authority-execution-rs",
    "commit": "66c21f26eeceac6027ad618cd96df07482a864c2",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "93ece0971da840404e69477c55125aadf7f93021f90a32fa0ecb3e695f97b80f"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "adapter": "embedded",
  "scope": "bounded-authority-finality-profile",
  "waiver": null,
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "3838b950f0def2bcd4ff777017781a7d3e49faede57a1c71e679c613b57c5547"
  },
  "status": "pending",
  "verified_at": null,
  "soak": "pending",
  "evidence": {
    "kind": "executable-binding-renewal-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-execution-20260928-01/report.json",
    "sha256": "6260b444191cfffa1d733c435870c13f4b99686d877e253bd9017b35ba15b06c"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  }
}
```
