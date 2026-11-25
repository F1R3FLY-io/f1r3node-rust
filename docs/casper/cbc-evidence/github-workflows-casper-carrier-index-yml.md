# CbC Evidence: .github/workflows/casper-carrier-index.yml

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-carrier-index.yml",
    "id": "github-workflows-casper-carrier-index-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "91215fdd0f86d321d10ee7f496c5d361af13e7e2ffe449cb6b821c146b833883",
    "sha256_before_change": "fbf03950c448d1872ac2cdc59772aadf1c9b96f657cda736dd1e9d4a9d2da97d"
  },
  "claim": "docs/claims/casper-soak-carrier-index.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-008"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-carrier-index.md": "4aab7367b93766df504bcd679670b8d4bcfeda3d4cd5ea3a2f6b2f39ca86a236"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-carrier-profile",
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
  "pending": [

  ],
  "previous_ledger": {
    "git_revision": "f7b0cb32f4d4ac8ac28996e99e75e0cd8cdb68c6",
    "path": "docs/casper/cbc-evidence/github-workflows-casper-carrier-index-yml.md",
    "sha256": "70c3a12844215e0f210a005286af364bf242d8b1150cb21e3b50dbe6f8e9750e"
  },
  "evidence_before_change": {
    "kind": "accepted-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-carrier-index-acceptance-20260919-01/report.json",
    "sha256": "06e5c0d15a9eef13c09f5e28c5b553a082685be7dc44f305fed5380e0c7860e7"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
