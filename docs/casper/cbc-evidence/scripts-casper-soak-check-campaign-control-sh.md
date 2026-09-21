# CbC Evidence: scripts/casper-soak/check-campaign-control.sh

Local checks supply review evidence. Source-bound acceptance, hosted verification, and live qualification remain pending.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/check-campaign-control.sh",
    "id": "scripts-casper-soak-check-campaign-control-sh",
    "commit": "4dd7a20126410beb3dd88cabc09ba81d286ee058",
    "commit_is_base": false,
    "sha256": "1d00e091dab501fa3d8bd1f9638c3a50295b399f2ddcaad6632b7f3964aacd3e",
    "working_tree": false
  },
  "claim": "docs/claims/casper-campaign-execution.md",
  "claim_ids": [
    "CLAIM-CASPER-CAMPAIGN-003"
  ],
  "claim_digests": {
    "docs/claims/casper-campaign-execution.md": "8014cdb6d99964e38175f2b1500f8d3ed3d0425ed50b8c33f367702dec4636f2"
  },
  "adapter": null,
  "status": "pending",
  "scope": "campaign-verification-awaiting-source-bound-acceptance",
  "evidence": {
    "kind": "local-verification-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-campaign-source-coverage-20260921-01/report.json",
    "sha256": "9914fb09e2946843821aacc054210ccd8d87057694c9629c634173e36f21610e"
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
        "path": "scripts/casper-soak/check-campaign-control.sh",
        "id": "scripts-casper-soak-check-campaign-control-sh",
        "commit": "ee468f5db3fa007ecde9b104eda545a544f1354a",
        "commit_is_base": true,
        "sha256": "74a33f12fe18bc03f7ffb937df1d0953158f1ce3301bb90ff55d18ff2c83fe55",
        "working_tree": true
      },
      "evidence": {
        "kind": "local-verification-not-acceptance",
        "ref": "docs/casper/cbc-evidence/runs/casper-campaign-control-20260921-01/report.json",
        "sha256": "27e1a2980f81360199826944dd413284a36669c2646b78237632309de1d0fb6f"
      }
    }
  ]
}
```
