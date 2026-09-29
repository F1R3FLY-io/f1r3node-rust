# CbC Evidence: scripts/casper-soak/src/profiles/authority_finality.rs

Executable binding changes require renewed verification and acceptance. Previous acceptance remains bound to its historical sources.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/authority_finality.rs",
    "id": "scripts-casper-soak-src-profiles-authority-finality-rs",
    "commit": "66c21f26eeceac6027ad618cd96df07482a864c2",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "ff64a61e06aceb71c73f82701fccaa1f096ddf4717f0efa39a945aaed2be041b"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "3838b950f0def2bcd4ff777017781a7d3e49faede57a1c71e679c613b57c5547"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-authority-finality-profile",
  "evidence": {
    "kind": "executable-binding-renewal-not-acceptance",
    "ref": "docs/casper/cbc-evidence/runs/casper-authority-execution-20260928-01/report.json",
    "sha256": "6260b444191cfffa1d733c435870c13f4b99686d877e253bd9017b35ba15b06c"
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
    "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
    "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
    "member": "docs/casper/cbc-evidence/scripts-casper-soak-src-profiles-authority-finality-rs.md"
  },
  "previous_records": [
    {
      "artifact": {
        "path": "scripts/casper-soak/src/profiles/authority_finality.rs",
        "id": "scripts-casper-soak-src-profiles-authority-finality-rs",
        "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
        "commit_is_base": true,
        "sha256": "18a1a18cebe632dc5f87483784e08a00c11f3870116f3a3b54766bbf68c45e47"
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
        "member": "docs/casper/cbc-evidence/scripts-casper-soak-src-profiles-authority-finality-rs.md"
      }
    }
  ]
}
```
