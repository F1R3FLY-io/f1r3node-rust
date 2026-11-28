# CbC Evidence: casper/src/rust/api/block_api.rs

- **Status:** pending
- **Adapter:** rust-regression
- **Commit:** 3adb6942037fa14ef624c97791770db3cea4cf43
- **Verified:** 2026-11-27T10:24:57Z

Claim:

> The reconciliation API checks the query identity and uses the adopted floor. Signature-only requests use the register.

```json
{
  "artifact": {
    "path": "casper/src/rust/api/block_api.rs",
    "commit": "3adb6942037fa14ef624c97791770db3cea4cf43",
    "commit_is_base": true,
    "working_tree": true
  },
  "claim": "The reconciliation API checks the query identity and uses the adopted floor. Signature-only requests use the register.",
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
  "verification_id": "5076a429-731e-425e-8c3f-b2b206567e55",
  "inputs": {
    "artifact": "casper/src/rust/api/block_api.rs",
    "source_sha256": "dde734d2b780f40d57cc555384b07b3bed5bbfa65297543b99034f6cfb95583b",
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
