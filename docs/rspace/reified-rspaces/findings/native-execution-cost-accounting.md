# Finding: native execution items for the cost-accounting owner

**Status.** Recorded on 2026-10-07 for the owner of native execution and cost accounting. The Reified RSpaces campaign does not change the accounting module or the host path of native replay. [Decision 47](../decision-register.md#decision-47) approves only the accessor `space_store`, and the consensus checklist keeps the rest byte-identical.

**Scope.** This document records five items that the work of decision 47 found in native execution: four findings (F-47-1 to F-47-4) and one measurement (M-47-1). Each item gives the mechanism, the evidence, and the reach. The [verification dossier](../verification.md#13-decision-47-spaces-in-native-execution) records the tests and the formal models of decision 47.

## Contents

1. [Summary](#1-summary)
2. [Terms](#2-terms)
3. [F-47-1: space errors have no typed class](#3-f-47-1-space-errors-have-no-typed-class)
4. [F-47-2: the session charge depends on the thread count](#4-f-47-2-the-session-charge-depends-on-the-thread-count)
5. [F-47-3: two readers for host continuations](#5-f-47-3-two-readers-for-host-continuations)
6. [F-47-4: a failed host operation leaves its waiters waiting](#6-f-47-4-a-failed-host-operation-leaves-its-waiters-waiting)
7. [M-47-1: verification bytes of native replay](#7-m-47-1-verification-bytes-of-native-replay)
8. [References](#8-references)

## 1. Summary

| Item | Severity | Reach |
|---|---|---|
| F-47-1 | Economic | A native deploy that fails with a space error is never charged, because a space error has no typed economic class. |
| F-47-2 | Determinism | The construction charge of a native replay session depends on the thread count of the validator. |
| F-47-3 | Determinism, not checked for reachability | Native play and native replay read host continuations and joins with two readers that differ in their depth limit. |
| F-47-4 | Liveness | A forged journal can stall native replay: an operation waits forever for a host row that failed closed. |
| M-47-1 | Capacity | Native replay reserves about 1.2 MB of `VerificationBytes` for each budget attempt. The v6 limit then bounds a certified deploy near 190 attempts. |

Decision 47 fixed the space instance of F-47-4 in its own backend (commit `c6cb371ba`). The other items belong to the owner.

## 2. Terms

- **Native play** runs a deploy with a native phlo budget and records a [journal](../glossary.md#journal-row) of its operations.
- **Native replay** runs the deploy again against that journal, as proposer self-certification and validator replay do.
- A **journal row** records one operation: its source, its locked footprint, its predecessors and its outcome.
- A **ticket** is the reservation of one journal row while its operation runs. Publishing the ticket completes the row.
- The **epoch** of a native replay session holds the state of its rows. Closing the epoch (`invalidate`) wakes every waiting operation, and each then fails with `Closed`.

## 3. F-47-1: space errors have no typed class

A space error that is not a host error becomes `InterpreterError::ReduceError("reified:…")` (`rholang/src/rust/interpreter/spaces/errors.rs:236-243`).
The economic failure policy classifies a `ReduceError` as Unclassified, and an Unclassified failure prevents the economic publication (`docs/casper/theory/cost-accounting-impl/economic-failure-observation.md:29`).
The production check of an offered witness rejects every failure that is not a user failure.
So a native deploy that fails with a space error, for example `CellOccupied` or `DanglingHandle`, is never charged.

Decision 47 keeps space errors Unclassified, as every host `ReduceError` is (point G9 of its arbitration).
A typed class for space errors is the "more precise typed contract" of the Unclassified row, and the owner decides it.

## 4. F-47-2: the session charge depends on the thread count

The native replay session reserves the cost of its constructor from the layout of its hot store (`rspace++/src/rspace/replay_rspace/native_session.rs:149-182`).
That layout uses `native_cache_shards` (`rspace++/src/rspace/hot_store.rs:1538-1543`), which multiplies `std::thread::available_parallelism` by four.
So two validators with different thread counts reserve different amounts.
A budget near its limit can then accept the construction on one validator and reject it on another.
The host-work rule forbids this: allocator layout and validator-local values must never control the verdict (`docs/casper/theory/host-work-budget.md:178-185`).

## 5. F-47-3: two readers for host continuations

For a host operation, native play reads continuations and joins through the plain hot-store reader.
Native replay reads them through the native history reader, which stops a record decode at nesting depth 128 (`rspace++/src/rspace/history/native_reader/typed.rs:15`).
A continuation nested deeper than that limit could therefore load in play and fail in replay.
This item was not checked for reachability.
Decision 47 removes the difference for space transactions: both backends load a cold view through one native-reader helper (`history_records`, `rspace++/src/rspace/space_store_handle.rs:1117`).

## 6. F-47-4: a failed host operation leaves its waiters waiting

**Mechanism.** The journal check derives the predecessors of each row from the footprints: the last earlier row that touches each channel.
So a forgery that adds a channel to the footprint of a row, and derives the predecessors again, passes the check.
A later operation that touches the added channel then waits for the forged row.

The forged row fails closed when its operation authenticates the footprint.
The failed operation only drops its ticket, which returns the row to the available state and does not close the epoch.
The waiting operation checks again, finds the row incomplete, and waits forever.

**Measurement.** A probe of 2026-10-07 forged the journal of a queue deploy of 37 rows (`target/verification/reified-rspaces/d47/step4/hang-probe-host-sweep.log`).
For 111 forgeries of a host row on a later host row, the journal check accepted every forgery.
66 replays ended and were rejected, and 45 replays hung.
The same probe found 64 hangs among 109 forgeries of a space row (`hang-probe-sweep.log`).

**Reach.** A proposer that publishes such offered evidence can stall the native replay of every validator. Proposer self-certification is affected in the same way.

**Fix for spaces.** Commit `c6cb371ba` closes the epoch when a native space operation fails, except after a published denial.
The test `space_native_forged_footprints_never_hang` replays every forgery of a space row on a later host row and requires each replay to end and to be rejected.
With the closure disabled, the test fails, and the TLA+ control `NoInvalidateUnsafe` of `NativeReplay.tla` refutes `NoDeadlock`.
The host path needs the same closure, and the owner decides it. The tracker holds this finding as a bug item of the project.

## 7. M-47-1: verification bytes of native replay

The probe measured the `VerificationBytes` dimension of the host-work budget of native replay.
The environment of one certification reserves about 36 MB before its evaluation.
Each evaluation then reserves about 1.2 MB for each budget attempt, for host operations and space operations alike:

| Deploy | Budget attempts | `VerificationBytes` of the evaluation |
|---|---|---|
| A host program | 3 | 4.1 MB |
| A factory call | 8 | 7.1 MB |
| A queue program | 87 | 104 MB |

The v6 limit of the dimension is 268,435,456 bytes, so a natively certified deploy can make about 190 budget attempts.
A space operation makes more attempts than its host equivalent, because its counter bumps and re-stores are operations of their own.
This is a capacity bound and not a defect.
It is recorded so that the owner can confirm that the bound is intended.

## 8. References

- [Decision 47](../decision-register.md#decision-47) and its design v2, sections 4.5, 5 and 10.
- [The verification dossier](../verification.md#13-decision-47-spaces-in-native-execution), section 13.
- `docs/casper/theory/host-work-budget.md` and `docs/casper/theory/cost-accounting-impl/economic-failure-observation.md`.
