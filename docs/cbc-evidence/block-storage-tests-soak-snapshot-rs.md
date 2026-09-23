# CbC Evidence: block-storage/tests/soak_snapshot.rs

The capture claim remains pending. Local tests do not discharge the claim or qualify a live adapter.

```json
{
  "artifact": {
    "path": "block-storage/tests/soak_snapshot.rs",
    "id": "block-storage-tests-soak-snapshot-rs",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "24bdee3183324720539475a0c1dcfc32e3919dcc5d1271dca6df5d6a00ddebdc"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "task-019-4-handoff-cycle-02",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json",
    "sha256": "79361b7df306cde8f0b157d0749dd4b09e2a33b24dd9ad99710ff9d198ee43d2"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "recorded-partial",
    "binding": "recorded-partial"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-23T16:55:28Z",
  "previous_record": {
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "sha256": "8bc2cf4abce3570c59c205ba5c3f342dcb766d49c967a69b5ac4f87a23fdee8d",
    "commit": "03d7f1b27544b2c5a93b664d8684b24ea16cf3e9"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "234b65b479864c0ce6b45c45f8c2f2c971d01e7639fc2343ad0256a3d728e723"
  },
  "acceptance": {
    "reviewer": "jltatbeach",
    "review_id": 5294038948,
    "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
    "submitted_at": "2026-09-23T16:55:28Z",
    "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
    "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded"
  }
}
```
