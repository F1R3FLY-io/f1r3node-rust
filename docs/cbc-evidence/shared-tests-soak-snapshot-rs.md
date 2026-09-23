# CbC Evidence: shared/tests/soak_snapshot.rs

The capture claim remains pending. The verification package requires named maintainer acceptance. Live adapter qualification remains separate.

```json
{
  "artifact": {
    "path": "shared/tests/soak_snapshot.rs",
    "id": "shared-tests-soak-snapshot-rs",
    "commit": "10e7b8452824e12a1fe2743dca7989b79fce2133",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "4ae3aa408e7de7adf032da424cac7d33d356eb285933a1ed6e840d102cc12880"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "4a2282757f77e01161e729f54c36b75c6c4074c9c8e37711d164cd64aa0196a8"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b1-bounded-detached-capture",
  "evidence": {
    "kind": "partial-reconciliation-not-discharge",
    "ref": "docs/cbc-evidence/runs/casper-node-model-reconciliation-10e7b8452-01/report.json",
    "sha256": "1ed89bb3221fddeba2782c1496f9f4a59a37a23359feea348bbc9f8981a3d09c"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md",
    "sha256": "2ca067309b05ce9b26aafda0b5dc313d6c24461d0c5ac04d2bd21ec3b3fd92c7",
    "archive": "records-pre-refresh"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "tier_results": {
    "refutation": "passed-bounded-controls",
    "construction": "passed-bounded-models-and-native-tests",
    "binding": "passed-source-and-test-map-no-machine-refinement"
  },
  "previous_working_tree_record": {
    "archive": "target/task-019-4-rust-verification/previous-metadata.tar.gz",
    "archive_sha256": "b595d2a87fc11dadec09f4d8ac50cc09a977ffc537b945edf06900c742e2243d",
    "member": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md",
    "sha256": "7d10375b8d97d69515055db7ae4f51e0afdb01b9a655dd52b4994d201a3b6147"
  }
}
```
