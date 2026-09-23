# CbC Evidence: shared/tests/soak_snapshot.rs

The capture claim remains pending. The verification package requires named maintainer acceptance. Live adapter qualification remains separate.

```json
{
  "artifact": {
    "path": "shared/tests/soak_snapshot.rs",
    "id": "shared-tests-soak-snapshot-rs",
    "commit": "4aa93d11cf7a4c318074975dd4266208575c8aca",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "4ae3aa408e7de7adf032da424cac7d33d356eb285933a1ed6e840d102cc12880"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "c2bd97d63c74e4fad19a8817a21e044fc358957623c3e8968c3f8535b60964c6"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b1-bounded-detached-capture",
  "evidence": {
    "kind": "bounded-models-and-source-bound-tests-awaiting-acceptance",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-4aa93d11c-03/report.json",
    "sha256": "dc7acd04b4da21aaac99b8a19d0bd56193a85a24a914e9bb09f0334892c6300c"
  },
  "previous_record": {
    "commit": "4aa93d11cf7a4c318074975dd4266208575c8aca",
    "path": "docs/cbc-evidence/shared-tests-soak-snapshot-rs.md",
    "sha256": "2ca067309b05ce9b26aafda0b5dc313d6c24461d0c5ac04d2bd21ec3b3fd92c7"
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
