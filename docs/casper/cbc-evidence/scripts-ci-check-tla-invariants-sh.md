# CbC Evidence: scripts/ci/check-tla-invariants.sh

The refreshed record covers node observation model registration. Named maintainer acceptance remains pending.

The gate retains its separate `CLAIM-CASPER-SOAK-001` obligations. Its prior discharge applies to the source bytes in the previous record.
The prior harness evidence remains recorded below. This package does not automatically extend that discharge to changed source bytes.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "id": "scripts-ci-check-tla-invariants-sh",
    "commit": "4aa93d11cf7a4c318074975dd4266208575c8aca",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "a12e47c0c9d4bb6c488dadc55baa77e3e5c108f32d2df3e888bc641a25dc442e"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001",
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b6552635d07fae63ac49c0d63cb670b35e6de045e20551deced1ac7f327275d8",
    "docs/claims/casper-node-authority-snapshot.md": "c2bd97d63c74e4fad19a8817a21e044fc358957623c3e8968c3f8535b60964c6",
    "docs/claims/casper-soak-harness.md": "b6d4f83f958af79037c9b938edd52f0a91ef6f4e8858d6a85a2e4aa8b6346faf"
  },
  "adapter": null,
  "status": "pending",
  "scope": "node-observation-model-registration",
  "evidence": {
    "kind": "bounded-models-and-gate-regression-awaiting-acceptance",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-4aa93d11c-03/report.json",
    "sha256": "dc7acd04b4da21aaac99b8a19d0bd56193a85a24a914e9bb09f0334892c6300c"
  },
  "previous_record": {
    "commit": "4aa93d11cf7a4c318074975dd4266208575c8aca",
    "path": "docs/casper/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "sha256": "519c3c3b0e2bb8dd546538b4fa4c4f6399ceda1c51d0e9e7c39377ffb75bcadc"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "waiver": null,
  "verified_at": null,
  "prior_harness_evidence": {
    "scope": "bounded-harness-only",
    "status": "discharged",
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
    "verified_at": "2026-09-19T22:43:31Z",
    "previous_ledger": {
      "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
      "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
      "member": "prior/ledgers/scripts-ci-check-tla-invariants-sh.md",
      "member_sha256": "53d6a6e981a22618e072840cee7af91a33acb3d872c55ac1d29b3d232f9f29cd"
    },
    "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-ci-check-tla-invariants-sh.md"
  },
  "previous_working_tree_record": {
    "archive": "target/task-019-4-rust-verification/previous-metadata.tar.gz",
    "archive_sha256": "b595d2a87fc11dadec09f4d8ac50cc09a977ffc537b945edf06900c742e2243d",
    "member": "docs/casper/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "sha256": "4f00bf92dc209855ab2a55e880742a51ac07d967e48530317ee370e459e35d14"
  }
}
```
