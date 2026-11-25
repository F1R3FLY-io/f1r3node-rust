# CbC Evidence: .github/workflows/casper-version-phlo.yml

The user accepted this bounded pre-merge binding and ratified the workflow tag. Node correctness and live execution remain outside this discharge.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "artifact": {
    "path": ".github/workflows/casper-version-phlo.yml",
    "id": "github-workflows-casper-version-phlo-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "working_tree": true,
    "sha256": "7e40e4f0ed4983ad3553d8125095d980511a12b74b83f9c97f0a1312f000156d",
    "sha256_before_change": "f0b66b1183d12b318b3d62066c4044155a90eba4e332f938c7b51ec752f3583d"
  },
  "claim": "docs/claims/casper-soak-version-phlo.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-007"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-version-phlo.md": "05e750fc023dd55ed869c1175b0e6639ecfd7e2be22ded37f32175003aa6757a"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-protocol-and-phlo-profile",
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
    "path": "docs/casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/previous-metadata.tar.gz",
    "sha256": "fb6c4fb15eeb121a2ffc01f1efd715aceddd9fbb7936a8a4524091d8b195aaae",
    "member": "previous-ledgers/github-workflows-casper-version-phlo-yml.md",
    "member_sha256": "5e6866b09dae3006add3344861173407f6b5cca9b6b4f6fd32931f61c2d76bca"
  },
  "evidence_before_change": {
    "kind": "accepted-bounded-refutation-and-controlled-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/report.json",
    "sha256": "114a1372249ed2488da96f3dc4346a41ea4cc2eddc8702ca3444e4008e52edc3"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```
