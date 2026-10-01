# CbC Evidence: casper/src/rust/api/block_api.rs

This record registers the approved Batch E claim before the production source changes.
The registration binds the handoff source and claim bytes.
It supplies no acceptance evidence.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "casper/src/rust/api/block_api.rs",
    "id": "casper-src-rust-api-block-api-rs",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "fe33e07118d43ab733870ac54840e648d4196a5890188b1b5343b5593d60cd26",
    "sha256_at_registration": "7c1553e367da17fe05b96aec1ce721afcc03e63c1a1c7d10d9ef06d7d2883892",
    "sha256_before_refresh": "fe33e07118d43ab733870ac54840e648d4196a5890188b1b5343b5593d60cd26",
    "sha256_before_verification_refresh": "fe33e07118d43ab733870ac54840e648d4196a5890188b1b5343b5593d60cd26"
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
  "claim_sha256_at_registration": "a016b2c43ee5c7b2e24c2944e4a329fddcb6c3056f9321be9054b21007e56e4c",
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
  "previous_refresh_record": {
    "path": "docs/cbc-evidence/casper-src-rust-api-block-api-rs.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "fd3ca83ed505c1c6bc710fd91531962ba7329d187067393178291b51548baf37"
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
    "path": "docs/cbc-evidence/casper-src-rust-api-block-api-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "2047073ddca9b4248d7499e08f658a629db281c6a5bbe2a68525509e295795ad"
  },
  "verification_observation": {
    "scope": "batch-e-final-source-verification-20261001-01",
    "refutation": "bounded-models-passed",
    "construction": "scoped-integer-proofs-passed-with-declared-gaps",
    "binding": "named-source-tests-passed-not-refinement",
    "adapter_discharge": false
  }
}
```
