# CbC Evidence: formal/tlaplus/node_observation/README.md

The approved verification artifact is pending implementation and verification.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/README.md",
    "id": "formal-tlaplus-node-observation-README-md",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "ee9db54b566f52f2d8e1ceaac09ebeeda2717cc5c74568ec9c6e4413fc9ed320",
    "accepted_sha256": "28cc17943cca3bb5460248c99e2cc0a3f740cb7b780d833716d4078f24a65a1b"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md",
    "sha256": "4949d92d8892e4df6f40b31f0159aac6516839cbaa901f3e42e2dc88396ddaa2",
    "commit": "03d7f1b27544b2c5a93b664d8684b24ea16cf3e9"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274",
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
