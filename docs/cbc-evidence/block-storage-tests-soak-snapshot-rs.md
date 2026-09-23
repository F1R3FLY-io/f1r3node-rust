# CbC Evidence: block-storage/tests/soak_snapshot.rs

The B2 claim remains pending. The source manifest binds the recorded tests to these working-tree contents.
Previous acceptance applies only to its recorded source revision.

```json
{
  "artifact": {
    "path": "block-storage/tests/soak_snapshot.rs",
    "id": "block-storage-tests-soak-snapshot-rs",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "15840e51d9ba769d2053760972ad35d71a7a67419ff3c0e4d981852372c90b09",
    "accepted_sha256": "24bdee3183324720539475a0c1dcfc32e3919dcc5d1271dca6df5d6a00ddebdc"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
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
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "commit": "d11acabcbd27b564eab7398778169cf3990762d1",
    "sha256": "d4751fa57283459bcda51f4969a890138842b27e38749f16af0f5544f0f15dff",
    "artifact_sha256": "24bdee3183324720539475a0c1dcfc32e3919dcc5d1271dca6df5d6a00ddebdc",
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
  },
  "accepted_at": "2026-09-23T16:55:28Z",
  "acceptance_scope": "The acceptance applies to CLAIM-CASPER-NODE-OBSERVATION-001 and CLAIM-CASPER-NODE-OBSERVATION-002 at revision 4c0c0dbe7c8958debefdb02f2b21795786c45900 and the accepted digest. This record is pending for CLAIM-CASPER-NODE-OBSERVATION-003 at the current digest."
}
```
