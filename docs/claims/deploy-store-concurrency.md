# Claim: deploy store operations preserve records without an outer pool mutex

```yaml
claim_id: CLAIM-DEPLOY-STORE-CONCURRENCY-001
artifacts:
  - shared/src/rust/store/key_value_store.rs
  - shared/src/rust/store/lmdb_key_value_store.rs
  - rspace++/src/rspace/shared/in_mem_key_value_store.rs
  - block-storage/src/rust/deploy/key_value_deploy_storage.rs
  - block-storage/src/rust/deploy/deploy_record_quarantine.rs
  - casper/src/rust/engine/multi_parent_casper/block_admission.rs
  - casper/src/rust/casper.rs
status: pending
adapter: agentic
mechanization: none
task: TASK-012-33
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/712
  - https://github.com/F1R3FLY-io/f1r3node-rust/pull/693
```

## Statements

**S1. Conditional delete.** A conditional delete removes a key only when its current bytes equal the expected bytes.
The comparison and delete form one atomic backend operation.
The result is true only when that operation removes a record.

**S2. Signature delete.** A signature delete performs one backend delete and returns whether that delete removed a record.
The operation does not read the record first.

**S3. Quarantine.** A scan preserves damaged bytes in quarantine before it deletes those bytes from the live store.
A concurrent replacement of the live bytes survives the conditional delete.
A conflict in quarantine preserves the live record.
An interrupted move can leave both copies, and a later scan can complete the move.
Restore removes only the quarantined bytes that it inspected after it inserts or finds an identical live copy.

**S4. Admission.** The shared rejected-buffer mutex still covers the buffer check and the atomic pool insert.
Concurrent insertion of one signature succeeds at most once while that signature remains in the pool.

## Boundaries

The storage codec, database names, and deploy signatures do not change.
The change adds no transaction across the pool, rejected buffer, and quarantine.
Lifecycle removals remain permitted during scans.
The conditional delete compares bytes, so replacement by identical bytes is equivalent for this claim.
Raw-store writes and operator recovery remain subject to the documented storage procedures.

## Discharge plan

1. Test S1 with matching, different, and absent bytes on memory and LMDB backends.
2. Test S2 with concurrent removers and a backend call counter.
3. Test S3 with deterministic replacement, concurrent scans, interrupted moves, errors, and reopen.
4. Test S4 with concurrent admission and rejected-buffer checks.
5. Record the tested source hashes and all results in the evidence record.

The claim remains pending maintainer review.
