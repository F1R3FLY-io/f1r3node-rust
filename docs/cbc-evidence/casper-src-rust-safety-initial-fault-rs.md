# CbC Evidence: casper/src/rust/safety/initial_fault.rs

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-005 at revision `1a9839b0e` on 2026-10-01 (PR #447, comment 5924422936). This record is discharged for the Batch E bytes of the file. The registration gap stays recorded in `registration_gap`.

```json
{
  "artifact": {
    "path": "casper/src/rust/safety/initial_fault.rs",
    "id": "casper-src-rust-safety-initial-fault-rs",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "a5bfd2da4bad84cf727762f4d8834e879279bad4ada7dfe850db3ee5e0717592",
    "sha256_at_registration": "f4faa014891dc292fe17304ce6ce6d72b1c7eea8bab77f437d61d9d8f57acc1c",
    "sha256_before_refresh": "e83a34307654ba7b33c7d2aabefc00dc91532e87a198d7fe10c5cf85cdef6bad",
    "sha256_before_verification_refresh": "a5bfd2da4bad84cf727762f4d8834e879279bad4ada7dfe850db3ee5e0717592"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "discharged",
  "scope": "batch-e-acceptance-01",
  "claim_sha256_at_registration": "a016b2c43ee5c7b2e24c2944e4a329fddcb6c3056f9321be9054b21007e56e4c",
  "previous_record": {
    "artifact": "casper/src/rust/safety/initial_fault.rs",
    "path": "docs/cbc-evidence/casper-src-rust-safety-initial-fault-rs.md",
    "sha256": "3d84ae98355c2a0c832f1ca0804e29e87943bb3cc2e951644590f0c2194fc726",
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
    "path": "docs/cbc-evidence/casper-src-rust-safety-initial-fault-rs.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "517143e34763b39364127800d020fae2e1d1ba60fe3b893b51da03814f365a7d"
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
    "path": "docs/cbc-evidence/casper-src-rust-safety-initial-fault-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "fe27769799dc94ed46f4e00269d948934afbd994e3b74bbee78882e08710584b"
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
