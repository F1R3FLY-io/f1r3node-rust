# CbC Evidence: formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-MC-CasperSoakHarness-samples-unsafe-cfg",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "commit_is_base": true,
    "sha256": "6937418bcfafd33557d27359f34e5e53e78fcfd68b43e68bfb3617d86cb6022b",
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
    "member": "prior/ledgers/formal-tlaplus-casper-soak-MC-CasperSoakHarness-samples-unsafe-cfg.md",
    "member_sha256": "9a9da3415770a68774ee59e8fc4179f27d086a0c05ad8e1186ca8960f0541934"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/formal-tlaplus-casper-soak-MC-CasperSoakHarness-samples-unsafe-cfg.md"
}
```
