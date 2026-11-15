# CbC Evidence: .github/scripts/ca-soak-dispatch.sh

- **Status:** discharged
- **Adapter:** embedded
- **Commit:** e91f2d8a07a3653adedc7ded045d8d043350d881
- **Verified:** 2026-11-11T06:05:26Z

Claim:

> docs/claims/soak-release-gates.md

```json
{
  "artifact": {
    "path": ".github/scripts/ca-soak-dispatch.sh",
    "id": "github-scripts-ca-soak-dispatch-sh",
    "commit": "e91f2d8a07a3653adedc7ded045d8d043350d881",
    "sha256": "68e8ae11a78955dc58d6079fc6d353e55d9d219e6c4bdbe13b25863ec129f3de",
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
  "verification_id": "15fb00e3-8af6-4408-8f99-3fa29a764147",
  "inputs": {
    "artifact": ".github/scripts/ca-soak-dispatch.sh",
    "source_sha256": "68e8ae11a78955dc58d6079fc6d353e55d9d219e6c4bdbe13b25863ec129f3de",
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
