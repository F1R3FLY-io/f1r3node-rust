# CbC Evidence: formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_order_unsafe.cfg

The user accepted this bounded pre-merge binding and ratified the workflow tag. Live execution remains outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/slashing/MC_Slashing_order_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-slashing-MC-Slashing-order-unsafe-cfg",
    "commit": "137b74fdb903969d186aef9241380eef24dd4833",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "7ff73a5f463d6510c84f9042e816db00687c4c86203917ed8e327d0c7885e6bf"
  },
  "claim": "docs/claims/casper-soak-slashing.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-006"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-slashing.md": "6954ff5fe9edb29909f88f94b328aced5229ac5c7387b67d02c185c4b717845b"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-slashing-profile",
  "evidence": {
    "kind": "ratified-bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/report.json",
    "sha256": "b47dc09e4d3f901503b88ae0ce346c94aecc4a0fc60d3b76079c8b19f54070ca"
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
  "verified_at": "2026-09-19T16:44:10Z",
  "pending": [
    "external-evidence-publication"
  ],
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-slashing-ratification-20260919-01/previous-ledgers.tar.gz",
    "sha256": "084343234fa3ce8b30c87a0de1143cbd32f91b7685873ade6dacce906ddfb998",
    "member": "formal-tlaplus-casper-soak-profiles-slashing-MC-Slashing-order-unsafe-cfg.md",
    "member_sha256": "86c54bec4a38d31b056efa3fe206a2b100e3b799525b7a88d6785cf813be0361"
  }
}
```
