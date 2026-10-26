# CbC Evidence: formal/tlaplus/casper_soak/profiles/merge_accounting/MC_MergeAccounting_epoch_unsafe.cfg

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/merge_accounting/MC_MergeAccounting_epoch_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-merge-accounting-MC-MergeAccounting-epoch-unsafe-cfg",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "8dc47ad44039efe548dc101b6e69a15204d57a9f6d94a519f98b7d247488c199"
  },
  "claim": "docs/claims/casper-soak-merge-accounting.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-005"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-merge-accounting.md": "22a6534ef37b1675b6fe69cd04dbae6473d2fa93d9306efb454499cd04c970f3"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-merge-accounting-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-merge-accounting-acceptance-20260919-01/report.json",
    "sha256": "ba73b6ff31e85ee528490be184a8b95441977bcaba7ceac6eb905fdb1e0a576b"
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
  "verified_at": "2026-09-19T06:42:15Z",
  "previous_ledger": {
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/formal-tlaplus-casper-soak-profiles-merge-accounting-MC-MergeAccounting-epoch-unsafe-cfg.md",
    "sha256": "74cbe2c5371027252333ba9254aed19bd147d5fc4796633026d6562d0253de4c"
  }
}

```
