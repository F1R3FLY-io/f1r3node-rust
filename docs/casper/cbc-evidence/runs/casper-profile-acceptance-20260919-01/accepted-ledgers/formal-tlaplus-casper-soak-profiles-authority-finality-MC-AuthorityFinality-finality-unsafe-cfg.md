# CbC Evidence: formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_finality_unsafe.cfg

The user accepted this bounded pre-merge profile binding. Node correctness and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/authority_finality/MC_AuthorityFinality_finality_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-authority-finality-MC-AuthorityFinality-finality-unsafe-cfg",
    "commit": "490d21093a751b902ad4856b3bcadbf3f4af3069",
    "commit_is_base": true,
    "sha256": "3bf95d421574c52be7e5f6c1088750f1b8005bf04678c149502ec88790decda3"
  },
  "claim": "docs/claims/casper-soak-authority-finality.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-002"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-authority-finality.md": "1af422a0628c37ffab2150b98701c954a2875bf77df4eb9f3e69ff56d23feff2"
  },
  "adapter": "embedded",
  "status": "discharged",
  "scope": "bounded-authority-finality-profile",
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
    "member": "docs/casper/cbc-evidence/formal-tlaplus-casper-soak-profiles-authority-finality-MC-AuthorityFinality-finality-unsafe-cfg.md"
  }
}
```
