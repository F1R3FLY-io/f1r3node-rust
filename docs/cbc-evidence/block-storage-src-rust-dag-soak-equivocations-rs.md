# CbC Evidence: block-storage/src/rust/dag/soak_equivocations.rs

This record registers existing source bytes after their implementation commit.
The registration gap remains explicit.
No verification or acceptance is claimed.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/soak_equivocations.rs",
    "id": "block-storage-src-rust-dag-soak-equivocations-rs",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "ddd1ff9c4ca71df0697758cbffa0db0c320035256fa798fa7aaa33c22a646738",
    "sha256_at_registration": "ddd1ff9c4ca71df0697758cbffa0db0c320035256fa798fa7aaa33c22a646738",
    "sha256_before_verification_refresh": "ddd1ff9c4ca71df0697758cbffa0db0c320035256fa798fa7aaa33c22a646738"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "registered_at": "2026-09-30T22:43:21.676Z",
  "registration_gap": "source_existed_before_first_record_registration",
  "source_registration_checkpoint": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
  "previous_record": null,
  "evidence": {
    "path": "docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json",
    "sha256": "52f766491135dcf59b72d935e8ef697251fba1cebbe2a5d3f11ae5d47f87393a",
    "validation": "docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/validation.json",
    "acceptance": "pending"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/block-storage-src-rust-dag-soak-equivocations-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "42caeae2851fc5075518ba9f94da16304bc406843039923189e734250014a390"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal"
}
```
