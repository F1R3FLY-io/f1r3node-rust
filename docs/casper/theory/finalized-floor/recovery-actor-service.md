# Recovery actor service

## Purpose and status

The runtime state requester retrieves missing RSpace roots while a node runs.
The requester receives chunks, receives owner commands, and retries requests after a timer expires.
These three inputs are its service lanes.
The enclosing [backpressure plan](admission-and-recovery-backpressure.md) defines the other repair boundaries.

The actor now uses explicit rotating service.
Production-linked scheduler tests, generated properties, and Loom checks passed.
All 11 focused Casper actor tests and strict library Clippy also passed.
One-step symbolic induction and bounded symbolic reachability through length eight passed.
This document does not claim completion of the backpressure task.

## Defect and correction

The previous biased selector always examined chunks first.
A continuous stream of chunks could prevent owner commands and retry ticks from receiving service.
Unknown chunks could cause this problem even when the importer rejected every chunk.
The previous timer also prevented the selector from terminating after both input channels closed.

`ServiceRotation` starts each search after the last selected lane.
It polls each lane at most once and returns the first ready event.
If no lane is ready, the cursor remains unchanged.
Pending channel and timer polls register the task for a later wakeup.

`RecoveryInbox` tracks channel closure separately from an empty channel.
A closed channel can still contain queued messages.
The inbox observes closure only after it receives all queued messages from that channel.
After it observes both closures, the inbox terminates without polling the timer again.

| Lane | Ready event | Work within one service turn |
| --- | --- | --- |
| Chunks | One message or drained-channel closure | Import one recognized chunk and prepare at most one request. |
| Commands | One command or drained-channel closure | Process the first command and at most 15 additional immediately available commands. |
| Retry timer | One due tick | Select at most 16 root requests through the existing recovery window. |

Each request batch remains concurrent.
The actor waits for that bounded batch before it selects the next lane.
The bootstrap sender retains its finite transport timeout.
This repair does not add a global lock, serialize validators, or change block validation and voting.

## Service bound

A continuously ready lane receives service after at most two other service turns in the three-lane requester.
Readiness must persist until the actor polls the lane.
New arrivals cannot repeatedly restart the search at the chunk lane.

Let $`n`$ denote the lane count.
Let $`r`$ denote the target lane's cyclic distance from the next search position.
Let $`d`$ denote completed service turns that bypassed the continuously ready target.
The service invariant is:

```math
0 \le r < n, \qquad d+r<n.
```

Selecting an earlier lane reduces the target's distance by at least one.
Selecting the target resets its bypass count to zero and its distance to the last position.
Thus, fewer than $`n`$ other service turns can bypass a continuously ready target.
The Rocq proof accepts any positive lane count.
The production requester instantiates that rule with three lanes.

This bound counts service turns, not seconds.
It requires terminating importer calls, finite request completion, and eventual task scheduling.
A slow importer or a blocked runtime can still delay every lane.
No wall-clock service guarantee follows without bounds on those operations.

## Implementation correspondence

| Formal element | Production element | Regression evidence |
| --- | --- | --- |
| Cyclic first-ready selection | `recovery_service_rotation.rs::ServiceRotation::poll` | Generated readiness histories and complete three-lane traces of length four. |
| `Serve` and cursor update | `recovery_actor_inbox.rs::RecoveryInbox::next` | Continuous chunk traffic with ready commands and due ticks. |
| Closed-input observation | The `items_open` and `commands_open` fields | Queued-message drain, independent channel closure, and terminal idempotence. |
| No service from a pending poll | `poll_fn` and cursor preservation | Cancellation of a pending future followed by both input arrivals. |
| `Finish` | Completion of the existing concurrent request batch | Actual actor tests with a 16-request barrier. |
| `Stop` | `run_requester` exits when `next` returns `None` | Actual closed-channel actor test and receiver-drop test. |
| Independent actors | Each requester owns its cursor, channels, and recovery core | Two-actor TLA+ exploration and concurrent queue producers in Loom. |

The model represents one bounded command batch as one command service turn.
Additional commands affect only that selected lane during the turn.
The scheduling proof does not assume a separate service turn for each command in that batch.

The model does not implement Tokio's internal channel algorithms.
Native asynchronous tests exercise those actual channels.
Loom imports the actual production selection kernel and explores concurrent producer, consumer, and closure operations.
Loom does not replace the native channel tests or prove whole-node memory bounds.

## Verification layers and limits

`formal/rocq/finalized_floor/theories/RecoveryActorService.v` contains 11 proved selection and service theorems.
Compilation and independent `coqchk` passed without custom assumptions.
The proof covers arbitrary finite bypass sequences, not only example traces.

`RecoveryActorService.tla` represents input arrivals, closures, due ticks, active service, completion, and independent requester actors.
The single-actor configuration uses channel capacity two.
The parallel configuration uses two actors and channel capacity one.
Weak fairness applies to actor service, batch completion, and shutdown.
The environment can continuously supply chunks and does not need to stop for commands to receive service.

The initial TLC checks passed 2,205 single-actor states and 1,010,025 parallel states.
The fixed-priority negative control violated `Inv_BoundedServiceGap`.
The timer-retention negative control violated `Live_ClosedInputsTerminate`.
The permanent TLC gate repeated both safe checks after the type annotations and inactive-lane strengthening.
It also reproduced both unsafe controls and verified all recorded input hashes.

The strengthened safety predicate also requires zero bypass debt for inactive lanes.
This condition permits an inductive check from arbitrary states that satisfy the safety predicate.
Ordinary bounded reachability starts from `Init` and remains a separate obligation.
The one-step check passed for two actors and channel capacity two.
The separate length-eight reachability check also passed, followed by another successful induction check.
Both logs report `EXITCODE: OK`, and their recorded model hashes still match.

Generated tests exercise 1, 2, 3, 17, and 64 lanes with histories of up to 999 readiness changes.
The complete three-lane test covers every four-event readiness sequence from each initial cursor position.
These finite tests check production correspondence but do not replace the parameterized proof.

The [task work log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md) records commands, input hashes, results, and memory limits.

## Reproduce the checks

Run the dedicated gate in a memory-limited scope:

```sh
systemd-run --user --scope -p MemoryMax=6G -p MemorySwapMax=0 \
  bash scripts/check-recovery-actor-service.sh all
```

The gate rejects an uncapped scope or enabled swap.
It stores evidence under `target/verification/recovery-actor-service/` and checks its recorded input hashes on exit.
Modes `rocq`, `tlc`, `apalache`, `native`, and `casper` select independent layers.
Mode `formal` selects Rocq, TLC, and Apalache.
The full finalized-floor gate includes the formal checks and both production-linked scheduler targets.
