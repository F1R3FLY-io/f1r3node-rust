# CbC Evidence: scripts/ci/check-tla-invariants.sh

The refreshed record covers node observation model registration. Named maintainer acceptance remains pending.

The gate retains its separate `CLAIM-CASPER-SOAK-001` obligations. Its prior discharge applies to the source bytes in the previous record.
The prior harness evidence remains recorded below. This package does not automatically extend that discharge to changed source bytes.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-001",
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

## Claim CLAIM-CASPER-NODE-OBSERVATION-005

This record moved from `docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md` during the EPIC-026 R8 migration. The original bytes are Git blob `c4a7284a05682aedf155d606f23bd5dd5e1ae1d1` at `docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md` in commit `a59d92667685e3d655f269afd6a010df2fb720a9`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-005",
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "id": "scripts-ci-check-tla-invariants-sh",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "e66529c2356c4cbca0f10da9b3e72495450178f99f5c45699f7750a1083bf0b6",
    "sha256_at_registration": "ca116baad47417a16dc7ad4f60cacf18cdb54fb5e7916bbb174965459632a3ed",
    "sha256_before_refresh": "ca116baad47417a16dc7ad4f60cacf18cdb54fb5e7916bbb174965459632a3ed",
    "sha256_before_verification_refresh": "e66529c2356c4cbca0f10da9b3e72495450178f99f5c45699f7750a1083bf0b6"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "discharged",
  "scope": "batch-e-acceptance-01",
  "previous_record": {
    "artifact": "scripts/ci/check-tla-invariants.sh",
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "sha256": "48ab8170ab49c620e1eb01d1b587a28138238736e1921a195c0503f570204cd7",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f"
  },
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json",
    "sha256": "52f766491135dcf59b72d935e8ef697251fba1cebbe2a5d3f11ae5d47f87393a"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "recorded-partial",
    "binding": "recorded"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-10-01T03:59:08+00:00",
  "previous_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "c20e890a0430bad7e0ab6d70065368c54e11e8e2d3cd67902bf5d573a995cd41"
  },
  "claim_digests_before_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal",
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-tla-invariants-sh.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "845d77d915ca90f43ce8b3aa9efbed516f45442ef09a247233f3ce6c05403ea2"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  },
  "acceptance": {
    "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-005",
    "accepted_by": "jltatbeach",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5924422936",
    "revision": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "reviewed_at": "2026-10-01T03:56:25Z"
  }
}
```
