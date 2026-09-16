# CbC Evidence: casper/src/rust/merging/rejected_slash.rs

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-006](../../claims/casper-soak-slashing.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "casper/src/rust/merging/rejected_slash.rs",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "casper-src-rust-merging-rejected-slash-rs",
    "sha256": "2a1d915f1b695e26659bed7897d2bc06a1d7bb84b4a2f982257a1ff0d6c95786"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "b25e0608368b5525b6716e0f24b5f6c5d895cf50e9891ade268e03c4883e85fb"
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
