# Buffer publication and retry ownership

## Scope and status

This document specifies retry ownership before and after the first durable buffer write.
It extends the [pruning preservation analysis](buffer-pruning-preservation.md).
The approved extension specifies local pending-policy storage. It does not change Casper consensus rules or approve the separate pruning architecture.

The TLA+ checks described below completed before production repairs.
The Rocq proof compiled and passed a separate kernel check.
The worker regression failed its ownership assertion, while the successful-publication control passed.
The capacity refinement adds a second failing worker case and a second passing control.
Receipt-policy property tests also reproduce counter and quarantine resets.
Those reproductions preceded the production repair.
The earlier local handoff implementation lost pending provenance.
Actual-worker regressions reproduced lost old-block eligibility after durable publication.
The diagnostic model also rejects retention alone as a complete repair.
The [execution record](../../../work-logs/task-pr216-buffer-publication-2026-09-08.md) identifies the exact artifacts and remaining verification requirements.

### September 9 integration status

Paired pending publication now preserves provenance without retaining each pending identity in the active tracker.
The worker distinguishes pending ownership, terminal DAG admission, and missing ownership.
Pending ownership does not authorize terminal cleanup.
The processor uses the retriever's buffer instance, and restarted retrievers load the committed pending policy.

The worker integration runs passed forty-two retriever tests, eighteen owner tests, thirteen transport-policy tests, and thirty-six node worker tests.
These runs include the three pending-provenance failures, three stale-completion failures, and the untracked-capacity-dispatch failure.
The work log identifies each captured source snapshot and the applicable lint results.

The subsequent retry-selection run passed forty-eight retriever tests, twenty owner tests, fourteen transport-policy tests, and strict Casper test lint.
The next section describes that correction and its formal limits.

These results do not establish whole-branch qualification or complete upstream retry-policy equivalence.
Quarantine budget renewal still requires upstream alignment.
The existing component proofs do not establish preservation against independent pruning or composition of every production lock.
The historical analyses below explain why the earlier abstractions and repairs were insufficient.

## Upstream retry selection

Retry selection chooses a waiting peer, a known peer, or a broadcast.
Pinned `dev` uses a 500 ms unresolved-request base with its existing adaptive multiplier.
The configured received-entry expiry does not control that retry clock.
Equality at the retry interval does not permit a retry.

Upstream selection advances the request timestamp before cooldown suppression.
Known-peer selection also advances the cursor before the peer-budget check.
Thus, peer-budget exhaustion can select broadcast after the cursor advances.
Suppression preserves those selection updates but does not change cooldown timestamps or retry counts.

The owner API separates three results:

| Result | Published state | Operation ownership |
|---|---|---|
| No selection | No candidate changes | Release the temporary permit |
| Suppressed selection | Commit only the selected timestamp and cursor | Release the temporary permit without transport or completion |
| Dispatch | Commit selection and cooldown changes | Retain the original owner and permit through transport and completion persistence |

Pending policy uses the existing checked revision and atomic store operation.
A failed selection write must not publish memory changes or successful-selection metrics.
The existing registry and owner locks protect selection publication. No lock spans network work.
Operation-capacity refusal can prevent selection before this boundary. The model does not prove upstream equivalence for that capacity case.
No-selection preservation concerns selector mutations. Earlier budget and quarantine transitions remain separate.

Action metrics record successfully published selections before transport.
Suppression adds its reason metric without a completion metric.
The completion metric records a returned action before fallible completion persistence.
A persistence retry emits neither metric again. Cancellation before a complete return emits no completion metric.
Recovery that appends peers without transport remains a counted action, as in upstream.

[`RetrySelectionPolicy.tla`](../../../../formal/tlaplus/block_admission/RetrySelectionPolicy.tla) checks this boundary with concurrent operations and request replacement.
Its two-operation configurations use two request incarnations and fixed peer membership.
Safe configurations cover durable owners, volatile owners, and no known peers.
The independent expected policy uses the current reference state, not the captured candidate snapshot.

| Invariant | Native correspondence |
|---|---|
| `Inv_ClockEquivalent` | Fixed-clock regression and generated adaptive-ladder boundary checks |
| `Inv_NoStalePublication` | Exact owner identity, checked revisions, and stale-owner regressions |
| `Inv_PolicyCorrespondence` | Generated cursor histories, peer-budget fallback, and no-selection tests |
| `Inv_PairedControl` | Durable-policy equality after successful selection and failed-publication checks |
| `Inv_OperationOwnership` | Suppression releases capacity without a retry charge |
| `Inv_MetricBoundary` | Selection before transport, cancellation, and failed-completion-persistence tests |

Ten unsafe controls cover wrong timing, discarded suppression updates, skipped cursor advancement, partial writes, retained permits, metric errors, and stale publication.
The stale-publication controls separately remove revision and incarnation checks.
The initial model run passed its safe configurations but exposed a checker-interface issue in the constant clock invariant.
The state invariant corrected that issue. The subsequent strengthened gate passed all controls before the production edit.

The model separates capture from publication as a conservative abstraction.
Production normally excludes those stale selection snapshots through its locks.
This abstraction does not prove fairness or progress.
The model cursor is a selected-peer index modulo peer count. Native properties check the production wrapping `u32` cursor separately.
The finite clock witness covers a normalized base. Native properties check the complete adaptive arithmetic and integer boundaries.
This model does not cover peer-membership changes, pending-handoff transitions, restart, or quarantine renewal.
Those operations retain their separate proof and integration obligations.

## Quarantine renewal: repair plan

The storage entry and renewal primitives are implemented and have focused verification evidence.
The complete owner and maintenance integration remains unfinished.
Pinned `dev` retires active retry tracking when a request exhausts its budget.
It retains the spent total and starts a ten-second quarantine.
Retirement clears peer-requery counts and request cooldown tracking.

Deadline expiry and budget renewal are different events.
The maintenance sweep clears expired quarantine entries and their spent totals.
The sweep uses the timestamp captured at maintenance entry.
It does not send a probe. Later recitation can create fresh retry scheduling.
Recitation after expiry but before the sweep can send an initial request without resetting the spent total.

The feature currently retains the spent total and permits an autonomous probe after expiry.
The original native regression reproduced that difference before repair.
Its first assertion observed one request where the upstream sweep requires zero.
Four separate tests now check volatile and durable owners, with independent assertions for probe behavior and budget reset.
Their names start with `quarantine_expiry_` in the retriever publication tests.

### Checked transition

Keep the canonical owner for the unresolved block obligation.
Use existing operation tokens to distinguish completions from the expired budget cycle.
Do not add a persisted cycle field unless implementation evidence requires one.

1. Capture the maintenance timestamp before the sweep.
2. Require an existing quarantine deadline at or before that timestamp.
3. Acquire the registry, owner, and operation locks in their existing order.
4. Compare the committed policy with the expected revision in one atomic storage operation.
5. Preserve dependency rows, provenance, and obligation identity.
6. Advance the revision, set total retry attempts to zero, and clear the quarantine deadline.
7. After confirmed commit, invalidate old-cycle tokens under the owner lock.
8. Release cancelled deferred tokens without another policy write.
9. Permit fresh scheduling only through the applicable upstream recitation path.

Ordinary policy updates must still reject counter decreases.
Only the checked renewal operation can reset a spent budget.
Renewal preserves every other policy field, including peer counters, cooldowns, timestamps, cursor, and provenance.
Quarantine entry clears peer state. Expiry must preserve any new scheduling state created by intervening recitation.
A completion can charge only its still-reserved token.
The normal `mark_committed` helper must not classify discarded old-cycle observations as persisted completions.

On failed comparison, preserve effective policy and tokens.
Effective policy can contain observed completions that storage has not yet committed.
Verify the storage error contract before implementation.
If an error can follow commit, reconcile the exact committed renewal revision before exposing either outcome.

### Quarantine entry and operation accounting

Quarantine entry retires the upstream peer schedule. Expiry renewal resets the total retry budget later.
The storage entry operation accepts the committed policy and its effective monotonic successor.
The effective successor can include observed completions whose earlier persistence failed.
The operation validates that successor before applying the peer-state reset.
One paired transaction preserves dependency bytes and commits the exact projected policy.
Ordinary updates still reject counter decreases.

The owner must confirm budget exhaustion and the absence of reserved operations before publishing entry.
Each reservation consumes one unit of the remaining retry allowance.
Completion transfers that unit from reserved work into the attempt count. Cancellation releases the reservation without charging an attempt.
These transitions imply that an exhausted budget has no outstanding reserved operation, provided all count increases use this boundary.
Expired autonomous probes violated that premise. The current integration removes them and follows dev's retirement and expiry sequence.

[`RetryQuarantineEntry.tla`](../../../../formal/tlaplus/block_admission/RetryQuarantineEntry.tla) checks this entry boundary before production integration.
Its two safe configurations use budgets one and two, with two and three immutable operation identities, respectively.
Completions, cancellations, schedule updates, persistence, and entry can interleave.
The model separates effective counts from stored counts. Entry must commit the effective total before resetting peer state.

| Invariant | Requirement | Native correspondence |
|---|---|---|
| `Inv_ReservationBound` | Completed attempts plus reservations cannot exceed the budget. | The last-reservation native case passes. Generated owner histories still need renewal coverage. |
| `Inv_Accounting` | Each completed operation contributes exactly one attempt. | Existing operation-owner tests check charging and cancellation. |
| `Inv_EntryProjection` | Entry preserves the total and provenance, clears peer state, and commits the effective policy. | The volatile and durable owner quarantine tests pass. Ordinary provenance retires, while durable pending provenance remains. |
| `Inv_FailedEntry` | A confirmed failed entry changes no modeled state. | Injected transaction failure preserves both stores. Owner-phase integration remains required. |
| `Inv_StoredBound` | Stored counts do not exceed effective counts. | Generated storage histories check exact policy values across entry, update, renewal, failure, and reopening. |

The safe searches covered 732 and 7,956 distinct states, at depths ten and twelve.
Twelve unsafe controls produced their expected invariant failures.
Schedule fields abstract cursor and cooldown values as untouched or touched. Native properties exercise full-width values.
The model covers one hash and one entry, not an entire renewed lifecycle or validator protocol.
The failure action represents a confirmed atomic failure. It does not replace the separate unconfirmed-commit model or native recovery obligations.
The model does not include revision comparison, parent-row identity, deadline calculation, owner replacement, or active-slot retirement.
Its initial state excludes restored over-budget policies and legacy expired probes.
Stored totals represent deferred completion, but the flush action does not distinguish individual observed and persisted token phases.
Those boundaries require the separate models and actual owner integration tests. Passing component checks do not establish their composition.

### Qualification and unresolved bounds

Model expiry, sweep completion, and recitation as separate transitions.
Include concurrent renewal, transport completion, deferred persistence, restart, and independent hashes.
Add separate unsafe controls for deadline checking, sweep ordering, revision checking, provenance preservation, token invalidation, and failure atomicity.
Run these checks before production changes.

Native and generated histories must cover each transition and each failure boundary.
Include recitation on both sides of the sweep, failed comparison, old reserved tokens, old observed tokens, and renewal isolation between hashes.
Test capacity refusal without deletion of existing obligations.

A weak owner reference cannot preserve a nonpersistent exhausted request by itself.
The user's upstream-authority rule selects upstream retirement for ordinary transport requests.
No separate defect evidence currently justifies blanket volatile-provenance retention after retirement.
The volatile expiry test's provenance assertion expresses a branch preference, not proof of such a defect.

An absent buffer row does not always identify transport-only work.
A failed first publication can leave an already stored block with the retriever as its last retry owner.
The actual-worker regression proves that handoff requirement, but not retention across later quarantine retirement.
That interaction needs a regression before the implementation selects a retention exception.
Do not classify all absent-row requests as transport-only or preserve all volatile owners without evidence.

The actual-worker retirement fixture now gates publication through the storage interface and drives thirty-two completed public recovery actions.
Its next recovery call reaches quarantine entry without counter or timestamp mutation.
The ownership assertion accepts a worker lease, complete pending ownership, or an existing retry owner.
The fixture then releases the storage failure and supplies a response if the worker completed without publication.
It requires complete pending publication and release of worker identity and bytes.
This fixture does not prove autonomous recovery. It assumes no backward system-clock change during the configured response intervals.
It must run again against the actual upstream-retirement integration. A pass with the current retained-owner implementation does not justify blanket retention.

Durable pending owners require a bounded maintenance traversal. Cold lookup must not reset their budgets opportunistically.
The traversal component now has independent recovery and expiry orders in one membership map.
Each expiry batch contains at most 64 entries. The helper does not hold its locks during later policy work.
The complete caller must serialize expiry passes, capture the initial membership count, and process every extracted batch before success.
The caller must retain the timestamp captured at maintenance entry.
The caller must yield between batches without changing recovery order.
These caller requirements and custody integration remain incomplete.

`BufferIndependentExpiry.tla` checks separate selection and processing with concurrent recovery, insertion, removal, and duplicate insertion.
The bounded safe configuration checks safety and pass completion under explicit weak fairness.
Its seven unsafe controls check order interference, overtaking, stale membership, skipped processing, early return, and overlapping passes.
Removal followed by reinsertion counts as a new membership interval.
The coverage invariant applies to initial members that remain present continuously.

| Formal obligation | Native coverage | Remaining boundary |
|---|---|---|
| Exact membership and independent order | Two reference queues, generated operations, bidirectional link checks, and concurrent helper tests | Caller selection and renewal composition |
| Coverage of continuously present initial members | Generated arrivals, removals, duplicate insertion, and competing recovery selections | Complete pass under storage errors |
| Bounded batch and complete return | Storage batch regression and modeled processing actions | Serialized asynchronous caller and complete batch processing |
| Pass completion | Bounded temporal model with weak fairness | Scheduler service and storage completion in production |

The extra queue links require memory proportional to candidate count. Temporary batch memory has a fixed upper bound.
This implementation does not settle the separate persistent-pruning architecture review.

### Accepted-block evidence through transport retirement

Transport provenance belongs to a request schedule. Accepted publication responsibility belongs to the bounded worker lease.
The worker captures provenance with the same lookup that establishes interest.
Private-field evidence binds the exact block hash and the same Casper allocation.
Successful format, signature, content-identity, and storage checks must precede its use for publication repair.
The evidence authorizes only restoration of pending ownership. It does not establish validation success or settled admission.
Captured provenance must accompany the first successful pending publication, not only a retry after publication failure.
Transport retirement can occur between the interest decision and that first publication.
The first-publication regression removes tracking during body storage and supplies no second delivery.
It failed before the evidence transfer and passed afterward.
This reproduces request cleanup, not the complete budget-retirement transition.

The worker keeps accepted evidence outside the error classification.
After an accepted publication failure, the worker retries stored publication with that evidence and retains its existing queue lease.
The worker does not repeat the interest decision or transfer accepted responsibility to volatile request tracking alone.
Terminal admission or successful paired publication ends that retry.

Stored restoration has separate preparation and publication steps.
Preparation verifies the stored body and captures its current provenance at the existing permitted restoration boundary.
The worker records the evidence before dependency reconstruction or publication can fail.
This path uses the stored body, not a conflicting incoming payload.
Missing or corrupted storage after acceptance produces an error. It does not discard accepted responsibility.

The evidence has no validation or settled-admission authority.
Existing validation, finality, and settled-history decisions remain unchanged.
Valid format and signature without valid content identity produce no accepted evidence.
That distinction preserves the existing well-formedness result while restricting the new publication evidence.

Task cancellation and process death remain outside the verified custody boundary.
Current shutdown aborts workers. In-memory evidence alone cannot prove durable accepted-work recovery before pending publication succeeds.

Publication now merges supplied captured provenance into an existing owner's candidate policy.
The candidate uses logical OR, so neither false input nor later publication removes established provenance.
The paired storage transaction commits that candidate with its dependency row.
Failure leaves the local owner unchanged. Native generated histories check successful merges, conflicts, and reopening.

`AcceptedPublicationCustody.tla` checks the intended worker boundary with two concurrent identities and two contexts.
It distinguishes provisional evidence from successful identity verification and storage.
Ten unsafe controls test lost custody, incorrect provenance, skipped identity checks, incorrect identity or context, terminal republication, and unverified stored restoration.
The metadata-loss control distinguishes terminal membership from the availability of its persisted metadata row.
Loss of that row must not remove terminal membership or authorize pending publication.
The safe model requires atomic exclusion between terminal admission and pending publication.
The publication path now holds the DAG read guard through its current admission check and synchronous paired publication.
The actual restoration regression demonstrates the previous missing boundary.
This model does not prove complete worker liveness, crash recovery, or pre-store failure handling.
Its initial restorable set represents previously verified stored bodies at an existing restoration entry point.
The old-block interest invariant applies to incoming acceptance, not that separate stored-restoration path.
Both paths still require verified identity, stored content, worker custody, and terminal exclusion.

| Invariant | Required implementation boundary | Current evidence |
|---|---|---|
| Verified identity | Activate captured evidence only after successful identity checks and storage | Private construction, verified stored preparation, and native rejection tests pass. |
| Custody | Retain the worker lease until terminal admission or complete pending publication | Actual-worker failure and retry-exhaustion tests pass without manual redelivery. Shutdown and retirement integration remain required. |
| Captured provenance | Preserve the original dependency decision through schedule retirement | Atomic owner merge and first-publication cleanup regression pass. Ordinary transport retirement remains incomplete. |
| Exact hash and context | Reject evidence for another block or Casper context | Model, explicit identity checks, and exhaustive input combinations followed by generated binding histories pass. |
| Terminal exclusion | Do not publish pending ownership after terminal admission, including metadata loss | Native race and missing-metadata regressions pass. Generated storage histories, four Loom tests, and focused strict lint pass. |

### Terminal admission and pending publication

The original restoration path checked DAG membership before reading the stored block and reconstructing dependencies.
Another admission could commit before the later pending publication.
The worker lease did not exclude direct certified-admission paths.
The deterministic regression inserts certified admission during the stored-block read, then checks that no pending row remains.
The unguarded implementation failed that assertion.
The first fixture attempt used an inconsistent sender-authority certificate. That fixture failure did not demonstrate the race.

The correction uses the existing DAG read lock. Other publication readers can retain their concurrency.
Terminal admission uses the existing DAG write lock.
The lock order is DAG, request registry, request owner, then buffer.
The callback must not acquire the DAG lock again or perform asynchronous work.
Dependency reconstruction occurs before the guarded publication call.

```text
reconstruct dependencies
acquire DAG read guard
read current DAG membership
if a member:
    validate persisted metadata or return its error
    return terminal without publishing
otherwise:
    publish dependency row and request policy synchronously
release DAG read guard
if terminal:
    remove obsolete request tracking
```

Terminal cleanup occurs after the DAG guard is released.
A cleanup failure returns an error. It does not authorize another pending publication.
The shared guard-lifetime helper has property tests for read errors, publication errors, early return, and guard release.
Loom exercises two concurrent publications against terminal admission through that production helper.
The unsafe control releases the read guard before publication and reproduces terminal pending ownership.

Review found that the initial guard reused the readiness predicate incorrectly.
Readiness returns false when terminal membership exists but its metadata row is missing.
Publication must instead return an error without invoking the publication callback.
The missing-metadata regression failed before this correction and passed afterward.
The readiness predicate remains unchanged.

Native tests enumerate membership, valid rows, missing rows, malformed rows, mismatched keys, and publication failures.
Generated histories repeat these transitions and check callback exclusion, exact missing-row errors, publication errors, and guard release.
The updated TLA+ model permits metadata loss after terminal admission.
Its safe configuration passed 33,728 distinct states at depth fifteen before the production predicate changed.
All nine unsafe controls failed at their expected invariants.
This finite model does not prove recovery from physical corruption.

This native reproduction uses the feature branch's restoration entry point.
Pinned upstream does not contain that entry point. These results do not establish an identical upstream execution trace.
The correction changes local publication ordering, not block validity, vote weighting, or finality rules.
The formal context check binds a fixed worker context. It does not model replacement of a live Casper context.
These repairs do not authorize a new pruning architecture, retry limit, quarantine duration, or admission rule.

### Retry-budget custody

Pinned dev separates a retired request's retry budget from its transport schedule and dependency provenance.
The local repair must preserve that distinction when pending publication transfers responsibility to storage.
An accepted worker retains its captured publication evidence separately. That evidence does not keep obsolete transport provenance alive.

`RetryBudgetCustody.tla` models one authoritative budget location per hash: volatile owner, paired pending policy, or retired budget record.
The pending location represents authoritative durable accounting, even when an active execution cache refers to that policy.
An atomic transfer moves the attempt total without duplication. Its quarantine deadline must also remain unchanged.
Successful publication combines captured provenance with existing pending provenance. Failed publication leaves the modeled custody and policy fields unchanged.
Capacity refusal cannot discard a retired budget.

The model distinguishes active dispatch eligibility from retained schedule data.
Publication can leave dormant schedule data while removing active eligibility. This does not establish physical cleanup of the production request data.
Ordinary retirement removes the active schedule and provenance. Pending retirement preserves required pending provenance.
Expiry clears the budget and deadline without dispatching a request or clearing a newly created active schedule.
An ordinary recitation after the deadline can precede the expiry sweep and carry the spent budget until renewal.

| Invariant | Required implementation check | Native correspondence |
|---|---|---|
| `Inv_SingleBudget`, `Inv_Budget` | Move the attempt total between authoritative locations without duplication or loss. | Generated repeated retirement histories check actual registry transfers and renewal. Capacity-refusal tests preserve the retired budget. |
| `Inv_Deadline` | Preserve the deadline across transfers. Only quarantine entry and expiry replace it. | Successful and conflicting retired-transfer tests check both total and deadline. |
| `Inv_Retired` | Remove ordinary transport activity, schedule, and provenance at retirement. | Both `quarantine_retirement_*` regressions pass after the production repair. |
| `Inv_PendingProvenance` | Preserve required provenance on durable pending handoff. | Worker tests and repeated retirement histories preserve pending provenance. |
| `Inv_ActiveSchedule` | Do not let expiry erase a schedule created by later recitation. | `recitation_before_sweep_preserves_new_schedule_fields` checks both volatile and durable owners. |
| `Inv_Activation` | Do not activate transport before quarantine ends. | `quarantine_and_capacity_refusal_preserve_retired_budget` checks deadline eligibility and capacity refusal. Public refusal-result accuracy remains under review. |
| `Inv_NoProbe` | Do not let expiry itself dispatch another request. | The four expiry maintenance tests check request counts and renewed policy. |
| `Inv_Failure` | Leave modeled ownership and policy unchanged after a confirmed failed publication. | `failed_retired_budget_publication_preserves_budget_and_competing_row` preserves the retired record and competing transaction, then retries successfully. This is a confirmed conflict, not a fatal storage-error test. |
| `Inv_Ownership` | Require active requests to have live custody and retired budgets to have nonzero deadlines. | Registry history tests must check both structural conditions after every transition. |

The safe configuration checked 17,295 distinct states at depth 27 in `run.vNcEQc`, including the additional structural ownership invariant.
All twelve unsafe controls failed at their expected invariants. The gate verified captured source hashes and exited successfully.
The controls include duplicate budgets, provenance loss, retained schedules, early activation, failed transfers, expiry probes, capacity loss, and lost deadlines.

The configuration has two hashes, one active slot, a two-attempt budget, clock values zero through two, and at most four completed retries per hash.
It permits repeated retirement and renewal. These finite bounds do not establish unlimited-cycle behavior.
Each transfer represents one local registry transaction. Independent hash actions can interleave in every enabled order.
The model does not replace the reservation, deferred-completion, concurrent-renewal, or independent-expiry models.
Those models retain responsibility for operation phases, competing sweep timestamps, and batch traversal.
Native integration must establish the composition of these boundaries and reject obsolete owner handles.
This model assumes confirmed atomic publication failure. It does not establish recovery from an unconfirmed storage outcome or process death.

### Maintenance order and fresh request clocks

Pinned dev examines active requests before it sweeps expired quarantine records.
The pass captures its time before examination. Retirement uses the current retirement event's time for its new deadline.
An expired, recited schedule can still have a spent budget when maintenance begins.
If that request is due, maintenance must retire it again rather than first resetting its budget and dispatching another retry.
If the request is not due, the later expiry sweep can renew its budget while preserving its new schedule.

The first retirement integration reset budgets before active examination.
`expired_recited_due_schedule_retires_before_renewal` demonstrated one incorrect dispatch where dev's order requires retirement.
The corrected caller examines active owners, renews expired budgets, then completes metrics and certificate maintenance.
It retains the captured pass time for renewal. Errors do not bypass the remaining certificate maintenance call.

An inactive durable pending owner also needs a fresh transport clock when a new network recitation activates its schedule.
The durable obligation keeps its original identity timestamp, retry total, deadline, and provenance.
The new schedule sets `last_request_timestamp` before it becomes active or sends its initial request.
The update and active publication occur under the registry boundary. A failed policy write does not publish the new schedule.
Existing active receipt and local handoff preserve the current schedule, including active quarantine.
Neither local receipt nor local handoff creates an absent quarantined transport schedule.

`pending_recitation_starts_a_new_request_clock` demonstrated the old timestamp through the production `admit_hash` path.
The test preserves the obligation's original timestamp while requiring the new transport timestamp.
Both ordering regressions failed at their intended assertions in `retirement-order-red.v7tWJ8`, with unchanged captured input hashes.

The first `RetryMaintenanceOrder.tla` checked serialized selection before the production corrections.
It checked 5,303 distinct states at depth eleven in `run.WHo7N5` and five unsafe controls.
Plan review found two correspondence limits. Recitation always replaced the modeled owner, and due observation shared one action with selection.
Production permits the same durable owner to reactivate. Production also separates due observation from selection.

The revised model separates `Observe` from `Apply` and permits same-owner recitation between those actions.
`Observe` uses the pass time. `Apply` uses current ownership, budget, quarantine, and action time without another due check.
Expiry waits until all observed items finish. Initial budgets include available and exhausted states, so the dispatch path is reachable.
Plan review also corrected modeled dispatch to clear quarantine, as production already does before dispatch commitment.
Without that correction, modeled expiry could erase a newly completed retry charge.
`run.As2kQa` checked 425,572 distinct states at depth 21 and all five exact unsafe controls.
The gate also found the required counterexample to `Inv_CurrentDue` without enabling an unsafe implementation variant.
The gate verified source hashes and exited successfully.

That counterexample rejects an unsupported proof claim. It does not establish an unauthorized reason to change upstream retry policy.
Pinned dev also separates its due check at `block_retriever.rs:1000–1035` from subsequent retirement and selection.
Its current-entry selection at `:1223–1255` does not repeat the due check.
For example, recitation can refresh a timestamp after observation while an earlier due decision still permits retirement against the current exhausted budget.
The durable pending obligation survives that retirement. This repair does not introduce a new due-revalidation rule.

| Invariant | Meaning | Native regression |
|---|---|---|
| `Inv_Order` | Finish observations and their applications before expiry. | The due, spent recitation test rejects an automatic retry caused by early renewal. |
| `Inv_Decision` | Use the captured due result, current budget, and current quarantine at application. | `observed_retry_uses_current_budget_after_same_owner_recitation` checks available and exhausted budgets across actual publication and recitation. |
| `Inv_Snapshot` | Do not act through a captured owner identity after a different owner replaces it. | Exact-owner and replacement-completion histories remain required at each affected callback. |
| `Inv_Clock` | Publish a fresh request clock with the new schedule. | The production pending-recitation test checks the clock before later maintenance. |
| `Inv_CurrentDue` | Unsupported requirement that the action must remain due when applied. | The dedicated configuration must produce a counterexample. It is not a safe-case invariant. |

The model permits recitation and pending publication between pass steps.
It uses representative due and not-due relations. The selection model and native properties cover the actual backoff arithmetic.
Its two hashes, two generations, three clock values, and single pass are finite conformance checks, not an unlimited execution proof.
Retry completion is atomic here. This model does not establish correctness for overlapping maintenance passes, failed storage writes, or deferred completion.
The model excludes received-state changes and suppressed selections. Native same-owner tests cover both budget branches, not intervening completion between observation and application.
The custody, renewal, and traversal models supply separate component checks. Their composition still needs production regression and concurrency evidence.

### Local activation refusal results

Local receipt and local recovery distinguish `Tracked`, `Quarantined`, and `AtCapacity`.
The registry captures the outcome under its existing lock. Later expiry or capacity changes cannot change that result.
An existing active owner remains available during quarantine, including when the tracker is full.
An inactive quarantined owner returns `Quarantined` before the capacity check. An eligible inactive owner returns `AtCapacity` only when the tracker is full.
Storage errors remain errors. Neither refusal activates transport or resets the budget, deadline, or provenance.
Only `AtCapacity` increments the capacity-refusal metric. Repeated quarantine refusal does not represent a new quarantine entry.
The worker retains its lease for either refusal and uses the existing retry delay.

Both public regression tests failed at the incorrect `AtCapacity` result in `quarantine-result-red.wxm2Z4` before this repair.
`LocalRequestActivation.tla` then checked 58,968 states and six exact unsafe controls in `run.gXQl6e` before production changed.
The controls cover incorrect reasons, delayed reclassification, masked storage errors, refused-state mutation, incorrect metrics, and premature lease release.
The model has two hashes, one active slot, and three clock values. It models the classification boundary, not subsequent receipt writes or complete worker execution.

### Renewal model and storage preconditions

[`RetryBudgetRenewal.tla`](../../../../formal/tlaplus/block_admission/RetryBudgetRenewal.tla) models exhausted requests and bounded renewal cycles.
Two maintenance actors can capture competing policy revisions.
The clock distinguishes a sweep's start time from the time of publication.
Recitation can occur before or after the expiry sweep.

Operation tokens retain their hash and issued cycle once reserved.
New operations consume unused tokens. They do not overwrite cancelled token identities.
This permits a delayed old completion to coexist with a new reservation.
The reference count checks issued cycles independently of the implementation's phase check.

The durable reference tracks committed counts separately from implementation storage.
Restart loads implementation counts from storage and reference counts from the durable reference.
The volatile configuration keeps revision one and completes accounting immediately.
Its renewal guard checks the current deadline, not a nonexistent changing volatile revision.

| Invariant | Requirement | Corresponding native checks |
|---|---|---|
| `Inv_FreshRenewal` | Reject stale renewal publication | The storage test permits one concurrent winner. Owner-level histories remain required. |
| `Inv_SweepTime` | Use the captured sweep timestamp | Storage rejects time 99 for deadline 100. Maintenance capture still needs integration coverage. |
| `Inv_AtomicFailure` | Preserve all modeled owner and policy fields after a confirmed atomic failure | Failed reset must preserve effective observations and operation tokens |
| `Inv_NoAutonomousProbe` | Only recitation or reservation dispatches transport | Durable and volatile expiry regressions pass without autonomous transport. |
| `Inv_Provenance` | Preserve provenance and paired dependency rows | Storage renewal preserves complete rows and policy fields across reopening. |
| `Inv_CycleAccounting` | Count only completions from the current cycle | Old reserved and observed tokens must not charge a renewed budget |
| `Inv_CurrentTokens` | Invalidate pre-renewal live tokens | Cancelled-token completion needs a separate guard regression |
| `Inv_DurableAccounting` | Keep committed counts equal to the independent durable reference | Storage reopening passes. Deferred owner and callback restart checks remain required. |
| `Inv_StoredBound` | Do not persist more attempts than effective accounting contains | Deferred-write and reset histories remain required |
| `Inv_UnconfirmedOwner` | Preserve effective ownership after an unconfirmed renewal write | Native fatal-error recovery remains required. |

The model gate includes seven safe configurations and thirteen separate unsafe controls.
The configurations vary persistence, hash count, operation count, cycle count, and unconfirmed commit outcomes.
The repeated-cycle configuration permits two cycles and three immutable operation tokens for one hash.
The largest search permits two hashes, two operation tokens, and an unconfirmed commit.
Persistent configurations permit one restart. All configurations use a normalized one-attempt budget.
These bounds do not establish unbounded-cycle liveness, arbitrary capacity, sweep traversal, or complete upstream lifecycle equivalence.

The initial state can combine an exhausted count with reserved operations.
This is a conservative abstraction. Production reservation guards may make that combination unreachable at quarantine entry.
The model does not yet represent peer cooldown fields or prove that production reachability condition.
Its storage cycle is reference history, not a proposed persisted field.

The failed-renewal action assumes a confirmed atomic failure, such as a failed compare-and-swap comparison.
It does not represent an arbitrary fatal storage error.
The memory backend validates every mutation before publication and has no fallible publication step.
The LMDB wrapper returns no error after `writer.commit()` reports success.
Existing fault hooks inject precommit failures or terminate the process after commit.

The unconfirmed-commit action permits either the old or new paired policy in storage.
The vendored LMDB metadata-write path attempts restoration after an I/O error but does not verify that restoration succeeded.
Therefore, a generic returned error does not prove that storage retained the prior policy.
An unconfirmed renewal must retain the spent effective budget and existing tokens without authorizing fresh-budget work.
The model blocks further durable mutations after this error. It does not assume that every local reservation needs a storage write.
Restart assumes a valid reopened database and cancels old callback identities.
Native integration must establish this recovery boundary before the renewal repair can claim complete storage-boundary correctness.

## Exact dependency-row guard

The pending-policy transaction compares both the dependency row and the policy record before publication.
A dependency row contains a serialized `HashSet`. Decoding can change iteration order without changing its parent set.
Re-encoding that set can produce different bytes. Comparing those new bytes against unchanged storage can cause a false transaction conflict.

This defect belongs to the branch's pending-policy extension. The repair does not change upstream Casper rules or the storage format.
The guard now captures exact stored bytes, validates those bytes through the existing decoder, and uses them unchanged in the paired transaction.
The backend read callback finishes before the write transaction starts.
The policy comparison remains unchanged. Missing and malformed dependency rows must still reject publication.

[`PendingPolicyRowGuard.tla`](../../../../formal/tlaplus/block_admission/PendingPolicyRowGuard.tla) specifies the independent acceptance condition.
An existing row permits the row guard exactly when its current bytes equal the captured bytes.
The model includes equivalent reordered rows, a duplicate-member representation, a changed parent set, and removal.
Two readers overlap with up to two replacements. The safe search checked 942 distinct states at depth seven.
The re-encoding control failed `Inv_ExactRowGuard`, as expected.

The deterministic native regressions use duplicate-member bytes that the existing decoder accepts as the original set.
These bytes are decoder-accepted storage, not ordinary writer output.
Both update and renewal failed before the repair. Both passed afterward, alongside the distinct-parent production-path tests.
Generated representations and controlled concurrent replacement provide additional correspondence checks.
The work log records each completed run separately from tests that are not yet verified.

An ABA change deletes a row and reinserts identical bytes before the comparison.
Raw-byte comparison cannot identify that intervening change. This repair does not establish generation-based ownership for dependency rows.

## Why the earlier abstraction was insufficient

The earlier pruning model combines receipt and durable publication in one `Buffer` action.
Its acknowledgement action requires an existing row.
That model cannot represent failure before the first row exists.

The actual worker stores a block body before publishing its buffer dependencies.
Its dependency-error handler calls `ack_processed`.
That call removes the request-tracker entry.
The outer worker records a validation failure and releases the queued item.
Failure accounting does not create a missing durable retry row.

This source path exists in both the feature branch and current upstream `dev`.
Source correspondence does not establish a complete upstream runtime reproduction.
The native regression must demonstrate the complete feature-worker failure before its repair.

## Pending provenance: unresolved admission boundary

**Request provenance** records that this node explicitly requested a block to satisfy a dependency.
The existing `requested_as_dependency` field carries that fact.
`check_if_of_interest` uses it to accept solicited history below the approved block height.
The field does not replace signature checks, sender authority, or block validation.

A durable buffer row records dependencies, not request provenance.
The earlier ownership models treated that row as sufficient for retry.
They omitted the admission predicate that a later retry must satisfy.
Therefore, their ownership theorems do not prove continued eligibility for solicited history.

Two original worker regressions exercise that omitted boundary:

- `publication_pending_solicited_history_preserves_old_block_eligibility` exercises ordinary missing-dependency publication.
- `publication_quarantined_solicited_history_preserves_old_block_eligibility` exercises repair during validation quarantine.

Both tests start with a signed block below the approved height.
The initial interest check rejects that block before a dependency request exists.
The real request API establishes provenance, and the interest check then accepts the block.
The real worker stores the block and publishes its complete dependency row.
Before repair, both tests failed because the next interest check rejected the still-pending block after request cleanup.

Current upstream `dev` at `cdf447ac18710d9702a27379bce6c946f421be46` has the same ordinary pending-acknowledgement pattern.
Its old-block predicate reads request provenance, and `ack_in_casper` deletes that provenance during nonterminal handoffs.
The feature worker also added an explicit cleanup after quarantine repair.
That new cleanup has the same defect.
This attribution comes from source comparison, not a full upstream node reproduction.

### Why retention alone is insufficient

The existing request fields can represent a pending handoff with `received=true` and `in_casper_buffer=true`.
That change can preserve provenance, retry budgets, original age, cooldowns, and quarantine during one process lifetime.
It also keeps each pending block inside the bounded request tracker.

Consider a tracker that contains a chain of pending blocks.
Earlier blocks depend on already-requested blocks that remain incomplete.
The chain's tail needs one more ancestor that the node has not requested.
The tracker cannot admit that ancestor when all slots are occupied.
For any finite slot count, a longer unresolved dependency chain can reproduce this condition.
Increasing the fixed limit or reserving a fixed number of slots does not prove progress for arbitrary chains.

The production tracker permits 2,048 entries.
A chain of 2,048 pending blocks and one missing ancestor needs approximately 2,049 buffer nodes.
That chain fits below the default 16,384-node buffer pressure limit.
Thus, the buffer limit does not exclude this counterexample to the proposed retention-only change.

This prefix assumes no applicable settled-history shortcut or helpful unsolicited delivery.
It also assumes that time-to-live pruning does not remove rows during the prefix.
Only the smaller model trace ran.
This analysis is not a native 2,049-block reproduction or proof of a current-runtime deadlock.

Restart poses a separate problem.
Request state is volatile, but buffer rows survive restart.
Retaining request entries cannot preserve provenance through restart.
Inferring authority from a persisted row requires a separately justified admission rule.

### Diagnostic model and limits

[`BufferPendingProvenance.tla`](../../../../formal/tlaplus/block_admission/BufferPendingProvenance.tla) separates request state, received bodies, pending rows, solicited history, and terminal completion.
Its dependency chain contains three distinct block identities.
The model permits request, receipt, publication, completion, and optional restart transitions.
It is a minimal counterexample model, not a full validator-network model.

| Configuration | Parameters | Required outcome |
|---|---|---|
| Default | Three keys, three request slots, retain pending provenance, no restart | All listed invariants pass. |
| `DeleteUnsafe` | Three keys, three slots, delete pending provenance | `Inv_PendingEligibility` fails. |
| `CapacityUnsafe` | Three keys, two slots, retain pending provenance | `Inv_DependencyProgressAvailable` fails. |
| `RestartUnsafe` | Three keys, three slots, retain pending provenance, permit restart | `Inv_PendingEligibility` fails. |

The default configuration checked 11 distinct states through depth 11.
The three controls failed at depths three, six, and four, respectively.
`Inv_DependencyProgressAvailable` checks whether a useful transition remains enabled.
It is not an eventual-delivery or consensus-finality theorem.
The passing configuration assumes that the complete dependency chain fits in memory and that the process does not restart.
Those restrictions prevent any claim of end-to-end production correctness.

### Required design decision

Pending acknowledgement must remain distinct from terminal acknowledgement.
Pending acknowledgement must preserve every fact required by the next admission attempt.
Terminal acknowledgement can remove request tracking after authoritative completion or an explicit terminal local disposition.

| Caller outcome | Required disposition |
|---|---|
| Missing block or certificate dependency | Pending. Publish the complete dependency set and preserve admission provenance. |
| `AlreadyBuffered`, `AwaitingBlock`, or `AwaitingState` | Pending. Keep the facts required for the deferred validation attempt. |
| Snapshot construction needs a missing block | Pending. Preserve provenance while the node retrieves history. |
| Validation quarantine with a complete buffer row | Pending unless the DAG independently confirms terminal admission. |
| Successful validation effects and buffer removal | Terminal. Remove obsolete request tracking. |
| Settled-history admission or `AlreadyAdmitted` | Terminal. Preserve any separate cleanup obligation, but release request tracking. |
| Malformed or uninteresting delivery with explicit local rejection | Terminal local disposition. Do not retain a retry solely for that rejected delivery. |
| `DuplicateInFlight` | Another worker owns the identity. Do not delete that worker's tracking. |

A complete repair must also separate active request capacity from durable pending provenance.
Possible designs include explicit durable provenance or a proven reconstruction rule from existing durable evidence.
Each design needs retention, pruning, restart, malformed-input, and dependency-progress obligations.
No new schema or reconstructed admission rule is selected here.
Those choices require upstream Casper review under the current task authority.

## Initial ownership abstraction

A **durable owner** is an existing buffer row for a block that still needs processing.
A **request owner** is a request-tracker entry that retains the block for another processing attempt.
**Terminal evidence** permits the node to finish processing that block without another buffer retry.

The initial model keeps durable rows, cache membership, terminal evidence, request ownership, and acknowledged blocks separate.
Each worker also has an independent block identity and processing phase.
Two workers can process distinct identities or the same identity.

| Action | Ownership effect | Implementation boundary |
|---|---|---|
| `Begin` | Establish a request owner for the processing attempt. | Receipt and worker admission. |
| `Commit` | Establish a durable owner. | Successful backend transaction. |
| `FailWrite` | Preserve existing owners. | Backend transaction returns an error. |
| `PublishCache` | Cache only currently durable data. | Publication after a successful commit. |
| `Acknowledge` | Release request ownership only when durable or terminal ownership exists. | `ack_processed` and request-tracker cleanup. |
| `Release` | Release the worker without forgetting an unresolved request owner. | Queued-item destruction after a processing error. |
| `Evict` | Remove cache membership only. | Local buffer memory management. |
| `Resolve` | Replace pending ownership with terminal evidence. | Authoritative terminal cleanup. |
| `Restart` | Discard volatile state but retain acknowledged durable obligations. | Node restart and cold buffer reconstruction. |

The TLA+ model exposes commit and cache publication as separate actions.
Resolution can occur between those actions.
Cache publication must not recreate cache membership for a row that terminal cleanup has removed.
The current implementation holds its state lock across commit and cache publication.
Any replacement must preserve that exclusion or revalidate publication under the appropriate lock.

## Safety invariants and regression obligations

| Model invariant | Required behavior | Implementation test obligation |
|---|---|---|
| `TypeOK` | Worker phases and owned identities agree. | Queue ownership and worker-release tests. |
| `Inv_CacheOwnership` | Every cached entry has a durable row. | Failed commits must not publish cache entries. Test resolution before delayed cache publication. |
| `Inv_AcknowledgedDurability` | Every acknowledged block has a durable row or terminal evidence. | Failed-first-publication regression and successful-publication control. |
| `Inv_LiveRetry` | Every unresolved block in the current session retains durable or request ownership. | Check ownership after actual worker release, including duplicate workers. |
| `Inv_ResidentBound` | Cache membership stays within the configured entry count. | Eviction and zero-capacity tests. Verify byte and backend limits separately. |

The native regression invokes `process_owned_block`, not a replacement worker model.
Its fault store fails the first actual buffer transaction.
The fixture checks block format, signature, content hash, and initial request ownership before processing.
It also checks body storage, absent DAG admission, the injected failure count, and worker-resource release afterward.

The control permits buffer publication and expects request ownership to transfer to the durable row.
The failing case requires request ownership to survive the unsuccessful first publication.
These tests do not establish that an unchanged request-tracker entry will receive future service.
Request scheduling, restart discovery, and persistence remain separate composition obligations.

## Initial TLA+ checks

The checked configurations use two blocks and two workers.
The safe configurations use one cache slot and zero cache slots.
TLC explored all reachable states in each configuration without an invariant violation.

| Safe configuration | Distinct states | Search depth |
|---|---:|---:|
| One cache slot | 23,306 | 21 |
| Zero cache slots | 13,118 | 19 |

Five unsafe variants must fail their exact expected invariants.
All five produced the expected counterexample in the recorded run.

| Unsafe behavior | Expected failed invariant |
|---|---|
| Acknowledge after an unsuccessful first write | `Inv_AcknowledgedDurability` |
| Publish cache membership before durable commit | `Inv_CacheOwnership` |
| Forget the request when a failed worker exits | `Inv_LiveRetry` |
| Delete a durable row during cache eviction | `Inv_AcknowledgedDurability` |
| Restore all durable rows into a bounded cache | `Inv_ResidentBound` |

The shortest acknowledgement trace is receipt, failed write, and acknowledgement without a durable row or terminal evidence.
The native regression targets that trace through the actual worker.
The other unsafe variants test specification sensitivity.
They do not each establish a new production defect.

## Initial Rocq correspondence

The Rocq source uses arbitrary key types and arbitrary finite operation histories.
It proves preservation of cache ownership, acknowledged durability, live retry ownership, and the cache-entry bound.
It also states that acknowledgement without durable or terminal ownership violates the acknowledgement invariant.

The proof erases worker-local phases but retains all ownership mutations.
Successful writes, acknowledgements, resolution, cache replacement, and restart correspond to explicit proof transitions.
Failed writes and worker release leave the ownership projection unchanged in the proposed safe design.
The native implementation must satisfy those conditions before the proof applies.

## Capacity and receipt refinement

`BufferRetryHandoff.tla` removes the assumption that every receipt obtains a tracker slot.
It separates queue reservation, receipt recording, queue publication, failed writes, retry handoff, and worker release.
A **worker lease** retains the existing identity claim and byte reservation.
A **retry-ready entry** has `received=false` and remains subject to its existing retry policy.
Retry readiness does not bypass quarantine, cooldowns, or exhausted retry budgets.

Two workers can operate concurrently on different block identities.
The existing per-hash identity claim excludes simultaneous owners of the same identity.
A repeated delivery can obtain a new lease after the previous lease ends.
This exclusion is local to one queue, not a serialization rule across validators.

| Refined operation | Required ownership behavior |
|---|---|
| `Reserve` | Acquire the identity, byte reservation, and channel slot before receipt changes. |
| `CancelReservation` | Release rejected or canceled reservations before receipt changes. |
| `Receive` | Preserve retry policy and mark receipt only after reservation succeeds. |
| `Publish` | Transfer the lease to the queue without an intervening asynchronous suspension. |
| `Reopen` | Return a retry-ready owner when tracker capacity permits. Preserve existing policy. |
| Capacity refusal | Retain the worker lease. Do not claim a successful tracker handoff. |
| `RetryPublication` | Permit another local publication attempt without waiting for tracker capacity. |
| `Release` | Require durable ownership, terminal evidence, or a retry-ready tracker entry. |

The model permits other workers between receipt and publication.
It excludes cooperative producer cancellation inside that synchronous section.
Process termination remains possible through `Restart`.
Cancellation after queue publication is not an independently verified recovery mechanism.
A supervisor that cancels one live worker must retain its item or establish another owner first.

### Late receipt race

Before repair, the network producer sent the queue item before it called `ack_receive`.
Both recovery producers also acknowledged receipt after publication.
The worker can finish before this delayed acknowledgement executes.

After a local failure, the delayed receipt can change a reopened entry back to `received=true`.
After successful publication, the delayed receipt can recreate an entry that durable acknowledgement already removed.
The initial worker regression pre-records receipt, so it does not reproduce this producer interleaving.
The TLA+ negative control exposes the ownership consequence of that ordering.
Queue tests now check the callback boundary against an independent observer thread.
Actual network and recovery producer tests also pass.
Those checks do not resolve the separate pending-provenance failure.

The repaired queue boundary uses a synchronous before-publication callback.
The callback runs after identity, byte, and channel reservations succeed.
It records receipt before `permit.send` exposes the worker item.
No `.await` can occur between these two operations.

Moving receipt before the whole queue operation is insufficient.
That order would mutate tracking for duplicates and rejected items.
All three production paths must use the shared publication boundary.
Recovery receipt failures must still disable that pass's proposal, as existing acknowledgement tests require.

### Retry policy

Local receipt and failure handoff must preserve these existing fields:

- Dependency authority.
- Initial request timestamp.
- Retry and peer-retry counters.
- Quarantine deadline.
- Dependency, broadcast, and peer cooldown records.

The TLA+ policy record models authority, an attempt counter, quarantine, cooldown, and initial age.
An independent expected-policy record changes only during explicit scheduler or restart actions.
Local mutations must leave both policy records equal.
Rocq quantifies over an arbitrary policy type, rather than a fixed list of policy fields.
Its local-history theorem therefore preserves every field placed in that policy projection.

Before repair, receipt called `cleanup_aux_tracking_for_hash`.
The property tests show that one receipt resets an existing counter and clears an existing quarantine deadline.
An additional example starts with the real retry limit exhausted and a future quarantine deadline.
Receipt replenishes that exhausted budget, so the example fails its budget assertion.
The generated tests also check cooldown maps, authority, and the original timestamp.
The failing quarantine assertion occurs before the cooldown assertions in that property.
Thus, the recorded failure alone does not establish execution of every later assertion.

### Refined invariants and test mapping

| Invariant or theorem | Meaning | Implementation evidence or remaining test |
|---|---|---|
| `Inv_LiveOwner` | Accepted unresolved work retains a durable, terminal, retry-ready, or worker owner. | Both tracked and full-tracker worker regressions fail before repair. |
| `Inv_TrackerBound` | Existing tracking never exceeds the configured count. | Full-tracker success and failure fixtures establish the capacity-refusal branch. Generated handoff histories remain required. |
| `Inv_UniqueLease` | Each identity has at most one local queue owner. | Existing identity tests remain applicable. New callback duplicate and rejection tests remain required. |
| `Inv_RetryPolicy` | Local actions do not alter authority or retry policy. | Three generated properties and one active-quarantine example exercise real retriever methods. Generated handoff histories remain required. |
| `Inv_AcknowledgedDurability` | Acknowledged work retains a durable row or terminal evidence through restart. | Successful-publication controls pass. Existing persistence checks remain separate. |
| `Inv_LocalRetryAvailable` | Tracker refusal does not disable local publication retry. | The full-tracker failure regression fails because the current worker releases its last owner. |
| `handoff_capacity_preserves_owner` | Capacity refusal returns the unchanged owner state. | The production handoff API must return explicit capacity refusal without releasing the lease. |
| `delayed_receipt_is_unsafe` | Receipt without a surviving owner can remove the last retry-ready state. | Deterministic network and recovery producer tests remain required. |

### Executed refined checks

The final refinement run includes cancellation before receipt and arbitrary scheduling between the two workers.
It checks two block identities, two workers, and a modeled retry limit of one.
The nonzero-capacity initial state includes one exhausted, quarantined, dependency-authorized request.
The second identity begins without authority or retry debt.
The retry limit and Boolean timer states are finite exploration parameters, not production settings.

| Safe configuration | Distinct states | Search depth |
|---|---:|---:|
| One tracker slot | 132,021 | 41 |
| Zero tracker slots | 3,823 | 22 |

All ten negative controls failed their intended invariant.
The ownership controls cover capacity release, delayed receipt, false retry readiness, and capacity-only waiting.
The policy controls cover authority gain, counter resets during handoff and receipt, quarantine clearing, cooldown clearing, and age reset.
These controls test the specification's sensitivity.
They do not each identify a separate production defect.

`BufferRetryHandoff.v` passed compilation and a separate kernel check.
All 19 printed theorem assumptions were closed under the global context.
The proof covers arbitrary key types, worker types, policy types, capacities, and finite local-operation histories.
Each local history preserves ownership, tracker bounds, unique leases, and the complete policy projection.
The separate restart theorem preserves ownership while permitting a fresh policy.
It does not claim that retry counters survive process restart.

The final combined gate also repeated the earlier publication-model and Rocq checks successfully.
The evidence directory is `target/verification/buffer-publication/run.utZSIT`.
The [work log](../../../work-logs/task-pr216-buffer-publication-2026-09-08.md) records native results and exact execution scope.

## Complete-row and maintenance boundaries

The implementation review found two additional composition gaps.
Individual dependency writes could leave a partial durable row.
Maintenance could use an old receipt observation after a worker had reopened the request.

The actual-worker regression reproduced partial publication with a full tracker and active validation quarantine.
The worker released ownership while the row lacked a required certificate dependency.
Without quarantine, the same later-write fault recovered on the next worker attempt.
That passing case did not disprove the quarantine failure.

The maintenance regression also reproduced an initial-timestamp reset through the actual `request_all` method.
The repair rechecks the current receipt state under the tracker lock.
It changes only receipt readiness and the scheduling timestamp.
It preserves initial age, authority, retry counters, quarantine, and cooldown records.

### Storage contract and upstream alignment

`add_dependencies` validates certificate keys before mutation.
It then reads the existing row under the buffer write lock.
One backend transaction stores the union of existing, block, and certificate dependencies.
Resident indexes change only after that transaction succeeds.
A row that already includes the complete submitted set requires no write.

An empty dependency set creates an explicit empty row when no row exists.
It does not erase existing dependencies.
`contains_durable_row` distinguishes this explicit row from an implicit missing parent.
An implicit parent can appear in the pendant index without owning a stored row.

Quarantine does not establish complete publication by itself.
Both quarantine return paths first reconstruct missing dependencies from the block and available authoritative metadata.
The early validation-quarantine path reads the stored detached block, not the incoming payload.
It checks the stored hash, content identity, format, and signature before using its dependencies.
If no stored body exists, normal interest, validation, and storage checks must precede quarantine repair.
They then repair any incomplete row before permitting durable handoff.
This also covers partial rows created before the repair.
The check does not run replay, issue votes, or bypass validation quarantine.

Current upstream `dev` already unions dependencies individually.
The batch transaction preserves those successful set semantics and the existing storage schema.
It changes failure atomicity, not Casper validity, fork choice, voting, finality, or wire data.
The implementation retains the existing lock order and does not query the DAG under the buffer write lock.

A dependency snapshot can become stale before publication.
For example, another worker can admit a parent before the publisher restores an older dependency edge.
This race also exists with upstream individual writes.
Buffer edges therefore remain conservative retry bookkeeping, not authoritative evidence that a dependency is unavailable.
Block readiness checks admitted metadata.
Certificate maintenance removes edges for certificates that are already available.
Regression tests cover both publication orders and subsequent edge resolution.
Those unit tests do not establish eventual network delivery or whole-node recovery latency.

### Boundary specification and regression mapping

`BufferHandoffBoundaries.tla` isolates these boundaries from the larger worker model.
It starts with two independent worker owners and possible pre-existing partial row contents.
One block requires no dependencies.
The other requires a block and a certificate.
Only complete publication establishes durable retry ownership in this model.

| Invariant | Required boundary | Regression coverage |
|---|---|---|
| `TypeOK` | Each state field stays in its declared domain. | Generated storage and request histories. |
| `Inv_CompleteRows` | Published ownership includes every required dependency. | Mixed-dependency atomic failure, quarantine, and old-partial-row tests. |
| `Inv_CanonicalDependencies` | Quarantine repair cannot import dependencies from a conflicting incoming payload. | Actual-worker example and generated incoming parent/certificate mutations. |
| `Inv_LiveOwner` | Worker release retains a complete durable or retry-ready owner. | Actual-worker tracked and full-tracker cases. |
| `Inv_Capacity` | Handoff cannot exceed tracker capacity. | Real tracker limit and refused-admission tests. |
| `Inv_Policy` | Local maintenance does not renew retry policy. | Generated handoff histories and the stale-maintenance regression. |
| `Inv_DurableReleaseEnabled` | An explicit empty row permits durable handoff. | Explicit-empty-row and implicit-parent distinction tests. |

The safe configurations completed with 80 and 9 distinct states for one and zero tracker slots.
Their search depths were eight and five.
Five negative controls failed their exact required invariants.
They cover partial publication, invisible empty rows, trusted old rows, conflicting incoming dependencies, and stale maintenance.
These small boundary checks supplement the larger concurrent handoff model.
They do not replace that model or model the whole node.

`BufferHandoffBoundaries.v` proves 13 results over arbitrary key, dependency, and policy types.
Successful publication retains existing dependencies and includes every required dependency.
Failed publication leaves all rows unchanged.
Independent row contents remain unchanged.
Maintenance preserves policy and initial age, including when a worker handoff precedes its mutation.
Compilation and the independent kernel check passed before the corresponding production changes.

### Identity assumption found during review

The first quarantine repair used the incoming block before normal identity checks.
Quarantine uses an advertised hash, so another payload could carry that hash with different dependencies.
The actual-worker regression reproduced publication of those different dependencies.
This defect arose in the local repair, not in the earlier source attribution to `dev`.

The earlier proof assumed that the required dependency set came from the correct block.
It did not enforce which payload supplied that set.
The refined model now distinguishes stored dependencies from conflicting incoming dependencies.
The negative control and generated native mutations check that distinction.
The parameterized proof establishes independence from incoming dependency lists.
The implementation must still establish stored identity through the concrete checks described above.

The current quarantine handoff retires the existing request entry after complete row or terminal ownership exists.
The original successful case used unsolicited data and did not expose lost provenance.
The new solicited-history regression rejects this cleanup rule for pending blocks.
The durable owner survives, but the next admission attempt loses required eligibility.
The [pending-provenance analysis](#pending-provenance-unresolved-admission-boundary) records why cleanup needs another design review.

The Loom test imports the production receipt-mutation function.
It explores concurrent maintenance observation and worker handoff under the tracker lock.
An unsafe control uses the earlier snapshot without revalidation and must fail.
The existing transaction Loom test imports the production commit-before-publication function.
Neither test substitutes Loom synchronization into the complete node implementation.
Native tests cover the actual queue, retriever, storage, and worker boundaries separately.

## Approved provenance repair and checked boundaries

The approved repair preserves explicit dependency-request history across pending publication and restart.
It must not change Casper admission predicates, voting, finality, fork choice, or network messages.
The separate pruning architecture still requires upstream review.

### Existing announcement promotion

An existing announcement did not become a dependency request when this node later requested the same hash.
`dependency_request_promotes_previously_announced_hash` reproduced that failure before the production change.
The repair merges the dependency fact under the existing request mutex before early returns.
An announcement cannot clear an established fact.
Promotion does not reset age, counters, quarantine, or receipt state.

`DependencyRequestProvenance.tla` checks exact authority, capacity, and preserved policy.
Its safe configuration uses two keys and one tracker slot.
It checked 21 distinct states at depth five.
Four controls detect new-entry-only promotion, overwriting authority, resetting policy, and an unlocked stale update.
`DependencyRequestProvenance.v` proves six results, including arbitrary request histories and order independence.
All six theorem assumptions were closed, and the independent kernel check passed before production changes.

The native suite then passed all 38 retriever tests, including the formerly failing regression and a generated history property.
Two Loom tests exercise the production merge helper and its unsafe stale-update control.
Strict Casper Clippy passed with unchanged source hashes.
These results qualify promotion only, not persistent handoff.

### Durable handoff specification

Pending provenance requires an explicit durable record beside the dependency row.
Parent edges cannot identify whether this node requested a block.
Keeping every pending identity in the active tracker can prevent further dependency requests at capacity.
The approved design therefore uses on-demand durable policy lookup without an unbounded resident policy map.

The durable record must preserve original age, dependency authority, retry counters, and relevant quarantine and cooldown state.
The record must have a schema version and an ownership revision.
Dependency rows and policy records must commit atomically in the same database environment.
A subset dependency update must still commit changed policy.
Failed publication must retain the active request or worker lease.

`DurablePendingHandoff.tla` separates capture, policy update, commit, retirement, retry, eviction, terminal disposal, and restart.
The checked configuration has two keys, two workers, one active slot, and two policy revisions.
It checked 106,856 distinct states at depth 19.
Six controls detect stale commit, stale cleanup, partial publication, terminal resurrection, eviction loss, and restart loss.
An initial model expression omitted parentheses around a Boolean assignment.
The negative-control gate detected the incomplete successor state and rejected that run.
The corrected model passed before any durable production change.

| Invariant | Implementation obligation |
|---|---|
| `Inv_CompletePublication` | Commit dependency rows and pending records together. |
| `Inv_Ownership` | Retain an active, durable, or authoritative terminal owner. |
| `Inv_Capacity` | Do not consume active slots for all dormant durable requests. |
| `Inv_DurablePolicy` | Reject stale policy publication and preserve the committed policy across restart. |
| `Inv_NoInventedAuthority` | Establish dependency authority only through a genuine request. |
| `Inv_NoStaleCleanup` | A stale acknowledgement cannot retire a newer policy revision. |
| `Inv_TerminalDominance` | A stale worker cannot restore terminal work. |

The model treats terminal evidence as authoritative input.
It does not verify the separate Casper database transaction or the network retry lifecycle.
Its restart transition preserves committed obligations, not receipts whose first durable write never succeeded.
The checked domain permits one restart and bounded revisions.
These results do not establish arbitrary-domain safety or eventual network delivery.

Three actual-worker tests remain failing before the durable repair.
They cover ordinary pending cleanup, quarantined pending cleanup, and reconstructed retriever and buffer instances over surviving rows.
Each test verifies complete durable dependencies before checking old-block eligibility.
The reconstruction fixture is not a separate operating-system process or an LMDB reopen test.
Production qualification also requires those concrete restart and transaction checks.

Old rows without provenance must remain unknown, not acquire fabricated authority.
Migration cannot reconstruct prior request history from dependency edges.
Corrupt or unsupported policy records must produce an error rather than an affirmative admission fact.

The implemented `PendingRequestPolicy` codec has schema version one and a maximum encoded length of 75 bytes.
It preserves the dependency fact, original and last-request timestamps, counters, peer cursor, and four optional cooldown or quarantine timestamps.
The record has fixed fields, not a fixed encoded length.
Absent optional timestamps use fewer bytes.
The record contains no peer list or block body.

The decoder rejects unsupported versions, zero revisions, truncated data, trailing data, and oversized input.
Revision exhaustion returns an error without changing the policy.
Four codec tests passed, including generated field round trips and arbitrary byte inputs.
Strict storage Clippy also passed.
The storage layer now opens `pending-request-policy-v1` beside `parents-map` in the same database environment.
Both stores are mandatory constructor inputs.
There is no implicit volatile fallback.

| Storage operation | Atomic effect | Failure behavior |
|---|---|---|
| `publish_pending_request` | Union block and certificate dependencies and compare-and-swap the policy. | Preserve both prior records and the memory projection. |
| `update_pending_request_policy` | Verify the parent row and compare-and-swap the policy. | Reject stale revisions and missing parent rows. |
| Explicit row removal | Remove the parent row and its policy together. | Preserve both records if the transaction fails. |
| `pending_request_policy` | Decode one bounded record and require its parent row. | Return unknown for legacy rows and errors for corrupt or orphaned records. |

Compare-and-swap accepts only the exact prior encoded policy.
An initial record has revision one.
Each replacement advances the revision by one without overflow.
A replacement cannot reset original age, clear established dependency authority, or decrease either retry counter.
Changed policy still commits when the dependency set is unchanged.

Six storage tests passed in `policy-integration.Rkvjrk`.
They cover transactional rollback, stale revisions, concurrent writers, legacy records, corrupt records, and generated operation histories.
A child-process test exits before or after the LMDB commit.
The parent then verifies that dependencies and policy appear together after database reopen.
Strict Clippy passed for storage, Casper, and node library and test targets in `policy-lint.UQK8lm`.

These storage results do not resolve the pending-worker failures.
The request owner and worker acknowledgement paths still need integration.
The broader storage run also reproduced the separately gated pruning defect.
The new policy namespace does not make destructive pruning safe.

### Delayed retry completion

Three deterministic transport-callback tests expose a separate request-ownership defect.
The callback removes the old request and creates a replacement before the old transport call returns.
Waiting-peer retry, known-peer retry, and dependency recovery each increment the replacement counter from seven to eight.
The existing completion code identifies the request only by hash.
Pinned `dev` has the same post-transport counter pattern.

The reviewed repair must associate each retry operation with its originating request incarnation.
An incarnation is one request lifetime, not merely its block hash.
Pending handoff transfers that lifetime rather than replacing it.
Terminal cleanup ends that lifetime.

The operation must reserve retry allowance before transport without holding a lock during transport.
Completed retry actions charge the same active or durable pending incarnation at most once.
Returned transport errors complete an action. Cancellation before completion releases the reservation without charging.
A completion must not charge a replacement incarnation or recreate terminal policy.
This contract restores pinned upstream's counting rule under the user's dev-first authority instruction.
The earlier attribution of success-only counting to upstream was incorrect.
The September 9 model revision separates returned errors from cancellation before implementation changes.

| Completed action or suppression | Total increment | Peer increment |
|---|---|---|
| Initial admission, including a returned transport error | Zero | Zero |
| Waiting-peer retry and its optional fallback broadcast | One for the complete action | Zero |
| Known-peer retry, with either returned result | One | One |
| Broadcast retry, with either returned result | One | Zero |
| Unsuppressed recovery action | One | Zero |
| Cancellation, cooldown, capacity refusal, or no selected action | Zero | Zero |

An action is not necessarily one transport call.
Recovery can append peers to a nonempty waiting list without transport, then count the completed action.
A waiting-peer action can include two transport calls but counts once.
Cancellation during the fallback broadcast does not count that incomplete action.
Transport errors must not suppress the fallback or abort unrelated block maintenance.
Storage errors remain errors and must retain any observed completion.
This counting correction does not establish upstream equivalence for quarantine expiry or other remaining policies.

`RetryOperationOwnership.tla` checks this contract with two incarnations of one hash and two concurrent operations.
The configuration permits one active request, one ordinary retry allowance, and one clock round.
The revised no-probe model checked 49,804 states at depth 21 in `run.Ce21VJ`.
Seven controls detect replacement charging, premature charging, ignored pending completion, duplicate completion, unreserved allowance, exhausted reservations, and success-only counting.
Every reservation requires unused budget. Expired quarantine alone grants no allowance.
The model covers one budget cycle. Separate renewal checks cover restoring allowance and invalidating obsolete operation tokens.
The model permits cancellation before or during transport and clears unfinished operations during restart.
It preserves completed counters and quarantine state for pending owners.
It does not count all possible external deliveries across a crash.

`RetryOperationOwnership.v` proves 16 results over arbitrary decidable owner and operation identities.
These include origin-only charging, duplicate-completion identity, cancellation preservation, terminal isolation, saturated counter bounds, and reservation arithmetic.
The action-outcome refinement proves equal charging for returned success and returned error.
The independent kernel check passed, and all theorem reports were closed under the global context.
The proof assumes that the concrete operation table supplies unique live tokens and consumes each token atomically.
It does not derive Rust lock placement or storage durability.

A fourth native regression demonstrates recovery after tracker-capacity refusal.
The tracker contains 2,048 requests before recovery starts.
Recovery still dispatches one request for an identity that has no tracker entry.
The failing assertion requires zero transport calls.
The same test requires no orphan counter or cooldown state after refusal.
This failure must be repaired at admission, not concealed through later map cleanup.

Required implementation tests also include pending completion, cancellation, competing final allowances, stale cleanup, and restart with unfinished transport.
The durable handoff model alone does not qualify those paths.

### Canonical policy owner

The reviewed implementation uses one shared policy owner for each live request incarnation.
A weak registry maps the block hash to that owner without retaining every dormant pending policy in memory.
An active request or a bounded operation handle retains the owner strongly.
Every lookup must upgrade the existing weak reference before loading a policy from storage.
Otherwise, a disk reload and an outstanding retry can modify separate copies of the same policy.

| Component | Required responsibility |
|---|---|
| Active request map | Enforce the existing 2,048-entry bound and retain active owners. |
| Weak registry | Reuse live owners and remove dead references before inserting new keys. |
| Policy owner | Serialize policy changes and distinguish volatile, durable pending, and terminal backing. |
| Operation handle | Retain the originating owner and reserve bounded retry allowance. |
| Operation permit | Bound outstanding handles, including handles for replaced terminal owners. |
| Durable policy store | Preserve pending policy without retaining all pending owners in memory. |

Pending publication holds the owner lock through the atomic dependency-row and policy-record commit.
Only a successful commit changes the owner backing to durable pending.
Removing active membership does not end that owner while an operation retains it.
A completed retry action updates that owner's volatile policy or durable record.
Terminal disposal closes the old owner before replacement becomes possible.
The replacement receives a distinct owner even though its block hash is unchanged.

The lock order is DAG, when required, then registry, owner, and buffer.
Completion requires only owner and buffer locks.
No operation may acquire the registry while holding an owner lock.
No lock may cross a network `await`.
Independent requests can therefore perform network I/O concurrently.
Synchronous policy and database transitions still require their existing local exclusion boundaries.

`PendingOwnerRegistry.tla` checks canonical loading, shared completion, terminal replacement, owner destruction, and dead weak-reference cleanup.
Its configuration uses three keys, three owner identities, and two bounded handles.
It checked 129,259 states at depth 16.
Four controls detect duplicate loaded owners, unbounded dead references, writes from terminal owners, and detached owners without handles.
The safe model checks eight invariants, including handle lifetime, owner bounds, canonical ownership, current writers, and coherent policy.

The model treats locked lookup, load, and publication as one action.
The implementation must retain that lock boundary and propagate load errors without publishing an owner.
The model's two handles represent bounded strong ownership, not a measured production memory limit.
Production qualification must map every active, retry, and temporary handle to an explicit finite bound.
The operation and registry models are complementary specifications, not a machine-checked composition of the full node.

Live-process identity does not require a persistent incarnation allocator when one registry controls the pending store.
No old owner handle or completion survives process restart.
That argument excludes completion tokens shared between processes and independent writers that bypass the registry.
Startup must preserve committed policy while treating unfinished external delivery as uncertain.

### Completed retry with failed policy persistence

A retry action can return success or error before its policy write fails.
Releasing the last owner reference at that point would discard the counter increment.
A later activation could then reload an older budget from disk.

The completion must retain its original owner, operation identity, and existing operation permit.
The effective policy includes the completed attempt before persistence.
Here, the effective policy means the current in-process counters and request metadata.
A storage error cannot restore that retry allowance.

Failed persistence transfers the operation into a bounded deferred-completion queue.
Each operation can occupy at most one queue slot.
The queue capacity equals the operation-permit capacity.
The transfer therefore needs neither another permit nor an unbounded allocation.
The queue retains no block body.

A subsequent policy write can include several completed operations.
The owner must mark every included completion as persisted under the same owner lock.
Draining such an entry releases its permit without another increment.
The drain performs storage work only, not another network request.

Remove an entry under the queue lock before acquiring its owner lock.
Release the owner lock before requeueing a failed entry.
Rotate failed entries and bound the drain batch.
Independent certificate maintenance must remain available.
Neither a missing row nor a compare-and-swap conflict proves terminal disposal.

`DeferredRetryCompletion.tla` separates transport, observed completion, failed persistence, deferred ownership, batch persistence, release, activation, terminal replacement, and restart.
The safe configuration checks two request incarnations and two concurrent operations with two permits.
The earlier success-only contract checked 2,538 states at depth 13 in `run.VqFVUz`.
The September 9 revision checked the same state count in `run.DakDXS`, with seven exact negative controls.
The new control detects failure to count a returned transport error.

| Invariant | Required regression |
|---|---|
| `Inv_ExactEffective` | Failure and cold activation cannot restore an already consumed allowance. Batch persistence cannot charge twice. |
| `Inv_CommittedPolicy` | A committed counter includes exactly the completions in that committed snapshot. |
| `Inv_CompletionRetained` | Every completed, unpersisted operation retains its exact owner and permit. |
| `Inv_PermitBound` | Saturated completion storage cannot admit another operation or lose an existing completion. |
| `Inv_CurrentWriter` | A terminal owner's completion cannot write into a replacement request. |
| `Inv_NoNetworkOnDrain` | Repeated persistence attempts issue no network requests. |
| `Inv_IndependentMaintenance` | Completion persistence failure does not disable certificate maintenance. |

This model assumes atomic storage success or failure, as qualified by the paired-store tests.
It treats storage-error classes identically because none authorizes release or policy reset.
It does not prove disk availability, network delivery, scheduling fairness, or wall-clock progress.
Restart explicitly discards uncommitted completion information and restores the committed policy.
The volatile queue cannot preserve uncommitted counters through process death.
Production ownership and concurrency tests must still establish correspondence with these transitions.

### Native owner-component evidence

The candidate [request-owner component](../../../../casper/src/rust/engine/block_retriever/request_ownership.rs) implements the canonical owner and deferred completion boundaries.
Its [native tests](../../../../casper/tests/pending_request_owners_spec.rs) use the actual Casper buffer implementation with an in-memory store manager.
The component is not yet connected to the production retriever.
These results therefore do not qualify the original worker regressions.

| Formal requirement | Native evidence |
|---|---|
| Canonical owner and coherent policy | Concurrent cold lookups retain the same owner. Activation reuses that owner. |
| Origin-only completion and terminal isolation | Delayed completion and repeated old-owner cleanup leave a replacement unchanged. |
| Exact counters and reserved capacity | Two generated properties check sequential histories and multiple outstanding operations, including cancellation and replacement. |
| Pending handoff preservation | Concurrent publication and completion preserve the committed charge. A new registry restores the committed policy. |
| Completion retention | Missing rows and revision conflicts retain the successful completion, effective counters, and permit. |
| Batch persistence without duplicate charge | A later completion commits an earlier deferred charge. Queue drainage does not increment it again. |
| Bounded operation retention | A full deferred queue prevents another reservation. Terminal disposal releases retained permits. |
| Independent queue progress | A failed first entry does not prevent persistence and release of a later entry. |
| Final allowance and expiry | Two competing threads obtain one final normal allowance and no expired-budget probe. Explicit renewal restores the budget. |
| Restart policy preservation | A new registry restores the claimed probe deadline and saturated counters. |
| Atomic mutation rejection | Rejected history changes preserve both policy and transient data. Failed publication preserves the active owner and stored snapshot. |

The final focused run passed 17 tests and strict Clippy in `request-owners.pCg7BU`.
Each generated property used 128 cases.
Source hashes matched before and after the run.
The scope used a 4 GiB memory cap, disabled swap, and one CPU.

The revision-conflict test deliberately changes storage outside the registry to inject a persistence error.
Independent registry writers are not a supported production arrangement.
The failure must remain visible without overwriting the conflicting snapshot or restoring consumed allowance.

Native thread tests do not exhaust all schedules.
The current component tests do not establish whole-node handle bounds or certificate-maintenance progress.
Production integration, same-component concurrency qualification, and the seven original worker and retry regressions remain required.
The tests preserve the feature's current successful-completion counter policy.
They do not ratify that policy as equivalent to pinned upstream `dev`.

## Limits

Terminal evidence is an input to this ownership model.
The model does not verify Casper votes, certificate validity, or finality.
The repair must obtain terminal evidence from the unchanged authoritative Casper paths.

Restart preserves acknowledged rows in the ownership abstraction.
It does not preserve volatile request provenance in the implementation.
It does not promise local recovery for an unacknowledged receipt whose first durable write failed.
The model does not establish network retransmission fairness or service latency.

The cache bound counts entries rather than allocated bytes.
It does not bound dependency-row size, captured scans, backend transaction resources, or process resident memory.
The [upstream refresh record](../../../work-logs/task-pr216-pruning-upstream-refresh-2026-09-08.md) identifies those remaining limits.

Individual dependency writes do not satisfy the complete-publication contract.
The repaired processor submits the whole missing set to the atomic union operation.
The boundary proof supplies the local composition contract.
Native transaction, quarantine, restart, and concurrency checks must also pass for implementation qualification.

The initial model still assumes that the request tracker accepts the identity.
Use the refined model for capacity refusal, worker ownership, and retry readiness.
The refinement does not prove the whole request scheduler's latency or eventual network delivery.
`Inv_LocalRetryAvailable` proves that a local retry action remains enabled, not that storage eventually succeeds.
Bounded backoff, live supervisor ownership, and eventual storage availability remain implementation and liveness obligations.

The Rocq proof and TLA+ model are specifications, not a machine-checked refinement of the complete Rust program.
The implementation must satisfy every operation guard and publication boundary before their guarantees apply.
Passing either model alone does not establish end-to-end node correctness.

The earlier `defer_for_admission` method was not a suitable unchanged replacement for error acknowledgement.
Its absent-entry path assigned dependency authority and cleared auxiliary retry state.
The repaired method shares the policy-preserving handoff operation.
A local publication failure must not manufacture dependency authority or replenish a retry budget.

## Reproduction

Run the gate from the repository root with resource limits:

```sh
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 -p CPUQuota=100% \
  bash scripts/check-buffer-publication-ownership.sh
```

Use `tla` or `rocq` as the script argument to select one checker family.
Use `handoff` to run only the refined TLA+ configurations.
Use `boundaries` for complete-row and maintenance checks with their Rocq proof.
Use `provenance` to reproduce the unresolved deletion, capacity, and restart controls.
Use `promotion` for request-history model checks and the corresponding Rocq proof.
Use `durable` for the proposed concurrent durable-handoff contract and its negative controls.
Use `retry-operations` for completion ownership, concurrent allowance checks, and the parameterized retry proof.
Use `owner-registry` for canonical loading and bounded shared-owner checks.
The script stores logs and source hashes under `target/verification/buffer-publication`.
It rejects timeouts, wrong counterexamples, and changed model inputs.
It requires successful Rocq compilation and a separate kernel check.

Sources:

- [TLA+ model](../../../../formal/tlaplus/block_admission/BufferPublicationOwnership.tla)
- [Rocq proof](../../../../formal/rocq/finalized_floor/theories/BufferPublicationOwnership.v)
- [Refined TLA+ model](../../../../formal/tlaplus/block_admission/BufferRetryHandoff.tla)
- [Refined Rocq proof](../../../../formal/rocq/finalized_floor/theories/BufferRetryHandoff.v)
- [Boundary TLA+ model](../../../../formal/tlaplus/block_admission/BufferHandoffBoundaries.tla)
- [Boundary Rocq proof](../../../../formal/rocq/finalized_floor/theories/BufferHandoffBoundaries.v)
- [Receipt Loom test](../../../../formal/loom/cost_accounting/tests/loom_receipt_policy_handoff.rs)
- [Worker regression](../../../../node/src/rust/instances/block_processor_instance/publication_tests.rs)
- [Receipt-policy properties](../../../../casper/src/rust/engine/block_retriever/publication_handoff_tests.rs)
- [Verification gate](../../../../scripts/check-buffer-publication-ownership.sh)
- [Pending policy codec](../../../../block-storage/src/rust/casperbuffer/pending_request_policy.rs)
- [Retry-operation model](../../../../formal/tlaplus/block_admission/RetryOperationOwnership.tla)
- [Retry-operation proof](../../../../formal/rocq/finalized_floor/theories/RetryOperationOwnership.v)
- [Canonical owner model](../../../../formal/tlaplus/block_admission/PendingOwnerRegistry.tla)
