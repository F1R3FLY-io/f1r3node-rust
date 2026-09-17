# Raw byte observations

The interpreter preserves raw byte quantities for accepted accounting events.
These quantities support typed funding evidence without reconstructing measurements from weighted charges.
The observations do not change the legacy byte schedule, create spendable balances, or authorize wallet settlement.

## Measurement and legacy charge

Each observation records an event identity, event kind, canonical authority, optional measurement, and optional legacy amount.
The measurement contains introduction bytes, transferred bytes, and trace bytes.
The existing V1 schedule maps these quantities to the legacy byte amount.
The observer retains the original overflow check and error representation.

| Accepted event | Raw measurement | Legacy byte event |
| --- | --- | --- |
| Measured introduction with positive cost | Present | Present |
| Measured COMM with billable authority and zero byte cost | Present | Absent |
| Measured COMM with billable authority and positive byte cost | Present | Present |
| Measured COMM with unit authority | Present | Absent |
| Cost-only legacy API call | Absent | Present only when the existing rule charges bytes |

COMM means communication between a matching send and receive.
A unit authority has no resource demand in the existing authority algebra.
Its raw quantities must not disappear merely because its legacy projection has no byte event.
Conversely, a raw observation must not introduce a legacy charge where the existing rule charges none.
The existing reservation validator rejects zero-weight introductions. Rejected introductions produce no accepted receipt.

## Publication and retries

`RuntimeBudget` stores each raw measurement and its legacy projection in one immutable observation.
The existing authority mutex protects receipt publication and accepted authority-event updates.
Introduction reservations use that mutex for their reservation and publication.
Persistent introductions also retain the existing introduction lock, acquired before the authority lock.
The implementation adds no global funding lock.

Persistent introductions and COMM retain their existing identity-based deduplication.
A measured retry must match the original authority, raw dimensions, and legacy projection.
Equal weighted costs do not establish equal measurements.
A measured retry cannot supply missing evidence for an earlier cost-only call.

Nonpersistent introductions remain separate occurrences, including repeated identities.
Rejected reservations add no accepted observation.
Reservation attempts can still affect the existing diagnostic and reconciliation machinery.
The receipt proof does not equate a rejected attempt with an unchanged diagnostic trace.

Persistent deduplication stores shared immutable observations, not indices into a mutable vector.
This prevents stale indices after allocation replacement.
Allocation replacement preserves the existing deduplication behavior and marks discarded receipt history as incomplete.
Additional observations cannot repair that lost history.

## Evaluation results

The interpreter captures one receipt snapshot after reduction completes.
It derives legacy byte events and their total from that same snapshot.
Snapshot rows preserve occurrences. Their append order is not a consensus order.
The legacy projection retains its existing canonical sort order.

An owned snapshot remains unchanged after another evaluation resets the runtime budget.
Full reset clears observations and identity history under the existing quiescent evaluation contract.
Quiescence means that the previous evaluation has no active producers.
These changes do not add a reset barrier or prove the complete runtime lifecycle.

`has_complete_measurements()` requires a metered context, intact history, and a measurement on every recorded occurrence.
A direct legacy COMM reservation marks the history incomplete because it supplies no raw measurement.
An empty metered execution can have complete measurements.
An early parse failure uses the default snapshot, which does not claim a metered context.

System operations outside the accounting scope and trusted unmetered operations remain excluded.
The runtime lifecycle must keep those scope boundaries consistent with evaluation ownership.
A receipt completeness check does not authenticate publicly supplied snapshot values or prove that every possible runtime caller uses the observer.

## Verification boundaries

[`PairedByteReceipts.v`](../../../../formal/rocq/cost_accounted_rho/theories/PairedByteReceipts.v) models paired publication, exact retry measurements, occurrence multiplicity, rejection, snapshots, and reset.
Its two-order theorem compares accepted concurrent operations when both orders succeed.
It does not assert that resource exhaustion accepts the same arrival order.
Its optional legacy projection includes raw-only events.

Rust regression tests compare measured and legacy traces, charges, and authority events.
Property tests vary event kinds, quantities, persistence, and execution order.
Concurrent tests inspect paired snapshots during publication.
Loom tests explore the production receipt-log component under a modeled mutex.
They do not explore the complete `RuntimeBudget` lock graph or distributed validators.
Runtime tests exercise actual RSpace observations, user abort, parse failure, and evaluation reuse.

Native settlement still needs counted resource quantities and authenticated acquisition provenance.
It must derive the complete execution witness before selecting a signed funding outcome.
The [observed outcome contract](observed-funding-outcome.md) defines those remaining integration boundaries.

Implementation and tests:

- [Receipt representation](../../../../rholang/src/rust/interpreter/accounting/byte_receipts.rs)
- [Receipt regressions and properties](../../../../rholang/src/rust/interpreter/accounting/byte_receipts/tests.rs)
- [Runtime receipt tests](../../../../rholang/src/rust/interpreter/reduce_byte_receipts_tests.rs)
- [RSpace observers](../../../../rholang/src/rust/interpreter/rho_runtime.rs)
