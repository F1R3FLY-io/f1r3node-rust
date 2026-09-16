# CbC Evidence: casper/src/rust/engine/engine_cell.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-003](../../claims/casper-soak-publication.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/engine/engine_cell.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-engine-engine-cell-rs",
    "sha256": "8709d051da6b61c20c8badfd2063cf7c1e3d1f044049f202c37f8cd368a507bf"
  },
  "claim": "docs/claims/casper-soak-publication.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-003"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-publication.md": "337a20a602b03fb699095c0f5c857eadcc31e1068064dc5556706af15d3c5c97"
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
