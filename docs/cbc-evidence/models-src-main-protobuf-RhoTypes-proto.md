# CbC Evidence: models/src/main/protobuf/RhoTypes.proto

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-007](../claims/casper-soak-version-phlo.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "models/src/main/protobuf/RhoTypes.proto",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "models-src-main-protobuf-RhoTypes-proto",
    "sha256": "14d9a8f880ba75db88ce9d2edff5b780be7ea5c368512ad2519f4612e8793afe"
  },
  "claim": "docs/claims/casper-soak-version-phlo.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-007"
  ],
  "claim_digests": {
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
