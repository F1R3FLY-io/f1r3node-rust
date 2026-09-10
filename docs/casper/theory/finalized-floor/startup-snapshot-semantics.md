# Startup snapshot semantics

## Scope and status

This document defines snapshot membership for the bounded recovery driver.
It refines the [recovery control specification](recovery-pump-control.md).
The ordered snapshot implementation follows a source review, six Rocq proofs, and a finite TLA+ check.
Its property, concurrency, buffer, dependency-DAG, and strict lint checks passed.
The startup driver does not yet use this interface.
The completion ticket and its context protection remain separate integration requirements.

This repair changes local metadata representation and scan execution.
It does not change block encoding, durable buffer encoding, validation rules, voting, fork choice, or settlement.
The ordinary retry FIFO remains unchanged.

## Existing startup contract

The existing startup function is `send_buffer_pendants_to_casper` in `casper_launch.rs`.
It first captures dependency-free block hashes from `get_pendants`.
This capture excludes typed certificate dependency keys.
The snapshot contains hash identities, not candidate incarnations.

Startup then performs two phases:

1. Call `BlockStore::contains` for each captured hash.
2. Process the hashes whose presence checks succeeded.

`contains` calls `get`, including decompression and decoding.
A failed presence check prevents all startup admissions.
Replacing that operation with `contains_key` would change this error policy.
The second phase loads each selected block again, reconciles DAG membership, attempts admission, and acknowledges successful admission.
A second-phase error can occur after earlier admissions.

Buffer membership can change during either phase.
The snapshot must preserve the following cases:

| Change after capture | Startup membership |
|---|---|
| Remove a captured hash. | Retain that hash in the snapshot. |
| Remove and reinsert a captured hash. | Visit that hash once, not once per incarnation. |
| Add a dependency to a captured hash. | Retain snapshot membership. Apply current eligibility separately. |
| Create a new pendant. | Exclude it from this startup snapshot. Ordinary retries can examine it. |
| Resolve a certificate dependency. | Exclude newly exposed pendants from this startup snapshot. |
| Insert a body after its presence check returned absent. | Do not add that hash to the selected second-phase set. |

The requested quarantine repair can defer a captured member.
That eligibility decision does not redefine snapshot membership.
Documentation must distinguish the new eligibility rule from preservation of the old snapshot contract.

## Persistent ordered root

`BlockDependencyDag::dependency_free` now uses the existing `imbl::OrdSet` dependency.
Other dependency maps and the ordinary candidate FIFO keep their existing representations.
All mutations remain inside the existing buffer publication boundary.
No second membership index requires synchronization.

`snapshot_pendant_candidates` clones the shared root under the state read guard and DAG mutex.
It releases both guards before the caller visits keys or accesses storage.
The raw snapshot includes typed certificate keys.
The driver must count and exclude those keys without loading a block body.

`OrderedSnapshot` stores the root, an exclusive previous-key cursor, and an exhaustion flag.
Its first step reads the minimum key.
Each later step reads the first key greater than the previous key through a bounded range traversal.
Each successful step clones one returned key and one cursor key.
The implementation does not use a consuming iterator or numeric sequence counter.

The sorted order replaces an unspecified hash-set enumeration order.
It is one permissible ordering of the same snapshot members.
This change does not claim identical admission timing across executions.

### Dependency limits

The inspected dependency version is `imbl 7.0.1` from `Cargo.lock`.
Its ordered nodes contain at most 16 keys and 17 child pointers under the default configuration.
The `small-chunks` feature changes those limits to six keys and seven pointers.
Root cloning shares ownership without cloning all keys.
Range traversal retains references along tree paths.

Let $`N`$ be the snapshot size and $`h`$ the balanced tree height.
A cursor step takes $`O(h)`$ traversal work and temporary path state.
Persistent mutation copies $`O(h)`$ bounded nodes, including possible sibling rebalancing.
Neither operation requires copying the complete snapshot.

Two consuming iterators do not meet the selected bound:

- A hash-set consuming iterator can copy a complete collision vector during one step.
- An ordered-set consuming iterator builds a deque of leaf pointers before yielding entries.

The production cursor avoids both operations.
The dependency implementation remains part of the trusted implementation boundary.
The project proofs do not verify every internal `imbl` tree operation.

## Driver integration contract

Capture the source root once for the startup request.
Initialize a selected root by sharing the source root.
Remove certificate keys and absent hashes from the selected root during presence checks.
Do not start admission until all presence checks succeed.
Then release the source root outside storage locks and start a new cursor over the selected root.

Each raw key visit consumes one unit of the page budget.
Certificate filtering must not hide unbounded visits inside one page.
The driver must test phase exhaustion before checking queue capacity.
An empty snapshot can finish even when the queue is full.
Certificate-only entries can also retire without body capacity.

Before `contains` or `get`, the driver must check count capacity.
If capacity is unavailable, retain at most one pending hash and park the pass.
Do not load a body merely to discover that the count queue is full.
A temporary admission failure preserves retry eligibility.

The driver permits one active startup ticket and one pending replacement.
Each ticket binds its snapshot to private context and request identities.
Replacement or cancellation prevents stale completion and stale startup success.
Already admitted work cannot be revoked by canceling its startup ticket.

Let $`N_{live}`$, $`N_{active}`$, and $`N_{pending}`$ denote current, active-snapshot, and pending-snapshot membership sizes.
The target retained metadata bound is $`O(N_{live} + N_{active} + N_{pending})`$.
Repeated mutations must not retain intermediate roots.
This bound is not constant whole-buffer memory.

The controller's slot count alone does not establish that target under concurrent replacement.
Let $`N_{incoming}`$ and $`N_{retiring}`$ denote total membership sizes retained by incoming calls and unfinished retirement operations.
Without pre-capture admission, the bound also includes $`N_{incoming} + N_{retiring}`$.
The runtime must reserve ownership before capture and retain that reservation until destruction finishes.
Cancellation and engine publication must not make a retiring reservation available early.
This reservation covers a scan episode, including its source and selected roots, not just one persistent-tree pointer.
The [completion contract](startup-completion-identity.md) records the current integration gap and required repair.

Release canceled roots when the driver no longer uses them.
Release replaced pending roots outside registration locks.
Root destruction can take $`O(N_{active})`$ time.
Neither snapshot cancellation nor an abort request proves immediate constant-time destruction.

## Formal and executable evidence

`StartupSnapshot.v` proves six properties over arbitrary finite lists.
The proofs cover selection membership, selection size, cursor progress, remaining-work bounds, sequence preservation, and unique visits.
Independent kernel checking confirms closed assumptions.

`StartupSnapshot.tla` separates capture, presence checks, admission, completion, failure, and cancellation.
Concurrent actions change live membership and available capacity.
Storage observations can report presence, absence, or error independently in each phase.
The safe three-key model explores 8,032 states.
It checks safety, not a wall-clock completion bound.

| Unsafe control | Required failure |
|---|---|
| Mutable captured root | `Inv_ImmutableSnapshot` |
| Admission before all presence checks | `Inv_FilterBeforeAdmission` |
| Prune selected hashes from current live membership | `Inv_SelectionPreserved` |

`property_startup_snapshot.rs` imports the production cursor.
Generated histories compare its output with an independent eager ordered-set reference during live insertions and removals.
Page tests include minimum and maximum integer keys.
A 65,536-key test checks zero key clones during capture and two clones for each examined cursor step.

`loom_startup_snapshot.rs` imports the same cursor and instruments the enclosing capture/mutation mutex with Loom.
The model permits capture between removal, insertion, and reinsertion.
It does not instrument internal `imbl` reference counts or prove the dependency's memory implementation.
Native buffer tests cover pendant changes and certificate resolution against the actual storage interface.

Run the focused formal gate inside the required memory limit:

```sh
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  bash scripts/check-recovery-pump-control.sh snapshot-formal
```

Run the focused native gate separately:

```sh
systemd-run --user --scope -p MemoryMax=5G -p MemorySwapMax=0 \
  bash scripts/check-recovery-pump-control.sh snapshot-native
```

Both gates record input hashes and use `target/verification/recovery-pump/` for temporary state.
The [task work log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md) records results and failed attempts.
The complete startup driver still needs phase, ticket, error-policy, and capacity integration tests.
