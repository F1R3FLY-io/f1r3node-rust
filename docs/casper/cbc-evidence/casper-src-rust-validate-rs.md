# CbC Evidence: casper/src/rust/validate.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-002](../../claims/casper-soak-authority-finality.md)
- [CLAIM-CASPER-SOAK-004](../../claims/casper-soak-recovery.md)
- [CLAIM-CASPER-SOAK-006](../../claims/casper-soak-slashing.md)
- [CLAIM-CASPER-SOAK-007](../../claims/casper-soak-version-phlo.md)
- [CLAIM-FINALITY-002](../../claims/repeat-deploy-carrier-index-equivalence.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/validate.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-validate-rs",
    "sha256": "fad8b162aa25d865f4159cc5906e146bbdb1f26bcc8586f9a912c56158e9cebc"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md; docs/claims/casper-soak-recovery.md; docs/claims/casper-soak-slashing.md; docs/claims/casper-soak-version-phlo.md; docs/claims/repeat-deploy-carrier-index-equivalence.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002",
    "CLAIM-CASPER-SOAK-004",
    "CLAIM-CASPER-SOAK-006",
    "CLAIM-CASPER-SOAK-007",
    "CLAIM-FINALITY-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "09ed1edd197372af2cc395a636dbe8c2e38090eb81ec9ad48958e457ebc35b47",
    "docs/claims/casper-soak-recovery.md": "413977506c780ee97d004fc9efe5932914b27f497d041474a8438db57cadee9f",
    "docs/claims/casper-soak-slashing.md": "b25e0608368b5525b6716e0f24b5f6c5d895cf50e9891ade268e03c4883e85fb",
    "docs/claims/casper-soak-version-phlo.md": "a387d213c0d3d4f7b7f424bd6862d37d288608fe091bf1c8880821638ca1c68e",
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
