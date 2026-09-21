# CbC Evidence: shared/tests/soak_snapshot.rs

The capture claim remains pending. Local tests do not discharge the claim or qualify a live adapter.

```json
{
  "artifact": {
    "path": "shared/tests/soak_snapshot.rs",
    "id": "shared-tests-soak-snapshot-rs",
    "commit": "2ccc4ae0ac1045232c247ecd76925e13fabd0ade",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "c7c6c3a514ce4997b19e1c2d8ab89490eaa658dc3702bb663b8588d45a1482cd"
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
    "ref": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/report.json",
    "sha256": "e0d607f7fb2f8a2f4eea29b9cd2f4ffdb2de4949aa99432439c3c0fe94d01a04"
  },
  "previous_record": {
    "commit": "2ccc4ae0ac1045232c247ecd76925e13fabd0ade",
    "path": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md",
    "sha256": "4db5440036b2e888d219f3cd8fa6360362fa7c95f0fe59e86b2be7f54831328c"
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
