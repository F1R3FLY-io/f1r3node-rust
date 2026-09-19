# CbC Evidence: formal/tlaplus/casper_soak/profiles/merge_accounting/verification-plan.jsonc

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/merge_accounting/verification-plan.jsonc",
    "id": "formal-tlaplus-casper-soak-profiles-merge-accounting-verification-plan-jsonc",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "ecf4584bdb69ec856a6ea01deb2768facf05e2c9e813391cc41396e38dd78fed"
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
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/formal-tlaplus-casper-soak-profiles-merge-accounting-verification-plan-jsonc.md",
    "sha256": "234750570a8f64c4f555eb428cd01c45f20ff9db3eafe4f191c7b5e8ae7d7b6b"
  }
}

```
