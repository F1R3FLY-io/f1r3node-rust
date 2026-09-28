# CbC Evidence: .github/workflows/casper-authority-adapter.yml

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-authority-adapter.yml",
    "id": "github-workflows-casper-authority-adapter-yml",
    "commit": "b1e33bbaccd289326321ff62759d079f9cbaa177",
    "commit_is_base": true,
    "sha256": "bebe95a7d16c8ff447fe6d916a8abad55130b40d4c12f9044db718bba75c8246",
    "working_tree": true
  },
  "claim": "docs/claims/casper-authority-adapter.md",
  "claim_ids": [
    "CLAIM-CASPER-AUTHORITY-ADAPTER-001",
    "CLAIM-CASPER-AUTHORITY-LIVE-001"
  ],
  "claim_digests": {
    "docs/claims/casper-authority-adapter.md": "5d13e8d1bd319566fe682bd8e02a32d4b16992222203dfdb1184fc38d3f921b0",
    "docs/claims/casper-authority-live-executor.md": "4bae2bef46c423f32bd14395ae2482beab4df9d25adf4fbe1072a47e67bdacd2"
  },
  "adapter": null,
  "status": "pending",
  "scope": "harness-authority-observation-mapping",
  "evidence": {
    "kind": "controlled-live-executor-and-adapter-regression-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-live-20260928-01/report.json",
    "sha256": "206d7935f5837645e62165ef51702ad59ceddceb799ff8cf1ef0ccfd90548f4a"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "previous_records": [
    {
      "artifact": {
        "path": ".github/workflows/casper-authority-adapter.yml",
        "id": "github-workflows-casper-authority-adapter-yml",
        "commit": "66c21f26eeceac6027ad618cd96df07482a864c2",
        "commit_is_base": true,
        "sha256": "b83051db1848ef2839ae9fc7bb6084c1484f069f3ea3b676b138cd4f032d2ea9",
        "working_tree": true
      },
      "claim": "docs/claims/casper-authority-adapter.md",
      "claim_ids": [
        "CLAIM-CASPER-AUTHORITY-ADAPTER-001"
      ],
      "claim_digests": {
        "docs/claims/casper-authority-adapter.md": "5d13e8d1bd319566fe682bd8e02a32d4b16992222203dfdb1184fc38d3f921b0"
      },
      "adapter": null,
      "status": "pending",
      "scope": "harness-authority-observation-mapping",
      "evidence": {
        "kind": "controlled-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-authority-adapter-20260928-01/report.json",
        "sha256": "89cdb6c70c900c5a90dd971bb7840f5122d6146c392e894c0c558a14aa9b84d8"
      },
      "tiers": {
        "refutation": "pending",
        "construction": "pending",
        "binding": "pending"
      },
      "phase_status": {
        "pre_pr216_merge": "pending",
        "post_pr216_merge": "blocked"
      },
      "soak": "pending",
      "waiver": null,
      "verified_at": null
    }
  ]
}
```
