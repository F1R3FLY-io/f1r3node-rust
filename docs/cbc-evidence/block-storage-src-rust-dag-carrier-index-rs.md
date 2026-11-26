# CbC Evidence: block-storage/src/rust/dag/carrier_index.rs


## Claim CLAIM-DAG-INSERT-BATCH-001

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/carrier_index.rs",
    "commit": "534e78f2e66e3ca512f12361d3305f374d9cacfc",
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
    "detail": "Binding tier. Counting-store tests show one table write for each table in a block and no write for a redelivered block. Equivalence tests compare the batch writers with test copies of the pre-change writers, and two tests check the stored rows directly. The construction tier is pending."
  },
  "waiver": null,
  "verified_at": "2026-11-25T14:46:00Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "f4a68000-064b-4bee-9b56-9b4365712d39",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "d4e5194f73b4a861aab3bef6c9cdbf3ea9e7f7afc53a6c25330989a51c1b4ce9",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "1418e4bb4c83b6b0fa9bc2ba69ba2d25156cebe396282e11c319de75590b04be"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "015178bde8b55b9e0a69d04c4101a88aeb5b44b8048908032faec50000de84e7"
  }
}
```
