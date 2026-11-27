# CbC evidence for scripts/ci/check-formal-invariants.sh

The node-observation records retain their original source digests and review decisions. The heartbeat CI record applies to the current source.

## Claim CLAIM-CASPER-NODE-OBSERVATION-001

This record retains the shared 001 and 002 acceptance at Git revision `c3e7854f3`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-001",
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "5118a35df0edf44f881d17d567357f139635409e0598302a68f9e6af53ed48cc"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "task-019-4-handoff-cycle-02",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json",
    "sha256": "79361b7df306cde8f0b157d0749dd4b09e2a33b24dd9ad99710ff9d198ee43d2"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "recorded-partial",
    "binding": "recorded-partial"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-23T16:55:28Z",
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "811df6d38ce13c14349f19cad11b5f4bd25c198df096a5080d493a72c252bbc6",
    "commit": "03d7f1b27544b2c5a93b664d8684b24ea16cf3e9"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274",
    "docs/claims/casper-node-authority-snapshot.md": "234b65b479864c0ce6b45c45f8c2f2c971d01e7639fc2343ad0256a3d728e723"
  },
  "acceptance": {
    "reviewer": "jltatbeach",
    "review_id": 5294038948,
    "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
    "submitted_at": "2026-09-23T16:55:28Z",
    "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
    "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded"
  }
}
```

## Claim CLAIM-CASPER-NODE-OBSERVATION-002

This record retains the shared 001 and 002 acceptance at Git revision `c3e7854f3`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-002",
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "5118a35df0edf44f881d17d567357f139635409e0598302a68f9e6af53ed48cc"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "task-019-4-handoff-cycle-02",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01/report.json",
    "sha256": "79361b7df306cde8f0b157d0749dd4b09e2a33b24dd9ad99710ff9d198ee43d2"
  },
  "tiers": {
    "refutation": "recorded",
    "construction": "recorded-partial",
    "binding": "recorded-partial"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-23T16:55:28Z",
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "811df6d38ce13c14349f19cad11b5f4bd25c198df096a5080d493a72c252bbc6",
    "commit": "03d7f1b27544b2c5a93b664d8684b24ea16cf3e9"
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274",
    "docs/claims/casper-node-authority-snapshot.md": "234b65b479864c0ce6b45c45f8c2f2c971d01e7639fc2343ad0256a3d728e723"
  },
  "acceptance": {
    "reviewer": "jltatbeach",
    "review_id": 5294038948,
    "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
    "submitted_at": "2026-09-23T16:55:28Z",
    "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
    "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded"
  }
}
```

## Claim CLAIM-CASPER-NODE-OBSERVATION-003

This record retains the Batch B2 acceptance at Git revision `fc30018b8`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-003",
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "d69e12151082d111f18c9e8f818ed7f3ba4f9377",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66",
    "accepted_sha256": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66"
  },
  "claim": "docs/claims/casper-node-observation.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081"
  },
  "adapter": null,
  "status": "discharged",
  "scope": "batch-b2-construction-01",
  "evidence": {
    "kind": "tiered-evidence-accepted",
    "ref": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02/report.json",
    "sha256": "4a9a8d296e921baab5e006be3e6aa5eb0181af8515921aa49330bb3f9bb43bf5"
  },
  "tiers": {
    "refutation": "inherited-models-only",
    "construction": "recorded-partial",
    "binding": "recorded"
  },
  "soak": "pending",
  "waiver": null,
  "acceptance": {
    "claim_ids": [
      "CLAIM-CASPER-NODE-OBSERVATION-003"
    ],
    "reviewer": "jltatbeach",
    "review_id": 5294038948,
    "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "revision": "237e43d723b9867985cd47fdfe312fd8d06352e8",
    "verified_at": "2026-09-23T21:01:02Z",
    "review_edited_in_place": true,
    "package": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02",
    "decisions": "C1, C13 accepted as bounded by design; construction gaps C2, C6, C10, C11, C14, C15 and the C4, C9, C12 remaining parts accepted as recorded; C7, C8 extension accepted",
    "artifact_sha256_at_acceptance": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66",
    "claim_digests_at_acceptance": {
      "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
    }
  },
  "accepted_claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "a04073da177f2e6a78fa644712c1303cff5b42752a094440242a138472d3405b",
    "commit": "78d696ea6780ca72105d9b35d2266b0329d777ed"
  },
  "accepted_at": "2026-09-23T21:01:02Z",
  "acceptance_scope": "The acceptance of CLAIM-CASPER-NODE-OBSERVATION-003 applies at revision 237e43d723b9867985cd47fdfe312fd8d06352e8 and the accepted digest. The earlier acceptance of claims 001 and 002 at revision 4c0c0dbe7c8958debefdb02f2b21795786c45900 is retained in acceptances.",
  "verified_at": "2026-09-23T21:01:02Z",
  "acceptances": [
    {
      "reviewer": "jltatbeach",
      "review_id": 5294038948,
      "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
      "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
      "submitted_at": "2026-09-23T16:55:28Z",
      "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
      "decisions": "A1, A2, A8, A10, B13 accepted as bounded by design; construction gaps A3, A4, A9, B9, B11, B12 and the A7 and B2 deadline parts accepted as recorded",
      "claim_ids": [
        "CLAIM-CASPER-NODE-OBSERVATION-001",
        "CLAIM-CASPER-NODE-OBSERVATION-002"
      ],
      "artifact_sha256_at_acceptance": "5118a35df0edf44f881d17d567357f139635409e0598302a68f9e6af53ed48cc",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-observation.md": "703418566c148f28a4d952e29e7dc135843242ea7fbb7e3fdc3111b6e4373274",
        "docs/claims/casper-node-authority-snapshot.md": "234b65b479864c0ce6b45c45f8c2f2c971d01e7639fc2343ad0256a3d728e723"
      },
      "verified_at": "2026-09-23T16:55:28Z"
    },
    {
      "claim_ids": [
        "CLAIM-CASPER-NODE-OBSERVATION-003"
      ],
      "reviewer": "jltatbeach",
      "review_id": 5294038948,
      "url": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
      "revision": "237e43d723b9867985cd47fdfe312fd8d06352e8",
      "verified_at": "2026-09-23T21:01:02Z",
      "review_edited_in_place": true,
      "package": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02",
      "decisions": "C1, C13 accepted as bounded by design; construction gaps C2, C6, C10, C11, C14, C15 and the C4, C9, C12 remaining parts accepted as recorded; C7, C8 extension accepted",
      "artifact_sha256_at_acceptance": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66",
      "claim_digests_at_acceptance": {
        "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
        "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
        "docs/claims/casper-node-authority-evaluation.md": "5956cff23feb6e32e175976019d4e483dc7755bc7a9040b6f2d35a0bebf758fe"
      }
    }
  ]
}
```

## Claim CLAIM-CASPER-NODE-OBSERVATION-005

This record retains the Batch E acceptance on dev at `3c6a46d2`. The maintainer accepted claim 005 in PR #447 on 2026-10-01.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-005",
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "8cf26f199b4ac1970cb74a6bf8f5c803bba411a5662b06367db99651ca618213",
    "sha256_at_registration": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66",
    "sha256_before_refresh": "d69aa2f8f72db96be2703f7ab8deb37c24ee0a96ee599c0d175c8bf22d1dda66",
    "sha256_before_verification_refresh": "8cf26f199b4ac1970cb74a6bf8f5c803bba411a5662b06367db99651ca618213"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-001",
    "CLAIM-CASPER-NODE-OBSERVATION-002",
    "CLAIM-CASPER-NODE-OBSERVATION-003",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "discharged",
  "scope": "batch-e-acceptance-01",
  "previous_record": {
    "artifact": "scripts/ci/check-formal-invariants.sh",
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "sha256": "2d59a2499c3742b7f1e12d5853c6bb4a38c3bb8147935ff72e450e82c9f54ba5",
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
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "99b68c3d313c9389c8152cc7945145d26aeb1d203299f99e0a68f2d5d29df984"
  },
  "claim_digests_before_refresh": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal",
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-observation.md": "b865e33216b210a8915662e6ebd4f91d2b397a68f8f4dcf31fcd306aa8ca6c38",
    "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
    "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "175f52478a274f8e0b46c342e85298a2dcb01946fbc58cde60a8ed40c4a12a6b"
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


## Claim CLAIM-HEARTBEAT-WAKE-CI-001

```json
{
  "artifact": {
    "path": "scripts/ci/check-formal-invariants.sh",
    "id": "scripts-ci-check-formal-invariants-sh",
    "commit": "60853a71900c38021f0315a9f14fae71d6e1553b"
  },
  "claim": "docs/claims/heartbeat-wake-ci.md",
  "adapter": "manual",
  "status": "discharged",
  "previous_record": {
    "path": "docs/cbc-evidence/scripts-ci-check-formal-invariants-sh.md",
    "commit": "60853a71900c38021f0315a9f14fae71d6e1553b",
    "sha256": "c9e6a8fceb5a495b3b35c5e0aeb8a83c0c66769cdd4415cf7dfc7b6f88212a45"
  },
  "evidence": {
    "kind": "formal-gate-registration-check",
    "ref": "formal/tlaplus/heartbeat_wake/README.md",
    "detail": "The 2026-11-27 review accepted heartbeat Rocq registration and the declaration guard. All eight scratch controls failed as expected, and the missing source directory failed. The clean run passed all eight builds and kernel checks. All 75 listed theorem checks reported closed assumptions."
  },
  "waiver": null,
  "verified_at": "2026-11-27T13:31:37Z",
  "verification_id": "6d60c0c8-84d4-4cce-a0ee-93351d2097c8",
  "inputs": {
    "artifact": "scripts/ci/check-formal-invariants.sh",
    "source_sha256": "902dd2a717231b4867e521fb371caacf069e87232ce120a37dde12792692b6e6",
    "claim": {
      "path": "docs/claims/heartbeat-wake-ci.md",
      "sha256": "f2243fdad69cce95f8e7dc9958bb20364ba8fa1eb693deed119c0e8a5d44b63f"
    }
  },
  "evidence_files": {
    "formal/tlaplus/heartbeat_wake/README.md": "925efffc5914b988249cfeee7de9b41ca3c05a0d61496a37f0a9ca88816a570a"
  },
  "scope": "heartbeat-rocq-registration-and-declaration-guard",
  "manual_review": {
    "reviewed_on": "2026-11-27",
    "revision": "60853a71900c38021f0315a9f14fae71d6e1553b",
    "technical_review": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/768#issuecomment-7219211333",
    "approval_ref": "Maintainer approval to proceed with step 7 on 2026-11-27"
  },
  "claim_id": "CLAIM-HEARTBEAT-WAKE-CI-001",
  "schema": "cbc-claim/v1"
}
```
