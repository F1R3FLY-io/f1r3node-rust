# CbC Evidence: formal/tlaplus/casper_soak/profiles/carrier_index/verification-plan.jsonc

The bounded checks passed. Binding acceptance and evidence publication remain pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/carrier_index/verification-plan.jsonc",
    "id": "formal-tlaplus-casper-soak-profiles-carrier-index-verification-plan-jsonc",
    "commit": "5cf96a74076e940474f143c21a4ca727dac76101",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "e914f6852dd5b01746215996df69bd000a6f8a074c1341cacb766e58382c3322"
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
    "sha256": "b18b3549273c469f2cb7a59f547834f5413ddb96f8791aa989ba93144543d33b"
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
