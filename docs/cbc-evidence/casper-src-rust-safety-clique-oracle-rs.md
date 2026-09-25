# CbC Evidence: casper/src/rust/safety/clique_oracle.rs

The B2 claim was accepted on 2026-09-23. The source manifest binds the recorded tests to these working-tree contents.
Previous acceptance applies only to its recorded source revision.

```json
{
  "artifact": {
    "path": "casper/src/rust/safety/clique_oracle.rs",
    "id": "casper-src-rust-safety-clique-oracle-rs",
    "commit": "d69e12151082d111f18c9e8f818ed7f3ba4f9377",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "a42a10bbefe048652d3c588dc45336a685261c7f23b90dae54ab7e6de53a69bc",
    "accepted_sha256": "a42a10bbefe048652d3c588dc45336a685261c7f23b90dae54ab7e6de53a69bc"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
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
  "previous_record": {
    "path": "docs/cbc-evidence/casper-src-rust-safety-clique-oracle-rs.md",
    "sha256": "7d07afe3ec3908da63704d50b0f4f24b13c9c50a8d5f1a6a9998919bb4cd84f2",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
  },
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
    "artifact_sha256_at_acceptance": "a42a10bbefe048652d3c588dc45336a685261c7f23b90dae54ab7e6de53a69bc",
    "claim_digests_at_acceptance": {
      "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
    }
  },
  "acceptances": [
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
      "artifact_sha256_at_acceptance": "a42a10bbefe048652d3c588dc45336a685261c7f23b90dae54ab7e6de53a69bc",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
      }
    }
  ],
  "accepted_at": "2026-09-23T21:01:02Z",
  "acceptance_scope": "The acceptance of CLAIM-CASPER-NODE-OBSERVATION-003 applies at revision 237e43d723b9867985cd47fdfe312fd8d06352e8 and the accepted digest."
}
```
