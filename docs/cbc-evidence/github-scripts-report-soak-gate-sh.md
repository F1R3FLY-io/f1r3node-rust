# CbC Evidence: .github/scripts/report-soak-gate.sh

- **Status:** discharged
- **Adapter:** embedded
- **Commit:** 3897437e3db927485830549fc38024a4d3460972
- **Verified:** 2026-11-14T17:59:11Z

Claim:

> docs/claims/soak-release-gates.md

```json
{
  "artifact": {
    "path": ".github/scripts/report-soak-gate.sh",
    "id": "github-scripts-report-soak-gate-sh",
    "commit": "3897437e3db927485830549fc38024a4d3460972",
    "sha256": "129397e58189be8168f95f2e5c0a1ffa566070bb6b2dab6cc5a0fdaea31310a7",
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
    "ref": "docs/cbc-evidence/formal/ca-canary-merge-policy-20261114T175911Z.json",
    "sha256": "2bb50561610e343a458c0a7e64ca513ca210f3f1e5ea8dbea7dc099dfe15451e",
    "counterexample": null
  },
  "tiers": {
    "refutation": "bounded-safety-pass",
    "construction": "not-applicable",
    "binding": "passed"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": "2026-11-14T17:59:11Z",
  "verification_id": "b15b5001-f656-47d5-b2d9-5975dba15c34",
  "inputs": {
    "artifact": ".github/scripts/report-soak-gate.sh",
    "source_sha256": "129397e58189be8168f95f2e5c0a1ffa566070bb6b2dab6cc5a0fdaea31310a7",
    "claim": {
      "path": "docs/claims/soak-release-gates.md",
      "sha256": "ecbec65a1cbb4482e80d8fc7dfa6c0f0e35a71ea7f52bfe4a15c4105c9fe6ff0"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/formal/ca-canary-merge-policy-20261114T175911Z.json": "2bb50561610e343a458c0a7e64ca513ca210f3f1e5ea8dbea7dc099dfe15451e"
  }
}
```
