# Buffer pruning and durable retry ownership

## Status and scope

This repair belongs to `pr216-admission-backpressure`.
The source audit, plan review, model counterexamples, and three production reproductions are complete.
Production pruning still has the defects described below.
The complete storage migration is not implemented or qualified.

The repair must preserve Casper validity, voting, certificate thresholds, fork choice, block encoding, and cost settlement.
The demonstrated failures lose recovery evidence.
They do not establish that a validator can admit an invalid block.
The ordinary resolver still checks authoritative metadata before admission.

The [admission plan](admission-and-recovery-backpressure.md) defines the containing task.
The [durable membership contract](buffer-durable-membership.md) defines the current row encoding.
The [dispatcher contract](recovery-dispatcher-composition.md) defines retry ownership and startup capture.

## Upstream review boundary

The user approved a demonstrated bug repair on 2026-09-07.
The user also required alignment with the upstream Casper architecture.
Use `dev` as the architecture baseline without discarding demonstrated fixes.
Existing comparison findings still require upstream Casper team review.
Formal verification does not replace that review.

The storage design below is a candidate, not an implementation mandate.
Persistent retry tickets, a derived namespace, disk-backed startup snapshots, and incremental cleanup require review before implementation.
First compare the smallest local repair with these alternatives.
Document each necessary difference, its evidence, its concurrency effects, and its migration requirements.
Do not infer architecture approval from the earlier bug-fix approval.

The pgmcp plan is `pr216-buffer-pruning-upstream-repair`.
Its nine tasks cover reproduction, provenance, design, upstream disposition, formal verification, implementation, regression testing, concurrency testing, and handoff.
Verified dependencies place reproduction before production changes.
The upstream disposition task is a required decision gate.
The containing admission task retains its other repair and verification obligations.

## Terms

A **durable retry row** identifies an unresolved buffered block in the existing `parents-map` store.
Its value contains unresolved block or certificate dependencies.
An empty value still identifies an explicitly buffered block.

A **resident cache** holds selected durable metadata in process memory.
A **cold row** remains durable but is absent from that cache.
A **retry owner** identifies where the node can discover unresolved work without another external announcement.

**Resolution** removes an obligation because authoritative terminal evidence permits removal.
**Eviction** removes cached data to enforce local resource limits.
Eviction is not resolution.

## Failure traces

### Eviction removes an unresolved edge

Consider an unavailable parent P, its buffered child B, and B's buffered child C.
The durable rows contain B's dependency on P and C's dependency on B.
Age pruning selects B because B waits below an unavailable root.

`enforce_limits` calls `remove`.
`remove_unlocked` deletes B's row and removes B from C's parent set.
C becomes a pendant in the buffer even though B has no admitted metadata.
Reopening the durable store preserves the incorrect projection.

The model's `resolve-on-eviction` control reproduces this trace.
`pruning_preserves_middle_block_dependency_across_restart` reproduces it through the actual storage implementation.

### Eviction removes the last retry owner

A buffered block can finish local processing while it still waits for dependencies.
`ack_processed` calls `ack_in_casper`, which removes its request-tracker record.
The durable buffer row then carries its retry obligation.

Age or pressure pruning deletes that row and its candidate entry.
The body can remain in BlockStore without remaining discoverable by ordinary buffer recovery.
Restart restores only surviving rows.
An external retransmission might recover the block, but it is not an ownership guarantee.

The model's `forget-retry` control explicitly acknowledges the block before eviction.
The isolated-owner regression reproduces the durable deletion under pressure.
That storage regression does not execute the request tracker.
The source trace and model cover the acknowledgment step separately.

### Eviction removes the last certificate waiter

Certificate maintenance derives active digests from buffered obligations.
Deleting the last waiting row also deletes the digest from that projection.
Request reconciliation can then discard the certificate request.

`pruning_preserves_last_certificate_waiter_across_restart` reproduces the missing dependency projection before and after reopen.
It does not execute network request dispatch.
The complete request-tracker integration regression remains required.

### Public-API regression for upstream review

The public fixture is [buffer_pruning_preservation.rs](../../../../block-storage/tests/buffer_pruning_preservation.rs).
It uses the buffer API that both branches expose.
One child waits for one unavailable parent.
Separate cases invoke age pruning and pressure pruning.
An unpruned control checks the same fixture and reopen behavior.

The feature run on 2026-09-07 selected all three tests.
Both pruning cases failed the preservation assertion.
The unpruned control passed.
Each failing case reported an absent durable row and absent dependency projections before and after reopen.
Both cases confirmed that pruning selected one entry.
No production source changed for this run.

The command was `cargo test --offline -p block-storage --test buffer_pruning_preservation -- --test-threads=1`.
The systemd scope used `MemoryMax=5G`, `MemorySwapMax=0`, and `CPUQuota=100%`.
The command used one build job and stored temporary artifacts under `target/verification`.
The expected reproduction exit was `101`, not a passing repair result.
The captured output is `target/verification/buffer-pruning/public-regression-20260907.log`.

The buffer implementation SHA-256 was `46bab7945cda1a7a20241acaccec0e32f349913b9aa8a6e0f49be913248bc443`.
The dependency DAG implementation SHA-256 was `60e5f43cd9ba3d16daf08fe31b2b9611096bef8adea24aadbe1f8083f46e51d1`.
The regression SHA-256 was `9a5bb3eefdcf1cad569f09d771f7930f5c80944f89880a6811e9a70f5b57051f`.

This fixture reopens the same in-memory store through a new buffer instance.
It does not simulate an LMDB crash, network recovery, or invalid-block admission.
### Controlled execution of pinned dev sources

The follow-up comparison pinned `dev` at `0f5d2b7414786cd27b8a686ca636485c35a5edce`.
The comparison compiled its unchanged buffer and dependency-DAG files with the current workspace dependencies.
The public fixture changed only its buffer import.
Both pruning cases failed the same preservation assertion.
The unpruned control passed.

The buffer Git blob was `1c15061f86aaecf2515ac96cb28c552830263bfd`.
The dependency-DAG Git blob was `545855f7db425f8de56461f7f3934bb7364ec2bc`.
These blobs match the earlier source audit.
The files, harness, and result summary are under `target/verification/buffer-pruning/dev-0f5d2b741478/`.
The temporary Cargo test target was removed after execution.

The command was `cargo test --offline -p block-storage --test buffer_pruning_dev_probe regression:: -- --test-threads=1`.
The build took 3.42 seconds.
The three selected tests took less than 0.01 seconds.
Fourteen unrelated upstream tests were filtered.
The scope used the same 5 GiB memory cap, disabled swap, and one build job.

This is an executed module comparison, not a complete `dev` build or network run.
The test establishes the same recovery-state loss in both buffer implementations.
It does not establish that either implementation admits invalid blocks.

### Relevant implementation differences

| Boundary | Pinned dev | Feature working tree | Review consequence |
| --- | --- | --- | --- |
| State mutation guard | Uses a buffer write guard. | Retains the guard. | Preserve its ordering with the DAG guard. |
| Removal cleanup | Removes parent links and handles orphaned references. | Retains cleanup and distinguishes explicit membership. | Do not restore dangling references. |
| Pruning victim | Selects a dependency-free root. | Selects waiters below a missing root when available. | Either selection can lose unresolved work. |
| Durable pendant | Deletes the final dependency row. | Retains an explicit empty row. | Preserve the intended retry guarantee or explicitly revise it. |
| Publication failure | Can change memory before fallible writes finish. | Commits rows before memory publication. | Preserve failure atomicity. |
| Certificate waiters | Has no certificate-key namespace. | Retains distinct certificate dependencies. | Include certificate ownership in any cold-storage review. |
| Candidate traversal | Uses complete in-memory graph traversal. | Adds rotation and ordered startup capture. | Treat stronger traversal guarantees as local design choices. |

## Minimal repair review

The second plan review separates the deletion defect from the broader resource design.
Stopping destructive pruning does not require persistent tickets or a new database.
However, skipping unresolved entries alone allows resident metadata to grow.
That option does not complete the approved resource repair.

| Option | Preserves unresolved ownership? | Remaining problem |
| --- | --- | --- |
| Restore only `dev` victim selection | No | The executed upstream fixture loses the same row. |
| Skip unresolved entries | Yes, at the pruning boundary | Unresolved resident metadata remains unbounded. |
| Remove only entries with existing terminal evidence | Yes | An unresolved backlog can still exceed memory limits. |
| Remove only leaf entries | No | A leaf can own the final durable retry row. |
| Transfer evicted work to BlockRetriever | Not established | The tracker is volatile, bounded, and can refuse entries. |
| Reject new work at buffer capacity | Not established | Rejection can lose ownership or prevent dependency progress. |
| Preserve durable rows and page resident metadata | Requires qualification | Lookup, restart, reconciliation, and capture need upstream review. |

### Why a buffer limit alone is insufficient

The [dependency-error handler](../../../../node/src/rust/instances/block_processor_instance.rs) calls `ack_processed` before returning the error.
That acknowledgment removes BlockRetriever tracking.
A new capacity error cannot use this path without a retained retry owner.
An initial storage-write failure needs the same ownership audit.
The complete storage-failure pipeline regression remains required.

A ready parent enters `commit_to_buffer(None)` before validation.
A blanket capacity rejection can therefore stop the parent that would release capacity.
Exempting needed parents solves only that immediate case.
A needed parent can reveal another missing ancestor.
An unlimited exemption removes the bound, while a fixed reserve requires an established dependency-depth or progress contract.

Waiting inside all workers can also stop the queued parent from starting.
Capacity deferral must transfer ownership without occupying every worker.
Dependency publication currently inserts relations one at a time.
A new capacity check must account for the complete mutation before publication.
An admission limit also cannot bound the current constructor, which loads all existing durable rows.

### Questions for upstream review

The history identifies pruning as part of memory-stability work.
The extracted workspace already contains the removal call in commit `c36613aa285ef912648ba3473dc2f4feed88957f`.
The feature branch adds stronger durable recovery claims and certificate-waiter handling.
The team must distinguish those guarantees from a deliberately best-effort upstream retry policy.
The regression proves loss, but does not settle that policy question.

1. Confirm the required retry guarantee after the node acknowledges a buffered block.
2. Specify whether capacity can refuse unrelated work before durable acknowledgment.
3. Specify how required dependencies progress when resident capacity is full.
4. Identify the retry owner after refusal, interruption, and restart.
5. Specify behavior when existing durable metadata exceeds the resident limit.
6. Review the smallest storage changes needed to satisfy those requirements.

The earlier retry-ticket proposal addresses local scan service, not a Casper protocol rule.
Do not make that proposal a prerequisite for fixing destructive removal without reviewing smaller alternatives.
Do not close the resource task with a safety-only containment that permits unlimited retention.

### Initial publication failure: source-level finding

The follow-up audit found an ownership gap before pruning can occur.
Assume a fresh hash has no prior buffer row, implicit dependency reference, or captured startup membership.
The node stores the body successfully, but the first buffer write fails.
The atomic storage helper leaves both the durable row and candidate entry absent.

The dependency-error handler then calls `ack_processed`.
BlockRetriever removes the hash from request tracking.
The worker records failure metadata, drops its queue item, and sends a hash-free recovery wake.
Ordinary recovery enumerates buffer candidates, while startup enumerates captured pendants.
Periodic maintenance examines buffer dependencies and existing retriever entries.
None of those inspected sources contains the fresh hash.

Another announcement or a newly buffered child could make the hash discoverable again.
That possibility does not establish a retained local retry owner.
An existing row can preserve an owner, so this trace requires the fresh-hash precondition.
The source audit does not replace a worker-level fault-injection regression.

The regression must fail the first buffer write after successful body storage and completed receive acknowledgment.
It must cover both ready-pendant insertion and first missing-dependency insertion.
It must restore writes and run recovery without new announcements.
It must also include successful-publication and preexisting-row controls.
A late receive acknowledgment must not mask the failing schedule by recreating tracking.
The restart case must check whether an actionable owner survives reopening.

The current storage proof qualifies atomic row publication, not the caller's acknowledgment after a failed publication.
The composed model must include that error-to-acknowledgment transition and the actual retry sources.
The relevant negative control is `AckAfterFailedReservation`, extended to actual storage failure.
Do not claim this runtime defect is demonstrated until the full regression reaches the specified boundary.

## Candidate storage design

Keep unresolved rows in the existing durable encoding.
Evict only resident data.
An empty resident cache must not imply an empty durable buffer.

| Boundary | Required behavior |
| --- | --- |
| Publication | Persist complete retry ownership before acknowledging buffered processing. |
| Resolution | Remove edges only with the existing authoritative terminal evidence. |
| Cache eviction | Preserve durable rows, reverse dependencies, and retry ownership. |
| Direct lookup | Consult durable state when a cache lookup misses. |
| Reverse lookup | Include cold children and cold certificate waiters. |
| Ordinary scan | Use an ordered durable cursor and an explicit round boundary. |
| Request cleanup | Establish authoritative absence or resolution before removing ownership. |
| Restart | Restore bounded working data without materializing every durable row. |

The key-value interface needs genuine ordered page reads.
Each read needs an exclusive cursor, a stable round boundary, and separate visit and output limits.
Restarting an iterator at the beginning does not provide bounded resumable work.
Collecting the whole store does not provide bounded temporary retention.

The in-memory backend must provide the same ordering contract as the persistent backend.
Fault-injection stores must preserve page-read failures and transaction failures.
No buffer, engine, or database mutex may remain held across an asynchronous wait.

A bounded request table can postpone dispatch while durable rows retain the obligation.
Reconciliation must not treat one resident page as the complete active set.
The current certificate API accepts a complete active set.
Its migration must preserve that meaning or replace it with an explicitly incremental ownership protocol.

## Candidate startup capture

Startup requires the complete captured membership and its phase-one presence selection.
A snapshot of resident rows would omit cold obligations.
Repeating presence checks in phase two would change the captured selection contract.

The existing lease bounds active and pending capture episodes.
It does not bound keys, bytes, or retained backend versions within one episode.
A whole heap set of cold keys therefore does not satisfy bounded resident retention.

The replacement needs an immutable durable view and bounded page buffers.
The lease must own that view, selected membership, cursors, and buffers through actual destruction.
The backend version-retention cost also needs a bound or explicit admission control.
This snapshot refinement remains a prerequisite to production migration.

## Storage-plan amendment under review

The approved task requires a process-local candidate index without a storage-format change.
That requirement predates the demonstrated pruning failures.
A derived persistent index would change this implementation constraint, although it would preserve existing parent rows and consensus bytes.
No such amendment has been applied.

### Candidate membership

Candidate membership is larger than the set of explicit parent rows.
The current constructor rebuilds implicit parent vertices from every stored dependency.
An unavailable parent can be a pendant without an explicit row of its own.
`add_relation_unlocked` adds both the parent and child to candidate rotation.

The replacement must preserve those implicit vertices.
It must distinguish an explicit empty row, an implicit dependency vertex, and an absent key.
Certificate dependency keys must remain distinct from block candidates.
A direct lookup that checks only explicit rows cannot replace all current buffer queries.

### Alternatives

| Design | Preserved property | Cost or missing guarantee |
| --- | --- | --- |
| Existing complete in-memory rotation | New arrivals cannot overtake existing candidates. | Every durable candidate remains resident. |
| Live lexical cursor with a fixed maximum key | Ordered traversal uses short read transactions. | New keys below the maximum can repeatedly precede an existing candidate. |
| Immutable per-round source | Captured membership cannot change during the round. | Each round must retain a backend version or materialize its complete key source. |
| Persistent monotonic retry tickets | New arrivals fall beyond the captured round tail. | Adds a derived index and an atomic publication/rebuild contract. |

The ticket design is the current recommendation, not an approved production change.
Each candidate keeps its ticket through cache eviction, duplicate announcements, and temporary admission rejection.
The ordinary round captures the current ticket tail once.
Each page seeks strictly after its previous ticket and does not extend that tail.
Resolution can remove a candidate.
A genuinely new candidate receives a later ticket.

This design must preserve a service-opportunity bound under new arrivals.
It does not establish wall-clock latency when the executor, storage, or admission capacity stops progressing.
The existing rotation proof establishes the analogous property for an in-memory queue.
A persistent implementation needs a separate representation proof and concurrent publication tests.

Cold reverse dependencies also need attention.
The existing value encoding stores each block's complete parent set in one value.
Without a reverse index, finding all cold children requires a complete store scan.
The proposed derived records below cover those requirements.

### Recommended amendment

The plan agent recommends one physical derived namespace, `casper-buffer-derived-v1`.
Tagged record types separate its indexes.
This name and record layout remain proposals until approval and detailed schema verification.

| Record type | Required meaning |
| --- | --- |
| `RetryByTicket` | Map each live candidate ticket to its block key. |
| `TicketByHash` | Retain the candidate's stable ticket and cleanup state. |
| `ReverseByDependency` | Enumerate all children of a block or certificate dependency. |
| `PendantByHash` | Identify exact dependency-free membership, including implicit vertices. |
| `IndexMeta` | Identify the format, generation, build state, and next ticket. |
| `TerminalCleanup` | Retain bounded cleanup progress until every affected record is reconciled. |

The index is derived from authoritative storage.
An absent, incomplete, or incompatible index must not authorize an `Absent` result.
Initial reconstruction must finish before the node enables buffer mutations.
An online reconstruction would instead require concurrent writers to maintain the building generation.
The initial implementation should use startup reconstruction and must not silently permit unsupported online reconstruction.

Publish a rebuilt generation only after complete coverage.
Invalidate old cursors when the generation changes.
The rebuild must include explicit rows, implicit vertices, reverse edges, pendants, and certificate dependencies.
It must retain unresolved explicit rows after their last incoming reference disappears.
It may remove an implicit vertex only after its last reference disappears.

Authoritative rows and their derived mutations must share a strict transaction within the buffer environment.
The store-manager mapping must place the new namespace beside `parents-map`.
This transaction cannot include the separate DAG environment.
The existing cross-environment crash-reconciliation requirement therefore remains necessary.

Terminal cleanup needs its own persistent record.
Each step may remove only edges justified by authoritative terminal evidence.
A partial step can retain conservative terminal edges.
It must not remove unresolved edges or report incomplete cleanup as complete.
The record remains available after failure or restart.

This incremental cleanup changes the current complete-removal operation boundary.
The existing atomic-removal proof cannot qualify the new boundary without a refinement.
In particular, the [buffer-DAG transition](../../../../block-storage/src/rust/dag/buffer_dag_transition.rs) and its callers need explicit correspondence checks.

Certificate reconciliation must use complete reverse-index coverage.
A page-local set must never erase a request that belongs to another page.
Request ownership must remain recoverable until terminal evidence permits removal.

### Implementation sequence after approval

1. Specify index publication, reconstruction, cursor identity, and terminal cleanup.
2. Extend the formal models with concurrent mutations, interruption, and generation replacement.
3. Prove the applicable parameterized preservation and selection properties.
4. Add ordered page and owned-view contracts to both storage backends.
5. Implement derived indexes and bounded resident caches.
6. Implement leased disk-backed startup sources and selected streams.
7. Integrate retry rounds, cold lookups, request reconciliation, and incremental cleanup.
8. Run production-linked property, Loom, crash, backend, and dispatcher tests.

These steps must preserve the current worker count and independent validator execution.
They must not change block validity, voting thresholds, certificates, fork choice, or cost settlement.

The affected interfaces include `KeyValueStore`, `KeyValueTypedStoreImpl`, LMDB and memory stores, environment caching, and store-manager mappings.
Buffer storage, candidate rotation, ordered snapshots, startup scanning, capture ownership, and the recovery driver also change.
Casper retry, reverse lookup, dependency maintenance, certificate reconciliation, and buffer-DAG cleanup require integration changes.
Fault stores and mocks must implement the same failure semantics.
No production interface in this amendment has been changed.

### Backend constraints

The local source audit used the locked `heed` version, `0.22.1`.
The following facts constrain the API design.

| Boundary | Source fact | Required consequence |
| --- | --- | --- |
| Transaction ownership | `Env::static_read_txn` returns a transaction that owns an environment clone. | An immutable view can have an owned lifetime without a self-referential wrapper. |
| Thread transfer | Only `RoTxn<WithoutTls>` has the required `Send` implementation. | An asynchronous owned view needs typed non-TLS environment construction. |
| Shared environment cache | `env_cache` tracks weak references to the returned `Arc<Env>`. | A view must retain that exact cache `Arc` until its transaction is destroyed. |
| Database ordering | LMDB keys use the outer `SerdeBincode<ByteBuffer>` encoding. | Page cursors must compare encoded storage keys, not assume raw-vector order. |
| In-memory ordering | The current backend uses an unordered `DashMap`. | The backend needs a matching ordered read representation. |
| Version retention | A live LMDB reader prevents reuse of pages freed by newer writes. | A capture-count bound is not a retained-page or database-growth bound. |
| Parent values | One encoded value contains an entire dependency set. | A row-count limit alone cannot bound decoding or atomic rewrite costs. |

The relevant project sources are
[the key-value interface](../../../../shared/src/rust/store/key_value_store.rs),
[the LMDB backend](../../../../shared/src/rust/store/lmdb_key_value_store.rs),
[the memory backend](../../../../rspace++/src/rspace/shared/in_mem_key_value_store.rs),
and [the environment cache](../../../../rspace++/src/rspace/shared/env_cache.rs).

An ordered page API must seek from an exclusive encoded cursor.
It must report visited entries separately from accepted output.
Borrowed values must not escape the synchronous read boundary.
The API must propagate decoding, cursor, and storage errors.
It must not implement paging through repeated scans from the first key.

Variable-length keys need cross-backend golden tests.
An encoded vector's length prefix can change its position relative to raw-vector lexical order.
Startup's `BlockHashSerde` order remains a separate requirement.
The startup source must sort and deduplicate in that order.

### Capture and resource limits

The proposed startup capture streams an immutable view into bounded-memory scratch runs.
It then releases the live database view before capacity waits.
External sorting produces an immutable candidate stream.
Phase one appends only present block keys to a selected stream.
Phase two consumes that selected stream without repeating phase-one membership decisions.

The existing startup lease must own the source, selected stream, buffers, and cleanup until their actual destruction.
Pending captures must not retain live database readers while an earlier capture waits for capacity.
Scratch files must reside in configured disk-backed storage, not RAM-backed temporary storage.

Finite capture counts do not imply finite capture bytes.
The implementation must define scratch reservation, exhaustion, cancellation, and read-version limits before claiming bounded retention.
A failed capture must preserve durable obligations and report failure.
It must not publish a partial source as a complete startup snapshot.

The bound report must distinguish these quantities:

- Resident cache bytes and retiring cache entries.
- Page buffers and copied key bytes.
- Dependency decoding and whole-row transaction costs.
- Scratch bytes and cleanup ownership.
- Pinned backend pages and retained database versions.

The current backend does not provide a hard per-reader pinned-byte limit.
No such guarantee follows from the existing two-episode startup ledger.
The complete migration remains subject to the explicit plan amendment and formal refinement.

## Formal evidence

### Graph model

`BufferPruningObligations.tla` separates durable rows, dependencies, cache entries, request records, acknowledgments, certificate requests, and restart.
Its transitions interleave those operations.
It does not assume that acknowledgment preserves a request record.

The initial gate `run.ziyKTJ` completed with exit code zero.
Each unsafe control failed with exit code 12 and its exact invariant name.

| Configuration | Distinct states | Result |
| --- | ---: | --- |
| Chain | 1,797 | All reachable states satisfied the configured invariants. |
| Shared certificate | 2,351 | All reachable states satisfied the configured invariants. |
| Isolated blocks | 1,219 | All reachable states satisfied the configured invariants. |
| Join dependencies | 2,079 | All reachable states satisfied the configured invariants. |

Each configuration uses three block identities, one certificate identity, and two abstract resident slots.
These finite configurations do not establish arbitrary graph sizes.

| Control | Required failure | Evidence class |
| --- | --- | --- |
| `resolve-on-eviction` | `Inv_Obligations` | Direct production defect. |
| `forget-retry` | `Inv_DurableRetry` | Direct production defect after acknowledgment. |
| `hot-only-certificates` | `Inv_CertificateCoverage` | Hazard in the proposed paging migration. |
| `restore-all-resident` | `Inv_ResidentBound` | Eager reconstruction violates the proposed cache-slot bound. |

The certificate control does not directly reproduce current last-waiter deletion.
Current pruning deletes the durable row as well.
The durable-owner invariant and native certificate regression cover that loss.

### Parameterized lemmas

`BufferPruningPreservation.v` proves preservation over arbitrary key types, dependency relations, cache capacities, and finite histories.
The relation can contain conservative extra edges.
Its unresolved-edge obligation requires inclusion, not exact canonical normalization.

Fourteen theorem checks cover publication, resolution, cache replacement, restart, acknowledgment, failed writes, and finite transition histories.
The final inclusion-based proof and independent kernel validation passed in `rocq.F94IX3`.
The two rejection lemmas prove that deleting an unresolved row or required edge violates the corresponding invariant.
Terminal evidence is a parameter of this component proof.
The proof does not establish the Casper rules that produce that evidence.

### Explicit limits

The graph model is a normalized graph contract, not complete production refinement.
Its `Buffer` action publishes all dependencies together.
Current `commit_to_buffer` performs separate relation writes.
Intermediate publication, partial failure, and restart during publication need their own refinement.

`FailedWrite` imports an unchanged-state contract.
It does not prove backend failure atomicity.
The existing transaction proof must be composed with the new storage boundaries.

`Inv_CanonicalRows` is stronger than necessary runtime preservation.
Production can conservatively retain an edge after metadata admission.
`Inv_Obligations` is the required preservation property.

The cache bound counts abstract row slots.
It excludes decoded dependency sets, page buffers, retiring entries, and captured backend versions.
The model has no temporal fairness property.
It does not prove eventual paging, bounded request delay, wall-clock latency, RSS, or uptime.

## Native evidence and acceptance

The three example regressions failed against unchanged production in `native.UkrMPy`.
Each failed for its preservation assertion after successful compilation.
No timeout, parser failure, or resource termination qualified as a reproduction.

The generated regression applies insertions, dependency additions, age pruning, pressure pruning, restart, and explicit resolution.
It compares durable rows against an independent map after every operation.
Its configuration uses 64 cases, eight synthetic keys, and sequences of up to 64 operations.
Only explicit resolution may remove expected rows or dependency edges.

The combined gate `run.gC3dPb` passed every formal leg, then failed all four native regressions against unchanged pruning.
The generated test reduced the failure to two operations: insert a pendant, then apply age pruning.
Its persisted regression seed is `04a63fc19553bf5605606c0dabece06ffadda08d69698792926cf7dcf6eda108`.
The failure occurred before 64 successful generated cases.
The configured case count is not a successful coverage count.

Run the formal gate with:

```bash
systemd-run --user --scope -p MemoryMax=2G -p MemorySwapMax=0 \
  -p CPUQuota=100% bash scripts/check-buffer-pruning-obligations.sh formal
```

Run the full component gate with:

```bash
systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0 \
  -p CPUQuota=100% bash scripts/check-buffer-pruning-obligations.sh
```

The full gate requires all four native regressions to pass before strict storage lint runs.
The gate stores logs and input hashes under `target/verification/buffer-pruning/`.
It rejects input changes during execution.
The currently failing regressions remain release blockers.

Production qualification must also cover concurrent edge insertion, pruning during capacity waits, request cleanup, partial writes, and cold startup capture.
Production-linked Loom tests must cover the actual cache and cursor kernels.
The complete dispatcher, storage, requester, and snapshot composition must pass before this task closes.
