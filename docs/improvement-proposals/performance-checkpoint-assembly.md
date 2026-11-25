# Reduce Checkpoint Assembly Work

Status: draft proposal. Snapshot and serialization changes require separate validation.
Date: 2026-11-25.

## Objective

Remove redundant deep clones while preparing a hard checkpoint.
Preserve consistent snapshots, recoverable failures, canonical serialization, and root publication.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Path

The [hot store](../../rspace++/src/rspace/hot_store.rs) already uses shared records and persistent maps for soft checkpoints.
This proposal does not replace that mechanism.
It concerns `changes()` when a hard checkpoint needs owned actions.

`changes()` holds `checkpoint_lock` while collecting continuations, data, and joins.
The `owned` helper clones payloads out of `Arc` values for insertion actions.
The final `[continuations, data, joins].concat()` clones the assembled actions again.
[Hot-store actions](../../rspace++/src/rspace/hot_store_action.rs) contain nested owned values, so those clones can copy complete terms.

These collections cover the current hot overlay, including records materialized during reads.
They are not a scan of every retained historical root.
The [hard-checkpoint caller](../../rspace++/src/rspace/rspace/ispace_impl.rs) passes the resulting actions to the history repository.

The [history repository](../../rspace++/src/rspace/history/history_repository_impl.rs) then transforms and serializes actions.
Its DEBUG-only `measure` path also serializes values to obtain diagnostic sizes.
That diagnostic cost is not present at every logging level.
The repository writes cold leaves before publishing their history references.

## Phase 1: Move Assembled Actions

Consume the three owned vectors when combining them.
Preserve the existing group order: continuations, data, then joins.
Preserve order within each collected group.
Do not sort, deduplicate, or change any action payload in this phase.

This removes the final action clone without changing the action type.
The earlier `owned` conversion still deep-clones shared payloads.
The [operation probe](performance-overview.md#reproducible-operation-probe) isolates only the final concatenation pattern.

Do not drain the authoritative hot-store maps to obtain owned payloads.
The node can still need those maps for execution, rollback, or a failed checkpoint.
Keep existing locking and snapshot behavior for this initial change.

## Phase 2: Shared Checkpoint Inputs

Introduce a private checkpoint input only if phase-1 measurements justify further work.
Capture stable shared payload references and owned channel metadata under the existing snapshot boundary.
Release the checkpoint lock before serialization or I/O.
Keep the captured references alive until serialization and publication finish or fail.

Adapt the serializer to traverse the referenced values without producing a second complete owned AST.
Serialize the same fields in the same order as the existing action path.
Do not serialize reference counts, pointer identity, or a new wrapper representation.
Use differential byte tests against the old serializer, not only decoded-value equality.

The captured view must remain consistent while live maps change.
Failure must leave the previous checkpoint and its hot state usable.
Do not consume rollback snapshots or installed continuations to reduce allocations.
Keep this representation private to checkpoint preparation rather than changing all runtime action APIs.

## Diagnostic Encoding

Where practical, compute DEBUG size metrics from already prepared canonical buffers.
Separate leaf bytes from the outer `PersistedData` envelope when reporting sizes.
Do not equate those byte counts because their hash inputs differ.

Preserve useful diagnostic fields and their logging-level gate.
If restructuring would complicate publication or retain buffers longer, leave the diagnostic path unchanged.
Measure DEBUG and normal logging separately.

## Publication and Failure Conditions

- Keep cold-leaf writes before history references become visible.
- Keep the existing history transaction, root pin, and pruning guard lifetimes.
- Keep parallel transformation thresholds and action semantics unchanged in phase 1.
- Keep delete actions and empty collections distinct from missing history.
- Preserve continuation patterns, guards, random state, joins, and duplicate records.
- Release temporary references after success, failure, or cancellation.

These are acceptance conditions, not a claim that a new shared action type already satisfies them.
Apply the existing verification requirements to any changed checkpoint ownership protocol.

## Regression Plan

| Case | Required assertion |
| --- | --- |
| Empty checkpoint and mixed insert/delete actions | Same ordered actions and canonical bytes |
| Repeated equal data and continuations | Multiplicity remains unchanged |
| Persistent receives, peeks, guards, and multi-channel joins | Same replay behavior, event log, and post-state root |
| Snapshot followed by live mutation | Captured checkpoint remains a consistent earlier view |
| Leaf write, history publication, and cancellation failures | Previous state remains usable and temporary ownership is released |
| Root pruning and restart | Published roots remain readable under the existing retention rules |
| Large continuation bodies | Removed clone is observable without moving work into serialization |
| DEBUG enabled and disabled | Same state and equivalent diagnostic size meanings |

Retain the [soft-checkpoint scaling regression](../../rspace++/tests/concurrent_rspace_test.rs).
A hard-checkpoint optimization must not restore deep copies in soft checkpoints.

## Benchmark and Delivery

Vary changed-channel count, materialized read count, records per channel, and continuation body size.
Measure snapshot capture, action assembly, canonical encoding, leaf writes, and root publication separately.
Include the work before the history repository's existing checkpoint timers.
Otherwise the removed hot-store cost can disappear from the comparison.

Report allocation count, copied bytes, peak heap, checkpoint-lock duration, CPU time, and latency percentiles.
Test ordinary and DEBUG logging with the same fixture.
Compare phase 1 before attempting shared checkpoint inputs.

Reject phase 2 if extended reference lifetimes raise peak heap enough to cancel the measured benefit.
Do not count the same bytes again as savings from [storage helper moves](performance-storage-buffer-copies.md).
Canonical bytes and disk formats stay unchanged, so rollback needs no migration.
