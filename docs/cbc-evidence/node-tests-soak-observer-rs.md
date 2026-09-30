# CbC Evidence: node/tests/soak_observer.rs

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

```json
{
  "artifact": {
    "path": "node/tests/soak_observer.rs",
    "id": "node-tests-soak-observer-rs",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "b39993c70bce6ef6594c733c2ad7061dfbe902ede77993b2c9377a939b10e354",
    "sha256_at_registration": "e70f2f90d0d2a8a42e02082322c8a1432c585c61dca8eb58f3243a84248290cc",
    "sha256_before_refresh": "e70f2f90d0d2a8a42e02082322c8a1432c585c61dca8eb58f3243a84248290cc"
  },
  "claim": "docs/claims/casper-node-display-projection.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-004",
    "CLAIM-CASPER-NODE-OBSERVATION-005"
  ],
  "claim_digests": {
    "docs/claims/casper-node-fork-choice-observation.md": "c007214aad05afbcd100c0e37fd18b166463147b5fa2e1e797743ab814b15a49",
    "docs/claims/casper-node-display-projection.md": "ace855346324009b5f08972b5c12e043abaf8889ff40501b525d0fa09cd49dce"
  },
  "status": "pending",
  "scope": "batch-e-registration",
  "previous_record": {
    "path": "docs/cbc-evidence/node-tests-soak-observer-rs.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "d30a7f6bc0440a440a6c72f1e7ce338a6ab308e637b45dada2dc3cb4d9ded1f6"
  },
  "evidence": null,
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
  "refreshed_at": "2026-09-30T22:43:21.676Z",
  "refresh_scope": "batch-e-source-registration-renewal"
}
```
