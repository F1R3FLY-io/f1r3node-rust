# CbC Evidence: .github/workflows/casper-authority-finality.yml

Controlled verification does not qualify a node candidate. Source-bound acceptance remains pending.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-authority-finality.yml",
    "id": "github-workflows-casper-authority-finality-yml",
    "commit": "63b9ace2b2c300b9dc605903d12bbb767566ae4b",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "db9a1044cb693690f66975ff45d7954ab10b4469d353d156fe5bc402e6746721"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "11570876df0aa9c642f28e602d3c120d0954337f59c174eae98bce5e9a6cfb8a"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-authority-finality-profile",
  "evidence": {
    "kind": "controlled-provider-verification-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/report.json",
    "sha256": "d85f9d4ce21cc13205e44f0ef94ab6ead8ca2a192a094febb128198135810470"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
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
    "path": "docs/casper/cbc-evidence/runs/casper-authority-provider-20260928-01/previous-records/github-workflows-casper-authority-finality-yml.md",
    "sha256": "40b115f6e4f1f1d68cb2ff41fe940cab5ac726220f0c10ac1a9e81d32cf04299",
    "commit": "63b9ace2b2c300b9dc605903d12bbb767566ae4b"
  },
  "previous_records": [
    {
      "artifact": {
        "path": ".github/workflows/casper-authority-finality.yml",
        "id": "github-workflows-casper-authority-finality-yml",
        "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
        "commit_is_base": true,
        "sha256": "db9a1044cb693690f66975ff45d7954ab10b4469d353d156fe5bc402e6746721"
      },
      "claim": "docs/claims/casper-soak-authority-finality.md",
      "claim_ids": [
        "CLAIM-CASPER-SOAK-002"
      ],
      "claim_digests": {
        "docs/claims/casper-soak-authority-finality.md": "1af422a0628c37ffab2150b98701c954a2875bf77df4eb9f3e69ff56d23feff2"
      },
      "adapter": "embedded",
      "status": "discharged",
      "scope": "bounded-authority-finality-profile",
      "evidence": {
        "kind": "bounded-refutation-and-executable-fixtures",
        "ref": "docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json",
        "sha256": "614d13c3e52ed11afb84a9a0b5f6345db42bc3a684518f9ce7060b15fc08c4da"
      },
      "tiers": {
        "refutation": "bounded-safety-pass",
        "construction": "not-applicable",
        "binding": "passed"
      },
      "phase_status": {
        "pre_pr216_merge": "discharged",
        "post_pr216_merge": "blocked"
      },
      "soak": "pending",
      "waiver": null,
      "verified_at": "2026-09-19T03:05:12Z",
      "previous_ledger": {
        "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
        "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
        "member": "docs/casper/cbc-evidence/github-workflows-casper-authority-finality-yml.md"
      }
    }
  ]
}
```
