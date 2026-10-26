# CbC Evidence: .github/workflows/slashing-tests.yml

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

The maintainer accepted the PR #829 change of the Java install step of this file on 2026-10-23 (PR #829, review 5628129141) at revision `f2f3645dd`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001",
  "artifact": {
    "path": ".github/workflows/slashing-tests.yml",
    "id": "github-workflows-slashing-tests-yml",
    "commit": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "commit_is_base": false,
    "sha256": "b9164f7d5aa8de7655fb9627b91701fe82787c2af1ba3203d7dfcffe5560ac6b",
    "working_tree": true,
    "sha256_before_change": "915af218194a292ec2c227c5b3e28b396b76eff7a0586fdf7c4fe87febe0eb15"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
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
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/github-workflows-slashing-tests-yml.md",
    "member_sha256": "dde15e00e81ef2bb3676560c669bcaf8143f98e4a1db41e29758e1f69593677e"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/github-workflows-slashing-tests-yml.md",
  "evidence_before_change": {
    "kind": "hosted-source-bound-workflow-renewal",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json",
    "sha256": "a7397d9c2fa35436452872d3112bdaae755bc0de0f947955e847d1bf9e4b8831"
  },
  "acceptance": {
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/829#pullrequestreview-5628129141",
    "revision": "f2f3645ddedae8d612e55141cfbd15e9c4ef884b",
    "reviewed_at": "2026-10-23T12:21:29Z"
  }
}
```

## Claim CLAIM-SOAK-GATE-001

This record moved from `docs/cbc-evidence/github-workflows-slashing-tests-yml.md` during the EPIC-026 R8 migration. The original bytes are Git blob `1dfe9b19964d142846ed487cc55f9d356ba82cbb` at `docs/cbc-evidence/github-workflows-slashing-tests-yml.md` in commit `a59d92667685e3d655f269afd6a010df2fb720a9`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-SOAK-GATE-001",
  "artifact": {
    "path": ".github/workflows/slashing-tests.yml",
    "id": "github-workflows-slashing-tests-yml",
    "commit": "bc226f89b46366ea4cb0c1907d6781050afb7d0c",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "37956499d722765a831cc70633130d419380317d01dface8a366eb0fd3d71fdd"
  },
  "claim": "CLAIM-SOAK-GATE-001",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001"
  ],
  "status": "pending",
  "scope": "agent-b-ci-correction-refresh-20260930-01",
  "adapter": null,
  "evidence": {
    "kind": "source-bound-hosted-checks-and-elf-mutation-controls",
    "ref": "docs/cbc-evidence/runs/node-observation-ci-refresh-20260930-01/report.json",
    "sha256": "7e844fe3c4dfb287de00c093d355ceddf3cbb32209c699577b00eb3b5092bbb3"
  },
  "verification_status": "recorded",
  "acceptance_status": "pending",
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-30T04:44:34.480367+00:00",
  "limits": "The report states the verification limits. Earlier acceptance does not cover the corrected source.",
  "previous_record": {
    "path": "docs/cbc-evidence/github-workflows-slashing-tests-yml.md",
    "commit": "bc226f89b46366ea4cb0c1907d6781050afb7d0c",
    "sha256": "3230815cb3fb435e32ea2ddfdfda619b60a83e52b0deb6bb11ddba3fbc6a6d07"
  },
  "previous_artifact_sha256": "991507a49e77745dd8a6a9ecfbd4be8d2cc9cb3645d9e1b1862b8430cd01267b"
}
```
