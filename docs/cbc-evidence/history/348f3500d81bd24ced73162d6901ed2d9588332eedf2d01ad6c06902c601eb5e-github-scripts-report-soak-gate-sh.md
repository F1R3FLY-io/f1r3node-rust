# CbC Evidence: .github/scripts/report-soak-gate.sh

- **Status:** discharged
- **Adapter:** embedded
- **Commit:** e91f2d8a07a3653adedc7ded045d8d043350d881
- **Verified:** 2026-11-11T06:05:26Z

Claim:

> docs/claims/soak-release-gates.md

```json
{
  "artifact": {
    "path": ".github/scripts/report-soak-gate.sh",
    "id": "github-scripts-report-soak-gate-sh",
    "commit": "e91f2d8a07a3653adedc7ded045d8d043350d881",
    "sha256": "5329a9d557c239505d51e4259a53f9ea8f35ebabe9a693fbec76b38976138d55",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "docs/claims/soak-release-gates.md",
  "claim_ids": [
    "CLAIM-SOAK-GATE-001"
  ],
  "adapter": "embedded",
  "status": "discharged",
  "scope": "optional-item-and-release-soak-controls",
  "evidence": {
    "kind": "bounded-script-tests",
    "ref": "docs/cbc-evidence/formal/ca-canary-merge-policy-20261111T060513Z.json",
    "sha256": "05b7a1d4f1f99e7ed1c8a7a74af33dbaf77cdffbd96c7a1946a7089f66803b0b",
    "counterexample": null
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-11-11T06:05:26Z",
  "verification_id": "53bd44ec-1afa-4f7f-844c-62708a9daaa5",
  "inputs": {
    "artifact": ".github/scripts/report-soak-gate.sh",
    "source_sha256": "5329a9d557c239505d51e4259a53f9ea8f35ebabe9a693fbec76b38976138d55",
    "claim": {
      "path": "docs/claims/soak-release-gates.md",
      "sha256": "ecbec65a1cbb4482e80d8fc7dfa6c0f0e35a71ea7f52bfe4a15c4105c9fe6ff0"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/formal/ca-canary-merge-policy-20261111T060513Z.json": "05b7a1d4f1f99e7ed1c8a7a74af33dbaf77cdffbd96c7a1946a7089f66803b0b"
  }
}
```
