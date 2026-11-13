# CbC Evidence: .github/workflows/casper-slashing.yml

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-slashing.yml",
    "id": "github-workflows-casper-slashing-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "working_tree": true,
    "sha256": "99d58b578284114f6e88c6ea9cc3b188d273438df66445dfb44162ca195520fc",
    "sha256_before_change": "53a2f0bcf732405cde493c1aaf13868913ad58c5d97a5116059d90b5cd502675"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "6954ff5fe9edb29909f88f94b328aced5229ac5c7387b67d02c185c4b717845b"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-slashing-profile",
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
    "external-evidence-publication"
  ],
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/previous-ledgers.tar.gz",
    "sha256": "084343234fa3ce8b30c87a0de1143cbd32f91b7685873ade6dacce906ddfb998",
    "member": "github-workflows-casper-slashing-yml.md",
    "member_sha256": "df12a54515cff2deb0574ab1182128fe7c1916411f49c7ce8888cf0fa3e97734"
  },
  "evidence_before_change": {
    "kind": "ratified-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/report.json",
    "sha256": "b47dc09e4d3f901503b88ae0ce346c94aecc4a0fc60d3b76079c8b19f54070ca"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
