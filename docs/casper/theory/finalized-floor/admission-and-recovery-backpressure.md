# Admission and recovery backpressure

## Status and scope

This document records the repair plan for pgmcp task `pr216-admission-backpressure`.
The source inspection date is September 6, 2026.
The actor-service and compact-result repairs passed focused native checks.
Actor symbolic reachability through length eight and the separate induction check passed.
The durable buffer, retention epoch, and candidate-index repairs passed their combined focused gate.
Admission identity ownership passed its combined gate and the focused producer follow-up.
Shared retry, dispatcher lifetime, uniform quarantine, and preservation composition remain incomplete.
The plan-agent review is complete.

The [actor service document](recovery-actor-service.md) records the first repair and its verification limits.
The [payload ownership document](payload-reservation-ownership.md) records the completion-body repair and remaining ownership obligations.
The [durable membership document](buffer-durable-membership.md) defines the buffer persistence correction required before bounded candidate indexing.
The [pruning preservation contract](buffer-pruning-preservation.md) records the remaining eviction defects and the required durable paging design.
The [identity document](admission-identity-ownership.md) defines queued and active cleanup ownership.

Backpressure limits new local work when a resource has no available capacity.
It must not change block validity or remove an unresolved consensus dependency.
This task preserves voting, certificate thresholds, fork choice, block bytes, and cost settlement.
Independent validators and independent block workers must remain concurrent.

The inspection includes uncommitted changes above feature commit `559eb07fac98da2e6392e3a84c93bac41558c86c`.
The inspected local `dev` commit is `62fa58f1183630d08919e4957d29018ecd1b3bcb`.
That local reference is not a claim about the latest remote revision.

## Confirmed source findings

| Boundary | Current behavior | Required correction |
|---|---|---|
| Periodic dependency retry | `running.rs::enqueue_dependency_free_blocks` loads all eligible block bodies before admission. | Load bounded candidates incrementally through the same ownership path used by other producers. |
| Dependency enumeration | `buffer_resolver.rs` copies candidate sets and the buffer DAG, then checks every candidate in one call. | Bound temporary enumeration and work per turn without dropping unresolved candidates. |
| Completed results | `BlockProcessorInstance::create` sends a full block through the result queue, then releases its admission reservation. | End payload ownership before release or transfer the reservation with every retained payload. |
| State requester selection | The biased selector always checks chunks before commands and retry ticks. | Give each continuously ready service lane a bounded number of intervening service turns. |
| Failure quarantine | The worker scan checks quarantine, but the periodic retry helper does not. | Apply one retry-eligibility rule at the shared processing boundary. |
| Background-task shutdown | The processor retains its own input sender, and the requester timer remains enabled after both input channels close. | Give each task an explicit termination owner and verify release of queued and active work. |

These findings follow from source inspection.
They do not establish that each finding caused a particular historical continuous-integration failure.
Each repair requires an executable counterexample before implementation.

The result queue retains up to 128 results.
Its sole production consumer discards each result in `node_runtime.rs`.
The inspected local `dev` also sends a full block in each result tuple.
Removing that payload would repair shared retention, not restore an existing `dev` result type.

The admission budget measures encoded block length.
It does not measure decoded allocations, serialization temporaries, retained clones, allocator overhead, or resident set size.
The configured admission limit comes from the transport stream limit in `setup.rs`.
No consensus block-size rule in `validate.rs` was established by this inspection.

## Existing controls to preserve

The block queue already reserves encoded bytes before nonblocking count admission.
The queue transfers each reservation to its worker.
Failed admission releases its reservation.
Tracked blocks remain available for later requests.
Untracked deferrals require another announcement or discovery from durable dependencies.

`RecoveryWindow` already limits tracked keys and rotates ready-key selection.
Its private dispatch identities prevent stale completion from changing a replacement entry.
Certificate and state requests already use finite transport deadlines.
These controls remain requirements, not replacement targets.

The [recovery budget document](recovery-budget-episodes.md) defines these identity and retry rules.
The [certificate retrieval document](finalization-certificate-retrieval.md) defines certificate ownership and dependency recovery.

## Planned repair boundaries

### 1. Payload and processing ownership

Use a compact completion result because the current consumer does not need the block body.
Preserve the processing status and any identifier required for diagnostics.
Remove the redundant worker-owned block copy where ownership can transfer directly.
Release the reservation only after all corresponding worker payloads have been released.

Place queued and active duplicate suppression under an explicit ownership contract.
Test failed enqueue, closed receiver, cancellation, panic, success, and processing error.
An old owner must not release a newer owner's identity.
Do not replace exact identity ownership with unrelated time-based eviction.

The processor dispatcher must not depend on input closure while retaining its own input sender.
Give the dispatcher and workers explicit cancellation and completion ownership.
Cancellation must release queued identities as well as active identities.

### 2. Bounded dependency retry

Use one shared retry service for startup, maintenance, certificate wakeups, and worker completion.
Coalesce repeated wakeups into one pending request for a bounded pump turn.
Retain durable dependency data as the source of unresolved work.
Keep only bounded candidate pages and one loaded scanner payload outside admitted work.
Release storage guards before block loading, network work, and asynchronous waits.
Release block-worker permits before independent pump work.

The scan must resume fairly instead of restarting from the same preferred prefix.
New arrivals must not repeatedly overtake an existing eligible candidate.
Capacity failure must leave the candidate discoverable.
Quarantine expiry must restore eligibility without another network announcement.
A full count queue must not trigger a complete payload-loading pass.

Maintain a process-local candidate index under the existing buffer mutation guard.
Rebuild that index from durable metadata without changing stored encodings.
Define its cursor and mutation semantics before coding.
Required cases include insertion, removal, duplicate wakeup, missing body, certificate completion, partial failure, and restart.
Hash-only materialization does not establish bounded enumeration work by itself.

### 3. Shared retry eligibility

Apply the existing quarantine rule to every producer through the common processing boundary.
Check eligibility before expensive replay and before duplicate ownership transfers.
Retain the block and its dependencies when retry is temporarily unavailable.
Keep local failure separate from objective invalidity.

The check must propagate a failed quarantine lookup.
It must not interpret a local lookup error as permission to retry.
Use explicit monotonic-time boundaries in examples and generated tests.

### Shared-pump implementation review

The plan-agent review specifies one supervisor-owned driver with bounded synchronous pages.
Startup, periodic maintenance, certificate completion, and worker completion must share that turn.
No caller creates a task or waiter for each wakeup.
External callers request work instead of executing turns.
This restriction removes the shared scan gate and its waiting callers.
The [control specification](recovery-pump-control.md) records the implementation contract and current evidence.

The turn rotates one candidate immediately before examination.
It checks existing ownership, quarantine, certificate dependencies, admitted metadata, and queue capacity before admission.
It loads at most one scanner body and drops a rejected body before the next examination.
Each turn has an explicit candidate-visit limit.

The dependency predicate needs a narrow authoritative metadata lookup.
`BlockDagKeyValueStorage::get_representation_internal` materializes latest-message, invalid-block, and equivocation maps.
Constructing that representation for each candidate would defeat bounded metadata work.
The narrow lookup must retain the exact dependency-hash semantics of `all_dependencies_have_admitted_metadata`.

A wake request sets pending work and preserves any proposal demand.
The driver consumes demand before starting a pass, not after completing it.
The notification mechanism must preserve requests between the driver's last check and sleep.
The driver must yield between ready continuation pages.

Capacity release and quarantine expiry require independent wake sources.
An unsuccessful complete pass must wait for another event or deadline.
Buffer nonemptiness alone must not trigger repeated immediate scans.
Byte release must wake retries only for previously admitted ownership.
A failed count admission also releases temporary bytes, so waking on every raw reservation drop creates a self-wake loop.

The supervisor must not retain its own strong queue sender while waiting for input.
It needs a weak admission endpoint and sender-independent metrics and wake state.
Startup must register a weak Casper context before the first completion wake can occur.
An owned worker set must make supervisor cancellation reach queued and active ownership.

The optional post-block proposal trigger awaits an actual proposer result.
The sole dispatch or retry loop must not await that result.
Its completion path needs a separate bound without reducing independent validation concurrency.
Retain the existing feature flag, finalized-floor bond check, `PendingDeploy` request kind, and proposer coalescing.

The old worker suppresses proposal triggering when its complete dependency scan fails.
A successful bounded page does not establish that a later page will succeed.
Preserve that scope with a bounded full-pass continuation and an aggregated error outcome.
Do not introduce an unrestricted waiter list or silently replace full-pass suppression with per-page suppression.

The new control models cover lost wakeups, capacity-release self-wake, full-pass continuation, newer proposal demand, and staged shutdown.
Quarantine expiry and full production composition remain incomplete obligations.
The single-driver design removes the former competing-gate scenario by construction.

Startup needs one context-bound completion ticket because its proposal and error policies differ from worker completion.
The reviewed startup sequence must retain its own callback after successful startup scanning.
The runtime now retains initialization ownership when another `select!` branch wins.
Its separate `JoinSet` and shared abort/drain path passed formal-first checks, production-linked regressions, generated event sequences, and strict node lint.
The [control specification](recovery-pump-control.md) records the evidence and cancellation limits.
The startup completion ticket and bounded driver integration remain incomplete.

### 4. Fair state-requester service

Replace fixed item priority with explicit rotating service across chunks, owner commands, and retry ticks.
Keep the existing bounded concurrent request batch and finite transport deadline unless tests establish a need for further separation.
Do not add an unbounded request-task queue.

Distinguish ready-key fairness inside `RecoveryWindow` from service fairness in its enclosing actor.
Both layers need evidence.
A continuously ready chunk lane must not prevent owner release or retry.
Closed command and chunk channels must permit actor termination despite the periodic timer.

A service-turn bound is not a wall-clock deadline.
Time bounds require terminating importer calls, finite transport deadlines, and eventual task scheduling.
State these premises beside each liveness claim.

Admission liveness also needs available peers and blocks that fit the local capacity.
Finite-work progress does not prove starvation freedom for unequal block sizes under continuous new traffic.
Define that stronger workload guarantee before adding any capacity-priority policy.

### 5. Preservation and composition audit

Check buffer pruning against unresolved dependency preservation before claiming end-to-end retry safety.
`enforce_limits` can remove buffer entries because of age or pressure.
Determine which removed obligations remain discoverable and test the exact recovery path.
Do not replace this analysis with unconditional eviction or unlimited resident retention.

Check ordinary block retries and certificate maintenance together.
The current block retriever processes its selected hashes before certificate maintenance.
Measure and model slow-peer delays before changing its dispatch policy.
Separate bounded delay from proven starvation.

Check every reservation transition through transport, decoding, admission, worker processing, completion, and release.
Report encoded-payload bounds separately from temporary allocations and whole-node memory.
Do not introduce a local capacity setting as a new consensus validity rule.

## Formal-first verification

Extend `BlockAdmission.tla` and `BufferScanResidency.tla` for every affected ownership boundary.
Reuse `RecoveryBudgetEpisodes.tla` for dispatch identity and exact owner semantics.
Add the enclosing actor and concurrent producer transitions explicitly.
Do not assume fairness for a production action that fixed-priority selection can disable indefinitely.

Prove parameterized selection, reservation, and ownership rules in Rocq where finite model checks cannot establish arbitrary cardinality.
Check the original initial state and each claimed inductive invariant separately.
State the complete implementation correspondence for every modeled transition.

| Obligation | Required executable evidence |
|---|---|
| Exact reservation conservation | Production-linked Loom and generated operation sequences, including overflow and cancellation. |
| Bounded scanner retention | Actual scanner tests with counted live payloads and large candidate sets. |
| Resumable fair selection | Properties over arbitrary page sizes, concurrent insertions, removals, and failed admissions. |
| Uniform quarantine | Tests through every producer, exact expiry boundaries, and lookup failures. |
| Actor service fairness | Deterministic scheduler tests with continuous chunk traffic and pending cleanup and retry work. |
| Shutdown | Closed-channel and cancellation tests that prove actor and dispatch-owner release. |
| Recovery preservation | Durable buffer, restart, certificate wakeup, and pruning counterexamples. |
| Validator concurrency | Overlapping producer and worker tests with no global consensus lock. |

The existing `loom_block_admission.rs` contains a separate budget implementation.
It does not establish production correspondence by itself.
Use the actual repaired production kernels in new Loom tests.
Keep native asynchronous tests for behavior that those kernels do not represent.

Negative controls must fail their named invariant or temporal property.
Parsing errors, timeouts, resource exhaustion, and empty test selections are not successful negative controls.
Preserve the original failing execution and its source hashes.

## Completion criteria

Record each source counterexample, formal obligation, production symbol, test, command, input hash, result, and limit.
Complete the production changes as one reviewed set before expensive native qualification.
Reuse unaffected evidence and repeat changed or missing checks only.
Run all heavy commands within explicit systemd memory limits with swap disabled.
Keep temporary databases and proof state outside `/tmp`.

This task does not establish whole-node memory bounds or month-long uptime.
The separate runtime-retention, measurement, integration, and soak tasks retain those obligations.
Local completion requires evidence for every repair boundary above.

## Inspected source identities

```text
0fe7eebe5b65f8b203c920c9f1d09f31c82ef192d062b4c8c11b52282578ac5c  casper/src/rust/blocks/block_processing_queue.rs
277fc617f7b0b22e3c28f18b0d2e4646f6aab507a8663dddcbeeb7e6748e2fb0  casper/src/rust/engine/running.rs
6dd3ae00b8531a36476d094663ecfaf86678a7cc25fda819938364c6da54d4c4  casper/src/rust/engine/runtime_state_requester.rs
e3099c34109dc8d9e674847ecab0ce855a18980a64b5dbc84c35ee3523855e31  casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs
993ec19dcebf17caa173f7c9bf6945408d398d14f1869fac43276da26aa3eb27  node/src/rust/instances/block_processor_instance.rs
```
