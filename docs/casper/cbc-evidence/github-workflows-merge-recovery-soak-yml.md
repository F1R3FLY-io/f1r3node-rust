# CbC Evidence: .github/workflows/merge-recovery-soak.yml

The manual campaign route changed this artifact. Its current source binding is pending.

The earlier accepted ledger remains at the Git revision below. Local fixture checks do not renew hosted verification or authorize campaign execution.

The current specification digest reflects the pending-status correction at `859cbc36c`. The reconciliation metadata identifies the previous ledger and both specification versions.

The linked fixture report retains its original specification digest and execution identity. This metadata correction does not renew verification or discharge Claim001.

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
    "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
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
  },
  "specification_reconciliation": {
    "scope": "metadata-only",
    "previous_ledger": {
      "commit": "7b8865aaa49fdec69b5b3dea8a239c06b4d399b6",
      "path": "docs/casper/cbc-evidence/github-workflows-merge-recovery-soak-yml.md",
      "sha256": "c8a13e8563e127a6a61d3deddea280aa92d78b4000a96c7fcc0cab98e662f64c"
    },
    "previous_claim": {
      "commit": "6ffd68230e00d14e399498fec376fe6d3bf532ca",
      "path": "docs/claims/casper-soak-harness.md",
      "sha256": "c1f6b7473f790a60ab964ce7fd7277841cd6cee5718e9853ad96cf4c671d5932"
    },
    "current_claim": {
      "commit": "859cbc36cb58ebac06b2097a256a1cd7b6740afb",
      "path": "docs/claims/casper-soak-harness.md",
      "sha256": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
    },
    "proof_execution": false,
    "hosted_verification": false,
    "claim_acceptance": false
  }
}
```
