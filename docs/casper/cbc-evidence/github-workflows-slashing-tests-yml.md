# CbC Evidence: .github/workflows/slashing-tests.yml

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001",
  "artifact": {
    "path": ".github/workflows/slashing-tests.yml",
    "id": "github-workflows-slashing-tests-yml",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "commit_is_base": true,
    "sha256": "1821ac5e6d95226e323423806efb1de9ceda8f423b5fa19bdc7572bf2209cc3b",
    "working_tree": true
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "c1f6b7473f790a60ab964ce7fd7277841cd6cee5718e9853ad96cf4c671d5932"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "hosted-source-bound-workflow-renewal",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json",
    "sha256": "a7397d9c2fa35436452872d3112bdaae755bc0de0f947955e847d1bf9e4b8831"
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
  "verified_at": "2026-09-19T22:43:31Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/github-workflows-slashing-tests-yml.md",
    "member_sha256": "dde15e00e81ef2bb3676560c669bcaf8143f98e4a1db41e29758e1f69593677e"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/github-workflows-slashing-tests-yml.md"
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
