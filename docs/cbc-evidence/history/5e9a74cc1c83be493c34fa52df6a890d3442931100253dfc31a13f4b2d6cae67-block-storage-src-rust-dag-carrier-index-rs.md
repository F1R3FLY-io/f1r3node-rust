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
  "verified_at": "2026-11-25T14:28:48Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "05fb1902-2933-474c-973b-2f8e82e8f73c",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "8f2538bb8a51370d49490823e7cc04e7a26b5b9f5c0d34262ca0fd42d137361d",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "54f205d3a562b364a70af32bf3285e5014f2a733114fb82f8b09e6e3ba04cdb5"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "d7abb77fc62e3816dbb7d89e41f04774e99c095bce5b359a98b4384f4be33f99"
  }
}
```
