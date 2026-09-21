# CbC Evidence: shared/tests/soak_snapshot.rs

The capture claim remains pending. Local tests do not discharge the claim or qualify a live adapter.

```json
{
  "artifact": {
    "path": "shared/tests/soak_snapshot.rs",
    "id": "shared-tests-soak-snapshot-rs",
    "commit": "6198821283b960c3aafae1e8c35966b883dfd2f3",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "4ae3aa408e7de7adf032da424cac7d33d356eb285933a1ed6e840d102cc12880"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "0a903c524a3e3e3b598fddd6ec149e578fb18f396199cd0ab5741faa87df6121"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b1-bounded-detached-capture",
  "evidence": {
    "kind": "local-tests-not-discharge",
    "ref": "docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/report.json",
    "sha256": "73848a230f169a62736d60c5440fa2ea24b991ab7a6f80b0d138718547c9d5a7"
  },
  "previous_record": {
    "commit": "6198821283b960c3aafae1e8c35966b883dfd2f3",
    "path": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md",
    "sha256": "57518aa190eb449c6767c62f8c51f33e6a92d34433cf0eb7d38e42e25ac1174a"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null
}
```
