# CbC Evidence: block-storage/src/rust/dag/block_dag_key_value_storage.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-003](../claims/casper-soak-publication.md)
- [CLAIM-FINALITY-002](../claims/repeat-deploy-carrier-index-equivalence.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
    "sha256": "c81af8d9b76fa66016bb25496c64cd32397dc1ae41ce511eca819b5622a9e0cf"
  },
  "claim": "docs/claims/casper-soak-publication.md; docs/claims/repeat-deploy-carrier-index-equivalence.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-003",
    "CLAIM-FINALITY-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-publication.md": "337a20a602b03fb699095c0f5c857eadcc31e1068064dc5556706af15d3c5c97",
    "docs/claims/repeat-deploy-carrier-index-equivalence.md": "58ef8fec8679514b85104e994b0c3fce5d910d99c1fd34001a504184ac7e260f"
  },
  "adapter": "embedded",
  "status": "pending",
  "evidence": {
    "kind": "scaffold",
    "ref": null,
    "counterexample": null,
    "detail": "No verifier, fixture, or soak has run for these claims."
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "construction_assumptions": null,
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "scaffold_base_commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
  "waiver": null,
  "verified_at": null
}
```
