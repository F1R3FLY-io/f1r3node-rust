# CbC Evidence: block-storage/tests/soak_snapshot.rs

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

```json
{
  "artifact": {
    "path": "block-storage/tests/soak_snapshot.rs",
    "id": "block-storage-tests-soak-snapshot-rs",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "15840e51d9ba769d2053760972ad35d71a7a67419ff3c0e4d981852372c90b09",
    "sha256_at_registration": "15840e51d9ba769d2053760972ad35d71a7a67419ff3c0e4d981852372c90b09"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "previous_record": {
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "d1c61081c099a08a2a9b38ece30b2a1c3e3d6d711adaed54234cc86fb4314950"
  },
  "evidence": null,
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
