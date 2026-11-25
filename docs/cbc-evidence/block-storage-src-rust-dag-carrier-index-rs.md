# CbC Evidence: block-storage/src/rust/dag/carrier_index.rs


## Claim CLAIM-DAG-INSERT-BATCH-001

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/carrier_index.rs",
    "commit": "77f16f7459b852cd1763a3d4dddddf514e6bc5f0",
    "id": "block-storage-src-rust-dag-carrier-index-rs"
  },
  "claim": "docs/claims/dag-insert-batched-writes.md",
  "adapter": "embedded",
  "status": "pending",
  "scope": "binding-tier",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json",
    "counterexample": null,
    "detail": "Binding tier reached. Counting-store tests prove P1, and equivalence tests against test copies of the pre-change writers cover the tested instances of P2. The claim stays pending until the construction tier proves P2 for every block sequence."
  },
  "waiver": null,
  "verified_at": "2026-11-25T15:15:40Z",
  "claim_id": "CLAIM-DAG-INSERT-BATCH-001",
  "schema": "cbc-claim/v1",
  "verification_id": "7c6c4a17-125d-46da-9b8e-f3b825d5ec8a",
  "inputs": {
    "artifact": "block-storage/src/rust/dag/carrier_index.rs",
    "source_sha256": "d4e5194f73b4a861aab3bef6c9cdbf3ea9e7f7afc53a6c25330989a51c1b4ce9",
    "claim": {
      "path": "docs/claims/dag-insert-batched-writes.md",
      "sha256": "3561abaa0464cd19d172d1306644448edc65831f045750c5a2386f558e4e605b"
    }
  },
  "evidence_files": {
    "docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json": "5323bb409bfa3f4f68768546c287cbc5dcf92e59090aa7c555738163d2908a77"
  }
}
```
