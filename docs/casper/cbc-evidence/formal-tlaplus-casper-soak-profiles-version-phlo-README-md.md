# CbC Evidence: formal/tlaplus/casper_soak/profiles/version_phlo/README.md

Bounded checks passed. Human binding acceptance and workflow-tag ratification remain pending. No node execution or policy activation is authorized.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/version_phlo/README.md",
    "id": "formal-tlaplus-casper-soak-profiles-version-phlo-README-md",
    "commit": "807bf94dcb0389fdd57100bc32f64eea20f64ea6",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "a2b26626b3381df7ee8d73dffe1ac8e4c070310646a6464efae70c40060af48d"
  },
  "claim": "docs/claims/casper-soak-version-phlo.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-007"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-version-phlo.md": "2a6167b965f804533e9ce45b1f665303d022b0af144126db103ff421be532c98"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-protocol-and-phlo-profile",
  "evidence": {
    "kind": "bounded-refutation-and-controlled-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-version-phlo-20260919-01/report.json",
    "sha256": "8baf8000334926228ead092e369c969d5d6ae8397afcc59ec6b41ed14d7facb2"
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
  "verified_at": "2026-09-19T18:31:09Z",
  "pending": [
    "human-binding-acceptance",
    "workflow-tag-ratification",
    "external-evidence-publication"
  ]
}
```
