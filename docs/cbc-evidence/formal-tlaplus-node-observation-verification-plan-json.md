# CbC Evidence: formal/tlaplus/node_observation/verification-plan.json

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/verification-plan.json",
    "id": "formal-tlaplus-node-observation-verification-plan-json",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "d133d8a2df991f34e9d71ef7b52648d4268cca43ef202e9ce307e448e41d6808",
    "sha256_at_registration": "cc0995fad1f025fed13ebc103c39a14ac9eb904f8b89c04160312486f906c96f",
    "sha256_before_refresh": "cc0995fad1f025fed13ebc103c39a14ac9eb904f8b89c04160312486f906c96f",
    "sha256_before_verification_refresh": "d133d8a2df991f34e9d71ef7b52648d4268cca43ef202e9ce307e448e41d6808"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-verification-plan-json.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "2fdb8410c1b193d58333ff0ea0af3b7df941da6fd9b029c02f3004e86f40a743"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-verification-plan-json.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "4958ea4c603e310deef922639d0aa2b466d76a863cc9f5c78c2f9a3f761367c1"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-verification-plan-json.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "3b360406c19e13b2a88645338082a5f1328789be2cd300bf51c086018ce44a4b"
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
