# CbC Evidence: scripts/casper-soak/src/authority_observer.rs

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/authority_observer.rs",
    "id": "scripts-casper-soak-src-authority-observer-rs",
    "commit": "66c21f26eeceac6027ad618cd96df07482a864c2",
    "commit_is_base": true,
    "sha256": "afdcfea29a14f5d007a72cfa66ce37e040607999c4bda60402c92683fc061744",
    "working_tree": true
  },
  "claim": "docs/claims/casper-authority-observer-client.md",
  "claim_ids": [
    "CLAIM-CASPER-AUTHORITY-CLIENT-001"
  ],
  "claim_digests": {
    "docs/claims/casper-authority-observer-client.md": "04d8f77c611fd94a1ea7ee0a658720a099ec9777b385a299e1e240646fea8c80"
  },
  "adapter": null,
  "status": "pending",
  "scope": "harness-observer-transport",
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
  "verified_at": null,
  "previous_records": [
    {
      "artifact": {
        "path": "scripts/casper-soak/src/authority_observer.rs",
        "id": "scripts-casper-soak-src-authority-observer-rs",
        "commit": "211a4e73a6c1c8c4e4d3de35f2debdf1869ab74b",
        "commit_is_base": true,
        "sha256": "71e960dea70de39f7f532dc3097aef2a6a271d729f3ab1f7ab62bcccb34951de",
        "working_tree": true
      },
      "claim": "docs/claims/casper-authority-observer-client.md",
      "claim_ids": [
        "CLAIM-CASPER-AUTHORITY-CLIENT-001"
      ],
      "claim_digests": {
        "docs/claims/casper-authority-observer-client.md": "51072e16713b99d45d3befafb119dd445d9a62a49390b505e99f186d42a98cb9"
      },
      "adapter": null,
      "status": "pending",
      "scope": "harness-observer-transport",
      "evidence": {
        "kind": "controlled-socket-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-authority-client-20260928-01/report.json",
        "sha256": "a845b4bada38b07d1cc2319700651031583a23258de13204bb712859131bb903"
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
