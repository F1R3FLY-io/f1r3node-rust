# CbC Evidence: .github/workflows/merge-recovery-soak.yml

The manual campaign route changed this artifact. Its current source binding is pending.

The earlier accepted ledger remains at the Git revision below. Local fixture checks do not renew hosted verification or authorize campaign execution.

```json
{
  "artifact": {
    "path": ".github/workflows/merge-recovery-soak.yml",
    "id": "github-workflows-merge-recovery-soak-yml",
    "commit": "5e26ba4c545ff8dafcf9ca9b20767703fa96f437",
    "commit_is_base": true,
    "sha256": "46807d26da6011c3643f376cdd620b1085c1553a08cc37d173df649d12ec044a",
    "working_tree": false
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": ["CLAIM-CASPER-SOAK-001"],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "c1f6b7473f790a60ab964ce7fd7277841cd6cee5718e9853ad96cf4c671d5932"
  },
  "adapter": null,
  "status": "pending",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "local-fixtures-not-workflow-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-campaign-dispatch-5e26ba4c5-01/report.json",
    "sha256": "853909cfd9f9b16380585b5e8c852f1468103826d44274605d4ecaaaeef23452"
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
    "commit": "5e26ba4c545ff8dafcf9ca9b20767703fa96f437",
    "path": "docs/casper/cbc-evidence/github-workflows-merge-recovery-soak-yml.md",
    "sha256": "f35743cd5901aaec916ae6b61b075607bb6ed0d1a34368db40ef4fbb6385803a"
  }
}
```
