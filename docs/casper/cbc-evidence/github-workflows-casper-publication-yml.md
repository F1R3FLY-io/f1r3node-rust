# CbC Evidence: .github/workflows/casper-publication.yml

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-publication.yml",
    "id": "github-workflows-casper-publication-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "sha256": "e8789552da5e982e6d01df684f23aba23f5cb2dd341f31e3cb7cbd2e68b07ab9",
    "sha256_before_change": "91c25c33c997588985913a9fc511b9b35b79f0e1a03abf733c0c5aac53f20421"
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
    "kind": "accepted-change",
    "ref": "docs/casper/cbc-evidence/runs/ci-preinstalled-jdk-acceptance-20261023-01/report.json",
    "sha256": "987eb5ae70d8b738c7adaa7619ff71a713a5f0e804cf067d136c44de873c7bd9"
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
  "verified_at": "2026-10-23T12:21:29Z",
  "previous_ledger": {
    "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
    "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
    "member": "docs/casper/cbc-evidence/github-workflows-casper-publication-yml.md"
  },
  "evidence_before_change": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json",
    "sha256": "614d13c3e52ed11afb84a9a0b5f6345db42bc3a684518f9ce7060b15fc08c4da"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
