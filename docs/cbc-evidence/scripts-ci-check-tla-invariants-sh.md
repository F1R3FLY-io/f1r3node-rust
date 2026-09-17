# CbC Evidence: scripts/ci/check-tla-invariants.sh

**Status:** Pending. The bounded model result does not discharge the full harness/profile claim.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)

The evidence package contains partial results only. No node correctness claim or construction proof belongs to this scope.

```json
{
  "artifact": {
    "path": "scripts/ci/check-tla-invariants.sh",
    "commit": "cc7e84b482887f0647277ccbacd6f65ae3cf749d",
    "id": "scripts-ci-check-tla-invariants-sh",
    "sha256": "ff2ca9e5e6b045db231e374cb43dbec2896e3bdea64e43b321ad73945974eeb9"
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
  "incremental_evidence": [
    {
      "cycle": "H10-trace",
      "status": "partial-binding-pass",
      "base_revision": "6814682e4c7de98f883b8791d3227e98bc3f2c15",
      "artifact_sha256": "5ad5058f26280a0f90730f6b8c14c40c5326ca0d2fc0dcf9c7ba8018be0397c8",
      "ref": "docs/casper/cbc-evidence/runs/casper-control-trace-20260917-01/report.json",
      "sha256": "7df040249f5ec1aed98b662ecf0913dc705283a328dd2f6ccadce13352d6e135",
      "detail": "RED reproduced a missing-trace acceptance. GREEN rejected the truncated trace. Full classification and CI bindings remain pending."
    },
    {
      "cycle": "H10-positive-search",
      "status": "partial-binding-pass",
      "base_revision": "4204340b0d573e14df9b224ae43e0c0aea083136",
      "artifact_sha256": "8b72bf9f0190b8b15b71d0e3da6af5af07ac8d811b923f402bc2952bc716cd7c",
      "ref": "docs/casper/cbc-evidence/runs/casper-positive-search-20260917-01/report.json",
      "sha256": "db4db3a796ccd1e7bfd8d11754c8b7870826df9030944b727ad6d9cf52ba3ea5",
      "detail": "RED reproduced acceptance of an incomplete positive search. GREEN requires a completed-search marker. Full classification remains pending."
    }
  ],
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
  "scaffold_base_commit": "cc7e84b482887f0647277ccbacd6f65ae3cf749d",
  "waiver": null,
  "verified_at": null
}
```
