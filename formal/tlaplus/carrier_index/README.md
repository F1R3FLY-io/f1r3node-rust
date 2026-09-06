# Carrier-Index Model

## Model-to-code map

| Model action | Rust surface |
| --- | --- |
| `RecordBlock` | `CarrierIndex::record_once` from `BlockDagKeyValueStorage::insert` |
| `PublishBlock` | DAG metadata publication in `BlockDagKeyValueStorage::insert` |
| `AdvanceWindow` | `KeyValueDagRepresentation::prune_carriers_below` |
| `FailRead` | Failure from `carrier_index_watermark` or `carrier_index_proves_absence` |
| `Crash` | Interruption between carrier recording and DAG publication |

## Configurations

| Configuration | Expected result | Purpose |
| --- | --- | --- |
| `MC_CarrierIndex.cfg` | Clean | Index-first publication, safe pruning, and read-failure fallback preserve absence soundness. |
| `MC_CarrierIndex_dag_first_pre_fix.cfg` | `IndexCompleteForWindow` violation | DAG-first publication permits a visible block without its carrier entries. |
| `MC_CarrierIndex_read_failure_pre_fix.cfg` | `AbsenceProofSound` violation | Treating a read failure as absence can accept a carried signature. |

The gating configuration is registered in `scripts/ci/check-tla-invariants.sh`.

The pre-fix configurations are manual negative controls. A clean negative control is a verification failure.
