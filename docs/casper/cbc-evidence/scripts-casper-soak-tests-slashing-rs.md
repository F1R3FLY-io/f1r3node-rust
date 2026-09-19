# CbC Evidence: scripts/casper-soak/tests/slashing.rs

The bounded checks passed. Human binding acceptance remains pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/tests/slashing.rs",
    "id": "scripts-casper-soak-tests-slashing-rs",
    "commit": "137b74fdb903969d186aef9241380eef24dd4833",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "e013ec602c58bd291aa02b9af7a3e135581ac51d9e29f3b3f02d8d38dcc44afb"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "6ceae81e04078a2fff89707ab231b9395ef66c047e3a9897501299ddb668f356"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-slashing-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures-awaiting-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-slashing-20260919-02/report.json",
    "sha256": "8d7d4583a67d3e54247421e95690fab1fcd6ea40f9cd92afc7be9d461aed01c2"
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
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "pending": [
    "human-binding-acceptance",
    "workflow-tag-ratification",
    "external-evidence-publication",
    "hosted-workflow-verification"
  ]
}
```
