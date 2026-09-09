# Recovery dispatcher composition

## Status and purpose

This document specifies the ownership integration for `pr216-admission-backpressure`.
It supplements the [recovery control contract](recovery-pump-control.md) and the [admission plan](admission-and-recovery-backpressure.md).
The plan-agent review completed on September 7, 2026.
This specification is not evidence that the dispatcher migration is complete.

The previous `BlockProcessorInstance::create` detached its dispatcher and block workers.
It retained strong queue senders and performed dependency scans and optional proposals inside worker permits.
The replacement now uses an awaited dispatcher and owned worker and service sets.
Its production integration checks remain in progress.

The replacement must preserve independent validators and the configured parallel block-worker count.
It must not change Casper validity, votes, certificates, fork choice, block encoding, or cost settlement.
The repair changes local scheduling and resource ownership.

## Ownership boundaries

`BlockProcessorInstance::run` must own the receiver, worker set, recovery service, and proposal service.
The runtime must await this dispatcher through its existing critical-task ownership boundary.
No worker or service may outlive that boundary without a retained cancellation owner.

The worker limit must include completed workers whose results have not yet been joined.
The dispatcher must not use unobserved completion as permission to allocate an unbounded set of additional handles.
Two workers must remain able to execute independently when the configured limit is two.

The recovery service must be the only producer of optional post-block proposal offers.
The proposal service must retain at most one active offer and one replaceable pending offer.
The proposal callback may wait without preventing block dispatch or recovery-page progress.

The startup initializer remains a separate owned task.
Its context identity must come from the same prepared registration that the engine publishes.
The [startup completion contract](startup-completion-identity.md) and [snapshot lease contract](startup-snapshot-ownership.md) remain required.
Context replacement cannot release old snapshot roots before their actual destruction.

## Physical release and cancellation

The composition model must track payloads, byte leases, identity leases, and armed wakes independently from worker program counters.
This prevents a state definition from assuming the ownership invariant that the checker must establish.

Use this worker release sequence:

1. Drop the processing payload and its retained context.
2. Release the encoded-byte reservation.
3. Release the exact admission identity.
4. Publish the successful admission's release wake.
5. Record worker retirement.
6. Join the completed worker handle.

Queue dequeue can publish a separate count-capacity wake before payload destruction.
That event is valid because dequeue releases a queue slot, not the worker's byte reservation.
Rejected admission must not arm a release wake.
Otherwise, repeated capacity rejection can generate an endless retry loop without new capacity.

An abort request only requests cancellation.
It does not establish payload destruction, service retirement, or snapshot release.
Orderly shutdown must observe child retirement before reporting completion.
Parent destruction requests cancellation but cannot synchronously guarantee that a non-yielding child has stopped.
The model's parent-drop transition starts destruction.
An independent receiver owner retains queued cleanup until that cleanup finishes.
Separate cancellation records represent submitted abort requests for active children.
Those records do not erase child payloads.

The worker bound includes attached handles and children that still await destruction after cancellation.
Orderly stop and post-cancellation quiescence are distinct outcomes.
Quiescence is not a successful return from the destroyed dispatcher future.

The external startup initializer can retain a capture while dispatcher shutdown completes.
The dispatcher must release its own active capture.
Only the outer runtime's initializer drain can establish that all external capture roots have disappeared.

## Queue endpoint lifetime

Long-lived actors must hold weak queue endpoints.
A temporary strong upgrade must remain visible in the model's independent sender ledger.
The actor must drop that upgrade before acknowledgment, yield, or any capacity wait.
An await transition must not silently erase the sender ledger.

After external senders disappear, internal weak handles must not keep the receiver open.
Workers must not retain strong queue senders through replay or proposal waits.
Native tests must exercise receiver closure while recovery waits and while it yields between pages.
The model includes a separate temporary sender for dispatcher metrics.
A surviving temporary sender can permit another weak upgrade after the last external sender disappears.
Each borrower must release its own sender.
The production dispatcher now records dequeue metrics through the weak endpoint and the receiver's length.
It does not upgrade a sender for this operation.
The model's optional metric borrow therefore overapproximates this production path.

## Current and successor demand

`RecoverySignal::wait` consumes the shared signal through `take`.
The driver can call it while an existing pass waits for capacity.
The returned value can include a new proposal request, not merely a capacity event.

The driver therefore needs one bounded successor-demand record.
It must merge consumed work into that record with logical OR for proposal flags.
The next pass consumes both shared and successor demand.
Current-pass completion and page continuation must preserve both successor sources.
Stop closes both sources.

The model must retain request identities in independent shared and successor witness sets.
A negative control that discards consumed work must retain its witness identities.
Discarding the state and its witness together would hide the defect.

Leaving shared demand permanently unconsumed is not an equivalent solution.
The notification adapter emits a notification only when demand changes from idle to pending.
Later releases can coalesce into pending demand without another notification.

## Capacity and context

Count-capacity parking must occur before another body load.
Byte-capacity rejection must drop the loaded body before parking.
Both cases must preserve the unfinished cursor and sticky pass failure.
The driver must define explicit resume events and a retry deadline.
Page continuation and rejected reservation cleanup are not external demand.
The composed model records each scanner body independently.
Selection consumes one pass visit and retains an independent outstanding-candidate witness.
A temporary rejection drops the body but retains the selected hash and that witness.
Reloading the same candidate consumes a page attempt, not another pass visit.
Completion requires no outstanding candidate.
Each load records the queue count at its own boundary.
The startup presence phase prevents ordinary body loading.
The model checks retained bodies at both the endpoint-release phase and the waiting phase.

Each page has an allowance and an independent count of spent attempts.
Their sum equals the configured page size.
A synchronous endpoint release cannot refill that allowance.
A page yield or capacity wait permits refill after resumption.
An acknowledgment resumes the existing page without refill.
An acknowledgment error sets sticky failure without selecting another candidate.
Shared and successor demand remain unchanged during acknowledgment completion.

An ordinary body retains the context that authorized its load.
Later context replacement does not revoke ordinary block processing that already holds that context.
Startup completion and proposal authorization use their separate exact-context checks.
This distinction does not change block validity or introduce a consensus lock.

Context replacement invalidates stale startup completion and future proposal authorization.
It does not invalidate already queued or executing block ownership.
The model must allow those workers to retain their original contexts until release.
It must not clear newer shared demand, successor demand, or proposal offers.

## Proposal offers

An offer requires a complete, successful ordinary pass and optional proposal demand.
The service must move a pending offer into its active slot before awaiting a callback.
New offers can replace or coalesce pending work while the active callback waits.
The old callback's completion must change only its own active slot.

Authorization must check the exact current registration and the existing finalized-floor bond condition.
The service must release temporary snapshot and Casper ownership before invoking the generic proposer.
Cancellation after authorization cannot undo a request already accepted by the generic proposer.
The contract does not claim otherwise.

## Composed verification obligations

`RecoveryDispatcherComposition.tla` is the draft production-composition model.
Keep `RecoveryDispatcherOwnership.tla` as the smaller detached-worker regression.
The new model must separate supervisor state, worker handles, physical ownership, service ownership, sender borrows, recovery demand, context, and proposal offers.
Its initial finite configuration must permit at least two concurrent workers, both services, competing demand, and context replacement.
The current configuration has three jobs, three requests, three offers, and two startup captures.
The queue and worker limits are two.
Each pass has two visits.
All eight request-to-proposal flag assignments are initial states before symmetry reduction.
Three requests allow an active offer, a pending offer, and a replacement pending offer.

| Required invariant | Required unsafe control |
|---|---|
| Each live worker and service retains an owner. | Detach a worker or service. |
| Worker handles remain within the configured limit. | Ignore completed, unjoined handles. |
| Each retained payload remains charged. | Release its byte lease before destruction. |
| Each byte lease retains its admission identity. | Release an identity before its byte lease. |
| Release wakes follow payload, byte, and identity release. | Publish a release wake early. |
| Rejected admission cannot create retry demand. | Arm rejected reservation cleanup. |
| Waiting actors hold no strong queue sender. | Skip endpoint release before an await. |
| Shared and successor demand match their witnesses. | Discard a consumed wake result. |
| Old callback completion preserves newer pending offers. | Clear the pending slot on callback return. |
| Proposal authorization uses the current context. | Authorize an obsolete offer. |
| Stop completion requires actual child retirement. | Treat an abort request as retirement. |
| Parent destruction submits cancellation for live children. | Lose the worker abort request. |
| Pending capture destruction retains its role. | Reuse that role before destruction returns. |
| Count capacity is checked before body loading. | Load while the count queue is full. |
| Waiting recovery retains no scanner body. | Park before dropping the body. |
| The startup presence phase precedes ordinary loading. | Load during presence checks. |
| Each body load belongs to a selected candidate. | Load without selecting a candidate. |
| Temporary rejection retains the candidate obligation. | Drop its hash but retain its independent witness. |
| Pass completion requires no outstanding candidate. | Complete with a pending candidate. |
| Each load consumes one page-attempt permit. | Load without an attempt permit. |
| Page allowance plus spent attempts equals page size. | Refill without a suspension boundary. |
| Active retry deadlines preserve successor demand. | Create demand without a maintenance event. |
| Acknowledgment failure suppresses the pass proposal. | Ignore the error but retain its independent witness. |

Stop completion must require drained input and observed child retirement.
Empty physical ledgers must follow from those states and ownership invariants.
They must not serve as premises that manufacture the desired conclusion.

Rocq proofs must cover arbitrary finite ownership histories, release ordering, demand preservation, and bounded mailbox ownership.
Finite TLA+ checks must state their domains, scheduling assumptions, and exact negative-control failures.
Component algebra alone must not be described as full dispatcher refinement.

### Parameterized worker proof

`RecoveryWorkerLifecycle.v` keeps phase, payload, byte lease, identity lease, wake, liveness, handle, and cancellation state separate.
Its transition function changes the independent fields at their respective boundaries.
The proof then establishes the relationships between those fields.
It does not define physical ownership as a direct function of phase.

The proof covers arbitrary finite interleavings over a population indexed by natural numbers.
An operation on one worker preserves every other worker.
Every reachable payload retains its byte lease, and every byte lease retains its identity.
A release wake requires all three physical resources to have been released.
Parent cancellation preserves physical resources until cleanup executes.

Seventeen theorem checks and independent kernel validation passed in `worker.spicqK`.
The check used a 2 GiB memory limit with swap disabled.
Six cleanup service steps suffice for each admitted worker's abstract lifecycle.
This is a service-step bound, not a wall-clock shutdown bound.
Actual destructors must return, and the executor must service cancellation.

The worker proof does not prove dispatcher scheduling, the worker-count limit, the proposal mailbox, or scanner progress.
The composition model and production correspondence tests must establish those additional obligations.

### Model execution and evidence

Run `bash scripts/check-recovery-dispatcher-composition.sh syntax`, `controls`, or `safe` inside a memory-limited systemd scope.
The checker stores logs and source hashes under `target/verification/recovery-dispatcher/`.
The checker rejects a negative control unless its specified invariant fails with the expected checker exit code.
Parsing failure, resource termination, and incomplete search do not qualify a control.

`DispatcherPresenceControl.tla` gives a focused negative-control path through the same composition model.
It reuses the original transitions without changing their definitions or initial state.
It excludes unrelated worker, proposal, and shutdown actions from this existential counterexample search.
This restriction does not qualify positive safety.
Positive safety still uses the complete transition relation.

The focused control failed `Inv_NoPresenceBypass` in `presence.bV9cy3`.
TLC returned exit code 12 after 10,541 generated states and 3,512 distinct states.
The counterexample had depth 15, and the run took five seconds.
The input hash check passed.
The separate full-transition control run remains recorded independently.

Worker-count and proposal controls use subsets of the same production-model actions to remove unrelated branching.
Each resulting counterexample remains a behavior of the full transition relation.
These targeted negative searches do not establish positive safety.
The safe configuration retains the complete transition relation.

The candidate-retention revision passed inductive safety checks for the stated finite domains.
The acknowledgment refinement passed its base and all eight preservation groups in `run.RvSnIh` and `run.G6E490`.
The composition model does not yet have a temporal liveness proof.
It does not establish a bounded retry frequency.
Its ownership predicates do not establish that the complete dispatcher implementation refines the model.

### Inductive composition check

The typed wrapper `MC_RecoveryDispatcherComposition.tla` imports the original module with `INSTANCE`.
It does not copy or replace the transition rules.
Its constants preserve three jobs, three requests, three offers, two captures, and both concurrent services.

The candidate inductive invariant combines safety with exact lifecycle and ownership relationships.
Its groups cover worker ledgers, supervisor state, services, proposals, pass demand, and startup captures.
The base check proves that every original initial state satisfies that candidate.
The step checks start from any state that satisfies the complete candidate.
Each step check uses the complete `Next` relation and checks one invariant group.

All groups must pass with identical source hashes.
Together, the base and step checks can establish safety for every history length within the stated identity domains.
They do not establish arbitrary domain sizes or temporal liveness.
The separate Rocq proofs provide parameterized results only for their stated component contracts.

Run `symbolic-base` and `symbolic-step` with `scripts/check-recovery-dispatcher-composition.sh`.
The default `all` mode requires unsafe controls, worker and retry proofs, symbolic obligations, native integration, and native admission tests.
The separate `safe` mode retains full TLC state exploration.
No restricted negative-control schedule serves as a positive proof.

The original composition base check passed in `run.2I3Pgn`.
All eight preservation groups passed in `run.oKI2lQ` with identical model hashes.
Each group used the full transition relation and the complete inductive invariant as its precondition.

Two earlier preservation checks exposed missing inductive relationships.
Retired proposal services cannot retain a pending offer.
Service retirement also implies that supervisor shutdown has started.
The original transitions already enforce both relationships.
Adding those relationships to the invariant closed the proof gaps without changing production rules.

These results establish model safety for arbitrary history lengths within the configured finite identity sets.
They do not establish arbitrary population sizes, temporal progress, or production refinement by themselves.

## Production integration

### Retry refinement and native checks

The first production review found a missing candidate-retention boundary in the original model.
That model discarded the body without retaining a separate unfinished-candidate obligation.
Its successful checks therefore did not cover premature completion after temporary rejection.

The refined model separates candidate selection, body loading, rejection, acknowledgment, and completion.
Five named negative controls passed in `run.JVUZK1`.
Its initial invariant passed in `run.bGJMP1`.
All eight full-transition induction groups passed in `run.UFc4qp`.
The fixture retains three jobs and concurrent workers and services.
Its page size is two, while the production page size is 64.

`RecoveryRetryAccounting.v` supplies parameterized retry and page-budget proofs.
The candidate type and initial selection budget are arbitrary.
The page-size parameter is also arbitrary.
The proof covers finite operation histories, candidate retention, exact selection charges, completion, sticky failure, and page-load bounds.
An explicit empty visit covers cursor exhaustion after concurrent removal.
An empty visit followed by failure covers a pre-selection error.
These component results do not prove the entire dispatcher.

The production repair passed `integration-native` in `run.P7YwDy`.
That gate passed 17 dispatcher tests, seven actual resolver tests, 26 context tests, and strict lint.
The new cases include actual byte rejection, reload, concurrent removal, context replacement, retry errors, and generated histories.
Generated histories use 128 cases, initial budgets from zero through 127, and up to 511 operations per case.
Each operation checks candidate ownership, remaining selections, and proposal eligibility against independent expected state.
The test gate rejects empty or incorrectly filtered selections through exact test-count checks.

A second review found two formal correspondence gaps.
The original refinement lacked acknowledgment failure and empty cursor-selection actions.
The runtime already handled acknowledgment failure, but the model could not represent that path after the final candidate.
The latest model adds an independent error witness and `Acknowledge(error)`.
Its new negative control ignores that error and must violate `Inv_ErrorPreventsProposal`.
The refined base passed in `run.RvSnIh`.
All eight full-transition preservation groups passed in `run.G6E490`.
Two diagnostic counterexamples exposed missing inductive relationships, not reachable runtime failures.
Acknowledgment ownership excludes a new load permit and requires the acknowledgment wait reason.
An active attempt requires a spent page permit.
A retained body excludes an unused load permit.
The original actions enforce these relationships.

The corresponding native batch passed in `run.tJ6mlp`.
It passed 24 dispatcher tests, seven actual resolver tests, 26 context tests, and strict node lint.
Four new tests cover actual acknowledgment failure, demand during suspended acknowledgment, empty selection, and pre-selection failure.
The acknowledgment failure test poisons the actual request-tracker lock and checks proposal suppression after the final candidate.
The suspended acknowledgment test invokes the actual tracker after an explicit suspension boundary.
It checks that proposal-bearing successor demand survives acknowledgment completion.
These tests do not establish a full blocked-proposer and worker-page integration trace.

### Input-closure liveness

`RecoveryInputClosure.tla` models closure observation without assuming that workers return.
It separates external senders, temporary sender borrows, queued jobs, active workers, payload ownership, and a persistent probe deadline.
The finite configuration has three jobs, two workers, two queue slots, and two temporary borrowers.
The safe check explored all 2,324 reachable states in `closure.9stNG2`.
It checked both safety and eventual observation of closed, empty input.

The temporal result assumes weak fairness for the timer and its probe.
Once no strong sender remains, weak upgrades cannot recreate a strong sender.
The empty queue then remains empty.
The persistent probe can observe closure even when all workers remain suspended.
Unrelated events do not reset its deadline.

Three controls distinguish the required behavior.
Requiring a free worker slot violates eventual closure observation.
Resetting the deadline after unrelated events also violates that property.
Dequeuing during a full-capacity probe violates the independent no-extra-body invariant.
Temporal controls require the checker's temporal-failure exit code.
The dequeue control requires its named invariant failure.

The production probe checks sender count before queue emptiness.
It uses a persistent supervisor-owned interval with missed-tick skipping.
The probe does not retain a sender, dequeue a body, or depend on recovery or proposal completion.
The queue-empty condition preserves the existing drain-before-EOF policy.
Closed input with buffered jobs and permanently blocked workers remains outside this closure-observation guarantee.

This result does not prove eventual release of temporary sender borrows or eventual child retirement.
It does not establish a wall-clock shutdown bound.
Cancellation still requires executor service, and synchronous destructors must return.
Three native regressions passed in `run.ASih3C` and again in `run.tJ6mlp`.
They cover full worker occupancy, the last temporary sender, and closed input with buffered work.
The occupancy fixture holds two synchronous payload destructors while other executor threads can service the supervisor.
It does not model two suspended asynchronous replay operations.
The tests distinguish prompt closure observation from subsequent destructor release and child joining.

### Runtime ownership

`NodeRuntime` now awaits `BlockProcessorInstance::run` through its existing critical-task set.
The dispatcher owns separate `JoinSet` values for block workers and the two recovery services.
Its worker limit counts completed, unjoined handles.
Its stop guard exists before the returned future receives its first poll.

Workers keep the queue item intact through validation.
The validation helper borrows the item's Casper and block.
The item releases its context and body before its byte lease, identity, and release wake.
Workers retain no queue sender and perform no retry scan or proposal callback.
The old result channel is removed because its only production consumer discarded every result.
Block status and processing errors remain in structured logs.

The recovery driver uses pages of at most 64 control steps.
Ordinary candidate visits use the durable buffer's existing rotating index.
The one-second retry timer uses missed-tick skipping.
The timer is a local scheduling parameter, not a consensus parameter or a wall-clock progress guarantee.
Other wake sources share the same driver.

| Production boundary | Preserved or strengthened contract |
|---|---|
| `MultiParentCasper::prepare_retry_candidate` | Checks live membership, certificate waits, and admitted metadata before loading one body. Dependency-read errors remain errors. |
| `MultiParentCasper::prepare_startup_candidate` | Loads the captured member before admitted-metadata reconciliation. It leaves dependency checks to the worker. |
| `StartupPass::step` | Completes every presence check before admission. Temporary capacity failure retains one pending hash, not its body. |
| `OrdinaryPass::step` | Checks count capacity before selection or loading. A failed stage suppresses that pass's optional proposal. |
| `StartupContext::context` | Resolves only the exact current registration, even when the same Casper allocation was republished. |
| `StartupContext::is_current` | Establishes the final authorization point after temporary snapshot and Casper ownership have ended. |
| `recovery_driver::propose` | Keeps an authorized callback active while a watch channel retains newer pending demand. |
| `transition_to_running` | Supplies initialization with the handle from the same prepared registration used for engine publication. |

Startup preserves the complete presence barrier and fatal body-read and acknowledgment errors.
It does not inherit ordinary dependency prechecks or post-block proposal conditions.
Quarantine deferral is the separately approved eligibility repair.
Stale startup cleanup remains nonfatal, but tracker cleanup now requires successful buffer removal.
This order preserves unresolved tracking when storage removal fails.

The driver releases its temporary sender and Casper before acknowledgment, page yield, or capacity wait.
An ordinary pass and a pending proposal retain only registration handles with weak ownership.
An active startup pass retains its separately charged snapshot lease.
Shutdown stops controls, cancels both child sets, discards queued items, and joins child retirement before returning.
Parent cancellation requests child cancellation but cannot establish immediate destruction of non-yielding work.

The focused `integration-native` gate covers dispatcher ownership, actual resolver behavior, registration tests, and strict node lint.
It writes logs and input hashes under the same evidence directory.
The candidate-retention revision passed this gate in `run.P7YwDy`.
Later changes require fresh evidence for their affected inputs.

## Production regression obligations

Tests must call the actual dispatcher implementation, not only queue or signal substitutes.
The required cases are:

- A blocked proposal callback while two workers and recovery pages make progress.
- Parent cancellation before child polling and during worker execution.
- Delayed payload destruction with admission ownership and wake inspection.
- External sender closure during recovery waits and page yields.
- Repeated capacity rejection without new retry demand.
- Proposal-bearing demand during a capacity wait.
- Context replacement with an active callback and a pending offer.
- Delayed startup-root retirement during shutdown.
- Queue destruction and child draining before orderly stop completion.

Each production result must identify its source revision, input hashes, command, limits, and covered invariant.
These tests do not replace replay, multi-node integration, or the final 24-hour soak.
