# CbC evidence for the finalization snapshot lock

A recovery scan takes its DAG snapshot after an in-flight finalization finishes writing. The [regression test](../../block-storage/tests/block_dag_storage_test.rs) pauses finalization inside its effect, before finality is visible. It polls the recovery snapshot and observes that it is pending. After the effect resumes, the snapshot completes and contains the finalized hash.

The Linux test passed on the pinned Rust toolchain. This run covers one controlled interleaving. Maintainer acceptance for the current source is pending. The previous record preserves the Batch E acceptance for its source digest.

```json
{
  "artifact": {
    "path": "block-storage/src/rust/dag/block_dag_key_value_storage.rs",
    "id": "block-storage-src-rust-dag-block-dag-key-value-storage-rs",
    "commit": "3473c4af8e39b60a0c5d0a5772e325f9e5154648",
    "commit_is_base": true,
    "working_tree": false,
    "sha256": "ded2bac64e8e6b32734e8a921249bee775275d44e2f675a79f3bbc363da99a8f"
  },
  "claim": "A recovery snapshot waits for an in-flight finalization to persist the finalized block.",
  "status": "pending",
  "scope": "observer-report-recovery-snapshot-ordering",
  "evidence": {
    "kind": "bounded-regression-tests",
    "ref": "block-storage/tests/block_dag_storage_test.rs",
    "sha256": "f5dc87fe0ee71fd7c41544c058f104bbc868c576e899325e6ce2aa16da0580ef",
    "test": "recovery_snapshot_waits_for_in_flight_finalization",
    "command": "bash scripts/ci/run-tests.sh cargo test --offline -p block-storage --test block_dag_storage_test recovery_snapshot_waits_for_in_flight_finalization -- --exact",
    "result": "1 passed, 0 failed",
    "run_date": "2026-11-25"
  },
  "previous_record": {
    "path": "docs/cbc-evidence/block-storage-src-rust-dag-block-dag-key-value-storage-rs.md",
    "commit": "3473c4af8e39b60a0c5d0a5772e325f9e5154648",
    "sha256": "66249e5087ae81aa66fd33ddcd02701e301d536a2148199192da00b58c72128b",
    "status": "discharged",
    "source_sha256": "8d74386319993358bccd1a2674f970c0c49c2187614fd60f19733f1619638ab2"
  },
  "waiver": null
}
```
