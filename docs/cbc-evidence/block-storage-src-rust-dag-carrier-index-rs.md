# CbC Evidence: block-storage/src/rust/dag/carrier_index.rs


## Claim CLAIM-DAG-INSERT-BATCH-001

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/carrier_index.rs",
    "commit": "cc76daf25c57814b4f490bf019f906e56265a0cb",
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
  "verified_at": "2026-11-25T14:59:37Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "275ea432-4487-4ef6-b65d-bd0f34d681a4",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "d4e5194f73b4a861aab3bef6c9cdbf3ea9e7f7afc53a6c25330989a51c1b4ce9",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "18eeaeb3242c4bea741549b62e49f683beb69bcfde8542ec94a80563f5e8eb70"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "1e99d08157ecbb5f9829adae03dff756aebdc5d4e7bb3aebf73163f0d836fa48"
  }
}
```
