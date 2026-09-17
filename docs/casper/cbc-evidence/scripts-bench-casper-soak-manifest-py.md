# CbC Evidence: scripts/bench/casper_soak_manifest.py

**Status:** Pending. The resume fixture does not discharge the full harness claim.

Claim contract: `docs/claims/casper-soak-harness.md`.

```json
{
  "artifact": {
    "path": "scripts/bench/casper_soak_manifest.py",
    "commit": null,
    "id": "scripts-bench-casper-soak-manifest-py",
    "sha256": "95fffcd3fc6530464f4d7fe9d8439965e2c8781a7149e9de6eae3ceb0ddb353e"
  },
  "claim": "docs/claims/casper-soak-harness.md",
  "claim_ids": [
    "CLAIM-CASPER-SOAK-001"
  ],
  "claim_digests": {
    "docs/claims/casper-soak-harness.md": "5ff2320c054ad8922ceaa6d654ec4e90e82c5c09892803e04ddd5f49d5db7932"
  },
  "adapter": "embedded",
  "status": "pending",
  "scope": "harness-only",
  "evidence": {
    "kind": "partial-verification",
    "ref": "docs/casper/cbc-evidence/runs/casper-manifest-resume-20260917-01/report.json",
    "sha256": "82d704d3e7b69e3e6183c690869740b7b594d5f439d9a9f579b8bc1cae17f7a0",
    "detail": "Exact-byte resume fixtures pass. Full binding, capability qualification, and publication remain pending."
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "construction_assumptions": null,
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
