# CbC Evidence: formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg

The user accepted the bounded H01–H10 binding review. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

Profile verification, node soaks, post-merge work, and inherited containment limits remain separate. Earlier ledger bytes remain in the linked archive.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/MC_CasperSoakHarness_samples_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-MC-CasperSoakHarness-samples-unsafe-cfg",
    "commit": "946743a7740e5dd3c0816263c3347e501f2d50d7",
    "sha256": "6937418bcfafd33557d27359f34e5e53e78fcfd68b43e68bfb3617d86cb6022b",
    "commit_is_base": false
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "c711fdc42bfed34ce28a6cd5da407e27d2e4745037b0eb250d8e760bee24a071"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/report.json",
    "sha256": "c510e4e393daf52f5a3d2e3affa908897ab0a574484de807939354d5ad8c5ec1"
  },
  "previous_ledger": {
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/prior-ledgers.tar.gz",
    "sha256": "4869ed3996b96dda70f84cc666ceb6f86d529f982f8c847b260287738d333e52",
    "member": "docs/casper/cbc-evidence/formal-tlaplus-casper-soak-MC-CasperSoakHarness-samples-unsafe-cfg.md"
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
  "verified_at": "2026-09-18T04:31:12Z"
}
```
