# CbC Evidence: formal/tlaplus/casper_soak/MC_CasperSoakHarness_control_unsafe.cfg

The user accepted the repaired bounded H01–H10 binding review. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

Profile claims, node soaks, post-merge work, and inherited containment limits remain separate. The linked archive preserves the previous acceptance records.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/MC_CasperSoakHarness_control_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-MC-CasperSoakHarness-control-unsafe-cfg",
    "commit": "f9273621c8887947b56d0093a71486338312138e",
    "commit_is_base": false,
    "sha256": "4fcf3c1b1b609a7a557a28da8daa7e908911a669d51bbb4abb1affeb5f2f90d9"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "51a863feac95f73d06aab10b45ef05ff01b07d5ebb91d1a4fb4879f5776c0134"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01/report.json",
    "sha256": "d5cc58da34bb33dce627c3c40bf055e61fef817657698921359f06a4f8a301b1"
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
  "verified_at": "2026-09-18T20:44:50Z",
  "previous_ledger": {
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/previous.tar.gz",
    "sha256": "690412e55d4e43d92980f9ba069237e1136d89730375dde24ab5724c4eeb1084",
    "member": "./docs/casper/cbc-evidence/formal-tlaplus-casper-soak-MC-CasperSoakHarness-control-unsafe-cfg.md"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/formal-tlaplus-casper-soak-MC-CasperSoakHarness-control-unsafe-cfg.md"
}
```
