# Claim: one synced write for each table in a DAG insert

```yaml
claim_id: CLAIM-DAG-INSERT-BATCH-001
artifacts:
  - block-storage/src/rust/dag/carrier_index.rs                 # CarrierIndex::record_block
  - block-storage/src/rust/dag/block_dag_key_value_storage.rs   # insert_internal and record_deploy_events call the batch writers
status: pending
adapter: embedded
mechanization: none   # binding tier only. The construction tier is pending.
task: issue #1151
references:
  - https://github.com/F1R3FLY-io/f1r3node-rust/issues/1151   # DAG insert holds the global lock for 2 to 4 s
  - docs/designs/adaptive-execution.md                        # section 10.4.1, Track A item from issue #1151
  - docs/claims/repeat-deploy-carrier-index-equivalence.md    # CLAIM-FINALITY-002, unchanged by this claim
```

## Context

`BlockDagKeyValueStorage::insert` holds the DAG global write lock while it records the carriers and the lifecycle events of a block. Before this change, it wrote these rows with about three synced LMDB commits for each deploy. Under the sustained load of the soak, the commits held the lock for 2 to 4.4 seconds, and `getBlocks` and `/status` stalled on every validator (issue #1151).

`deploy_lifecycle_types.rs` also changes. It does not have the `cbc=mandatory` tag, but property P2 covers its batch writer.

## Definitions

- A **table write** is one call of `KeyValueStore::put` on one table. On an LMDB store, one table write is one synced commit.
- The **per-deploy path** calls `CarrierIndex::record_once` once for each deploy, in block order. It calls `DeployLifecycleTables::append_event_once` once for each included deploy and each rejected record.
- The **batch path** is one call of `CarrierIndex::record_block` and one call of `DeployLifecycleTables::append_events_once_batch` for the block.

## Claim statements

**P1. Bounded writes.** One DAG insert makes at most one table write to each of these tables: the carrier table, the seed-carrier table, and the lifecycle events table. The count does not depend on the number of deploys in the block. A redelivered block that is already recorded makes no table write.

**P2. Equivalence.** For each sequence of blocks, the batch path and the per-deploy path leave the same state in the three tables of P1. This includes deploys that share a name seed, a sig with a terminal record, a rejected record for an included sig, and a redelivered block.

**P3. Completeness order.** `insert_internal` records the carriers of a block before the block becomes visible in the DAG. This change keeps that order, so the completeness contract of `CLAIM-FINALITY-002` does not change.

## Seam premises (documented, not proven)

- `KeyValueTypedStore::put` with several pairs writes all of them in one LMDB write transaction (`LmdbKeyValueStore::put`).
- Reads in `record_block` and `append_events_once_batch` see the committed rows. The caller holds the write lock of the carrier index or of the lifecycle tables for the whole call. No other writer of those tables can change the rows during the call. The insert path also holds the DAG global write lock, and `validation_dispatcher.rs` holds only the lifecycle lock.
- The seed-carrier write and the carrier write of `record_block` are two commits. A failure between them leaves seed rows without their carrier rows for one block. The block is not visible in the DAG until the insert completes. A redelivery writes the missing rows, because both writes are idempotent per (key, block_hash). The per-deploy path had the same gap for one deploy.

## Tiers

| Property | Tier reached | Evidence |
|---|---|---|
| P1 | Binding | `a_block_records_its_carriers_in_one_write_for_each_table`, `a_redelivered_block_writes_nothing`, and `a_block_appends_its_events_in_one_write` count the table writes with a counting store. A local subprocess `test_load` run measured 3 commits for each insert, against about 25 before the change. |
| P2 | Binding, for the tested instances | `a_block_record_matches_the_pre_change_per_deploy_writer` and `a_block_append_matches_the_pre_change_per_event_writer` compare the batch writers with test copies of the pre-change writers, which use only `put_one` and `append_events`. `a_block_record_stores_one_entry_for_each_carrier` and `a_block_append_stores_each_event_once_and_skips_terminal_sigs` check the stored rows directly. The cases include shared seeds, a repeated deploy, a terminal sig, a rejected record, a repeated event of one sig, and a redelivered block. The unbounded equivalence needs a construction-tier proof (`construction: pending`). |
| P3 | Not changed | The call order in `insert_internal`. The block-storage suite passes (273 tests). |

The new tests did not compile before the change, because the batch writers did not exist. The evidence for the change in behavior is the commit count of the local run in the run package.

## Discharge record

The run package `docs/cbc-evidence/runs/dag-insert-batch-20261125-01/report.json` holds the test results, the source digests, and the local load measurement.

`carrier_index.rs` and `block_dag_key_value_storage.rs` have named records for this claim, with the status `pending`. The binding tier is reached for P1 and for the tested instances of P2. The claim stays pending until the construction tier proves P2 for every block sequence. The ledger of `block_dag_key_value_storage.rs` moved from one legacy record to named claims on 2026-11-25. The run package `docs/cbc-evidence/runs/block-dag-storage-named-claims-20261125-01/report.json` gives that migration.
