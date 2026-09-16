# CbC Evidence: scripts/ci/check-tla-invariants.sh

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)

The source digest records scaffold inputs, not proof. Post-merge evidence requires the actual #216 merge and new bindings.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": "7646b65ac230f9bb8421d08a9833ffcb1b4f82d1",
    "id": "scripts-ci-check-tla-invariants-sh",
    "sha256": "ff2ca9e5e6b045db231e374cb43dbec2896e3bdea64e43b321ad73945974eeb9"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "3df463a1d696fa19dbd4e2cd2934fdda22a0a17b414e452fce5e4877b18a9271"
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
    "construction": "not-applicable",
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
