# CbC Evidence: scripts/casper-soak/check-version-phlo.sh

The user accepted this bounded pre-merge binding and ratified the workflow tag. Node correctness and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/check-version-phlo.sh",
    "id": "scripts-casper-soak-check-version-phlo-sh",
    "commit": "807bf94dcb0389fdd57100bc32f64eea20f64ea6",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "39399af4f440489cbc05177607b0a80c142115ad4d6977fac1f372af93d9d794"
  },
  "claim": "docs/claims/casper-soak-version-phlo.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-007"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-version-phlo.md": "05e750fc023dd55ed869c1175b0e6639ecfd7e2be22ded37f32175003aa6757a"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-protocol-and-phlo-profile",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-controlled-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/report.json",
    "sha256": "114a1372249ed2488da96f3dc4346a41ea4cc2eddc8702ca3444e4008e52edc3"
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
  "verified_at": "2026-09-19T19:15:26Z",
  "pending": [],
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-version-phlo-acceptance-20260919-01/previous-metadata.tar.gz",
    "sha256": "fb6c4fb15eeb121a2ffc01f1efd715aceddd9fbb7936a8a4524091d8b195aaae",
    "member": "previous-ledgers/scripts-casper-soak-check-version-phlo-sh.md",
    "member_sha256": "39335469096b5312ab69bb77309e4617cbde1b418a4df95a090f4bfa2bee0219"
  }
}
```
