# CbC Evidence: .github/workflows/casper-recovery.yml

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-recovery.yml",
    "id": "github-workflows-casper-recovery-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "sha256": "b1f16d34161db3d198df771927baad6f367f626aff1bbc12ec351b7734a2fce1",
    "sha256_before_change": "2745a031e27a851a9503874be3a8932d55f74bc61acfab6b6fab2fe1a7e5dc5d"
  },
  "claim": "docs/claims/casper-soak-recovery.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-004"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-recovery.md": "057e4cfe06c19ac3ae7d61a15568fe6c72a8e0083f4737215fdf749cde4c5afe"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-recovery-profile",
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
    "member": "docs/casper/cbc-evidence/github-workflows-casper-recovery-yml.md"
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
