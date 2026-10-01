# CbC Evidence: formal/tlaplus/node_observation/bindings.json

This pending successor preserves the committed prior record through `previous_record`.
Earlier acceptance does not cover the Batch E source changes.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/bindings.json",
    "id": "formal-tlaplus-node-observation-bindings-json",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "a6567f8bdcf4cebca29163910ad93fe1418e1151b6d583ca628f4cba2866884e",
    "sha256_at_registration": "4ade939bc06cb1195c7537539c91930f18c90ab5889e024c0d8ef543fe6756d9",
    "sha256_before_refresh": "4ade939bc06cb1195c7537539c91930f18c90ab5889e024c0d8ef543fe6756d9",
    "sha256_before_verification_refresh": "4ade939bc06cb1195c7537539c91930f18c90ab5889e024c0d8ef543fe6756d9"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-bindings-json.md",
    "commit": "28606f1103343a6d4e1e425a7d9a96c93459c57a",
    "sha256": "9effa039201442ff0e43f12b5d5f9b8b34916db4bc539707de5f9a0a2ad2e03a"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-bindings-json.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "83a3021114c21ac1e1f237875a1c7d08c266e08429f678538e08ded4029b0f76"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-bindings-json.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "0d275d6ccd57d83cf837d36e0a97fe1f95bd538c84a982db4de7169d717df2d3"
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
