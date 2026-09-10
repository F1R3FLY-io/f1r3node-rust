# Cost-accounting concurrency checks

This crate runs Loom models without the full node dependency graph. Each test calls Loom through its model interface.

Loom explores schedules both with and without `--cfg loom`. That flag does not change a model into a single execution.

## Production correspondence

The [production cache tests](tests/loom_production_replay_cache.rs) import the actual [cache transition module](../../../casper/src/rust/util/rholang/replay_cache_state.rs).

Production uses these transitions under `std::sync::Mutex`. The tests use them under `loom::sync::Mutex` with small immutable keys and results.

The shared code preserves cache replacement, lookup order, eviction, byte charges, and clear operations. The tests check returned clones after eviction.

The shared publication helper calls persistence before optional cache publication. Persistence failure prevents that invocation from publishing.

The publication model retains metadata during lookup. It does not prove metadata retention after an independent deletion.

The production runtime separately checks mergeable metadata before accepting a cache hit. The genesis cache-hit path does not repeat persistence.

These tests do not execute the Rholang interpreter, protobuf normalization, or persistent store. Production regressions must verify those boundaries separately.

The [persistent charging tests](tests/loom_execution_result_reuse.rs) remain abstract models. They do not establish production charging correctness by themselves.

Other files retain their documented abstraction boundaries. Importing one production module does not establish production correspondence for every model.

The [completion-query tests](tests/loom_production_effect_observation.rs) import the production ledger's [query kernel](../../../block-storage/src/rust/finality/finalization_ledger/effect_observation.rs).
Each point read acquires a separate model store guard.
This preserves the production boundary between the first cursor read, receipt read, and final cursor recheck.
The tests cover concurrent queries, all four effect kinds, and receipt creation or deletion.
The old two-read sequence must fail its named assertion as a negative control.

These tests explicitly explore without a preemption limit, iteration limit, duration limit, or checkpoint resume.
Their finite executions use at most four threads and a 1,000-branch limit that fails on exhaustion.
They do not model LMDB durability or turn separate query-and-effect operations into one atomic transaction.
The [ledger design](../../../docs/casper/theory/finalized-floor/bounded-finalization-ledger-audit.md#completion-queries-during-compaction) specifies their assumptions and backend regressions.

The [integrity-page tests](tests/loom_production_integrity_pages.rs) import the actual audit capture and page kernel.
The [recovery-page tests](tests/loom_production_ledger_pages.rs) import actual effect selection, advancement, receipt iteration, and compaction.
The native adapters retain record validation, encoding, and backend durability responsibilities.
The [page correspondence table](../../../docs/casper/theory/finalized-floor/bounded-finalization-ledger-audit.md#shared-page-implementations) separates those obligations.
Both targets use four threads and a 5,000-branch limit that fails on exhaustion.
They set no preemption, permutation, duration, or checkpoint limit.

The recovery compactor scenario applies the [proved observer reduction](../../../docs/casper/theory/finalized-floor/bounded-finalization-ledger-audit.md#compactor-observer-reduction).
It retains both concurrent compactors and their complete page traversals.
The redundant covering-cursor query runs after both workers join and must not read a receipt.
All six recovery-page tests passed in release mode after this reduction.
The original three-worker run was stopped with user approval, not counted as passed.

The [sparse transaction tests](tests/loom_production_sparse_transaction.rs) import production staging, guarded publication, and snapshot guard lifetime.
They retain separate map operations under Loom locks.
The tests cover competing writers, snapshot readers, store aliases, rollback, explicit deletion, and manager rejection.
Two negative controls remove the existing transaction or snapshot gate and must fail their named safety assertion.
These tests use three threads and 5,000 branches per execution without schedule cutoffs or checkpoint resume.
Native properties separately exercise actual store handles, all operation kinds, and preservation of unrelated allocations.

Four additional sparse-transaction tests cover the state-import alias contract.
They check two-alias commit guards, unrelated concurrent writes, and raw-first reads split across compatible insertion.
The added negative control omits the opposite-alias guard and must expose conflicting bindings.
These tests use the same production transaction code and exploration bounds.
The [state-import proof record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#production-transaction-concurrency-checks) states the proof correspondence and remaining integration limits.

Four history-transaction tests also use the production staging function.
Concurrent identical batches must both succeed.
Conflicting batches must commit exactly one complete result without the rejected batch's private prefix.
A conflicting duplicate must roll back the entire history batch while preserving an unrelated cold-store commit.
The unsafe control replaces the current absent-or-identical guard with an unconditional write and must expose a conflicting overwrite.
These tests use the same exhaustive schedule exploration within the stated thread and branch bounds.
The [history transaction record](../../../docs/casper/theory/finalized-floor/state-import-validity.md#observed-history-transactions-and-committed-traversal) separates transaction guarantees from page validation and root publication.

## Qualification

The [retry control tests](tests/loom_recovery_pump_control.rs) import the actual atomic wake module.
They cover request coalescing, proposal retention, stop races, and notification before waiter registration.
The stored-token adapter uses the pinned Tokio dependency's sequentially consistent notification boundary.
It does not reproduce Tokio's complete waiter list.
Native asynchronous tests exercise the actual notification adapter and cancellation.

The [retry properties](tests/property_recovery_pump_control.rs) import the production wake and pass modules with standard atomics.
They compare arbitrary request histories with a request-list reference and check page partitions, error preservation, and full-range counters.
The [control specification](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md) states the model domains and unfinished pipeline boundaries.

The [admission identity tests](tests/loom_admission_identity.rs) import the actual hash-sharded registry and guard destructor.
They replace only the synchronization primitives with Loom primitives.
Four tests cover competing claims, stale release, colliding shard keys, and queued ownership transfer.
Each execution has a 1,000-branch limit and at most three threads, with no schedule cutoffs or checkpoint resume.
The native lifecycle regression checks actual Tokio cancellation, queue destruction, block-byte destruction, and reservation release.
The [identity specification](../../../docs/casper/theory/finalized-floor/admission-identity-ownership.md) separates identity safety from unfinished supervisor and retry obligations.

The [buffer publication tests](tests/loom_buffer_transaction_publication.rs) import the production commit-before-publication helper.
They model two writers and a reader under the buffer's existing mutation guard.
All combinations of successful and failed writes must preserve the published projection.
A negative control releases the guard between commit and publication and must fail its named assertion.
The safe test sets no schedule cutoff and fails if its 1,000-branch execution limit is exhausted.
Native buffer properties cover the graph implementation separately.
Real LMDB process-exit tests cover restart before commit and after commit, before memory publication.
The [buffer specification](../../../docs/casper/theory/finalized-floor/buffer-durable-membership.md) states these boundaries.

The [candidate-index tests](tests/loom_buffer_candidate_rotation.rs) import the production FIFO links.
They test concurrent arrivals and retirement while an existing candidate waits for examination.
They also include the production module's four native example and property tests.
The [candidate specification](../../../docs/casper/theory/finalized-floor/buffer-candidate-rotation.md) states the service-bound premises and unfinished pump obligations.

The [recovery actor tests](tests/recovery_actor_service.rs) import the production inbox and rotating selector.
The [concurrent selector tests](tests/loom_recovery_service_rotation.rs) import that same selector under Loom.
The tests cover concurrent producers, ordered drain, independent channel closure, and a continuously ready timer.
Native Tokio tests cover actual channel polling, cancellation, and receiver drop.
The [actor specification](../../../docs/casper/theory/finalized-floor/recovery-actor-service.md) states the correspondence and service-bound premises.
Use `scripts/check-recovery-actor-service.sh` for its uncapped schedule exploration and required formal checks.

Run the gate inside the approved systemd resource limit:

```sh
systemd-run --user --scope -p MemoryMax=8G -p MemorySwapMax=0 \
  env CARGO_BUILD_JOBS=2 \
  TMPDIR=/home/dylon/Workspace/f1r3fly.io/f1r3node-rust/target/verification/pr216/duplicate-execution \
  bash scripts/check-cost-accounted-rho-loom.sh
```

The gate defaults to three preemptions and 1,000 branches per execution. One finite-domain test explicitly raises its branch limit to 250,000.

The gate reports the configured bounds. A passing run establishes only exploration within those bounds, not all possible production schedules.

The gate rejects missing tools, timeouts, empty results, ignored tests, and filtered tests. It also rejects these inherited settings:

- `LOOM_MAX_PERMUTATIONS`
- `LOOM_MAX_DURATION`
- `LOOM_CHECKPOINT_FILE`

These settings can limit or resume exploration without proving that this run explored its complete configured domain.

The [gate regression script](../../../scripts/test-check-cost-accounted-rho-loom.sh) checks success, failure, bounds, and incomplete results with isolated command doubles.

The [verification guide](../../../docs/casper/theory/cost-accounted-rho-verification.md#c6-shared-replay-cache-context) connects these checks to Rocq, TLA+, and production regressions.

## Runtime initialization ownership

`tests/initializer_ownership.rs` imports the production runtime supervision module.
It uses native Tokio execution, not Loom instrumentation.
The tests cover canceled selection, result retention, parent cancellation, shutdown races, and destruction before a child's first poll.
Generated event histories retain the initializer across competing startup and critical-task notifications.
The legacy negative control reproduces the former take-before-await ownership loss.

The [control specification](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md) records the proofs, scheduling assumptions, and remaining dispatcher obligations.
Use the memory-limited `check-recovery-pump-control.sh initializer` gate for this formal and native boundary.

## Startup snapshot ownership

`property_startup_snapshot_lease.rs` reconstructs lease histories from accepted events and checks an independent physical-episode ledger.
It checks each intermediate state, including capture, retirement, role transfer, and stale release.
`loom_startup_snapshot_lease.rs` explores concurrent reservation, delayed active destruction, and same-identity wrong-role release.
Both targets import the production lease kernel.
The native startup owner tests additionally exercise blocked builders, ordered destructor gates, cancellation, notifications, and unwind.
The common driver and actual storage lifetime still need integration qualification.

`property_startup_snapshot.rs` imports the production ordered cursor and compares it with an eager reference set under generated live mutations.
A counted-key regression checks zero key clones at capture and two clones per cursor step for a 65,536-key snapshot.
`loom_startup_snapshot.rs` instruments the enclosing capture and mutation mutex around that same cursor.
The test explores capture before, between, or after removal and reinsertion.
It does not instrument internal `imbl` reference counts.
The [snapshot specification](../../../docs/casper/theory/finalized-floor/startup-snapshot-semantics.md) states the formal contracts and resource limits.

`property_startup_scan.rs` checks the production two-phase scanner against an eager snapshot reference.
Presence generation includes all-present, all-absent, and mixed snapshots.
The examples cover full-capacity completion, stable selection, cancellation, and errors in both phases.

## Startup completion ownership

`property_startup_completion.rs` checks the production completion kernel against an independent event-history reference.
`loom_startup_completion.rs` explores competing authorization, stop, replacement, cancellation, and retirement at the kernel mutex boundary.
These kernel tests do not verify wrapper lock placement by themselves.
The [completion specification](../../../docs/casper/theory/finalized-floor/startup-completion-identity.md) states the identity freshness contract and remaining integration obligations.

`startup_runtime.rs` imports the production owner and engine-cell source for native Tokio regressions.
The engine-cell tests substitute its surrounding context, engine-trait, and error dependencies.
They test actual publication guards, controller binding, cancellation, weak ownership, and rejected or retired engine destruction.
Owner tests include generated operation histories, suspended callbacks, exact cancellation, root destruction, and committed-result preservation.
The tests use actual notifications and futures, not a copied completion state machine.
They do not instrument Tokio internals with Loom.

Use `bash scripts/check-recovery-pump-control.sh startup-native` inside a memory-limited systemd scope.
That gate also runs a Casper publication/event integration regression and strict Casper and node lint.

## Recovery metadata readiness

`property_recovery_metadata.rs` imports the production admitted-read and observation-fold kernels.
Generated histories check visibility, row validity, first-error propagation, and complete examination after a missing dependency.
An independent reference computes the expected result and counts backend reads.
Source-group partition tests check that grouping does not change readiness.

`loom_recovery_metadata.rs` puts the production admitted-read kernel inside an instrumented read-write lock.
It explores row persistence, publication, later corruption, and two concurrent reader observations.
The configured exploration permits three threads and 1,000 branches without a permutation, duration, or preemption cutoff.
It does not instrument the production `parking_lot` lock implementation.

The native storage regression blocks an actual backend read and checks both enclosing production locks.
The Casper regressions compare borrowed dependency extraction with the canonical dependency set.
Examples also cover empty lists, absent certificates, sentinels, and repeated references whose results change.
Actual resolver tests use in-memory stores and a real `MultiParentCasperImpl`, without replay or network execution.

Run `bash scripts/check-recovery-pump-control.sh metadata-native` inside a memory-limited systemd scope with swap disabled.
This gate runs property, Loom, storage, dependency, and resolver tests with strict lint checks.
The [read contract](../../../docs/casper/theory/finalized-floor/recovery-pump-control.md#narrow-dependency-reads) states the formal correspondence and remaining integration limits.

## Active-pass demand transfer

`property_recovery_pump_control.rs` imports the production `RecoveryWake::merge` operation.
It checks identity, absorption, associativity, commutativity, idempotence, and transfer conservation across all four wake states.
Generated request partitions check arbitrary repeated transfers and terminal stop.
An example preserves a new proposal request across a failed older pass.

`loom_recovery_pump_control.rs` also checks transfers while two producers publish independently.
Another case races proposal publication, stop, and transfer into local demand.
These tests use the production compare-and-exchange and merge operations with Loom atomics.
They do not execute the unfinished production dispatcher.

Run `bash scripts/check-recovery-pump-control.sh handoff-native` inside a memory-limited systemd scope with swap disabled.
The gate runs seven property/example tests, six Loom tests, and strict standalone and Casper/node lint.
