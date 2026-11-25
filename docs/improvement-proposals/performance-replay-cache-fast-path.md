# Audit Replay Cache Hit Initialization

Status: draft audit and optimization proposal. Runtime construction must not be removed without side-effect evidence.
Date: 2026-11-25.

## Objective

Determine whether replay-cache hits can avoid temporary runtime construction and event-index assembly.
Preserve replay validation, mergeable recovery, root lifetime, and initialization effects.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Hit Path

The [runtime manager](../../casper/src/rust/util/rholang/runtime_manager.rs) builds a replay-cache key from pre-state, sender, sequence, and payload fingerprint.
The fingerprint also distinguishes genesis handling through its inputs.
The [cache entry](../../casper/src/rust/util/rholang/replay_cache.rs) holds a shared event log and post-state hash.
It is not a complete persisted mergeable-state record.

On a hit, `replay_compute_state` checks for the associated mergeable-store entry.
If that entry is absent, it falls through to full replay to reconstruct it.
That fallback is essential recovery behavior, not redundant computation.

When the mergeable entry is present, the method spawns a replay runtime.
It converts cached events, rigs that runtime, and returns the cached post-state.
The temporary runtime is not returned to the caller.
This makes initialization cost a candidate for investigation, not proof that initialization is unnecessary.

## Why Deletion Is Not Yet Safe

[Runtime construction](../../rholang/src/rust/interpreter/rho_runtime.rs) builds the environment and system processes.
With registry initialization enabled, it bootstraps the registry and creates a checkpoint.
The replay spawn path enables that initialization.
It also installs external service context and filesystem slot state.

The [replay space](../../rspace++/src/rspace/replay_rspace.rs) resets history, restores installed processes, and builds a hot store during spawn.
Its `rig` method constructs replay indexes from events.
These structures are temporary on this hit path, but construction can still fail or affect storage and resource lifetimes.

Deleting spawn and `rig` could remove required side effects or suppress existing errors.
Returning a cached hash is also insufficient if pruning made required state unavailable.
The audit must distinguish an optional optimization from an existing validation or recovery obligation.

## Phase 0: Trace the Contract

Measure hit lookup, mergeable-presence check, runtime spawn, registry initialization, checkpoint, event conversion, and rigging separately.
Keep miss and missing-mergeable recovery paths as distinct outcomes.
Establish cache-hit frequency and initialization work per hit before optimizing any path.
This phase changes observability only if separately approved.

For each initialization operation, record:

| Audit item | Question to resolve |
| --- | --- |
| Registry bootstrap | Does the hit path rely on new persisted state or installed process definitions? |
| Initialization checkpoint | Does it publish, pin, or refresh a root that another caller needs? |
| History reset and installed processes | Which availability checks or errors disappear if spawn is skipped? |
| Filesystem and external services | Which registration, snapshot, handle, or cleanup effects are observable? |
| Event conversion and rigging | Is any validation performed, or only temporary index construction? |
| Cancellation and drop | Which permits, guards, references, or handles are released? |
| Concurrent pruning | What guarantees the cached post-state and mergeable record remain usable? |

Document evidence from callers and fault-injection tests for each answer.
Do not infer that an operation is unnecessary merely because its return value is unused.

## Phase 1: Explicit Validated Hit Path

Proceed only if the audit identifies operations that are genuinely redundant.
Factor any required readiness or lifetime checks into a narrow helper.
Retain runtime initialization where its effects cannot be replaced with an equivalent smaller operation.
Skipping all construction is one possible result, not a predetermined design decision.

Keep the existing replay-cache key and payload checks unchanged in the optimization patch.
Do not broaden cache eligibility or trust a peer-supplied result as a local validated entry.
Treat current key sufficiency as a prerequisite for the audit, not a new proof supplied by this proposal.

Keep the mergeable-presence check and the full-replay fallback when that record has been collected.
A successful result must leave the recovery caller's required state available.
If a new readiness check detects missing state, follow the established recovery path.
Do not classify locally missing storage as successful validation or a slashable remote fault.

Check state lifetime under the existing pruning and root-pin protocol.
A lookup followed by an unprotected return does not prevent concurrent collection.
Use the established guard lifetime where needed rather than retaining roots indefinitely in the cache.
Do not bypass replay admission limits when the hit path falls back to execution.

## Excluded Changes

- No pooling or reuse of mutable runtimes between requests.
- No increase in replay concurrency or cache capacity.
- No removal of payload fingerprints or mergeable-record recovery.
- No new persisted cache format or peer-shared execution result.
- No weakened initialization, corruption, or validation errors without separate behavior review.

Runtime pooling would require an independent reset protocol for hot state, random state, cost, and external services.
It is not a small extension of this audit.

## Regression Plan

| Case | Required assertion |
| --- | --- |
| Cold replay and valid warm hit | Same post-state, validation result, required stored state, and relevant side effects |
| Hit with collected mergeable entry | Full replay recreates the entry before success |
| Collected or corrupt post-state data | Established recovery or error behavior, never an unchecked cached success |
| Concurrent pruning during hit processing | Required roots and records remain available for the caller's guarded operation |
| Same sender and sequence with changed fingerprint inputs | No unsafe cache hit |
| Genesis versus ordinary replay | Existing context separation remains intact |
| Initialization or external-service failure | No suppressed required failure or leaked resources |
| Cancellation at each phase | Permits, pins, references, and handles are released correctly |
| Rejected counter deploy recovery | Exactly-once canonical effects and unchanged rejection verdicts |

Retain the existing missing-mergeable regression as a mandatory compatibility case for this candidate.
Compare durable effects and error behavior, not only the returned hash.
Required formal evidence remains governed by the existing verification policy.

## Benchmark and Decision

Use locally produced cache hits, foreign-block misses, and hits requiring mergeable reconstruction.
Vary event-log length and history-cache warmth.
Measure runtime creations, registry checkpoints, rigged events, CPU, allocations, and latency percentiles.
Report hit-path improvement separately from aggregate replay throughput.

Stop if required initialization effects cannot be preserved with a simpler operation.
Stop if hits are too rare for the measured benefit to justify another execution path.
Retaining the existing behavior with better phase measurements is a valid audit outcome.
Any accepted optimization must support rollback without a state migration.
