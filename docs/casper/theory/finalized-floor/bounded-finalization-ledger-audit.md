# Bounded finalization-ledger audit

## Status and scope

This document specifies the `pr216-ledger-startup` repair.
The relevant Rocq proofs and model counterexamples preceded the production repairs.
The local repair now has passing proof, native, concurrency, crash, and resource evidence, as recorded below.

Fixed-target record scans, projection guards, incremental integrity pages, receipt deletion pages, and effects-cursor pages now exist in the working tree.
Ledger point reads now check allocation bounds before deserialization.
Recovery enumeration uses borrowed rows and retains only recovery records.
These results do not establish complete campaign verification or the required shard soak result.

The repair changes local audit scheduling and recovery memory use.
It does not change Casper votes, certificates, fork choice, finalization thresholds, or cost allocation.
The [atomic finalization contract](finalization-atomicity-and-recovery.md) remains authoritative for publication and effects.

## Original behavior and remaining limits

The original `FinalizationLedger::validate_integrity` read one round record and its witness at a time.
It did not collect the complete history.
However, it held the append lock throughout the historical audit.
Total validation work still grows with the records and witness bytes that the audit reads.

The original `records_after` collected the complete unfinished suffix.
Constructor projection, runtime projection, and effect recovery now use fixed-target record scans.
The original receipt compaction collected all deletion keys across its unfinished suffix.
Receipt compaction now retains one round manifest and one deletion page.

`record_round_effects_completed` now divides a long contiguous sequence of previously completed rounds into bounded pages.
Separately, metadata projection traverses finalized blocks and propagates fault tolerance.
Ledger paging does not automatically bound that metadata work or the whole-DAG index.

## Implemented integrity pages

`begin_integrity_scan` captures the genesis anchor and durable head under the append lock.
The opaque scan starts with only the genesis prefix validated.
The scan does not accept a saved cursor from a caller or persistent store.

`validate_next_page` checks at most its positive record budget before it releases the append lock.
Each page verifies the current anchor, head, cursor bounds, and captured endpoint.
Each new record receives the same record and witness validation as the original full audit.
Only a completely successful page advances the process-local validated prefix.

A failed scan remains failed.
Repair of the underlying data does not reactivate that scan.
A new scan must restart validation at genesis.
Successful completion requires equality with the exact captured head.
Later supported appends do not move the captured target.

The synchronous adapter uses 32 records per page.
This local scheduling limit does not change the accepted ledger history.
The asynchronous adapter schedules one blocking page at a time.
Storage construction awaits this adapter before projection recovery and before it returns usable storage.

Cancellation prevents the asynchronous adapter from scheduling another page.
An active blocking page can finish, then releases its scan and store references.
The cancellation regression observes both the active-page boundary and final store-reference release.
It does not infer worker termination from caller cancellation alone.

The counting-store regression rejects whole-store iteration.
For an advanced ledger, each page performs six endpoint and cursor reads, plus two reads for each newly validated round.
This read-count bound does not establish a decoded-byte or elapsed-time bound.
The point-read path now uses borrowed outer payloads and checks inner layouts before deserialization.
Recovery enumeration now validates borrowed row layouts before selective decoding.

## Audit identity and readiness

A **scan target** contains the captured genesis anchor and exact durable head.
The head includes its revision, block identity, height, record digest, and certificate digest.
A **validated prefix** contains every successfully checked round from genesis through the current audit cursor.

An integrity scan is opaque and process-local.
The scan cannot accept a caller-supplied cursor as evidence that earlier records passed validation.
Restart begins integrity validation at genesis.
Durable projection and effects cursors retain their separate recovery roles.

The constructor must not return usable storage until validation and required projection recovery succeed.
Cancellation or an error cannot create readiness.
A checkpoint stored beside mutable history cannot justify skipping unread history after restart.

The constructor regressions invoke `BlockDagKeyValueStorage::new`, not only the ledger's audit adapter.
One test injects witness-read errors at revisions 1, 31, 32, 33, 64, and 65 of a 65-round history.
Each exact error must reach the caller before any projection-cursor write or usable storage result.
Another test cancels construction during the first or second audit page.
The active blocking page may finish, but no later page starts from that caller.
The test waits for release of the constructor's last observed store reference.
It then corrupts the first witness and requires a new constructor to reject that previously audited position.
The existing successful-restart tests separately require completed projection and the exact captured finalization base.

Capture the scan target under the ledger lock.
Validate a bounded page against that target.
Release the lock between pages.
Use a blocking adapter for synchronous storage operations.
Do not retain an LMDB read transaction across an asynchronous yield.

Each scan checks the following properties.

1. Require a complete bootstrap or an empty store.
2. Validate the genesis anchor, durable head, and cursor bounds.
3. Require every revision through the captured head.
4. Validate each predecessor, record digest, manifest digest, and increasing finalized height.
5. Require and validate each portable witness.
6. Compare the witness target, height, and finalized manifest with its round record.
7. Require the final validated head to equal the captured head.
8. Publish readiness only after required metadata projection succeeds.

Supported writers cannot replace committed records or witnesses.
This immutability premise permits a logical snapshot without a long-lived database transaction.
Later appends extend the suffix and do not change the scan target.
The implementation must reject a detected head regression or conflicting replacement.

This contract does not guarantee detection of an arbitrary coherent rewrite of all storage and its trust anchors.
It also does not make a mutable backend immutable.
Backend ownership and transaction guarantees remain explicit premises.

## Resource contract

Record count, decoded bytes, and receipt-key count require separate bounds.
A one-record page can still contain a large witness or manifest.

The current certificate and witness validation paths impose these limits.

| Field | Existing limit |
| --- | --- |
| Portable certificate encoding | 2 MiB |
| Exact latest messages | 10,000 entries |
| Shard identity | 256 bytes |
| Local supporting manifest | 262,144 hashes |
| Local finalized manifest | 262,144 hashes |

`LocalFinalizationWitness` stores full manifests, while its portable certificate stores manifest digests and counts.
The certificate encoding limit therefore does not bound the complete local witness to 2 MiB.
These limits also do not establish a strict decoder byte bound.
For example, malformed serialized lengths can affect allocation before structural validation.

The repair must preserve all historically valid records.
A local page target cannot become a new consensus rejection rule.
The implementation must identify the maximum valid item representation and its transient decoding and hashing allocations.
An oversized valid item requires separate processing within that documented bound.

The generic typed-store read path decodes and clones values before returning them.
Its LMDB backend also decodes an outer byte-vector representation.
A strict malformed-row bound must apply before both decoding layers allocate from untrusted lengths.
The audit cannot establish that bound through a check after decoding.

### Decoder evidence and remaining design

The dependency lock selects bincode 1.3.3, heed-types 0.21.0, and serde_core 1.0.229.
The heed `SerdeBincode` codec calls slice-based `bincode::deserialize` for the outer byte vector.
The typed store calls the same bincode entry point for the inner value.

Bincode's slice reader checks available bytes before it copies a byte buffer or string.
Serde also limits initial sequence reservation through its cautious size hint.
A forged length alone therefore does not establish an arbitrary allocation from that length in this path.
These protections do not provide the required per-record resource contract.
A physically large row can still be copied and decoded before ledger validation rejects its contents.

The decoder repair must preserve the existing encoding and all valid writer-produced rows.
It must inspect outer encoded lengths before copying payloads.
It must apply field and container limits before allocating inner values.
A check after generic typed-store deserialization is insufficient.

The local witness requires a different bound from the portable certificate.
Its current bincode representation has nine hash fields, three collection lengths, and variable shard and manifest fields.
Let $`n`$ denote latest-message entries, $`s`$ supporting hashes, $`f`$ finalized hashes, and $`d`$ shard bytes.
The candidate encoded witness bound, including its ledger-value discriminant, is:

```math
B_{\mathrm{witness}} = 432 + d + 113n + 40s + 40f.
```

The existing field limits give 22,102,208 bytes for this representation.
The outer byte-vector length adds eight bytes.
This calculation is an encoded-size bound, not a heap or resident-memory measurement.
The maximum-witness regression confirms this size against the actual serializer.
It also passes standalone witness validation and tests every collection limit with over-limit and maximum-integer declarations.

Other ledger row variants need separate contracts.
In particular, the recovery-episode validator does not currently impose the witness shard-length limit.
A global ledger-value cap can therefore reject an otherwise valid row.
The repair must select bounds by the expected row type and resolve these differences explicitly.

### Borrowed-read contract

The selected design adds a required borrowed-read operation to the storage trait.
On a successful read, each backend invokes its callback exactly once for a present or absent key.
An outer decoding error returns before callback entry.
The callback receives a stable slice and cannot retain that borrow after the read returns.
The callback must not reenter storage or acquire another store guard in the same backend environment.

LMDB supplies the encoded value through a read transaction and a raw-value codec.
The outer byte-vector decoder returns a borrowed payload after checking its length against available bytes.
It does not allocate a payload vector.
The in-memory backend holds its existing read guard while the callback examines the stored slice.
Both backends release their read resources when the callback returns, including error returns.

The ledger callback checks the expected value discriminant and every length-bearing field before it calls the existing typed deserializer.
The layout check uses slices and checked arithmetic, not temporary collections.
Collection counts must satisfy both the existing semantic cap and the available encoded-byte bound.
Hash and validator fields must have their required lengths.
The normal serializer remains unchanged.

Recovery charge strings retain their existing accepted range.
Their allocation bound depends on the checked, physically present string length.
The bounded audit rows instead have fixed ceilings from existing certificate and manifest limits.
This distinction preserves valid recovery rows without claiming a constant size for an unrestricted owned string.

Rocq specifies checked spans, complete layout consumption, and collection count bounds before production implementation.
The proofs establish available-byte bounds, semantic limits, rejection, completeness for valid spans, and arithmetic bounds.
Serializer round trips and malformed-length properties must connect the concrete layout checker to those obligations.
Backend concurrency tests must confirm the borrowed snapshot and resource-release contracts.

The LMDB regression confirms that the callback receives the mapped payload address, not an allocated copy.
Another regression commits a replacement from a separate thread while the callback retains the old snapshot.
The callback observes the old bytes until return, and the next read observes the replacement.
The in-memory regression checks the original value address and the held read guard.
Both tests check release after a callback error.
No production serializer change or new whole-ledger size cap accompanies this API.

### Recovery enumeration and admission summaries

`settled_recovery_state` previously used the typed store's `to_map` method during startup recovery.
That call copied and deserialized unrelated round records and witnesses before selection.
The replacement uses `visit_entries` to inspect every raw row through one borrowed snapshot.
Only recovery charges and usage rows become retained typed values.

The scan preserves recovery key/value mismatch checks, charge validation, digest checks, and duplicate usage detection.
The caller preserves exact usage totals, duplicate-target rejection, and atomic migration.
The admission visitor validates each complete target and citer before it emits a compact summary.
Validation-only startup counts admissions without collecting summaries.
Reconciliation retains summaries without retaining admitted block bodies.

In-memory migration now stages only changed keys instead of copying every participating store.
These changes remove identified copying paths.
They do not establish constant total recovery memory or identify the cause of a reported CI memory failure.
The [resource report](bounded-finalization-ledger-resources.md) measures retained recovery records, admission summaries, mutation preparation, and metadata projection separately.
It states the input sizes, measurement method, results, and remaining whole-system limits.

Let $`H`$ denote the captured revision.
Let $`s_i`$ denote the bytes read and validated for round $`i`$ and its witness.
Complete historical validation retains total work proportional to the complete input:

```math
W_{\mathrm{audit}} = O\!\left(H + \sum_{i=1}^{H}s_i\right).
```

Bounded steps control scheduling intervals and temporary retention.
They do not make complete historical validation constant-time.
End-to-end startup measurements must report metadata projection separately.

## Recovery and receipt deletion

### Projected effect selection

The source review found a mismatch with the existing projection contract.
The original startup checks compared each cursor with the head but omitted the requirement that effects remain behind projection.
The original receipt and round-completion methods also omitted this projection guard.

The original runtime recovery selector had a related two-snapshot gap.
It first reconciled projection, then read pending effects through a separately captured head.
Another worker can append a new round between those operations.
The selector can therefore return an unprojected round.

The following schedule requires no invalid block or changed consensus rule.

1. Worker A completes projection through revision zero.
2. Worker B appends revision one.
3. Worker B pauses before projection.
4. Worker A selects pending effects through revision one.
5. Worker A can start effects while the projection cursor remains zero.

Source inspection identified this schedule, and the pre-fix Rust regression reproduced its incorrect selection.
It does not establish that this schedule caused a reported CI consensus failure.
The five projection-order regressions failed before the repair and passed afterward.

The repair must capture the effects cursor, projection cursor, and head together.
Selection must stop at the captured projection cursor.
The effect entry point must check projection readiness before its first external action.
Receipt and completion writes must enforce the same bound under the ledger lock.

Projection lag is a retryable readiness condition.
An already persisted effects cursor ahead of projection is an integrity error.
The repair must not reset cursors or synthesize projection to hide that error.

An older projected round remains eligible for effects.
The readiness check therefore uses an upper-bound comparison, not equality with the current projection cursor or head.
This preserves concurrent append and out-of-order completion of projected rounds.

`FinalizationEffectSelection.tla` models two workers, separate projection and selection stages, append, direct effect entry, receipts, and restart.
Its unsafe controls select a newer head or defer readiness checks until receipt creation.
Both controls must expose an external effect before projection.
The safe selector and direct-entry guard must preserve the projected-prefix invariant.

The Casper caller regression executes `apply_finalization_effects` against a committed but unprojected round.
It requires the pending-projection error before counter updates, block lookup, or ledger mutation.
An exhausted counter and a missing block make an incorrectly ordered guard observable.
The property test varies committed rounds, projected prefixes, and requested revisions.
Projected requests reach block lookup, including requests for older rounds.
Unprojected requests leave the counter, ledger, and event buffer unchanged in the example regression.
These tests exercise entry ordering, not successful completion of every downstream effect.

### Bounded recovery passes

Each recovery pass captures a finite upper revision.
The pass reads records in order without collecting the complete suffix.
A later pass handles records appended after capture.

Projection advances its durable cursor only after metadata updates succeed.
Effects retain their existing idempotent operation and receipt order.
Bounded effects-cursor advancement must stop at the first incomplete round.
Concurrent completion cannot cause the cursor to skip a gap.

Receipt compaction uses bounded deletion pages within a round.
The process retains its current position, but the durable compaction cursor remains a whole-round cursor.
Advance that cursor only after every receipt key for the round has been deleted.
Restart can repeat deletion from the last durable round boundary.

The following pseudocode states the required order.

```text
capture a finite completed-effects target
for each uncompacted round through that target:
    read the immutable round manifest
    for each bounded receipt-key page:
        delete that page durably
    delete the round-completion marker
    persist the whole-round compaction cursor
```

A strict transaction can combine final-page deletion, marker deletion, and cursor advancement when the backend supports that operation.
The cursor must never precede successful deletion.
Partial deletion with an unchanged cursor is recoverable through idempotent retry.

Durable completion and physical receipt presence are different concepts.
An effects cursor certifies completed work even after individual receipts are removed.
`effect_completed` must preserve this distinction after restart.

### Implemented effects-cursor pages

`begin_effects_cursor_advance` captures the projected revision under the append lock.
The scan advances through durable completion markers and stops at the first missing marker or its captured target.
Later projection does not extend that target.

`advance_next_page` checks at most its positive round budget.
Each page checks four durable endpoints before it reads completion markers.
The default adapters use 32 rounds per page.
The append lock protects each page and is released between pages.

A page starts from the current durable effects cursor.
It does not overwrite progress from another worker with its older scan-local cursor.
The scan rejects cursor regression and inconsistent endpoint ordering.
It publishes page progress only after the durable cursor write succeeds.
A failed scan requires a fresh scan from durable progress.

Round completion and cursor advancement are separate durable phases.
The completion phase checks projection and every required receipt before it writes the completion marker.
The cursor phase can then include that round when all earlier rounds are complete.
Startup resumes durable markers before receipt compaction and pending effect selection.

The asynchronous adapter schedules one blocking page at a time.
Cancellation permits its active page to finish but prevents another page from that caller.
The regression aborts the caller during a cursor write and waits for the worker to release its storage reference.
It then checks the exact durable page boundary and completes recovery with a fresh adapter.

Property tests vary completion gaps, page budgets, and restart boundaries.
They require the exact contiguous completed prefix, monotonic progress, and the page-step bound.
Separate tests inject a cursor-write failure and interleave two cursor workers on different operating-system threads.

### Implemented receipt pages

`begin_effect_compaction` captures the completed-effects target and current compaction cursor under the append lock.
The scan retains this target when later rounds complete.
`delete_next_page` reads at most one new round manifest and deletes at most its positive key budget.
The default synchronous adapter uses 256 keys per page.

The key iterator emits all four receipt kinds for each finalized block.
It emits the round-completion marker last.
The key budget includes that marker.
The iterator retains at most one additional key for lookahead.

The scan advances the durable cursor only after the final deletion page succeeds.
A deletion or cursor-write error permanently fails that scan.
A new scan resumes from the durable whole-round cursor and can repeat completed deletions.
An incomplete deletion never authorizes a cursor advance.

The append lock protects each page, but the scan releases it between pages.
Concurrent compactors can repeat deletion without changing logical effect completion.
If another compactor advances the durable cursor, a scan discards its obsolete iterator.
A detected cursor regression fails the scan.

The regression tests inject partial deletion and cursor-write failure.
Other tests check batch size, concurrent compactors, fixed targets, generated page partitions, restart, and preservation of unrelated storage.
The asynchronous adapter schedules one deletion page per blocking task.
Casper awaits that adapter during recovery and after durable effect completion.
Cancellation allows the active page to finish but prevents that caller from scheduling another page.
The cancellation test observes final store-reference release and the unchanged receipts of later rounds.
A separate test compares synchronous and asynchronous storage results across out-of-order completion and duplicate completion.
The point-read decoder now enforces its row contract.
Recovery enumeration now uses borrowed rows.
The resource report supplies separate phase measurements, not a constant end-to-end startup memory bound.

### Completion queries during compaction

A completion receipt can disappear after the effects cursor covers its revision.
The logical completion state must remain true across this deletion.
A query must therefore consider both the receipt and the effects cursor.

The previous query used two separate reads.
It first read the cursor and then read the receipt.
A worker could advance the cursor and delete the receipt between those reads.
The query then returned false although the effect remained complete throughout the query.
The controlled regression reproduced this failure against both the in-memory backend and LMDB.

The query repair must use this order:

1. Read the effects cursor.
2. If the cursor covers the requested revision, return true.
3. Read the receipt.
4. If the receipt exists, return true.
5. Otherwise, read the effects cursor again and return whether it covers the revision.

Each executed read must preserve its storage or decoding error.
Successful earlier reads must not suppress a later executed read's error.
An earlier true result requires no further reads.

The repair requires a monotonic effects cursor and fresh point-read snapshots.
The page writer retains its existing append guard and reads current durable progress before advancement.
Compaction deletes receipts only below the completed-effects boundary.
LMDB opens a read transaction for each point read.
The in-memory backend acquires its existing shared coordinator guard for each point read.

A true result has a witness at the covering cursor read or the present-receipt read.
If the final cursor remains below the requested revision, the cursor was also below that revision at the earlier receipt read.
An absent receipt therefore gives a valid false result at that earlier read.
This is the query's linearization point, which means one instant during the query that explains its result.

The repair adds at most one point read.
It adds no reader lock, retry loop, persistent row, or serialized field.
It does not change Casper votes, certificate thresholds, fork choice, or cost allocation.
It does not make the separate query-and-external-effect sequence atomic or establish exactly-once event delivery.
The reproduced local race does not establish the cause of any earlier CI failure.

`FinalizationEffectObservation.tla` separates cursor reads, receipt reads, completion, cursor advancement, receipt deletion, and read errors.
Its configured domain contains two readers and two revisions.
The receipt-read cursor is a specification-only observation, not an additional production read.
`Inv_FalseWitness` requires a false result to have an incomplete receipt snapshot.
`Inv_TrueWitness` requires a true result to have a covering cursor or a present receipt.
The safety predicate also tracks snapshot bounds, pending queries, receipt meaning, and preservation of logical completion.

The Rocq theorem `completion_query_is_linearizable` covers arbitrary natural-number revisions and monotonic cursor observations.
`completion_query_cannot_lose_a_completed_receipt` proves that completion at the receipt snapshot cannot produce a false result.
These theorems assume coherent snapshots and monotonic cursor writes.
They do not prove the storage adapters or extract executable Rust.

The production-linked Loom test imports the query kernel by path.
Separate store guards keep successive reads independently interleavable.
The model covers all four effect kinds, two query threads, and receipt creation or removal.
Its negative control retains the old two-read sequence and requires the named false-negative assertion to fail.
The finite test does not claim coverage of every production schedule or process-crash durability.

### Shared page implementations

The native ledger and Loom tests now use the same page implementations.
The [recovery kernel](../../../../block-storage/src/rust/finality/finalization_ledger/recovery_pages.rs) contains selection, readiness, advancement, receipt iteration, and compaction.
The [integrity kernel](../../../../block-storage/src/rust/finality/finalization_ledger/integrity_pages.rs) contains target capture and complete audit-page transitions.
The native adapters retain the existing record validator, storage operations, error types, and append lock.
These extractions do not introduce another lock or change stored bytes.

| Boundary | Shared behavior | Separate production obligation |
| --- | --- | --- |
| Audit capture | Bootstrap cases, fixed genesis and head, endpoint validation before capture | Native endpoint decoding and identity validation |
| Audit page | Cursor checks, bounded validation, exact target, publication after complete success, terminal failure | Native record and witness validation |
| Effects page | Current durable cursor, captured projection target, completion gaps, write before local publication | Durable marker and cursor storage |
| Receipt page | Current durable cursor, stale iterator removal, bounded deletion, cursor write last, terminal failure | Durable deletion and cursor storage |
| Receipt iterator | Every effect kind per block, marker last, cached lookahead | Native key encoding and complete round manifest |

The integrity Loom tests use two readers with different captured targets and a concurrent append.
Other cases exercise coherent cursor snapshots, invalid records, conflicting endpoints, failed-page rollback, and restart.
The missing-gate control must expose a false cursor-integrity rejection.
The model validator checks ordering and explicit validity flags, not cryptographic hashes.
Native tests supply the concrete validation evidence.

Each page model permits at most four threads and 5,000 branches per execution.
Branch exhaustion fails the test.
These models set no preemption, permutation, duration, or checkpoint limit.
Their finite input domains do not establish arbitrary production schedules or storage durability.

### Compactor observer reduction

One recovery Loom scenario combines two compactors with a completion-query observer.
The scenario starts with effects cursor one and queries revision one.
Neither compactor changes the effects cursor.
The production query therefore returns true after its first cursor read and never reads a receipt.

The original three-worker exploration was stopped with user approval after its inputs were checked.
Rocq proofs and a separate experiment preceded the permanent test change.
The permanent scenario retains both compactors, every receipt kind, and both complete page traversals.
It moves the redundant query after both workers join.
The query uses a receipt callback that panics if the short-circuit contract fails.
All six permanent recovery-page tests then passed without a schedule cutoff.
Strict Clippy also passed, and the finalization gate now explicitly includes this target.
These results do not count the stopped original run as completed.

The Rocq trace proofs establish read erasure, mutation-trace embedding, observer reinsertion, invariant preservation, and constant observer results.
Their application requires each of these concrete premises:

- The effects cursor remains one throughout the scenario.
- The observer changes no durable data or compactor-local state.
- The observer acquires no page gate and releases its data guard after one cursor read.
- Mutators do not inspect observer presence, lock contention, reference counts, timing, or scheduling counters.
- Observer execution neither panics nor poisons its guard.
- The safety predicate excludes observer-local state and lock bookkeeping.

Projection removes observer steps and hides its temporary mutex ownership.
Each remaining mutation step remains permitted because the observer changes no state or condition that a mutator uses.
Reinsertion places the observer after both compactors finish, before its join.
The observer returns true and preserves the same complete mutation trace.

The proof does not establish fairness, starvation freedom, or completion-query behavior below the requested revision.
The separate production query tests retain cursor advancement and receipt-creation or deletion races.
No page transition, compactor, effect kind, or page budget was removed from the reduced mutation scenario.
Replacing full traversals with isolated page samples would require additional induction over durable cursors and complete iterator state.
The current reduction does not make that replacement.

### Process-crash recovery evidence

The Unix process-crash test opens a real LMDB environment in a child process.
It uses production ledger operations to reach each boundary.
The child confirms the boundary before the parent sends `SIGKILL` to that child.
The parent waits for process termination before it reopens the database.
Each case uses a separate directory under `target/verification/pr216/ledger-startup/` and removes that directory after all handles close.

| Crash boundary | Required state after reopening |
| --- | --- |
| Two audit records validated | A fresh audit rejects corruption in the previously validated interior record. No saved progress bypasses that record. |
| Round commit before projection | The committed head remains at revision three. Projection and effect cursors remain zero. Recovery can finish all rounds. |
| One effect receipt before the round marker | The completed effect remains complete. The other effects and round remain incomplete. |
| Round marker before effects advancement | The marker remains present. The effects cursor remains zero until recovery advances it. |
| One effects-cursor page | The cursor remains at revision one. Recovery continues through the remaining complete prefix. |
| Receipt deletion before compaction-cursor publication | Deleted receipts remain absent. Logical completion remains true, and the compaction cursor remains zero. |
| One completed compaction page | The compaction cursor remains at revision one. Recovery removes subsequent receipts without regressing progress. |
| Before migration invocation | No migration charge or usage increment exists. A fresh migration can commit all four charges. |
| After migration commit | All four charges and their usage total exist. An identical retry changes no raw row. |

The receipt-deletion cut uses a test adapter immediately before the production cursor write.
The deletion transaction has already committed at this boundary.
The migration cuts surround the production migration call, not individual writes inside its LMDB transaction.

A separate [LMDB transaction test](../../../../shared/src/rust/store/lmdb_transaction_crash_tests.rs) interrupts the actual strict transaction implementation.
The implementation exposes checkpoints only when both `test` and `unix` are configured.
Production builds contain no checkpoint calls or environment-variable checks.
The test covers two stores, repeated keys, replacement, conditional insertion, deletion, and compare-and-swap.
Its child confirms each checkpoint before the parent terminates that child and reopens the environment.

| Transaction boundary | Required reopened state |
| --- | --- |
| After each of seven operations, before commit | Both stores equal their complete pre-transaction state. No partial operation becomes visible. |
| Immediately before commit | Both stores retain the old state, and a new writer can commit the complete transaction. |
| Immediately after commit | Both stores contain the complete new state. A stale compare-and-swap retry changes neither store. |

All nine backend boundaries passed against real LMDB.
They supplement the nine ledger-level process boundaries instead of replacing them.
The tests remove their temporary database directories after all handles close.
They do not simulate power loss, disk-controller failure, or interruption inside every backend instruction.
The tests also do not supply the separate phase-specific resource measurements.

## Formal models and their limits

The verification gate includes audit, recovery, effect-selection, and completion-observation families.
The default invocation runs all four families.
A second argument selects one family for focused repair validation.
For example, `bash scripts/check-finalization-ledger-audit.sh all recovery` checks only the recovery TLA+ cases and the shared Rocq proofs.
Family selection does not establish that omitted checks passed.
Use evidence from each required family before reporting a complete gate result.

`FinalizationLedgerAudit.v` extends the existing finalization proof family.
It proves page-partition equivalence for arbitrary finite record lists and deterministic state-dependent validators.
It also proves checked-prefix completion, suffix independence, bounded cursor steps, restart reset, and receipt-deletion properties.
The effects-cursor proofs establish step bounds, completed-prefix preservation, stopping at a gap, page-partition equivalence, and restart from durable progress.
These proofs quantify over arbitrary finite budgets and completion predicates.
The production page implements a finite composition of the recovery model's single-round cursor action.

The generic validator is an explicit parameter.
The proof does not extract or verify the Rust record validator.
Production conformance tests must connect every concrete validation condition to this parameterized contract.
Digest collision resistance and backend durability remain separate premises.

`FinalizationLedgerAudit.tla` models two independent audit readers and a concurrent append action.
Each reader captures its own target and can fail, complete, crash, or restart.
The initial state enumerates every corrupt-record subset through the initial head.
Later supported appends preserve the captured prefix.

The checked configuration uses three rounds, two readers, one crash per reader, and a two-record page limit.
These values bound model exploration, not production capacity.
The Rocq page-partition theorem is not limited to three rounds or two readers.

The audit termination property assumes weak fairness of enabled startup, audit, and completion actions.
Crashes are finite, and storage actions eventually return.
The property supplies no wall-clock deadline or production uptime estimate.

`FinalizationRecovery.tla` now separates physically stored receipts from historical completion evidence.
Deletion pages can occur before a crash and before compaction-cursor advancement.
The model permits interleaved projection, out-of-order effect completion, append, deletion, and restart.

The recovery configuration uses three rounds and two representative receipt slots per round.
Production has four effect kinds for each finalized block.
The model does not claim that two slots exhaust all manifests.
Rocq deletion theorems quantify over arbitrary receipt-key lists, and production properties must vary complete manifests and all effect kinds.

Neither model verifies Casper certificate arithmetic, arbitrary storage corruption, or LMDB power-loss behavior.
The models refine local persistence and scheduling after the existing certificate and storage contracts hold.

The effect-selection induction check initially admitted an unreachable state with a captured projection target above the committed head.
From that state, projection could exceed the head in one step.
`Inv_CapturedProjectionBound` now records the missing relationship for every worker.
Capture takes the current head, append increases the head, and restart clears the targets.
These transitions preserve the relationship without a new production restriction.

Apalache checks the strengthened safety predicate as an initial predicate and its preservation through one transition.
The check uses three rounds, two workers, and one crash.
The normal initial-state checks establish the base case separately.
This proves induction within that finite configuration, not arbitrary validator counts or the complete Casper protocol.
The original unsafe controls still violate their named invariants.

## Invariant-to-test obligations

### Recovery snapshot contract

The reviewed repair replaces recovery enumeration through `to_map` with one synchronous borrowed snapshot scan.
The backend must visit each physical row once without copying its key or value payload.
The callback must not reenter storage.
Recovery episode validation occurs after snapshot release because that validation reads the ledger again.

LMDB uses one read transaction.
The in-memory backend holds its existing coordinator read guard throughout the scan.
This contract preserves snapshot consistency without ordered pagination or repeated DashMap traversal.
It does not bound snapshot duration or startup elapsed time.

The scan validates every key layout and every value layout before selection.
Both recovery-key mismatch directions remain errors.
Unrelated key/value combinations retain their existing classification behavior.
Malformed unrelated values cannot disappear through prefix filtering.
Only recovery charges and usage rows become retained typed values.

`streaming_selection_matches_full_validation` compares the scan with full validation followed by selection over the same finite snapshot.
`successful_selection_checks_unselected_rows` requires validation of discarded rows too.
The partition and unselected-row theorems describe composition and preservation of selected output.
These proofs parameterize row validation and selection.
They do not verify the Rust codecs or establish a machine-memory bound.

The caller must validate each complete target and citer before retaining a compact admission summary.
That summary contains the target hash, shard identity, protocol version, citer validator, and bond generation.
The summary must use the validated target's actual identity.
All admission and usage checks must finish before the existing atomic migration write.

Total recovery memory still depends on charge count, episode count, admission summaries, migration size, and physically present recovery strings.
Legacy migration can exceed the normal 512-charge episode capacity.
The in-memory transaction backend now stages changed keys without copying unrelated rows.
Its separate transaction-order and rollback proofs specify that change.
Neither repair establishes a constant bound on total migration memory.

### Sparse transaction staging

The transaction repair replaces complete in-memory store snapshots with entries for changed keys only.
An entry contains either a replacement value or an explicit deletion.
Absence from the staging map means that the transaction must read the original store.
This distinction prevents a deletion from revealing the original value to a later operation.

The existing coordinator write guard covers staging and publication.
Operations inspect earlier staged results in their original order.
The backend publishes no staged entry until every operation succeeds.
Unchanged keys retain their original allocations.
The repair does not add a lock or restrict existing transaction concurrency.

`sparse_staging_preserves_sequential_transaction_results` compares sparse staging with complete-store mutation for arbitrary operation lists.
The model parameterizes operation evaluation and key equality.
A key can identify both a store and a row.
The touched-key and staging-size theorems exclude dependence on unrelated store entries.
`failed_sparse_transaction_publishes_no_changes` establishes rollback at the modeled publication boundary.

The proof models an overlay as a newest-first list.
Production uses one final staging entry per changed key.
Both representations resolve the latest value, including deletion, before evaluating the next operation.
Generated tests must compare all operation kinds, repeated keys, store aliases, successful commits, and failures.
Backend exclusion and publication remain explicit premises, not consequences of the pure Rocq model.

The native implementation now calls the shared [sparse transaction kernel](../../../../rspace++/src/rspace/shared/sparse_transaction.rs).
The kernel retains manager validation, alias grouping, operation order, explicit deletion, rollback, and publication under the existing write guard.
The native adapter retains backend checks, pointer-based store identities, DashMap operations, and the original error messages.
Its borrowed operation views do not copy value payloads.
Borrowed point reads and scans call the shared snapshot helper, which holds the existing read guard through callback return.

The production-linked sparse Loom tests import that kernel directly.
Two writers compete for one compare-and-swap and publish payloads in another store.
Exactly one writer can succeed, and the losing transaction cannot publish its staged writes.
A separate reader observes either the complete old snapshot or the complete new snapshot during multi-key publication.
Callback errors must release the read guard.
Further cases cover aliased handles, repeated operations, explicit deletion, late failure, and rejection of different managers before locking.

The missing-write-gate control must expose two successful claims on the same absent key.
The missing-read-gate control must expose partial publication.
The tests retain independent point-operation guards, so publication is not one indivisible model assignment.
They use three threads and a 5,000-branch limit that fails on exhaustion, without schedule cutoffs or checkpoint resume.
The tests substitute Loom locks and small maps for native locks and DashMap.
They do not prove DashMap internals or create an atomic snapshot across separate public read calls.

The generated transaction property varies one through eight stores and up to 64 operations.
The key domain contains six keys per store, and separate handles can share the same backing store.
These values bound test generation, not production capacity or the Rocq theorem domains.
The concurrent transaction regression requires exactly one winning compare-and-swap and no writes from the losing transaction.

The admission regression uses four admissions at body payload sizes of zero, 1,024, and 65,536 bytes.
It corrupts each admitted body separately and requires an unchanged raw ledger after rejection.
Successful reconciliation and its retry must preserve the exact four-charge usage total.
This checks functional behavior at different body sizes, not peak-memory scaling.

### Production evidence

The production tests in this table remain required before task completion.

| Formal obligation | Required production evidence |
| --- | --- |
| `Inv_ValidatedPrefix` and `audit_success_checks_every_record` | Corrupt or remove first, middle, last, and boundary-adjacent records and witnesses. Require the same failure as the complete audit. |
| `arbitrary_page_partitions_preserve_verdict` | Generate histories and page partitions. Compare incremental and complete validation results, including empty and partial pages. |
| `Inv_ExactTarget` and `captured_prefix_ignores_later_appends` | Append between pages. Require a fixed target and identical captured-prefix verdict. Test conflicting endpoint replacement separately. |
| `Inv_CapturedProjectionBound` | Capture the committed projection target. Append afterward and require the original target to remain fixed. |
| `Inv_Readiness` | Fail or cancel startup between pages. Confirm that no usable storage object becomes available. |
| `Inv_RestartClearsProgress` | Restart an incomplete audit. Reject forged progress and recheck corruption before the former cursor. |
| `Inv_BoundedBatch` and `integrity_page_is_bounded` | Count storage operations and retained records. Cover zero, one, exact, partial, and oversized-item page boundaries. |
| `Inv_EffectsCursorPrefix` | Generate completion permutations and failures. Check every prefix after each operation and restart. |
| `completion_page_partition_equivalence` and `completion_page_is_bounded` | Vary completion gaps, budgets, and restart boundaries. Require the exact prefix and bounded advancement. |
| `completion_page_restarts_at_durable_progress` | Interleave cursor workers and cancel an active asynchronous page. Resume from durable progress without regression. |
| `Inv_RequiredReceiptsPresent` | Interleave partial completion and compaction. Preserve every receipt above the durable effects cursor. |
| `Inv_CompactionCursorClean` | Inject failure or terminate the process around deletion and cursor writes. Require deletion before cursor advancement. |
| `Inv_EffectsAfterProjection` | Reproduce append between projection and selection. Require typed rejection before early effects and unchanged storage after rejected receipts. |
| `completed_receipt_deletion_preserves_effect_status` | Compare effect queries before deletion, after partial deletion, and after reopening the database. |
| `completion_query_is_linearizable` and `Inv_TrueWitness` | Generate full-width revision and monotonic cursor values. Compare the production kernel result with the observed logical states. |
| `completion_query_cannot_lose_a_completed_receipt` and `Inv_FalseWitness` | Pause an actual query before receipt access. Advance the cursor and delete the receipt in both backends. Require true. |
| Completion observation read errors and pending-state rules | Enumerate failures at each executed read. Require unchanged errors, exact read order, and short-circuit behavior. |
| `receipt_deletion_pages_commute` | Permute and repeat deletion pages. Preserve the same final receipt state and unrelated keys. |
| `streaming_selection_matches_full_validation` | Compare generated recovery histories with materialized selection across insertion orders. |
| `successful_selection_checks_unselected_rows` | Cover all key/value variant combinations and every truncation. Reject malformed discarded rows. |
| `validated_unselected_row_does_not_change_output` | Add unrelated valid ledger rows. Require unchanged recovery output and one visit per physical row. |
| `sparse_staging_preserves_sequential_transaction_results` | Compare generated multi-store transactions with a complete-store reference. Include repeated keys and shared store handles. |
| `sparse_staging_retains_only_touched_keys` | Check unchanged payload addresses after commit and rollback. Retain no unrelated payload copies during staging. |
| `failed_sparse_transaction_publishes_no_changes` | Inject late compare-and-swap failures and invalid admission bodies. Require unchanged durable contents. |

A counting store must detect whole-store scans and excessive mutation batches.
LMDB history tests must distinguish total startup work from peak ledger working memory.
Production-linked Loom tests must cover captured cursors, concurrent append, and completion or deletion interleavings.
Loom scheduling evidence does not replace persistence fault injection.

### Evidence checkpoint

The [work log](../../../work-logs/task-pr216-ledger-startup-2026-09-06.md) records completed commands, intermediate failures, and source hashes.
The matrix below connects those results to the production boundaries.
The preceding invariant table identifies the individual required tests within each group.
`resource-checkpoint.SgZL22` subsequently passed all 77 ledger tests and eight projection tests against the final formatted source snapshot in release mode.
Its source hashes remained unchanged during the run.

| Obligation group | Production symbols | Evidence and result | Bounds and remaining limits |
| --- | --- | --- | --- |
| Complete audit, fixed target, rollback, and page limits | `begin_integrity_scan`, `validate_next_page`, `integrity_pages::capture`, `integrity_pages::validate_page` | `sparse-adapters.RY45XK`: all 72 ledger tests passed. `integrity-page-loom.t51uPL`: all five shared-kernel tests passed. | Native properties vary histories and partitions. Loom permits four threads and 5,000 branches per execution. It does not verify the native cryptographic validator. |
| Constructor readiness and restart | `BlockDagKeyValueStorage::new`, `validate_integrity_async` | `startup-readiness.smGkIM`: two constructor tests and six projection tests passed. | A 65-round history covers six error positions and two active-page cancellation points. Successful projection has separate assertions. |
| Projection before effects and monotonic recovery | `begin_effects_cursor_advance`, `advance_next_page`, `apply_finalization_effects` | `rust-caller.9RdrJv`: seven Casper tests passed. The native ledger runs cover gaps, parallel workers, and cancellation. `recovery-page-loom.oiv60G`: all six shared-kernel tests passed. | Generated projected prefixes include older eligible rounds. Loom's finite configuration does not establish every production schedule. |
| Receipt deletion and completion observation | `delete_next_page`, `effect_completed`, `effect_observation::effect_is_complete` | Ledger races passed on both backends. `observation-production.PFwViJ` passed the shared query tests. | Cursor monotonicity and fresh snapshots remain premises. The query does not make external effects exactly once. |
| Layout checks and selective enumeration | `bounded_decode`, `settled_recovery_state`, backend `with_value` and `visit_entries` | `borrowed-scan.2oYeJd` and the later 72-test ledger run passed. Maximum-witness allocation passed in `resource-phases.n4LGSr`. | Tests cover row variants, truncations, forged lengths, and physically present long recovery strings. The byte contract remains row-specific. |
| Sparse transaction equivalence and atomic publication | `sparse_transaction::apply`, `strict_atomic_mutate` | `sparse-adapters.RY45XK`: nine native in-memory tests, six shared-kernel Loom tests, and ten Casper admission tests passed. | Generated cases use one through eight stores and up to 64 operations. Loom includes independently guarded point operations and both missing-gate controls. |
| Durable restart boundaries | Ledger recovery and LMDB `strict_atomic_mutate` | `durability.XzOgx2`: nine ledger cuts passed. `transaction-crash.mMkfRP`: nine backend cuts and all 29 LMDB tests passed. | Controlled child-process termination does not model power loss or every machine instruction. |
| Phase-specific allocation | Audit, recovery enumeration, admission visitor, migration, and metadata projection | `resource-phases.dA4aTE`: all five probes, both meter tests, maximum-witness check, and strict Clippy passed in release mode. | The [resource report](bounded-finalization-ledger-resources.md) states every input range and measurement exclusion. Earlier debug-profile results remain separately identified. |
| Compactor observer reduction | Shared compaction kernel and covering-cursor query | `verification.bfq4ac`: Rocq and independent kernel checks passed. `recovery-page-loom.oiv60G`: all six permanent tests and strict Clippy passed after the reviewed reduction. | The proof preserves mutation traces under the listed premises. It does not establish fairness or starvation freedom. The stopped original run is not counted as passed. |

The current shared-kernel hashes are recorded below.
The work log records the corresponding sparse kernel, Loom sources, and resource-test hashes.

```text
460f1ce719ca57c78418b81a61266758f8b3ec0a98208d06c297c4142dfafb34  recovery_pages.rs
c590e749c1c46be88e6977b3e2b0e5c57002f29f09b4e3b825ded2baf799a111  integrity_pages.rs
4b7c0a77e2f542a6d2c3f3f36a0ea5e6d66a1dda4f9626d3b16d962e30fdea90  effect_observation.rs
168402b42c7b3c66839d81946287ba65cef614e0146cceb08103398d8af6294e  FinalizationLedgerAudit.v
9c2df36308df6936eb25048506d01a00e5b557f387ff6c4a929ed588c4ab2c2a  FinalizationEffectSelection.v
b93367b61177bf47eac9002168e1e1c4b33781a09e3f0a695f2298e1a4a8b5fe  FinalizationAtomicity.v
d5af5e51a719ba76ec904e0697da39dfe9bece8ba0feb38940f4b36d867a333b  FinalizationLedgerAudit.tla
25059866d0e0caf8e79239f487fc75b46d37d91171c558417a5633a4d0c9b2de  FinalizationRecovery.tla
e9ef63abc5b3882c4d34c850c4cccf49a0109aeb673080441535d9ddc65e10d2  FinalizationEffectSelection.tla
ece92abc2c45d0369fa6da7db7dff39e28bcba6bc8a7490db01daf146655920e  FinalizationEffectObservation.tla
```

The latest Rocq run checks the complete shared theories, including earlier decoder, selection, transaction, and query theorems.
Separate TLC and Apalache records retain their normal initial-state and negative-control results.
The work log distinguishes normal reachability, finite induction checks, and earlier unsuccessful attempts.
The recovery target also passed all six tests with the exact finalization-gate flags in `recovery-gate.EM9XrK`.
That run preserved its input hashes and passed shell and documentation checks.
Neither this matrix nor a parameterized theorem establishes complete Casper verification or readiness for the campaign's soak test.

## Verification commands

Run the targeted gate under a memory-limited scope on this workstation.

```bash
systemd-run --user --scope -p MemoryMax=8G -p MemorySwapMax=0 \
  env JAVA_TOOL_OPTIONS=-Xmx2g \
  bash scripts/check-finalization-ledger-audit.sh all
```

The script accepts `rocq`, `tlc`, and `apalache` for targeted checks.
The Apalache gate checks each selected family's induction step after its normal initial-state check.
It records input hashes and checks that those inputs did not change during the run.
Evidence remains under `target/verification/pr216/ledger-startup/`.
The finalization atomicity gate invokes this targeted gate.

The thirteen negative controls cover five audit defects, five recovery defects, two effect-selection defects, and the completion-query race.
Each control must violate its named invariant.
A parse error, timeout, or tool failure is not a successful negative control.

The formal-first milestone does not complete this repair.
Completion also requires the production implementation, invariant-derived properties, Loom coverage, persistence regressions, and measured resource bounds.
