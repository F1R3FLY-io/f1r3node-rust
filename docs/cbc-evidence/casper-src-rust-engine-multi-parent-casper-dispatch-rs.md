# CbC Evidence: casper/src/rust/engine/multi_parent_casper/dispatch.rs

This record registers the approved Batch E claim before the production source changes.
The registration binds the handoff source and claim bytes.
It supplies no acceptance evidence.

The final source refresh preserves the historical identities and the registration gaps.
The report records verification observations, not named acceptance.
The status remains pending.

```json
{
  "artifact": {
    "path": "casper/src/rust/engine/multi_parent_casper/dispatch.rs",
    "id": "casper-src-rust-engine-multi-parent-casper-dispatch-rs",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "commit_is_base": false,
    "working_tree": false,
    "sha256": "7f20e21e0808fee5e22d279ef4d9760553bce05ef26761395ee800372027a664",
    "sha256_at_registration": "e917a68e49d130b99a16a7c446c3409cba9c49c5d6811bea7e7ed66ef422b68a",
    "sha256_before_refresh": "7f20e21e0808fee5e22d279ef4d9760553bce05ef26761395ee800372027a664",
    "sha256_before_verification_refresh": "7f20e21e0808fee5e22d279ef4d9760553bce05ef26761395ee800372027a664"
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
    "path": "docs/cbc-evidence/casper-src-rust-engine-multi-parent-casper-dispatch-rs.md",
    "commit": "7b023678c8ab6e32fe8266624cf10e087ef89d57",
    "sha256": "624e959d52e72801a8b57ce6ec4d9db866f9e2690df7982ef4317dca32f928a2"
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
    "path": "docs/cbc-evidence/casper-src-rust-engine-multi-parent-casper-dispatch-rs.md",
    "commit": "c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03",
    "sha256": "fd624e9d7b152ac5b6e80de5d2a52aa457052d110ed4eda6243ff570bed2c8c2"
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
