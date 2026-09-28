# CbC Evidence: node/src/rust/soak_observer.rs

The B2 claim was accepted on 2026-09-23. The source manifest binds the recorded tests to these working-tree contents.
Previous acceptance applies only to its recorded source revision.

```json
{
  "artifact": {
    "path": "node/src/rust/soak_observer.rs",
    "id": "node-src-rust-soak-observer-rs",
    "commit": "d69e12151082d111f18c9e8f818ed7f3ba4f9377",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "59f40fd5632955d7fbaae0cd6221e942d3dc7505ad8a096a2d95bce10d2ed878",
    "accepted_sha256": "59f40fd5632955d7fbaae0cd6221e942d3dc7505ad8a096a2d95bce10d2ed878"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "batch-b2-construction-01",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/report.json",
    "sha256": "4a9a8d296e921baab5e006be3e6aa5eb0181af8515921aa49330bb3f9bb43bf5"
  },
  "tiers": {
    "refutation": "inherited-models-only",
    "construction": "recorded-partial",
    "binding": "recorded"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-23T21:01:02Z",
  "acceptance": {
    "claim_ids": [
      "CLAIM-CASPER-NODE-OBSERVATION-003"
    ],
    "reviewer": "jltatbeach",
    "review_id": 5294038948,
    "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "revision": "237e43d723b9867985cd47fdfe312fd8d06352e8",
    "verified_at": "2026-09-23T21:01:02Z",
    "review_edited_in_place": true,
    "package": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02",
    "decisions": "C1, C13 accepted as bounded by design; construction gaps C2, C6, C10, C11, C14, C15 and the C4, C9, C12 remaining parts accepted as recorded; C7, C8 extension accepted",
    "artifact_sha256_at_acceptance": "59f40fd5632955d7fbaae0cd6221e942d3dc7505ad8a096a2d95bce10d2ed878",
    "claim_digests_at_acceptance": {
      "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
      "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
    }
  },
  "accepted_at": "2026-09-23T21:01:02Z",
  "acceptance_scope": "The acceptance of CLAIM-CASPER-NODE-OBSERVATION-003 applies at revision 237e43d723b9867985cd47fdfe312fd8d06352e8 and the accepted digest. The earlier acceptance of claims 001 and 002 at revision 4c0c0dbe7c8958debefdb02f2b21795786c45900 is retained in acceptances.",
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
  },
  "previous_record": {
    "artifact": "node/src/rust/soak_observer.rs",
    "path": "docs/cbc-evidence/node-src-rust-soak-observer-rs.md",
    "sha256": "0ff5eab586364b570a9153c128959faee3d2ab0ad4e27650261aa193ab5e9baa",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1"
  },
  "acceptances": [
    {
      "reviewer": "jltatbeach",
      "review_id": 5294038948,
      "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
      "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
      "submitted_at": "2026-09-23T16:55:28Z",
      "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
      "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded",
      "claim_ids": [
        "CLAIM-CASPER-NODE-OBSERVATION-001",
        "CLAIM-CASPER-NODE-OBSERVATION-002"
      ],
      "artifact_sha256_at_acceptance": "542a454a57db3774663e7a2010ffdf5b0b7d746078fabbab7849404b5322f68d",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274"
      },
      "verified_at": "2026-09-23T16:55:28Z"
    },
    {
      "claim_ids": [
        "CLAIM-CASPER-NODE-OBSERVATION-003"
      ],
      "reviewer": "jltatbeach",
      "review_id": 5294038948,
      "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
      "revision": "237e43d723b9867985cd47fdfe312fd8d06352e8",
      "verified_at": "2026-09-23T21:01:02Z",
      "review_edited_in_place": true,
      "package": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02",
      "decisions": "C1, C13 accepted as bounded by design; construction gaps C2, C6, C10, C11, C14, C15 and the C4, C9, C12 remaining parts accepted as recorded; C7, C8 extension accepted",
      "artifact_sha256_at_acceptance": "59f40fd5632955d7fbaae0cd6221e942d3dc7505ad8a096a2d95bce10d2ed878",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
        "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
      }
    }
  ]
}
```
