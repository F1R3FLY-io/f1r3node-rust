# CbC Evidence: scripts/bench/casper-soak.sh

The user accepted the refreshed bounded H01–H10 binding review on 2026-09-19. This record discharges only CLAIM-CASPER-SOAK-001 in the pre-merge phase.

Profile claims, node soaks, post-merge work, and inherited containment limits remain separate. The previous acceptance package preserves the earlier records.

```json
{
  "artifact": {
    "path": "scripts/bench/casper-soak.sh",
    "id": "scripts-bench-casper-soak-sh",
    "commit": "ab682eea1760c50867dcf7416e37f155b63e5dbc",
    "commit_is_base": true,
    "sha256": "19b6e0c9ff481428955c68631fc1224ec2506502d01a3c3f1bf6634d8a64f353"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "053fd6236464828a7315c5cb4015491c9b1fc798f09375eaefb25ea22da2e061"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding-refresh",
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-refresh-acceptance-20260919-01/report.json",
    "sha256": "95f1f8aff4bed6463b9520b9d70ae4827854b84d76a5cbf590be1f7742bfab07"
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
  "verified_at": "2026-09-19T02:47:12Z",
  "previous_ledger": {
    "ref": "docs/casper/cbc-evidence/runs/casper-driver-rebind-acceptance-20260918-01/ledgers.tar.gz",
    "sha256": "74614d00fa9643d6df096b8e774598b34e1040b4f80559987468f00fde685e5d",
    "member": "./docs/casper/cbc-evidence/scripts-bench-casper-soak-sh.md"
  },
  "review_candidate": "docs/casper/cbc-evidence/runs/casper-driver-rebind-20260918-01/candidate-ledgers/scripts-bench-casper-soak-sh.md"
}
```
