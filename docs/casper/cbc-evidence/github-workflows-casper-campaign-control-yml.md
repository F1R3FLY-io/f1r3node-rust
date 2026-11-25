# CbC Evidence: .github/workflows/casper-campaign-control.yml

Local checks supply review evidence. Source-bound acceptance, hosted verification, and live qualification remain pending.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-campaign-control.yml",
    "id": "github-workflows-casper-campaign-control-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "sha256": "1229b3a91d4ff91f55da363913196af72d2fdc1583876b99c340cd6e5fb756c2",
    "working_tree": true,
    "sha256_before_change": "109a71c33f50577dc42daa7a8800504a8df7c0bb81ef844c8fd97e7c533a7541"
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
        "path": ".github/workflows/casper-campaign-control.yml",
        "id": "github-workflows-casper-campaign-control-yml",
        "commit": "ee468f5db3fa007ecde9b104eda545a544f1354a",
        "commit_is_base": false,
        "sha256": "109a71c33f50577dc42daa7a8800504a8df7c0bb81ef844c8fd97e7c533a7541",
        "working_tree": true
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json",
        "sha256": "27e1a2980f81360199826944dd413284a36669c2646b78237632309de1d0fb6f"
      },
      "claim_digests": {
        "docs/claims/casper-campaign-execution.md": "8014cdb6d99964e38175f2b1500f8d3ed3d0425ed50b8c33f367702dec4636f2"
      }
    },
    {
      "artifact": {
        "path": ".github/workflows/casper-campaign-control.yml",
        "id": "github-workflows-casper-campaign-control-yml",
        "commit": "515a011598c9db66ff5a1b2b014b2a2894f7c409",
        "commit_is_base": false,
        "sha256": "109a71c33f50577dc42daa7a8800504a8df7c0bb81ef844c8fd97e7c533a7541",
        "working_tree": true
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
  ],
  "source_change_acceptance": {
    "ref": "docs/casper/cbc-evidence/runs/ci-preinstalled-jdk-acceptance-20261023-01/report.json",
    "sha256": "987eb5ae70d8b738c7adaa7619ff71a713a5f0e804cf067d136c44de873c7bd9",
    "note": "The source digest follows the accepted install change. The ledger stays pending."
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
