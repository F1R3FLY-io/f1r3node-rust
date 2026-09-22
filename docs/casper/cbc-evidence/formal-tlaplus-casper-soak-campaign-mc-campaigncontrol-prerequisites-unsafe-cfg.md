# CbC Evidence: formal/tlaplus/casper_soak/campaign/MC_CampaignControl_prerequisites_unsafe.cfg

Local verification does not establish source-bound acceptance or live qualification.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/campaign/MC_CampaignControl_prerequisites_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-campaign-mc-campaigncontrol-prerequisites-unsafe-cfg",
    "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "f535f81c05659121ceb6c6518416c4cd1879a4a32cc343a396dde21e40fb2041"
  },
  "claim": "docs/claims/casper-campaign-execution.md",
  "claim_ids": [
    "CLAIM-CASPER-CAMPAIGN-003"
  ],
  "claim_digests": {
    "docs/claims/casper-campaign-execution.md": "1574f70fc5b1f432b7bf1fa98a6deca125d3221e3b29864686a42da70a2b31bb"
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
  "prior_records": [
    {
      "artifact": {
        "path": "formal/tlaplus/casper_soak/campaign/MC_CampaignControl_prerequisites_unsafe.cfg",
        "id": "formal-tlaplus-casper-soak-campaign-mc-campaigncontrol-prerequisites-unsafe-cfg",
        "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
        "commit_is_base": true,
        "working_tree": true,
        "sha256": "f535f81c05659121ceb6c6518416c4cd1879a4a32cc343a396dde21e40fb2041"
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-stability-20260922-01/report.json",
        "sha256": "bdfd4e878489e618de82bcf2a69e913e0ea6d525aaabcdc16d3f992f652fccb7"
      },
      "claim_digests": {
        "docs/claims/casper-campaign-execution.md": "8840cb6021093cb3dae8f1add287cd299a1e1ed9eb1a8b3a8f3c753e10de3f1b"
      }
    }
  ]
}
```
