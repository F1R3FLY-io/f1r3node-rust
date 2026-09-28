# CbC Evidence: .github/workflows/merge-recovery-soak.yml

Local checks supply review evidence. Source-bound acceptance, hosted verification, and live qualification remain pending.

```json
{
  "artifact": {
    "path": ".github/workflows/merge-recovery-soak.yml",
    "id": "github-workflows-merge-recovery-soak-yml",
    "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
    "commit_is_base": true,
    "sha256": "0e61d97222f962d973e9bb5322e6f70d3d302c7936be0ac59e05efafc86c9ece",
    "working_tree": true
  },
  "claim": "docs/claims/casper-campaign-execution.md",
  "claim_ids": [
    "CLAIM-CASPER-CAMPAIGN-003",
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-campaign-execution.md": "1574f70fc5b1f432b7bf1fa98a6deca125d3221e3b29864686a42da70a2b31bb",
    "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
  },
  "adapter": null,
  "status": "pending",
  "scope": "campaign-verification-awaiting-source-bound-acceptance",
  "evidence": {
    "kind": "local-verification-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-campaign-phase-20260922-01/report.json",
    "sha256": "cdd7e051aaa4959f84bf8a87840630e3aacf1d0bd8c09b3f6297afca69d3248f"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "previous_ledger": {
    "commit": "ee468f5db3fa007ecde9b104eda545a544f1354a",
    "path": "docs/casper/cbc-evidence/github-workflows-merge-recovery-soak-yml.md",
    "sha256": "5d384fce9a87818246c80eb607fcd279bf4575212aa7e59c6a9c71e68dc8d2af"
  },
  "prior_records": [
    {
      "artifact": {
        "path": ".github/workflows/merge-recovery-soak.yml",
        "id": "github-workflows-merge-recovery-soak-yml",
        "commit": "ee468f5db3fa007ecde9b104eda545a544f1354a",
        "commit_is_base": true,
        "sha256": "4ae6cf3dd11b122123e9dff529dbc788f616be129e5e457b47594259c09c2e7b",
        "working_tree": true
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json",
        "sha256": "27e1a2980f81360199826944dd413284a36669c2646b78237632309de1d0fb6f"
      },
      "claim_digests": {
        "docs/claims/casper-campaign-execution.md": "8014cdb6d99964e38175f2b1500f8d3ed3d0425ed50b8c33f367702dec4636f2",
        "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
      }
    },
    {
      "artifact": {
        "path": ".github/workflows/merge-recovery-soak.yml",
        "id": "github-workflows-merge-recovery-soak-yml",
        "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
        "commit_is_base": true,
        "sha256": "0e61d97222f962d973e9bb5322e6f70d3d302c7936be0ac59e05efafc86c9ece",
        "working_tree": true
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-stability-20260922-01/report.json",
        "sha256": "bdfd4e878489e618de82bcf2a69e913e0ea6d525aaabcdc16d3f992f652fccb7"
      },
      "claim_digests": {
        "docs/claims/casper-campaign-execution.md": "8840cb6021093cb3dae8f1add287cd299a1e1ed9eb1a8b3a8f3c753e10de3f1b",
        "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
      }
    }
  ]
}
```
