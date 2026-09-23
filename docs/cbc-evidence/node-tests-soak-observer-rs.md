# CbC Evidence: node/tests/soak_observer.rs

The B2 claim remains pending. The source manifest binds the recorded tests to these working-tree contents.
Previous acceptance applies only to its recorded source revision.

```json
{
  "artifact": {
    "path": "node/tests/soak_observer.rs",
    "id": "node-tests-soak-observer-rs",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "3b69fa3df590f5687fdb719578557a0eef0df05c4fe60072e30c95cc8e782e86",
    "accepted_sha256": "b49d6fd51be0d865d89bff5e851a219dad6b2d6d170d7ce1042a28780fe30741"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-evaluation.md": "d37e548ae04d8fb6bfce28665ef0bdd3538571a6b4a795d741ae01c1873ec813"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b2-source-verification",
  "evidence": {
    "kind": "source-bound-rust-evidence-pending-acceptance",
    "ref": "docs/cbc-evidence/runs/casper-node-authority-b2-d11acabcb-01/report.json",
    "sha256": "abd1403dde113356fffaf2f5e091c6a6d0b35205d6336d2d44ba08c4ec7db505"
  },
  "tiers": {
    "refutation": "inherited-models-only",
    "construction": "pending",
    "binding": "recorded-partial"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-23T18:59:22.351007+00:00",
  "previous_record": {
    "path": "docs/cbc-evidence/node-tests-soak-observer-rs.md",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1",
    "sha256": "b4c057c272f9d3b3ea75493358799ec83d371ed4a9fb675c0ef83923647fc045",
    "artifact_sha256": "b49d6fd51be0d865d89bff5e851a219dad6b2d6d170d7ce1042a28780fe30741",
    "status": "discharged",
    "acceptance": {
      "reviewer": "jltatbeach",
      "review_id": 5294038948,
      "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
      "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
      "submitted_at": "2026-09-23T16:55:28Z",
      "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
      "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded"
    }
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
  },
  "accepted_at": "2026-09-23T16:55:28Z",
  "acceptance_scope": "The acceptance applies to CLAIM-CASPER-NODE-OBSERVATION-001 and CLAIM-CASPER-NODE-OBSERVATION-002 at revision 4c0c0dbe7c8958debefdb02f2b21795786c45900 and the accepted digest. This record is pending for CLAIM-CASPER-NODE-OBSERVATION-003 at the current digest."
}
```
