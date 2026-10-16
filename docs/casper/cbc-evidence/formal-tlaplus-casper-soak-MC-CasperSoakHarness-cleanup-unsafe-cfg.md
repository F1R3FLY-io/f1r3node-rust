# CbC Evidence: formal/tlaplus/casper_soak/MC_CasperSoakHarness_cleanup_unsafe.cfg

The user authorized this bounded harness renewal after fresh hosted verification of the formal-gate workflow. Node execution and protection-rule activation remain outside this discharge.

The previous ledger remains in the archive below. Earlier reports retain their execution identities and limitations.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/MC_CasperSoakHarness_cleanup_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-MC-CasperSoakHarness-cleanup-unsafe-cfg",
    "commit": "191e184be556c1f190748143377ab369586c53b6",
    "commit_is_base": true,
    "sha256": "94510f73bec2b328dc9ccebe5d0d97b6ef3af93570991dfee557a37c3ba5a3a0",
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
    "member": "prior/ledgers/formal-tlaplus-casper-soak-MC-CasperSoakHarness-cleanup-unsafe-cfg.md",
    "member_sha256": "917ded80e276b22c84ba6bc9e6dd688f51059e1375217c5981f3e46039fc0b42"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/formal-tlaplus-casper-soak-MC-CasperSoakHarness-cleanup-unsafe-cfg.md"
}
```
