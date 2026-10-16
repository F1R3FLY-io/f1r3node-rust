# CbC Evidence: formal/tlaplus/node_observation/MC_ObserverSession_identity_unsafe.cfg

The combined package records current source hashes and scoped verification results. Both node claims remain pending named maintainer acceptance.

Model proofs and finite Rust correspondence retain the limits stated in the package. This record does not qualify a live campaign.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/MC_ObserverSession_identity_unsafe.cfg",
    "id": "formal-tlaplus-node-observation-MC-ObserverSession-identity-unsafe-cfg",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "88a91a691356a5e37315e925bd3dd1b5877d1fee9327df61980d7060050a8c27"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38"
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
    "artifact": "formal/tlaplus/node_observation/MC_ObserverSession_identity_unsafe.cfg",
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-MC-ObserverSession-identity-unsafe-cfg.md",
    "sha256": "d66f8c378eb511b357f5d7b326ab66b0ecc7bec684426d8399fc8098a5e324a8",
    "commit": "03d7f1b27544b2c5a93b664d8684b24ea16cf3e9"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274"
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
