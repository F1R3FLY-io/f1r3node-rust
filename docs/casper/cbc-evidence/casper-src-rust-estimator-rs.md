# CbC Evidence: casper/src/rust/estimator.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-002](../../claims/casper-soak-authority-finality.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/estimator.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-estimator-rs",
    "sha256": "400b4d46b5532cc5a50ff3fae74425875ded1e9fa10612731a3a4b8c9e21bf45"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "09ed1edd197372af2cc395a636dbe8c2e38090eb81ec9ad48958e457ebc35b47"
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
