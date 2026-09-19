# CbC Evidence: formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_order_unsafe.cfg

The bounded checks passed. Human binding acceptance remains pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_order_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-slashing-MC-Slashing-order-unsafe-cfg",
    "commit": "137b74fdb903969d186aef9241380eef24dd4833",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "7ff73a5f463d6510c84f9042e816db00687c4c86203917ed8e327d0c7885e6bf"
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
