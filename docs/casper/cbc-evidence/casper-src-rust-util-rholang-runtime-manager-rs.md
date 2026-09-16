# CbC Evidence: casper/src/rust/util/rholang/runtime_manager.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-005](../../claims/casper-soak-merge-accounting.md)
- [CLAIM-CASPER-SOAK-007](../../claims/casper-soak-version-phlo.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/util/rholang/runtime_manager.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-util-rholang-runtime-manager-rs",
    "sha256": "c8c59de931744ff84230571a95a7328bd15940f7db6925e01bb5975100ee8142"
  },
  "claim": "docs/claims/casper-soak-merge-accounting.md; docs/claims/casper-soak-version-phlo.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-005",
    "CLAIM-CASPER-SOAK-007"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-merge-accounting.md": "26026d6d89db86d76256290b10b87f50ee63bd2f1b4aeefb15b56519f2254cde",
    "docs/claims/casper-soak-version-phlo.md": "a387d213c0d3d4f7b7f424bd6862d37d288608fe091bf1c8880821638ca1c68e"
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
