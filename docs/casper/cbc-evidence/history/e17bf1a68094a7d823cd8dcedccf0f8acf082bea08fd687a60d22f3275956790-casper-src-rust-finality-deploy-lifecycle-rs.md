# CbC Evidence: casper/src/rust/finality/deploy_lifecycle.rs

- **Status:** pending
- **Adapter:** rust-regression
- **Commit:** 3adb6942037fa14ef624c97791770db3cea4cf43
- **Verified:** 2026-11-27T10:24:57Z

Claim:

> The expiry bound saturates on overflow. The reconciliation horizon adds the merge floor-distance allowance.

```json
{
  "artifact": {
    "path": "casper/src/rust/finality/deploy_lifecycle.rs",
    "commit": "3adb6942037fa14ef624c97791770db3cea4cf43",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "The expiry bound saturates on overflow. The reconciliation horizon adds the merge floor-distance allowance.",
  "adapter": "rust-regression",
  "status": "pending",
  "evidence": {
    "kind": "rust-binding-tests",
    "ref": "docs/cbc-evidence/deploy-reconciliation-pr-1180.md"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "recorded"
  },
  "verified_at": "2026-11-27T10:24:57Z",
  "verification_id": "b05c5d86-9cf4-4630-99cb-540d6b3b3f8f",
  "inputs": {
    "artifact": "casper/src/rust/finality/deploy_lifecycle.rs",
    "source_sha256": "97b8a6c0709b4570843d3e8c36ad5637f051e456f6724ef3b56f3b0becb39d6b",
    "claim": {
      "path": "docs/claims/deploy-reconciliation.md",
      "sha256": "aaade0deafda4545fce67f39e78ef49f6faea6aeeb4012de3035ebdce9b949d6"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/deploy-reconciliation-pr-1180.md": "82412589ad822423bbcfea8cd3e5eb81c5712d830dd8bbb843a8118661634482"
  }
}
```
