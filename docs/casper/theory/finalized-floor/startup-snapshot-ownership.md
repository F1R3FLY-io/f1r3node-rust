# Startup snapshot ownership

## Status and purpose

This document specifies the snapshot-ownership repair.
The lease kernel and owner API now implement reservation before capture and exact release after destruction.
Their first native run passed 27 owner and engine tests.
The common recovery driver and production startup caller still need integration.
The [completion owner](startup-completion-identity.md) already checks request identity, callback completion, cancellation, and engine publication.
Its earlier controller-slot bound did not bound physical snapshots held by concurrent callers.

The approved target permits one active scan episode and one pending scan episode.
An episode includes the scanner's source and selected persistent roots.
The bound applies during capture and destruction, not only after public methods return.

This control concerns local resource ownership.
It does not change Casper validity, voting, fork choice, settlement, independent validators, or the two block workers.

## Counterexample

The earlier `submit(P)` API accepted a previously constructed snapshot.
Replacement moves old pending work into an outside-lock cleanup object.
The caller can then pause before that object finishes destruction.
Other callers can repeat this sequence and retain more retired snapshots.

One pending field therefore does not establish one physical pending owner.
The sequential replacement regression misses this concurrent schedule.
Serializing only the controller mutation also misses it.

## Required ownership states

Use one explicit pending-snapshot lease.
The lease is a non-clone ownership token with a private allocation identity.

| State | Owner and permitted work |
|---|---|
| Free | A current startup ticket can reserve capture permission. |
| Capturing | One caller holds permission to construct its snapshot. |
| Stored | The controller owns a completed snapshot for that request. |
| Retiring | An outside-lock owner must finish destroying the snapshot. |

The existing active slot covers both active scanning and active-root destruction.
Each physical episode must belong to exactly one pending or active lease.
A two-permit semaphore alone is insufficient because it can admit two pending captures without an active scan.

The lease state is separate from `StartupCompletion` phases.
A request can remain pending while its lease is capturing.
Activation requires matching stored work and a free active slot before it calls the qualified completion kernel.

## API and caller contract

The implementation replaces `submit(P)` with request creation, capture reservation, and capture under the granted lease.
The internal interface has this structure:

```rust
StartupContext::request_startup(callback_required)
StartupTicket::try_reserve_snapshot(&mut self)
StartupCapture::capture(self, build)
StartupTicket::finish(self, callback)
```

These lines identify the intended API operations, not complete Rust declarations.
The first operation creates only the small request and outcome record.
It cancels the preceding request and starts retirement of any stored pending work.

An unsuccessful reservation returns busy without constructing a snapshot, allocating another waiter, or spawning a task.
The generic builder boundary remains private to the crate.
The public `capture_buffer_snapshot` method accepts a storage reference, not a snapshot or an arbitrary builder.
It constructs the concrete scanner only after reservation.
The production caller must supply the buffer associated with its published context.

The initializer must use the context handle published with its engine.

1. Request peer tips.
2. Create the startup ticket.
3. Attempt to reserve capture ownership.
4. If the reservation is busy, await a change within the existing initializer future.
5. Capture and store the snapshot under the lease.
6. Await the ticket's existing completion path.

Ordinary recovery requests must not create startup tickets or completion waiters.
The runtime must retain ownership of the single pending startup initializer and cancel superseded initializer futures.

## Cancellation, publication, and destruction

Cancellation invalidates the request but does not release a capture lease.
If cancellation precedes capture, reject capture and release the empty reservation.
Cancellation can also occur during an admitted synchronous capture.
That capture may finish, but its result must fail publication and be destroyed before lease release.

For stored pending work, use this sequence:

1. Cancel the request under the controller mutex.
2. Mark its lease as retiring.
3. Move the leased work outside the mutex.
4. Destroy its roots.
5. Release the exact lease.
6. Notify the waiting initializer.

Engine publication must not wait for destruction under either publication guard.
It can commit a new engine while the old lease remains capturing or retiring.
The replacement initializer then receives busy before it can capture another snapshot.

Activation transfers stored pending ownership to active ownership atomically.
The transfer creates no snapshot and exposes no uncharged ownership interval.
Active-root destruction must finish before the active identity becomes available again.

A stale lease destructor must compare its private allocation identity.
It cannot release a replacement lease or a different ownership role.
Use explicit destruction order in the private leased-work container.

Preserve this stop sequence:

1. Publish startup stop.
2. Stop the ordinary recovery signal.
3. Destroy roots outside the guards.
4. Release the exact leases.

## Wake ownership

The driver already consumes the owner's single-consumer change notification.
The capture waiter must not compete for that wake token.
Pending-lease retirement must also notify the current ticket's existing request notification.
Register that waiter before checking reservation availability.

This order prevents a lost wake between the busy check and suspension.
It also prevents the driver from consuming the only wake intended for the initializer.
No additional task, channel, or wait-list entry is permitted for a busy attempt.

## Formal and executable acceptance

Compose lease transitions with `StartupCompletion` rather than treating the pending field as the physical snapshot count.
Model capture, cancellation, engine replacement, activation, stop, and destruction as separate interleaving steps.
Include a capture that finishes after cancellation.
State the bounds and fairness assumptions separately from the safety invariants.

| Unsafe control | Required invariant failure |
|---|---|
| Capture before reservation | `Inv_EverySnapshotLeased` |
| Release capture permission during cancellation | `Inv_AtMostTwoSnapshotOwners` |
| Release pending ownership before destruction | `Inv_ReleaseAfterDestruction` |
| Bypass pending retirement during publication | `Inv_ReleaseAfterDestruction` |
| Admit two pending leases | `Inv_OnePendingOwner` |
| Activate before work is stored | `Inv_ActivationRequiresStoredWork` |
| Release a replacement from an old destructor | `Inv_ExactLeaseRelease` |
| Release the same lease through its stale pending role | `Inv_ExactLeaseRelease` |
| Release active ownership before destruction | `Inv_OneActiveOwner` |
| Allocate internal waiters on busy attempts | `Inv_BusyAllocatesNothing` |

Parameterized proofs must establish lease conservation and the two-owner bound over arbitrary finite histories.
Property tests must derive physical ownership from an independent allocation and destruction history.
Loom must instrument the production lease-transition kernel and its competing ownership operations.
Native tests must use actual capture, publication, cancellation, and destruction paths.

Required native schedules include these cases:

- Block pending-root destruction while other callers request capture.
- Block capture before storage publication, then replace the engine context.
- Block active-root destruction while a stored replacement attempts activation.
- Publish a new engine during pending retirement and confirm that engine reads still proceed.
- Drop an unused capture reservation.
- Cancel between root creation and storage publication.
- Stop during capture or retirement.
- Repeat concurrent busy attempts and check that builders do not run.
- Probe controller and engine guards from destructors.
- Deliver stale lease destruction after request replacement and role transfer.
- Compete driver wakes with capture-availability wakes.

Let $`N_{live}`$ denote live buffer membership size.
Let $`N_{active lease}`$ and $`N_{pending lease}`$ include their episodes throughout capture, storage, scanning, and destruction.
The required metadata bound is $`O(N_{live} + N_{active lease} + N_{pending lease})`$.
This is not a constant-memory or month-long uptime guarantee.
The common driver must satisfy this ownership contract before the resource repair can be marked complete.

## Formal evidence and refinement boundary

`StartupSnapshotLease.tla` extends `StartupCompletion.tla` directly.
Its safe configuration retains the completion guards and adds lease guards.
Request replacement, context publication, cancellation, failure, and stop retain outstanding capture and active ownership.
The finite configuration uses two contexts, three requests, and three lease identities.
Fresh identity selection removes equivalent allocation orders, not operation interleavings.

The first composed check explored 489,518 reachable states to depth 45.
The subsequent role-release check retained that state count and added a tenth unsafe control.
All ten unsafe controls failed their named invariants.
The final timing correction moved abandoned-scan failure from retirement start to completed release.
Its model check passed 493,862 reachable states at depth 45.
The strengthened Rocq gate passed 28 closed theorems and independent kernel checking.
The model checks safety without fairness assumptions or a wall-clock claim.

The model's `DiscardCapture` action represents builder return, abandonment, or unwind.
It does not represent an external cancellation thread clearing a running builder.
Its `Destroy` action means that the entire episode has ended.
That boundary includes selected-root construction during scanning, not only the initial source capture.

`StartupSnapshotLease.v` proves resource conservation over arbitrary finite histories without a finite identity bound.
Its independent ledger records capture events, live episodes, and live builders.
The physical episode bound follows from ledger correspondence and role ownership.
No transition assumes the two-episode bound as a premise.

The Rocq proof is a resource projection, not an unbounded proof of the complete startup API.
The projection requires once-only capture admission, actual builder completion, and actual episode destruction.
The Rust phase guards and private ownership types must establish these premises.
The composed TLA+ model separately checks completion-phase correspondence within its stated finite domain.
The single external initializer waiter remains an integration obligation.

## Executable correspondence

The owner constructs a fresh private lease identity only after the pending role becomes free.
`StartupCapture::capture` consumes its capture handle and calls a `FnOnce` builder.
The lease kernel rejects a second transition from reservation to capture.
External cancellation cannot retire a capturing lease.
Only its capture handle can perform capture abandonment.

`LeasedWork` retains the lease while its payload exists.
Its destructor starts retirement, destroys the payload outside the controller guard, and then releases the exact identity and role.
A separate retirement guard also performs release during destructor unwind.
Active release clears completion ownership only after payload destruction.
The native tests must establish this order for the actual payload paths.

The public active-scan API exposes scanner operations without a replaceable payload reference.
The generic mutable accessor remains inside the trusted queue module.
This restriction prevents callers from extracting a scanner while its lease retires.

| Formal obligation | Executable evidence |
|---|---|
| Once-only capture and independent episode count | The lease property test reconstructs accepted events and counts construction events separately. |
| Capture survives request replacement | A blocked builder retains capacity while a replacement makes repeated capture attempts. |
| Retirement survives publication | A deferred cleanup retains its pending lease after the new context becomes visible. |
| Both episode roots precede release | Separate field destructors block source and selected cleanup independently. |
| Active destruction precedes new activation | A blocked active destructor permits one pending capture but prevents activation. |
| Exact role and identity release | Native and Loom tests attempt stale pending release after active transfer. |
| Capture and driver wakes remain independent | A native test consumes driver notifications while a capture waiter remains registered. |
| Unused reservation and builder unwind release ownership | Native tests drop an unused reservation and unwind a builder that has created a payload. |
| Destructor unwind preserves charges through remaining fields | Native tests panic in a stored payload or its source field and verify remaining field destruction before reuse. |
| Stop preserves an in-flight capture lease | A native test stops the owner while the builder remains blocked. |

The independently owned field probes are test payloads, not measurements of actual RSpace root retention.
Storage integration, runtime resource measurements, and the final soak remain separate acceptance requirements.
The property and Loom targets import the production lease-transition kernel.
The native owner target imports the production owner and engine-cell source.
