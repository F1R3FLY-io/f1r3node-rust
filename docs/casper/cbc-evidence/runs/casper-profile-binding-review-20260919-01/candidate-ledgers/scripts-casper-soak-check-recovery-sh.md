# Pending CbC Candidate: scripts/casper-soak/check-recovery.sh

Human binding acceptance remains pending. This record does not discharge a claim.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/check-recovery.sh",
    "id": "scripts-casper-soak-check-recovery-sh",
    "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
    "commit_is_base": true,
    "sha256": "1784eb2d09dbf6a2cc0996aee84952107ead4c7f17e6ce571ebd65464ba032e2"
  },
  "claim": "docs/claims/casper-soak-recovery.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-004"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-recovery.md": "3b38680bbf99b7156609b3b05c2e21590fbecd5149a006def590e4e49895ce6d"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-recovery-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/report.json",
    "sha256": "b4b2cedccde429e4f07adff9d9ce5f4c7c729114b696bfae5574bdb65a470c57"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending-review"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-19T02:59:51Z",
  "previous_ledger": {
    "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
    "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
    "member": "docs/casper/cbc-evidence/scripts-casper-soak-check-recovery-sh.md"
  }
}
```
