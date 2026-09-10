# Startup completion identity

## Purpose and scope

Startup completion must belong to the engine context and request that started the scan.
An old callback result must not complete a replacement request.
This control changes local initialization ownership, not Casper validity, voting, fork choice, or proposal routing.

A **context identity** identifies one engine publication.
A **request identity** identifies one startup request within that context.
Production identities use private allocations and pointer identity, not counters that can wrap or values that can repeat.
A **startup ticket** owns one request until its terminal result or cancellation.

The [snapshot contract](startup-snapshot-semantics.md) defines the two-phase scan.
All presence checks must finish before startup admission starts.
All admission visits must finish before the ticket becomes ready.
An error in either phase prevents startup success.

## State and ownership

The controller retains at most one current request and one active scan identity.
A replacement request can wait while the driver retires old active work.
The driver cannot activate the replacement before it releases the old active slot.
The pending request owns at most one snapshot root.
The driver owns the active cursor and its roots.

| State | Meaning | Permitted next states |
|---|---|---|
| Pending | The request awaits the driver. | Presence, failed, or canceled |
| Presence | The driver checks the captured hashes. | Admission, failed, or canceled |
| Admission | The driver processes the selected hashes. | Ready, failed, or canceled |
| Ready | Both scan phases completed successfully. | Authorized, succeeded, failed, or canceled |
| Authorized | The request consumed its callback permission. | Callback succeeded, failed, or canceled |
| Callback succeeded | The controller recorded a successful callback result. | Succeeded, failed, or canceled |
| Succeeded | Startup success was committed. | No further transition |
| Failed | Startup failed. | No further transition |
| Canceled | Cancellation, replacement, or stop revoked the request. | No further transition |

An active identity can remain after its request becomes canceled.
Cancellation does not establish that an asynchronous operation stopped or that its resources were released.
Only the driver that owns that identity can retire the active slot.

## Authorization and success

Callback authorization has one atomic commit point under the controller lock.
The controller requires the exact current context, exact current request, ready state, and a controller that has not stopped.
Only that transition grants callback permission.
No controller lock remains held during callback polling or execution.

Authorization is not the callback's first poll, queue admission, or proposal execution.
The existing proposal consumer selects the current Casper when it processes the request.
The startup snapshot contract does not require proposal execution against the captured Casper.
The implementation must preserve `ProposeRequestKind::PendingDeploy` and the existing generic routing.

Replacement before authorization prevents authorization.
Replacement after authorization cannot revoke work that already received permission.
It must prevent that old request from subsequently committing startup success.
After the callback returns successfully, the controller checks both identities and records that result from the authorized state.
The success commit requires that recorded result for a callback-required request.
Permission alone cannot complete startup.
Without a configured callback, the controller commits success directly from the ready state.

Success also has an atomic commit point.
Replacement before that commit suppresses success.
Replacement after that commit cannot revoke a completed result that the caller has not observed yet.
An error or cancellation cannot change an already committed success.

## Engine publication and stop

Engine publication and context registration must follow the same order.
Separate registration and engine-swap operations can publish engine A while the recovery controller identifies engine B.
Hold the existing engine write guard and controller mutex through registration replacement and the pointer swap.
Every engine replacement must participate, including replacement with a non-running engine.

Use this lock order:

1. Acquire the engine write guard.
2. Acquire the bound controller mutex.
3. Validate the replacement before changing either state.
4. Revoke the old registration and install the replacement registration.
5. Replace the engine slot.
6. Release the controller mutex.
7. Release the engine write guard.
8. Notify affected tasks and destroy retired owners outside both guards.

Releasing the controller mutex before the slot swap exposes intermediate state to driver and callback threads.
Controller operations must never acquire or await the engine lock.
A private synchronous swap closure can perform the infallible slot replacement under both guards.
That closure must not execute callbacks, access storage, publish events, or destroy retired owners.

Bind one recovery controller to each engine cell.
Preserve this binding across non-running replacements.
Reject a different controller by allocation identity before mutation.
Controller migration is not part of the shared-driver repair.

For a non-running replacement, clear registration and cancel uncommitted startup under both guards.
Preserve active scan ownership until its driver retires the work.

Do not rely on the old engine's destruction to revoke its registration.
Other readers can retain clones of that engine.
An old registration guard must revoke only its exact identity.
Release retired engines and snapshot roots after releasing publication and controller locks.

Stop must acquire the same controller lock that grants callback permission.
A separate stop-flag check followed by permission mutation leaves a race.
Stop prevents new requests, new activations, new callback permissions, and new startup-success commits.
Stop does not revoke an already authorized callback or undo a committed success.

## Algorithm

The ticket follows this sequence:

1. Submit the captured snapshot with both private identities.
2. Await the matching driver's result.
3. If a callback exists, consume callback permission under the controller lock.
4. Invoke and await the callback outside the lock.
5. If a callback exists, record its successful result under the controller lock with the same identities.
6. Commit success under the controller lock with the same identities.

Each failure reports an error to its own ticket.
Dropping a ticket cancels only its exact request.
Ordinary retry requests allocate no startup completion channel.
Ordinary retry completion cannot satisfy or fail a startup ticket.

## Verification obligations

The TLA+ model separates engine publication, request replacement, scan phases, authorization, callback return, success commit, cancellation, retirement, and stop.
Its bounded configuration contains repeated requests within one context and replacement by another context.
Named unsafe controls must fail the intended invariant, not parsing, time limits, or resource limits.

| Obligation | Required test |
|---|---|
| Exact scan completion | Complete old work after installing a replacement request. |
| Exact authorization | Replace a context after a ready notification but before authorization. |
| Exact success commit | Replace a context during an authorized callback. |
| Committed result stability | Replace a context after success but before caller observation. |
| Exact retirement | Drop old tickets and registrations after installing replacements. |
| Stop ordering | Race stop against authorization and success commit. |
| Bounded ownership | Repeatedly replace pending work while an old scan remains active. |
| Publication ordering | Race two engine publications and compare their registrations. |
| Phase ordering | Attempt early completion and deliver duplicate or stale phase results. |

Property tests must compare production transitions with an independent event-history reference.
Loom tests must import the production transition kernel and explore competing operations at its lock boundary.
Native tests must cover actual channels, ticket destruction, callback suspension, and root retirement.
Kernel tests alone do not establish correct placement of the engine publication lock or correct integration of the driver.

## Evidence status

The plan-agent review established these boundaries through source inspection.
The production `startup_completion.rs` kernel now implements the local phase and identity transitions.
Its generic identity parameters do not allocate identities.
The `startup_owner.rs` wrapper supplies fresh private allocations for every context publication and request.
Reusing an old identity is outside the kernel's freshness contract.

`StartupCompletion.v` contains seventeen closed Rocq proofs with independent kernel checking.
These proofs cover exact origin, authorization, callback-result observation, stop exclusion, exact retirement, and success preservation across arbitrary cleanup histories.
They do not prove the complete recovery service or extract Rust from Rocq.

`StartupCompletion.tla` checks two contexts and three unique requests.
Two requests share one context, and the third belongs to the replacement context.
The configuration includes callback-required and callback-free requests.
The reviewed model explored 20,219 distinct states and ten named unsafe controls.
It does not claim liveness or unlimited identity-domain model checking.

The shutdown refinement leaves the published engine pointer intact.
After stop, the controller rejects new permissions even if an engine reader retains the old pointer.
The publication invariant applies while the controller operates.
This avoids assuming a shutdown pointer swap that the implementation does not perform.

Six completion property/example tests and five Loom tests passed with strict standalone lint.
The property reference retains an independent event history rather than copying the kernel's current-state record.
Generated histories include stale identities, complete phase transitions, replacement, failure, retirement, and an optional stop position.
Loom instruments competing calls to the actual kernel under a mutex.
Separate native tests exercise the wrapper's actual mutexes, notifications, asynchronous callbacks, and destructors.

The review added a callback-result observation to the kernel and wrong-context probes to the property corpus.
The TLA+ model now retains terminal current records, matching Rust.
Its ownership controls cover stale active retirement separately from stale request cancellation.
The final component qualification passed these reviewed changes and strict Casper library/test lint.

The model records a successful callback return in its `returned` set.
Rust represents the corresponding live request with `StartupPhase::CallbackSucceeded`.
The wrapper must report that observation only after the actual callback future returns successfully.
The wrapper now records that observation only after its actual callback future returns successfully.
The kernel's standalone guarantee still depends on that caller behavior.

The evidence directories and source hashes are recorded in the [work log](../../../work-logs/task-pr216-admission-backpressure-2026-09-06.md).
## Runtime ownership and publication

The runtime owner holds one request record, one pending work item, and one active identity.
Each ticket retains its own outcome record after replacement.
The controller does not retain a history of old tickets.
External callers can retain old tickets, but those tickets do not retain snapshot roots or strong Casper references.

The controller-slot bound does not yet bound snapshots held by concurrent incoming calls or retirement cleanup.
Several replacement callers can each retain an old root after releasing the controller lock.
The sequential replacement regression does not establish a concurrent physical-resource bound.
The shared-driver integration must acquire a snapshot ownership lease before capture and retain that lease through actual root destruction.
It must include publication-triggered retirement, not only ordinary submission.
Until that integration passes, resource accounting must include incoming and retiring roots separately.
The [snapshot ownership plan](startup-snapshot-ownership.md) specifies the required reservation, role transfer, and retirement protocol.

The wrapper registers a notification waiter before it reads the request outcome.
This order closes the notification-before-wait race.
Every outcome update occurs under the controller lock before notification.
Notifications occur outside the controller lock.

Ticket destruction cancels the exact request.
Destruction of an unpolled completion future also destroys its ticket.
During callback execution, the completion future waits for either the actual callback result or a terminal request outcome.
Cancellation can destroy an authorized callback future before its first poll.
Authorization permits execution but does not require execution after cancellation.

`ActiveStartup` owns the scanner work and holds weak references to its controller.
It releases scanner work before it retires the active identity.
Abandoning a current presence or admission phase fails that ticket and wakes its waiter.
An old active owner cannot change the replacement request.

`EngineCell::set_running` publishes the registration and engine under both guards.
`EngineCell::set` revokes registration for non-running replacements and preserves the bound controller.
Both methods destroy retired engines after releasing their guards.
Rejected incoming engines also survive until guard release.
This last rule prevents destructor re-entry from deadlocking a rejected publication.

Stop separates state publication from cleanup.
`RecoveryControl::stop` first stops the startup owner, then stops the ordinary recovery signal.
Only then does it destroy pending roots.
Root destruction can take linear time and must not delay the ordinary stop signal.

`transition_to_running` publishes `EnteredRunningState` only after a successful engine-slot commit.
A rejected engine produces no Running-state event.
The current event implementation cannot return an error for `EnteredRunningState`.
The caller still propagates a future event-publication error without rolling back the committed engine.
Events and success logs occur outside both guards.

## Publication model and native evidence

`StartupPublication.tla` separates two concurrent publishers, both guards, registration, engine swap, rejection, owner destruction, event reporting, and stop.
Its safe configuration explores 624 states with both publishers and an independent stop operation.
Four unsafe configurations expose early controller unlock, destruction under a guard, delayed stop signaling, and premature Running-state reporting.
These checks establish the declared local safety properties, not whole-node liveness or multi-validator consensus correctness.

The native `startup_runtime` target imports the actual owner, completion kernel, and `EngineCell` source.
Its publication tests substitute only the surrounding engine, Casper-context, and error dependencies.
The gate also runs a Casper integration regression against the actual `transition_to_running` and event publisher.
That regression distinguishes successful publication, a stopped controller, and a different bound controller.

Generated wrapper histories compare ticket outcomes and retained-root counts with an independent reference.
The examples test suspended callbacks, callback errors, stale readiness, owner destruction, stop, weak references, and committed-result preservation.
Destructor probes test lock release before root and engine destruction.
The kernel Loom corpus remains a separate instrumented concurrency check.
Native Tokio tests do not claim exhaustive scheduler exploration.

The common recovery driver and startup caller still need integration.
The caller must receive the same prepared context handle that `transition_to_running` publishes.
It must not create another identity for that startup request.
The current component evidence does not establish that the old eager startup or maintenance scans have been replaced.
