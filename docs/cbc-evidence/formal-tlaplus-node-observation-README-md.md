# CbC Evidence: formal/tlaplus/node_observation/README.md

The named maintainer accepted CLAIM-CASPER-NODE-OBSERVATION-005 at revision `1a9839b0e` on 2026-10-01 (PR #447, comment 5924422936). This record is discharged for the Batch E bytes of the file. The registration gap stays recorded in `registration_gap`.

```json
{
  "artifact": {
    "path": "formal/tlaplus/node_observation/README.md",
    "id": "formal-tlaplus-node-observation-README-md",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "b0e066cdeb0b1e3b6f541f9261cfcbba8f4de690acefa75ceffb5b9c8a8f171f",
    "sha256_at_registration": "73a969dcbd908fc69769f8c54cb2572a1310df4ccd3e6cb820417b7521d9fd1b",
    "sha256_before_refresh": "73a969dcbd908fc69769f8c54cb2572a1310df4ccd3e6cb820417b7521d9fd1b",
    "sha256_before_verification_refresh": "73a969dcbd908fc69769f8c54cb2572a1310df4ccd3e6cb820417b7521d9fd1b"
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
  "status": "discharged",
  "scope": "batch-e-acceptance-01",
  "previous_record": {
    "artifact": "formal/tlaplus/node_observation/README.md",
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md",
    "sha256": "48ba2133320195bab6c5f464e2549e719ff96a5362762de92fb93c681313c9c9",
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "10df20d31b19080abbfcc1d7aa27877d8a33ff028bc353db0faf166aceba115e"
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
    "path": "docs/cbc-evidence/formal-tlaplus-node-observation-README-md.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "ab237470465609559b455dd34d52786f014c5c638c02a4185684caefbdd12405"
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
