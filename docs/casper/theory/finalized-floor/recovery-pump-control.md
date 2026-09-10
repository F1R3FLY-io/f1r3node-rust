# Bounded recovery control

## Scope and status

This document defines the retry control for `pr216-admission-backpressure`.
It refines the [admission and recovery plan](admission-and-recovery-backpressure.md).
The control models passed before production implementation.
The production wake and pass modules now have focused Loom and property-test results.
Queue release wiring, native lifecycle checks, and strict Casper/node lint passed.
The startup ticket and lease components have focused qualification.
The shared driver, context-bound initializer call sites, and owned dispatcher are now implemented.
Their combined production qualification remains in progress.

The repair controls local work admission and task ownership.
It does not change Casper voting, certificate thresholds, fork choice, block validity, block encoding, or cost settlement.
Independent validators and bounded block workers remain concurrent.

## One retry driver

One driver executes retry pages.
Startup, periodic maintenance, certificate completion, worker completion, and capacity release request work from that driver.
Those callers do not execute scans or allocate a task for each request.
This ownership rule removes competition for the previous shared scan lock.

The driver retains a constant number of control records.
The [candidate index](buffer-candidate-rotation.md) retains one entry per live buffer identity.
These are separate bounds.
A constant-size control state does not imply a constant-size durable buffer or whole-node heap.

## Wake state

The production module is `recovery_signal_state.rs` under `casper/src/rust/blocks/block_processing_queue/`.
It uses an atomic state with four valid values.
A proposal request means that a successful later pass can permit an optional post-block proposal.
It does not directly invoke the proposer.

| State | Meaning |
|---|---|
| `Idle` | No unconsumed request exists. |
| `Work { proposal: false }` | At least one retry request exists. |
| `Work { proposal: true }` | Retry work and post-block proposal demand exist. |
| `Stopped` | The driver must not accept more work. |

`request` combines repeated requests with compare-and-exchange operations.
Combining requests preserves proposal demand.
The operation reports whether it changed idle state into pending work.
The adapter then calls `Notify::notify_one`.
`take` consumes demand before a pass starts.
Pass completion never clears newly arrived demand.

An active pass can consume another wake while it waits for capacity.
That wake can carry proposal demand for the next pass.
The driver must merge the returned wake into one successor-demand record.
`RecoveryWake::merge` combines work with logical OR and treats `Stopped` as terminal.
The merge is associative, commutative, and idempotent, with `Idle` as its identity.
These properties permit repeated capacity wakes without losing prior proposal demand.

The expanded `RecoveryPumpDemand.tla` records shared and successor request witnesses independently.
Its new unsafe control consumes a shared wake but discards the returned work.
The witness survives that mutation and exposes the lost request.
The revised safe configuration explores 2,466 states with three requests and all eight proposal-flag assignments.
`RecoveryDemandHandoff.v` proves ten merge and transfer properties over arbitrary finite input histories.
The formal gate passed before the production merge operation was added.
The driver now merges consumed wake results into successor demand.
Its combined concurrency qualification remains in progress.

`stop` replaces any state with `Stopped`.
Neither a late producer nor a canceled worker's release can reverse this transition.
The stop adapter also notifies the driver.

The notification must retain one token before a waiter registers.
The driver checks demand, then awaits a notification when no demand exists.
A producer can notify between these operations.
A retained token closes this interval.
An old token can cause one redundant check, but cannot create a retry request.

## Pass state and capacity

`RecoveryPass` records the remaining candidate visits, a failure flag, and the pass's proposal demand.
Each visit decrements the remaining count exactly once.
A failure remains recorded through all subsequent pages.
Only a complete, successful pass can permit its optional proposal.

The driver must yield between pages even when more work is ready.
Continuation is driver-local state, not a new external request.
Otherwise, each completed pass could manufacture another pass indefinitely.

A full count queue parks an unfinished pass before another body load.
The pass resumes after a capacity event or a retry deadline.
Parking does not report success or erase a failure.
The driver must preserve a proposal request that arrives during an older failed pass.

The model includes competing admissions that can consume newly available capacity.
Completion requires eventually sustained usable capacity and fair driver scheduling.
Repeated short capacity openings do not prove admission fairness for arbitrary block sizes under unlimited competing traffic.
This limitation is explicit rather than hidden in a serial model.

## Admission release order

The queue first reserves bytes and stages the complete queue item.
It then obtains a nonblocking channel permit.
A failed channel reservation drops the staged body and lease without a retry wake.
Only successful transfer arms a release wake.

| Release stage | Effect |
|---|---|
| Block body or processing future | Releases the corresponding payload. |
| Byte reservation | Returns the encoded-byte capacity. |
| Admission identity | Removes exactly that ownership token. |
| Release wake | Requests retry work after both capacity and identity release. |

Dequeue provides a separate count-capacity wake.
Rejected temporary byte reservations must not wake the driver.
Otherwise, a rejected count admission can repeatedly request the same unsuccessful scan without any useful capacity change.

The weak queue endpoint does not keep the input channel open.
It retains accounting and control handles, not a strong channel sender.
The Casper context is also weak.
The driver must release each temporary strong upgrade before it waits.

## Startup and dispatcher integration requirements

Startup has a distinct proposal path.
It requests peer tips, waits for the startup scan, and then invokes the existing startup proposal callback.
Startup must not inherit the post-block heartbeat condition, environment flag, or extra bond check.

The reviewed interface permits one owned startup ticket per registered context.
Private allocation identities bind the ticket and context.
A previous pass cannot complete a newer ticket.
Cancellation, replacement, and shutdown prevent a later startup-success commit.
They cannot revoke a previously committed result or undo an already authorized callback.
The [completion identity contract](startup-completion-identity.md) defines these separate commit points.
Ordinary retry requests do not allocate completion waiters.

The old startup scan snapshots pendants and has a different error policy from ordinary retries.
The [startup snapshot contract](startup-snapshot-semantics.md) defines persistent membership and bounded cursor traversal.
The driver must preserve this distinction or explicitly document and justify a semantic change.
A normal-only candidate error must not fail the startup ticket.
A completed normal pass must not satisfy that ticket accidentally.

`BlockProcessorInstance::run` now owns its queue receiver, bounded worker set, retry driver, and bounded proposal service.
The [composition specification](recovery-dispatcher-composition.md) defines the remaining model, implementation, and regression obligations.
Supervisor cancellation must reach these children.
Orderly shutdown must close input, discard queued items, request worker cancellation, and observe worker retirement.
An abort request does not prove immediate payload destruction.
That destruction requires task or runtime progress.

The startup review also found a separate cancellation boundary in `NodeRuntime`.
Its former `select!` branch took the initialization handle before awaiting completion.
A different winning branch could drop that handle and detach initialization.
The deterministic negative control reproduces this loss while the child continues execution.

The runtime now owns initialization through a dedicated, one-member `JoinSet`.
The production `runtime_supervision.rs` module selects through borrowed `join_next` operations.
A canceled selection does not remove a pending child or an unobserved result.
The parent removes each child only when its completion wins selection.
The same module aborts both task sets before awaiting either set, then drains both concurrently.

The runtime preserves its existing result policy:

| Observed initialization result | Startup sealing | Runtime action |
|---|---|---|
| Success | Seal startup. | Continue monitoring. |
| Application error | Seal startup. | Log the error and start shutdown. |
| Task panic or cancellation | Do not seal startup. | Log the task error and start shutdown. |

Shutdown draining does not invoke these handlers.
If shutdown wins a completion race, draining does not introduce a later startup-sealing event.
The existing error-return policy also remains unchanged.

The existing 30-second timeout limits the orderly shutdown wait under cooperative scheduling.
A timeout does not prove that a non-yielding task has stopped.
Dropping either task set requests cancellation, but cannot await destruction.
Only a completed drain establishes retirement of every child owned by those sets.
The block dispatcher now retains its nested worker and service handles.
Its production cancellation and termination checks remain separate qualification obligations.

## Formal correspondence and test scope

### Narrow dependency reads

The shared driver needs admitted metadata, not the complete DAG representation.
Each narrow read must check admitted-set membership and decode the corresponding metadata row.
A persisted row without admitted membership does not authorize retry.
A missing row returns false, even when the admitted set contains its hash.
A decoding, validation, key-mismatch, or storage error must remain an error.

The read uses the existing shared lock order: global storage lock, metadata index, then DAG state.
It retains no guard across the next candidate, block load, or asynchronous wait.
Parallel readers retain shared access.
The driver must not use `Casper::dag_contains`, which constructs a full representation and suppresses errors.

Dependency traversal borrows hashes from the loaded block.
It includes parents, justifications, successful slash evidence, and header evidence.
Certificate dependencies include latest messages, the predecessor floor, the predecessor carrier, and the target floor.
Both hashes of each evidence pair remain dependencies.
Only certificate-derived all-zero sentinel hashes are excluded.
The existing sorted, deduplicated dependency function remains unchanged for consensus callers.

The retry predicate visits every dependency occurrence unless a read returns an error.
A missing dependency must not hide a later error.
Duplicate references can cause repeated reads but cannot add authority.
Each read observes current admitted metadata rather than the old pass-wide membership snapshot.
This can advance local retry timing when another worker admits a dependency.
The block worker must still perform normal validation.

The page limit bounds candidate visits, not the size of each candidate.
A candidate with many dependencies still needs one metadata read per dependency occurrence.
Traversal creates no copied dependency collection and reads one metadata key at a time.
The existing typed store can temporarily retain two decoded copies of that row because `get_one` clones its result.
The serialized row can also coexist with its decoded value during decoding.
These copies do not accumulate across dependency reads.
Traversal also inspects source entries that yield no dependency, including failed system deploys and certificate sentinels.
Certificate prechecks still inspect the candidate's parent edges.
These bounds do not establish constant work per candidate or a fixed whole-node memory bound.

The ordinary driver uses one error policy for all wake sources.
It records stage failures, preserves unresolved obligations, continues later candidates, and suppresses optional proposals for the failed pass.
Successful admission precedes retriever acknowledgment.
Stale-buffer removal must succeed before tracker cleanup.
This replaces route-specific eager failure timing and duplicate body reads.
Startup retains its separate complete presence barrier and documented fatal and nonfatal outcomes.

`RecoveryMetadataReadiness.tla` models row publication and mutation between independent dependency reads.
Its witnesses describe the state at each read, not a snapshot that remains valid after arbitrary storage corruption.
Four unsafe controls remove visibility, row existence, error propagation, or the check after a missing dependency.
`RecoveryMetadataReadiness.v` proves readiness and fault properties for arbitrary finite observation lists and source groups.
The formal gate passed eight closed Rocq theorems and independent kernel checking before the production change.
TLC explored 39,996 distinct safe states with two keys, three reference occurrences, and eight initial reference assignments.
All four unsafe controls violated their specified invariants.
These results concern read witnesses and error propagation, not complete recovery-driver integration or consensus liveness.

The production bridge uses storage tests, generated dependency lists, and actual `MultiParentCasperImpl` resolver tests.
The lock regression blocks the backend read and requires both publication and metadata-index write attempts to fail.
This handshake avoids relying on thread scheduling before the read begins.
The resolver regressions cover corruption after a missing dependency, missing bodies, missing rows, later admission, and certificate waits.
The in-memory fixture executes the production resolver without genesis replay or a running network.
It therefore does not qualify replay, multi-node recovery, or the campaign soak.

| Model | Checked behavior | Safe domain |
|---|---|---|
| `RecoveryPumpWake.tla` | Split publication, notification, last idle check, waiter registration, service, and stop | Two independent actors and two independently assigned requests. |
| `RecoveryPumpPass.tla` | Bounded pages, competing admission, capacity parking, failure preservation, and proposal suppression | Five candidates and a two-visit page budget. |
| `RecoveryPumpDemand.tla` | Concurrent shared demand, successor transfers, continuation, newer proposal demand, and terminal stop | Three requests with all proposal-flag assignments. |
| `RecoveryDispatcherOwnership.tla` | Concurrent queue/worker ownership, closure, abort request, retirement, and joining | Three jobs, two queue slots, and two worker slots. |
| `InitializerOwnership.tla` | Canceled selection, completed results, parent cancellation, concurrent shutdown, and retirement | One initializer and two independent critical tasks, with success, error, panic, and cancellation outcomes. |

The safe state counts are 11,124, 123, 194, 356, and 4,338, respectively.
All twelve unsafe controls must violate their named invariant.
Parser errors, incomplete successor states, timeouts, and resource exhaustion do not satisfy a negative control.

`RecoveryPumpControl.v` proves twenty parameterized control properties.
The proofs cover coalescing, stop preservation, request ordering, page arithmetic, and failure preservation over arbitrary finite histories.
Independent kernel checking passed with closed assumptions.
These proofs do not establish the unfinished startup ticket or complete dispatcher implementation.

`InitializerOwnership.v` adds eight closed ownership proofs with independent kernel checking.
The proofs preserve ownership across arbitrary finite operation histories.
They distinguish an abort request from child retirement.
The TLA+ model checks interactions among the initializer, other tasks, completion observation, and supervisor shutdown.
Neither proof replaces the cooperative runtime-scheduling premise.

Loom imports the actual atomic wake module with instrumented atomics.
Its four tests cover competing requests, proposal retention, stop races, and the notification interval.
The configured maximum is three threads and 1,000 branches per execution.
There is no preemption, permutation, duration, or checkpoint cutoff.

The notification adapter models a stored token, not Tokio's complete waiter list.
The pinned Tokio 1.53.1 implementation uses sequentially consistent read-modify-write operations for this token.
The first test approximation used weaker notification operations and failed its schedule assertion.
That approximation did not match the dependency implementation.
The corrected adapter retains the production wake module's acquire/release orderings unchanged.
Native asynchronous tests exercise the actual Tokio notification adapter and wait cancellation.

Generated tests compare operation histories with an independent request-list reference.
Other properties vary page boundaries, error positions, and the full `usize` counter range.
Queue regressions distinguish successful release wakes from rejected reservations.
The existing lifecycle regression continues to observe actual body destruction and identity ownership.

`initializer_ownership.rs` imports the actual runtime event-selection and shutdown module.
Its ten tests include the legacy negative control and direct Tokio contract checks.
Production-linked checks cover success, application error, panic, wait cancellation, parent cancellation, and completed-result races.
Captured drop probes check cancellation before a child's first poll and cancellation after execution starts.
Shutdown tests require both task sets to become empty before the drain returns.
Generated sequences mix wait cancellation, startup notifications, critical-task completion, and scheduler yields before observing the initializer exactly once.
These are native Tokio tests, not exhaustive Loom exploration of Tokio internals.

## Reproduction and remaining limits

Run the focused gate within a memory-limited scope:

```sh
systemd-run --user --scope -p MemoryMax=5G -p MemorySwapMax=0 \
  bash scripts/check-recovery-pump-control.sh
```

The gate supports `formal` and `native` modes.
The `initializer` mode checks only initializer proofs, controls, native tests, and standalone lint.
The `initializer-native` mode runs those native tests and strict node library/test lint without repeating unchanged formal checks.
The `snapshot-formal` and `snapshot-native` modes isolate the persistent snapshot checks.
The `metadata-formal` and `metadata-native` modes isolate admitted-metadata readiness and actual resolver coverage.
The `handoff-formal` and `handoff-native` modes isolate successor-demand transfer and its production-kernel tests.
It records input hashes and checks them at exit.
It writes proof state, build temporaries, and logs under `target/verification/recovery-pump/`, not memory-backed `/tmp`.
Keep the combined memory limits of simultaneous verification jobs within the approved host budget.

Passing these component checks does not establish complete pipeline integration, whole-node memory bounds, or month-long uptime.
The remaining work includes combined driver qualification, quarantine consistency, and pruning/retriever composition.
Integration tests and the pinned-revision soak remain required.
