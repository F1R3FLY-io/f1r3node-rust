# CbC Evidence: scripts/ci/test-check-tla-invariants.sh

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "scripts/ci/test-check-tla-invariants.sh",
    "id": "scripts-ci-test-check-tla-invariants-sh",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "ac647001494500c87fdff20b5c1743508e643fab96cc41505b15762fbf7b8c00",
    "sha256_at_registration": "af7fd80db3397c204e96da3ec706083e9c39e89430dc9970384af075c66025e6",
    "sha256_before_refresh": "af7fd80db3397c204e96da3ec706083e9c39e89430dc9970384af075c66025e6",
    "sha256_before_verification_refresh": "ac647001494500c87fdff20b5c1743508e643fab96cc41505b15762fbf7b8c00"
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
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "783e4dc1508ba3e6de1c505db62313b8748b715e762fce51f85cfbc23bb8e133"
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
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "6d5ec6c42eac1250387178bf620942317162c0d7e1974660ff5a8b6b83bd4cfa"
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
    "path": "docs/cbc-evidence/scripts-ci-test-check-tla-invariants-sh.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "4bfd9be2599dc204d2910e77a6d0db64261cbd244e4a106dae630c0b9dd89630"
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
