# Block admission and transport residency

## Current correspondence limits

`LocalRequestActivation.tla` checks refusal classification at the locked decision, before later capacity or clock changes.
It checks quarantine precedence, storage errors, unchanged refused ownership, capacity metrics, and retained worker leases.
Run `bash scripts/check-buffer-publication-ownership.sh local-activation` for its safe configuration and six unsafe controls.
Its two hashes, one active slot, and three clock values cover 58,968 states. It abstracts weak caches and storage error internals.

`RetryMaintenanceOrder.tla` checks the caller order around the existing retry and expiry models.
It captures active owner identities and time, then separates due observation from retry selection before expiry.
Concurrent recitation can retain the same durable owner or create a replacement owner.
Fresh schedule publication must set its request clock atomically. A delayed clock repair cannot undo an incorrect visible schedule.
Run one safe configuration, five unsafe controls, and one unsupported-claim witness with `bash scripts/check-buffer-publication-ownership.sh retry-maintenance`.
The finite configuration has two hashes, two generations, three clock values, and one maintenance pass.
Initial budgets include both available and exhausted states. Selection can dispatch or retire after a captured due observation.
The unsupported claim requires each action to remain due at selection. Both source branches separate observation from selection without that requirement.
This witness identifies a proof limit, not a newly established production defect.
It checks local ordering and identity, not validator scheduling, network liveness, or exact production backoff intervals.
Retry completion is atomic in this harness. Separate operation models cover reservation, cancellation, and deferred completion.
Deadlock checking is disabled because this finite harness permits a completed pass with no further enabled action.

`RetryBudgetCustody.tla` checks atomic transfers among volatile owners, durable pending policy, and retired budget records.
Its configuration has two hashes, one active slot, a two-attempt budget, and bounded repeated renewal cycles.
Run its safe case and twelve exact unsafe controls with `scripts/check-buffer-publication-ownership.sh retry-custody`.
Independent reference values check attempt totals, quarantine deadlines, and authorized dispatch counts.
Capacity refusal preserves retired budgets. Failed publication preserves modeled ownership and policy state.
The model permits arbitrary interleavings at the local registry transaction boundary, not serialized validator execution.
It abstracts completed retry actions. Separate operation and renewal models cover reservations, cancellation, and captured sweep timestamps.
It does not prove stale-owner exclusion, unconfirmed storage recovery, unlimited cycles, or complete production integration.
The [custody contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md#retry-budget-custody) states those boundaries and the native regression obligations.

`AcceptedPublicationCustody.tla` checks exact-block publication responsibility after a completed local interest decision and verified storage.
Its safe configuration has two identities and two Casper contexts.
Run the safe case and ten exact unsafe controls with `scripts/check-buffer-publication-ownership.sh accepted-custody`.
The model checks custody, provenance, identity, context, and terminal exclusion.
Metadata loss preserves terminal membership. The metadata-availability control detects pending republication after that loss.
Stored restoration starts from a separately identified, previously verified body set.
Incoming old-block acceptance requires captured request provenance. Stored restoration uses its existing local entry point, without granting validation authority.
The restoration control rejects acceptance of a body outside the verified stored set.
It assumes an atomic publication boundary and does not prove liveness or complete worker integration.

`BufferIndependentExpiry.tla` checks two traversal orders over shared candidate membership.
It models bounded batches, separate processing, concurrent membership changes, and fair pass completion.
Run its safe case and seven exact unsafe controls with `scripts/check-buffer-publication-ownership.sh retry-traversal`.
Native properties and Loom tests exercise the production queue helper.
Whole-pass caller serialization and retry renewal remain integration requirements.

`RetryQuarantineEntry.tla` checks reserved allowance, concurrent completion, cancellation, peer-schedule retirement, and the committed entry projection.
Two safe configurations and twelve unsafe controls run through `scripts/check-buffer-publication-ownership.sh retry-entry`.
The [entry contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md#quarantine-entry-and-operation-accounting) maps its invariants to native evidence.
Storage entry passes focused tests. Complete owner retirement and maintenance integration remain unfinished.
This one-hash, one-entry model does not establish full lifecycle or whole-node correctness.

`RetryBudgetRenewal.tla` checks competing maintenance actors, immutable operation tokens, and an independent durable reference.
Its seven safe configurations and thirteen unsafe controls run through `scripts/check-buffer-publication-ownership.sh retry-renewal`.
One configuration permits two renewal cycles. Other configurations include two hashes and two operations.
The [renewal contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md#renewal-model-and-storage-preconditions) identifies the native tests and remaining storage preconditions.
Separate actions represent confirmed atomic failure and an unconfirmed commit with either paired storage outcome.
Reopening assumes valid storage that contains the old or new complete policy. The model does not cover arbitrary physical corruption.
Unbounded cycles, custody bounds, traversal bounds, and complete native recovery remain separate obligations.

`PendingPolicyRowGuard.tla` checks exact byte comparison for an unchanged dependency row, including equivalent decoded representations.
Two readers can overlap with two row replacements or removals.
The safe configuration and re-encoding control run through `scripts/check-buffer-publication-ownership.sh row-guard`.
The independent acceptance condition detects false conflicts as well as false acceptance.
The [row-guard contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md#exact-dependency-row-guard) explains native correspondence and the unchanged ABA limitation.

`RetrySelectionPolicy.tla` checks upstream retry selection, suppression, checked publication, and metric event boundaries.
Its three safe configurations and ten unsafe controls run through `scripts/check-buffer-publication-ownership.sh retry-selection`.
The [publication contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md#upstream-retry-selection) states each invariant and its correspondence limits.
The two-operation model is not a complete retry lifecycle, restart, or quarantine-renewal proof.

The admission models measure modeled encoded payload sizes, not decoded heap usage or whole-node resident memory.
They do not yet represent every completion-queue, scanner, and cancellation boundary.
The [backpressure audit](../../../docs/casper/theory/finalized-floor/admission-and-recovery-backpressure.md) records those missing obligations.
Do not use these models alone as an end-to-end memory guarantee.

`RecoveryActorService.tla` adds explicit requester service lanes, concurrent actors, channel closure, and timer-retention counterexamples.
The [service specification](../../../docs/casper/theory/finalized-floor/recovery-actor-service.md) maps the model to production and states its liveness premises.

`BlockPayloadOwnership.tla` separates completion payloads, worker copies, and reservation release.
The [ownership specification](../../../docs/casper/theory/finalized-floor/payload-reservation-ownership.md) explains both counterexamples and the logical-byte bound.

`AdmissionIdentityOwnership.tla` tracks each duplicate-suppression owner through producer, queue, worker, completion, cancellation, and receiver closure.
The [identity specification](../../../docs/casper/theory/finalized-floor/admission-identity-ownership.md) maps those transitions to the private queue lease and registry.
Three unsafe controls remove queued cleanup, duplicate exclusion, or exact-token release.
A fourth separates early producer rejection from body destruction and detects premature identity release.

`RecoveryPumpWake.tla`, `RecoveryPumpPass.tla`, and `RecoveryPumpDemand.tla` cover the single-driver control contract.
`RecoveryDispatcherOwnership.tla` separates input closure, child cancellation, retirement, and joining.
Their [control specification](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md) defines all nine unsafe controls and the remaining integration limits.

`BufferDurableMembership.tla` separates durable commit, memory publication, failure, and restart under the existing buffer mutation guard.
It checks explicit empty rows, referenced roots, and certificate namespace separation.
Five unsafe configurations expose row loss, early publication, and partial commit.
The [durable membership document](../../../docs/casper/theory/finalized-floor/buffer-durable-membership.md) specifies the production correspondence and qualification limits.

`BufferRetentionEpoch.tla` checks restored age epochs and complete identity counting.
`BufferCandidateRotation.tla` checks FIFO membership and examination fairness during arrivals, removals, and early capacity exits.
Their [candidate specification](../../../docs/casper/theory/finalized-floor/buffer-candidate-rotation.md) separates these local guarantees from unfinished retry-pump integration.

`BufferPublicationOwnership.tla` separates the first durable write from acknowledgement and cache publication.
`BufferRetryHandoff.tla` adds tracker capacity, worker leases, receipt ordering, retry policy, and cancellation before publication.
`BufferHandoffBoundaries.tla` checks complete dependency publication, explicit empty rows, old partial rows, and concurrent maintenance observations.
Its safe configurations use two workers and zero or one tracker slots.
Five negative controls must fail their exact boundary invariants, including conflicting incoming dependencies during quarantine repair.
Run `bash scripts/check-buffer-publication-ownership.sh boundaries` in a memory-limited scope for these checks and the corresponding Rocq proof.
The [publication specification](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md) records the checked configurations and the remaining implementation obligations.

`BufferPendingProvenance.tla` exposes an additional unresolved ownership boundary.
A complete durable row does not preserve the admission fact that an old block was explicitly requested.
Its controls expose premature provenance deletion, tracker saturation, and provenance loss after restart.
The passing configuration requires enough tracker slots for the complete dependency chain and excludes restart.
It does not establish production liveness or authorize an admission-rule change.
Run `bash scripts/check-buffer-publication-ownership.sh provenance` in a memory-limited scope to reproduce these limits.

`DependencyRequestProvenance.tla` checks monotonic promotion when an announced hash becomes an explicitly requested dependency.
Its two-key, one-slot safe configuration checked 21 states.
Four controls detect incorrect authority and policy changes.
Run the publication gate with `promotion` for these checks and six parameterized Rocq theorem checks.

`DurablePendingHandoff.tla` separates concurrent capture, update, commit, retirement, terminal disposal, eviction, and restart.
Its two-key, two-worker, one-slot configuration checked 106,856 states at depth 19.
Six controls detect stale publication, stale cleanup, torn records, terminal resurrection, eviction loss, and restart loss.
Run the publication gate with `durable` for these checks.
This model specifies the proposed durable boundary, not a completed production repair.
It excludes network-operation completion and treats authoritative terminal evidence as input.

`RetryOperationOwnership.tla` checks two incarnations of one hash and two concurrent retry operations.
Its no-probe safe configuration checked 49,804 states at depth 21 in `run.Ce21VJ`.
Seven controls detect incorrect completion ownership, premature charging, lost pending charges, duplicate charges, unreserved allowance, exhausted reservations, and success-only charging.
Elapsed quarantine time grants no reservation. Renewal remains in the separate renewal model.
The `retry-operations` mode also checks 16 parameterized Rocq results and runs independent kernel validation.
This single-cycle operation model does not establish retirement storage, renewal token invalidation, deferred persistence, or sweep ordering.

`PendingOwnerRegistry.tla` checks reuse of live policy owners before on-demand loading from storage.
Its three-key, three-owner, two-handle configuration checked 129,259 states at depth 16.
Four controls detect duplicate owners, retained dead references, terminal writers, and unowned allocations.
Use `owner-registry` to reproduce these checks.
The model assumes atomic locked loading and bounded strong handles.
Native integration must establish those boundaries before this design qualifies as an implementation repair.

## Earlier admission and transport models

TLA+ model of byte-bounded admission for the block-processing pipeline,
written from the counterexamples found after the 2026-08-04 daily-soak breach
(run 30880995655). It distinguishes the historical count-only queue, a lossy
byte-bound repair, the implemented request-backed byte-bound refinement, and
the historical whole-buffer materialization path.

The same directory models the transport boundary immediately before block
admission: byte/item ownership, compressed wire residency, lazy chunking,
fanout, remote completion, HTTP/2 admission, service parallelism, peer-slot
initialization, queue retirement, and request-local validation.

## Model ↔ code

| Model | Code |
|---|---|
| `queued` (FIFO, `CountCap`) | `BlockProcessingQueueSender::try_enqueue` — nonblocking mpsc count admission |
| `RetainedBytes` (≤ `ByteCap`) | an RAII byte reservation owned by each queue item and moved intact into `BlockProcessorInstance` for replay |
| `processing` (≤ `MaxParallel`) | `BlockProcessorInstance` owns a bounded worker `JoinSet`. Completed, unjoined workers still count toward the limit. |
| `resident` (≤ `MaxDeliveries`) | decoded inbound `BlockMessage` held by a receiving task between arrival and the admission decision |
| `tracked` (≤ `RequestCap`) | `BlockRetriever::requested_blocks`; existing unresolved work is never evicted to admit a new hash, while queue ownership remains valid without a tracker entry |
| `unsolicited` | a one-shot full `BlockMessage` received while the request tracker is full; it may enter the byte-owning queue without consuming a hash slot |
| `pending` re-request pool | unresolved tracked hashes; no decoded block payload is retained |
| `Reannounce` | a later hash announcement or dependency scan admits previously untracked work after request capacity becomes available |
| `Defer` transition | Queue admission releases its own identity and payload. `Running::handle` reopens retriever state when a slot exists. Otherwise, another announcement or durable dependency scan must rediscover the hash. |
| `RetainedBytes + ResidentBytes` | modeled encoded payload residency, including the abstract delivery window, not total node memory |
| `BufferScanResidency.scanning` | The shared driver replaces periodic body collection with bounded candidate pages. Production integration qualification remains in progress. |
| `TransportPayloadResidency.Live` | inbound or outbound payloads whose RAII byte/item reservation has not reached its terminal owner |
| `TransportPayloadResidency.reportedSuccess` | public stream successes; membership requires completion by every target peer, not local enqueue |
| `TransportConcurrency.TransportActive` | HTTP/2 requests initiated before or after SETTINGS; bounded independently from application handler execution |
| `TransportConcurrency.Handling` | gRPC application calls holding a shared service semaphore permit before and during generated protobuf decoding; their worst-case decoded ownership is `Cardinality(Handling) × MaxDecodedBytes` |
| `TransportConcurrency.Retained` | acknowledged payloads still owned by a bounded peer queue or handler |
| `TransportPeerLifecycle.initGuards` | map-locked ownership spanning peer-slot lookup and `OnceCell` initialization |
| `TransportPeerLifecycle.sendGuards` | the single-word activity gate carried through queue residence and handling |

## Configurations

| Config | Knobs | Expected | Shows |
|---|---|---|---|
| `MC_BlockAdmission` | `ByteBounded`, `DeferralRerequests` | **clean** (CI-gated) | The fix: byte and total-residency bounds, no tracked work shed, and finite-work progress under explicit per-block fairness |
| `MC_BlockAdmission_pre_fix` | `¬ByteBounded` | `Inv_RetainedBytesBounded` violated | Historical design: a count cap admits up to `(CountCap + MaxParallel) × MaxBlockBytes` regardless of budget |
| `MC_BlockAdmission_drop_pre_fix` | `ByteBounded`, `¬DeferralRerequests` | `Live_AllBroadcastProcessed` violated | The naive fix: shedding over-budget blocks wedges the shard |
| `MC_BufferScanResidency` | `ScanBounded` | **clean** (CI-gated) | Durable buffered work is scanned one payload at a time, stays inside the total residency envelope, and eventually processes |
| `MC_BufferScanResidency_pre_fix` | `¬ScanBounded` | `Inv_ScannerSinglePayload` violated | Historical scan materializes every eligible buffered block at once |
| `TransportPayloadResidency` | byte/item bounds, compressed-wire charge, lazy chunks, completion ACK | **clean** (CI-gated) | Exact live residency, one shared fanout reservation, terminal release, and success only after remote completion |
| `TransportPayloadResidencyCountOnlyUnsafe` | no byte bound | `Inv_ActualResidencyBounded` violated | A count cap alone does not bound retained bytes |
| `TransportPayloadResidencyDecodedOnlyUnsafe` | compressed wire omitted | `Inv_ReservationCoversActual` violated | Decoded length alone undercounts simultaneous compressed and decoded ownership |
| `TransportPayloadResidencyEagerChunksUnsafe` | eager copied chunks | `Inv_ReservationCoversActual` violated | A second full wire representation escapes the reservation |
| `TransportPayloadResidencyEnqueueSuccessUnsafe` | local enqueue reports success | `Inv_SuccessRequiresRemoteCompletion` violated | A caller can observe success before any peer completes |
| `TransportConcurrency` | aligned finite wire/item window; smaller handler limit; decoded-message bound | **clean** (CI-gated) | Pre-SETTINGS bursts are accepted while application execution and pre-reservation decoder bytes remain bounded |
| `TransportConcurrencyWireLimitUnsafe` | wire limit below the client pre-SETTINGS window | `Inv_NoRequestsRefused` violated | HTTP/2 resets an otherwise valid initial request before service execution |
| `TransportConcurrencyItemLimitUnsafe` | item limit below the wire window | `Inv_NoPayloadBudgetRejection` violated | An admitted tiny request is rejected while earlier ACKed work remains live |
| `TransportConcurrencyHandlerLimitUnsafe` | service execution bypasses the handler semaphore | `Inv_PreReservationDecodedBounded` violated | Parallel generated decoders escape the configured aggregate byte envelope |
| `TransportPeerLifecycle` | guarded initialization, idle-only retirement, request-local validation | **clean** (CI-gated) | Accepted work retains an owner and parallel requests cannot exchange validation context |
| `TransportPeerLifecycleInitRaceUnsafe` | unguarded initialization | `Inv_InitializingOwnsMappedSlot` violated | Cleanup orphans a new queue between lookup and initialization |
| `TransportPeerLifecycleActiveEvictionUnsafe` | post-ACK active retirement | `Inv_AcknowledgedWorkPreserved` violated | Cleanup aborts resident or handling work after ACK; the control cannot terminate through unrelated idle cleanup |
| `TransportPeerLifecycleSharedContextUnsafe` | shared validation context | `Inv_ValidationUsesRequestContext` violated | Concurrent headers can be decided using another request's network |

Pre-fix configs are required formal counterexamples. Run the area gate from
the repository root; it bounds the TLC heap and workers, writes search state
under `target/`, verifies the safe models, and requires every named unsafe
configuration to fail for its exact advertised property:

```bash
scripts/check-cost-accounted-rho-block-admission.sh
```

## Implementation obligations

`StartupSnapshot.tla` checks captured hash membership and the startup presence-check barrier during concurrent membership changes.
Its three-key domain explores 8,032 states.
Three unsafe controls detect mutable snapshots, early admission, and selected-set pruning from live membership.
The [snapshot specification](../../../docs/casper/theory/finalized-floor/startup-snapshot-semantics.md) maps these properties to production cursor and storage tests.

`InitializerOwnership.tla` models one initializer and two concurrent critical tasks.
It separates task ownership, temporary selection ownership, abort requests, completion, and observation.
Three unsafe configurations expose selection detachment, missing cancellation on parent destruction, and cleanup before child retirement.
The safe configuration explores 4,338 states and checks eventual retirement after shutdown under fair child-cancellation scheduling.
The [recovery control specification](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md) maps this model to the actual runtime selector and native regressions.
Use `scripts/check-recovery-pump-control.sh initializer` inside a memory-limited systemd scope.

## Startup completion

`StartupCompletion.tla` separates context publication, request replacement, scan phases, callback authorization, callback return, success commit, retirement, and stop.

`StartupSnapshotLease.tla` composes those actions with reservation, capture, storage, role transfer, destruction, and exact release.
Its initial safe configuration checked 489,518 states with two contexts, three requests, and three lease identities.
Ten unsafe controls detect unleased capture, premature release, duplicate pending ownership, activation without stored work, stale release, and internal waiter allocation.
The [ownership specification](../../../docs/casper/theory/finalized-floor/startup-snapshot-ownership.md) defines each abstraction boundary and its native test obligations.
The corresponding Rocq proof derives physical resource bounds over arbitrary finite histories through an independent event ledger.
That proof covers resource conservation, not complete unbounded startup-phase refinement.
The safe configuration has two contexts and three requests, including two requests within one context.
It explores 20,219 distinct states without a liveness claim.
Ten unsafe configurations check the failure modes listed in the completion contract and gate.
They include missing callback-result ordering and stale active-slot retirement, in addition to the original eight controls.

The model permits callback execution after its prior authorization, even if cancellation occurs between those events.
Cancellation prevents later success but cannot revoke a committed result.
The [completion contract](../../../docs/casper/theory/finalized-floor/startup-completion-identity.md) records source correspondence and proof limits.
Use `scripts/check-recovery-pump-control.sh completion-formal` inside a memory-limited systemd scope.

`StartupPublication.tla` expands the local publication boundary into separate engine-lock and controller-lock steps.
Two concurrent publishers can succeed or fail while an independent stop operation changes the startup and ordinary recovery controls.
The model also separates engine destruction and Running-state reporting from the engine-slot commit.
Its safe configuration explores 624 states.
Four unsafe configurations detect premature unlock, destruction under a guard, destruction before stop signaling, and events without committed publication.
The model checks local safety, not consensus liveness or production timing.
Use `bash scripts/check-recovery-pump-control.sh publication-formal` inside a memory-limited systemd scope.

The block models require ownership of queued and active byte reservations through payload release.
They also require durable rediscovery after deferral and bounded scanner payload retention.
The local encoded-byte budget does not establish a universal consensus block-size limit or a bound on decoded process memory.
Finite-work progress assumes that the required blocks fit the local admission capacity.
Shared-driver integration must still establish the production scan and shutdown bounds.
The [verification guide](../../../docs/formal-verification.md) maps claims to tools and process conventions.

The transport models add six obligations: reserve every post-handoff wire and
decoded representation; bound pre-reservation generated-decoder ownership by
the service semaphore and checked message-size product; retain both byte and
item guards until the last owner releases; generate chunks lazily and share
fanout payloads; report success only after each remote ACK; align the finite
pre-SETTINGS wire and item windows while making slot initialization, activity,
cleanup, and request validation linearizable. The
implementation rationale and operational envelope are specified in
[P2P transport resource and completion semantics](../../../docs/node/transport-resource-lifecycle.md).

The gate checks each `Safety` conjunction with both TLC and bounded symbolic
Apalache exploration. TLC additionally checks temporal progress and executes
the exact historical negative controls.

## Recovery metadata readiness

`RecoveryMetadataReadiness.tla` models independent metadata publication and row mutation between dependency reads.
Rows can be absent, valid, or faulty.
Publication requires a valid row, but later storage faults remain possible.
Reference lists can repeat a key.
Each observation records its own admitted-set and row witness.

The safe configuration has two keys and three reference occurrences.
It explores 39,996 distinct states from all eight reference assignments.
The model checks safety, not eventual admission or consensus finality.

| Unsafe configuration | Required failure | Defect |
|---|---|---|
| `VisibilityUnsafe` | `Inv_ReadWitness` | A persisted but unpublished row authorizes retry. |
| `RowUnsafe` | `Inv_ReadWitness` | Admitted-set membership substitutes for a valid row. |
| `ErrorUnsafe` | `Inv_ErrorPreserved` | A storage fault becomes a missing dependency. |
| `EarlyUnsafe` | `Inv_CompleteOrError` | A missing dependency prevents examination of later references. |

Configuration names use the `RecoveryMetadataReadiness` prefix.
`RecoveryMetadataReadiness.v` proves eight properties over arbitrary finite observation lists and source groups.
The proof does not derive source extraction or Rust lock placement.
Native production tests establish those separate correspondence obligations.

Run `bash scripts/check-recovery-pump-control.sh metadata-formal` inside a memory-limited systemd scope with swap disabled.
The gate requires safe completion, independent Rocq kernel checking, and each specified unsafe failure.
The [read contract](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md#narrow-dependency-reads) records the implementation and resource limits.

## Active-pass demand transfer

`RecoveryPumpDemand.tla` now separates shared demand from successor demand consumed during an active pass.
Independent request-identity sets record the work owed by each location.
Begin consumes both locations, while continuation and completion preserve their newer work.
Stop closes both demand locations.

The safe configuration uses three requests and all eight proposal-flag assignments.
It explores 2,466 states and checks eventual demand service under its stated fairness assumptions.
This does not prove progress under permanently unavailable admission capacity.
The four unsafe controls detect manufactured continuation demand, lost new proposal demand, stop reversal, and discarded consumed demand.
The last control must violate `Inv_ExactNextDemand`.

`RecoveryDemandHandoff.v` proves ten merge and transfer properties, including conservation across arbitrary finite transfer histories.
Run `bash scripts/check-recovery-pump-control.sh handoff-formal` inside a memory-limited systemd scope with swap disabled.
The gate checks both the new proofs and their imported control definitions with the Rocq kernel.

## Combined dispatcher ownership

`RecoveryDispatcherComposition.tla` connects workers, services, receiver cleanup, temporary senders, recovery demand, startup captures, and proposal offers.
The configuration retains three jobs, three requests, three offers, two captures, two queue slots, two worker slots, and two visits per pass.
All eight request proposal-flag assignments are initial cases.
No validator-voting or fork-choice rule changes in this local ownership model.

The transition system separates abort submission, child destruction, resource release, and observed retirement.
Parent destruction retains an explicit receiver cleanup owner.
An external startup capture can outlive dispatcher cleanup until the outer initializer owner drains.
The model therefore distinguishes orderly dispatcher return from post-cancellation quiescence.

The original 18 negative controls passed their specified invariant checks in `run.RqbhET`.
They cover detached workers/services, unjoined worker capacity, early byte/identity/wake release, rejected wakes, sender retention, lost demand, and proposal ownership.
They also cover stale authorization, missing cancellation, premature capture reuse, count-before-load, retained scanner bodies, presence barriers, and visit exhaustion.
The gate requires checker exit code 12 and the exact invariant name for every control.
These counterexamples are not a completed positive-safety or liveness proof.

The separate `RecoveryWorkerLifecycle.v` proof passed 17 closed theorem checks and independent kernel validation in `worker.spicqK`.
It covers arbitrary finite interleavings over an arbitrary worker population.
Its scope excludes dispatcher scheduling, mailbox correctness, capacity arithmetic, and complete runtime refinement.
See the [composition contract](../../../docs/casper/theory/finalized-floor/recovery-dispatcher-composition.md) for production correspondence and remaining proof obligations.

`MC_RecoveryDispatcherComposition.tla` imports the same transition module through `INSTANCE`.
Its typed state domain supports symbolic induction without a copied transition implementation.
The candidate invariant adds exact stage-to-ledger relationships and supervisor, service, proposal, demand, and capture constraints.
The original composition completed its base and eight preservation groups in `run.2I3Pgn` and `run.oKI2lQ`.
The candidate-retention refinement passed its base and all groups in `run.bGJMP1` and `run.UFc4qp`.
The acknowledgment refinement passed its base in `run.RvSnIh` and all eight full-transition preservation groups in `run.G6E490`.
Its native batch passed 24 dispatcher tests, seven resolver tests, 26 context tests, and strict node lint in `run.tJ6mlp`.
The original unsafe controls still require qualification against this latest revision.

Each one-step check starts from the entire candidate invariant and retains the complete `Next` relation.
Only the checked invariant group changes between checks.
If every group passes, the combined proof covers every history length for the declared finite identity domains.
It does not establish arbitrary domain sizes or temporal liveness.

Use `bash scripts/check-recovery-dispatcher-composition.sh symbolic-base` and `symbolic-step` for these obligations.
The `worker-formal` and `worker-native` modes reproduce the parameterized worker proof and actual admission regressions.
The default `all` mode requires symbolic obligations, unsafe controls, worker and retry proofs, closure liveness, native integration, and admission tests.
The separate `safe` mode runs unrestricted TLC exploration.

The [dispatcher composition specification](../../../docs/casper/theory/finalized-floor/recovery-dispatcher-composition.md) defines the remaining production ownership model.
The demand result does not qualify worker cancellation, service retirement, or proposal-mailbox integration.

## Retry attempts and input closure

The combined model now retains a pending candidate independently from its body and pass-visit counter.
Temporary rejection must retain the candidate witness.
Each reload consumes a page-attempt permit without selecting another candidate.
An acknowledgment completes within the same page.
Its error outcome suppresses the pass proposal without discarding newer demand.

Six new controls cover lost candidates, premature completion, unsuspended refill, unpermitted loads, synthetic retry demand, and ignored acknowledgment errors.
Run `bash scripts/check-recovery-dispatcher-composition.sh retry-controls` in a memory-limited systemd scope.
Run `retry-formal` for the 21 parameterized Rocq theorem checks and independent kernel validation.
The proof includes empty cursor visits, pre-selection errors, pending-candidate errors, and arbitrary finite retry histories.

The `symbolic-group` mode reruns one named preservation group against the full transition relation.
This diagnostic mode does not replace the complete base and preservation checks.
All required groups must pass before the combined inductive invariant qualifies.

`RecoveryInputClosure.tla` models a persistent supervisor probe separately from worker and service progress.
Its safe configuration checked 2,324 reachable states, including parallel workers and overlapping temporary sender ownership.
Weak fairness for the timer and probe establishes eventual observation of closed, empty input.
The model does not assume worker completion.
It does not establish eventual sender-borrow release, child retirement, or a wall-clock deadline.

Run `bash scripts/check-recovery-input-closure.sh` in a memory-limited systemd scope.
Two controls must fail the temporal property by requiring a worker slot or repeatedly resetting the deadline.
A third control must fail `Inv_NoExtraBody` by dequeuing during a full-capacity probe.
The gate requires the corresponding temporal or invariant failure exit code.
The [composition contract](../../../docs/casper/theory/finalized-floor/recovery-dispatcher-composition.md) states the production policy and regression coverage.

## Deferred retry completion

`DeferredRetryCompletion.tla` models completed retry actions separately from policy persistence.
Returned success and returned error both count. Cancellation does not count.
It checks deferred ownership, coalesced policy writes, cold activation, cancellation, terminal replacement, and restart.
The safe case checked 2,538 states at depth 13 with two request incarnations and two concurrent operation permits.
Seven controls cover lost completions, stale reloads, double charges, terminal writes, network resends, excess permits, and success-only counting.

Run `bash scripts/check-buffer-publication-ownership.sh deferred-completion` in a memory-limited systemd scope.
The gate requires exact invariant failures for all controls and success for the safe case.
Its result is a bounded component check, not proof of the complete Rust implementation.
Restart retains committed policy only.
The [publication contract](../../../docs/casper/theory/finalized-floor/buffer-publication-ownership.md) maps the invariants to required production regressions.

## Pruning and cold durable obligations

`BufferPruningObligations.tla` separates durable retry rows from resident cache entries and request records.
Four safe graph configurations use three blocks, one certificate, and two resident slots.
Four named controls cover unresolved-edge deletion, last-owner deletion, page-local certificate cleanup, and eager reconstruction.
The first two controls reproduce production defects.
The other controls constrain the proposed paging migration.

`BufferPruningPreservation.v` proves the component ownership and unresolved-edge rules for arbitrary key types and finite operation histories.
It permits conservative extra edges.
It does not prove backend transaction atomicity, complete block publication, snapshot ownership, or eventual paging.

Run `bash scripts/check-buffer-pruning-obligations.sh formal` inside a memory-limited systemd scope.
The default mode also requires the actual preservation regressions and strict storage lint.
The [pruning contract](../../../docs/casper/theory/finalized-floor/buffer-pruning-preservation.md) states the source traces, proof assumptions, and incomplete production work.
