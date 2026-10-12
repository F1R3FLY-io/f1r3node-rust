# CbC Evidence: formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_fields_unsafe.cfg

The user accepted this bounded pre-merge binding and ratified the workflow tag. Node correctness and live execution remain outside this discharge.

```json
{
  "artifact": {
    "path": "formal/tlaplus/casper_soak/profiles/version_phlo/MC_VersionPhlo_fields_unsafe.cfg",
    "id": "formal-tlaplus-casper-soak-profiles-version-phlo-MC-VersionPhlo-fields-unsafe-cfg",
    "commit": "807bf94dcb0389fdd57100bc32f64eea20f64ea6",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "cdcd43f3e8c0fcff6e8a4f8371077f2da2d4a0f6729c5d4e63a611fcc8c0948c"
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
    "member": "previous-ledgers/formal-tlaplus-casper-soak-profiles-version-phlo-MC-VersionPhlo-fields-unsafe-cfg.md",
    "member_sha256": "6d3a072cc5ee389b6f148de3e6ab1b7fe0324c9f54dca4e7e5b6a412ac621f27"
  }
}
```
