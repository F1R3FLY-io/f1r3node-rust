# CbC Evidence: scripts/casper-soak/test-campaign.sh

Local checks supply review evidence. Source-bound acceptance, hosted verification, and live qualification remain pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/test-campaign.sh",
    "id": "scripts-casper-soak-test-campaign-sh",
    "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
    "commit_is_base": true,
    "sha256": "82bcb927fbfc9872ce820c92b42f0765645909e3bbddf5f58476c9d19d419647",
    "working_tree": true
  },
  "claim": "docs/claims/casper-soak-campaign.md",
  "claim_ids": [
    "CLAIM-CASPER-CAMPAIGN-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-campaign.md": "4440eec4b9710119bda6deed38d228ceebfa17e0c263c826a008702d8e2e69e5"
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
    "path": "docs/casper/cbc-evidence/scripts-casper-soak-test-campaign-sh.md",
    "sha256": "c19f0768d12580b33119285af40081291f905af81d45662c87451ebbfff6c894"
  },
  "prior_records": [
    {
      "artifact": {
        "path": "scripts/casper-soak/test-campaign.sh",
        "id": "scripts-casper-soak-test-campaign-sh",
        "commit": "ee468f5db3fa007ecde9b104eda545a544f1354a",
        "commit_is_base": true,
        "sha256": "c9432c74cbf592b4e1e608b90b7bbedc12cbc6e4428232fb19e44d428bd0375f",
        "working_tree": true
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json",
        "sha256": "27e1a2980f81360199826944dd413284a36669c2646b78237632309de1d0fb6f"
      },
      "claim_digests": {
        "docs/claims/casper-soak-campaign.md": "b5d3853a19c5bb4bf019f10d89297a5938c145c0597376c2a203c793b194bb92"
      }
    }
  ]
}
```
