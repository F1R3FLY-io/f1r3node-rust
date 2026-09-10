---
task_id: pr216-admission-backpressure
status: in_progress
handoff_status: active
next_steps:
  - Complete the actor verification and gate integration.
  - Model and repair the remaining ownership and bounded-scanner boundaries.
  - Verify quarantine, cancellation, pruning, and maintenance composition.
---

# Admission and recovery repair progress

## Scope

The [reviewed plan](../casper/theory/finalized-floor/admission-and-recovery-backpressure.md) defines this task.
The [actor service document](../casper/theory/finalized-floor/recovery-actor-service.md) explains the first production repair.
The task remains incomplete.
No vote threshold, fork-choice rule, consensus validity rule, or cost-settlement rule changed in this repair.

## Completed work

The state requester now rotates between chunks, commands, and retry ticks.
Both channels drain before actor shutdown.
The timer no longer retains the actor after both channels close.
Concurrent bounded request batches and existing transport deadlines remain unchanged.

The formal model preceded production changes.
Rocq proved selection and service bounds for arbitrary positive lane counts and finite service histories.
TLC checked independent actors and both unsafe controls.
Production-linked tests use the actual selection and inbox modules, not copied implementations.

## Evidence

All evidence paths below are relative to `target/verification/pr216/admission-backpressure/`.
Each driver records SHA-256 input hashes and checks those hashes when the command exits.
The transient evidence remains on disk, outside the memory-backed `/tmp` filesystem.

| Evidence | Result and resource limit |
| --- | --- |
| `actor-tlc.9r4Rfi` | Initial single-actor and two-actor models passed safety and shutdown liveness. Both named unsafe controls failed as expected. The scope limit was 2 GiB. |
| `actor-rocq.YCDwX9` | All 11 theorem assumptions were closed. Independent `coqchk` passed. The scope limit was 2 GiB. |
| `actor-native.Xs6IPn` | Twelve reported tests passed across two targets. Three selector tests occur in both targets. Strict Clippy passed. The scope limit was 4 GiB. |
| `actor-apalache.r7wyTy` | The JVM rejected conflicting garbage-collector options before model checking. This attempt provides no verification evidence. |
| `actor-apalache.XxotZ8` | Reachability through length eight and the subsequent one-step induction check passed. Both logs report `EXITCODE: OK`. The scope is inactive, and recorded hashes match. The scope limit was 3 GiB. |
| `actor-casper.wuRwoG` | All 11 focused Casper requester tests and strict library Clippy passed. The scope limit was 5 GiB. |
| `actor-induction.NIo98h` | The strengthened actor safety predicate passed one-step Apalache induction for two actors and capacity two. The scope limit was 2 GiB. |
| `../../recovery-actor-service/run.aHuXzy` | The permanent TLC gate passed both safe domains and both named unsafe controls. All recorded hashes matched. The scope limit was 2 GiB. |
| `payload-tlc.vheiUU` | The payload model passed 10,893 states. Result-body and worker-tail controls each violated exact reservation coverage. The scope limit was 1 GiB. |
| `payload-rocq.kxIDpQ` | All 13 ownership theorems compiled with closed assumptions. Independent `coqchk` passed. The scope limit was 1 GiB. |
| `payload-native.ltwXrk` | Four production-budget Loom tests, strict Loom Clippy, and nine native admission tests passed. Node compilation found a missing deterministic host-work error mapping. The scope limit was 5 GiB. |
| `payload-node.chSChb` | After the mapping repair, all 20 focused node handler tests and strict node and Casper library Clippy passed. All recorded hashes matched. The scope limit was 5 GiB. |
| `buffer-tlc.3zYz6A` | The two-block, one-certificate, two-client domain passed 4,008 states. All five unsafe controls violated their named invariant. The scope limit was 2 GiB. |
| `buffer-rocq.KbhEBw` | All 15 row-semantics theorems had closed assumptions. Independent kernel checking passed under a 1 GiB limit. |
| `buffer-red.mCrNvx` | Seven new regressions reproduced the old storage defects. One dependency-preservation test passed. This was the required failing baseline, not a successful repair check. |
| `buffer-native.BhJHa1` | Nineteen buffer tests and twelve atomic-transition tests passed. A missing `move` capture prevented Loom compilation. This run did not qualify Loom. |
| `buffer-native.O8wnqI` | Twenty buffer tests, twelve atomic-transition tests, two production-publication Loom tests, and both strict Clippy checks passed. The scope limit was 5 GiB. |
| `../../buffer-durable-membership/run.oimpzM` | The combined gate passed 30 Rocq theorems, four safe TLC domains, and ten named unsafe controls. Native counts were 31 buffer tests and 12 atomic-transition tests. Both Loom targets and strict lint passed. Four candidate-native tests also occur in the candidate Loom target. Hashes matched. The scope limit was 5 GiB. |
| `../../admission-identity/run.bkBK3B` | A proof tactic failed before model checking. This attempt does not establish formal correctness. |
| `../../admission-identity/run.vpNver` | Ten Rocq theorems and independent kernel checking passed before production edits. TLC explored 1,400 safe states. All three controls violated exact ownership. Hashes matched. The scope limit was 2 GiB. |
| `../../admission-identity/run.fXkfnt` | Four production-registry Loom tests, eleven queue tests, and the lifecycle regression passed. Strict lint rejected two unused imports from the type migration. The scope limit was 5 GiB. |
| `../../admission-identity/run.m941MA` | The expanded model passed 2,648 states and four named controls. Twelve Rocq theorems passed kernel checking. This evidence preceded the early-rejection repair. The scope limit was 2 GiB. |
| `../../admission-identity/run.5yJS7h` | The combined gate passed twelve Rocq theorems, 2,648 safe TLC states, four named controls, four Loom tests, eleven queue tests, and the expanded lifecycle test. Strict Casper/node library and test lint passed. Recorded hashes matched. The scope limit was 5 GiB. |
| `../../admission-identity/run.8mFyyV` | The lifecycle test and both existing delivery regressions passed after the deterministic fixture correction. Strict Casper integration-test lint passed. Recorded hashes matched. Production source, formal models, and Loom code were unchanged from the preceding passing gate. The scope limit was 5 GiB. |

The combined active verification limits do not exceed 8 GiB.
Swap is disabled for these scopes.

## Remaining obligations

The payload repair now emits only the block hash and processing status.
It transfers the block into processing instead of retaining a worker clone.
The existing admission reservation remains held until processing and result publication finish.
The result queue no longer retains the corresponding block body.

The admission CAS loop and reservation destructor now form a production module that Loom imports directly.
The extraction retains the same arithmetic, atomic orderings, and metric updates.
Its focused native and concurrent checks passed.
Full worker cancellation and retained-payload qualification remain necessary.

The abstract payload proof measures logical encoded payload ownership, not physical clone multiplicity or resident memory.
It does not establish duplicate-marker identity or durable retry preservation.

The actor results do not qualify duplicate ownership, the shared scanner, or quarantine boundaries.
Those boundaries require their own counterexamples, formal correspondence, production repairs, and tests.
Buffer pruning and ordinary-retriever interference still require the preservation audit in the reviewed plan.
The separate integration and soak tasks remain necessary.

## Buffer persistence review

The plan agent completed the bounded candidate-index review.
The review found four durable-membership defects in the existing buffer implementation.

1. Restart reconstruction ignores rows with empty parent sets.
2. `put_pendant` does not retain a durable row after success.
3. Resolving the final dependency deletes the newly ready child's row.
4. Removing a child can delete an orphaned parent's row despite that parent's unresolved dependencies.

The relevant defects also exist in pinned local `dev`, commit `62fa58f1183630d08919e4957d29018ecd1b3bcb`.
This comparison does not describe an unexamined remote revision.
Removal also changes memory before fallible, separately committed storage writes.

The planned correction preserves explicit buffered identities as rows, including empty parent sets.
Reference-only missing roots remain distinct from explicit buffered identities.
One atomic mutation commits the affected rows before memory publication.
The repair must not clone the complete graph to prepare that mutation.

The FIFO index must rotate candidates when the scanner examines them.
Rotating a complete page before an early capacity exit can repeatedly skip later candidates.
New entries must not move ahead of existing entries.
The index bounds temporary scan work, not total memory for all live buffer identities.

The durable-membership repair now uses strict atomic row mutations before memory publication.
Generated operation histories check rows and both graph directions against an independent reference after every operation.
The histories include duplicates, cycles, injected transaction failures, and restart.
The real LMDB regression exits its child process immediately before commit and immediately after commit, before memory publication.
Reopening must expose the complete old state or complete new state, respectively.

Loom imports the production commit-before-publication helper.
It checks two competing writers, a reader, and all combinations of transaction success and failure under the shared mutation guard.
A negative control releases that guard between commit and publication and must fail its named assertion.
This Loom model checks publication control flow, not the graph implementation or LMDB internals.
Native reference properties and separate backend transaction tests cover those boundaries.

The second storage review identified two retention defects.
Restored identities lacked an age origin, and pressure counts omitted isolated ready identities.
Both new regressions failed before the repair.
The repair seeds a startup-local age epoch and counts the disjoint live-identity partition.
It does not persist age across restarts or prove the complete pruning policy.

The candidate index now rotates one identity per examination.
The combined gate checked 1,641 candidate states, including conditional service liveness, and 2,300 retention states.
Durable membership passed the 4,008-state and 393,044-state domains.
No production retry path yet uses the index.

## Admission identity repair

The source audit found queued marker leakage when a receiver drops before worker dequeue.
The former cleanup guard existed only inside the worker.
The new queue lease owns both the byte reservation and an exact admission identity.
All producer paths use queue-owned duplicate admission instead of inserting or clearing bare hashes.
The registry keeps hash-sharded metadata locks outside asynchronous processing.

The worker receives the complete queue item before splitting ownership.
Its tuple bindings place the lease before the body, so cancellation drops the body first.
The native lifecycle test observes actual block-byte destruction through `Bytes::from_owner`.
It tests receiver destruction, unpolled cancellation, active cancellation, panic, error, and success.

The new model verifies identity safety, not supervisor shutdown or retry progress.
The shared pump still needs coalesced wakes, bounded turns, uniform quarantine checks, and explicit dispatcher ownership.
The plan agent identified release-triggered self-wake and later-page proposal-suppression cases for that integration.

The identity implementation review found an early-rejection drop-order gap.
Rust drops local variables before function arguments.
The rejected body was still an argument when the local identity dropped.
The repair places the body in a local variable after the identity before fallible sizing and reservation.
The expanded model exposes this ordering as a separate unsafe control.

The review also found that the fixture inspector dropped leases, retained bodies, and silently retried enqueue.
The inspector now reads admission identities without moving queue items.
The expanded native regression checks both corrections, concurrent duplicate enqueue, and nested-future cancellation.
All known type-migration unused imports were removed before the combined gate.

The combined identity gate passed.
The later fixture audit found that an existing block's random encoding could exceed the capacity derived from another random block.
The fixture now clones the measured body and changes only its fixed-length block hash.
This correction makes the intended byte-capacity branch deterministic.
The focused follow-up also runs the existing direct-delivery and duplicate-delivery engine regressions.

## Payload allocation audit

The network parser decodes `packet.content.as_ref()` in `casper/src/rust/protocol/mod.rs`.
The block store decodes its decompressed byte slice in `block-storage/src/rust/key_value_block_store.rs`.
Neither inspected path passes an owned `Bytes` buffer into protobuf decoding.
This inspection does not establish a shared backing allocation between the decoded hash and the complete block payload.
No speculative hash-copy repair was applied.

## Retry control qualification

The final plan review replaced competing scan callers with one owned retry driver.
Only the driver executes pages.
External callers publish coalesced requests, and continuation does not manufacture external demand.
The [control specification](../casper/theory/finalized-floor/recovery-pump-control.md) records the complete contract and its unfinished integration boundaries.

The formal checks preceded the production wake and pass modules.
The later pass model also includes competing admissions.
Its completion property requires eventually sustained usable capacity rather than assuming serial resource use.

| Evidence under `target/verification/recovery-pump/` | Result |
|---|---|
| `run.nUh9hG` | Fourteen Rocq proofs passed. A TLA+ successor was incomplete because a Boolean assignment lacked parentheses. The strict negative-control gate rejected this attempt. |
| `run.e717cW` | Twenty Rocq proofs and independent kernel checking passed. Four safe models and all nine named unsafe controls passed their expected checks. The scope limit was 2 GiB. |
| `run.lb0UVB` | The expanded pass model included competing admissions. Safe state counts were 11,124 for wake delivery, 123 for passes, 356 for dispatcher ownership, and 194 for demand. All nine controls failed their intended invariant. The scope limit was 2 GiB. |
| `run.JE5HVl`, `run.A3KJSZ`, `run.nwM2VR` | The initial Loom notification approximation failed. Diagnostic runs preserved the same failure. These attempts do not qualify the native gate. |
| `run.Jkpyw2` | Four production-wake Loom tests, four generated/example tests, fifteen queue/control tests, and the actual lifecycle regression passed. Strict Casper/node library and test lint passed, as did strict standalone test lint. Recorded hashes matched. The scope limit was 5 GiB. |

The failed notification approximation used a release store and an acquire-release swap.
The dependency source uses sequentially consistent read-modify-write operations for its stored notification token.
The corrected Loom adapter models that boundary without changing the production wake kernel's orderings.
Actual Tokio tests cover notification before wait, stop during wait, and wait cancellation.
The approximation's failure is not evidence of a deployed node defect.

The queue now arms its release wake only after nonblocking count reservation succeeds.
Rejected staged items release their bodies, bytes, and identities without producing retry demand.
Admitted lease release requests work after byte and identity release.
The weak enqueue endpoint does not keep the input channel open.
The driver does not yet consume these requests.

## Startup integration review

Startup needs a distinct, single-owner completion ticket.
Its callback runs with autopropose even when heartbeat is disabled.
The post-block callback has additional configuration and bond checks.
The shared driver must not conflate these paths or their error policies.

A ticket must bind a private context identity and a private request identity.
Only its corresponding pass can report success or failure.
Cancellation, replacement, or shutdown must not complete another ticket.
One pending startup slot bounds waiters independently from ordinary fire-and-forget retry requests.

The old startup scan snapshots pendants before reading their bodies.
The next implementation must explicitly resolve this membership scope against concurrent candidate rotation.
It must not claim snapshot equivalence from a visit-count bound alone.
Capacity parking must preserve error-before-proposal ordering across all startup pages.

The source review found a runtime initialization ownership defect at `node_runtime.rs:817`.
The `select!` branch takes the initialization handle before awaiting completion.
If another branch wins, the dropped branch detaches initialization and leaves the stored option empty.
The correction needs a failing regression, cancellation-safe handle observation, and explicit shutdown ownership.
The following qualification preceded the runtime initialization correction.

## Initializer ownership repair

The plan agent confirmed the ownership defect and reviewed the production integration.
The repair uses a separate, one-member initialization `JoinSet`.
The extracted runtime module contains the actual event selection and concurrent abort/drain operations.
The runtime retains its existing logging, startup sealing, and error-return policies.
The initializers and critical tasks receive abort requests before either drain begins.

Pinned `dev` revision `62fa58f1183630d08919e4957d29018ecd1b3bcb` contains the same take-before-await pattern at `node_runtime.rs:809`.
Its shutdown path also drains only the critical-task set.
This comparison establishes that the initializer defect exists in that inspected `dev` revision.
It does not establish the state of a newer remote revision.

The formal model separates live children, completed results, temporary selection ownership, abort requests, observation, and supervisor destruction.
Three unsafe controls expose lost selection ownership, missing abort-on-drop, and premature cleanup claims.
Eight Rocq lemmas prove ownership preservation over arbitrary finite histories.
The safe TLA+ model contains one initializer and two concurrent critical tasks.

| Evidence under `target/verification/recovery-pump/` | Result |
|---|---|
| `run.vwaJLk` | The final earlier control gate passed. Input hashes matched. |
| `run.N6ROc1` | Before production edits, eight closed Rocq proofs, independent kernel checking, 4,338 safe TLA+ states, and three named unsafe controls passed. Three native tests reproduced the legacy defect and checked Tokio ownership behavior. |
| `run.sJAbgT` | After production integration, the formal checks and all ten native tests passed. Strict test lint rejected two redundant async blocks. This attempt did not qualify the complete gate. |
| `run.XVehBC` | All ten corrected native tests, strict standalone lint, and strict node library/test lint passed. Input hashes matched. The scope limit was 5 GiB with swap disabled. |

The generated test imports the production selector and mixes competing events before initialization completes.
Other production-linked tests observe captured resource destruction and cancellation before the first child poll.
The shutdown race test accepts either legal event order and rejects duplicate observation.
The test corpus does not establish exhaustive scheduler coverage of Tokio internals.

The runtime's 30-second shutdown timeout remains unchanged.
An abort request does not establish immediate retirement of a non-yielding child.
Nested block-processor tasks still need the separately planned ownership migration.
The bounded retry driver, startup ticket, uniform quarantine, and pruning composition remain incomplete.

## Persistent startup snapshot

The plan agent confirmed that the live FIFO cannot represent the old startup snapshot.
The chosen representation changes only `BlockDependencyDag::dependency_free` to the existing persistent ordered set.
The ordinary FIFO and durable buffer encoding remain unchanged.
The [snapshot specification](../casper/theory/finalized-floor/startup-snapshot-semantics.md) records semantics and retention limits.

The review rejected consuming iterators after inspecting pinned `imbl 7.0.1` source.
Hash-collision copying and eager leaf-pointer collection would violate the intended bounded-step claim.
The implementation instead retains an immutable root and an exclusive key cursor.
Root capture copies no keys, and traversal copies only the returned key and cursor key.

Formal checks preceded the representation change.
`run.AZactJ` passed the six Rocq proofs but failed TLA+ initialization because the set-valued variable lacked an enumerating constraint.
The gate rejected this attempt rather than accepting an invalid negative control.
`run.M2IU3P` passed six closed proofs, independent kernel checking, 8,032 safe states, and all three named unsafe controls.

`run.DqKxe8` and `run.dI6YQ3` failed compilation because the pinned collection API needed explicit generic and integer types.
No semantic assertions were weakened.
`run.l78vIC` passed four cursor property/example tests and one Loom boundary test.
It also passed 32 native buffer tests, 11 dependency-DAG tests, strict storage library/test lint, and strict standalone lint.
The terminal exit code was zero, and all input hashes matched.
The scope used a 5 GiB limit with swap disabled.
The snapshot API is not yet connected to the bounded startup driver.
The two-phase presence/error barrier and context-bound completion remain required.

## Two-phase scan and completion identity

`run.9YtYBi` passed the first five production scan tests and strict standalone lint.
The process exited successfully, and the input manifest matched during the later status check.
The scanner preserves the presence-error barrier before admission and retains one pending hash during capacity parking.

The completion plan agent reviewed context registration, lazy proposal callbacks, engine publication, and startup-result ownership.
Its review requires one atomic authorization event and a separate atomic success commit.
An authorized callback can execute after replacement, but its old request cannot subsequently commit startup success.
The design preserves the existing generic proposal routing and `PendingDeploy` request kind.
It introduces no validator serialization or consensus lock.

The new [completion contract](../casper/theory/finalized-floor/startup-completion-identity.md) records the complete local design.
The production transition kernel implements exact phase, context, request, active-owner, and terminal-state checks.
The surrounding ticket wrapper and engine publication integration remain incomplete.

| Evidence under `target/verification/recovery-pump/` | Result |
|---|---|
| `run.ItOEnx` | Fourteen Rocq proofs and independent kernel checking passed. The model gate rejected an incompletely assigned unsafe transition caused by missing parentheses. This attempt does not qualify the model. |
| `run.hKS8QM` | Fourteen closed proofs, independent kernel checking, 13,525 safe states, and all eight named unsafe controls passed before production implementation. |
| `run.n74VfX` | The shutdown refinement no longer assumes that stop removes the engine pointer. Fourteen proofs, independent kernel checking, 17,181 safe states, and all eight controls passed before production implementation. |
| `run.jkbxzf` | Seven scanner tests, five completion property/example tests, five completion Loom tests, and strict standalone lint passed. Input hashes matched. The scope limit was 3 GiB with swap disabled. |

The property corpus uses an independent event log and generated stale/current/active identity selections.
Stop has an optional generated position, so it does not truncate every generated history early.
The scanner corpus now explicitly generates all-present, all-absent, and mixed body-presence results.
This change replaces independent random sets that rarely overlapped and therefore under-exercised admission.
The examples also cover an admission error after earlier successful work and an absent body that appears after filtering.

These checks qualify the component transitions, not the complete recovery pipeline.
The generic kernel requires fresh identities from its future private-allocation wrapper.
The runtime must still place engine registration and publication under the same publication guard.
The controller must place stop and callback authorization under the same control guard.
The driver must still own active resources through cancellation and connect both scan phases to real storage and admission.

## Completion review corrections

The plan agent found no reachable Rust guard violation under the fresh-identity contract.
It found four correspondence or coverage gaps before wrapper integration.

1. TLA+ required callback return, but the first Rust kernel relied on a future caller to enforce that precondition.
2. The first Rocq success function permitted an unreachable callback-free authorized state that Rust rejected.
3. TLA+ cleared current records during cancellation and stop, while Rust retained terminal records.
4. The property reference compared request identifiers alone in two operations and did not generate wrong-context pairs.

The correction adds an explicit callback-success observation before startup success.
It tightens the Rocq guard, retains terminal records in TLA+, and compares both identity components in the independent reference.
Generated histories now include wrong-context pairs with an unchanged request identifier.
A deterministic regression probes those pairs at every phase and ownership boundary.
The actual wrapper must still obtain the observation from a successful callback future.

`run.sCHV1o` passed all preceding native component tests and strict Casper library/test lint before these review corrections.
Its full lint run completed in four minutes and eleven seconds within a 5 GiB scope.
The documentation syntax check passed for 353 files.

`run.69JKWW` qualified the reviewed semantics before the production correction.
It passed seventeen closed Rocq proofs, independent kernel checking, 20,219 safe TLA+ states, and ten named unsafe controls.
The two added controls reject missing callback-result ordering and stale active-slot retirement.
All recorded input hashes matched, and the scope used 2 GiB with swap disabled.

`run.PGvaHd` passed the reviewed tests but failed strict standalone lint on a Boolean expression in the independent reference.
The correction used the equivalent `is_none_or` expression without changing the tested contract.
`run.tHOG5S` then passed seven scanner tests, six completion property/example tests, five completion Loom tests, and both strict lint checks.
The strict Casper library/test lint completed in 30.22 seconds after the earlier dependency build.
`run.rO9JSH` passed the final seventeen proofs, independent kernel checking, 20,219 safe states, and all ten named unsafe controls.
Both final scopes exited successfully with matching input hashes.
The concurrent memory limits totaled 7 GiB, and both scopes disabled swap.

The completed kernel does not yet allocate private identities or own completion channels.
The wrapper and engine integration must supply those identities and report only actual successful callback results.
The next implementation connects these verified transitions to that ownership boundary, then to the common driver.

The final plan-agent publication review requires both the engine write guard and controller mutex through the engine-slot swap.
Releasing the controller mutex first would expose intermediate state to controller readers.
The engine cell binds one controller and rejects another controller before mutation.
Non-running replacements use the same ordered revocation and preserve active work until retirement.
The contract records the exact lock order and requires notifications and owner destruction outside both guards.
Both plan agents completed their reviews, and all verification scopes for this step finished.

## Runtime owner and engine publication

The next implementation adds `StartupOwner` around the qualified completion kernel.
It supplies private allocation identities, request outcomes, actual callback observation, weak context references, and active-work retirement.
`EngineCell` now binds one recovery controller and holds both guards through registration and engine replacement.
Non-running replacements revoke the registration without depending on old engine destruction.

The plan-agent review found three integration ordering defects during implementation.

1. Rejected publication initially destroyed its incoming engine while the engine guard remained held.
2. Startup stop initially destroyed pending roots before it stopped the ordinary recovery signal.
3. Running-state reporting initially preceded the new fallible engine publication.

The correction retains rejected engines outside the guard lifetime and separates stop publication from root cleanup.
Running-state reporting now follows the committed engine swap.
These changes preserve Casper validity, voting, fork choice, and proposal routing.

`StartupPublication.tla` separates two concurrent publishers, both publication guards, rejection, destruction, reporting, and an independent stop operation.
`run.lF0ApG` checked 624 safe states and four named unsafe controls before the stop and event corrections.
`run.zmqOk4` repeated those checks after the corrections with matching source hashes.
The model checks local ordering safety, not end-to-end consensus correctness or production timing.

| Evidence | Result |
|---|---|
| `target/verification/startup-runtime/run.ffck4x` | Twelve native owner tests passed. |
| `target/verification/startup-runtime/run.nTrkIX` | Twenty owner and engine-source tests passed. Strict Casper library/test lint passed. |
| `target/verification/recovery-pump/run.vSKfsf` | Twenty-one native tests and strict standalone lint passed. The Casper integration fixture failed compilation because its new `ApprovedBlock` omitted `floor_seed`. |

The fixture correction supplies `floor_seed: None`, matching the existing approved-block fixture.
No production assertion or protocol validation rule changed for that correction.
The final native gate uses `run.lebG7T` and scope `pr216-startup-wrapper-gate-20260906d`.
Its memory limit is 5 GiB with swap disabled.
The gate runs the actual Casper transition/event regression and strict Casper and node lint.
Its result must be checked before claiming that integration passed.

The gate subsequently passed with matching input hashes.
All twenty-one native wrapper tests and strict standalone lint passed.
The actual Casper transition/event regression passed in 32.12 seconds after compilation.
Strict Casper and node library/test lint passed in four minutes and eleven seconds.
The gate exited successfully without increasing its memory limit.
The documentation syntax check passed for 353 files.
The aggregate finalized-floor gate now requires the native startup gate as well as the formal publication checks.

The native harness imports actual owner, completion-kernel, and engine-cell source.
Its engine-cell boundary tests replace only the surrounding engine, context, and error dependencies.
Generated histories compare ticket outcomes and retained roots with an independent reference.
The examples cover callback suspension, cancellation before polling, stale readiness, exact retirement, stop, weak references, and committed-result preservation.
Destructor probes detect root or engine destruction under publication guards.

The final resource review identified a separate integration obligation.
One controller pending slot does not bound roots retained by concurrent replacement cleanup objects.
Incoming snapshots also consume resources before submission.
The strict physical-root bound needs admission before capture and an ownership lease that survives actual destruction.
Publication-triggered retirement must obey the same bound.
The sequential replacement regression alone does not prove this concurrent resource property.

The common driver, snapshot lease, and startup caller remain unfinished.
The caller must receive the same prepared context handle that the engine publishes.
No git index, commit, branch, or remote changed during this step.

## Narrow dependency metadata qualification

Ordinary dependency readiness now uses admitted membership and a validated metadata row without constructing a full DAG representation.
The borrowed dependency iterator preserves all canonical dependency sources.
It does not change the existing sorted, deduplicated function used by other callers.
A missing dependency does not suppress a later storage error.
Each lookup observes its own current row, not a pass-wide snapshot.

Formal run `run.hyiiSe` passed before these production changes.
It checked eight closed Rocq theorems, independent kernel validation, and 39,996 safe TLC states.
The finite domain has two metadata keys and three reference occurrences, including duplicate references.
All eight initial reference assignments were explored.
Four unsafe controls violated their required invariants for visibility, row existence, error preservation, and premature completion.

Native run `run.x0zCID` passed the initial property, Loom, storage, dependency, helper, and strict lint checks.
The review then identified a weakness in the lock test.
A reader-start notification did not prove that the reader reached the lock before the test continued.
The replacement blocks the actual backend read and checks that both production write-lock attempts fail.

Two new actual-resolver tests use `MultiParentCasperImpl` and shared in-memory stores.
The tests cover corruption after a missing dependency, row deletion, later admission, missing bodies, and certificate waits.
They call both the hash-returning and body-returning production resolver methods.
Additional examples check empty dependency lists and repeated references with changing results.

The first fixture attempt used a block-hash length for its validator key.
After that correction, the fixture retained random certificate references from the general block generator.
The fixture now uses the validator-length constant and certificate references drawn from its declared parents.
An explicit dependency-set assertion guards fixture construction.
Neither correction changed production acceptance rules.

Native run `run.NdE6y0` passed all 19 selected tests.
These comprise three property tests, one Loom test, four storage tests, six dependency tests, two actual-resolver tests, and three helper tests.
Strict block-storage, Casper, and node library/test lint passed, as did standalone property/Loom lint.
The gate exited with status zero, and all recorded source hashes matched.
The run used a 5 GiB systemd scope, one build worker, one test thread, and no swap.
The documentation syntax check passed for 355 files under a separate 256 MiB scope.

The documentation now distinguishes one metadata key per lookup from the typed codec's temporary copies.
The existing codec can retain two decoded row copies, and serialized bytes can coexist with decoding.
No copy accumulates across dependency reads.
This result does not establish a decoded-heap limit, replay correctness, network recovery, or full-driver completion.
The common recovery driver, owned dispatcher/services, and context-bound initializer still require implementation and composed verification.
No git index, commit, branch, remote, or sibling worktree changed during this batch.

The final plan-agent review completed the strict snapshot-lease design.
The [ownership plan](../casper/theory/finalized-floor/startup-snapshot-ownership.md) records its API, state transitions, counterexample, formal controls, and native schedules.
It distinguishes free, capturing, stored, and retiring pending ownership.
Activation transfers the exact lease to active ownership without an uncharged interval.
The next step is formal composition with `StartupCompletion`, followed by the reservation and caller implementation.
Both plan agents and all verification commands finished for this implementation step.

## Snapshot capture and retirement leases

The next goal turn implements the approved snapshot-lease boundary.
The preceding user-status turn made no implementation change.
The source and task-board checks confirmed that the snapshot lease remained the next required action.

The new TLA+ model directly extends the qualified startup-completion model.
It separates reservation, capture admission, builder completion, storage, activation, retirement, destruction, and release.
Cancellation and publication retain capturing and active leases.
The model records physical episode ownership separately from pending-slot occupancy.

`run.R1ZLRr` passed 489,518 safe states and nine named unsafe controls before the production lease change.
The cancellation control constructed three overlapping physical episodes when cancellation released a capture lease early.
The plan agent requested an additional same-identity wrong-role release control.
That control detects stale pending release after the lease transfers to active ownership.

The Rocq proof first establishes resource conservation.
An independent event ledger then establishes once-only construction, live builder charges, and the physical episode bound over arbitrary finite histories.
The proof does not assume its target cardinality bound as a transition premise.
Later lemmas connect the ledger transitions to the resource projection and establish role separation and pending-builder ownership.

The first Rocq attempts failed on type inference, an omitted explicit theorem argument, and incomplete proof scripts.
Those failures did not qualify any proof or authorize a production semantic change.
`run.VfijJJ` subsequently passed 24 closed theorems, independent kernel checking, 489,518 safe states, and ten named unsafe controls.
That result preceded implementation of the lease kernel and owner API.
`run.Pe5YIR` later passed the strengthened 28-theorem set and the same finite model configuration.

The new `SnapshotLeases` kernel checks exact identity, role, and phase transitions.
`StartupCapture` reserves before construction and consumes a private `FnOnce` builder.
`LeasedWork` holds capacity through payload destruction.
`SnapshotRetirement` releases the exact lease after destruction, including destructor unwind.
Active completion ownership remains occupied until that release.
The concrete public capture method accepts a storage reference and creates its snapshot under the lease.

The capture waiter uses its existing request notification.
It does not compete with the driver's single-consumer notification.
The waiter registers before it checks capture availability.
Busy reservation attempts create no snapshots, lease identities, tasks, or additional waiter records.
The single external initializer owner still needs production integration.

The plan-agent implementation review found two further issues.

1. Public mutable scanner access permitted safe code to extract retained roots through replacement.
2. TLA+ initially reported abandoned-scan failure before destruction, while production reported it after destruction.

The correction restricts generic mutable access to the trusted queue module.
The public API exposes only concrete scanner operations that do not export or replace its payload.
The model now reports abandoned-scan failure at release, matching production.
The shared driver must preserve this no-export boundary when it is integrated.

The first updated native run passed 27 owner and engine-source tests.
The next strict gate failed on Clippy's type-complexity check for the publication return type.
A named result alias preserves that interface without suppressing the lint.

The review also requested stronger destruction evidence.
The expanded tests use two separately owned root fields and block each destructor independently.
They also panic in the parent destructor or source destructor and require the remaining fields to disappear before lease reuse.
A deterministic kernel regression now tests stale identities against retiring and destroyed replacements.
Another regression stops the owner while its builder remains blocked.

The final native gate for this batch uses `run.1XglDu` under a 5 GiB scope.
It includes native ownership tests, event-history properties, Loom schedules, actual Casper startup integration, and strict Casper/node lint.
The corresponding revised formal gate uses `run.nQx1xr` under a 2 GiB scope.
Both scopes disable swap, use one worker, and retain source hashes.
Their final results must be checked before this batch is qualified.

The revised formal gate subsequently passed 28 closed theorems, independent kernel checking, 493,862 safe states, and all ten unsafe controls.
The native gate passed 30 owner/engine tests, two lease property/example tests, three Loom tests, and both standalone lint checks.
The actual Casper integration test then passed in 32.87 seconds.
Strict Casper/node library and test lint passed.
The native gate exited with status zero, and all recorded input hashes matched.
The documentation syntax check passed for 355 files.

The lease proofs remain resource-projection proofs, not an unbounded proof of the complete startup API.
The finite TLA+ model separately checks startup-phase composition.
The private runtime API and actual allocation/destruction tests must establish the projection premises.
Actual storage lifetime measurements, common-driver integration, single-initializer integration, and the campaign soak remain unfinished.
No git index, commit, branch, or remote changed during this step.

## Successor-demand transfer and composition review

The next plan-agent review inspected the actual dispatcher and its component proofs.
It confirmed that worker ownership, owned services, and their combined shutdown behavior still lack production integration.
The full remaining contract is in [Recovery dispatcher composition](../casper/theory/finalized-floor/recovery-dispatcher-composition.md).
That document preserves parallel workers and records independent ownership witnesses, exact unsafe controls, and actual-runtime regression obligations.

The review identified a specific integration hazard in `RecoverySignal::wait`.
The call consumes demand even when an active pass only needs a capacity wake.
Discarding its returned work could lose a new proposal request.
This was a design hazard found before shared-driver integration, not a newly observed consensus failure.

`RecoveryPumpDemand.tla` now tracks shared demand and one bounded successor-demand record separately.
Independent request-identity sets witness the work owed at both locations.
The negative control discards consumed demand but retains its witness, so the checker can detect the loss.
Completion and continuation preserve successor work, and stop closes both locations.

Formal run `run.FGob2z` passed before the production merge change.
It checked ten new closed Rocq theorems, the twenty imported control theorems, and independent kernel validation.
TLC explored 2,466 safe states with three requests and all eight proposal-flag assignments.
All four unsafe controls violated their specified invariants.
The new consumed-wake control violated `Inv_ExactNextDemand`.

The production `RecoveryWake::merge` operation implements the verified four-state merge.
It preserves work and proposal demand, treats idle as identity, and makes stop absorbing.
Native run `run.FXABcv` passed seven property/example tests and six Loom tests.
The tests include exhaustive merge laws, generated request partitions, two concurrent producers, stop races, and an older failed pass.
Strict standalone lint and Casper/node library/test lint passed.
The native gate exited zero, and all recorded input hashes matched.
The run used a 5 GiB memory limit with swap disabled.
The documentation syntax check passed for 357 files under a separate 256 MiB limit.

These results qualify the merge kernel and its demand model, not the unfinished dispatcher composition.
The next implementation boundary remains the shared driver, owned worker/services, bounded proposal mailbox, and exact-context initializer integration.
No source, branch, index, or remote in a sibling repository changed.
No commit or push occurred.

## Dispatcher composition and worker lifecycle

The September 7 continuation resumed `pr216-admission-backpressure` under scheduler revision 488.
The previous user-status turn did not change implementation or verification state.
The renewed task lease expires at 08:00:52 UTC.

The plan agent reviewed the draft combined ownership model twice.
The review found missing identity-retention, receiver-cleanup, scanner-parking, presence-phase, cancellation, and visit-budget obligations.
These were gaps in the draft verification model, not newly observed validator-consensus failures.
The model now contains explicit bridges for recovery body loading and admission.
It also preserves independent physical ledgers through parent cancellation and delayed destruction.

The initial composition run failed on an absent-capture lookup.
That failure did not qualify the control.
An explicit optional-capture branch corrected the lookup.
The next run detected eleven intended controls before the worker-capacity search became expensive.
I stopped only that run to apply the completed review corrections.
Worker-capacity and proposal counterexamples now restrict unrelated actions without changing the original transition operators.

Composition run `run.RqbhET` passed all 18 named negative controls.
The run used a 2 GiB memory cap, one worker, and no swap.
Every control failed its exact invariant with exit code 12.
The gate returned zero and validated its source hashes.
The complete positive-safety search and temporal liveness proof remain unfinished.

`RecoveryWorkerLifecycle.v` adds a parameterized proof over arbitrary finite interleavings and worker identities.
The first proof attempt terminated with exit code 143 and did not qualify.
The revised tactic discards impossible states before expanding operation cases.
Run `worker.spicqK` then passed all 17 closed theorem checks and independent kernel validation under a 2 GiB cap.
The proof establishes physical release order, identity retention, cancellation ownership, interference separation, and finite abstract cleanup service.
It does not establish a wall-clock cleanup bound or full dispatcher refinement.

Three new native regressions exercise the production queue and admission leases.
They cover 64 generated operation histories, overlapping temporary senders, and delayed payload destruction after cancellation.
Run `native.XGCmos` compiled the tests and started the five-test admission module.
Its final test and lint results must be checked before qualification.
The scope has a 5 GiB cap, one build job, one test at a time, and no swap.

No production dispatcher migration, git commit, push, or sibling-repository change occurred in this batch.

The native run subsequently passed all five admission tests in 101.36 seconds.
Strict Casper integration-test lint passed, and all recorded input hashes matched.
The generated property used 64 cases with variable queue capacity, byte capacity, keys, payload sizes, closure, dequeue, release, and proposal demand.
The delayed-destruction regression confirmed that cancellation does not release byte or identity leases before the destructor returns.
The documentation syntax check passed for 357 files.
Progress note 9870 records this checkpoint.

The symbolic checker first rejected an unsupported typecheck option and then required explicit record and tuple types.
Those attempts did not establish model safety.
The typed wrapper uses a module instance of the same transition source rather than a second transition implementation.
The type annotations are checker input and do not alter the TLA+ actions.

The annotated wrapper passed its original safety initialization check in `symbolic.mtnAQ1`.
The expanded inductive candidate then passed its base check in `run.XwpnV6`.
That base check included 121 invariant conjuncts.
The candidate follows the plan agent's source-derived stage, ownership, cancellation, proposal-history, and demand-witness relationships.

Run `run.Y52O6o` now checks inductive preservation by invariant group.
Every group retains the complete candidate as its initial predicate and the complete original `Next` relation.
The run uses a 3 GiB memory limit with swap disabled.
It must pass all groups before the combined safety argument is qualified.
The argument would cover all history lengths for the configured finite domains, not arbitrary identity-domain sizes.

The state-domain preservation check passed against all 53 symbolic transition branches.
The first safety step check found an incomplete inductive predicate.
It permitted a pending proposal while the proposal service was already joined.
The actual service-retirement transition clears that slot.
The candidate now states that retired or joined proposal services have no pending offer.
The correction changes the proof predicate, not a production transition or shutdown guard.
Both base and step checks must pass again for that stronger candidate.

The next candidate passed state-domain, safety, worker, supervisor, and service preservation checks.
Proposal-state preservation then exposed another missing inductive relationship.
The arbitrary starting state permitted a retired service while the supervisor was running.
Service retirement occurs only after shutdown reaches the aborting phase.
The candidate now includes that source-derived relationship.
This is another proof-strengthening correction, not an observed production consensus defect.

## Completed composition induction and runtime integration

The final base check passed in `run.2I3Pgn`.
All eight preservation groups passed in `run.oKI2lQ`.
The model inputs matched at both gate exits.
The checks retain the original transition relation, all concurrent services, and the stated finite domains.
These results establish inductive model safety, not unrestricted population size, temporal liveness, or production correctness.

The next implementation batch replaces detached block processing with `BlockProcessorInstance::run`.
The runtime now awaits that future through its existing critical-task set.
Separate owned task sets retain block workers and the recovery/proposal services.
Completed, unjoined workers count toward the parallel worker limit.
Workers borrow their intact queue item and release that item before recovery demand or other waits.
The obsolete result channel and shared scan mutex are removed.

The shared driver now receives startup, periodic, certificate, completion, and capacity demand.
It uses the existing rotating candidate index and a 64-step page limit.
Its one-second retry timer skips missed ticks.
It retains successor proposal demand during capacity waits.
Proposal offers retain exact weak registration handles.
Authorized callbacks continue through registration replacement while newer pending offers remain separate.

The plan agent identified an important startup distinction during the source review.
Startup performs its second body read before stale-DAG reconciliation.
It does not perform ordinary dependency or certificate prechecks before queue admission.
Separate required resolver methods preserve those policies.
Both methods propagate narrow admitted-metadata errors.
Startup still performs all presence checks before any startup admission.
Temporary startup capacity failure retains one pending hash and drops its body before parking.

Initialization now receives the handle from the exact registration published by `transition_to_running`.
The other initialization routes retain their no-op callbacks.
Startup proposal invocation retains its existing independent configuration.
The new handle operations distinguish republication of the same Casper allocation.
Their tests cover replacement, revocation, stop, and weak ownership.

New resolver tests exercise the actual implementation without genesis replay.
They cover absent membership, missing bodies, admitted metadata, metadata corruption, certificate waits, and captured startup membership.
Generated histories compare actual candidate rotation with an independent FIFO reference.

Three new native dispatcher tests cover unpolled destruction, external-sender closure, and two simultaneous workers during delayed payload destruction.
The last case inspects byte and identity ownership before destruction returns.
It uses quarantined candidates to isolate ownership from replay.
It does not establish consensus or replay correctness.

The focused gate `run.EGl24d` is active under `pr216-dispatcher-integration-20260907a.scope`.
Its limits are 5 GiB memory, no swap, one build job, and a 100-percent CPU quota.
No input source changes are permitted while that gate runs.
The runtime changes remain unqualified until their tests, review, and strict lint pass.
The task remains active.
No commit or push occurred.

The production review found two conformance gaps in this first integration batch.
An ordinary temporary rejection drops the candidate hash after its selection consumed the final pass visit.
That path can complete the pass without resolving its pending candidate.
The active retry timer also creates successor demand instead of only resuming the parked pass.
The correction must retain an ordinary pending hash and separate retry resumption from new periodic demand.

The combined model needs an independent outstanding-candidate witness before this correction can be qualified.
Its previous load transition consumed a visit but did not retain that obligation after temporary rejection.
This missing abstraction boundary prevented the prior safety checks from detecting the implementation gap.
The planned controls separately remove pending ownership and permit completion with an outstanding candidate.
These findings do not establish a new consensus-validity defect.
They concern the local recovery pass and its optional proposal timing.

The first native dispatcher selection passed all 11 tests in 0.09 seconds.
Those tests establish their stated ownership cases, not the missing pending-candidate contract.
The resolver and context tests in `run.EGl24d` subsequently passed.
Strict lint rejected unused imports in `casper_launch.rs`.
The next source batch removed those imports and the unused dispatcher `BlockHash` import.

## September 7 retry refinement

The refined model separates selected candidates from loaded bodies.
Temporary rejection retains the selected hash and its independent witness.
A separate page allowance bounds attempts, including reloads of the same candidate.
Acknowledgment does not refill that page allowance.
An active retry deadline no longer creates a successor maintenance pass.

Five named negative controls passed in `run.JVUZK1`.
The initial invariant passed in `run.bGJMP1`.
All eight full-`Next` induction groups passed in `run.UFc4qp`.
Nineteen initial Rocq theorems and kernel validation passed in `retry-rocq.KrwEug`.
These checks preceded the corresponding production retry changes.

The integrated source batch passed in `run.P7YwDy`.
The gate passed 17 dispatcher tests, seven actual resolver tests, 26 context tests, and strict lint.
Actual resolver and queue tests reproduced byte rejection, repeated reload, concurrent removal, and context replacement.
Generated histories check the retry state against independent expected state after every operation.

The next read-only review found formal correspondence gaps for acknowledgment errors and empty selections.
These were missing model cases, not additional observed runtime defects.
The latest model adds acknowledgment ownership and an independent pass-error witness.
The latest Rocq proof adds empty visits and pre-selection failure accounting.
The final expanded base passed in `run.RvSnIh`.
All eight full-transition preservation groups passed in `run.G6E490`.
The 21 parameterized Rocq theorem checks and independent kernel validation passed in `run.IkmXoP`.
All six new negative controls failed their named invariants in `run.suDtFL`.
Two intermediate induction failures required additional state relationships, not changes to production transitions.
Those relationships exclude unused load permits during acknowledgment or body ownership.
They also connect acknowledgment ownership to its wait reason and active attempts to spent page permits.

The input-closure model passed all 2,324 reachable states and its liveness property in `closure.9stNG2`.
Its three controls refuted worker-slot-dependent probes, resettable deadlines, and probes that dequeue an extra body.
The production closure probe and three native closure regressions are implemented.
The complete native batch passed in `run.tJ6mlp`.
It passed 24 dispatcher tests, seven actual resolver tests, 26 context tests, and strict node lint.
The dispatcher tests took 4.97 seconds.
The native scope used a 5 GiB memory limit.
The formal scope used a 2 GiB limit.
Both scopes disabled swap and completed with exit code zero.
The original negative controls now run against the refined model in `run.f87Z9D`.

The pruning audit found two additional preservation defects within the approved recovery boundary.
Age or pressure eviction invokes the same removal operation as dependency resolution.
For an unresolved chain, eviction can delete the middle block from the surviving child's durable parent row.
Eviction can also delete the last durable retry owner after the request tracker has acknowledged the buffered block.
The older pruning model restricts removal to anonymous sibling waiters with another waiter remaining.
It does not cover either trace.
No pruning production repair has been applied.

## Pruning preservation evidence

The plan review confirmed both loss traces and the required separation between durable rows and resident caches.
It also identified cold certificate reconciliation and startup capture as required migration boundaries.
The detailed contract is [buffer-pruning-preservation.md](../casper/theory/finalized-floor/buffer-pruning-preservation.md).

The new graph model passed four finite safe configurations in `run.ziyKTJ`.
The configurations cover chains, shared certificate dependencies, isolated blocks, and joins.
Each uses three block identities, one certificate identity, and two abstract resident slots.
Four negative controls failed their exact named invariants.
Two controls reproduce current defects.
Two constrain the proposed paging and restart design.

The parameterized Rocq proof initially required exact canonical edges.
Plan review identified that restriction as stronger than production preservation requires.
The final proof requires every unresolved edge but permits conservative extra edges.
Fourteen theorem checks and independent kernel validation passed in `rocq.F94IX3`.
The proof covers arbitrary key types, dependency relations, cache capacities, and finite transition histories.
It imports authoritative terminal evidence and atomic operation boundaries.
It does not prove complete paging, intermediate publication, backend failures, or snapshot refinement.

Three actual storage regressions reproduced the defects in `native.UkrMPy`.
The isolated-row test uses `is_pendant` after restart because `contains` describes nonempty parent mappings.
The generated regression compares durable rows against an independent map after every operation.
The combined gate `run.gC3dPb` passed the formal checks and failed all four native regressions.
Property shrinking reduced row loss to an inserted pendant followed by age pruning.
The regression seed remains in the standard block-storage regression corpus.
The configured 64 cases did not complete successfully.

The original dispatcher controls remain active in `run.f87Z9D`.
No input of that running check has changed.
Production pruning remains unchanged while its complete storage and capture refinement is prepared.
No task was closed, and no commit or push occurred.
The task remains active.
No commit or push occurred.
All new temporary artifacts remain under `target/verification`.

## Storage paging decision

The read-only storage plan is complete.
The plan requires an explicit amendment to the current process-local-index constraint.
Its recommendation adds one rebuildable derived namespace and leased disk-backed startup storage.
The existing parent-row encoding and consensus decisions remain unchanged.

The source audit confirmed that candidates include implicit parent vertices, not only explicit durable rows.
It also confirmed that cold reverse lookup needs more than a retry-ticket index.
The proposed namespace contains ticket, reverse-edge, pendant, generation, and terminal-cleanup records.
The detailed alternatives and affected interfaces are recorded in the pruning preservation document.

The owned-view design uses the installed `heed 0.22.1` API.
It needs typed non-TLS transactions and the exact environment-cache `Arc` lifetime.
Live readers must be released after source materialization, before capacity waits.
Episode counts do not bound retained database pages.
Dependency row sizes and atomic rewrite costs remain separate resource obligations.

The focused presence negative control passed its required refutation in `presence.bV9cy3`.
It reuses original composition transitions.
It found `Inv_NoPresenceBypass` after 3,512 distinct states at depth 15.
The separate full-transition control remains active in session `63745`.
Its most recent checked output showed continued state exploration, not completion.
No active verification input changed.

The pruning migration requires approval before production edits.
Four native pruning regressions still fail against unchanged production.
No task was closed.
No commit or push occurred.

## Upstream authority and regression-first repair

The user approved a repair only after a regression demonstrates the bug.
The user then required upstream architecture alignment and Casper team review.
The broader storage proposal remains unapproved.
No existing comparison finding constitutes upstream ratification.

The pgmcp plan `pr216-buffer-pruning-upstream-repair` contains nine weighted tasks with verified dependency gates.
Scheduler revision 550 selected `pr216-pruning-red` first.
The new public-API fixture covers age pruning, pressure pruning, and an unpruned positive control.
It checks durable parent rows, both dependency projections, and reopen behavior.
This batch does not change production code or frozen verification inputs.

The public regression selected three tests under a 5 GiB systemd memory cap with swap disabled.
Age pruning and pressure pruning each failed the intended preservation assertion.
The unpruned positive control passed.
The run returned the expected reproduction exit `101` without compiler warnings.
The durable row and both dependency projections were absent after pruning and after reopen.
The fixture reopens an in-memory store, not an LMDB process.
The evidence file is `target/verification/buffer-pruning/public-regression-20260907.log`.
The pruning design records source hashes and the remaining evidence limits.
No production repair, architecture migration, commit, or push occurred in this batch.

Focused Clippy passed with warnings denied in a 5 GiB systemd scope.
The regression file passed the format check.
Criterion 3655 records the expected pre-fix failures, not a passing repair.
The reproduction task is claimed complete.
The repair and upstream review remain incomplete.

## Pinned upstream execution and minimal repair review

Work continued without a new approval request.
Scheduler revision 552 selected the pinned-source comparison.
Analysis-only dependencies accept recorded completion while trusted evidence remains pending.
Production still requires verified reproduction, formal qualification, and the upstream disposition.

The current remote `dev` pin is `0f5d2b7414786cd27b8a686ca636485c35a5edce`.
Its buffer and dependency-DAG blobs match the prior audit.
A controlled harness compiled those unchanged files with current workspace dependencies.
Both pruning cases failed the same preservation assertion, and the unpruned control passed.
The harness selected three cases and filtered fourteen unrelated upstream tests.
This was not a complete upstream checkout or network run.
The temporary Cargo test target was removed after execution.
Its source and result summary remain under `target/verification/buffer-pruning/dev-0f5d2b741478/`.

The second plan review rejected a storage redesign as a prerequisite for the minimal deletion fix.
However, skipping unresolved pruning does not satisfy the resource bound.
A simple admission cap can lose ownership, stop needed parents, or leave restoration unbounded.
The permanent pruning document now states the alternatives and upstream decisions.
No architecture option was adopted.

The review also traced an initial-publication failure through error acknowledgment and hash-free recovery.
For a fresh hash, the inspected path leaves no actionable local retry owner.
This is a source-level finding, not a completed worker-level reproduction.
The required regression and composed-model boundary are recorded in the pruning document.
The existing long-running dispatcher control remains active, with its inputs unchanged.
