# CbC Evidence: scripts/run-merge-recovery-soak.sh

**Status:** Pending. The bounded model result does not discharge the full harness/profile claim.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)
- [CLAIM-CASPER-SOAK-002](../claims/casper-soak-authority-finality.md)
- [CLAIM-CASPER-SOAK-003](../claims/casper-soak-publication.md)
- [CLAIM-CASPER-SOAK-004](../claims/casper-soak-recovery.md)
- [CLAIM-CASPER-SOAK-005](../claims/casper-soak-merge-accounting.md)
- [CLAIM-CASPER-SOAK-006](../claims/casper-soak-slashing.md)
- [CLAIM-CASPER-SOAK-007](../claims/casper-soak-version-phlo.md)
- [CLAIM-CASPER-SOAK-008](../claims/casper-soak-carrier-index.md)

The evidence package contains partial results only. No node correctness claim or construction proof belongs to this scope.

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "commit": "cc7e84b482887f0647277ccbacd6f65ae3cf749d",
    "id": "scripts-run-merge-recovery-soak-sh",
    "sha256": "8d6c071e85c57ed4d1d9a5666f3ef99d47aaae898383a1c80822f66977c83f68"
  },
  "claim": "docs/claims/casper-soak-harness.md; docs/claims/casper-soak-authority-finality.md; docs/claims/casper-soak-publication.md; docs/claims/casper-soak-recovery.md; docs/claims/casper-soak-merge-accounting.md; docs/claims/casper-soak-slashing.md; docs/claims/casper-soak-version-phlo.md; docs/claims/casper-soak-carrier-index.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001",
    "CLAIM-CASPER-SOAK-002",
    "CLAIM-CASPER-SOAK-003",
    "CLAIM-CASPER-SOAK-004",
    "CLAIM-CASPER-SOAK-005",
    "CLAIM-CASPER-SOAK-006",
    "CLAIM-CASPER-SOAK-007",
    "CLAIM-CASPER-SOAK-008"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "636bdd0681096a3fb054a88f114641e4b23812001bff02d873dce5f6d52aa4c5",
    "docs/claims/casper-soak-authority-finality.md": "c3a2a33ba88af166e897da4659b5b6d2aa2f657144edfdc041637107d6c713ec",
    "docs/claims/casper-soak-publication.md": "00ada28c16cc9387de8d762d646174a2cd36117c5718a93962fcced5a0fd74c9",
    "docs/claims/casper-soak-recovery.md": "6aaa2b3d2de8c3eaa5432e46defed512902101d20ed444222b86ec8d9446cae6",
    "docs/claims/casper-soak-merge-accounting.md": "1d347351b057f7510c5854516b9fce6060fb8c757b34817289565d9261952a64",
    "docs/claims/casper-soak-slashing.md": "bb6a9470a7d67dd9bff80c81cb8188990798f183732c9bb7ef203c0bacaaa5a4",
    "docs/claims/casper-soak-version-phlo.md": "98787a08770d40e8e27f1e395a7128b3d82b6004e6a762417dae7da5bd4b9b34",
    "docs/claims/casper-soak-carrier-index.md": "0b203c184aa3f846f25e0d655dc0f2c3df8e9550f5dbafe971193b55f8588f10"
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
      "cycle": "restart-state-data-loading",
      "status": "partial-binding-pass",
      "base_revision": "c9faa7d2f4c1cf4564b544a6cf3aa4c95f869d26",
      "artifact_sha256": "4a2a909b56d9e6585e897726aa9867a63193894627622a954d65dff32b23f32b",
      "ref": "docs/casper/cbc-evidence/runs/casper-driver-integration-20260917-01/report.json",
      "sha256": "f8ad774a9856bc9d986058b77caa6e375c92dbc2a0ebccda589afadad339285c",
      "detail": "RED executed saved shell input. GREEN validated restart data without execution. Six state cases and 42 disk scenarios passed. Manifest bindings remain pending."
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
