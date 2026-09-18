# CbC Evidence: scripts/ci/check-casper-soak-bindings.sh

Focused inventory and interruption checks pass. Complete fixture verification, interrupted capture, and the harness claim remain pending.

```json
{
  "artifact": {
    "path": "scripts/ci/check-casper-soak-bindings.sh",
    "id": "scripts-ci-check-casper-soak-bindings-sh",
    "commit": null,
    "sha256": "3b8000c2e85b27825ca493af665f3d3b6a1b67511bfd5e212daa7195ed470826"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "8a0ccde7e3752aecdcd5d92527b6ddceefd733b551b1ee4bea9f2893802c9d27"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {
    "kind": "partial-driver-binding",
    "ref": "docs/casper/cbc-evidence/runs/casper-binding-gates-20260918-01/report.json",
    "sha256": "f07c3f7955d7a098b3ce4e646d96bd4472d8f99f56bc43689693c2fb9ecb6039"
  },
  "previous_evidence": {
    "kind": "language-migration-verification",
    "ref": "docs/casper/cbc-evidence/runs/casper-rust-migration-20260917-01/report.json",
    "sha256": "6696a0659bb58fa72b27542a414ef4598c3a649bc638136d810d1796c98b2555"
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
  "waiver": null,
  "verified_at": null
}
```
