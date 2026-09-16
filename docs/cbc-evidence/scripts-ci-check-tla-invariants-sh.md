# CbC Evidence: scripts/ci/check-tla-invariants.sh

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)

These claims cover the harness and profiles only. The node is the system under test, not a proof artifact.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": "40e2d2d7c409d9f644c04221286bd95a187d1434",
    "id": "scripts-ci-check-tla-invariants-sh",
    "sha256": "ff2ca9e5e6b045db231e374cb43dbec2896e3bdea64e43b321ad73945974eeb9"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "a280729b02c810dda2a4386444581919eeae9046a365bbcc4a391c54bea6cd7b"
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
  "scaffold_base_commit": "40e2d2d7c409d9f644c04221286bd95a187d1434",
  "waiver": null,
  "verified_at": null,
  "scope": "harness-and-profiles-only"
}
```
