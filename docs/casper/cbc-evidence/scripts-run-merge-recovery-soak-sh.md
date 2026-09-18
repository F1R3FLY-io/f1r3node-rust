# CbC Evidence: scripts/run-merge-recovery-soak.sh

The user accepted the bounded H01–H10 binding review for the driver at commit `946743a77`. The driver changed on 2026-09-18, so this record is pending again until a new acceptance binds the current driver. The `drift` field keeps the accepted commit and digest.

Profile verification, node soaks, post-merge work, and inherited containment limits remain separate. Earlier ledger bytes remain in the linked archive.

```json
{
  "artifact": {
    "path": "scripts/run-merge-recovery-soak.sh",
    "id": "scripts-run-merge-recovery-soak-sh",
    "commit": "ab5d92eeee7c487dbcf340c15fc825a754b0a907",
    "sha256": "5552bb673d0735ac04a10566753d80b5eaf6db7d238be6a943c15c384f5187a0",
    "commit_is_base": false
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "5beea6f6bfc7e349b98e7452b80538ab8f2db73847d6e71f1fe0fdc15a966ae8"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-harness-only",
  "evidence": {
    "kind": "accepted-bounded-refutation-and-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/report.json",
    "sha256": "c510e4e393daf52f5a3d2e3affa908897ab0a574484de807939354d5ad8c5ec1"
  },
  "previous_ledger": {
    "ref": "docs/casper/cbc-evidence/runs/casper-task-017-4-acceptance-01/prior-ledgers.tar.gz",
    "sha256": "4869ed3996b96dda70f84cc666ceb6f86d529f982f8c847b260287738d333e52",
    "member": "docs/casper/cbc-evidence/scripts-run-merge-recovery-soak-sh.md"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null,
  "drift": {
    "accepted_commit": "946743a7740e5dd3c0816263c3347e501f2d50d7",
    "accepted_sha256": "7f4ba9b1c9078c984251317a4cb5032772a092645bfb83d07f6950c0b43542b2",
    "changed_at": "2026-09-18",
    "reason": "The driver changed after acceptance: the Python removal and the numeric kill-order fix. The accepted evidence binds the earlier driver only."
  }
}
```
