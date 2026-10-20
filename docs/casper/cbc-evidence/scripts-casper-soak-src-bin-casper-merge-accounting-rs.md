# CbC Evidence: scripts/casper-soak/src/bin/casper-merge-accounting.rs

The user accepted this bounded pre-merge binding. Node accounting and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/bin/casper-merge-accounting.rs",
    "id": "scripts-casper-soak-src-bin-casper-merge-accounting-rs",
    "commit": "134e1deaa78d65dda9dd5df49404115044fb9b1e",
    "commit_is_base": true,
    "sha256": "127b1e4ffffa0047bb0860ee08ba52f3ce2affc980e330652d95b99ef46de506"
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
    "path": "docs/casper/cbc-evidence/runs/casper-merge-accounting-20260919-01/ledgers/scripts-casper-soak-src-bin-casper-merge-accounting-rs.md",
    "sha256": "d15bab88ea8b90661645649f68691f22aab945504444532981754b2887ace91b"
  }
}

```
