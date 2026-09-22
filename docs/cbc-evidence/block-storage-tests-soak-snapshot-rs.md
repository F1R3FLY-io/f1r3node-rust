# CbC Evidence: block-storage/tests/soak_snapshot.rs

The capture claim remains pending. Local tests do not discharge the claim or qualify a live adapter.

```json
{
  "artifact": {
    "path": "block-storage/tests/soak_snapshot.rs",
    "id": "block-storage-tests-soak-snapshot-rs",
    "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "1503fb0497becd0b9ac7ec706c621268a7fe0a3e6aefaa0acbd8d80adf042403"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "0a903c524a3e3e3b598fddd6ec149e578fb18f396199cd0ab5741faa87df6121"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b1-bounded-detached-capture",
  "evidence": {
    "kind": "local-tests-not-discharge",
    "ref": "docs/casper/cbc-evidence/runs/casper-campaign-stability-20260922-01/report.json",
    "sha256": "bdfd4e878489e618de82bcf2a69e913e0ea6d525aaabcdc16d3f992f652fccb7"
  },
  "previous_record": {
    "commit": "6198821283b960c3aafae1e8c35966b883dfd2f3",
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "sha256": "e9cf88f8345d072cd4f72b593582c5693d0118b662ff012f13e3b139ae111674"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "prior_records": [
    {
      "artifact": {
        "path": "block-storage/tests/soak_snapshot.rs",
        "id": "block-storage-tests-soak-snapshot-rs",
        "commit": "6198821283b960c3aafae1e8c35966b883dfd2f3",
        "commit_is_base": true,
        "working_tree": true,
        "sha256": "7e3c1571eac4d6b9517a6e956c98240a341631d35359923e571952c6f6effa77"
      },
      "evidence": {
        "kind": "local-tests-not-discharge",
        "ref": "docs/cbc-evidence/runs/casper-node-snapshot-completion-619882128-01/report.json",
        "sha256": "73848a230f169a62736d60c5440fa2ea24b991ab7a6f80b0d138718547c9d5a7"
      }
    }
  ]
}
```
