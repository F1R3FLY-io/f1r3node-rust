# CbC Evidence: scripts/casper-soak/check-carrier-index.sh

The bounded checks passed. Binding acceptance and evidence publication remain pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/check-carrier-index.sh",
    "id": "scripts-casper-soak-check-carrier-index-sh",
    "commit": "5cf96a74076e940474f143c21a4ca727dac76101",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "7312a341117291d6affe23c42239a4f3968d38524c2c340c9ef8d768b5914e00"
  },
  "claim": "docs/claims/casper-soak-carrier-index.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-008"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-carrier-index.md": "bad35868513365e7702f57899a67ef40fcec5e8471c1cfe4ec2265291897aec3"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-carrier-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures-awaiting-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-carrier-index-20260919-01/report.json",
    "sha256": "4b0642726f3765ccc079ed43e6f6e58876a581e8fe6044dc492cb0e57a9b25a2"
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
    "external-evidence-publication",
    "hosted-workflow-verification",
    "workflow-tag-ratification"
  ]
}
```
