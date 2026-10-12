# CbC Evidence: formal/tlaplus/casper_soak/profiles/merge_accounting/MC_MergeAccounting_settlement_unsafe.cfg

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/merge_accounting/MC_MergeAccounting_settlement_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-merge-accounting-MC-MergeAccounting-settlement-unsafe-cfg",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "bc7b3f34a625150c1a84b9cfb393d8f861e0c951e8988b9291a6c7093e74c8fc"
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
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/formal-tlaplus-casper-soak-profiles-merge-accounting-MC-MergeAccounting-settlement-unsafe-cfg.md",
    "sha256": "72fc05d532fd73d7a80e82d285a6bd07313edac2d9e0dff1d2d22e0da0933cde"
  }
}

```
