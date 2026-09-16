# CbC Evidence: scripts/run-merge-recovery-soak.sh

**Status:** Pending scaffold. No verification result exists for the claims below.

- [CLAIM-CASPER-SOAK-001](../claims/casper-soak-harness.md)
- [CLAIM-CASPER-SOAK-002](../claims/casper-soak-authority-finality.md)
- [CLAIM-CASPER-SOAK-003](../claims/casper-soak-publication.md)
- [CLAIM-CASPER-SOAK-004](../claims/casper-soak-recovery.md)
- [CLAIM-CASPER-SOAK-005](../claims/casper-soak-merge-accounting.md)
- [CLAIM-CASPER-SOAK-006](../claims/casper-soak-slashing.md)
- [CLAIM-CASPER-SOAK-007](../claims/casper-soak-version-phlo.md)
- [CLAIM-CASPER-SOAK-008](../claims/casper-soak-carrier-index.md)

These claims cover the harness and profiles only. The node is the system under test, not a proof artifact.

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "commit": "40e2d2d7c409d9f644c04221286bd95a187d1434",
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
    "docs/claims/casper-soak-harness.md": "a280729b02c810dda2a4386444581919eeae9046a365bbcc4a391c54bea6cd7b",
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
