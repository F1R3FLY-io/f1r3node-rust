# CbC Evidence: .github/workflows/ca-soak-dispatch.yml

- **Status:** discharged
- **Adapter:** embedded
- **Commit:** 3897437e3db927485830549fc38024a4d3460972
- **Verified:** 2026-11-14T17:59:11Z

Claim:

> docs/claims/soak-release-gates.md

```json
{
  "artifact": {
    "path": ".github/workflows/ca-soak-dispatch.yml",
    "id": "github-workflows-ca-soak-dispatch-yml",
    "commit": "3897437e3db927485830549fc38024a4d3460972",
    "sha256": "6d05acdd7ded17690e0d7e1e9f575628a6f8a24106ff01c945582d748362742c",
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
  "verification_id": "c821a430-a544-4fb5-8ea3-8f1174bc9a5e",
  "inputs": {
    "artifact": ".github/workflows/ca-soak-dispatch.yml",
    "source_sha256": "6d05acdd7ded17690e0d7e1e9f575628a6f8a24106ff01c945582d748362742c",
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
