# CbC Evidence: block-storage/tests/soak_snapshot.rs

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-005 at revision `1a9839b0e` on 2026-10-01 (PR #447, comment 5924422936). This record is discharged for the Batch E bytes of the file. The registration gap stays recorded in `registration_gap`. The artifact bytes changed on 2026-10-02 through the PR #447 review remediation `490dc0d51`: the test fixtures hold a `tempfile::TempDir`, so the LMDB directories are removed. The acceptance covers the bytes at `1a9839b0e`. The maintainer has not reviewed the remediation.

```json
{
  "artifact": {
    "path": "block-storage/tests/soak_snapshot.rs",
    "id": "block-storage-tests-soak-snapshot-rs",
    "commit": "cf11229eb0b1e49fecd098b4c26782cf624fea9b",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "66d8854484290af7a227ba8cbd653c814c38c8ec236c8522a76771774ae46a09",
    "sha256_at_registration": "15840e51d9ba769d2053760972ad35d71a7a67419ff3c0e4d981852372c90b09",
    "sha256_before_refresh": "15840e51d9ba769d2053760972ad35d71a7a67419ff3c0e4d981852372c90b09",
    "sha256_before_verification_refresh": "89b316b204937a1721adbd711ae3bf87ed56ccbe3bbe5f142445f92621c2b520",
    "sha256_at_acceptance": "f04c69d90ee1559ac48cf0e7bab7abfb0c4dd83769ecea46caa8a4cc99a2ef2b"
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
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "sha256": "c81add42ebb1daacba185e48307f3123f1820b8d3cd40feb5761b520e987ad3c",
    "commit": "e38e7433e3deae7da36fe2b5f9d6b39403b283db",
    "status": "discharged",
    "artifact_sha256": "f04c69d90ee1559ac48cf0e7bab7abfb0c4dd83769ecea46caa8a4cc99a2ef2b"
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
  "verified_at": "2026-10-03T14:10:00+00:00",
  "previous_refresh_record": {
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "2f93b4caa79c992f72b5b3f389c2472e98c87473865e56c5d52d4d21ef85aab0"
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
    "path": "docs/cbc-evidence/block-storage-tests-soak-snapshot-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "b49270f0b2bcb7f16815f2665e1a8e24ceeece866aee7b3e4781972ba8fb80fe"
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
  },
  "refresh": {
    "date": "2026-10-03",
    "reason": "Review remediation 490dc0d51 changed unique_dir to return the TempDir guard with the path and stored the guard in the fixture. Test assertions are unchanged.",
    "delta_source": "branch commit 490dc0d51 (PR #447 review, comment 5965523714)",
    "branch_bytes_unchanged": false,
    "acceptance_covers_branch_change": false
  }
}
```
