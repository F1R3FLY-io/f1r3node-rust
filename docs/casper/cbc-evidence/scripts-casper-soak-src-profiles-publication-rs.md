# CbC Evidence: scripts/casper-soak/src/profiles/publication.rs

The bounded model and controlled fixtures pass. Human binding acceptance remains pending. This record does not discharge the claim.

```json
{
  "artifact": {
    "path": "scripts/casper-soak/src/profiles/publication.rs",
    "id": "scripts-casper-soak-src-profiles-publication-rs",
    "commit": "b5c24da0217465eeef5fb07e3d9126fb759a5b59",
    "commit_is_base": true,
    "sha256": "23fda6085a32c86ac69108ee437a92af92cc4a298bf6ecbd66e60454896c078b"
  },
  "claim": "docs/claims/casper-soak-publication.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-003"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-publication.md": "f894b0908290bafa98ff3ed7e17cbe47a8ff3d01844fe139f0b56a41f74c55a7"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "bounded-publication-profile",
  "evidence": {
    "kind": "bounded-refutation-and-executable-fixtures",
    "ref": "docs/casper/cbc-evidence/runs/casper-publication-20260918-01/report.json",
    "sha256": "2a5479f67c895a27c004f171152e2aff67fb1dff77be2a0d7949cd4175c10884"
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "pending-review"
  },
  "phase_status": {
    "pre_pr216_merge": "pending",
    "post_pr216_merge": "blocked"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-09-18T15:31:59Z"
}
```
