# CbC Evidence: formal/tlaplus/casper_soak/CasperSoakHarness.tla

**Status:** Pending. The bounded model result does not discharge the full harness/profile claim.

- CLAIM-CASPER-SOAK-001 (`docs/claims/casper-soak-harness.md`)

The evidence package contains partial results only. No node correctness claim or construction proof belongs to this scope.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/CasperSoakHarness.tla",
    "commit": null,
    "id": "formal-tlaplus-casper-soak-CasperSoakHarness-tla",
    "sha256": "e08ce354f1e09b9f0873e6f730e509c4be6b394acc0bce7ff5ef53805d91b0c4"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "636bdd0681096a3fb054a88f114641e4b23812001bff02d873dce5f6d52aa4c5"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-and-profiles-only",
  "evidence": {
    "kind": "partial-verification",
    "ref": "docs/casper/cbc-evidence/runs/casper-harness-controls-20260916-01/report.json",
    "sha256": "b192d4c2152081f3d47c26fa951a98d14d96aaeeed74d55818c14457134346c7",
    "counterexample": null,
    "detail": "Bounded lifecycle controls and runner unit tests pass. Driver/profile bindings, shared CI integration, and soaks remain pending."
  },
  "tiers": {
    "refutation": "bounded-pass",
    "construction": "not-applicable",
    "construction_assumptions": null,
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "scaffold_base_commit": "cc7e84b482887f0647277ccbacd6f65ae3cf749d",
  "waiver": null,
  "verified_at": null
}
```
