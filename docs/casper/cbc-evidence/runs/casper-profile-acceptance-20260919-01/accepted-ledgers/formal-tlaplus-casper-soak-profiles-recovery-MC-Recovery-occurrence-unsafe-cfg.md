# CbC Evidence: formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_occurrence_unsafe.cfg

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/recovery/MC_Recovery_occurrence_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-recovery-MC-Recovery-occurrence-unsafe-cfg",
    "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
    "commit_is_base": true,
    "sha256": "9d12e5541c2fa5c5c61801a85d450551878e405965bdc8ae73a36f5d6c652ecd"
  },
  "claim": "docs/claims/casper-soak-recovery.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-004"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-recovery.md": "057e4cfe06c19ac3ae7d61a15568fe6c72a8e0083f4737215fdf749cde4c5afe"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-recovery-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-profile-acceptance-20260919-01/report.json",
    "sha256": "614d13c3e52ed11afb84a9a0b5f6345db42bc3a684518f9ce7060b15fc08c4da"
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
  "verified_at": "2026-09-19T03:05:12Z",
  "previous_ledger": {
    "archive": "docs/casper/cbc-evidence/runs/casper-profile-binding-review-20260919-01/ledgers.tar.gz",
    "sha256": "52d9f45f0cff756cc13da2b5770023fd57072c4c5ba984d3822bb75a26acd5db",
    "member": "docs/casper/cbc-evidence/formal-tlaplus-casper-soak-profiles-recovery-MC-Recovery-occurrence-unsafe-cfg.md"
  }
}
```
