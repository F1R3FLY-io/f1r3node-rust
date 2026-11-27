# CbC Evidence: casper/src/rust/engine/multi_parent_casper/finalization_runner.rs

- **Status:** pending
- **Adapter:** rust-regression
- **Commit:** 3adb6942037fa14ef624c97791770db3cea4cf43
- **Verified:** 2026-11-27T10:24:57Z

Claim:

> The finalizer preserves carrier index coverage through the reconciliation cutoff on an LFB advance.

```json
{
  "artifact": {
    "path": "casper/src/rust/engine/multi_parent_casper/finalization_runner.rs",
    "commit": "3adb6942037fa14ef624c97791770db3cea4cf43",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "The finalizer preserves carrier index coverage through the reconciliation cutoff on an LFB advance.",
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
  "verification_id": "a0c03004-a551-4ede-99c9-2c386e1618f7",
  "inputs": {
    "artifact": "casper/src/rust/engine/multi_parent_casper/finalization_runner.rs",
    "source_sha256": "71b968dd2df61c10673025cc26719ee4c3fe016ebe523ae2cc3829f682db3575",
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
