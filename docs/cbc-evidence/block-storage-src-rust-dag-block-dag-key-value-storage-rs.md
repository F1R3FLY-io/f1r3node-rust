# CbC Evidence: block-storage/src/rust/dag/block_dag_key_value_storage.rs

The B2 claim was accepted on 2026-09-23. The source manifest binds the recorded tests to these working-tree contents.
Previous acceptance applies only to its recorded source revision.

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
    "commit": "d69e12151082d111f18c9e8f818ed7f3ba4f9377",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "1736d6770bb72d2b4d768010920269da75901f89914080951e071cc78a6f0a06",
    "accepted_sha256": "1736d6770bb72d2b4d768010920269da75901f89914080951e071cc78a6f0a06"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
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
    "artifact_sha256_at_acceptance": "1736d6770bb72d2b4d768010920269da75901f89914080951e071cc78a6f0a06",
    "claim_digests_at_acceptance": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
    }
  },
  "accepted_at": "2026-09-23T21:01:02Z",
  "acceptance_scope": "The acceptance of CLAIM-CASPER-NODE-OBSERVATION-003 applies at revision 237e43d723b9867985cd47fdfe312fd8d06352e8 and the accepted digest. The earlier acceptance of claims 001 and 002 at revision 4c0c0dbe7c8958debefdb02f2b21795786c45900 is retained in acceptances.",
  "accepted_claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
  },
  "previous_record": {
    "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
    "sha256": "ce1ab52bed557dbe2bdbdc0de64dd372ad23138441795c5be847900194c9560f",
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
      "artifact_sha256_at_acceptance": "7bfd28ef62b84ee912a1c2979bd77d6f1f3d804c6cb5d6c9549f9f86ddf43bc2",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-authority-snapshot.md": "234b65b479864c0ce6b45c45f8c2f2c971d01e7639fc2343ad0256a3d728e723"
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
      "artifact_sha256_at_acceptance": "1736d6770bb72d2b4d768010920269da75901f89914080951e071cc78a6f0a06",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
        "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
      }
    }
  ]
}
```
