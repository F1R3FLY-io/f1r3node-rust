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

The baseline and both negative controls are registered in `scripts/ci/check-tla-invariants.sh`.

Each negative control must exit with TLC code 12 and report its expected invariant violation. A clean control, another invariant violation, or a tool failure fails the gate.

`scripts/ci/test-check-tla-invariants.sh` tests result classification through the real gate with a verifier-process fixture. That fixture is not formal proof evidence.

Pull requests and pushes run the carrier baseline, both carrier negative controls, and the replay baseline through `check-tla-invariants.sh --soak-pr`.

This tier uses two workers and a fixed two-minute limit per configuration. The workflow allows 15 minutes for the complete job.

Scheduled and manual runs retain the existing full configuration list. Hosted confirmation of the new pull-request route remains pending.
