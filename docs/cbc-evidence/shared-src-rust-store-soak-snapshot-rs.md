# CbC Evidence: shared/src/rust/store/soak_snapshot.rs

The capture claim remains pending. Local tests do not discharge the claim or qualify a live adapter.

```json
{
  "artifact": {
    "path": "shared/src/rust/store/soak_snapshot.rs",
    "id": "shared-src-rust-store-soak-snapshot-rs",
    "commit": "2ccc4ae0ac1045232c247ecd76925e13fabd0ade",
    "commit_is_base": true,
    "working_tree": true,
    "sha256": "9c68785eb2dbf919225810428a110d0b2e1b687bf08b2386f0fc0fcf1090e99c"
  },
  "claim": "docs/claims/casper-node-authority-snapshot.md",
  "claim_ids": [
    "CLAIM-CASPER-NODE-OBSERVATION-002"
  ],
  "claim_digests": {
    "docs/claims/casper-node-authority-snapshot.md": "0a903c524a3e3e3b598fddd6ec149e578fb18f396199cd0ab5741faa87df6121"
  },
  "adapter": null,
  "status": "pending",
  "scope": "batch-b1-bounded-detached-capture",
  "evidence": {
    "kind": "local-tests-not-discharge",
    "ref": "docs/cbc-evidence/runs/casper-node-snapshot-hardening-2ccc4ae0a-01/report.json",
    "sha256": "e0d607f7fb2f8a2f4eea29b9cd2f4ffdb2de4949aa99432439c3c0fe94d01a04"
  },
  "previous_record": {
    "commit": "2ccc4ae0ac1045232c247ecd76925e13fabd0ade",
    "path": "docs/cbc-evidence/shared-src-rust-store-soak-snapshot-rs.md",
    "sha256": "cd280518fce33c6397c2981a69c909d23cd8c2ef2785e8dcb39cdf034f81d972"
  },
  "tiers": {
    "refutation": "pending",
    "construction": "pending",
    "binding": "pending"
  },
  "soak": "pending",
  "waiver": null,
  "verified_at": null
}
```
