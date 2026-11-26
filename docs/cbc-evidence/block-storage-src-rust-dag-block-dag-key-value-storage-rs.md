# CbC Evidence: block-storage/src/rust/dag/block_dag_key_value_storage.rs

This ledger uses named claims since the migration of 2026-11-25. Before the migration, it held one legacy record for `CLAIM-CASPER-NODE-OBSERVATION-002`, `-003`, and `-005`. A named maintainer accepted each claim separately: 002 at `4c0c0dbe7` and 003 at `237e43d72` (PR #447, review 5294038948), and 005 at `1a9839b0e` (PR #447, comment 5924422936).

The original bytes are in `history/66249e5087ae81aa66fd33ddcd02701e301d536a2148199192da00b58c72128b-block-storage-src-rust-dag-block-dag-key-value-storage-rs.md`. The run package `docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json` gives the mapping.

The file changed after each acceptance. The records for 002, 003, and 005 therefore have the status `pending` until a maintainer accepts them for the current bytes. Each of these records cites its own acceptance in its `acceptance` field, and keeps the original combined record in its `historical` field.


## Claim CLAIM-DAG-INSERT-BATCH-001

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "commit": "77f16f7459b852cd1763a3d4dddddf514e6bc5f0",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs"
  },
  "claim": "docs/claims/dag-insert-batched-writes.md",
  "adapter": "embedded",
  "status": "pending",
  "scope": "binding-tier",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json",
    "counterexample": null,
    "detail": "Binding tier reached. Counting-store tests prove P1, and equivalence tests against test copies of the pre-change writers cover the tested instances of P2. The claim stays pending until the construction tier proves P2 for every block sequence."
  },
  "waiver": null,
  "verified_at": "2026-11-25T15:15:41Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "976c88f6-b55c-474d-8e2d-0fdf482b25e2",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "source_sha256": "a247350dda3ee9a9e6e355cd3d7f709e9f13ce23dddace5f4bf80b47c8b23d6a",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "3561abaa0464cd19d172d1306644448edc65831f045750c5a2386f558e4e605b"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "5323bb409bfa3f4f68768546c287cbc5dcf92e59090aa7c555738163d2908a77"
  }
}
```


## Claim CLAIM-CASPER-NODE-OBSERVATION-002

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "commit": "77f16f7459b852cd1763a3d4dddddf514e6bc5f0",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "adapter": null,
  "status": "pending",
  "scope": "awaiting-maintainer-acceptance",
  "evidence": {
    "kind": "binding-tests-supporting",
    "ref": "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json",
    "counterexample": null,
    "detail": "A maintainer accepted this claim at revision 4c0c0dbe7. The file changed after that acceptance. The node observation binding tests pass on the current bytes, but they do not discharge the claim. The claim waits for a new acceptance."
  },
  "waiver": null,
  "verified_at": "2026-11-25T15:15:40Z",
  "acceptance": {
    "accepted_by": "jltatbeach",
    "accepted_at": "2026-09-23T16:55:28Z",
    "revision": "4c0c0dbe7c8958debefdb02f2b21795786c45900",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "package": "docs/cbc-evidence/runs/casper-node-claim-gate-78d696ea6-01",
    "source": "docs/claims/casper-node-authority-snapshot.md"
  },
  "historical": {
    "artifact": {
      "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
      "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
      "commit_is_base": true,
      "working_tree": false,
      "sha256": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2",
      "sha256_at_registration": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_refresh": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_verification_refresh": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2"
    },
    "claim": "docs/claims/casper-node-display-projection.md",
    "claim_ids": [
      "CLAIM-CASPER-NODE-OBSERVATION-002",
      "CLAIM-CASPER-NODE-OBSERVATION-003",
      "CLAIM-CASPER-NODE-OBSERVATION-005"
    ],
    "claim_digests": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
    },
    "status": "discharged",
    "scope": "batch-e-acceptance-01",
    "previous_record": {
      "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "sha256": "fc02b9a019f31c04bd77045040fcb0ccbcf7640c26585ca6ce89a86676029dce",
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
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
      "sha256": "91df4bc20cc2a38cc11e08d515fea9b70c88e8b041992da77188528e1770d256"
    },
    "claim_digests_before_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "refreshed_at": "2026-10-01T01:06:08.639Z",
    "refresh_scope": "batch-e-final-source-evidence-renewal",
    "claim_digests_before_verification_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "verification_refresh_record": {
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
      "sha256": "413b9ddeec57072ef4f474f83bd98b5bc22389e94a6b6a6beb239ceee182392c"
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
  },
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-002",
  "schema": "cbc-claim/v1",
  "verification_id": "df500e7f-d00d-455c-b4ce-fa3cc5382a76",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "source_sha256": "a247350dda3ee9a9e6e355cd3d7f709e9f13ce23dddace5f4bf80b47c8b23d6a",
    "claim": {
      "path": "docs/claims/casper-node-authority-snapshot.md",
      "sha256": "fccdbb13c9e437d577764db7306db43ee8f4b5bdffbab9d2580e2baf2a8be8d1"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json": "909b65b6fcc69e49fd8d1de3866355a4bf7db9ff6a20472543dd8bb9884a28f4"
  }
}
```


## Claim CLAIM-CASPER-NODE-OBSERVATION-003

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "commit": "77f16f7459b852cd1763a3d4dddddf514e6bc5f0",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs"
  },
  "claim": "docs/claims/casper-node-authority-evaluation.md",
  "adapter": null,
  "status": "pending",
  "scope": "awaiting-maintainer-acceptance",
  "evidence": {
    "kind": "binding-tests-supporting",
    "ref": "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json",
    "counterexample": null,
    "detail": "A maintainer accepted this claim at revision 237e43d72. The file changed after that acceptance. The node observation binding tests pass on the current bytes, but they do not discharge the claim. The claim waits for a new acceptance."
  },
  "waiver": null,
  "verified_at": "2026-11-25T15:15:40Z",
  "acceptance": {
    "accepted_by": "jltatbeach",
    "accepted_at": "2026-09-23T21:01:02Z",
    "revision": "237e43d723b9867985cd47fdfe312fd8d06352e8",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#pullrequestreview-5294038948",
    "package": "docs/cbc-evidence/runs/casper-node-authority-b2-d69e12151-02",
    "source": "docs/claims/casper-node-authority-evaluation.md"
  },
  "historical": {
    "artifact": {
      "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
      "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
      "commit_is_base": true,
      "working_tree": false,
      "sha256": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2",
      "sha256_at_registration": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_refresh": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_verification_refresh": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2"
    },
    "claim": "docs/claims/casper-node-display-projection.md",
    "claim_ids": [
      "CLAIM-CASPER-NODE-OBSERVATION-002",
      "CLAIM-CASPER-NODE-OBSERVATION-003",
      "CLAIM-CASPER-NODE-OBSERVATION-005"
    ],
    "claim_digests": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
    },
    "status": "discharged",
    "scope": "batch-e-acceptance-01",
    "previous_record": {
      "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "sha256": "fc02b9a019f31c04bd77045040fcb0ccbcf7640c26585ca6ce89a86676029dce",
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
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
      "sha256": "91df4bc20cc2a38cc11e08d515fea9b70c88e8b041992da77188528e1770d256"
    },
    "claim_digests_before_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "refreshed_at": "2026-10-01T01:06:08.639Z",
    "refresh_scope": "batch-e-final-source-evidence-renewal",
    "claim_digests_before_verification_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "verification_refresh_record": {
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
      "sha256": "413b9ddeec57072ef4f474f83bd98b5bc22389e94a6b6a6beb239ceee182392c"
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
  },
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-003",
  "schema": "cbc-claim/v1",
  "verification_id": "bbd34980-a81e-4140-910e-f6201f0a6bbc",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "source_sha256": "a247350dda3ee9a9e6e355cd3d7f709e9f13ce23dddace5f4bf80b47c8b23d6a",
    "claim": {
      "path": "docs/claims/casper-node-authority-evaluation.md",
      "sha256": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json": "909b65b6fcc69e49fd8d1de3866355a4bf7db9ff6a20472543dd8bb9884a28f4"
  }
}
```


## Claim CLAIM-CASPER-NODE-OBSERVATION-005

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "commit": "77f16f7459b852cd1763a3d4dddddf514e6bc5f0",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "adapter": null,
  "status": "pending",
  "scope": "awaiting-maintainer-acceptance",
  "evidence": {
    "kind": "binding-tests-supporting",
    "ref": "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json",
    "counterexample": null,
    "detail": "A maintainer accepted this claim at revision 1a9839b0e. The file changed after that acceptance. The node observation binding tests pass on the current bytes, but they do not discharge the claim. The claim waits for a new acceptance."
  },
  "waiver": null,
  "verified_at": "2026-11-25T15:15:40Z",
  "acceptance": {
    "accepted_by": "jltatbeach",
    "accepted_at": "2026-10-01T03:56:25Z",
    "revision": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "record": "https://github.com/F1R3FLY-io/f1r3node-rust/pull/447#issuecomment-5924422936",
    "source": "the acceptance field of the original record"
  },
  "historical": {
    "artifact": {
      "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
      "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
      "commit_is_base": true,
      "working_tree": false,
      "sha256": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2",
      "sha256_at_registration": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_refresh": "9296a9f4e27c6ec0939fe90071985d2d7778705945096bf0345f1c09899ed966",
      "sha256_before_verification_refresh": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2"
    },
    "claim": "docs/claims/casper-node-display-projection.md",
    "claim_ids": [
      "CLAIM-CASPER-NODE-OBSERVATION-002",
      "CLAIM-CASPER-NODE-OBSERVATION-003",
      "CLAIM-CASPER-NODE-OBSERVATION-005"
    ],
    "claim_digests": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
    },
    "status": "discharged",
    "scope": "batch-e-acceptance-01",
    "previous_record": {
      "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "sha256": "fc02b9a019f31c04bd77045040fcb0ccbcf7640c26585ca6ce89a86676029dce",
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
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
      "sha256": "91df4bc20cc2a38cc11e08d515fea9b70c88e8b041992da77188528e1770d256"
    },
    "claim_digests_before_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "refreshed_at": "2026-10-01T01:06:08.639Z",
    "refresh_scope": "batch-e-final-source-evidence-renewal",
    "claim_digests_before_verification_refresh": {
      "docs/claims/casper-node-authority-snapshot.md": "741d16a73647f7107d44f7416dc1b878054ffbc4ae78136365b5fd536b271822",
      "docs/claims/casper-node-authority-evaluation.md": "15cc3c2951f8291b4670cf9554103755833777b857cd700feac1776d02924081",
      "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
    },
    "verification_refresh_record": {
      "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
      "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
      "sha256": "413b9ddeec57072ef4f474f83bd98b5bc22389e94a6b6a6beb239ceee182392c"
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
  },
  "claim_id": "CLAIM-CASPER-NODE-OBSERVATION-005",
  "schema": "cbc-claim/v1",
  "verification_id": "4bee7e69-fdac-408d-9aba-839bff45c1f5",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "source_sha256": "a247350dda3ee9a9e6e355cd3d7f709e9f13ce23dddace5f4bf80b47c8b23d6a",
    "claim": {
      "path": "docs/claims/casper-node-display-projection.md",
      "sha256": "fc708010cdb35534168ba72b63ee5fd9c0f83b319694255bc89d8d5b114bc459"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json": "909b65b6fcc69e49fd8d1de3866355a4bf7db9ff6a20472543dd8bb9884a28f4"
  }
}
```
