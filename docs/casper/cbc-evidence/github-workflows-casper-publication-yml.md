# CbC Evidence: .github/workflows/casper-publication.yml

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-publication.yml",
    "id": "github-workflows-casper-publication-yml",
    "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
    "commit_is_base": true,
    "sha256": "91c25c33c997588985913a9fc511b9b35b79f0e1a03abf733c0c5aac53f20421"
  },
  "claim": "docs/claims/casper-soak-publication.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-003"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-publication.md": "f49995b85ec66fe5da179efeadef28559eaec3df2a55fa69479c7688492b00db"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-publication-profile",
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
    "member": "docs/casper/cbc-evidence/github-workflows-casper-publication-yml.md"
  }
}
```
