# CbC Evidence: casper/src/rust/finality/floor.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-002](../../claims/casper-soak-authority-finality.md)
- [CLAIM-CASPER-SOAK-003](../../claims/casper-soak-publication.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/finality/floor.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-finality-floor-rs",
    "sha256": "e24927b344ddaea0a1625c3f067a2da16058e7d5910d930b6b8af828a473ffe0"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md; docs/claims/casper-soak-publication.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002",
    "CLAIM-CASPER-SOAK-003"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "09ed1edd197372af2cc395a636dbe8c2e38090eb81ec9ad48958e457ebc35b47",
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
