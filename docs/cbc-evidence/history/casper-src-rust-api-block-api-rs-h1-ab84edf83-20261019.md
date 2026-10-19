# CbC Evidence: casper/src/rust/api/block_api.rs

**Status:** pending.

H1 converts the two legacy callback booleans into equivalent `ProposeRequest` values. No API response or display-projection rule changes in that conversion.

The historical Batch E record remains byte-identical at `docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md`.
Its acceptance applies only to its recorded source and claim digests. Those digests differ from the current files, so acceptance is not transferred here.

This record covers the H1 bridge only. Renewal of the display-projection claim remains separate and pending.

```json
{
  "artifact": {
    "path": "casper/src/rust/api/block_api.rs",
    "id": "casper-src-rust-api-block-api-rs",
    "commit": "ab84edf83",
    "working_tree": false,
    "sha256": "9bf8f175f132f89b373a10af6eba70a9325a6ec9c4fc15cb8ab72a0a71d080f2"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b"
  },
  "status": "pending",
  "adapter": null,
  "scope": "h1-legacy-request-conversion-only",
  "evidence": {
    "kind": "source-review-pending-acceptance",
    "ref": "docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
    "sha256": "2ec8a9362307056af54418fccc09cd0f198362e61fa51912c0790209a7652848"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "legacy-boolean-conversion-review",
    "binding": "pending"
  },
  "related_claims": {
    "docs/claims/casper-node-display-projection.md": {
      "status": "renewal-pending",
      "sha256": "1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591"
    }
  },
  "previous_ledger": {
    "path": "docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md",
    "sha256": "8d550febbc1e4ad062be64c97af94631c74e281a6758a1dd3fa824c591d606be",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f"
  },
  "waiver": null,
  "verified_at": null
}
```
