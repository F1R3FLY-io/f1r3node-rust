# CbC Evidence: node/tests/soak_observer.rs

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "node/tests/soak_observer.rs",
    "id": "node-tests-soak-observer-rs",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "d2893e4805ab7485126c8357f843359f403bf1cbae89d5681c0629828e1a0d18",
    "sha256_at_registration": "e70f2f90d0d2a8a42e02082322c8a1432c585c61dca8eb58f3243a84248290cc",
    "sha256_before_refresh": "e70f2f90d0d2a8a42e02082322c8a1432c585c61dca8eb58f3243a84248290cc",
    "sha256_before_verification_refresh": "b39993c70bce6ef6594c733c2ad7061dfbe902ede77993b2c9377a939b10e354"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "c007214aad05afbcd100c0e37fd18b166463147b5fa2e1e797743ab814b15a49",
    "docs/claims/casper-node-display-projection.md": "6310f626415a6ba2d1250e2d656fdcb576415c6153deda1a26da55c617b8faad"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "previous_record": {
    "path": "docs/cbc-evidence/node-tests-soak-observer-rs.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "d30a7f6bc0440a440a6c72f1e7ce338a6ab308e637b45dada2dc3cb4d9ded1f6"
  },
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
    "path": "docs/cbc-evidence/node-tests-soak-observer-rs.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "c7ff74d647b59259bb9751aa2a58a02790168e210c954abfbcfbb561998b8ce6"
  },
  "claim_digests_before_refresh": {
    "docs/claims/casper-node-fork-choice-observation.md": "24619b06b7c3c4b3849227cea5126c237a9f45e97c628205003866ee6c3c1940",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "refreshed_at": "2026-10-01T01:06:08.639Z",
  "refresh_scope": "batch-e-final-source-evidence-renewal",
  "claim_digests_before_verification_refresh": {
    "docs/claims/casper-node-fork-choice-observation.md": "c007214aad05afbcd100c0e37fd18b166463147b5fa2e1e797743ab814b15a49",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "verification_refresh_record": {
    "path": "docs/cbc-evidence/node-tests-soak-observer-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "d80497ad456b3cf9ebe1c8c80f1d9240eea6eaae43914eb264f8793f25029232"
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
