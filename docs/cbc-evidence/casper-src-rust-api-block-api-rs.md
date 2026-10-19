# CbC Evidence: casper/src/rust/api/block_api.rs

**Status:** pending.

H1 converts the two legacy callback booleans into equivalent `ProposeRequest` values. No API response or display-projection rule changes in that conversion.

The historical Batch E record remains byte-identical at `docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md`.
Its acceptance applies only to its recorded source and claim digests. Those digests differ from the current files, so acceptance is not transferred here.

This record includes the H1 bridge and the API-scoped display-projection renewal. Current-source acceptance and full claim discharge remain pending.

```json
{
  "artifact": {
    "path": "casper/src/rust/api/block_api.rs",
    "id": "casper-src-rust-api-block-api-rs",
    "commit": "f41f2a8ab",
    "working_tree": false,
    "sha256": "9bf8f175f132f89b373a10af6eba70a9325a6ec9c4fc15cb8ab72a0a71d080f2"
  },
  "claim": "docs/claims/heartbeat-proposal-amplification-bound.md",
  "claim_digests": {
    "docs/claims/heartbeat-proposal-amplification-bound.md": "0f62182a84d4f83eae00e6dab4647d1299ffddedb0e98d5840b42df7e57db44b",
    "docs/claims/casper-node-display-projection.md": "1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591"
  },
  "status": "pending",
  "adapter": null,
  "scope": "h1-legacy-request-bridge-and-api-display-projection-renewal",
  "evidence": {
    "kind": "source-review-pending-acceptance",
    "ref": "docs/casper/validation/heartbeat-h1-verification-2026-10-19.md",
    "sha256": "1dddefc95b540d4a5873d212d891e385f14137c17e2e2eba229d68427b423304"
  },
  "tiers": {
    "refutation": "partial-executable-checks-passed",
    "construction": "legacy-boolean-conversion-review",
    "binding": "pending"
  },
  "related_claims": {
    "docs/claims/casper-node-display-projection.md": {
      "status": "api-scope-renewal-recorded-acceptance-pending",
      "sha256": "1729a5758c702a3c4c006295f5e07ad609f11b9c7d91c875f57199a4097df591",
      "evidence": {
        "ref": "docs/casper/validation/api-display-projection-renewal-2026-10-19.md",
        "sha256": "acfd02d5a6b477d453767a9b6b7871eb3c163b660f41f4c89b8ea613a2d46701"
      },
      "limits": [
        "linux-admission-not-run",
        "rocq-unavailable",
        "no-rust-refinement-proof",
        "historical-acceptance-not-transferred"
      ]
    }
  },
  "previous_ledger": {
    "path": "docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-batch-e-20261001.md",
    "sha256": "8d550febbc1e4ad062be64c97af94631c74e281a6758a1dd3fa824c591d606be",
    "commit": "1a9839b0e52e494e20ab13c0a79a55bd2164e34f"
  },
  "previous_review_record": {
    "path": "docs/cbc-evidence/history/casper-src-rust-api-block-api-rs-renewal-base-f41f2a8ab-20261019.md",
    "sha256": "796b616070da1ce041af839383313cb2612b053ae539fe78769f17af7cdc14aa",
    "report": {
      "path": "docs/casper/cbc-evidence/history/heartbeat-h1-verification-renewal-base-f41f2a8ab-20261019.md",
      "sha256": "97c86f2906fb9588e4ca6afea70a34f92a02ab3eb4925c10925fa2c3b3908d43"
    }
  },
  "verification_sources": {
    "node/src/rust/instances/heartbeat_responsive_tests.rs": "edcc7468032f7edb6f605f2b6028c673f052d033224e0f61b7e2a6d24066835c",
    "node/src/rust/instances/heartbeat_additional_tests.rs": "fd73cd87c979683de84c3fdf57921e3a16ed6810d26688cc690eb6a43cf2c1e3",
    "node/src/rust/instances/proposer_instance.rs": "c76ea6698d479db9617bba7cad5f7868a74f217d7582b28ae7ec8c6309aa9431",
    "casper/tests/add_block/h1_proposer_spec.rs": "b9f54549857fa29b0f008210c7e9b685a4304fd88a9eea1a79ec0b00d702b1c9"
  },
  "waiver": null,
  "verified_at": null
}
```
