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
  "verified_at": "2026-11-25T14:33:08Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "cf681444-20f3-4ef8-a107-1a82ee34fd2d",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "8f2538bb8a51370d49490823e7cc04e7a26b5b9f5c0d34262ca0fd42d137361d",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "fe6cda7a1ea060d65685fce04cd2e887b67fc9afb1a1dc910ec2f39e7af0a50f"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "323afb1e761b7e48aa5f4917578e16dcf5286354ffab77a0228937837d376f37"
  }
}
```
