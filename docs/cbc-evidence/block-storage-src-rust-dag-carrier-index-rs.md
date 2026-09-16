# CbC Evidence: block-storage/src/rust/dag/carrier_index.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-FINALITY-002](../claims/repeat-deploy-carrier-index-equivalence.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/carrier_index.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "block-storage-src-rust-dag-carrier-index-rs",
    "sha256": "c6eb50e73dfc7f3dd019ac56acc1799528427718b230c8f186f6b2f124733247"
  },
  "claim": "docs/claims/repeat-deploy-carrier-index-equivalence.md",
  "claim_ids": [
    "CLAIM-FINALITY-002"
  ],
  "claim_digests": {
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
