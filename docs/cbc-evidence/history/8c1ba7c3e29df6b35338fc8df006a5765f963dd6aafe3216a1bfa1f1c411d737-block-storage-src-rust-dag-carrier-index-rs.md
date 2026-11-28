# CbC Evidence: block-storage/src/rust/dag/carrier_index.rs


## Claim CLAIM-DAG-INSERT-BATCH-001

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/carrier_index.rs",
    "commit": "1caf7b8cfdf7e8d447b348af1c90ae3615f19010",
    "id": "block-storage-src-rust-dag-carrier-index-rs"
  },
  "claim": "docs/claims/dag-insert-batched-writes.md",
  "adapter": "embedded",
  "status": "discharged",
  "scope": "binding-tier",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json",
    "counterexample": null,
    "detail": "Binding tier. Counting-store tests show one table write for each table in a block and no write for a redelivered block. Equivalence tests compare the batch path with the per-deploy path on the tested instances. The construction tier is pending."
  },
  "waiver": null,
  "verified_at": "2026-11-25T14:29:15Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "93f90546-2141-4c44-adf6-d3af523fb1bd",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "8f2538bb8a51370d49490823e7cc04e7a26b5b9f5c0d34262ca0fd42d137361d",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "fb0674b9156b9347c48879b40b427dbc4ed27cfc553711b9eefdb2e3f75b2a59"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "f8df2aaf095f289225f9e2d45950fd5884e50763db3af8a9596a3635fffa13be"
  }
}
```
