# CbC Evidence: scripts/bench/write-soak-summary.sh

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001",
  "artifact": {
    "path": "scripts/bench/write-soak-summary.sh",
    "id": "scripts-bench-write-soak-summary-sh",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "commit_is_base": true,
    "sha256": "4ae6cf85bddd0f1e96a7b50da1db7e1033952d40244f697ab584846b857ff3e1",
    "working_tree": true
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "c1f6b7473f790a60ab964ce7fd7277841cd6cee5718e9853ad96cf4c671d5932"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "hosted-source-bound-workflow-renewal",
    "ref": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/report.json",
    "sha256": "a7397d9c2fa35436452872d3112bdaae755bc0de0f947955e847d1bf9e4b8831"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "phase_status": {
    "pre_pr216_merge": "discharged",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-19T22:43:31Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-formal-gate-renewal-20260919-01/previous-metadata.tar.gz",
    "sha256": "855516a6685384190c8a4f427ff547fc2c8a0f2b6d894849d26dea9fde91b6d9",
    "member": "prior/ledgers/scripts-bench-write-soak-summary-sh.md",
    "member_sha256": "a4d6dfc2f11299e05b07904cbcc36cda265d467b2f6b834f834426dbb8998bce"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-bench-write-soak-summary-sh.md"
}
```

## Claim CLAIM-CASPER-SOAK-001-008

This record moved from `docs/cbc-evidence/scripts-bench-write-soak-summary-sh.md` during the EPIC-026 R8 migration. The original bytes are Git blob `e248f1b01653f59c00b32737e2f8ebde194058fb` at `docs/cbc-evidence/scripts-bench-write-soak-summary-sh.md` in commit `a59d92667685e3d655f269afd6a010df2fb720a9`.

```json
{
  "schema": "cbc-claim/v1",
  "claim_id": "CLAIM-CASPER-SOAK-001-008",
  "artifact": {
    "path": "scripts/bench/write-soak-summary.sh",
    "commit": "cc7e84b482887f0647277ccbacd6f65ae3cf749d",
    "id": "scripts-bench-write-soak-summary-sh",
    "sha256": "373b612d2b3e09b84cb4d5d5943e7ae13fc21b0b1cb535dbd7a7f4c0678cb061"
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
