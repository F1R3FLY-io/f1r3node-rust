# Generation checkpoint publication invariant catalog

Date: 2026-08-28

Status: the checkpoint phase model and its original executable bindings have prior verification evidence. The current-source focused Rocq, TLC, and checkpoint-property gates passed on 2026-10-08. The physical write/sync fault seam and exhaustive crash matrix are separately tracked as claimed done; independent acceptance review remains.

Scope: fresh PathMap-DAG generation checkpoints for durable RSpace

## Purpose

This report is the traceability authority between the verified publication-phase models and their executable Rust refinement. It prevents an implementation or test suite from silently covering only the successful publication path. Every obligation at this abstraction level has a stable identifier, a property-based test target, and a negative control where one is meaningful. The phase model treats each pre-catalog file write and synchronization operation atomically; it does not claim the separately tracked byte-progress refinement for short writes, zero progress, interruptions, synchronization uncertainty, or loss of unsynchronized names after a power failure.

The required refinement order is:

1. verify the formal model and its controls;
2. extract every invariant into this catalog and executable property tests;
3. implement the Rust refinement;
4. require all proofs, properties, differentials, and focused integration tests to pass.

No Rust implementation claim is justified by the abstract model alone. Filesystem durability, checksums, bounded parsing, partial writes, and semantic admission are executable proof obligations.

The task 7920 caller/limit ruling is recorded in
[engineering journal J-067](pathmap-engineering-journal-2026-08-23.md#j-067--2026-09-06-separate-trusted-state-authority-from-cold-execution-evidence).
It separates the planned trusted-successor validity/inventory capability from
exact cold-execution evidence. Existing fresh-generation preflight, cold admission
and publication-owner obligations below remain binding. J-067's finite future
refinement list is not a new verified theorem/property manifest: its corresponding
proofs and executable witnesses must be added through the refinement order above
before runtime implementation. No verified binding is removed or relabeled.

## Hot-store journal refinement (2026-09-06)

The `HotStoreJournalAcknowledgement` section of `GenerationCheckpointPublication.v` adds
ten closed lemmas to the seventeen publication capstones. Its journals are abstract finite
encodings of five domains: data deltas, continuation deltas, continuation content transitions,
continuation source bytes, and join deltas. Equality in this model must be refined by exact
PathMap equality in Rust. It establishes the local capture/acknowledgement obligations below;
it does not establish the unfinished owner/publication/recovery composition.

| ID | Formal lemma | Executable obligation in `tests/hot_store/journal_commit.rs` |
|---|---|---|
| HJ-01 | `acknowledgement_preserves_live_state` | `acknowledgement_clears_five_populated_journals_and_preserves_live_records` compares resident and installed values. |
| HJ-02 | `exact_acknowledgement_clears_all_five_journals` | The same test populates every domain before clearing; run in debug and release. |
| HJ-03 | `stale_acknowledgement_changes_nothing` | `mismatch_or_missing_capability_in_any_journal_rejects_without_partial_clear` compares all journals after each refusal. |
| HJ-04 | `successful_acknowledgement_requires_exact_journal` | `stale_journal_cannot_clear_a_newer_revision` and ten missing/changed-domain cases. |
| HJ-05 | `post_state_addition_recovers_base` | `journal_count_inversion_matches_live_multiplicities` varies the base and successor counts. |
| HJ-06 | `post_state_removal_recovers_base` | The same property covers removal to a positive count and to zero. |
| HJ-07 | `one_occurrence_removal_requires_unit_delta` | Persisted counterexample `before=3, after=0` detects journaling the prior multiplicity at every removal. |
| HJ-08 | `missing_count_is_distinct_from_captured_zero` | `absent_count_evidence_is_not_a_captured_zero` deletes captured evidence and requires refusal. |
| HJ-09 | `journal_lookup_uses_exact_native_record_key` | `join_journal_uses_native_key_and_repeated_channel_positions` checks native bytes, duplicates, additions and removals. |
| HJ-10 | `reframed_join_key_misses_native_record` | The same test rejected the previous extra-framed producer before its correction. |

The model was checked before producer changes under `MemoryMax=2G`, `MemorySwapMax=0`.
The focused baseline passed six tests and failed three; the corrected debug and release gates
pass nine tests each under `MemoryMax=8G`, `MemorySwapMax=0`, with four build jobs.
These are configured resource ceilings, not measurements of peak resident memory.
The arithmetic lemmas use natural numbers; checked `u64` overflow/underflow remains an
executable refinement requirement. The finite randomized multiplicity property is not an
exhaustive proof over the Rust integer domain.

`tests/persistence/transaction_sealing.rs::live_cold_journals_seal_exact_three_namespace_successor`
adds the composition check from actual hot-store mutations through source staging and semantic
sealing, including first-representative source retention and repeated receive-channel positions.
Sealing alone must leave the durable base and hot journals unchanged. Durable publication,
unknown-outcome reconciliation, receipt identity and acknowledgement under the operation gate
remain separate required obligations.

## Publication owner composition (2026-09-06)

`RSpaceJournalPublicationOwner.tla` models one nonempty checkpoint and its retries. A receipt
binds store identity, semantic profile, hot-store incarnation, journal epoch, durable base, target,
transaction identity and all five journals. Two incarnations and two epochs permit a stale
receipt to carry byte-identical journal contents. The model also admits foreign stores/profiles,
the same target with a different transaction, and a third recovery state as candidate inputs.

The required sequence is `Ready → Captured → Publishing → AwaitingAck → Done`. Unknown
publication outcomes enter `RecoveryRequired`. An authenticated exact predecessor returns to
`Ready` with the journal retained; an authenticated exact successor enters `AwaitingAck` without
publishing the mutation again. A successor generation lease is acquired while predecessor
protection remains held. Only exact acknowledgement clears the journals and rebases the live
store. Reopening after process termination reconstructs fresh volatile journals from the selected
durable state; it does not resume arbitrary unfinished interpreter execution.

| ID | Model invariant | Required Rust property before owner activation |
|---|---|---|
| JO-01 | `TypeInvariant` | Every success/error/recovery result contains the appropriate typed authority; no path-only recovery state. |
| JO-02 | `GateHeldUntilResolution` | An exclusive operation permit remains held through capture, publication, uncertain recovery and acknowledgement; cancellation cannot release it while a worker still commits. |
| JO-03 | `ContinuousAuthority` | The same writer lease and generation registry survive every error; predecessor protection overlaps successor acquisition. |
| JO-04 | `ExactPendingReceipt` | Foreign store/profile/incarnation/epoch/base and byte-identical stale receipts are rejected without journal mutation. |
| JO-05 | `WholeJournalAcknowledgement` | A mismatch in any domain leaves all five journals unchanged. |
| JO-06 | `AcknowledgementRequiresExactSuccessor` | No clear before authenticated publication and successor lease acquisition; failed acquisition retains the admitted successor. |
| JO-07 | `AcknowledgementRequiresExactReceipt` | The consumed receipt is the exact captured intent, including all journals. |
| JO-08 | `ReadyHasCoherentBase` | Published-but-unacknowledged state cannot become ready with a journal still relative to its predecessor. |
| JO-09 | `RecoveryAcceptsOnlyExactTerminal` | Compare authenticated store/profile/base/target/transaction bindings; equal target identity alone is insufficient. |
| JO-10 | `AtMostOnePublication` | Retry after an ambiguous successful commit acknowledges the original intent and never applies it twice. |
| JO-11 | `ReopenDiscardsVolatileJournal` | Restart reconstructs a fresh hot-store incarnation with empty pending journals relative to the recovered checkpoint. |
| JO-12 | `PreparedWitnessBound` | Derive the expected complete terminal from the checked publication layout; retain its exact sequence, transaction, pack and index frontiers through reconciliation. |
| JO-13 | `AtMostOneAcknowledgement` | Retrying an acknowledgement after unwind does not clear journals or rotate the epoch twice. |
| JO-14 | `ClearedPendingJournalHasExactMarker` | An interrupted owner recognizes a cleared journal only through its exact committed receipt and successor marker, never emptiness alone. |
| JO-15 | `AcknowledgementAcceptsOnlyExactInput` | Reject incoming acknowledgement requests with a different receipt or terminal, including retries after the original journals were cleared. |

The original control explored 544 distinct states with all eleven original invariants satisfied. Eight
single-defect configurations fail their designated invariants: foreign receipt, early clear,
partial clear, generation-lease gap, premature ready/rebase, duplicate publication, gate release,
and wrong recovery terminal. Four reachability witnesses establish successful acknowledgement,
recovery of the predecessor, recovery of the successor, and refused receipt binding.
`bash scripts/check-rspace-journal-owner-model.sh` verifies the control and checks each
counterexample's named invariant, or TLC's "Deadlock reached" for the lock-order deadlock
control, and requires the view control's temporal-check line ("Checking temporal properties for
the complete state space"), so a parse error or resource failure cannot count as a pass.
The runner is included in the existing stack-safe proof gate and uses its shared bounded TLC
helper: 512 MiB JVM heap, 2 GiB memory cap, two workers, no swap, and on-disk state storage.

This is bounded protocol evidence, not a proof of the Rust owner implementation. Journals are
abstract equality values; object validation and filesystem write/sync behavior are supplied by
the existing publication models and executable failure-injection suites. The model does not
establish fairness, eventual repair of permanent I/O faults, or unbounded multi-transaction
progress. Empty mutations use the separate no-op obligations. The JO properties above are
required implementation tests, not yet executed owner tests.

### Capture abort, cancellation and interrupted acknowledgement refinement

The revised control explores 13,784 distinct states (334,852 generated) with all fifteen
invariants satisfied. Fifteen causal mutants and eight reachability witnesses pass their
designated checks. The additional mutants separately exercise acknowledgement-marker loss,
stored-marker corruption, wrong incoming acknowledgement identity, duplicate acknowledgement,
prepared-witness corruption, gate release on unwind and gate release on cancellation. Additional
witnesses reach capture abort, cancellation, unwind and exact acknowledgement retry after
unwind. Invalid acknowledgement inputs vary base, target, transaction and journal as well as
store, profile, incarnation and epoch; these inputs do not broaden legitimate preparation.

Acknowledgement and owner completion are separate actions. The first clears all journals,
rebases and records an exact marker under one hot-state boundary. If completion is interrupted,
recovery of the same successor can recognize that marker and finish without a second clear or
publication. Capture abort has no disk effect and retains the journal. Cancellation preserves
the owned permit and authority.

This refinement still models one successful publication, not repeated checkpoint/reset/restore
cycles. Repeated-epoch ABA protection therefore requires separate generated implementation
histories. The model's expected sequence and frontiers are synthetic functions of its receipt:
JO-12 checks preservation of that witness, not correctness of Rust layout construction. Rust
must derive the witness from the same checked PREPARE/COMMIT/index plan as publication and test
same-root but wrong sequence, transaction digest and frontiers independently. Cancellation assumes
synchronous publication/recovery with no detached worker still writing after return; adding such
a worker would require extending this model before implementation. No unbounded liveness or
complete Rust refinement claim follows from this finite exploration.

The completed command was `bash scripts/check-rspace-journal-owner-model.sh`; raw evidence is
`/tmp/rspace-journal-owner-unwind-ack-matrix-r2-20260906.log`, SHA-256
`b78b4d623a01642671ad90140f1d0a1632f90d231a23c2cef619efccc64705fe`.
The existing runner enforces the resource bounds stated above and validates each named
counterexample rather than treating an arbitrary failed invocation as evidence.

The 2026-10-27 [trusted successor](#trusted-successor-capability-2026-10-27) extension adds
tightened cold limits, the empty checkpoint and the absence of cold evidence on trusted adoption
to this model: two invariants (`ColdRecoveryHonorsIncomingLimits` and
`TrustedSuccessorHasNoColdEvidence`), two causal mutants and two reachability witnesses. The
counts above are those of the 2026-09-06 revision; the verification record gives the extended
model's counts.

The concrete implementation must preserve `DurableWriterLeaseV1` through physical recovery and
semantic-admission errors. `DurableDirectoryV1::try_clone_reader` shares the opened directory
capability and generation registry; reopening `PathBuf` does not. Factor borrowed recovery and
source admission, or return failures carrying the original lease. Retain the admitted successor
when its new generation lease is unavailable. The owner must construct mutations from its
private observation and retain the sealed intent through reconciliation and acknowledgement.

### Borrowed recovery and admission refinement (2026-09-06)

The `JournalOwnerReadiness` module in `GenerationCheckpointPublication.v` refines
JO-03, JO-04 and JO-08 at the construction and abort boundaries. Its binding is an
opaque registry identity paired with the complete catalog identity; `None` models
a hot-store base invalidated by clear or snapshot replacement. It does not model
those operations as legitimate mutations through an already-held operation gate.

| Formal lemma | Extracted executable obligation |
|---|---|
| `readiness_requires_exact_registry_and_catalog` | Refuse a foreign HotStore even with identical catalog bytes, preserve the rejected constructor's original view, and refuse a Ready observation after base invalidation. |
| `invalid_binding_retains_the_original_pending_receipt` | `invalidated_base_cannot_release_a_refused_capture_as_ready` and `invalidated_base_cannot_release_old_recovery_or_report_ready` retain the receipt, permit, writer and predecessor pin after clear or snapshot replacement. |
| `valid_base_allows_abort_independently_of_receipt_revision` | `a_stale_capture_is_rejected_before_publication_even_when_its_journal_bytes_match` permits a fresh capture after a same-base journal restore without accepting the stale receipt. |

This boundary theory distinguishes authority validity from receipt freshness; the
implementation must not enumerate journal endpoints merely to check a base.
The two invalidation regressions fail on the pre-guard owner, respectively at
capture refusal and exact-predecessor recovery. They are concrete refinement
tests, not a claim that the bounded publication model explores out-of-band resets.

The first executable JO-03 refinement factors the existing validation bodies into
`recover_physical_from_lease_v1(&mut DurableWriterLeaseV1, ...)` and
`admit_physical_source(&DurableWriterLeaseV1, ...)`. The caller retains the writer capability
on both success and failure; neither kernel reopens a pathname or constructs a generation
registry. Existing consuming startup wrappers delegate to these bodies and retain their old
startup API. The checkpoint owner must use the borrowed kernels, not those consuming wrappers.

The following tests in `tests/persistence/durable_startup.rs` were written before the refactor:

| Test | JO-03 refinement exercised |
|---|---|
| `borrowed_recovery_retains_writer_and_pin_across_every_rebuild_failure` | Six existing rebuild failpoints; lock contention and the original generation pin remain observable after each failure and retry. Explicit owner release is the observer's positive control. |
| `borrowed_recovery_unwind_keeps_the_owners_writer_and_generation_pin` | An injected recovery panic unwinds only the borrowed operation; the test retains the owning scope, verifies exclusion, and retries successfully. |
| `borrowed_admission_does_not_release_authority_on_a_semantic_error` | A physically valid but semantically invalid fixture is repeatedly rejected without dropping the owner. |
| `borrowed_recovery_and_admission_retry_histories_keep_exact_authority` | Generated physical-limit/admission-limit failure sequences followed by successful admission, preserving the exact catalog and pack bytes. |

The focused debug suite passes 17/17. The broader release durable-persistence suite passes
70/70 with 256 property cases configured. Its first run passed 68/70: two existing high-volume
publication tests exhausted the inherited 1,024-descriptor limit because every trial used a
process-lifetime scratch lock. Only those test fixtures and the new generated retry fixture now
use scoped child directories under one protected process root. The shared helper, descriptor
limit and test coverage are unchanged. A separate 1,100-case lifetime regression passes.
All heavy checks used an 8 GiB systemd memory cap with swap disabled; the corrected release
run used on-disk scratch. No new dependency was introduced.

These tests do not yet establish complete JO-03 owner integration, successor-pin handoff,
asynchronous cancellation or all JO-01..JO-11 properties. In particular, retaining a lease in
an outer test scope through panic is not proof that the unfinished owner remains recoverable
after every panic. Publication outcome classification and journal acknowledgement remain open.

### Borrowed preparation and streamed terminal witness refinement (2026-09-06)

The admitted source now supports transaction opening, sealing, exact successor inventory,
object classification and WAL preparation while its original writer lease is retained separately.
Existing writer methods delegate to these bodies. The source retains the original generation
registry identity and checks a borrowed lease before sealing, binding, preparation or publication;
equal catalog bytes from another opened store do not establish this authority.

The publisher exposes the exact successor slot through a synchronous `FnOnce` callback before
writing COMMIT. This slot uses the PREPARE identity computed during the actual streaming write,
the existing checked index append frontier and the same COMMIT/catalog constructors used for
publication. No second PREPARE serialization, payload hash pass or copied object projection is
required. The callback receives an expected terminal, not proof that publication has succeeded.

| JO obligations | Executable refinement |
|---|---|
| JO-03, JO-04 | `borrowed_source_preparation_keeps_exact_authority_through_refusal_and_publication` tests byte-identical foreign stores, crossed source/lease pairs, wrong sequence, preparation limit refusal, stale predecessor preparation, continuous writer exclusion, complete output bytes and cold reopen. |
| JO-09, JO-12 | The existing phase/torn-prefix/unwind matrix now checks that the retained terminal equals the uninterrupted control's complete catalog slot whenever capture has occurred. Failures during PREPARE/index emission leave capture absent and cannot have published a successor. |
| JO-03, JO-12 | `incremental_terminal_capture_precedes_commit_and_survives_callback_unwind` inspects the exact PREPARE/index bytes and unchanged catalog during capture, then unwinds, recovers the predecessor and retries to the exact final files. |

Physical PREPARE/index staging precedes terminal capture, but no COMMIT or catalog publication
can precede it. These unselected bytes have no authority under the catalog-appointed recovery
model and the checked suffix-retry laws. The abstract model's expected successor remains the
intended transaction throughout; the concrete owner may leave its physical terminal unset until
capture. In that case recovery must accept only the predecessor, never guess a successor from
its target root. Once captured, retain the complete slot through every later failure. Tests of
this callback do not establish owner permit ownership, atomic HotStore acknowledgement or
repeated-epoch ABA safety; those JO obligations still require owner implementation and tests.

## Identity-bound HotStore acknowledgement refinement — 2026-09-06

`CapturedHotJournalV1` is an immutable, non-cloneable receipt created under one HotStore
read boundary. It binds the exact catalog predecessor, the store's current journal version,
an independent capture identity, five copy-on-write journal roots and native data/join counts.
Two stores with equal catalogs and equal journal bytes cannot exchange receipts. Restoring
the same journal bytes advances the version; clearing or replacing the live state invalidates
its durable base. Read-side hydration, duplicate inserts and rejected mutations do not
advance the version. Installed-state mutation conservatively invalidates outstanding receipts.

Acknowledgement compares the expected and admitted full catalog slots, receipt/version/base,
successor sequence and all five journal domains under one HotStore write boundary. It prepares
the new marker before clearing, then clears and rebases together without changing live records
or installed state. A retry accepts only the same capture identity, terminal and current
post-acknowledgement version with all journals empty. Emptiness alone cannot authorize a retry
or fabricate a successor. The marker retains identity tokens and a catalog slot, not the
cleared PathMap journals. The owner must supply the genuinely admitted terminal: this component
does not independently read or authenticate disk state.

| Obligations | Executable component evidence |
|---|---|
| HJ-01..04, JO-05/07/13/14/15 | `durable_acknowledgement_is_exact_atomic_and_idempotent`, foreign/all-domain and wrong-terminal tests check atomic clearing, nonmutation, exact identity and replay. |
| JV-01..03, JO-04 | Retained-version and checkpoint/restore property histories cover stale identical journals and explicit `u64::MAX` rollover. |
| JO-04 | `every_durable_mutator_invalidates_retained_journal_versions` checks twelve production mutation routes, including accepted-match data, continuation and join removal. |
| JO-04/13 | `durable_reads_noops_and_refused_mutations_preserve_the_capture` and `an_empty_durable_journal_cannot_fabricate_a_successor` check the complementary boundaries. |

The three `JournalVersionFreshness` capstones were compiled before the Rust version
implementation. They bring the existing theory to 36 closed assumption reports. The model
uses an increasing abstract identity; the Rust refinement relies on fresh `Arc` allocations
while retained receipts keep their old tokens alive. This is an explicit identity-freshness
argument, not a machine-checked Rust allocator proof. Normal mutations increment a local
`u64` without allocating; rollover creates a new token rather than imposing a lifetime limit.
The central derivation matrix now has 94 IDs. Its single property-family predicate is shared
by catalog token recognition and derivation-row recognition, with required-family coverage
checked independently of test success.

These tests use synthetic, format-valid catalog slots for HotStore isolation. They do not
establish continuous writer ownership, authenticated recovery, successor-pin handoff or
asynchronous cancellation of the production owner. Those remain required task 6626 integration
tests; legacy-authority separation and production activation remain separately gated.

## Executable publication-owner composition — 2026-09-06

The component-only limitations above describe their historical verification stage.
The production owner now retains one private capture, exclusive operation permit,
original writer/registry and predecessor pin through publication and reconciliation.
Its sixteen tests execute real catalog/pack/index operations with the generic RSpace
semantic profile. The following table binds the separate JO model to those tests;
the central 97-ID matrix additionally checks every printed Rocq capstone and its
executable target. JO names remain a separate model boundary, not extra central IDs.

| JO obligations | Executable owner evidence in `tests/persistence/durable_rspace_authority.rs` |
|---|---|
| JO-01, JO-02, JO-03 | `owner_uncertain_old_and_new_publications_retain_the_gate_and_original_writer` and the capture/preparation/acknowledgement unwind matrix verify typed unresolved states, continuous writer exclusion and pin overlap. |
| JO-04, JO-05, JO-07 | Stale-capture and acknowledgement-conflict tests retain all five journals; the constructor regression rejects identical-catalog foreign registry provenance without losing the original view. All-domain and incoming-receipt variations also remain covered by the HotStore tests. |
| JO-06 | `successor_pin_failure_retains_the_admitted_successor_old_pin_and_permit_for_retry` forces the real lease-counter refusal and verifies retry without changed physical files. |
| JO-08 | Both invalidated-base regressions retain unresolved ownership after clear or replacement; non-pending invalidated owners refuse a usable Ready state. JR-01..03 supply the additional formal readiness boundary. |
| JO-09, JO-12 | `owner_recovery_rejects_foreign_third_and_same_state_different_terminals` rejects a different full terminal even with the same semantic target. The selected-file damage/repair test preserves the original expected terminal and authority. |
| JO-10 | Exact-successor retries and generated multiplicity/cold-reopen histories publish each changed step once; no-op steps do not advance generation. |
| JO-11 | `cold_restart_before_acknowledgement_selects_old_or_new_without_delta_replay` destroys the unresolved owner and old hot incarnation before recovery, checks all five fresh journals, and then publishes another mutation. |
| JO-13, JO-14, JO-15 | The unwind matrix and acknowledgement-conflict test distinguish exact retry from changed marker/version, including non-panicking and panicking post-acknowledgement restores. A shared read-only exact-marker predicate guards owner completion. |
| JO-02 cancellation | `cancelling_a_checkpoint_waiting_for_the_exact_gate_does_not_capture_or_publish` drops a pending caller before gate acquisition. There is no await or detached worker after capture. |
| JO-11 journal replay (2026-10-27) | The owner model's `journal-replay` fault keeps the volatile journal across `CrashAndReopen` and violates `ReopenDiscardsVolatileJournal`, the invariant's first negative control (the faulted trusted histories section). TH-08's owner mirror, in `tests/persistence/trusted_history_properties.rs`, reopens the owner after each restart with an empty journal, whose checkpoint publishes nothing and runs no cold check. Task 11968 restarts the real owner by path, in `tests/persistence/owner_history_production.rs`, from every phase that a history reaches. All five fresh journals are empty and the hot store equals the durable state. |
| JO-09 pathname recovery (2026-10-27) | The `pathname-recovery` fault accepts the old report as exact while a decoy is bound to the store's pathname and violates `RecoveryAcceptsOnlyExactTerminal`; the witness `NeverReconciledUnderDecoy` shows that exact recovery under a decoy is reachable. Task 11968 reconciles the real owner in `tests/persistence/owner_history_production.rs` under such a decoy. The owner recovers its held directory's selected slot, either the predecessor or the exact terminal, and the decoy's files stay byte-identical. |

Combined HotStore/persistence debug and release gates pass 403/403 with 233 other
library tests excluded. The owner property has an explicit 64-history configuration,
not 256 histories despite the command-level environment setting. The readiness
theory's three new capstones bring the existing theory to 39 closed reports and
pass independent kernel checking. Raw evidence and red controls are recorded in
[J-062](pathmap-engineering-journal-2026-08-23.md#j-062--2026-09-06-retain-the-journal-owner-through-exact-durable-resolution).

This does not activate the durable backend, prove power-loss behavior on every
filesystem, or prove full Rust refinement. Actual RSpace gate wiring and whole-owner
teardown containment remain task 7756's single-authority activation obligations.
The TLA+ model still assumes ordinary mutations honor that gate; out-of-band base
invalidation is covered by the separate readiness theory and concrete regressions.

## Exact incremental-successor inventory refinement — 2026-09-06

`GenerationCheckpointPublication.v::ExactSuccessorInventory` proves byte conservation,
the removed-byte subtraction bound, exact replacement accounting, and a concrete
counterexample to predecessor-plus-staged-minus-manifest arithmetic. Its four capstones
are closed under the global context; the theory now prints 31 closed capstones. These
are finite partition laws, not a proof that Rust constructs the correct partitions or
that the complete publication owner refines the model. Canonical closure membership is
distinct by digest and remains subject to the existing full-byte collision checks.

Extracted implementation obligations, before production correction:

| ID | Formal/accounting obligation | Executable correspondence |
|---|---|---|
| SI-01 | Exact replacement removes predecessor-only objects and counts shared objects once. | `incremental_inventory_tracks_replacement_removal_sharing_and_reuse` compares every sealed target and cold successor to full traversal, under both retention writers. |
| SI-02 | Repeated transitions preserve the exact distinct-object partition, including empty states and reused digests. | `incremental_inventory_matches_cold_admission_at_every_history_step` generates count updates and removals with a fresh union ledger and physical reopen at every step, under both retention writers. |
| SI-03 | Current semantic closure and retained semantic union are separate from retention metadata. | Both history checks compare manifest/current/retained fields independently; under the V2 writer the anchor and radix nodes stay out of the semantic oracle and the retained union, the sealed anchor equals `RetentionOracle`'s and cold admission appoints its metadata closure; the existing DATA-inventory laws continue to account for the separate retention object. |
| SI-04 | One predecessor/target-bound inventory supplies all publication fields. | A private durable-only sealed/prepared capability must reject foreign/stale catalog bindings before writes; generic staged diagnostics cannot serve as current-state inventory. |
| SI-05 | Resource refusal leaves durable predecessor bytes unchanged. | Structural/union/read/history limits must be enforced during inventory construction; policy mismatch and failed preparation must not publish or release the borrowed writer. |

The complete persistence gates now pass 312/312 in both debug and release. SI-01/SI-02
run 256 generated histories from both empty and populated three-namespace states. SI-04
is exercised by `incremental_inventory_is_bound_to_its_opened_authority_and_exact_sequence`;
SI-05 by `incremental_inventory_limits_refuse_before_any_durable_mutation` and
`incremental_inventory_policy_cannot_change_after_preparation`. Closure-read budgets count
requests against staged and inherited objects, not only physical I/O. These concrete checks
are not established by the arithmetic lemmas. The existing full overlay traversal is
the immediate exact source. Task 7755 separately owns its proven incremental replacement
and the historical-retention encoding cost; no delta-scaling claim follows from this repair.

The first broad release run passed 311/312 because the exhaustive central derivation matrix
omitted these new capstones and ten earlier journal capstones. The matrix and executable
bindings now include all 89 stable IDs and all 31 printed capstones; no completeness check
was removed. The publication-owner JO obligations are a separate model/integration boundary,
not silently included in this 89-ID claim. See J-056 in the engineering journal.

## Incremental publication ownership and retry refinement — 2026-09-06

Before changing incremental append, `CatalogAppointedPrefix` proves preservation of
the appointed byte prefix when an unselected suffix is discarded and a replacement
is appended. These list laws do not grant filesystem mutation authority: Rust must
hold the same writer lease, authenticate the exact catalog, preserve all bytes below
its selected frontier and reject a short authoritative file. Existing catalog-selection
and JO ownership models supply the separate publication/recovery contract.

Extracted tests must cover pre-catalog failure, every torn slot prefix, post-publication
failure and panic while an outer owner retains the writer and old-generation pin.
The observer must still see lock contention and the same generation registry; releasing
the owner is its positive control. Old recovery preserves the selected pack prefix and
retries the original transition once; new recovery never reapplies that transition.
This is a borrowed publication refinement, not completed hot-store acknowledgement,
incarnation binding, operation-gate ownership or cancellation integration.

The executable refinement now checks complete catalog equality against an uninterrupted
control: state, epoch, sequence, transaction digest, retention reference and pack/index
frontiers. Pre-slot injected failures recover the exact predecessor; completed slot writes
observed by same-process recovery select the exact successor. Torn prefixes may select
only one of those two complete authorities. This is fault-injected filesystem observation,
not an assertion that an unsynchronized write survives power loss.

For predecessor recovery, an unchanged whole catalog image permits reuse of the original
prepared capability. If a torn inactive slot changes that image, the original capability
must be refused without writes; the same mutation is resealed under the recovered image
and must produce the identical receipt and successor. Recovered successors are never
republished. Full pack/index byte comparisons and a final independent reopen constrain
the result, rather than checking only its semantic root.

| Executable test in `durable_publication.rs` | Required refinement |
|---|---|
| `borrowed_incremental_publication_recovers_and_retries_every_phase` | Exact outcome classification and authority across all 16 incremental publication phases, same lease/registry/pin, selected-prefix preservation and retry. |
| `borrowed_incremental_publication_retains_authority_for_every_torn_slot_prefix` | Every proper slot prefix; image-sensitive capability reuse/refusal, exact resealing and retry. |
| `borrowed_incremental_publication_unwind_keeps_the_outer_owners_capabilities` | Four injected unwinds retain the outer test owner's lease and pin; this is not yet runtime owner cancellation coverage. |
| `borrowed_incremental_publication_never_extends_a_short_appointed_pack` | Four lengths below the selected frontier fail without extension or any catalog/pack/index mutation. |
| `borrowed_incremental_publication_trims_only_under_exact_store_authority` | Stale catalog and foreign registry refuse before truncation; an exact authority discards only the unappointed suffix. |
| `production_trusted_crash_recovers_and_retries_every_phase` | Added by task 11946 and called by TS-11's test function: the same crash check for the trusted publication at its 13 phases (hooks 2 to 14 of the trusted successor publication section) and at every proper slot prefix, with the same outcome classes, retained lease, registry and pin, stale-capability refusal, resealed retry and final byte comparisons. |

The persistence module passes 317/317 in debug and release. The existing theory checks
with 33 closed assumption reports, and the exhaustive central matrix now includes
91 IDs with the two IP prefix-law bindings. JO owner-state obligations remain separate.
See [J-058](pathmap-engineering-journal-2026-08-23.md#j-058--2026-09-06-exact-incremental-publication-recovery-and-retry).

## Vocabulary and notation

- **Canonical object reference**: a pair `$(h, n)$`, where `$h$` is the canonical-object digest and `$n$` is its declared canonical byte length.
- **Current closure** (`$L$`): the exact canonical-object closure of the current semantic state.
- **Retained union** (`$R$`): the digest-deduplicated union of every retained semantic-state closure, including `$L$`.
- **Checkpoint DATA inventory** (`$D$`): `$R$` plus the retention-manifest object, which is not allowed to alias an object in `$R$`.
- **Authority**: the one catalog-selected generation from which restart-grade recovery is permitted.
- **Uncertainty boundary**: the instant immediately before the publisher attempts the first byte of the inactive catalog-slot write.
- **Definitely not published** (DNP): the publisher did not attempt that slot write. It does not assert that an external actor left the catalog unchanged.
- **Indeterminate**: the slot write began, but the publisher did not establish durable completion.
- **Published**: a complete new catalog authority was selected, even if subsequent recovery or semantic admission failed.

For a sequence `$X$`, `$|X|$` denotes its distinct object count and `$\operatorname{bytes}(X)$` denotes the sum of declared canonical lengths. The required inventory equations are:

```math
D = R \uplus \{m\},\qquad |D| = |R| + 1,\qquad
\operatorname{bytes}(D) = \operatorname{bytes}(R) + \operatorname{len}(m)
```

where `$m$` is the retention-manifest reference and `$\uplus$` is disjoint union.

## Formal evidence

- Rocq theory: `formal/rocq/stack_safe_pda/theories/GenerationCheckpointPublication.v`
- TLA+ state machine: `formal/tlaplus/stack_safe_pda/RSpaceGenerationCheckpointPublication.tla`
- Integrated gate: `scripts/check-stack-safe-pda-proofs.sh`
- Captured verification output: `scratch/formal-first-full-gate.log` (ignored measurement evidence)

The corrected formal evidence closes 17 Rocq capstones, checks 7,896 distinct TLA+ states from both zero and nonzero seeded runtime-accounting snapshots without an invariant failure, rejects 28 targeted mutants by their intended invariant, and reaches 14 outcome, boundary, resource-family, and recovery witnesses that make the model non-vacuous. The repository-wide integrated gate completed successfully across Rocq, Verus, Z3, TLC, and mCRL2 before the executable refinement was accepted.

## Exhaustive executable property catalog

The Rust property target is `rspace++/tests/persistence/generation_checkpoint_properties.rs`. Rows that were deliberately established as failing controls before implementation retain that history in the verification record below; every current target is green and non-ignored.

### Inventory and closure properties

| ID | Required property | Generator and oracle |
|---|---|---|
| INV-01 | Merging a fresh manifest reference with `$R$` produces an exact permutation of `$R \uplus \{m\}$`. | Generate a strictly ordered map of unique digest/length pairs plus a disjoint manifest; compare the zipper visitor with a mathematical ordered merge. |
| INV-02 | The merge increases distinct cardinality by exactly one. | Same generator; assert `$|D|=|R|+1$`. |
| INV-03 | The merge increases canonical bytes by exactly the manifest length. | Same generator; use checked reference-length addition. |
| INV-04 | Every member of `$R$` occurs exactly once in `$D$`. | Count observations by the complete reference pair. |
| INV-05 | The manifest occurs exactly once in `$D$`. | Count the generated manifest reference. |
| INV-06 | `$D$` is strictly increasing by digest without materialization or sorting in production. | Assert adjacent visitor output and compare with the mathematical merge only in the test oracle. |
| INV-07 | A manifest digest already present in `$R$` is rejected, including equal-digest/different-length aliases, and the error reports both lengths. | Generate the retained length and an independent conflicting manifest length. |
| INV-08 | The current closure is a subset of the retained union and `$D$` is exactly the retained union plus the manifest. | Generate a dependent current subset; do not discard cases with `prop_assume!`. |

### Descriptor properties

| ID | Required property | Generator and oracle |
|---|---|---|
| DES-01 | The descriptor carries all six exact inventory scalars: `$|L|$`, `$\operatorname{bytes}(L)$`, `$|R|$`, `$\operatorname{bytes}(R)$`, `$|D|$`, and `$\operatorname{bytes}(D)$`. | Generate dependent `$L \subseteq R$`; compare construction and parsed view with independently folded values. The recorded pre-refinement control lacked `$|L|$`. |
| DES-02 | Canonical encode/parse round-trip preserves all six scalars, profile, state, retention reference, and ordered digest. | Round-trip every generated coherent descriptor. |
| DES-03 | Mutating any one inventory scalar without consistently rebuilding the authenticated record is rejected. | Enumerate the six scalar fields rather than choosing an unstructured byte offset. |
| DES-04 | `$D$` count and byte values equal the checked disjoint-union equations. | Independent arithmetic oracle. |
| DES-05 | Both current count and current bytes are bounded by the corresponding retained-union values. | Generate valid dependent values, then mutate each inequality across its boundary. |
| DES-06 | The ordered inventory digest is a domain-separated fold over expected count, expected bytes, and every ordered `$(h,n)$` pair. | Independent reference hasher using the frozen domain and V1 field order. |
| DES-07 | Omission, insertion, duplication, reordering, digest substitution, and declared-length substitution are detected by construction, parsing, or commit binding. | Use an explicit `InventoryMutation` enumeration with one case per mutation family. |
| DES-08 | Every count, byte, and offset addition fails closed on `u64` overflow. | Bias generation around zero, one, and `u64::MAX`; no wrapping oracle arithmetic. |

### Wire-layout properties

| ID | Required property | Generator and oracle |
|---|---|---|
| WIR-01 | The target checkpoint descriptor canonical length is exactly 216 bytes. | Constant assertion plus arbitrary coherent descriptor encodings. The recorded pre-refinement control was 208 bytes. |
| WIR-02 | The checkpoint PREPARE length is `$288 + 40|D| + \operatorname{bytes}(D) + 8$`. | Compare the encoded length and authenticated trailer position with checked arithmetic. |
| WIR-03 | The fresh index tail is `$64 + 72|D|$`. | Compare the index plan and emitted record cursor. |
| WIR-04 | The checkpoint COMMIT address is `$128 + \operatorname{prepareLen}$`. | Compare the planned address with the pack-header and PREPARE extents. |
| WIR-05 | The final pack tail is `$\operatorname{commitAddress} + 400$`. | Compare planned catalog tail, file length, and recovered terminal address. |
| WIR-06 | Ordinary incremental V1 PREPARE/COMMIT wire images remain byte-for-byte unchanged. | Keep their frozen golden vectors separate from checkpoint-kind goldens. |

### WAL and index properties

| ID | Required property | Generator and oracle |
|---|---|---|
| WAL-01 | Checkpoint PREPARE and fresh index contain the identical complete ordered `$D$` inventory. | Collect both streaming visitors and compare complete reference sequences. |
| WAL-02 | COMMIT binds the exact PREPARE identity, complete descriptor authority, index publication authority, previous terminal, address, store, digest, and sequence. | Generate one coherent publication tuple and verify the parsed commit through relational binding predicates rather than trusted Boolean claims. |
| WAL-03 | Mutating any bound receipt, authority, or membership fact causes verification to fail. | Enumerate the 16 formal binding facts and map each to its typed parser, verifier, or restart-recovery boundary; index count/digest are authenticated authority facts, not invented standalone COMMIT fields. |
| WAL-04 | Every generation-checkpoint object is emitted as DATA; REUSE is forbidden even when an older generation contains the same digest. | Replay a multi-generation fixture with maximal overlap and reject any REUSE disposition. |
| WAL-05 | Every truncation, trailing extension, checksum mutation, discriminator mutation, and authenticated-length mutation is rejected without panic. | Generate all prefixes plus structured single-field mutations and arbitrary hostile byte strings. |

### Catalog selection and recovery properties

| ID | Required property | Generator and oracle |
|---|---|---|
| CAT-01 | Empty, old-only, and torn inactive-slot images recover the old complete authority. | Enumerate every inactive-slot prefix shorter than one complete slot. |
| CAT-02 | A complete, checksum-valid, restart-grade successor slot recovers the new authority. | Generate a coherent successor image and durable artifacts. |
| CAT-03 | A complete selected successor with authoritative pack, terminal, descriptor, or unsafe index-identity damage is rejected and never silently falls back to old. A missing or safely rebuildable derived index is reconstructed from authenticated WAL authority while the exact successor remains selected. | Exercise one two-complete-slot integration table over fatal authoritative damage, unsafe index identity, and every safely repairable derived-index shape. |
| CAT-04 | Compare-and-swap compares the exact full 672-byte old catalog image before attempting the inactive-slot write. | Mutate every byte position and assert no publisher slot write; external bytes remain exact. |
| CAT-05 | Recovery can select only the old or the exact successor authority; no third authority can be synthesized. | Compare complete recovered authority tokens with the two generated tokens. |
| CAT-06 | Initial selection of new implies exact PREPARE/index authority plus DATA, index, COMMIT, artifact-directory, and catalog-file durability. Same-process physical recovery, semantic readmission, and acknowledgement additionally require catalog-directory durability; restart selection may independently observe an already complete durable slot. | Use phase hooks and recovered physical evidence, preserving the formal distinction between catalog-file selection and the following directory barrier. |

### Affine publication and outcome properties

| ID | Required property | Generator and oracle |
|---|---|---|
| PUB-00 | Initial admitted state owns exactly one old-writer lease. | Attempt a competing writer open while retaining the admitted writer. |
| PUB-01 | Every transition consumes or transfers the unique lease without cloning or a lock gap. | Probe contention at every observable phase and after success/error destruction. Duplicate the lock's open file description and prove that dropping the affine Rust lease explicitly unlocks it before the duplicate closes, covering the transient descriptor inheritance caused by a concurrent process spawn. |
| PUB-02 | Every phase has exactly its specified predecessor; phase skipping cannot publish. | Enumerate the protocol phase/failure matrix through typed hooks. |
| PUB-03 | Certificates occur in exact order: preflight, cleanup, PREPARE write/sync, index write/sync, COMMIT write/sync, artifact-directory sync, exact CAS, slot write, catalog-file sync, catalog-directory sync, physical recovery, semantic readmission, acknowledgement. | Record a test-only event trace from real operations and compare with the formal sequence. The recorded pre-refinement control lacked several write/start and admission hooks. |
| PUB-04 | Resource preflight precedes successor cleanup, and cleanup precedes every generation mutation. | Seed authorized orphan artifacts and force each preflight refusal. |
| PUB-05 | New authority is unobservable before all generation artifacts and their directory entries are durable. | Fail at every pre-catalog phase and reopen old. |
| PUB-06 | The slot write begins only after exact full-image CAS succeeds. | Mutate each catalog byte at the CAS hook. |
| PUB-07 | No pre-slot phase can classify as Indeterminate or Published. | Enumerate pre-slot failpoints and assert DNP. |
| PUB-08 | Every pre-slot failure is DNP, returns no stale writer, reopens old authority, and supports an exact retry. | Phase/failure matrix with filesystem evidence. |
| PUB-09 | Every torn slot prefix is Indeterminate and restart selects only an intact old or complete new authority. | Enumerate prefix lengths from zero through 335 bytes. The recorded pre-refinement control lacked partial catalog-write injection. |
| PUB-10 | A complete slot followed by catalog-file sync failure is Indeterminate and restart selects new when the complete bytes reached storage. | Inject the existing catalog-sync failure and compare exact authority. |
| PUB-11 | Uncertainty begins immediately before the first slot-write byte, never at preflight/CAS and never after the write. | Boundary failpoints bracketing the slot operation. |
| PUB-12 | Physical recovery of new authority is impossible before the catalog-directory durability barrier. | Inject failure between catalog-file and catalog-directory sync. The recorded pre-refinement control lacked the post-catalog directory sync and typed error. |
| PUB-13 | Success is returned only after the ordinary restart physical-recovery path and semantic admission reproduce unchanged state and inventories. | Nontrivial multi-object, multi-retained-state fixture. |
| PUB-14 | Failure after durable publication but during physical recovery is typed Published, returns no writer, and later restart selects new. | Damage or deny the selected generation at the recovery hook, then repair and reopen. |
| PUB-15 | Failure after physical recovery but during semantic admission is typed Published, returns no writer, and later restart still selects new. | Inject semantic-admission failure. The recorded pre-refinement control lacked admission failure injection. |
| PUB-16 | DNP, Indeterminate, Published, and success form a total, mutually exclusive outcome partition for every injected operation-level phase failure and catalog-slot prefix. | Generate the modeled phase and failure kind as a dependent pair. Byte-progress and synchronization-uncertainty faults belong to the finer crash-I/O refinement and are not discharged by this row. |
| PUB-17 | DNP means this publisher did not attempt a slot write; it does not overwrite an externally changed catalog with the old image. | CAS race property with an exact externally supplied 672-byte image. |
| PUB-18 | Restart after success preserves semantic state, all six inventory scalars, retention bytes, and terminal authority for a nontrivial fixture. | Generate a bounded canonical DAG with shared subgraphs and one to four retained states. |
| PUB-19 | A later successful restart does not retroactively change an earlier Indeterminate or Published error classification. | Retain the original error value across repair/restart and compare its outcome. |
| PUB-20 | Cleanup or retry mutation never runs after the uncertainty boundary in the same publication attempt. | Seed recognizable successor evidence and inject every post-boundary failure. |

### Semantic accounting, resources, and totality

| ID | Required property | Generator and oracle |
|---|---|---|
| SEM-01 | Checkpoint publication emits no accounting event and preserves an arbitrary, possibly nonzero, complete runtime-budget snapshot: one-unit-per-COMM consensus total, remaining budget, canonical diagnostic trace digest and event count, last out-of-phlogiston event, and per-lane demand. | The formal model quantifies over seeded zero and nonzero snapshots. While the crate-private publisher remains dormant below `rholang`, require an architectural capability/dependency guard plus the exhaustive publication outcome matrix; before production activation, the higher-layer adapter must compare complete quiescent snapshots across success and every reachable failure class. |
| SEM-02 | Local pack, index, catalog, orphan, dead, and recovery-work bytes are physical diagnostics and cannot affect COMM accounting. Any future restored storage tariff is a separate typed logical ledger whose versioned definition—not physical layout—selects its input. | Perturb physical layout while holding semantic state fixed; prove exact semantic/successor equivalence and that the publisher has no accounting capability. Reject mutants that add physical bytes to COMM cost. |
| SEM-03 | Publication and restart preserve the exact semantic state rather than an observational subset. | Compare canonical semantic roots, version, retained states, and admitted reports. |
| RES-01 | Arbitrary descriptor, PREPARE, COMMIT, index, catalog, pack, and retention bytes are total: accept or return a typed error without panic. | Separate bounded arbitrary-byte properties per parser. |
| RES-02 | Every applicable durable, checkpoint-WAL, and semantic-admission size/count limit is authenticated and decided before destructive cleanup or authority mutation. `required−1` refuses with exact evidence; the exact boundary succeeds. Zero-demand axes accept their constructible minimum. | Generate one typed resource at a time from the exact admitted-source/layout certificate. Cover durable constructor-zero refusal separately from positive demand, and retain byte-identical catalog/orphan evidence for every refusal. |
| RES-03 | Recovery uses bounded scratch, stack, and open descriptors independent of object cardinality. | Measure retained-union cardinalities 2 (the minimum valid closure: state manifest plus root), 16, and 256 in isolated test processes; run recovery on a 256 KiB stack, require one pack reader, enforce 64 KiB read scratch, check descriptor closure, and cap peak process RSS. |

## Negative-control traceability

| Mutant | Must be rejected by | Executable property IDs |
|---|---|---|
| Affine owner clone | `AtMostOneMutableAuthority` | PUB-00, PUB-01 |
| Lease gap | `AuthorityIsContinuous` | PUB-01 |
| Phase skip | `PhaseFollowsAffineProtocol` | PUB-02, PUB-03 |
| Premature catalog | `NewAuthorityRequiresRestartGradeGeneration` | PUB-05, CAT-06 |
| Membership substitution | `CheckpointAndIndexExactlyAdmitted` | INV-04, INV-05, WAL-01, WAL-03 |
| Stale CAS | `CatalogWriteUsesExactOldImage` | CAT-04, PUB-06, PUB-17 |
| Third authority | `CrashRecoverySelectsOnlyValidAuthority` | CAT-01 through CAT-05 |
| Early acknowledgement | `AcknowledgementRequiresRestartPathReadmission` | PUB-13, PUB-15, SEM-03 |
| Inventory count | `DescriptorInventoriesAreExact` | DES-01 through DES-05 |
| Inventory bytes | `DescriptorInventoriesAreExact` | DES-01 through DES-05 |
| Inventory digest | `OrderedInventoryIsExact` | DES-06, DES-07 |
| Dirty-slot selection | `CrashRecoverySelectsOnlyValidAuthority` | CAT-01 through CAT-03 |
| Catalog failure mislabeled DNP | `OutcomeBoundaryIsExact` | PUB-07 through PUB-11, PUB-16 |
| Missing catalog-directory barrier | `PhysicalRecoveryRequiresCatalogDirectory` | PUB-12 |
| Cleanup after uncertainty | `PreflightAndCleanupPrecedeMutation` | PUB-04, PUB-20 |
| Wrong wire layout | `WireLayoutIsExact` | WIR-01 through WIR-05 |
| COMM charge pollution | `CheckpointPreservesSemanticAccounting` | SEM-01 |
| Diagnostic trace pollution | `CheckpointPreservesSemanticAccounting` | SEM-01 |
| Physical bytes charged as COMM cost | `PhysicalAccountingIsSeparated` | SEM-02 |
| Dishonest receipt binding | `ReceiptBindingsAreExact`, `EverySingleBindingMutationIsRejected` | WAL-02, WAL-03 |
| Required-minus-one accepted | `PreflightRequiresExactResourceAdmission`, `UnderprovisionedResourceCannotReachCleanup` | PUB-04, RES-02 |
| Exact boundary rejected | `ExactResourceBoundaryIsAccepted` | RES-02 |
| Late semantic-admission refusal | `ResourceRefusalPrecedesAllMutation` | PUB-04, RES-02 |
| Resource family omitted | `PreflightRequiresExactResourceAdmission` | RES-02 |
| Damaged successor falls back to old | `CrashRecoverySelectsOnlyValidAuthority` | CAT-03 |

## Non-vacuity requirements

The executable suite must contain at least one real trace for each outcome class rather than proving implications whose antecedents never occur.

| ID | Required witness |
|---|---|
| NV-01 | Successful acknowledgement after restart-grade readmission. |
| NV-02 | Old-authority recovery after a pre-publication or torn-write failure. |
| NV-03 | New-authority recovery after a complete successor slot. |
| NV-04 | Published error after new authority is durable but recovery or admission fails. |
| NV-05 | Indeterminate error after the slot-write boundary without a durable completion receipt. |
| NV-06 | Durable-resource `required−1` refusal before cleanup. |
| NV-07 | Invalid durable resource configuration refusal before publisher entry. |
| NV-08 | Checkpoint-WAL resource `required−1` refusal before cleanup. |
| NV-09 | Semantic-admission resource `required−1` refusal before cleanup. |
| NV-10 | Durable-resource exact-boundary acceptance reaches cleanup. |
| NV-11 | Checkpoint-WAL exact-boundary acceptance reaches cleanup. |
| NV-12 | Semantic-admission exact-boundary acceptance reaches cleanup. |
| NV-13 | Repairable derived-index damage reconstructs the index and recovers the exact successor. |
| NV-14 | Authoritative or unsafe-index damage rejects the selected successor without old fallback. |

## Incremental reachability cursor model (2026-09-06)

Task 7755 extends `GenerationCheckpointPublication.v` with `IncrementalReachability`.
This is a formal-first, single-domain graph model and a test-only executable specification.
It is not the production successor constructor, a proof of Rust memory safety, or a
replacement for cold object/semantic admission. Its current-state, retained-semantic and
metadata instances are composed in
[Current, retained and metadata inventory composition](#current-retained-and-metadata-inventory-composition-2026-10-25)
(task 7922).

Let $`R(v)`$ be explicit root multiplicity, $`C(v)`$ the derived incoming count,
$`A(v)`$ the active bit, and $`I(A,v)`$ the sum of outgoing edge occurrences
from active parents. Each distinct parent is enumerated once; duplicate edges are
preserved. Coherence means $`A(v) = (C(v)>0)`$. At quiescence,
$`C(v)=R(v)+I(A,v)`$. A finite graph with strictly decreasing edge ranks makes
that quiescent solution unique, with positive support exactly equal to rooted reachability.
The proof's finite rank induction is not a runtime traversal-depth limit.

Pending work $`P(v)`$ includes explicit events and every suspended cursor suffix.
Activation preserves $`C(v)+P(v)=R_+(v)+I(A,v)`$, where $`R_+`$ includes
added roots. Deactivation preserves $`C(v)=R_f(v)+I(A,v)+P(v)`$, where
$`R_f`$ subtracts authorized removals. Count/sign change and complete-cursor
installation are one abstract transition. Transfer emits exactly one reference;
discard requires an empty suffix. A completed two-phase execution agrees with any
independently computed coherent, quiescent ledger for the final roots.

The executable cases live in `tests/persistence/incremental_reachability_properties.rs`.
They use a small test-only vector graph and a separate cold reachability traversal;
this is not a conversion of a production PathMap into a vector. Generated DAGs preserve
duplicate references and sharing, and assertions cover every intermediate microstep.

| ID | Required property and negative-control boundary |
|---|---|
| IR-01 | Each reachable parent contributes once; repeated outgoing references each contribute. Distinct-parent enumeration is required. |
| IR-02 | Transfer exactly one cursor occurrence and preserve every vertex's pending count. |
| IR-03 | Only an exhausted cursor may disappear without transferring its residual contributions. |
| IR-04 | Activation uses the pre-update count and installs outgoing work only at zero-to-one. |
| IR-05 | Balance funds every legal decrement; coherence alone does not reject natural-number saturation at zero. |
| IR-06 | Deactivation installs outgoing work only at one-to-zero; repeated references are not collapsed. |
| IR-07 | Initialize against old roots plus every added occurrence, with added vertices in the graph domain. |
| IR-08 | Authorize removals against root multiplicity, not incoming count; preserve domain support. |
| IR-09 | Complete activation before deactivation; completed replacement agrees pointwise with the independently computed quiescent cold ledger. |
| IR-10 | Suffix validity does not prove processed history or authenticate real object bytes/provenance. A skipped prefix is a limitation witness. |
| IR-11 | Positive support equals root reachability at quiescence on the finite ranked graph, not during draining. |
| IR-12 | Any two coherent quiescent ledgers for the same acyclic graph and roots have identical counts and membership. |
| IR-13 | Every permitted microstep preserves balance, coherence, domain support and cursor suffixes; a completed run has exact support. |
| IR-14 | A rootless self-cycle admits a positive quiescent ledger; omitting acyclicity makes the intended reachability claim false. |
| IR-15 | Every legal microstep spends exactly one unit of the ghost potential, in either phase and any event/cursor schedule. |
| IR-16 | Every valid finite-domain machine reaches drainage; independent event/transfer/discard counters agree with the exact abstract step count, including empty-frame disposal. |
| IR-17 | Only vertices changing sign spend expansion credit; extending an unchanged, unreachable part of the graph does not change cursor work. |
| IR-18 | A resumed execution must count its initial cursor suffixes and frame disposal, not just its remaining explicit events. |
| IR-19 | Termination and exact work do not require acyclicity; a terminating self-cycle can still have unsupported positive membership after root removal. |
| IR-20 | Drainage is zero pending work, not zero total potential; untouched expansion credits may remain. A drained machine cannot step. |
| IR-21 | Wrong edge-occurrence weights, omitted frame-disposal cost and incorrect transition behavior must be distinguished from the exact potential law. |

### Checked counter and unique inventory refinement

`CheckedReachability` refines the natural-number cursor ledger with a symbolic
counter limit. The Rust property instantiates that limit at `u64::MAX` and uses
`u128` as an independent oracle for a one-step sum or difference. For each
vertex, its reference count changes on every edge occurrence, while its object
and byte contributions change only when the count crosses zero. The byte weight
is the object's declared canonical length; the object weight is one. The test
model has two explicit vertices and an unchanged background inventory. The
background is included in the independent `u128` oracle and never changed by an
edge event. Generated states require zero background bytes when there are zero
background objects, so the pair represents a valid weighted inventory. It
permits the exact `u64::MAX` distinct-object boundary without
allocating that many vertices; the two explicit vertices and large byte weights
also exercise byte overflow. A zero-weight abstract object isolates the object
counter's overflow check from the byte counter. That arithmetic control is
permitted by the natural-number theorem but is not a claim that a zero-length
canonical object can be admitted by production code.

| ID | Required property and negative-control boundary |
|---|---|
| CR-01 | A checked increment succeeds exactly below the symbolic limit, preserves bounded counts and coherence, and includes the exact maximum and one-below-maximum cases. |
| CR-02 | A checked decrement succeeds exactly for positive counts, refuses at zero, preserves bounded counts and coherence, and leaves its predecessor unchanged on refusal. |
| CR-03 | Activating a zero-count vertex adds one distinct object and its byte weight exactly; another edge to an active vertex changes neither inventory. Object/byte overflow refuses before changing the predecessor. |
| CR-04 | Deactivating the last reference subtracts exactly one object and its byte weight; earlier duplicate-edge removals change neither inventory. A coherent exact inventory cannot underflow, and an invalid zero-count decrement is rejected. |

The normal-suite bindings are exact: CR-01 and CR-03 select
`checked_reference_and_unique_inventory_refine_natural_arithmetic`; CR-02 and
CR-04 select `checked_inventory_boundary_controls_preserve_predecessor`.
`model_property_manifest_is_unique_and_exhaustive` checks these
four bindings against the catalog and included test sources. The eighteen
`CheckedReachability` theorem names in the derivation matrix below are the
formal targets; their compiled theory is independently kernel-checked.

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/GenerationCheckpointPublication.v` | `5b9e5199c4001e9620d25ff4b0c700f2670fe23303ca5d1b293d53df2a8b7500` |
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `b7dd63b1d5c2adc6fb6b3d59cc0c690e2c4d506cdf21be772fca8581566be307` |
| `rspace++/tests/persistence/property_support.rs` | `5a3d0c00ce1c4afb1c547bf7164198a0e7533e08b49f754bdd6cd4eb95bcd7c3` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `7b6f0504278b474155ab53f86ef8c4f4677c4a33d9489d0f099a5cd0f99733fb` |

These are abstract checked-arithmetic and test-model bindings. They do not prove
that a production successor maintains the exact ledger or authenticates objects.
Failure-atomic bounded trials are covered at the model and test-model level in
[Failure-atomic bounded private ledger trials](#failure-atomic-bounded-private-ledger-trials-2026-10-25);
production copy-on-write nonmutation remains task 7924.

### Exact abstract work and termination

Let $`d(v)`$ count outgoing edge occurrences of vertex $`v`$, including duplicates,
and define its one-time expansion credit as $`g(v)=1+2d(v)`$. The one counts the
eventual disposal of its child frame; the two counts transfer and consumption of
each child occurrence. Let $`E`$ be the explicit event list and $`F`$ the suspended
frames, with $`s_f`$ the unprocessed suffix in frame $`f`$. The pending-step cost is

```math
Q = |E| + \sum_{f\in F}(1+2|s_f|).
```

For the finite, duplicate-free vertex domain $`V`$, let $`U`$ sum $`g(v)`$ over
inactive vertices during activation, or active vertices during deactivation.
The ghost potential is $`\Phi=Q+U`$. It is mathematical proof instrumentation,
not a production initialization scan, stored counter or fuel parameter.
Every actual cursor transition satisfies

```math
\Phi_{\mathrm{before}} = 1 + \Phi_{\mathrm{after}}.
```

An event without a zero crossing removes one pending unit. A crossing installs
the full cursor and releases exactly its expansion credit. A transfer removes
two suffix units and adds one event; disposal removes the final frame unit.
Progress holds whenever events or frames remain, and the strictly decreasing
natural-number potential excludes infinite sequences of actual steps. Indefinite
external stuttering or a scheduler that never calls the machine is not covered.

Let $`\Delta`$ be the vertices whose active bit changes during one phase. Each
changes at most once. For a drained execution of $`k`$ actual steps,

```math
k = Q_{\mathrm{initial}} + \sum_{v\in\Delta}(1+2d(v)).
```

With initially empty frames, $`Q_{\mathrm{initial}}`$ is exactly the explicit
root-event count. A resumed cursor requires the full expression for $`Q`$.
Untouched vertices can leave positive $`U`$ after drainage; only $`Q=0`$ is the
drainage criterion. These termination/work results do not need a ranked graph;
acyclicity is still required for the separate exact rooted-support result.

**Coverage boundary.** Natural-number proofs and bounded test arithmetic do not yet
establish checked production `u64` overflow/refusal, failed CoW-trial nonmutation,
unique-byte/kind inventories, or cold-resource certificates. The next section proves
checked refusal and failure atomicity for the abstract trial, its Verus counters and
its test model only. Abstract termination
and the exact changed-frontier step formula are proved; runtime initialization,
byte access, lookup, validation, allocation and CoW costs need separate refinement.
The test-only graph scans and vector operations are not production complexity
evidence. `authenticated_cursor` means only
an exact suffix of an abstract parent's children; digest/length/kind, source identity
and namespace/exact-path authorization require their own concrete refinement.
Acyclicity and complete drainage remain essential for exact rooted support.

### Failure-atomic bounded private ledger trials (2026-10-25)

Task 7921 adds the trial layer that the incremental successor (task 7755) needs
before it may touch real state. A *trial* runs the two-phase cursor machine of
the previous sections on a private copy of the predecessor's ledger: activation
of the added root occurrences drains first, then deactivation of the removed
ones. Every transition is charged against separate budgets *before* it is
accepted. A refused trial returns its predecessor unchanged. A drained trial
returns prepared evidence, which carries no durable receipt and no owner
authority. Three artifacts implement the layer:

- the Rocq theory `formal/rocq/stack_safe_pda/theories/IncrementalLedgerTrial.v`
  (module `CheckedLedgerTrial`, built on `IncrementalReachability` and
  `CheckedReachability`);
- the Verus model of the `u64` reservation code in
  `formal/verus/incremental_prepare_streaming.rs`;
- the executable test model in
  `rspace++/tests/persistence/incremental_reachability_properties.rs`.

None of them is the production successor constructor. That constructor is the
subject of tasks 7924 (copy-on-write reachability ledger) and 7927 (trusted
successor).

![Lifecycle of a bounded private ledger trial: authorization, two charged phases, the final guard, and the refusal edges that all return the predecessor](figures/ledger-trial-lifecycle.svg)

**Inputs.** A predecessor $`\pi = (\Lambda, R, O, B)`$ consists of a ledger
$`\Lambda`$ (reference counts and active bits), root multiplicities $`R`$, and
the exact object and byte inventories $`O`$ and $`B`$ of $`\Lambda`$. A trial
adds the root occurrences in the list $`\alpha`$ and removes those in the list
$`\rho`$. Write $`\mathrm{occ}_\rho(v)`$ for the number of occurrences of
$`v`$ in $`\rho`$, and $`c_v`$ for the reference count of $`v`$ in the current
ledger. Each object $`v`$ has a byte weight $`w(v)`$ (its canonical
length), an externally quoted read size $`q(v)`$, and an actual read size
$`r(v)`$. A schedule chooses, at step $`i`$ of a phase, between the top cursor
and the top event whenever both exist. It is the same rule the executable
cursor machine uses.

**Budget axes.** A trial's usage $`u`$ records seven step quantities and one
unbudgeted observation. Limits $`L`$ assign a natural number to each of nine
axes. A *peak* axis records the largest value a live quantity has reached; a
*cumulative* axis records a running total; a *final* axis is checked once, on
the drained ledger.

| Axis | Kind | What it measures | Charged by |
|---|---|---|---|
| Count | peak | the largest reference count an increment reaches | an activation event on $`v`$: $`c_v + 1`$ |
| UnionObjects | peak | distinct objects held by the add-before-remove union | the activation start ($`O`$), then each activation zero-crossing ($`+1`$) |
| UnionBytes | peak | bytes held by that union | the activation start ($`B`$), then each activation zero-crossing ($`+w(v)`$) |
| Scratch | peak | live scratch: pending events plus suspended frames, $`\lvert E\rvert + \lvert F\rvert`$ | every step and every phase start |
| References | cumulative | reference events: root occurrences plus cursor transfers | the phase starts ($`\lvert\alpha\rvert`$, then $`\lvert\rho\rvert`$) and each transfer ($`+1`$) |
| Expansions | cumulative | child-list expansions, the zero-crossings of either phase | $`+1`$ per expansion |
| QuotedBytes | cumulative | externally quoted read bytes | $`+q(v)`$ per expansion |
| FinalObjects | final | the drained ledger's object inventory | checked after deactivation drains |
| FinalBytes | final | the drained ledger's byte inventory | checked after deactivation drains |

The consumed read bytes $`\sum r(v)`$ over expansions are recorded but have no
limit: they are what an external quote promises to bound. The union axes and
the final axes are separate domains on purpose. During add-before-remove the
ledger holds the closures of both the old and the new roots, so the union peak
can exceed the final inventory. Live scratch is the peak of a live quantity,
not a count of allocations.

**One checked step.** Let $`k`$ be the charge of the scheduled transition: the
count, objects, bytes and live scratch it would reach, and the reference,
expansion and quoted-byte amounts it would add. The committed usage is

```math
\mathrm{commit}(u, k)_a =
\begin{cases}
\max(u_a, k_a) & \text{if } a \text{ is a peak axis},\\
u_a + k_a & \text{if } a \text{ is a cumulative axis}.
\end{cases}
```

The step is refused at the first axis $`a`$, in the order of the table's first
seven rows, with $`\mathrm{commit}(u, k)_a > L_a`$. A refusal changes nothing:
not the ledger, the inventories, or the usage. A deactivation event on a zero
count is refused as `ZeroCountDecrement`, and an inventory subtraction that
would underflow as `InventoryUnderflow`; neither can occur from a valid
configuration. Each phase runs for at most its ghost potential $`\Phi`$ steps
(previous section). A phase that has not drained by then is refused as
`PotentialExhausted`, which valid input never reaches.

**The trial, in literate form.** Each numbered part names one refinement
obligation; every refusal in parts 1–6 returns
$`(\pi, \mathrm{Refused}(\mathit{why}))`$.

1. ⟨Authorize the removals⟩. Refuse `UnauthorizedRemoval` unless
   $`\mathrm{occ}_\rho(v) \le R(v) + \mathrm{occ}_\alpha(v)`$ for every $`v`$ in
   $`\rho`$. The check precedes all work, so an unauthorized trial costs
   nothing.
2. ⟨Start activation⟩. The workspace is $`(\Lambda, \alpha, [\,])`$ with
   inventories $`(O, B)`$. Reserve the start charge: $`\lvert\alpha\rvert`$ live
   scratch, $`\lvert\alpha\rvert`$ references, and the predecessor's inventory
   as the first union peak. Refuse on the first exceeded axis.
3. ⟨Drain activation⟩. Repeat ⟨one checked step⟩ with the schedule's bit for
   step $`i`$ until the workspace has no events and no frames.
4. ⟨Start deactivation⟩. The workspace is $`(\Lambda_{\mathrm{mid}}, \rho, [\,])`$.
   Reserve $`\lvert\rho\rvert`$ live scratch (as a peak) and
   $`\lvert\rho\rvert`$ references, or refuse.
5. ⟨Drain deactivation⟩. As in part 3.
6. ⟨Final guard⟩. Refuse on FinalObjects if the drained object inventory
   exceeds its limit, otherwise on FinalBytes if the byte inventory does.
7. ⟨Prepare⟩. Return the evidence
   $`(\Lambda_{\mathrm{fin}}, R + \alpha - \rho, O_{\mathrm{fin}}, B_{\mathrm{fin}}, u)`$
   together with the untouched predecessor.

**Theorems.** The module proves twenty capstones. Each has a
`Print Assumptions` line, every one reports a closed context, and the compiled
module passes independent kernel checking.

| # | Theorem (`CheckedLedgerTrial.`…) | Statement |
|---|---|---|
| 1 | `checked_attempts_are_exactly_scheduled_cursor_steps` | An accepted attempt is a `cursor_step`. An attempt reports drained exactly when events and frames are empty. From a valid configuration with exact counters, no attempt is invalid, and every cursor step is the attempt of some schedule bit. |
| 2 | `checked_step_is_failure_atomic` | A refused step returns the phase's current state. Its reason is either the first exceeded axis of the attempted usage or an invalid attempt. |
| 3 | `accepted_event_reservations_are_checked_arithmetic` | An accepted activation event is `checked_increment` under the Count limit and `checked_add` under the union limits. An accepted deactivation event is `checked_decrement` and `checked_sub`. |
| 4 | `every_trial_retains_the_bit_identical_predecessor` | For every variant except the partial-commit control, the retained predecessor is the input. |
| 5 | `prepared_trial_agrees_with_cold_ledger` | On a ranked graph, a prepared ledger equals any coherent quiescent ledger for the final roots, with exact roots and inventories. |
| 6 | `union_peak_is_the_add_phase_cold_inventory` | On a ranked graph, the union peak equals the inventory of the quiescent ledger for $`R + \alpha`$. |
| 7 | `checked_trial_is_the_guarded_unbounded_trial` | The trial under $`L`$ prepares evidence $`e`$ exactly when the unbounded trial prepares $`e`$ and $`L`$ covers $`e`$, that is, $`e_a \le L_a`$ for every axis $`a`$. |
| 8 | `trial_accepts_at_exact_measured_limits` | Limits equal to the measured evidence are accepted. |
| 9 | `trial_refuses_one_below_on_that_axis` | From the exact limits, lowering one positive axis by one refuses on exactly that axis. |
| 10 | `every_valid_authorized_trial_prepares_without_limits` | Valid, authorized input always prepares when unbounded. |
| 11 | `refused_trial_retries_from_identical_predecessor` | Retrying from the retained predecessor with covering limits prepares the unbounded evidence. |
| 12 | `live_scratch_is_bounded_by_its_reserved_peak` | Each phase start charges one reference per root occurrence. Live scratch stays at or below its reserved peak, which stays within the limit. |
| 13 | `sound_quotes_bound_consumed_read_bytes` | If $`r(v) \le q(v)`$ for every $`v`$, then consumed bytes $`\le`$ quoted bytes $`\le L_{\mathrm{QuotedBytes}}`$. |
| 14 | `unsound_quote_admits_unbounded_consumption` | With $`q = 0`$ and a zero quote limit, a trial that consumes $`r = n + 1`$ bytes is admitted for every $`n`$. |
| 15 | `partial_commit_mutant_replaces_the_predecessor` | Negative control: committing in-flight state on refusal changes the predecessor. |
| 16 | `final_union_conflation_mutant_refuses_exact_final_limit` | Negative control: checking the union peak against the final limit refuses an admissible trial. |
| 17 | `scratch_allocation_conflation_mutant_refuses_admissible_trial` | Negative control: charging expansions as scratch refuses an admissible trial. |
| 18 | `skipped_cursor_contribution_mutant_is_not_quiescent` | Negative control: dropping a transferred child yields a non-quiescent prepared ledger. |
| 19 | `duplicated_cursor_contribution_mutant_is_not_quiescent` | Negative control: transferring a child twice yields a non-quiescent prepared ledger. |
| 20 | `uncharged_phase_initialization_mutant_overruns_scratch` | Negative control: leaving phase starts uncharged admits live scratch above the limit. |

**Verus counters.** `incremental_prepare_streaming.rs` gains the `u64`
reservation code that a production trial would run on each step:

- `checked_trial_reserve` reserves a cumulative axis. It forms the sum only
  when the sum fits the limit, so machine overflow is a limit refusal.
- `checked_trial_peak` reserves a peak axis, and `checked_trial_release`
  releases live scratch or inventory without underflow.
- `reserve_trial_step` reserves all seven axes into locals before it writes
  `usage`. Its postcondition says that a refusal names
  `first_refused_axis_spec` and leaves `usage` unchanged, and that an
  acceptance commits exactly `committed_usage_spec`.
- Four proof functions pin the boundaries: `exact_trial_limit_is_accepted`,
  `one_below_trial_limit_is_refused`, `trial_overflow_is_a_limit_refusal`
  and `released_scratch_is_reusable_under_its_peak`.

The file now verifies 23 functions, up from 15.
`scripts/generate-ledger-trial-verus-mutant.py` derives three mutants, and the
integrated runner requires each to fail at its named obligation:

| Mutant | Change | Obligation that must fail |
|---|---|---|
| `partial_commit` | writes the Count peak before a later axis refuses | `reserve_trial_step`: a refusal leaves `usage` unchanged |
| `unchecked_limit` | refuses only on machine overflow, never on the limit | `checked_trial_reserve`: the result equals the specified reservation |
| `scratch_conflation` | charges expansions as scratch | `reserve_trial_step`: the refused axis is `first_refused_axis_spec` |

**Executable bindings.** The Rust test model mirrors the Rocq semantics and is
checked against an independent cold-graph oracle. `Machine::step` is split into
a pure `scheduled_action` and its application, so the trial charges exactly the
transition that the cursor machine then takes. The activation increment is now
checked, refusing with `Refusal::CountOverflow`. Usage is held in `u64`, and
candidate live values are computed in `u128`, so a value above `u64::MAX` is a
limit refusal, as in Verus. Each generated case has:

- a DAG of one to six objects, each with up to three children of lower index,
  duplicates kept;
- byte weights and quoted and consumed read sizes from 0 to 4;
- a cold predecessor for up to four root occurrences;
- up to four added occurrences, and an authorized removal subset of the old
  and added roots;
- a schedule of one to eight bits, repeated cyclically.

Each property runs 256 cases.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| LT-01 | Every accepted step equals the executable cursor machine's scheduled step from the same configuration and schedule bit. It keeps the cursor-model configuration invariants and exact inventories, spends one unit of potential and lowers no usage counter, and the two schedule bits reach every available step. | `checked_trial_refines_cursor_steps_and_matches_cold_oracle` |
| LT-02 | A refused step leaves the whole trial state unchanged and names the axis it exceeded. Each axis that a step raises is accepted at exactly the reached value and refused one below it. Accepted events are checked increments, decrements, sums and differences. Host controls: `u64` count, background-object and quoted-byte overflow are limit refusals, and the cursor machine's own increment refuses at `usize::MAX`. | `checked_trial_budget_boundaries_are_exact_and_atomic` |
| LT-03 | Every trial of every variant except partial commit, whether prepared or refused, retains the bit-identical predecessor. An unauthorized removal is refused before any work: even the partial-commit variant then retains the predecessor. | `refused_trial_retry_matches_unbounded_evidence` |
| LT-04 | A prepared trial equals the independently computed cold ledger and inventory for $`R + \alpha - \rho`$, with exact final roots. Its union peak equals the cold inventory for $`R + \alpha`$. Every valid authorized trial prepares without limits, and both complementary schedules prepare the same ledger. | `checked_trial_refines_cursor_steps_and_matches_cold_oracle` |
| LT-05 | The union peak and the final inventory are separate domains, and the peak is never below the final inventory. The union-on-final-limit mutant refuses exactly when the peak exceeds the final inventory, on the exceeded union axis, and a lowered final limit refuses on its own axis. | `budget_domains_are_not_conflated` |
| LT-06 | Live scratch never exceeds its reserved peak, which never exceeds the limit, and each phase start charges one reference per root occurrence. Without initialization charges, references fall short by exactly $`\lvert\alpha\rvert + \lvert\rho\rvert`$, the activation start's live scratch exceeds its reservation exactly when $`\alpha`$ is non-empty, and on a fixture the uncharged variant admits live scratch above the limit. | `budget_domains_are_not_conflated` |
| LT-07 | Scratch is a live peak, not cumulative allocation: the expansions-as-scratch mutant refuses exactly when expansions exceed the scratch peak. | `budget_domains_are_not_conflated` |
| LT-08 | Expansion charges are exact: their count and their quoted and consumed bytes are sums over the vertices whose sign changes. With sound quotes, consumed $`\le`$ quoted $`\le`$ limit under the generated limits of LT-10. An unsound quote is admitted under a zero quote limit with consumption above every tested excess, up to `u64::MAX - 1`. | `external_quotes_bound_consumption_only_when_sound` |
| LT-09 | Exact measured limits accept with identical evidence, and one below any positive axis refuses on exactly that axis. | `checked_trial_budget_boundaries_are_exact_and_atomic` |
| LT-10 | Under generated limits within two units of the measured evidence on every axis, the checked trial prepares exactly when the limits cover the unbounded evidence, and then with identical evidence. A refused trial retried from its retained predecessor with covering limits prepares the unbounded evidence. | `refused_trial_retry_matches_unbounded_evidence` |
| LT-11 | Partial commit retains the in-flight ledger at every generated refusal and changes the predecessor on some of them. Whenever an activation-only trial transfers a contribution, skipped contributions always prepare a non-quiescent ledger and duplicated ones never prepare a quiescent one. | `trial_mutants_violate_atomicity_or_cold_oracle` |

`model_property_manifest_is_unique_and_exhaustive` checks these eleven
bindings. It reads `IncrementalLedgerTrial.v` as part of its Rocq source,
requires each of the theory's twenty `Print Assumptions` capstones to be cited
by an LT row of the derivation matrix, and requires the integrated runner to
compile the theory.

**Boundary.** These results hold for the abstract trial, its Verus counters and
its test model. They do not show that the production successor runs this
trial, that its copy-on-write ledger or trusted successor refines it, that
object bytes are authenticated, or that quoted read sizes are sound. Quote
soundness is a premise of theorem 13, and theorem 14 shows that it cannot be
dropped. Peak scratch counts events and frames, not their byte sizes.
The models bound each phase by the ghost potential of the previous section,
which is proof instrumentation, not a production counter; theorem 10 shows that
valid input drains before that bound is reached. Exact rooted support still
requires an acyclic graph.

The trial evidence in the verification record was taken at these sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/IncrementalLedgerTrial.v` | `4545a7d762bf5e9b70ace4576305f4f2aad1f328e15c50c0f073824fabdc3258` |
| `formal/verus/incremental_prepare_streaming.rs` | `b5a54606e927a98c6408b8c5210f4842b75128c5bc7857d23aabebbb74b5d388` |
| `scripts/generate-ledger-trial-verus-mutant.py` | `d9b277da4353ff311e51df4fe3ed202ffd7ddfdcc9a3d82545085f072fbdad7e` |
| `scripts/check-stack-safe-pda-proofs.sh` | `3b482473d67814358b3dbc4449aa52c098fff4fd7551dcdb1f1f0090143ab677` |
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `b7dd63b1d5c2adc6fb6b3d59cc0c690e2c4d506cdf21be772fca8581566be307` |
| `rspace++/tests/persistence/property_support.rs` | `5a3d0c00ce1c4afb1c547bf7164198a0e7533e08b49f754bdd6cd4eb95bcd7c3` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `7b6f0504278b474155ab53f86ef8c4f4677c4a33d9489d0f099a5cd0f99733fb` |

### Current, retained and metadata inventory composition (2026-10-25)

Task 7922 composes the single-domain trial of the previous section into the
three inventory domains that a durable successor keeps:

- the *current* domain roots the closure of the current retention entry's
  manifest;
- the *retained* domain roots the union of the closures of every retention
  entry, historical and current;
- the *metadata* domain roots the separately authenticated retention metadata,
  over its own object graph.

Two artifacts implement the composition:

- the Rocq theory
  `formal/rocq/stack_safe_pda/theories/IncrementalDomainComposition.v` (module
  `CheckedDomainComposition`, built on `CheckedLedgerTrial`);
- the executable test model in
  `rspace++/tests/persistence/incremental_reachability_properties.rs`.

The composition adds no arithmetic and no concurrency: every domain runs the
unchanged trial, whose counters the Verus model of the previous section already
covers, so it needs no new Verus or TLA+ model. Neither artifact is the
production successor. Task 7924 replaces the production ledger's per-object
union bit with per-domain counts, and task 7927 builds the trusted successor.

![A composed trial: a retention step derives one signed root delta per domain; the current, retained and metadata trials run in order; the first refusal keeps all three predecessors; pins reach only physical protection](figures/domain-composition.svg)

**Retention entries and root counters.** A retention entry $`e = (g, m)`$
pairs a generation number $`g`$ with a manifest root $`m`$, written $`m(e)`$.
The entries $`E = (e_c, H)`$ are the current entry $`e_c`$ and the list $`H`$
of historical entries. For a list $`\ell`$ of vertices,
$`\mathrm{occ}(\ell)(v)`$ counts the occurrences of $`v`$ in $`\ell`$, and
$`m(H)`$ is the list of the manifests of $`H`$. The entries determine two root
counters on the semantic graph:

```math
R_L(E) = \mathrm{occ}\bigl([\,m(e_c)\,]\bigr), \qquad
R_U(E) = \mathrm{occ}\bigl(m(H) \mathbin{+\!\!+} [\,m(e_c)\,]\bigr),
```

where $`\mathbin{+\!\!+}`$ concatenates lists. Two entries with the same
manifest are *root aliases*: they contribute two occurrences to $`R_U`$, not
one. The metadata domain has its own root counter $`R_M`$ over the metadata
graph. Pins form a fourth counter $`P`$ over semantic objects, but not a fourth
ledger: no trial ever reads it.

**Signed root deltas.** A root delta $`\delta = (\alpha, \rho)`$ lists added
and removed root occurrences, the inputs of one trial. Applying it to a root
counter $`R`$ gives $`R + \mathrm{occ}(\alpha) - \mathrm{occ}(\rho)`$. It is
*authorized* when $`\mathrm{occ}(\rho)(v) \le R(v) + \mathrm{occ}(\alpha)(v)`$
for every $`v`$, which is the trial's own first check. A retention step derives
one delta per domain:

| Step | Precondition | Current $`\delta_L`$ | Retained $`\delta_U`$ | Metadata $`\delta_M`$ | Pins $`\delta_P`$ |
|---|---|---|---|---|---|
| Commit $`t`$ | none | add $`m(t)`$, remove $`m(e_c)`$ | add $`m(t)`$ | an authorized change | none |
| Evict $`v`$ | $`v`$ is in $`H`$ | none | remove $`m(v)`$ | an authorized change | none |
| Pin $`p`$ | none | none | none | none | add $`p`$ |
| Unpin $`p`$ | $`P(p) > 0`$ | none | none | none | remove $`p`$ |

A commit makes $`t`$ current and appends $`e_c`$ to $`H`$. An eviction removes
the first entry of $`H`$ equal to $`v`$. The current delta removes the old
current manifest, but the retained delta does not, because the old entry stays
retained. The test model's metadata change replaces the single metadata root on
every commit and eviction.

Production today only commits. `bind_durable_transaction` appends the previous
current state to the historical list and makes the target current
(`rspace++/src/rspace/persistence/durable/admission.rs:1479-1483`), so retention
is keep-all. Eviction is modeled anyway, because removal is the only operation
under which counting root aliases matters: with commits alone the retained roots
only grow, and a set of roots then has the same support as a multiset.

**The composed trial, in literate form.** Write $`\pi_L`$, $`\pi_U`$ and
$`\pi_M`$ for the three domain predecessors, each a predecessor
$`(\Lambda, R, O, B)`$ of the previous section, and $`L_L`$, $`L_U`$ and
$`L_M`$ for three separate limit vectors. Every refusal in parts 1–3 returns the
composed predecessor $`(\pi_L, \pi_U, \pi_M)`$ unchanged, together with the
refusing domain and its reason $`w`$; no later domain runs.

1. ⟨Current trial⟩. Run the trial over the semantic graph from $`\pi_L`$ with
   $`\delta_L`$ under $`L_L`$, or return
   $`\mathrm{Refused}(\mathrm{Current}, w)`$.
2. ⟨Retained trial⟩. Run the trial over the semantic graph from $`\pi_U`$ with
   $`\delta_U`$ under $`L_U`$, or return
   $`\mathrm{Refused}(\mathrm{Retained}, w)`$.
3. ⟨Metadata trial⟩. Run the trial over the metadata graph from $`\pi_M`$ with
   $`\delta_M`$ under $`L_M`$, or return
   $`\mathrm{Refused}(\mathrm{Metadata}, w)`$.
4. ⟨Prepare⟩. Return the three prepared evidences. The successor base is the
   triple of the predecessors they carry.

The pin delta is an input to no part. An object is *physically protected* when
it is active in the retained or the metadata ledger, or pinned:

```math
\mathrm{protected}(v) \iff A_U(v) \lor A_M(v) \lor P(v) > 0 ,
```

where $`A_U`$ and $`A_M`$ are the active bits of the retained and metadata
ledgers.

**Premises.** A composed base is *valid* for entries $`E`$ and metadata roots
$`R_M`$ when each domain's ledger is coherent, its roots equal $`R_L(E)`$,
$`R_U(E)`$ or $`R_M`$ respectively, it is quiescent for those roots, and its
inventories are exact. The theorems that compare prepared ledgers with cold
ledgers assume, as the single-domain theorems do, that each graph is ranked:
every edge descends a rank, so the quiescent ledger for given roots is unique.
The data-inventory theorem also assumes that both graphs are closed over their
universes, that each domain's roots lie in its own universe, and that the
semantic universe $`S`$ and the metadata universe $`M`$ are disjoint lists
without repeats, written $`\mathrm{NoDup}(S \mathbin{+\!\!+} M)`$. Production
enforces the disjointness where the two inventories meet:
`try_visit_checkpoint_references` refuses with `DuplicateInventoryDigest` when
the retention manifest's digest is also in the retained union
(`rspace++/src/rspace/persistence/checkpoint.rs:306-340`), and
`existing_digest_is_rejected` proves that the ordered inventory rejects any
digest it already holds.

**Theorems.** The module proves eleven capstones. Each has a
`Print Assumptions` line, every one reports a closed context, and the compiled
module passes independent kernel checking.

| # | Theorem (`CheckedDomainComposition.`…) | Statement |
|---|---|---|
| 1 | `composed_trial_is_the_conjunction_of_domain_trials` | For every variant, the composed trial keeps the composed predecessor; its outcome is the first-refusal combination of the three domain outcomes; and it prepares $`e`$ exactly when each domain trial prepares its component of $`e`$ from its own predecessor. |
| 2 | `composed_trial_agrees_with_cold_domains` | On ranked graphs, with valid trial inputs, each prepared domain equals any coherent quiescent ledger for that domain's applied roots, with exact roots and inventories. |
| 3 | `canonical_retention_deltas_realize_entry_roots` | For every admissible step from $`E`$ to $`E'`$, the canonical current and retained deltas are authorized, and applying them to $`R_L(E)`$ and $`R_U(E)`$ gives exactly $`R_L(E')`$ and $`R_U(E')`$. |
| 4 | `current_closure_is_within_retained_union` | On a ranked graph, the cold ledgers for $`R_L(E)`$ and $`R_U(E)`$ satisfy $`A_L \subseteq A_U`$, and for every weight the retained inventory is the current inventory plus the weight of the retained-only objects. |
| 5 | `evicting_one_root_alias_keeps_retained_support` | If the evicted entry's manifest is still the manifest of a remaining entry, the cold retained ledgers before and after the eviction have the same active set and the same inventory for every weight. |
| 6 | `data_inventory_is_disjoint_retained_plus_metadata` | Under the premises above, no object is active in both the retained and the metadata ledger, and the data inventory over $`S \mathbin{+\!\!+} M`$ equals the retained inventory plus the metadata inventory. |
| 7 | `pins_protect_physically_without_changing_semantic_evidence` | Replacing the pin delta never changes the faithful composed result, and $`\mathrm{protected}(v)`$ holds exactly when $`A_U(v)`$, $`A_M(v)`$ or $`P(v) > 0`$ holds. |
| 8 | `canonical_composed_step_preserves_entry_domains` | From a base valid for $`E`$ and $`R_M`$, a prepared canonical step yields a successor base valid for $`E'`$ and the applied metadata roots, whose current and retained ledgers satisfy theorem 4. Whole histories follow by induction on the steps, since theorem 1 keeps the base of a refused step. |
| 9 | `retained_sign_mutant_drops_the_predecessor_closure` | Negative control (sign): giving the retained domain the current delta drops the old manifest's closure. |
| 10 | `deduplicated_alias_mutant_drops_a_shared_closure` | Negative control (alias): adding a retained root only when it is absent loses a shared closure once one alias is evicted. |
| 11 | `pinned_root_mutant_inflates_the_retained_union` | Negative control (pin): treating pins as retained roots adds the pinned object to the retained union, while the faithful result protects it without retaining it. |

Theorems 9–11 are closed computations on one fixture: a semantic graph of four
objects, numbered 0 to 3, with the single edge from 1 to 0 and unit weights, and
a metadata graph of one object. Each variant changes only the retained delta:

| Variant | Retained delta it uses | Fixture history | Retained objects, faithful | Retained objects, mutant |
|---|---|---|---|---|
| `CurrentDeltaOnRetained` | the current delta | entry $`(0, 1)`$ current; commit $`(1, 2)`$ | 3 | 1 |
| `DeduplicatedRetainedRoots` | additions only for manifests not yet rooted | entry $`(0, 1)`$ current; commit $`(1, 1)`$; evict $`(0, 1)`$ | 2 | 0 |
| `PinsAsRetainedRoots` | the retained delta plus the pin delta | entry $`(0, 2)`$ current; pin 3 | 1 | 2 |

**Executable bindings.** The Rust test model mirrors the Rocq definitions one
for one: `RetentionEntries`, `HistoryState::canonical_delta`,
`retained_delta_of` for the variants, `ComposedCase::run` (the nested
first-refusal composition), `composed_outcome_spec` and `physically_protected`.
Each domain trial is the `TrialCase` of the previous section. The oracles are
independent of the trial: `composed_base` computes each domain's cold ledger by
traversal from the entry-derived roots, and `joint_data_environment` builds one
data graph with the metadata objects numbered after the semantic ones, so the
two universes are disjoint by construction. Each generated history has:

- a semantic DAG of one to eight objects, each with up to three children of
  lower index, duplicates kept, and byte weights from 0 to 4;
- a metadata DAG of one to four objects, each with up to two children of lower
  index, and byte weights from 1 to 4;
- zero read quotes, since quote soundness is the trial's concern (LT-08);
- an initial entry and metadata root, then one to three steps drawn with weights
  3 for a commit, 2 for a commit that reuses an existing manifest, 2 for an
  eviction, 1 for a pin and 1 for an unpin; an eviction or unpin with nothing to
  remove is skipped;
- a schedule of one to eight bits.

A history therefore holds at most four retention entries, within the generator
discipline below. Each property runs 256 cases. The history property counts
commits, alias commits, evictions, alias evictions, strict retained unions and
pins outside the retained support, and fails unless every count is positive.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| DC-01 | A composed trial keeps the composed predecessor, and for every variant its outcome is the first-refusal combination of the three domain outcomes, each domain trial keeping its own predecessor. Each domain has its own limits: exact measured limits accept with identical evidence, one below a positive axis of one domain refuses as that domain on that axis, and when two domains are tightened the earlier one refuses. | `composed_domain_budgets_are_separate_and_atomic` |
| DC-02 | Every canonical step of a generated history prepares without limits, and its successor base equals, domain by domain, the cold ledger, roots and inventories for the successor entries' roots and metadata root. The complementary schedule prepares the same successor base. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-03 | The canonical current and retained deltas of every generated step are authorized and realize exactly the successor entries' root counters. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-04 | The current support lies within the retained support, and the retained inventory is the current inventory plus the objects and bytes of the retained-only objects; some generated step has a strict difference. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-05 | An eviction leaves the current domain bit-identical. Evicting one of several entries that share a manifest keeps the retained active set and inventory, although the alias's root count falls. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-06 | On the joint data graph, the cold ledger for the retained and metadata roots, restricted to each part, equals that domain's counts and active set, and the joint inventory is the retained inventory plus the metadata inventory, so no object is counted twice. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-07 | Replacing a step's pin delta leaves the faithful composed result identical, pin and unpin steps leave all three domains unchanged, and an object is physically protected exactly when the joint cold ledger has it active or it is pinned; some generated pin protects an object outside the retained support. | `composed_domain_histories_match_independent_cold_oracles` |
| DC-08 | Fixtures reproduce theorems 9–11 value for value. On generated histories, with each variant continuing from its own successor bases, the faithful composition never diverges from the cold retained union, and each mutant diverges on some history, by refusing or by a different retained active set or inventory. | `domain_mutants_violate_the_cold_retained_union` |

`model_property_manifest_is_unique_and_exhaustive` checks these eight bindings.
It reads `IncrementalDomainComposition.v` as part of its Rocq source, requires
each of the theory's eleven `Print Assumptions` capstones to be cited by a DC
row of the derivation matrix, and requires the integrated runner to compile the
theory.

**Boundary.** These results hold for the abstract composition and its test
model. They do not show that the production successor maintains these domains.
The production closure ledger, `StructuralClosureLedgerV1`, keeps one
`retained_union_member` bit per digest
(`rspace++/src/rspace/persistence/closure.rs:182-194`). A bit records membership
in the union, but it cannot be decremented: when one of two entries that share
an object is removed, the bit alone cannot tell whether another entry still
reaches the object, which is exactly the alias mutant's failure. Task 7924
replaces the bit with per-domain checked counts, which must refine this model.
Path and namespace rewrite provenance, which ends the closure walk at an
occurrence exactly when the provenance set names its namespace, kind, path and
reference, checked before the staging overlay (`closure.rs:971-989`; the
path-exact incremental closure section, PX-01 to PX-05), and full-byte
collision checks belong to object admission: the model's vertices are already
authenticated digests. The model's
pin protects the pinned object itself; which closure a production pin protects,
and how leases and the WAL frontier interact with it, is the garbage
collector's contract (tasks 7943 to 7945). Reset to an older root alias is task
7756. Exact rooted support still requires an acyclic graph.

The composition evidence in the verification record was taken at these sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/IncrementalDomainComposition.v` | `5f9701702c1d9e400119c50b8859d9a1d721f792608f8a7e15b3823fbc733d9a` |
| `formal/rocq/stack_safe_pda/_CoqProject` | `c928e89709719aed7e9e2b917571bad39442d7c3b1b9739bc868b700a378b88e` |
| `scripts/check-stack-safe-pda-proofs.sh` | `44f8ef6185f33ac7a481c31872ee2c0bd56f342cf658845d3a085b331ac4f0d2` |
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `fa8288d65ea8032e0e9413e132aef9d2df2f4cbccde5b32e1e317ef238c8f0df` |
| `rspace++/tests/persistence/property_support.rs` | `1c72135f5fb4d2071fe31f1e76f18ad98725c2eae895fe6546bc7087551f41e2` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `7d1cf5076d3b54a2523682a4ee664c8beb445ef8cc8cda19d06607bd231c2254` |

### Copy-on-write reachability ledger (2026-10-26)

Task 7924 carries the three preceding models into production. Until now the
production closure ledger, `StructuralClosureLedgerV1`, kept one
`retained_union_member` bit per digest. The boundary of the previous section
explains why a bit is not enough: it cannot be decremented, so the ledger cannot
tell whether an object stays reachable after one of the states that reach it is
removed. The task makes four changes:

1. It replaces the bit with checked reference counts in two domains, *current*
   and *retained*, together with root multiplicities and exact per-domain
   inventories.
2. It derives those counts during the cold admission walk that already runs.
   The walk reads no additional object, so the admission read budget is
   unchanged.
3. It adds the checked add-before-remove trial of the 7921 section as
   production code, running on a private copy-on-write clone of the ledger.
4. It composes a current and a retained trial with first-refusal semantics, as
   the 7922 section does for three domains.

Four artifacts implement the task:

- the Rocq theory `formal/rocq/stack_safe_pda/theories/CreditedClosureLedger.v`
  (module `CreditedClosureLedger`, built on `IncrementalReachability`,
  `CheckedReachability` and `CheckedDomainComposition`), which proves that the
  counts the walk derives are the cold ledgers of the composition model;
- `rspace++/src/rspace/persistence/closure.rs`, which holds the counted ledger
  and the credited walk;
- `rspace++/src/rspace/persistence/closure/ledger_trial.rs`, which holds the
  trial engine and the two-domain composition;
- the properties in
  `rspace++/tests/persistence/cow_reachability_ledger_properties.rs`, which build
  real canonical objects and compare the production ledger with independent cold
  traversals and with the test models of the two preceding sections.

The trial engine's production caller is the trusted bind of task 11945 (the
trusted successor bind section), whose successor task 11946 publishes. Like the
rest of the persistence substrate, the engine is compiled in every build and
exercised only by tests until task 7931 wires the substrate into RSpace, and it
no longer carries a dead-code expectation of its own.
The metadata domain remains with tasks 7925 and 7926. The ledger itself is never
serialized. Only its retained-union report and the digest-ordered enumeration of
that union reach persisted bytes, through the catalog pack state
(`rspace++/src/rspace/persistence/durable/publication.rs:880-905`) and the
checkpoint descriptor (`rspace++/src/rspace/persistence/checkpoint.rs:188-215`),
and both are unchanged.

![One credited entry of the closure walk: the entry counts its credit with checked increments and records which domains crossed from zero; a memoized entry only stores its counts, and refuses if anything crossed; otherwise the cycle check, inventory growth at the crossings, the bounded load, and an expansion that credits each outgoing reference with exactly the crossed domains](figures/credited-closure-enter.svg)

**The counted ledger.** Every component of the model has one production
counterpart:

| Model component | Production counterpart |
|---|---|
| a vertex | an object digest, a key of `states: PathMap<DigestState>` |
| the children of a vertex, duplicates kept | the outgoing references of the parsed object: a semantic manifest's data-bag, continuation-map and join-bag roots, in that order; a DAG node's content endpoint, if it has one, then its children in edge order; nothing for a blob |
| the reference count $`C_d(v)`$ in domain $`d`$ | `DigestState::counts[d]`, a `u64` |
| the active bit $`A_d(v)`$ | not stored: it is `counts[d] > 0`, so every ledger is coherent by construction |
| the root multiplicity $`R_d(v)`$ | `roots: PathMap<[u64; 2]>`, keyed by digest |
| the inventories $`(O_d, B_d)`$ | `inventories[d]`: the number of active objects and the sum of their declared canonical lengths |
| the byte weight $`w(v)`$, quoted read $`q(v)`$ and consumed read $`r(v)`$ | all three are the reference's declared canonical length; `load_reference` refuses an object whose length or digest differs, so quotes are sound by construction |

`ClosureDomainV1` names the two domains, and a `ClosureCreditV1` is a set of
them. The retained-union report is `inventories[Retained]`, and the
digest-ordered enumeration visits exactly the digests whose retained count is
positive.

**Credited cold construction.** The structural verifier visits one state
manifest per *epoch*. Its memo is keyed by digest and expectation and is cleared
at an object's first visit in each epoch, so every epoch walks the whole closure
of its manifest again. A count therefore cannot be incremented at every visit:
an object shared by two states would be counted once per state, not once per
active parent. Instead every work item carries a *credit* $`c`$, the set of
domains in which it counts. An epoch for manifest $`m`$ with caller-chosen
credit $`D`$ runs as follows. Every refusal aborts the walk, and every caller
then discards its ledger.

0. ⟨Credit the root⟩. Increment $`R_d(m)`$ for each $`d \in D`$, refusing
   `CounterOverflow` above `u64::MAX`, and push the entry of $`m`$ with credit
   $`D`$.

Each entry of a reference $`r`$ with credit $`c`$ then takes the following
steps:

1. ⟨Admit the visit⟩. As before: on the epoch's first visit of $`r`$, the
   distinct-object and byte limits; then the declared-length check.
2. ⟨Count the credit⟩. For each $`d \in c`$, increment $`C_d(r)`$, refusing
   `CounterOverflow`. The *crossing set* $`\kappa`$ holds each $`d \in c`$ whose
   count was zero before its increment.
3. ⟨Memoized entry⟩. If the entry's expectation is already verified for $`r`$
   in this epoch, store the counts and stop when $`\kappa = \varnothing`$, and
   otherwise refuse `UnexpandedClosureCrossing`: the children of such a crossing
   would never be credited.
4. ⟨Cycle check⟩. As before.
5. ⟨Grow the inventories⟩. If $`\mathrm{Retained} \in \kappa`$, check the union
   limits and grow the retained inventory, at the point where the old walk grew
   the union. If $`\mathrm{Current} \in \kappa`$, grow the current inventory.
6. ⟨Load and expand⟩. Load, parse and validate the object as before, then push
   one entry for each outgoing reference occurrence, each with credit
   $`\kappa`$.

The callers choose the credit:

| Caller | Credit $`D`$ | Result |
|---|---|---|
| cold admission, the current state (`admission.rs`) | current and retained | both domains, built from the empty ledger |
| cold admission, each historical state | retained | the retained domain gains the state's closure |
| `successor_inventory` | retained | the successor's retained union, on a private clone discarded after its report |
| `verify_structural_closure_in_ledger_with_limits` (semantic admission and existing tests) | retained | the union semantics of the old bit |
| `verify_structural_closure_with_limits` | retained, on a fresh ledger that is then discarded | nothing observable |

**Why no crossing is left unexpanded.** The refusal in step 3 is a guard. On
the ledgers that the callers supply it never fires, so the walk derives exactly
the credited epoch of the theory. The argument below is not mechanized, and the
CL properties test it.

*Lemma.* Let an epoch with credit $`D`$ start from a ledger that is quiescent,
for each $`d \in D`$, for its roots $`R_d`$ over the graph of outgoing
references, and suppose the walk succeeds. If $`C_d(x) = 0`$ at the start of
the epoch for some $`d \in D`$, then the first entry of $`x`$ in the epoch, if
there is one, carries $`d`$.

*Proof.* By strong induction on the order in which entries leave the work
stack. Let $`e`$ be the first entry of $`x`$ in the epoch.

- If $`e`$ is the root entry, it carries $`D`$, which contains $`d`$.
- Otherwise the expansion of an entry $`e_y`$ of some object $`y`$ pushed
  $`e`$, so $`x`$ is an outgoing reference of $`y`$ and $`e_y`$ left the stack
  before $`e`$. Let $`f_y`$ be the first entry of $`y`$ in the epoch, which left
  the stack no later than $`e_y`$.
  - Suppose $`C_d(y) > 0`$ at the start of the epoch. The starting ledger is
    quiescent for $`R_d`$ and $`y`$ is active and references $`x`$, so
    $`C_d(x) = R_d(x) + I(A_d, x) \ge 1`$, which contradicts $`C_d(x) = 0`$.
  - So $`C_d(y) = 0`$ at the start of the epoch. By the induction hypothesis
    $`f_y`$ carries $`d`$, so $`d`$ crosses at $`f_y`$. The first visit of a
    digest in an epoch clears its verified expectations and its active mark, so
    $`f_y`$ expands, and it pushes entries of $`x`$ that carry $`d`$ above the
    exit item of $`y`$.
    - If $`e_y = f_y`$, then $`e`$ is one of those entries and carries $`d`$.
    - If $`e_y \ne f_y`$, then $`e_y`$ was either already below the exit item of
      $`y`$ when $`f_y`$ expanded, or pushed after that exit item left the
      stack, or pushed while $`y`$ was still active. In the last case a
      descendant of $`y`$ pushed it, so popping it finds $`y`$ active and
      refuses `ObjectCycle`, which contradicts the success of the walk. In the
      first two cases an entry of $`x`$ pushed by $`f_y`$ leaves the stack before
      $`e_y`$, hence before $`e`$, which contradicts the choice of $`e`$. ∎

*Corollary.* If $`d`$ crosses at an entry of $`x`$, that entry is the first
entry of $`x`$ in the epoch, and it expands. A crossing needs $`C_d(x) = 0`$
just before the entry. Counts never decrease during a walk, so no earlier entry
of $`x`$ carried $`d`$. By the lemma the first entry carries $`d`$, so it is the
crossing entry, and a first entry always expands. Hence the guard of step 3
never fires.

*Consequence.* Each object that crosses $`d`$ in the epoch crosses exactly once,
at its first entry, whose expansion credits $`d`$ to each of its outgoing
reference occurrences. Every later expansion of the same object, under another
expectation, credits nothing in $`d`$, and every pushed entry leaves the stack
before a successful walk returns. So for every $`d \in D`$ the counts after the
epoch satisfy

```math
C'_d(v) = C_d(v) + \mathrm{occ}\bigl([\,m\,]\bigr)(v) + I(\kappa_d, v),
\qquad
\kappa_d = \{\, y \mid C_d(y) = 0 \land C'_d(y) > 0 \,\},
```

which is the theory's `credited_epoch` with the added roots $`[\,m\,]`$. Counts in
a domain outside $`D`$ do not change. Cold admission starts from the empty
ledger, which is quiescent for zero roots, and theorem 1 below supplies the
premise of each later epoch. The successor-inventory walk starts from a clone of
a cold-built ledger.

The union also grows exactly where the old bit grew it. When the retained domain
is in every epoch's credit, an object's retained count is zero exactly when its
old bit was clear. By the corollary a retained crossing happens at the first
visit of an object whose count was zero, which is where the old walk grew the
union, and by the lemma every such first visit crosses. The union therefore
grows at the same entries, in the same order and under the same limits, and
reports the same objects and bytes.

**Theorems.** The module proves eight capstones. Each has a `Print Assumptions`
line, every one reports a closed context, and the compiled module passes
independent kernel checking. Its credited epoch relates two coherent ledgers:
the counts after it are the counts before it, plus the added root occurrences,
plus one for each outgoing reference occurrence of a crossed object.

| # | Theorem (`CreditedClosureLedger.`…) | Statement |
|---|---|---|
| 1 | `credited_epoch_is_quiescent_for_added_roots` | If a coherent ledger is quiescent for roots $`R`$ and a coherent ledger follows it by a credited epoch for added roots $`\alpha`$, the second ledger is quiescent for $`R + \mathrm{occ}(\alpha)`$. |
| 2 | `credited_epoch_agrees_with_cold_ledger` | On a ranked graph, under the premises of theorem 1, the credited ledger equals any coherent quiescent ledger for $`R + \mathrm{occ}(\alpha)`$, count for count and bit for bit. |
| 3 | `credited_epoch_inventory_grows_by_crossed_objects` | For every weight, the inventory after a credited epoch is the inventory before it plus the weight of the crossed objects. |
| 4 | `credited_crossings_are_newly_reachable_objects` | On a ranked graph, under the premises of theorem 1, an object crosses exactly when it is reachable from the added roots and was inactive before. |
| 5 | `credited_build_is_quiescent` | A sequence of credited epochs from the empty ledger yields a coherent ledger that is quiescent for the occurrences of all the epochs' roots. |
| 6 | `credited_cold_build_realizes_entry_domains` | The current build, one epoch for $`m(e_c)`$, is coherent and quiescent for $`R_L(E)`$. The retained build, one epoch for $`m(e_c)`$ and then one for each historical manifest in order, is coherent and quiescent for $`R_U(E)`$. |
| 7 | `epoch_wide_credit_mutant_double_counts_a_shared_child` | Negative control: crediting the children of every object an epoch visits, not only of the crossed ones, counts a shared child twice. |
| 8 | `unexpanded_crossing_mutant_leaves_an_uncredited_child` | Negative control: a crossing that credits no children leaves the child of an active parent uncounted. |

Theorems 7 and 8 are closed computations on one fixture: four objects numbered 0
to 3 and the chain of edges $`3 \to 2 \to 1 \to 0`$. The required count is the
root count plus the incoming count from the variant's own active set, which is
the quiescence equation each control violates. In both rows the faithful counts
satisfy that equation at every object.

| Control | Starting ledger | Added root | Faithful count | Control's count | Required count |
|---|---|---|---|---|---|
| epoch-wide crediting | quiescent for root 1: $`C(1) = C(0) = 1`$ | 2 | 1 at object 0 | 2 at object 0 | 1 at object 0 |
| unexpanded crossing | empty | 3 | 1 at object 2 | 0 at object 2 | 1 at object 2 |

**The trial engine, in literate form.** `run_ledger_trial` transliterates the
faithful `run_trial` of `CheckedLedgerTrial` for one domain $`d`$, under the
schedule that always prefers an event to a cursor. It borrows the predecessor
ledger immutably and returns either a prepared successor or a typed refusal, so
a refusal cannot change the predecessor: the borrow enforces theorem 4 of the
trial section. Its private state starts as a clone of `states` and of `roots`.
Each clone is a reference-count increment, so the trial copies only the trie
paths it changes. Every refusal in parts 1–6 drops that private state.

1. ⟨Authorize the removals⟩. On the cloned roots, add one occurrence of each
   added root, refusing `CounterOverflow` above `u64::MAX`, then remove one
   occurrence of each removed root, refusing `UnauthorizedRemoval` at zero. This
   is the model's check against $`R + \mathrm{occ}_\alpha`$, and it precedes all
   cursor work.
2. ⟨Start activation⟩. The events are the added roots in order, and there are
   no cursors. Reserve the model's start charge, or refuse
   `Budget(axis)`.
3. ⟨Drain activation⟩. Repeat one checked step until no event and no cursor
   remains:
   - if an event remains, take the last one, $`v`$. Charge the increment of
     $`C_d(v)`$, where an absent digest counts zero and a declared length that
     differs from the stored one is refused. Reserve all seven step axes, which
     records the new usage. On a zero crossing, load and parse $`v`$ and push a
     cursor over its outgoing references. Then pop the event and store the count
     and the inventory;
   - otherwise, if the top cursor has a reference left, transfer it to the
     events, charging one reference and one unit of live scratch;
   - otherwise discard the exhausted top cursor.
4. ⟨Start deactivation⟩ and 5. ⟨Drain deactivation⟩. As in parts 2 and 3, with
   the removed roots and decrements. A zero count is refused
   `ZeroCountDecrement`, and an inventory underflow `InventoryUnderflow`.
6. ⟨Final guard⟩. Refuse `Budget(FinalObjects)`, then `Budget(FinalBytes)`.
7. ⟨Prepare⟩. Return the successor ledger and the usage. The successor holds
   the changed `states` and `roots`, the other domain's counts, roots and
   inventory unchanged, and the drained inventory of domain $`d`$.

All cursors share one stack of references. A crossing appends the object's
outgoing references, each cursor records its start, next and end positions, and
discarding the top cursor truncates the stack to its start, which is the end of
the cursor below it. Nothing recurses, and live scratch counts events and
cursors, as in the model. An object is loaded only when it crosses, after the
step's reservation is accepted, and a failed load, parse or kind check is
refused as `Closure`. The model has no counterpart of the load or of the
declared-length check, because its vertices are already authenticated digests.
The engine keeps no potential counter: the ghost potential of the cursor model is
proof instrumentation, and its termination theorem bounds the number of steps.

**The composition.** `run_domain_trials` runs the current trial on the
predecessor and then the retained trial on the current trial's successor, and it
returns the first refusal together with its domain. Each trial reads and writes
only its own domain's counts, roots and inventory, so the retained trial gives
the same result on the current successor as on the predecessor, and the final
successor carries both domains. Each domain has its own limits. This refines
`run_composed` of `CheckedDomainComposition` with the metadata domain absent.
The deltas of a retention step come from the caller, as the delta table of the
7922 section specifies; task 7927 derives them.

**Executable bindings.** Each generated case builds real canonical objects in a
`CanonicalObjectStore`:

- one to four *multiplicity* nodes, each with a distinct multiplicity endpoint
  and up to three children drawn from the earlier multiplicity nodes, duplicates
  kept under distinct edge labels;
- zero to two *content* nodes, each with a content endpoint on the single
  continuation-source blob and up to two children drawn from the earlier
  content nodes;
- the empty node, which may appear only as a namespace root;
- one to four state manifests. Each takes its data-bag and join-bag roots from
  the empty node and the multiplicity nodes, and its continuation-map root from
  the empty node and the content nodes.

A multiplicity node may be the root of both bags and also a child, and the walk
then verifies it once per expectation in every epoch: these are the task's
path-sensitive objects. A case holds at most eight objects below its one to
four manifests. The *abstract
mirror* of a case lists its objects in creation order, with each object's
outgoing references as indices and its declared length as its weight, quoted
read and consumed read. The mirror drives the independent cold oracle (a
traversal from the entry-derived roots) and the test models of the two preceding
sections, and the production ledger is never its own oracle. A history adds one
to three steps after its initial entry, drawn with weight 3 for a commit of any
generated manifest, 2 for a commit of the manifest of an existing entry and 2
for an eviction; an eviction with no historical entry is skipped. Each generated
property runs 256 cases.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| CL-01 | The credited cold build of every generated entry list (the current state with both domains, then each historical state with the retained domain) equals the independent cold oracle domain by domain: every digest's count, every root multiplicity and both inventories. The guard of step 3 never fires. | `credited_cold_build_matches_independent_domain_oracles` |
| CL-02 | Credits change no observation of cold admission. Per-state reports equal the standalone verifier's. The retained-union report and its digest-ordered enumeration equal the set union of the per-state closures and equal those of a build that credits only the retained domain, which reads the same objects the same number of times. Under union limits one below the exact union, both builds refuse with the same error. | `credited_cold_build_matches_independent_domain_oracles` |
| CL-03 | For either domain, a generated trial prepares or refuses exactly as the `TrialCase` model does on the abstract mirror under the event-first schedule: the same refusal, or the same counts, roots, inventory and all eight usage quantities. A trial starts from a cold-built predecessor, adds up to four manifest occurrences, removes a subset of the old and added roots, and in some cases also removes an unrooted manifest. Generated limits lie within two units of the measured evidence on every axis. | `production_ledger_trial_refines_the_model_trial` |
| CL-04 | Every refused trial leaves the predecessor's observation identical, and a retry from it with covering limits prepares the unbounded evidence. An unauthorized removal is refused before any read, and an added manifest missing from the store is refused as a load failure; both keep the predecessor. | `production_ledger_trial_refines_the_model_trial` |
| CL-05 | Exact measured limits accept with identical evidence, and lowering one positive axis by one refuses on exactly that axis, for each domain trial and for each domain of the composition. When both domains are tightened, the current domain refuses. | `production_trial_limits_are_exact_per_axis_and_domain` |
| CL-06 | At every step of a generated history, the composed trial, run without limits from the previous step's successor, prepares a successor equal to the credited cold build for the new entries, domain by domain. The two have the same retained-union enumeration and the same checkpoint object inventory: its object count, its bytes and its ordered digest. Some history evicts one of two entries that share a manifest. | `composed_production_trials_match_cold_domains_and_checkpoint_inventory` |
| CL-07 | A commit's trials do the same cursor work, read the same objects and reach the same count and scratch peaks whether the predecessor holds only the current state or also up to eight unrelated historical states. Only the union and final inventories differ, by exactly the unrelated states' objects and bytes. | `production_trial_work_is_independent_of_unchanged_history` |
| CL-08 | On a thread with a 256 KiB stack, the credited cold build of a state whose data-bag root heads a 20,000-node chain succeeds, and so does a composed commit that replaces that state with one over a disjoint 20,000-node chain; both ledgers equal the cold oracle. | `twenty_thousand_level_chain_is_credited_and_traded_on_a_small_stack` |
| CL-09 | Fixtures reproduce theorems 7 and 8 value for value. On generated cases the differential detects every mutant. Each non-faithful `TrialVariant` of the trial model differs from the production trial on some case. The composition fed the `CurrentDeltaOnRetained` or the `DeduplicatedRetainedRoots` retained delta differs from the cold retained domain on some history. The epoch-wide and the unexpanded crediting rules, applied to the abstract mirror, differ from the cold oracle on some entry list. | `cold_oracle_differential_detects_credit_trial_and_domain_mutants` |

`model_property_manifest_is_unique_and_exhaustive` checks these nine bindings.
It reads `CreditedClosureLedger.v` as part of its Rocq source, requires each of
the theory's eight `Print Assumptions` capstones to be cited by a CL row of the
derivation matrix, and requires the integrated runner to compile the theory.

**Boundary.** These results show that production ledgers built by cold
admission hold the exact per-domain counts, roots and inventories of the model,
and that the production trial and composition compute the modeled successor
from them. They leave these questions open:

- The no-unexpanded-crossing lemma is argued, not mechanized. CL-01 and CL-02
  test it.
- Production runs one schedule, the event-first one. The model's results for
  every schedule are not re-tested in production.
- The trusted bind of task 11945 derives the deltas and runs the trials; the
  trusted publication that replaces cold re-admission is task 11946, its fault
  acceptance is task 7928 (the faulted trusted histories section), and the
  scaling measurement against geometrically growing history is the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29):
  thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history,
  while allocated bytes fail gate G4. CL-07 shows work independence on small
  cases only.
- A trial that removes an object's last reference in both domains leaves a
  zero entry in `states`. Every report, enumeration and inventory ignores it.
  Production retention is keep-all, so no production path creates one before
  eviction exists, and reclaiming such entries is the garbage collector's
  contract (tasks 7943 to 7945).
- The trial reads outgoing references only, and refuses an object of any other
  kind. Expectation validation (namespaces, endpoint policies and root
  normalization) stays with the structural verifier, which has admitted every
  object a trial can load.
- Exact rooted support still requires an acyclic graph. The verifier refuses
  cycles.

The production-ledger evidence in the verification record was taken at these
sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/CreditedClosureLedger.v` | `959ac7a2f796574537a64f6921c6b21a59302870769d5ed235c38e7806d311ba` |
| `formal/rocq/stack_safe_pda/_CoqProject` | `f0bda5edd3868c4f2572072ca2313104cd4888647bb600f8022a3896d14cfe53` |
| `scripts/check-stack-safe-pda-proofs.sh` | `00a051c6d0c89d034e154f088c17f2220ebe063e5df87645773e03134c57aedf` |
| `rspace++/src/rspace/persistence/closure.rs` | `f93416fdca113f0211bd8dd0972f638a8a38d0d6f4bee0d92cca5cf14a6950fb` |
| `rspace++/src/rspace/persistence/closure/ledger_trial.rs` | `43175d7046d6642edc9979248f541391c825a1e2696170d4c760e769bf393d0c` |
| `rspace++/src/rspace/persistence/canonical.rs` | `82cec4237febd1f5e10cf5b66b36a45835639f5f2a67794573d9f7eb22e4a7f2` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `dca108062c614d4f8224b156e0db78f45b464d50a0d0cb66ba4b5bd79ae928dc` |
| `rspace++/tests/persistence/cow_reachability_ledger_properties.rs` | `8397c7a4969057b30c13b91f4ff22e11a530cc256f8f13b96d17f2a9226de796` |
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `aa8518010c4c203a92e8da9b4738b95af2332e777f43ff46d2c6f60fd1d458a4` |
| `rspace++/tests/persistence/property_support.rs` | `c387935b91a6f045929302c167b0e63317c1d24a21e014e3eba3f6b6cdd154d2` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `99beab63dff1a686a6c8332e33330e18f685d68392f440f6151497f93213e193` |
| `rspace++/tests/persistence/mod.rs` | `10141ebee87e4e8a38ed53031256fd87dbfdec19138036e93a411cc1dce931f4` |

### Shared retention V2 format (2026-10-26)

Task 7923 freezes the shared retention format that engineering journal entry
J-063 proposed and J-066 constrained. It proves the format's properties and its
compatibility with readers that know only V1, and it extracts every property
into tests, before any code writes the format. The V1 retention manifest stores
the current retained state and the complete list of historical retained states
in one object, so every successor collects and re-encodes the whole history
(`bind_durable_transaction`, J-063). V2 keeps the same information but splits
it. A constant-size *anchor* carries the current entry and an exact count of the
historical entries, and a radix tree of content-addressed nodes carries the
history itself. A successor that changes one entry can then copy one
root-to-leaf path and share every other node with its predecessor, which is the
path copying of Driscoll et al. (1989). Task 7926 implements that editing. This
task changes no persisted byte. The production module that freezes the domains,
the key and the anchor is compiled into every build but has no production
caller, and it is marked as expected dead code outside test builds until task
7925 (the cold reader) and task 7926 (the writer) use it.

Five artifacts implement the task:

- the Rocq theory `formal/rocq/stack_safe_pda/theories/RetentionV2Format.v`
  (module `RetentionV2Format`, built on `ChannelImage` and on the
  `CheckedDomainComposition` model of the 7922 section), which proves the format
  properties;
- the TLA+ module `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2Upgrade.tla`,
  with its base configuration, four negative-control configurations and three
  non-vacuity configurations, which checks old-reader compatibility;
- `rspace++/src/rspace/persistence/retention/v2_format.rs`, which holds the
  frozen constants, `RetentionKeyV2` and `RetentionAnchorV2`, together with the
  anchor's refusal `RetentionHistoryRootMismatch` in
  `rspace++/src/rspace/persistence/canonical.rs`;
- the properties in
  `rspace++/tests/persistence/retention_v2_format_properties.rs`, which compare
  the production key and anchor with an independent test model of the radix
  tree;
- one checkpoint-record property in `rspace++/tests/persistence/checkpoint_wal.rs`
  and two catalog properties in `rspace++/tests/persistence/durable_catalog.rs`,
  which run the unchanged V1 codecs against V2 bytes.

![The objects of the shared retention V2 format. The catalog slot keeps its envelope unchanged, and its opaque retention reference at bytes 216 to 256 appoints either a V1 retention manifest or a 136-byte V2 anchor. The anchor carries the V1 current entry verbatim, the historical count n and the historical root, which is the 50-byte empty radix node when n is 0, a single leaf when n is 1, and an inner node otherwise. Inner nodes branch on edge bytes; a leaf's path and prefix spell a 48-byte key of big-endian generation, manifest digest and big-endian length, which decodes to the state identity that names a semantic state manifest](figures/retention-v2-format.svg)

**Vocabulary.**

- A *state identity* is a generation $`g`$ together with the `ObjectRefV1` of a
  semantic state manifest: the manifest's 32-byte BLAKE2b-256 digest $`d`$ and
  its encoded length $`\ell`$ in bytes. An encoded length includes the 32-byte
  object envelope, so $`\ell \ge 32`$.
- A *retained state entry* adds a byte claim $`c \ge \ell`$ to a state identity.
  V1 encodes an entry in 56 bytes: $`g`$ as a little-endian `u64`, the digest,
  $`\ell`$ as a little-endian `u64`, and $`c`$ as a little-endian `u64`.
- The *entry order* compares generations, then digests byte by byte, then
  encoded lengths. The V1 manifest requires its historical entries to increase
  strictly in this order and refuses `RetainedStatesNotStrict` otherwise. V1
  accepts two entries that share a generation, two that share a manifest at
  different generations, and two that share a generation and a digest with
  different lengths. Each such pair is an *alias pair*.
- Every canonical object begins with a 32-byte *envelope*: a 16-byte domain,
  then the format version 1 and the body length, each a little-endian `u64`.
- The *retained semantic union* $`R`$ is the set of objects reachable from the
  manifests of all retained states, current and historical, and
  $`\mathrm{bytes}(X)`$ is the sum of the encoded lengths of the objects in a set
  $`X`$.

**The retention key.** A V2 history is keyed by the identity of each entry:

```math
\kappa(g, d, \ell) \;=\; \mathrm{BE}_{64}(g) \,\Vert\, d \,\Vert\, \mathrm{BE}_{64}(\ell),
\qquad
\lvert \kappa(g, d, \ell) \rvert \;=\; 8 + 32 + 8 \;=\; 48,
```

where $`\mathrm{BE}_{64}`$ is the 8-byte big-endian encoding of an unsigned
integer and $`\Vert`$ is concatenation. A 48-byte string is a key exactly when
its last eight bytes decode to a length of at least 32. Two choices carry the
design:

- *Big-endian integers.* Lexicographic order on fixed-width big-endian strings
  is numeric order, so byte order on keys is the entry order (theorem 2), and a
  radix tree over keys lists its entries in V1 order. Little-endian integers
  break this: with $`\mathrm{LE}_{64}`$ the 8-byte little-endian encoding,
  $`\mathrm{LE}_{64}(256)`$ begins with the byte `00` and
  $`\mathrm{LE}_{64}(1)`$ with `01`, so generation 256 would sort before
  generation 1 (theorem 3).
- *All three fields.* Each kind of alias pair differs in exactly one field. A
  key of the generation alone, of the digest and length alone, or of the
  generation and digest alone would map some alias pair to one key (theorem 4),
  and the history would lose an entry.

The key is a bijection between valid identities and keys, so no surrogate
identifier is needed (theorem 1).

**The anchor.** The anchor has the domain `RSpaceDAG.RetA\0\0` and exactly 136
bytes:

| Bytes | Field | Encoding |
|---|---|---|
| 0..16 | domain `RSpaceDAG.RetA\0\0` | ASCII padded with NUL bytes |
| 16..24 | format version 1 | little-endian `u64` |
| 24..32 | body length 104 | little-endian `u64` |
| 32..88 | current entry | the 56-byte V1 entry, verbatim |
| 88..96 | historical count $`n`$ | little-endian `u64` |
| 96..136 | historical root | an `ObjectRefV1`: the 32-byte digest, then the length as a little-endian `u64` |

The count is zero exactly when the historical root is the empty radix node.
`RetentionAnchorV2::new` and `RetentionAnchorV2::parse` refuse every other
combination with `RetentionHistoryRootMismatch`. The current entry is not
re-encoded: bytes 32..88 of an anchor equal bytes 32..88 of the V1 manifest with
the same current entry.

**Radix nodes.** A radix node has the domain `RSpaceDAG.RetR\0\0`, followed by
the canonical DAG node body without any change: the prefix length as a
little-endian `u64`, the prefix bytes, the endpoint (a tag and its payload), the
branch tag (`Leaf` 0, `Node4` 1, `Node16` 2, `Node48` 3, `Node256` 4), the child
count as a little-endian `u64`, and the children, each an edge byte and an
`ObjectRefV1`. Reusing the body reuses the existing iterative radix editor,
staging store and validators, as J-063 requires. The body parser already refuses
the noncanonical shapes: an empty node with a prefix, an absent endpoint with
exactly one child, and edges that do not strictly increase. A one-child chain is
therefore always compressed into a prefix, as in Morrison's PATRICIA (1968), and
the branch encodings are the adaptive node sizes of Leis et al. (2013). The
domain must differ from the semantic node domain because the empty radix node
and the empty semantic node have the same body. Under one domain they would be
one object with one digest, and a checkpoint inventory would refuse the second
copy as a duplicate (theorem 7).

A radix tree with declared count $`n`$ is valid when it satisfies eight rules.
Each node is reached along a *path* $`\pi`$, the bytes spelled by the prefixes
and edges above it. Let $`\rho = 48 - \lvert \pi \rvert`$ be the number of key
bytes that remain at the node, and $`p`$ its prefix.

| Rule | Requirement | Refusal of the reference decoder |
|---|---|---|
| R1 | Every endpoint is `Absent` or `Multiplicity`; `ValueFree`, `Unit` and `Content` never occur. | `EndpointKind` |
| R2 | A `Multiplicity` endpoint occurs only on a node without children at full depth, $`\lvert p \rvert = \rho`$. Such a node is a *leaf*. | `EndpointDepth` |
| R3 | Every other node except an empty root is *inner*: its endpoint is `Absent`, $`\lvert p \rvert < \rho`$, it has at least two children with strictly increasing edges, and every child is a radix node. | `EndpointDepth`, `Body`, `NotRadixDomain` |
| R4 | The empty node, with an absent endpoint, no prefix and no children, occurs only as the root, and exactly when $`n = 0`$. | `EmptyBelowRoot`, `CountMismatch`, `RetentionHistoryRootMismatch` |
| R5 | The key $`\pi \Vert p`$ of a leaf decodes to an identity, and the leaf's claim is at least that identity's length $`\ell`$. | `LeafKey`, `LeafClaim` |
| R6 | One object may occur on several paths, because equal subtrees are one content-addressed object. A count is a function of the object and $`\rho`$, and is memoized on that pair, never on the digest alone. | none |
| R7 | $`n`$ equals the number of leaf occurrences, counting a shared leaf once for each path that reaches it. | `CountMismatch`, `CountOverflow` |
| R8 | Every historical generation is less than the current generation. | `NonHistoricalState`, in admission |

Rules R1 to R7 belong to the codec and hold for any reader of the format. Rule
R8 relates the history to the current state, so it stays in cold admission,
where V1 already checks it
(`rspace++/src/rspace/persistence/durable/admission.rs:1706-1713`), and the V2
reader of task 7925 inherits it.

The frozen encodings are pinned by length and BLAKE2b-256 digest (RV-04). In the
table, $`\texttt{aa}^{32}`$ is 32 bytes of value `aa`, and an entry is written
$`(g, d, \ell, c)`$:

| Object | Bytes | Digest |
|---|---|---|
| the empty radix node | 50 | `50e61ba04a451666697cf575964a1c6ada6a5d6a3a43f321c31f898d79f5cda8` |
| the empty semantic node, for contrast | 50 | `21dd2a72f6a9d98af1a2fd6cf2a871f2076bec013ce1cd6c7cca6d46ff4c0dc2` |
| the anchor of an empty history with current entry $`(1, \texttt{aa}^{32}, 50, 50)`$ | 136 | `aafa9f0acc53e37a3a686374191493dbaae8041691ed96f1e8af8b3e6730511a` |
| the leaf of the single entry $`(1, \texttt{bb}^{32}, 50, 50)`$ | 106 | `ed69c9c67bfd19d28914472c29d1082d30017a86e4b31186db5d2e292d3abfd2` |
| the anchor of that one-entry history with current entry $`(2, \texttt{aa}^{32}, 50, 50)`$ | 136 | `c618aca6611095642bc65d31754660b79ecbaf6da0d5c30f9134b18263770571` |

The empty radix node is the 32-byte envelope followed by an 18-byte body of
zeros: a zero prefix length, the `Absent` tag, the `Leaf` branch tag and a zero
child count. A leaf is 106 bytes: the envelope, the prefix length, a 48-byte
prefix, a 9-byte `Multiplicity` endpoint, the branch tag and the child count.

**The reference decoder, in literate form.** The test model's `unfold_radix` is
the decoder that the reader of task 7925 must refine. It keeps all of its work
on an explicit stack, so its native stack depth does not depend on the tree.
Given a store, the historical root and the declared count $`n`$:

1. ⟨Start⟩. Push the root with the empty path. The leaf list is empty.
2. ⟨Load⟩. Pop a reference $`r`$ and its path $`\pi`$. Refuse `MissingObject` if
   the store lacks the digest of $`r`$, and `ReferenceMismatch` if the stored
   bytes differ from $`r`$ in length or digest. Refuse `NotRadixDomain` unless
   the domain is `RSpaceDAG.RetR`. Parse the body with the production DAG node
   parser, and map its refusals to `Body`. Set $`\rho = 48 - \lvert \pi \rvert`$.
3. ⟨Dispatch⟩. Select on the endpoint, on whether the node has children, and on
   whether $`\pi`$ is empty:
   - an absent endpoint, no children and an empty path: the empty history.
     Return no leaves if $`n = 0`$, and otherwise refuse `CountMismatch`;
   - an absent endpoint, no children and a nonempty path: refuse
     `EmptyBelowRoot`;
   - a `Multiplicity` endpoint with claim $`c`$, no children and
     $`\lvert p \rvert = \rho`$: a leaf.
     Form the key $`k = \pi \Vert p`$. Refuse `LeafKey` unless $`k`$ decodes to
     an identity, and `LeafClaim` unless $`c \ge \ell`$. Append $`(k, c)`$;
   - an absent endpoint, children and $`\lvert p \rvert < \rho`$: an inner node.
     Push each child with the path $`\pi \Vert p \Vert e`$, where $`e`$ is the
     child's edge byte, in decreasing edge order, so that the smallest edge is
     popped first;
   - any other node with an `Absent` or `Multiplicity` endpoint: refuse
     `EndpointDepth`;
   - any other endpoint: refuse `EndpointKind`.
4. ⟨Count⟩. When the stack is empty, refuse `CountMismatch` unless exactly
   $`n`$ leaves were appended, and return them.

Children are popped in increasing edge order and each key extends its path, so
the leaves come out in strictly increasing key order, which is the entry order
by theorem 2. The decoder does work proportional to $`n`$. A check that needs
only the count runs the memoized post-order count, `memoized_count`, instead:
the empty node counts 0, a leaf 1, and an inner node the checked sum of its
children's counts. Memoized on the pair of digest and $`\rho`$ (rule R6), it
loads each distinct object at each depth once. The two differ exactly when
sharing is counted wrongly. In RV-06 two entries that differ only in their
generation share one leaf object: the decoder appends two leaves and the
memoized count returns 2, while a count of distinct leaf objects would return 1
(theorem 9).

**Metadata closure and checkpoint inventory.** The *metadata closure* $`M`$ of
an anchor is the anchor together with every radix node reachable from its
historical root. It always holds the anchor and the root, which is the empty
node when $`n = 0`$, so

```math
\lvert M \rvert \;\ge\; 2,
\qquad
\mathrm{bytes}(M) \;\ge\; 136 + 50 \;=\; 186 .
```

A V2 checkpoint must make $`M`$ durable beside $`R`$. The V1 descriptor counts
exactly one retention object
(`rspace++/src/rspace/persistence/checkpoint.rs:222-240`):

```math
\mathrm{DATA}_{\mathrm{objects}} \;=\; \lvert R \rvert + 1,
\qquad
\mathrm{DATA}_{\mathrm{bytes}} \;=\; \mathrm{bytes}(R) + \ell_{\mathrm{ret}},
```

where $`\ell_{\mathrm{ret}}`$ is the encoded length of the retention manifest.
The V2 descriptor keeps the 216-byte V1 layout under its own domain
`RSpaceDAG.Chkp2\0` and counts the union of the two sets:

```math
\mathrm{DATA}_{\mathrm{objects}} \;=\; \lvert R \rvert + \lvert M \rvert,
\qquad
\mathrm{DATA}_{\mathrm{bytes}} \;=\; \mathrm{bytes}(R) + \mathrm{bytes}(M) .
```

Three facts make the V2 rule sound:

1. *The union is disjoint.* The metadata domains differ from every existing
   domain (theorem 5). Under collision-checked storage, where equal digests
   imply equal bytes, no metadata object therefore shares a digest with a
   semantic object (theorem 6), and the checked digest-ordered merge of $`M`$
   into $`R`$ succeeds and adds both counts and bytes (theorem 10). A collision
   that storage failed to prevent is refused as `DuplicateInventoryDigest`
   (RV-14).
2. *The rules cannot be confused.* Because $`\lvert M \rvert \ge 2`$, no V2
   inventory satisfies the V1 rule (theorem 11). The V1 codec therefore refuses
   a V2 inventory even when it is presented under the V1 domain (RV-10).
3. *The inventory is closed.* It must hold every reachable radix node. An
   inventory of the anchor alone, which is what the V1 rule would produce, is
   not closed under references (theorem 12).

J-066 records the consequence for reads. Its exact read count for cold admission
charges V1 one read for the retention manifest, and V2 cannot inherit that single
read: its reader must charge the reads of the metadata closure. That refinement
belongs to task 7925.

**Old-reader compatibility.** The question is what a reader that knows only V1
does with a store after its first V2 publication. J-063 located the danger in
catalog selection (`CatalogSelectionV1::select`,
`rspace++/src/rspace/persistence/durable/catalog.rs:421-470`). When one slot
fails to parse and the other parses, selection takes the parsable slot and
records the other as ignored. If a V2 publication changed the catalog envelope,
a V1 reader would therefore select the older slot and adopt a stale state
without any error. J-063 named two remedies: keep the supported envelope and
version only the appointed content, or prove a whole-store upgrade fence. V2
takes the first and changes no envelope byte. A slot that appoints a V2 anchor
differs from a slot that appoints a V1 manifest only in the 40-byte retention
reference at bytes 216..256 and in the CRC at bytes 328..336 (RV-11). Every
reader parses both slots, and every reader selects the same, newest one. The
format is decided only after selection, by the domain of the appointed object:
a dual reader parses a `RSpaceDAG.RetA` anchor, while a V1-only reader refuses
it with `UnknownObjectDomain` from the object classifier
(`rspace++/src/rspace/persistence/object.rs:763-785`) or with `InvalidDomain`
from the V1 manifest parser (RV-09). The refusal is terminal: recovery never
falls back to the other slot after the selected slot fails (RV-13).

![Opening a catalog under the V2 format. Both slots are parsed, and if neither is valid the reader refuses with NoValidCatalog. Otherwise the newest valid slot is selected from the unchanged envelope, so a V1-only reader and a dual reader select the same slot. The domain of the appointed retention object then decides: every reader adopts a V1 manifest; a dual reader adopts a V2 anchor after unfolding its history and checking the count; a V1-only reader refuses the anchor terminally, without falling back to the older slot. Notes name the four negative controls, each of which breaks one of these decisions](figures/retention-v2-old-reader.svg)

`RSpaceRetentionV2Upgrade` checks this design in TLA+ (Lamport, 1994). Its state
has four parts:

- two catalog slots, each with an epoch, an image (`empty`, `valid` or `torn`),
  the appointed format and the envelope;
- `published`, the newest acknowledged epoch;
- `migrated`, which becomes true at the first acknowledged V2 publication;
- one view for each reader, `old` (supports V1) and `dual` (supports V1 and V2),
  each `closed`, `adopted` or `refused`, with the slot it selected and the epoch
  and format it read.

Three actions interleave. `Publish` writes epoch `published + 1` into the
inactive slot, with either format until `migrated` holds and with V2 afterwards,
acknowledges it and closes both views. `CrashDuringPublish` leaves the inactive
slot torn, acknowledges nothing and closes both views. `Open` selects the newest
valid slot whose envelope the reader can parse, then adopts its state if the
reader supports the appointed format and refuses otherwise. The base
configuration, with `MaxEpoch = 4`, checks four invariants:

- `RetentionV2TypeInvariant`: every variable stays in its domain;
- `AdoptionIsNewestPublished`: an adopted view holds the active slot and the
  acknowledged epoch, so no reader adopts a stale state;
- `NoReaderAdoptsUnsupportedContent`: an adopted format is one the reader
  supports;
- `ReadersSelectTheSameSlot`: any two open views name the same slot.

Each negative control adds one action that breaks one decision, and TLC must
report the named violation. Each non-vacuity configuration checks a `Never…`
predicate that the faithful specification must violate, which shows that the
behavior the predicate denies is reachable.

| Configuration | What it changes | Required TLC result | Generated / distinct states, depth (evidence run) |
|---|---|---|---|
| base | nothing | no error | 349 / 92, 7 |
| `CatalogEnvelopeMutant` | a V2 publication changes the envelope | violates `AdoptionIsNewestPublished` | 201 / 86, 5 |
| `SelectionDivergenceMutant` | selection consults the appointed format | violates `ReadersSelectTheSameSlot` | 223 / 77, 6 |
| `FallbackOnUnsupportedMutant` | a refusing reader falls back to the other slot | violates `AdoptionIsNewestPublished` | 93 / 46, 4 |
| `AdoptUnsupportedMutant` | a reader adopts without checking the format | violates `NoReaderAdoptsUnsupportedContent` | 64 / 38, 4 |
| `NeverOldReaderRefusesV2` | non-vacuity | violated: a V1-only reader refuses V2 content | 55 / 41, 5 |
| `NeverDualReaderAdoptsV2` | non-vacuity | violated: a dual reader adopts V2 content | 59 / 45, 5 |
| `NeverAdoptsBehindTornSuccessor` | non-vacuity | violated: a reader adopts the previous authority while the inactive slot is torn | 50 / 34, 4 |

The counts come from the evidence run named in the verification record. A
configuration that must fail stops at its first violation, and the number of
states TLC has generated by then depends on its random seed and on how its four
workers interleave, so only the base configuration's counts are exactly
reproducible. What each failing configuration must reproduce is its named
violation.

The model abstracts bytes into its envelope, format and image fields, and the
executable properties bind those fields to bytes. RV-11 shows that real V1 and
V2 slots differ only in the retention reference and the CRC, and that selection
picks the newer slot for every pair of formats in both slot orders. RV-12
changes each envelope field that a V2 publication could change: the domain, the
version, the reserved word at bytes 256..264, the flags, each reserved byte in
288..328, and the commit length. Each change makes the V1 parser refuse the
newer slot with its typed error, so selection returns the older slot, which is
exactly the stale adoption of `CatalogEnvelopeMutant`. RV-09 and RV-10 show that
V1 readers refuse every V2 object and descriptor, and RV-13 and RV-19 bind the
existing no-fallback and torn-slot tests to the model's terminal refusal and
torn-successor witness.

**Migration and the resume binding.** The migration from V1 happens once and in
one direction. It copies the current entry verbatim, sets $`n`$ to the number of
historical entries, and inserts each historical entry under its key with its
claim. Reading the result back yields the original V1 manifest; the count is the
length of the history, the keys strictly increase, and the current and retained
root counters of `CheckedDomainComposition` are unchanged (theorem 13). The
executable property also re-encodes the restored manifest and compares it byte
for byte with the original, including an alias set whose entries share a
generation, a digest or a length (RV-15), and it checks that the three semantic
roots are unchanged (RV-17). The flag `migrated` of the TLA+ model records the
one-way rule: once a V2 publication is acknowledged, every later publication
appoints V2. There is no export from V2 to V1.

A state then has two durable representations, one in each format, with the same
state version. A transfer session or resume token that binds only the state
version would let a V2 token resume a V1 session and the reverse (theorem 15).
The binding must therefore include the retention reference and the two domains
that name the format:

```math
\beta \;=\; H\bigl(\, g \,\Vert\, \mathrm{ref}(m) \,\Vert\, \mathrm{ref}(a) \,\Vert\, \delta_{\mathrm{ret}} \,\Vert\, \delta_{\mathrm{desc}} \,\bigr),
```

where $`m`$ is the semantic manifest, $`a`$ the appointed retention object,
$`\mathrm{ref}(\cdot)`$ the 40-byte encoding of an object reference,
$`\delta_{\mathrm{ret}}`$ the retention domain (`RSpaceDAG.Retn` or
`RSpaceDAG.RetA`), $`\delta_{\mathrm{desc}}`$ the descriptor domain
(`RSpaceDAG.Chkpt` or `RSpaceDAG.Chkp2`), and $`H`$ a collision-resistant hash;
the test model uses BLAKE2b-256. With the domains in the binding, a token of one
format never resumes a session of the other, even for the same state and the
same retention reference (theorem 14, RV-16).

**The contract for tasks 5689, 5690 and 5691.** J-063 asked for a compatibility
matrix covering V1 stores, V2 stores, unsupported appointed content, mixed and
torn catalog slots, migration, cold recovery and interrupted transfers. Three
tasks consume it. Task 5689 (the LMDB contract and migration) uses it for
import, restart, transfer and rollback. Task 5690 (DAG export, import and state
synchronization) must freeze reader, exporter and importer behavior and must
never let a resumed import change the physical retention version. Task 5691 (the
consensus root epoch) needs to know whether the format reaches consensus
identity. This section freezes the matrix:

| Situation | V1-only reader | Dual reader | Evidence | Remaining work |
|---|---|---|---|---|
| V1 store | adopts | adopts | `AdoptionIsNewestPublished` | none |
| V2 store | refuses with `UnknownObjectDomain` or `InvalidDomain`, terminally | adopts after unfolding the history and checking the count | `NeverOldReaderRefusesV2`, `NeverDualReaderAdoptsV2`; RV-09, RV-27 | none for reading; the writer, task 7926 |
| unsupported appointed content | refuses; never falls back | refuses unknown domains; never falls back | `NoReaderAdoptsUnsupportedContent`; RV-13, RV-20, RV-27 | none |
| a V1 slot beside a V2 slot | selects the newest slot | selects the same slot | `ReadersSelectTheSameSlot`; RV-11, RV-12 | none |
| a torn inactive slot | adopts the previous authority | adopts the previous authority | `NeverAdoptsBehindTornSuccessor`; RV-19 | none |
| migration | not applicable | migrates once, at the first V2 publication; no export to V1 | theorem 13; RV-15 | the writer, task 7926 |
| cold recovery | V1 admission, unchanged | charges the reads of $`M`$ and keeps rule R8 | J-066; theorems 10 to 12 and 16 to 21; RV-22 to RV-26 | none |
| interrupted transfer | resumes only within V1 | resumes only within V2 | theorems 14 and 15; RV-16 | tasks 5690 and 7938 |
| consensus identity | the three semantic roots | the same three roots; retention metadata never enters identity | theorem 13; RV-17 | task 5691 |

**Theorems.** The module proves fifteen capstones without axioms. Each has a
`Print Assumptions` line that reports a closed context, and the compiled module
passes independent kernel checking.

| # | Theorem (`RetentionV2Format.`…) | Statement |
|---|---|---|
| 1 | `retention_key_is_an_exact_48_byte_bijection` | The key of a valid identity has 48 bytes and decodes to that identity; a string that decodes is the key of the identity it decodes to; and distinct valid identities have distinct keys. |
| 2 | `retention_key_order_is_entry_order` | For valid identities, byte order on their keys equals the entry order. |
| 3 | `little_endian_key_mutant_inverts_generation_order` | Negative control: two concrete identities in entry order have keys in byte order, while their little-endian keys are in the reverse order. |
| 4 | `lossy_key_mutants_collapse_v1_aliases` | Negative control: a key of the generation alone, a key of the reference alone and a key without the length each map one V1-accepted alias pair to a single key, while the exact keys of each pair differ. |
| 5 | `retention_domains_are_pairwise_distinct` | The eighteen persistence domains, namely the fifteen existing ones, the two metadata domains and the V2 descriptor domain, are pairwise distinct and have 16 bytes each. |
| 6 | `domain_separation_excludes_cross_universe_digest_aliases` | For any digest function under which equal digests imply equal bytes, a metadata object and an object of an existing domain have different digests. |
| 7 | `shared_node_domain_mutant_aliases_empty_metadata_and_semantic_roots` | Negative control: the empty radix and empty semantic nodes are distinct 50-byte objects, but under the semantic node domain the empty radix node is the empty semantic node, and for any digest function the checked inventory insertion refuses it beside that node. |
| 8 | `radix_count_denotes_entry_occurrences` | Whenever the unfold of a node succeeds, the count recursion returns the number of unfolded leaves, every leaf key has the remaining width, the keys increase strictly, and there is at least one leaf. |
| 9 | `distinct_leaf_count_mutant_undercounts_shared_entries` | Negative control: in a store where two entries that differ only in generation share one leaf, the count and the unfold both give 2 and decode to the two entries, while the count of distinct leaf objects is 1. |
| 10 | `v2_checkpoint_inventory_is_retained_plus_metadata` | A successful checked merge of the metadata objects into a sorted retained inventory is a sorted permutation of the two lists and satisfies the V2 rule, and the merge succeeds whenever all digests are distinct. |
| 11 | `plus_one_rule_mutant_rejects_every_v2_inventory` | Negative control: when the metadata holds at least two objects, an inventory that satisfies the V2 rule violates the V1 rule for every retention-manifest length. |
| 12 | `anchor_only_checkpoint_mutant_is_not_reference_closed` | Negative control: on a fixture of two state manifests, an anchor and a one-leaf radix root, the faithful inventory is closed under references and the anchor-only inventory is not. |
| 13 | `v1_migration_preserves_entries_order_count_and_roots` | For every valid V1 manifest, reading its migration returns the manifest, the count is the length of the history, the history keys increase strictly, and the current and retained root counters of `CheckedDomainComposition` are unchanged for every numbering of manifests. |
| 14 | `retention_format_binding_separates_representations_of_one_state` | For every state and every pair of retention references, a V1 token never resumes a V2 session and a V2 token never resumes a V1 session. |
| 15 | `state_version_only_binding_mutant_resumes_across_formats` | Negative control: a binding of the state version alone is equal for the V1 and V2 representations of one state. |

**Executable bindings.** The generators bias every field toward its boundaries:

- identities draw generations from 0, 1, 255, 256, $`2^{64} - 1`$ and arbitrary
  values; digests from all zeros, all `ff` bytes and arbitrary values; and
  lengths from 32, 33, 50, $`2^{64} - 1`$ and arbitrary values of at least 32;
- *clustered* identities draw generations from 0 to 3, 255, 256 and
  $`2^{64} - 1`$, digests from six values that agree in their first 31 bytes, and
  lengths from 32, 50 and 51, so that the keys of a history share long prefixes,
  form alias pairs and build deep radix trees;
- a *history* maps the keys of up to 24 clustered identities to claims of the
  identity's length plus 0 to 2;
- a *product history* crosses two to four generations at one random base with
  one to four digests, so that each digest's leaf is shared by every generation;
- a *retained union* holds one to eleven references with random digests and
  lengths from 32 to 4095.

The independent test model builds a radix image with an explicit-stack planner.
It splits each range of sorted keys at the longest common prefix of the range's
first and last keys, groups the range by the next byte, and then encodes the
planned nodes in reverse order, so that every child is encoded before its
parent. It encodes each node with the production `CanonicalDagNode` encoder,
retagged to the radix domain, and reads images back with the reference decoder.
The production key and anchor are compared with the model's own byte layouts,
never with themselves. The generated properties run 256 cases, except the
key-codec, V1-refusal, collision and totality properties, which run 64.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| RV-01 | Every key has exactly 48 bytes: parsing refuses every other width from 0 to 49 as `LengthOutOfRange`, and a length below 32 as `ObjectLengthTooSmall`. The production key equals the model key, parses back and decodes to its identity, and on arbitrary 48-byte strings the production parser and the model decoder agree. | `retention_keys_are_exact_48_byte_bijections` |
| RV-02 | Production key order, raw byte order and model key order all equal the entry order. Histories of clustered identities in key order are valid V1 manifests, and two or more of their entries in reverse order are refused `RetainedStatesNotStrict`. Little-endian keys put generation 256 before generation 1. | `retention_key_bytes_sort_like_v1_identities_but_little_endian_keys_do_not` |
| RV-03 | A generation alias, a digest alias and a length alias each form a pair that V1 accepts. The key without the generation, the key without the reference and the key without the length each collapse one pair, and the exact keys keep all three distinct. On generated single-field variations, two keys are equal exactly when their identities are. | `generation_and_reference_aliases_keep_distinct_keys` |
| RV-04 | The three domains, the 48-byte width, the 136-byte anchor and the 50-byte empty node equal their model values. The empty radix node is the empty semantic node retagged, and the five objects of the golden table match their pinned lengths and digests. Anchor bytes 32..88 equal the V1 manifest's current entry. | `retention_v2_encodings_are_frozen` |
| RV-05 | Radix nodes are canonical DAG node bodies under `RSpaceDAG.RetR`, and the reference decoder enforces rules R1 to R5 and R7 with typed refusals: of `ValueFree`, `Unit` and `Content` endpoints, a short leaf, a `Multiplicity` node with children, a full-width inner node, a child under the semantic domain, an anchor in place of a node, an empty child, a claim below the length, an undecodable key, a wrong count, a unary node, unordered edges, a missing child and a substituted reference. The anchor refuses both mismatches of count and empty root. Generated histories round-trip with strictly increasing keys and a memoized count of $`n`$. | `radix_nodes_reuse_the_dag_body_under_the_metadata_domain` |
| RV-06 | Two entries that differ only in generation share one leaf: the decoder yields two leaves and the memoized count is 2. A product history of $`G`$ generations and $`D`$ digests counts $`G \cdot D`$ entries with only $`D`$ distinct leaf objects, and declaring $`D`$ is refused `CountMismatch`. The anchor constructor alone accepts that count, so checking it is the reader's duty. | `radix_count_is_entry_occurrences_under_leaf_sharing` |
| RV-07 | The metadata closure of every generated history is exactly the anchor plus the radix image. It contains the historical root, is closed under child references, has at least two members and 186 bytes, carries only the anchor and radix domains, and excludes the current semantic manifest and the empty semantic node; no radix node's digest equals that of its retagging to the semantic domain. A 4096-entry history yields 4114 members, the anchor, the root, 16 inner nodes and 4096 leaves, on a thread with a 256 KiB stack. | `metadata_closure_is_complete_closed_and_domain_separated` |
| RV-08 | For generated retained unions and histories, the metadata closure is disjoint from the union by digest, and the checked ordered inventory builder accepts their merge with exactly $`\lvert R \rvert + \lvert M \rvert`$ objects, $`\mathrm{bytes}(R) + \mathrm{bytes}(M)`$ bytes and the independently computed inventory digest. The V1 inventory rule refuses the same inventory with the exact `InventoryIncoherent` counts, and an anchor-only inventory omits the historical root. | `v2_checkpoint_data_is_the_disjoint_union_of_retained_and_metadata` |
| RV-09 | Every V2 object is refused by every V1 reader: the empty radix node, a leaf, an empty-history anchor, and generated anchors and radix nodes. The domain classifier, the object view and the streaming validator refuse with `UnknownObjectDomain`, and the V1 manifest and DAG node parsers with `InvalidDomain`. The anchor parser refuses a V1 manifest. | `v1_object_readers_refuse_every_frozen_v2_object` |
| RV-10 | The V1 checkpoint codec refuses a frozen V2 descriptor, whether its domain is retagged, its inventory appoints an anchor and its metadata, or both, with the exact `InvalidDomain` or `InventoryIncoherent` error. It does so for the descriptor alone, inside a resealed prepare record and inside a commit record with a corrected CRC, while the unchanged records parse. | `v1_checkpoint_records_refuse_the_frozen_v2_descriptor` |
| RV-11 | Catalog slots that appoint a V1 manifest and a V2 anchor for one state differ only in the retention reference, bytes 216..256, and in the CRC. Both round-trip, and for every pair of formats in both slot orders selection picks the newer slot and ignores none. | `catalog_selection_is_blind_to_the_appointed_retention_format` |
| RV-12 | Each envelope change, of the domain, the version, the reserved word at 256, the flags, each reserved byte in 288..328, or the commit length, makes the V1 slot parser refuse the newer slot with its typed error. Selection then returns the older slot and reports the changed one as ignored, in both positions: the stale adoption of `CatalogEnvelopeMutant`. | `every_catalog_envelope_change_makes_old_readers_select_the_stale_slot` |
| RV-13 | An existing test, bound as the executable counterpart of terminal refusal: damage to the selected successor pack is refused with a typed recovery error, leaves the catalog unchanged and never falls back to the previous slot, while damage to its derived index is repaired from the selected pack. | `selected_successor_damage_never_falls_back_and_repairs_safe_index` |
| RV-14 | The fifteen existing and three new domains are distinct, and the new ones are unknown to the V1 classifier. The empty radix and empty semantic nodes share a body but not a digest, and retagging the radix node to the semantic domain makes the inventory refuse a duplicate. A generated inventory that also holds an impostor, with a metadata object's digest and a different length, is refused with the exact `DuplicateInventoryDigest`. | `metadata_and_semantic_universes_are_disjoint_and_collisions_fail_closed` |
| RV-15 | Migrating generated V1 manifests, and a fixed set of alias entries, preserves the current entry, the count and every historical entry in order: the anchor parses back, the history unfolds to the V1 entries, and re-encoding the restored manifest reproduces the V1 bytes. | `v1_migration_preserves_entries_order_count_aliases_and_roots` |
| RV-16 | For generated states, the binding that includes the format separates the V1 and V2 representations, with their real retention references, with arbitrary references, and even with one shared reference, while the binding of the state version alone cannot. | `resume_binding_separates_v1_and_v2_representations_of_one_state` |
| RV-17 | Migration preserves the three semantic roots: the migrated current entry still names the semantic manifest, whose parsed roots are unchanged. | `v1_migration_preserves_entries_order_count_aliases_and_roots` |
| RV-18 | The key parser, the anchor parser, the reference decoder and the memoized count terminate without panicking on arbitrary bytes and on bit flips, truncations and extensions of valid keys, anchors and nodes, and whatever the key and anchor parsers accept re-encodes to the same bytes. | `arbitrary_retention_v2_bytes_are_total` |
| RV-19 | An existing test, bound as the executable counterpart of `NeverAdoptsBehindTornSuccessor`: for every torn prefix of a successor slot in the inactive position, selection keeps the previous authority and reports the torn slot as ignored, until the image is byte for byte the complete successor. | `every_observably_torn_inactive_slot_preserves_the_previous_authority` |

`model_property_manifest_is_unique_and_exhaustive` checks these nineteen bindings
among its 169. It reads `RetentionV2Format.v` as part of its Rocq source, and
`RSpaceRetentionV2Upgrade.tla` with its base configuration as part of its TLA+
source. It requires each of the theory's fifteen `Print Assumptions` capstones and
each of the four base invariants to be cited by an RV row of the derivation
matrix. It requires each `Never…` witness that an RV row cites to have a
configuration whose `INVARIANT` line names it, and the integrated runner to name
that configuration and to compile the theory.

**Boundary.** These results freeze the bytes and prove that the format is sound
and compatible with V1 readers. They leave these questions open:

- Nothing writes or reads V2 in production. The cold reader that unfolds a V2
  history, charges its metadata reads and keeps rule R8 is task 7925. The
  path-local writer that emits the first V2 publication is task 7926. That
  emission changes persisted bytes, so it stops for an explicit confirmation
  before it becomes the default.
- The reference decoder and the memoized count are test models. Their
  production refinement, with its object, byte and work limits, is task 7925.
- The V2 descriptor rule is frozen as a contract. The descriptor codec that
  accepts it is task 7925, and the inventory builder that produces it is task
  7926.
- The resume binding is a contract, not an implementation. The transfer
  protocol that realizes it is tasks 5690 and 7938.
- The TLA+ model is bounded at four epochs and abstracts bytes. RV-09 to RV-13
  and RV-19 bind its abstractions to bytes.
- Each V2 successor supersedes the radix nodes on its changed path. Reclaiming
  metadata objects that no catalog slot appoints follows the garbage collector's
  contract (tasks 7943 to 7945).

Cited works:

- J. R. Driscoll, N. Sarnak, D. D. Sleator and R. E. Tarjan, "Making data
  structures persistent", *Journal of Computer and System Sciences* 38(1):86–124,
  1989, [doi:10.1016/0022-0000(89)90034-2](<https://doi.org/10.1016/0022-0000(89)90034-2>).
- L. Lamport, "The temporal logic of actions", *ACM Transactions on Programming
  Languages and Systems* 16(3):872–923, 1994,
  [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726).
- V. Leis, A. Kemper and T. Neumann, "The adaptive radix tree: ARTful indexing
  for main-memory databases", *2013 IEEE 29th International Conference on Data
  Engineering*, 38–49, 2013,
  [doi:10.1109/ICDE.2013.6544812](https://doi.org/10.1109/ICDE.2013.6544812).
- D. R. Morrison, "PATRICIA—Practical Algorithm To Retrieve Information Coded in
  Alphanumeric", *Journal of the ACM* 15(4):514–534, 1968,
  [doi:10.1145/321479.321481](https://doi.org/10.1145/321479.321481).

The retention-format evidence in the verification record was taken at these
sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/RetentionV2Format.v` | `aa2750b94222ab8d3f36f00f5e1551ec5ed874158a85d5f699d76a3e2d74ea1e` |
| `formal/rocq/stack_safe_pda/_CoqProject` | `f754dbe3f5663238c12cd381ceb7c25888b8fc6a6f53d391373ee4ce712c1be3` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2Upgrade.tla` | `455d55405917277d1cff07fbd2248cf57547f54b2ddcf1acdde3a43b5add88db` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2Upgrade.cfg` | `1097087b71ed72e3b49b917e08834ff4610eff89827051bec730e346049f17b2` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeCatalogEnvelopeMutant.cfg` | `807fe344daae51b2062beff44912dfa41700afc6bfb0ca3900c016f540d021ae` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeSelectionDivergenceMutant.cfg` | `dc31f0b191dca75e62c036d090c07bcf13eb72d9ba5d049fb0d9f48cb2fd33c2` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeFallbackOnUnsupportedMutant.cfg` | `11e60a952d3d379f4f36bd0b01988190bec250e55c43962a3f367c42a1eb8c90` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeAdoptUnsupportedMutant.cfg` | `28f9772509d7ee8d93ce780d31a14a14571f921dd4bdb523570ac0a57e0da04f` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeNeverOldReaderRefusesV2.cfg` | `4faad9ee0ee1a87e5dbd9a05c701c24d1c1d56afefb148531767d53f8caeeba4` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeNeverDualReaderAdoptsV2.cfg` | `1b906349800ddc23e03b14f5d070dbcd0539798407e0f78586b3ca608a7978ff` |
| `formal/tlaplus/stack_safe_pda/RSpaceRetentionV2UpgradeNeverAdoptsBehindTornSuccessor.cfg` | `167e78f9ce730d27c1f0661f027c7c220a27ec0ab53964491477133f75740e81` |
| `scripts/check-stack-safe-pda-proofs.sh` | `4d1dfdaa5ec88924be766cba19d183adf8f6c4383514d0218c0871f225d8e4e4` |
| `rspace++/src/rspace/persistence/retention/v2_format.rs` | `5ba756aa6ff2ded5e9116669884ebdeab5b33804205dd8a7bf0ae1e5c2d5be74` |
| `rspace++/src/rspace/persistence/retention.rs` | `6b552c902392fdd6f33619e5eb26d643c8190fe41d6883d8f7f65f668e3669ed` |
| `rspace++/src/rspace/persistence/canonical.rs` | `5476bc048bbd46fa9e44e41d23180cdac01e7d3f96c5697ff58653e44d787003` |
| `rspace++/tests/persistence/retention_v2_format_properties.rs` | `07964557cdd3b5c542c3b055869e995141d3d2f041a6e06ffdcf91d29b0bc1d5` |
| `rspace++/tests/persistence/checkpoint_wal.rs` | `9a2443532894e61771f1ad8974fdd5ebbcdee6cbd7ea639ee40a164629a9e559` |
| `rspace++/tests/persistence/durable_catalog.rs` | `dec0de63a24433b6af01bc1a420322646227d15cf5aaacda6ec34c02953b145a` |
| `rspace++/tests/persistence/durable_publication.rs` | `1d45872beaa832504b1682f7849f0454d3afd66465dca6cf0c0f34d00fb775e2` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `0b548fbd3aade06e64fab0281b42b1a33501675ee44bde7106277197058c6e68` |
| `rspace++/tests/persistence/property_support.rs` | `baa2c0415898412559e80bfe2d83df297269f54e8e044ea0522fe0844236862e` |
| `rspace++/tests/persistence/mod.rs` | `b11541ecfd8fc4992678a1bc70ec39348b3b28c53dc0d936b9f09f1a9bf784f0` |

### Shared retention V2 cold reader (2026-10-26)

Task 7925 implements the reader that the format section above froze. Cold
recovery and cold admission now read both retention versions under an explicit
reader capability, and every V1 store is read exactly as before. A dual reader
adopts an appointed V2 anchor only after it has authenticated the complete
metadata closure $`M`$ separately from the retained semantic union $`R`$,
applied the V1 rules of strict history order and current exclusion to the
decoded entries, and charged every metadata read against the existing limits.
Unsupported or corrupt appointed content is refused before adoption, and a
V1-only reader refuses V2 content as the compatibility matrix above requires.
This is the first production code that reads V2. Nothing writes it: the writer
is task 7926, and its first emission changes persisted bytes, so it stops for an
explicit confirmation. Until then a source admitted from V2 content refuses
every successor before any durable mutation, and no persisted byte changes.

Six artifacts implement the task:

- six new closed capstones, numbered 16 to 21, with six helper lemmas, in
  `formal/rocq/stack_safe_pda/theories/RetentionV2Format.v`;
- the new module `rspace++/src/rspace/persistence/retention/v2_reader.rs`, which
  dispatches the appointed retention object, reads a radix history in two
  phases and builds the metadata inventory, and the reader capability
  `RetentionReader` and the metadata minimum in `retention/v2_format.rs`;
- the capability-aware object classifier and stream validator in
  `rspace++/src/rspace/persistence/object.rs`, with a refinement fix to the
  stream validator;
- the versioned checkpoint descriptor in `rspace++/src/rspace/persistence/checkpoint.rs`
  and the capability-parameterized record parsers in
  `rspace++/src/rspace/persistence/wal/{checkpoint,checkpoint_commit,codec,mod}.rs`;
- the directory's capability, its threading through recovery, V2 admission and
  the successor refusal in
  `rspace++/src/rspace/persistence/durable/{io,startup,recovery,admission}.rs`;
- the properties `RV-20` to `RV-29` in
  `rspace++/tests/persistence/retention_v2_format_properties.rs`,
  `checkpoint_wal.rs` and `durable_publication.rs`, an owner-checkpoint helper in
  `durable_rspace_authority.rs`, and an overflow regression test in
  `canonical_object_streaming_properties.rs`.

![Cold admission of the appointed retention object under the dual reader. Physical recovery, in grey, authenticates each checkpoint object's length and digest, classifies its domain prefix under the reader capability, and parses the checkpoint descriptor by its domain. Cold admission, in blue, loads the appointed object; a RetA domain under the dual reader parses as an anchor, in green, and every other domain as the strict V1 manifest, in amber. The retained-states limit and the current-entry check refuse, in red, before any radix node is read. On the V2 path, phase one reads each reachable radix node once and counts leaf occurrences with a memo keyed by digest and width; a count that differs from the anchor's refuses before any entry is enumerated. Phase two reserves exactly that many entries and enumerates them, the history rules run, and the metadata closure is the anchor plus the nodes read. Both paths then compare DATA minus R with the metadata, walk the closures and report](figures/retention-v2-cold-reader.svg)

**Vocabulary.** The terms of the format section above keep their meaning; these
are added:

- A *reader capability* is `RetentionReader::V1Only` or `RetentionReader::Dual`.
  `RetentionReader::admits(format)` holds for V1 under both capabilities and for
  V2 under the dual one only (`retention/v2_format.rs:44-46`).
- The *declared count* $`n`$ is the anchor's historical count. A node
  *occurrence* is a node together with the path of edges that reaches it from
  the root, and its *width* $`\rho`$ is the number of key bytes still unread
  where it is reached: $`\rho = 48`$ at the root, and a child of an inner node
  with prefix $`p`$ reached at width $`\rho`$ is reached at width
  $`\rho - \lvert p \rvert - 1`$, because the edge byte is one more key byte.
- *Physical recovery* is the bounded scan of the live packs that verifies or
  rebuilds the derived object index. *Cold admission* (`admit_physical_source`)
  then parses and checks the selected state completely before anything is
  adopted.
- $`\mathrm{DATA}`$ is the object inventory a checkpoint descriptor declares,
  and $`R_{\mathrm{decl}}`$ is the retained union it declares; $`R`$ is the
  retained union that admission computes from the closures of the retained
  states.

**Reader capability and dispatch.** `DurableDirectoryV1::open` gives every
directory the dual capability (`durable/io.rs:838`). A test build can downgrade
a directory with `with_retention_reader_for_test` (`durable/io.rs:851`), which
is how one directory image is read by both readers. Recovery's pack-provider
trait, `CatalogPackProviderV1`, defaults to V1-only (`durable/recovery.rs:189`),
and the directory's provider forwards the directory's capability
(`durable/startup.rs:328`). Every reader dispatches on the domain alone, and
none retries one format after the other refuses:

- `CanonicalObjectKind::from_domain_prefix_for` (`object.rs:780-798`) runs the
  V1 classification first. Only an `UnknownObjectDomain` under the dual
  capability is examined again, and only the domains `RSpaceDAG.RetA` and
  `RSpaceDAG.RetR` become the new kinds `RetentionAnchor` and
  `RetentionRadixNode`. The strict `CanonicalObjectView::parse` refuses both new
  kinds with `UnknownObjectDomain`, so every caller that names no capability
  keeps the V1 behavior.
- `GenerationCheckpointDescriptorV1::parse_for` (`checkpoint.rs:627-645`) parses
  the domain `RSpaceDAG.Chkp2` under the V2 inventory rule when the capability
  admits V2, and passes every other descriptor to the strict V1 `parse`.
- `AppointedRetention::parse` (`retention/v2_reader.rs:39-51`) parses a
  `RSpaceDAG.RetA` object as an anchor under the dual capability, and every
  other object as a V1 manifest, whose parser refuses any other domain with
  `InvalidDomain`.

**Physical and admission layers.** Recovery must stay bounded and cheap, so it
checks only what selection and index binding need; complete parsing belongs to
admission. The checks run in this order:

| Check | Layer and code | V1-only reader on V2 content | Dual reader |
|---|---|---|---|
| the checkpoint descriptor in the prepare header and the commit record | recovery: `preflight_generation_checkpoint_prepare_for`, `GenerationCheckpointCommitViewV1::parse_for` | `InvalidDomain` at the descriptor | the V2 rule: `RetentionAnchorLength`, `RetentionMetadataBelowMinimum` |
| each checkpoint object's length, digest and domain prefix | recovery: `stream_generation_checkpoint_prepare` | `UnknownObjectDomain` | anchors and radix nodes classified; any other unknown domain refused `UnknownObjectDomain` |
| the kind of the designated retention object | recovery: `designated_retention_object(format)` | not reached | an anchor for a V2 descriptor; a 136-byte DAG node in its place is refused `CheckpointDesignatedObjectKind` |
| each object of an incremental mutation, streamed | recovery: `scan_one_incremental_object_primary` with `CanonicalObjectStreamValidatorV1::new_for` | `UnknownObjectDomain` | anchors and radix nodes validated structurally |
| the anchor's envelope, version and body | admission: `AppointedRetention::parse` | not reached; the V1 manifest parser would refuse `InvalidDomain` | `UnsupportedVersion` for an anchor of version 2; `RetentionHistoryRootMismatch` |
| radix nodes, shapes, count and history rules | admission: `admit_retention_history_v2` | not reached | the typed refusals of the reader below |
| the metadata closure against $`\mathrm{DATA}`$, the generation rule, closures and the union | admission: `admit_physical_source` | not reached | the inventory refusals below |

**The versioned descriptor.** A V2 descriptor keeps the 216-byte V1 layout
under `RSpaceDAG.Chkp2\0`. Its rule, `require_v2_metadata`
(`checkpoint.rs:335-358`), refuses `RetentionAnchorLength` unless the appointed
retention reference has the anchor's 136 bytes, and
`RetentionMetadataBelowMinimum` unless

```math
\mathrm{DATA}_{\mathrm{objects}} - \lvert R_{\mathrm{decl}} \rvert \;\ge\; 2
\quad\text{and}\quad
\mathrm{DATA}_{\mathrm{bytes}} - \mathrm{bytes}(R_{\mathrm{decl}}) \;\ge\; 186,
```

with both subtractions checked, so that a $`\mathrm{DATA}`$ smaller than the
declared union is refused rather than wrapped. The constants are
`RETENTION_METADATA_MIN_OBJECTS` and `RETENTION_METADATA_MIN_CANONICAL_BYTES`
(`retention/v2_format.rs:18-20`): the anchor and the radix root. The codec
cannot compute $`M`$ exactly, because $`M`$ depends on the bytes of the radix
nodes, which the codec never reads; cold admission computes $`M`$ and compares
it exactly. Writing a V2 descriptor uses the V2 domain, but only the test-only
constructor `from_test_ordered_v2` builds a V2 inventory, because no production
code writes V2.

**The two-phase radix reader, in literate form.** `admit_retention_history_v2`
(`retention/v2_reader.rs:263-295`) authenticates a history against its anchor.
Every read goes through the bounded admission store, so every read is charged.

1. ⟨Empty history⟩ If $`n = 0`$, the anchor parser has already required the
   historical root to be the 50-byte empty radix node. Load the root exactly,
   refusing a missing object, a wrong length or a wrong digest. The entries are
   empty, and $`M`$ is the anchor and the root. Go to step 7.
2. ⟨Open an occurrence⟩ `open_count_frame` (`:383-428`) receives a reference
   $`r`$ and a width $`\rho`$:
   1. If a node with the digest of $`r`$ was already read under another
      reference, which can only differ in its length, refuse
      `ObjectLengthMismatch`. This check runs before the memo, so a digest
      reached under two lengths is refused whatever was counted for it.
   2. If the memo holds a count for the pair (digest of $`r`$, $`\rho`$), return
      it.
   3. If the node has not been read, read it with `read_radix_node`
      (`:499-552`): load it exactly through the store (`MissingObject`,
      `ObjectLengthMismatch`, `DigestMismatch`, or the store's read limit),
      refuse `NotRadixDomain` unless its domain is `RSpaceDAG.RetR`, parse the
      body with `CanonicalDagNodeView::parse_in_domain` (`Body`), map an absent
      endpoint to an inner node and a multiplicity to a leaf with its claim
      (`EndpointKind` for any other endpoint), and refuse `EndpointDepth` if its
      prefix is longer than $`\rho`$. The node is kept, keyed by its digest.
   4. Decide the shape at $`\rho`$. A childless leaf whose prefix has exactly
      $`\rho`$ bytes counts 1, which is memoized. A childless inner node is
      refused `EmptyBelowRoot`: the anchor admits the empty node only as the
      root of an empty history. An inner node with children and a prefix
      shorter than $`\rho`$ opens a frame whose children are reached at width
      $`\rho - \lvert p \rvert - 1`$. Every other combination is refused
      `EndpointDepth`.
3. ⟨Count⟩ `count_radix_history` (`:324-381`) keeps the open frames on an
   explicit stack, preallocated for 48 frames. It opens the top frame's next
   child at the frame's child width; a child that is counted at once is added to
   the frame's total with a checked sum (`CountOverflow`). A frame whose
   children are exhausted is popped, memoized at its pair and added to its
   parent. The root's total is the count.
4. ⟨Compare⟩ Refuse `CountMismatch` with the declared and counted values unless
   the count equals $`n`$. At this point no entry has been decoded and nothing
   proportional to $`n`$ has been allocated.
5. ⟨Enumerate⟩ `enumerate_radix_history` (`:430-466`) reserves exactly $`n`$
   entries with `try_reserve_exact`, refusing `EntryAllocation` if the
   reservation fails or $`n`$ does not fit a `usize`. It walks the nodes already
   read, depth first in edge order, with one 48-byte key buffer and an explicit
   stack. Entering a node copies its prefix into the buffer. A leaf decodes the
   buffer as a `RetentionKeyV2` (`LeafKey`) and forms the retained entry with its
   claim (`LeafClaim` if the claim is below the encoded length). An inner node
   pushes a frame, and each child first writes its edge byte. This phase reads
   no object.
6. ⟨History rules⟩ `validate_historical`, the function the V1 manifest parser
   applies, refuses `RetainedStatesNotStrict` unless the entries increase
   strictly in entry order and `RetentionContainsCurrent` if one of them is the
   current state.
7. ⟨Metadata⟩ `metadata_inventory` (`:298-322`) merges the anchor into the
   digest-ordered references of the nodes read, with a checked byte sum. This is
   $`M`$.

Every edge strictly lowers the width, so the pairs of digest and width form an
acyclic graph even when the stored objects reference themselves, a frame can
never reach its own pair again, and at most 48 frames are ever open. Phase two
reaches only pairs that phase one checked, because it starts at the root at
width 48 and follows the same edges.

**Why the reader counts before it enumerates, and why the memo has the width.**
A history of $`n`$ entries has at least $`n`$ leaf occurrences, but sharing lets
far fewer objects denote them. Forty-eight inner nodes, each with two children
that are the same next node, above a single leaf, 49 objects in all, denote
$`2^{48}`$ distinct keys. A reader that
enumerated first would do work proportional to $`2^{48}`$ before it could compare
with the anchor. The count reads each of the 49 objects once and compares.
Theorem 16 shows that a successful shape-checked count is the length of a
successful enumeration, so the count authorizes exactly $`n`$ enumeration steps.
Theorem 17 shows that a count without the shape checks would authorize an
enumeration that fails: it counts a history with an empty node below the root as
one entry. The memo follows Michie's memo functions: it remembers results so a
shared subtree is counted once (Michie, 1968). A node's shape depends on the
width at which it is reached, so a memo keyed by the digest alone would reuse a
count from one width at another and skip the shape check there. Theorem 19 is
that counterexample, drawn below, and theorem 18 proves that the count memoized
on digest and width equals the count without a memo, for every memo whose entries
are correct.

![Why the count memo is keyed by digest and width, on the store of theorem 19. The root, an inner node with an empty prefix reached at width 48, has two children: node 2 on edge 0 and node 3 on edge 1, both reached at width 47. Node 2 is a leaf with a 47-byte prefix, which fits at width 47 and counts 1. Node 3 is an inner node with an empty prefix whose children are reached at width 46: node 2 again on edge 0, in red, where its 47-byte prefix does not fit, and node 4, a leaf with a 46-byte prefix, on edge 1. A memo keyed by the digest alone reuses node 2's count and accepts three entries. The reader's memo keyed by digest and width sees a new key at width 46, runs the shape check and refuses EndpointDepth with remaining width 46 and prefix 47, without reading node 4](figures/retention-v2-depth-memo.svg)

The cost follows from the structure. Let $`N`$ be the number of distinct radix
objects reachable from the root and $`E`$ the number of child references among
them. Phase one reads each object once and opens each pair of object and width
at most once, and an object has at most 49 widths, so it does
$`O(49\,(N + E))`$ work and keeps the $`N`$ parsed nodes and at most 48 frames.
Phase two does $`O(48\,n)`$ work, since every key has 48 bytes and every inner
occurrence has at least two children, and holds the $`n`$ entries. Cold
admission checks $`n + 1`$ against `max_retained_states` before phase one runs,
so phase two is bounded by that limit, and phase one by the object-read limits.

**Admission order.** `admit_physical_source` (`durable/admission.rs:1684-1756`)
runs these steps for both formats:

1. Load the appointed retention object exactly through the bounded store.
2. Parse it with `AppointedRetention::parse` under the directory's capability.
3. Refuse the retained-states limit unless $`n + 1`$, computed with a checked
   addition, is within `max_retained_states`. For V2 this runs before any radix
   node is read.
4. Refuse `RetentionCurrentMismatch` unless the current entry names the
   catalog's state.
5. For V2, run `admit_retention_history_v2`. Its store refusals surface as
   `ObjectStore`, its canonical refusals as `Canonical` and its radix refusals as
   `RetentionRadix` (`durable/admission.rs:588-596`).
6. Fix the metadata counts: one object of $`\ell_{\mathrm{ret}}`$ bytes for V1,
   $`\lvert M \rvert`$ objects of $`\mathrm{bytes}(M)`$ bytes for V2.
7. At a checkpoint terminal, refuse `CheckpointRetentionMetadataObjects` or
   `CheckpointRetentionMetadataBytes` unless
   $`\mathrm{DATA} - R_{\mathrm{decl}}`$ equals the metadata counts. V1 is
   unchanged by this step, because its descriptor rule already makes the
   difference one object of $`\ell_{\mathrm{ret}}`$ bytes.
8. Then, unchanged: the current closure; each historical state in order, with
   the generation rule R8 (`NonHistoricalState`), its own closure and its byte
   claim; the retained-union comparisons `CheckpointRetainedUnionObjects` and
   `RetainedUnion`; and the semantics of the current state.

The report gains `retention_metadata_objects` and
`retention_metadata_canonical_bytes`, and the admitted source keeps an
`AdmittedRetention`: nothing more for V1, the anchor and $`M`$ for V2.

**The metadata closure is authenticated separately from the retained union.**
Admission must show that every object of $`\mathrm{DATA}`$ is accounted for.
Theorem 20 reduces that to checks admission can make one object or one count at
a time: if $`\mathrm{DATA}`$, $`R`$ and $`M`$ are each free of duplicates,
$`R`$ and $`M`$ are disjoint and both included in $`\mathrm{DATA}`$, and

```math
\lvert \mathrm{DATA} \rvert \;=\; \lvert R \rvert + \lvert M \rvert ,
```

then $`\mathrm{DATA}`$ is a permutation of $`R`$ followed by $`M`$, and its bytes
are the sum of theirs. Production realizes each premise:

- *Inclusion.* A checkpoint commit makes its own pack the first live pack
  (`wal/checkpoint_commit.rs:282-286`), and at a checkpoint terminal that pack
  holds the terminal commit, so the admission store serves exactly
  $`\mathrm{DATA}`$. Every object of $`R`$ and of $`M`$ is loaded and
  authenticated by digest through that store.
- *Disjointness.* The closure walk parses only the semantic object kinds and
  refuses a metadata object with `UnknownObjectDomain`, while the radix reader
  refuses any child outside `RSpaceDAG.RetR` with `NotRadixDomain` and reads the
  anchor only as the appointed object. The domains are distinct (theorem 5), so
  no object is in both sets, and under collision-checked storage no two objects
  share a digest across them (theorem 6).
- *No duplicates.* $`R`$ is the closure ledger and $`M`$ the node map, both keyed
  by digest.
- *Counts.* Step 7 gives
  $`\lvert \mathrm{DATA} \rvert - \lvert R_{\mathrm{decl}} \rvert = \lvert M \rvert`$,
  and the retained-union comparison gives
  $`\lvert R_{\mathrm{decl}} \rvert = \lvert R \rvert`$; the bytes follow the
  same way.

Theorem 21 shows that disjointness cannot be dropped. With $`R = \{1, 2\}`$,
$`M = \{2, 3\}`$ and $`\mathrm{DATA} = \{1, 2, 3, 4\}`$, every inclusion, count
and byte check passes, yet object 4 is in neither set. `RV-24` builds the same
situation from real objects: the empty semantic node and the empty radix node
both have 50 bytes, so a metadata set that took the empty semantic node from
$`R`$ in place of the empty radix node would match every count, while the empty
radix node in $`\mathrm{DATA}`$ would be authenticated by nothing.

**Read accounting and resource bounds.** V1 admission reads the manifest once,
$`\ell_{\mathrm{ret}} = 96 + 56n`$ bytes. V2 admission reads $`M`$, each object
once, so relative to a V1 twin of the same source

```math
\mathrm{reads}_{V2} \;=\; \mathrm{reads}_{V1} - 1 + \lvert M \rvert,
\qquad
\mathrm{bytes}_{V2} \;=\; \mathrm{bytes}_{V1} - \ell_{\mathrm{ret}} + \mathrm{bytes}(M).
```

No limit was added: the 35 boundaries of the resource matrix are unchanged, and
the metadata reads are charged against the existing object-read and
object-read-byte limits. A limit one below the metadata demand refuses inside the
radix reader; an exact one lets the reader finish, and the next read, the first
of the current closure, is refused. One bound did change. A V1 manifest grows by
56 bytes per entry, so the object-read byte limit bounded $`n`$ implicitly. A V2
anchor has 136 bytes for every $`n`$, and sharing lets a few objects denote many
entries, so `max_retained_states` is now the only bound on $`n`$, and admission
checks it before the first radix read.

**Successor refusal.** Every successor of an admitted source writes a V1
manifest built from the admitted history, and only task 7926 can write V2. Four
entry points therefore refuse a V2 source before any durable mutation:
`seal_transaction` and `bind_durable_transaction` with
`RetentionFormatNotWritable(V2)` (`durable/admission.rs:1499-1508`), and
`checkpoint_descriptor` and `try_visit_checkpoint_references` with
`SourceRetentionFormatNotWritable(V2)` (`durable/admission.rs:1126-1160`). The
publishers and the owner checkpoint reach these first, so a V2 source yields
`DefinitelyNotPublished`, an empty publication trace and an unchanged directory
(`RV-28`).

**A refinement fix in the stream validator.** `RV-20` compares the chunked
stream validator with the contiguous parser, and one of its fixed candidates
exposed a divergence that predates this task. Take an anchor retagged to the DAG
node domain whose current generation is $`2^{64} - 1`$. The DAG node parser reads
that generation, at body offset 0, as the prefix length. The contiguous cursor
then refuses `LengthOutOfRange` for the field `cursor extent`, because the end of
a prefix that starts at body offset 8 overflows (`ReadCursor::read_exact`,
`canonical.rs:482-489`). The stream validator compared the length with the 96
bytes left and refused `Truncated` instead. Both refuse, but the validator must
report the contiguous parser's error. The fix adds the same overflow check to the
validator's body-region check, `require_body_aggregate` (`object.rs:1166-1182`),
and to its check of the envelope's body length (`object.rs:1272-1280`). The
regression test `overflowing_declared_extents_refine_the_contiguous_oracle` sets
five length fields to the smallest overflowing value and to one below it, and
requires every chunking to report `LengthOutOfRange` and `Truncated`
respectively. It failed before the
fix. Only the error reported for already-refused input changes, so no admission
outcome and no persisted byte changes. The Rocq model uses unbounded natural
numbers, so this machine-level detail does not arise in it.

**Theorems.** The module now proves twenty-one capstones without axioms, and
passes independent kernel checking. The six new ones are:

| # | Theorem (`RetentionV2Format.`…) | Statement |
|---|---|---|
| 16 | `radix_count_success_unfolds_with_that_many_leaves` | Whenever the shape-checked count of an occurrence returns $`c`$, the unfold of that occurrence succeeds with exactly $`c`$ leaves. |
| 17 | `shape_free_count_mutant_counts_an_empty_node_below_the_root` | Negative control: on a store whose root has an empty inner child and a leaf child, the count and the unfold both refuse, while a count without the shape checks returns 1. |
| 18 | `memoized_radix_count_agrees_with_radix_count` | For every memo whose entries are correct counts at their widths, and enough fuel, the count memoized on object and width agrees with the count without a memo: both refuse, or both return the same count and the extended memo stays correct. |
| 19 | `digest_only_memo_mutant_accepts_a_misplaced_shared_leaf` | Negative control: on the store drawn above, the count and the memoized count refuse, while a memo keyed by the object alone returns 3. |
| 20 | `count_and_inclusion_authenticate_the_disjoint_union` | If the data, the retained union and the metadata are each free of duplicates, the union and the metadata are disjoint and included in the data, and the data's length is the sum of theirs, then the data is a permutation of the union followed by the metadata, and its byte sum is the sum of theirs. |
| 21 | `overlapping_count_mutant_accepts_an_unauthenticated_object` | Negative control: on data of four objects, a union of two and metadata of two that share one object, every premise of theorem 20 except disjointness holds, including the byte sums, yet one object of the data is in neither set. |

**Executable bindings.** The production reader is compared with the test model
of the format section, never with itself. The reference decoder and the
shape-strict count of that model are the oracles; the count is memoized like the
reader, and like the reader it refuses a digest reached under another length
before it consults the memo. Generated properties run 256 cases, except the
dual-reader, versioned-codec and totality properties, which run 64, and the
union verdicts, which run 512.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| RV-20 | `admits` holds exactly for V1 under both capabilities and V2 under the dual one. On fixed and generated candidates (valid objects of every kind, anchors, radix nodes, V1 and V2 descriptors, and their bit flips, truncations, extensions, version changes and retags to eleven domains), the classifier equals independent V1 and dual models; the strict and dual results agree off the two metadata domains; the stream validator under both capabilities and every chunking equals the contiguous parser; `validate_object_for` agrees for the declared and every foreign kind; `parse_for` equals the strict descriptor parse, except that a V2 descriptor parses under the dual capability, re-encodes to its bytes and meets the V2 minimum; and `AppointedRetention::parse` equals the models. | `dual_object_readers_extend_v1_readers_only_on_metadata_domains` |
| RV-21 | V1 descriptors, prepare records and commit records parse identically under both capabilities. A descriptor retagged to the V2 domain over a V1 inventory is `InvalidDomain` for the strict and V1-only readers and `RetentionAnchorLength` for the dual reader; a V1-domain descriptor appointing a V2 inventory is `InventoryIncoherent` for every reader; the complete V2 descriptor is `InvalidDomain` for the strict and V1-only readers and parses under the dual reader. Inside records whose layout was not adjusted, the dual reader refuses the prepare record `LengthMismatch` and the commit record `CheckpointIndexTail`. The migrated minimum has $`\mathrm{DATA} = R + 2`$ objects and 186 more bytes; changing either by one accepts at and above the minimum and refuses below it, anchor lengths 135 and 137 are refused, version 2 is `UnsupportedVersion`, and a 136-byte DAG node in the anchor's place is `CheckpointDesignatedObjectKind`. On generated sources the prepare record lists exactly $`R \cup M`$ in digest order with the ordered digest, the commit's index tail and `verify_prepare`, and V2 data under the V1 domain, V1 data under the V2 domain and a V1-sized tail are refused. | `versioned_checkpoint_codec_accepts_v2_inventory_only_under_the_dual_reader` |
| RV-22 | The production reader refines the reference decoder. An empty node below the root is refused by both, while the count without shape checks returns 1; an enumeration with reversed edge order is refused `RetainedStatesNotStrict`, and a history that contains the current state `RetentionContainsCurrent`. Each of 20 radix defects at every occurrence in five histories, and 256 generated images, is refused by both with the same class, and whenever the shape-strict count succeeds the unfold yields that many leaves unless a leaf key or claim is invalid. | `production_radix_admission_refines_the_reference_decoder` |
| RV-23 | The misplaced shared leaf is refused `EndpointDepth` with remaining width 46 and prefix 47 after reading the root, node 2 and node 3 once each and never node 4, while the digest-only memo returns 3. A two-way shared graph of 48 levels (49 objects) declared $`2^{48} - 1`$ is refused `CountMismatch`, a four-way one of 32 levels `CountOverflow` after 33 reads, and a 256-way one of 7 levels declared $`2^{56}`$ `EntryAllocation` after 8 reads; each reads every object once. A comb of 48 inner nodes and 49 leaves is admitted on a 256 KiB stack with $`\lvert M \rvert = 98`$. On generated images every object is read exactly once, the reads equal $`\lvert M \rvert - 1`$, and the digest-only count equals the depth-keyed count on valid images. | `radix_memo_is_keyed_by_depth_and_shared_graphs_cannot_force_enumeration` |
| RV-24 | For historical counts 0 to 8, $`M`$ is exactly $`\mathrm{DATA}`$ outside $`R`$, their merge is $`\mathrm{DATA}`$ with the descriptor's ordered digest, the counts and report fields match, $`R`$ holds only semantic objects and $`M`$ only metadata objects. A verdict function agrees with theorem 20 on all 4096 choices of a size of 1 or 2 and a membership in $`R`$, $`M`$ and $`\mathrm{DATA}`$ for three objects, and on generated cases, the overlapping rule of theorem 21 is unsound on them and on its witness, and the real-object twin of the witness is shown. An extra radix node in $`\mathrm{DATA}`$ is refused `CheckpointRetentionMetadataObjects` one above the computed count; a radix child naming a semantic object `NotRadixDomain`; a child reference with a longer length `ObjectLengthMismatch` and with a shorter one the materialization limit; one leaf shared under two lengths the cached `ObjectLengthMismatch`, also with three reads allowed, and the metadata read limit with two; a semantic node naming radix nodes `UnknownObjectDomain` in the closure. A V1 source whose retained union counts an unreachable anchor is refused `CheckpointRetainedUnionObjects` (3 declared, 2 computed) by the dual reader and `UnknownObjectDomain` in recovery by the V1-only reader. | `v2_admission_authenticates_metadata_separately_from_the_retained_union` |
| RV-25 | On ten sources, V1 reports are identical under both capabilities, and each V2 twin has the V1 report's disposition, structure, semantics, retained states, aggregates and union, with the reads and bytes of the accounting equations above. A read or byte limit one below the metadata demand is refused in the reader, and at exactly the demand the refusal moves to the first closure read. `max_retained_states` admits at its exact value and refuses one below, before any radix read, even with one read allowed. No refusal changes the directory. V1 and V2 twins refuse alike a historical state whose generation is not below the current one, a current mismatch, a length alias in the closure and a history that contains the current state. | `v2_cold_admission_matches_v1_admission_and_charges_metadata_reads_exactly` |
| RV-26 | On a migrated V2 source, the exact limits admit the same report. The six constructible durable, four positive checkpoint-WAL and eighteen admission boundaries each refuse one below their exact demand, the three zero-demand WAL axes stay at zero, and no refusal changes the directory. The four per-state structural boundaries are also exercised on the fixture whose historical state needs more than its current one. | `every_v2_cold_admission_resource_boundary_refuses_one_below_and_accepts_exactly` |
| RV-27 | Migrated V2 directories with three history lengths are refused by the V1-only reader at the descriptor and adopted by the dual reader; a product history of ten retained states reads each shared node once. A torn inactive successor slot leaves both readers on the old slot, and an appointed staged V2 successor is refused by the V1-only reader and adopted by the dual one. An anchor of version 2 is refused `UnsupportedVersion` in admission, and an unknown `RSpaceDAG.RetX` object `UnknownObjectDomain` in recovery. Of the 20 radix defects as directory images, exactly the two length shifts and the forged digest are refused in recovery (`ObjectReference`), a current entry in the history `RetentionCurrentMismatch`, an envelope of version 2 `UnsupportedVersion` in the body, and every other defect with the unit reader's refusal. Two restarts give identical reports and authority. With the derived index removed, the V1-only reader refuses and writes nothing, while the dual reader rebuilds and verifies the index, then verifies it as existing, with equal bytes. | `v2_restart_fixtures_are_refused_by_old_readers_and_adopted_by_dual_readers` |
| RV-28 | For an admitted V2 source under the V1 writer, which the test selects explicitly, the writer's publication, the writer's compaction and the publication authority refuse with `SourceRetentionFormatNotWritable(V2)`, `DefinitelyNotPublished` and an empty trace; `seal_transaction` and `bind_durable_transaction` refuse `RetentionFormatNotWritable(V2)`; `checkpoint_descriptor` refuses, and the reference visitor refuses after no visit; the owner's checkpoint, over a directory opened with the V1 writer, refuses before durable mutation. Admission afterwards is identical, and the directory is unchanged. | `v2_admitted_sources_refuse_every_successor_before_durable_mutation` |
| RV-29 | On arbitrary bytes and on bit flips, truncations and extensions of valid objects, the dual classifier equals its model, the stream validator with arbitrary chunk widths equals the contiguous parser, `validate_object_for` equals its expected result, whatever the descriptor and anchor parsers accept re-encodes to the same bytes, and the radix node parser terminates without panicking. On arbitrary radix graphs, including a forged object that references itself, each object is read at most once and the reads never exceed the objects; admission succeeds only when the declared count is at most 4096 and the entries equal the reference decoder's, and `CountMismatch` reports the model's count. | `dual_retention_readers_are_total_on_arbitrary_bytes_and_graphs` |

`model_property_manifest_is_unique_and_exhaustive` now checks 179 bindings,
these ten among them. It requires each of the theory's twenty-one
`Print Assumptions` capstones to be cited by an RV row of the derivation matrix,
and the integrated runner now compiles the theory expecting twenty-one closed
capstones.

**Boundary.** The reader is complete for every byte image that a V2 writer
could produce, and every V2 image in the tests is built by test-only
constructors. These questions remain open:

- The path-local writer that emits the first V2 publication is task 7926. Its
  first emission changes persisted bytes and stops for an explicit
  confirmation. Until it lands, a V2 source refuses every successor.
- The trusted successor that replaces cold re-admission on ordinary commits is
  task 7927, which task 11946 completes (the trusted successor publication
  section); its comparison with cold admission after every step is task 7928
  (the faulted trusted histories section), and the scaling evidence is the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29):
  thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history.
- At a terminal that is not a checkpoint, no $`\mathrm{DATA}`$ is declared, so
  step 7 has nothing to compare. A V2 source can have such a terminal only after
  a V2 successor, which this task refuses; task 7926 must bind the metadata of
  incremental terminals.
- The TLA+ model of the format section abstracts bytes; `RV-27` binds its
  refusal and adoption witnesses to directory images.
- Reclaiming metadata objects that no catalog slot appoints remains the garbage
  collector's contract (tasks 7943 to 7945).

Cited works:

- R. C. Merkle, "A digital signature based on a conventional encryption
  function", *Advances in Cryptology — CRYPTO '87*, Lecture Notes in Computer
  Science 293, 369–378, 1988,
  [doi:10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32), for
  authenticating a tree through the digests of its nodes, as the reader does
  with every radix node.
- D. Michie, "'Memo' functions and machine learning", *Nature* 218(5136):19–22,
  1968, [doi:10.1038/218019a0](https://doi.org/10.1038/218019a0), for
  memoization.

The cold-reader evidence in the verification record was taken at these sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/RetentionV2Format.v` | `437370608a13ba4cd21503d61e8b1d28dafa875fbc4d7c6bca14b98c114ff85b` |
| `scripts/check-stack-safe-pda-proofs.sh` | `cfd06e85a83898513280854e49b1fe1eab21072612c3705fd9acb6ef1ca7f183` |
| `rspace++/src/rspace/persistence/retention/v2_reader.rs` | `2c29081694c53ed6cadb8e49f42b75d656ffca6446c8460fa9251feed7902864` |
| `rspace++/src/rspace/persistence/retention/v2_format.rs` | `0e0d462a24e997860de604acb883521719530c98bf001f66c88c312336c8bc2a` |
| `rspace++/src/rspace/persistence/retention.rs` | `e2948e80e00a69e7fe8ea0d476b204b8712ea96af09aadcacd3ed925049b73e7` |
| `rspace++/src/rspace/persistence/object.rs` | `363be76fdee8d273222aaea3d9a5f4688ed7522758bbf91edb997f08e8d41391` |
| `rspace++/src/rspace/persistence/checkpoint.rs` | `70ffc489897dcc860c1589c11b9940e6a2b5ab6fbd6fe435dff44daa18dc87c5` |
| `rspace++/src/rspace/persistence/wal/checkpoint.rs` | `74196184fbb2da7684b7254a122b08180afe19459d6747a909323d70c47dc1b9` |
| `rspace++/src/rspace/persistence/wal/checkpoint_commit.rs` | `6803994600eeb2d53f4ebb8c5cdc79279d090ed9e9bef2c1c74663be9f14ed0b` |
| `rspace++/src/rspace/persistence/wal/codec.rs` | `f65e9d012e9cbac894e1688928cf06c48c97cd7e235266f08636f76e6964f81b` |
| `rspace++/src/rspace/persistence/wal/mod.rs` | `3d79d479690a3769860c2e6e950fd25451c5a97118c0c35b0fbe5d665176dc35` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `eec8a71928fa53b4ad0e42214ef34a81f4bee590c4dd0fd9aca77aed19e13ddf` |
| `rspace++/src/rspace/persistence/durable/io.rs` | `a6d90d91e03f3b16df0dce838a3f3ed4f4755fe02f9b53e0245b88465233a856` |
| `rspace++/src/rspace/persistence/durable/recovery.rs` | `60c375acd1eeefd8f821bea60ffec84fa9b1eb51b0856541d01059c8423fcfb8` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `ab6a9c12bebe2aa56ccae88a4139932370c128ffda7ff38ea9bbbd31c65655b0` |
| `rspace++/tests/persistence/retention_v2_format_properties.rs` | `529bf1aa2439209f6cdeadb7c39bacb4696698d43a6185fa10c9f8d280c0eb1f` |
| `rspace++/tests/persistence/checkpoint_wal.rs` | `82511408d3e29e4f792e0ba54d4be94235e23edfc1c638e095f4cfcac932b7cc` |
| `rspace++/tests/persistence/durable_publication.rs` | `88b163a5ff91b5148395c9521344d650dd42516a12f3a24abab4794399f01f20` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `f5887d7a6ce94c6f97c70a850207aa21404fa679ee7823841bee4a521b998962` |
| `rspace++/tests/persistence/canonical_object_streaming_properties.rs` | `5f5b1557b47e02a0889dfb33db29a56831d4baac2972c4586c64e07360d0fbad` |
| `rspace++/tests/persistence/property_support.rs` | `9404ad361d979e386dd9323cac7e76c14ee1dc9b44c28a1d726bd93f3be87039` |

### Path-local retention V2 metadata writer (2026-10-26)

Task 7926 implements the writer of the representation that the format section
froze and the cold reader above admits. The V1 writer collects the whole history
at every successor and re-encodes it as a new manifest of $`96 + 56(n + 1)`$
bytes. The V2 writer instead edits the history radix along the one search path
of the outgoing current state's key, with the existing iterative radix editor of
the persistent DAG, appoints one new 136-byte anchor, and accounts for the
metadata closure with one private ledger trial that expands only the objects
whose reference count crosses zero. The successor's historical count comes from
the compare-and-swap outcomes, never from the number of objects the editor
stored. A V1 source migrates once, at its first successor under the V2 writer,
by one sorted batch from the empty radix. The V1 writer is unchanged. This task
kept it the writer of every opened directory, with only the test-build setter
`with_retention_writer_for_test` (`durable/io.rs:865`) selecting the V2 writer,
because the first V2 successor of a directory is a one-way change for that
directory; the next commit made V2 the default
([activation](#retention-v2-writer-activation-2026-10-26)). No block, state root
or wire byte changes, and no running node uses the durable backend yet: the
authority backends are `Memory` and `Lmdb`
(`state/rspace_state_authority.rs:142-145`).

This section discharges the three items that the format and cold-reader sections
above left to this task: the production builder of the V2 checkpoint inventory,
the production migration, and the binding of the metadata of incremental
terminals.

Nine artifacts implement the task:

- sixteen closed capstones, T1 to T8 and their negative controls T1m to T8m, in
  the new `formal/rocq/stack_safe_pda/theories/RetentionV2Editor.v`, which
  `_CoqProject` lists and the integrated runner
  (`scripts/check-stack-safe-pda-proofs.sh`) compiles;
- the radix-domain seam of the existing editor in
  `rspace++/src/rspace/persistence/rewrite.rs`: `RadixDomainV1` and the entry
  point `rewrite_retention_history`;
- the retention radix encoding `RetentionRadixNodeV2` and the reader-parameterized
  validator `validate_canonical_object_for` in `object.rs`, and the staging
  overlay's reader capability in `store.rs`;
- the new module `rspace++/src/rspace/persistence/retention/v2_writer.rs`, with
  the commit, the eviction and the migration;
- the metadata domain `ClosureDomainV1::Metadata`, its seed and its cursor in
  `closure.rs` and `closure/ledger_trial.rs`, and the reference counts that the
  cold reader now reports in `retention/v2_reader.rs`;
- `SealedDagTransaction::with_retention_metadata` and the sealed transaction's
  reader in `transaction.rs`, with which the WAL encoder in `wal/codec.rs`
  validates;
- the writer switch, the coherence refusal, the successor binding, the checkpoint
  builder and the post-publication comparison in
  `durable/{io,admission,publication}.rs` and `checkpoint.rs`;
- the properties `RV-30` to `RV-37` in the new
  `rspace++/tests/persistence/retention_v2_editor_properties.rs`, and four
  directory tests in `durable_publication.rs`;
- the three figures of this section.

![One durable successor under the V1 and V2 retention writers. Cold admission runs once per opened writer: after physical recovery, in grey, a V2 writer with a V1-only reader is refused RetentionWriterUnreadable, in red, before the store is pinned; otherwise the source is admitted, and a V2 source seeds the metadata ledger domain with the reader's occurrence counts, in blue. Sealing refuses a V2 source under the V1 writer before the mutation is sealed. The sealed successor requires n + 2 within max_retained_states and closes the target entry c'. The appointed format and the writer then select one of three branches: V1 under the V1 writer re-encodes the whole manifest, in amber; V1 under the V2 writer migrates once, in purple, and runs a trial that only activates the new anchor; V2 under the V2 writer commits one compare-and-swap, in green, and runs a trial that activates the new anchor before it deactivates the old one. Both V2 branches attach the anchor and the staged radix nodes to the sealed object set. Publication validates every sealed object under the transaction's reader, writes the WAL transaction, commits the catalog slot and re-admits the successor cold; a metadata closure that differs from the trial's is reported as PublishedMetadataInventory with the outcome Published, and otherwise the successor is adopted](figures/retention-v2-writer-commit.svg)

**Vocabulary.** The terms of the two sections above keep their meaning; these are
added:

- $`w = 48`$ is the key width. A retention key $`\mathrm{key}(e)`$ is the
  48-byte big-endian encoding of an entry's generation, manifest digest and
  manifest length, so keys sort as entries do (format section).
- An anchor $`A = (c, n, r)`$ carries the current entry $`c`$, the historical
  count $`n`$ and the radix root $`r`$. A successor of the target state $`t`$
  closes the *target entry* $`c' = (t, b_t)`$, where $`b_t`$ is the canonical
  byte count of the closure of $`t`$, and appoints $`A' = (c', n', r')`$.
- An *endpoint* is `Absent` or `Multiplicity(b)`; a leaf's endpoint carries its
  entry's claim $`b`$. A *CAS edit* $`(k, e, e')`$ replaces the endpoint $`e`$
  at key $`k`$ by $`e'`$, and is refused unless the image holds $`e`$ at
  $`k`$. An *insertion* is $`(k, \mathrm{Absent}, \mathrm{Multiplicity}(b))`$
  and an *eviction* $`(k, \mathrm{Multiplicity}(b), \mathrm{Absent})`$.
- The *search path* $`P(k)`$ of a key in an image is the sequence of nodes that a
  search for $`k`$ visits from the root, ending where the key ends, leaves a
  node's prefix or misses an edge. Every node after the root consumes at least
  its edge byte, so $`\lvert P(k) \rvert \le w + 1`$.
- The *canonical image* of a set of entries is the radix that satisfies the shape
  rules of the format section. The *independent rebuild* is the test model's
  `build_radix`, and the theory's `partition_build`: the longest common prefix of
  the sorted entries, then one child per next byte.
- $`M`$ and $`M'`$ are the metadata closures of $`A`$ and $`A'`$: the anchor and
  every radix node reachable from it. The objects that change are
  $`M \,\triangle\, M' = (M \setminus M') \cup (M' \setminus M)`$.
- The *occurrence count* of a metadata object is its reference count in the
  metadata domain: its root multiplicity, which is 1 for the anchor and 0
  otherwise, plus one for each edge of a distinct metadata object that names it.

**The editor seam.** The persistent DAG already has a radix editor:
`RadixEditor` behind `rewrite_root` (`rewrite.rs`). It applies a sorted batch of
CAS edits with a longest-common-prefix cursor, expands children lazily, and
finalizes iteratively, pruning childless absent nodes and merging a unary absent
node into its child, the root included. It keeps its work on the heap and
preallocates. It was tied to the semantic DAG in exactly four places, and task
7926 parameterizes those four and nothing else:

1. `RadixDomainV1` (`rewrite.rs:43-55`) is `Semantic(namespace)` or
   `RetentionHistory`, and `CompareAndSwapFailure` (`rewrite.rs:58`) names it.
   Under `RetentionHistory` the editor records no semantic provenance.
2. `load_node` (`rewrite.rs:1498`) accepts, under `RetentionHistory`, only an
   object that the dual reader classifies as a retention radix node, and parses
   it with `CanonicalDagNodeView::parse_in_domain` under `RSpaceDAG.RetR`
   (`rewrite.rs:1523`).
3. The encoder of `CanonicalDagNode` is split into the envelope header and a
   shared `write_body` (`object.rs:239`). `as_retention_radix` (`object.rs:235`)
   writes the same body under `RSpaceDAG.RetR` as a `RetentionRadixNodeV2`
   (`object.rs:292`), and `build_final` (`rewrite.rs:1355`) emits that encoding
   for the retention domain (`rewrite.rs:1375`). Semantic bytes are unchanged,
   and a semantic node and a radix node with the same body have different
   digests, because their domains differ (theorem 5 of the format section).
4. `StagingObjectStore` gains a reader capability, V1-only by default
   (`store.rs:126`). `with_retention_reader` (`store.rs:133`) selects the dual
   reader, and `stage` and `put_if_absent_or_equal` validate with
   `validate_canonical_object_for` (`object.rs:1841`), which for the V1-only
   reader is exactly `CanonicalObjectView::parse`.

The entry point is `rewrite_retention_history` (`rewrite.rs:624-636`). When the
root is the empty radix node, the editor starts from a vacant node and does not
read it (`rewrite.rs:665-667`): the format fixes that node's bytes, and the
editor stages it whenever an edit empties a history.

**The commit, in literate form.** `commit_retention_v2`
(`retention/v2_writer.rs:50-60`) receives the staging overlay, the predecessor's
anchor $`A = (c, n, r)`$, the target entry $`c'`$ and the editor's limits:

1. ⟨Edit⟩ Form the one insertion
   $`(\mathrm{key}(c), \mathrm{Absent}, \mathrm{Multiplicity}(b_c))`$
   (`:108-121`) and apply it from $`r`$ with `rewrite_retention_history`
   (`edit_history`, `:123-137`). Every historical generation is below the
   current one (rule R8 of the format section), and keys sort by generation
   first, so $`\mathrm{key}(c)`$ is above every historical key and its search
   path takes the largest edge at every node. The editor loads the old search
   path, splits the node whose prefix $`\mathrm{key}(c)`$ leaves, builds the new
   leaf and stages the new search path. It returns the new root $`r'`$ and its
   work report.
2. ⟨Count⟩ `cas_count` (`:142-151`) folds the batch with checked arithmetic: an
   insertion adds one, an eviction subtracts one, and any other edit keeps the
   count. The editor refuses the whole batch unless every CAS matches, so the
   fold counts exactly the entries that changed, and $`n' = n + 1`$.
3. ⟨Anchor⟩ Build $`A' = (c', n', r')`$ with `RetentionAnchorV2::new`, which
   refuses unless $`n' = 0`$ exactly when $`r'`$ is the empty node.
4. ⟨Stage⟩ `stage_anchor` (`:153-163`) encodes $`A'`$ and puts it into the
   overlay that holds the radix nodes. The step returns the anchor and the
   editor's report.

Nothing reaches the directory here: the staged objects stay in the private
overlay until the binder attaches them to a sealed transaction.

The count must come from the CAS outcomes because a new entry need not add a
stored object. A leaf object holds only the bytes of its key below its parent's
edge and its claim, so two entries that differ only above that edge and have
equal claims have byte-identical leaves, which are one content-addressed object.
The editor's `inserted_objects` (`rewrite.rs:1404`) counts the objects that the
store reports as newly inserted, so it undercounts such an insertion
(theorem T4m, `RV-33`).

**Eviction.** `evict_retention_v2` (`:67-77`) applies the one eviction
$`(\mathrm{key}(v), \mathrm{Multiplicity}(b_v), \mathrm{Absent})`$ and appoints
$`(c, n - 1, r')`$. When the history empties, the editor stages the empty node
and the anchor names it. Retention is keep-all, and nothing in production evicts:
the eviction policy is task 7756. The function exists so that the theory's
eviction step and the properties below exercise the editor's removals.

**Migration of a V1 source.** `migrate_retention_v1` (`:82-106`) runs once, at
the first successor of a V1 source under the V2 writer. It reserves exactly
$`n + 1`$ edits with `try_reserve_exact`, inserts the $`n`$ historical entries
in their strict V1 order followed by the current entry $`c`$, as one sorted
batch from the empty root, which is vacant and never read, and appoints
$`A' = (c', n + 1, r')`$. Theorem T8 shows that this is the canonical rebuild of
the committed history and equals a commit on the migrated anchor. Migration
enumerates the history once, which the acceptance criterion allows; every later
successor is a path-local commit. A migration is as crash-atomic as any
successor, because its objects reach the directory only inside the commit's WAL
transaction.

![One V2 commit as a path-local edit of the history radix, on a history whose search path for the outgoing current entry's key is the root r, the inner node n1 and the leaf L1. In red, the predecessor's anchor A and that old search path, which the editor loads once and the successor no longer reaches. In green, the successor's anchor A' and the new search path: r', n1', the split point m and the new leaf Lc. In purple, the old leaf's remainder L1'', split off beside the path: key(c) continues past L1's prefix s with a byte b' above L1's byte b, so it leaves L1's prefix there. In grey, the subtrees X and Y, which both images share by reference and the commit neither reads nor writes. The commit loads 3 nodes, emits 5 and reuses 2 child references. Every node has at most 10,370 bytes, a search path at most 49 nodes, and the staged radix bytes at most 518,500, which the V1 manifest of 96 + 56n bytes passes exactly when n is at least 9,258. The count is n + 1 from the CAS outcome, even when Lc is an object the history already stores](figures/retention-v2-path-local-edit.svg)

**What one edit touches.** Let $`P`$ and $`P'`$ be the search paths of the edited
key in the old and the new image. The editor's report (`DagRewriteReport`,
`rewrite.rs:100-115`) is determined by them. For an insertion,

```math
\mathrm{loaded} = \lvert P \rvert,
\qquad
\mathrm{emitted} = \lvert P' \rvert + s,
\qquad
\mathrm{reused} = \sum_{v \in P} \lvert \mathrm{children}(v) \rvert - \bigl(\lvert P \rvert - 1\bigr),
```

where $`s`$ is 1 when a node is split off beside the new path and 0 otherwise.
The reused child references are every child of the old path except the one
followed at each step, which the new path carries by reference only. For an
eviction, $`\mathrm{loaded} = \lvert P \rvert + m`$, where $`m`$ is 1 when the
evicted leaf's parent is left with one child, which is then loaded to merge with
it, and $`\mathrm{emitted} = \lvert P' \rvert`$. An insertion into the empty
history loads nothing, emits two nodes, the leaf built below the vacant root and
the root that it merges into, and stages one object.

Every node of an image of key width $`w`$ has at most

```math
\beta(w) \;=\; \underbrace{32}_{\text{envelope}} \;+\; \underbrace{8 + w}_{\text{prefix}} \;+\; \underbrace{1}_{\text{endpoint}} \;+\; \underbrace{9}_{\text{branch}} \;+\; \underbrace{32 + 40 \cdot 256}_{\text{child table}} \;=\; 10\,322 + w
```

bytes. The child table has 41 bytes per child for Node4 and Node16, a 256-byte
slot table and 40 bytes per child for Node48, and a 32-byte bitmap and 40 bytes
per child for Node256. An inner node's prefix is shorter than $`w`$ and its
endpoint is absent, so the largest node is an inner Node256 node, and a leaf has
at most $`58 + w`$ bytes. One insertion or eviction stages at most $`w + 2`$
radix nodes, a search path and one split-off node, so whatever the history's
length it writes at most

```math
(w + 2)\,\beta(w) \;=\; 50 \cdot 10\,370 \;=\; 518\,500 \text{ bytes at } w = 48 .
```

A V1 manifest of $`n`$ historical entries has $`96 + 56n`$ bytes, and passes
that bound exactly when

```math
96 + 56n \;>\; 518\,500 \iff n \;\ge\; 9\,258 .
```

Theorem T3m proves that the V1 manifest grows by 56 bytes per entry without bound
and passes the path-local bound once $`n`$ is at least the bound itself; the
exact crossover is executable evidence (`RV-32`).

The bound is a worst case, and short histories invert the comparison. Derived
from the formulas above, not measured: in a dense history, whose generations are
consecutive, the last inner node of the right spine gathers up to 256 children,
and every commit re-encodes it, up to $`\beta(w)`$ bytes, while a V1 manifest
reaches that size only at about 184 entries. Below roughly two hundred entries a
V2 commit can therefore write more bytes than the V1 manifest it replaces; its
cost, unlike the manifest's, does not grow with $`n`$. The
[fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29)
measures it: at 256 to 65,536 retained states the median V2 step wrote 1,538
to 4,088 bytes, so the V1 manifest exceeds it from 26 to 72 entries on and is
10.1 to 2,387 times larger at those history sizes.

**Binding a successor, in literate form.** `bind_durable_transaction`
(`durable/admission.rs:1586-1671`) turns a sealed mutation into a durable
successor:

1. ⟨Authority⟩ Refuse `AuthorityMismatch` unless the sealed receipt's base is the
   admitted state and its profile the admitted profile, then refuse a V2 source
   under the V1 writer with `RetentionFormatNotWritable(V2)`
   (`require_writable_retention`, `:1570-1577`). `seal_transaction`
   (`:1552-1566`) makes the same refusal before it seals the mutation, so a
   refused successor seals nothing.
2. ⟨Preflight⟩ Parse the appointed retention object under the dual reader and
   require $`n + 2`$, computed with a checked addition, within
   `max_retained_states` (`:1599-1610`). Only the count is read, so for V2 the
   preflight does constant work.
3. ⟨Close the target⟩ Compute the successor's inventory and the target entry
   $`c' = (t, b_t)`$ from its current closure (`:1611-1615`).
4. ⟨Write⟩ Dispatch on the appointed format and the writer:
   - V1 under the V1 writer: collect the history, append $`c`$, and attach a
     complete V1 manifest (`:1617-1623`), unchanged by this task;
   - V1 under the V2 writer: migrate (`:1624-1636`);
   - V2 under the V2 writer: commit (`:1637-1654`);
   - V2 under the V1 writer: refuse `RetentionFormatNotWritable(V2)`
     (`:1655-1659`).

   Both V2 branches stage into `retention_staging` (`:1675-1677`), a private
   overlay over the pinned store with the dual reader. The editor runs without
   limits of its own: a commit touches at most $`w + 2`$ nodes (theorem T3), and
   a migration inserts $`n + 1`$ entries, which the preflight bounded by
   `max_retained_states`.
5. ⟨Account⟩ `attach_retention_v2` (`:1683-1706`) runs one ledger trial in the
   metadata domain from the predecessor's ledger: $`[A']`$ added and $`[A]`$
   removed for a commit; for a migration nothing is removed, because a V1
   manifest is not a metadata object. The trial's budgets are the admission
   limits that bound the cold reader's walk of the same closure
   (`metadata_trial_limits`, `:2079-2089`); no limit is added.
6. ⟨Attach⟩ `SealedDagTransaction::with_retention_metadata`
   (`transaction.rs:726-789`) merges the staged objects into the sealed,
   digest-ordered object set. It refuses a second attachment, a digest that
   names different bytes (`ContentCollision`) and an anchor missing from the
   merged set; it appoints the anchor and makes the transaction's reader dual
   (`retention_reader`, `transaction.rs:793`). The successor's inventory carries
   the trial's metadata inventory $`(\lvert M' \rvert, \mathrm{bytes}(M'))`$.

Preparing the transaction validates every sealed object under the transaction's
reader (`wal/codec.rs:360-364`), so the anchor and the radix nodes pass, and no
metadata object can enter a transaction without attached V2 metadata.

![The metadata ledger trial of one V2 commit, on the objects of the path-local edit figure. The predecessor ledger was seeded once at cold admission with occurrence counts, so the shared subtrees X and Y count 1 and the anchor A counts 1. The writer commits, the editor loads the old search path and stages the new one together with A', and the trial runs with A' added and A removed. Activation of A' comes first: it expands A', r', n1', m, L1'' and Lc, whose counts cross from 0 to 1, and raises X and Y from 1 to 2 without expanding them. Deactivation of A follows: it expands A, r, n1 and L1, whose counts cross from 1 to 0, and lowers X and Y back to 1. The trial expands the 10 changed objects with 14 references, within 2w + 7 = 103 root events and expansions, and its result is the cold ledger of A'. Two negative controls: deactivating first expands both whole closures, and a seed that counts each object once deactivates a leaf shared by two parents that the successor still references](figures/retention-v2-metadata-trial.svg)

**The metadata trial.** The trial is the private ledger trial of the
copy-on-write section above (task 7924), run in a third domain.
`ClosureDomainV1::Metadata` widens the per-domain arrays to three
(`closure.rs:196-209`), and `ClosureDomainV1::SEMANTIC` (`closure.rs:203`) names
the two semantic domains that the semantic trials still run over. The cursor
decodes a metadata object by its domain: an anchor references its radix root, a
radix node references its children, and any other object is refused, so no
state-DAG object enters the metadata domain (`push_metadata_references`,
`closure/ledger_trial.rs:398-432`). Root changes are authorized against the root
multiplicity in the trial's domain (`authorized_roots`, `:197-224`).

The trial activates $`[A']`$ before it deactivates $`[A]`$. During activation an
object is expanded when its count crosses from zero to one; an object of
$`M \cap M'`$ already has a positive count, so it is only incremented. During
deactivation an object is expanded when its count crosses from one to zero; an
object of $`M \cap M'`$ keeps a positive count. The trial therefore expands
exactly the changed objects (theorem T6):

```math
\mathrm{expansions} \;=\; \lvert M \,\triangle\, M' \rvert,
\qquad
\mathrm{references} \;=\; 2 + \sum_{v \,\in\, M \triangle M'} \mathrm{outdeg}(v),
```

where the 2 counts the two root events, an anchor's out-degree is 1, and a radix
node's is its number of children. In the theory every cursor step spends one unit
of expansion credit, $`1 + 2\,\lvert \mathrm{children}(v) \rvert`$ for an
expanded object (`GenerationCheckpointPublication.v:2390`), so the trial takes
$`2 + \sum_{v \in M \triangle M'} \bigl(1 + 2\,\lvert \mathrm{children}(v) \rvert\bigr)`$
steps. For one insertion or eviction, one side of the change has at most
$`w + 3`$ objects, a search path, the node split off or merged away and an
anchor, and the other at most $`w + 2`$, a search path and an anchor, so

```math
2 + \mathrm{expansions} \;\le\; 2 + (w + 3) + (w + 2) \;=\; 2w + 7 \;=\; 103 \text{ at } w = 48 .
```

The trial reads each departing object, $`M \setminus M'`$, once from the pinned
store, and each arriving object from the overlay. The editor has already read
every departing object except the anchor $`A`$, so the old search path, and the
merged sibling of an eviction, are read twice: at most $`w + 2`$ objects.

The order matters (theorem T6m). Deactivating $`[A]`$ first drives every object
of $`M`$ to zero and expands it, and activating $`[A']`$ afterwards expands every
object of $`M'`$: $`\lvert M \rvert + \lvert M' \rvert`$ expansions, which grow
with the history, although the final ledger is the same. This is Collins's
reference counting (Collins, 1960) with the increments of a pointer replacement
applied before its decrements, so a shared subtree never passes through zero.

**The seed.** Cold admission of a V2 source seeds the metadata domain once
(`durable/admission.rs:2011-2013`). `seed_retention_metadata`
(`closure.rs:441-479`) gives every object of $`M`$ its occurrence count and the
anchor a root multiplicity of one, and refuses a second seed. The cold reader
computes the counts while it builds $`M`$ (`metadata_inventory`,
`retention/v2_reader.rs:323-368`), from the children of the nodes it has already
parsed, so seeding reads no object again. Theorem T7 shows that this seed is the
ledger that activating $`[A]`$ from an empty domain builds. A seed that counts
each object once is wrong (theorem T7m): a leaf shared by two parents would count
1, and evicting one of its aliases would deactivate it while the successor still
references it, dropping that object and its bytes from $`M'`$.

**Publication and its metadata check.** After the catalog slot is committed,
`publish_incremental_with_terminal_v1` (`durable/publication.rs:838-1002`)
re-admits the published successor cold, as every cold-checked incremental
publication does; since task 11946 the trusted publication does not (the
trusted successor publication section). A V2 successor ends in an incremental
terminal, which declares
no DATA, so step 7 of the cold reader has nothing to compare. The publisher
therefore compares the trial's metadata inventory with the admitted one and
reports `PublishedMetadataInventory{trial, admitted}` when they differ
(`durable/publication.rs:981-995`). The slot is already committed, so the outcome
is `Published` (`:297`): the directory holds the successor, and the error reports
that the writer's accounting disagrees with what a cold reader adopts. This binds
the metadata of incremental terminals.

**Writer and reader coherence.** `DurableDirectoryV1` now carries a writer beside
its reader (`durable/io.rs:801-802`). `open` selects the dual reader and, at this
task, the V1 writer; since the activation commit it selects the V2 writer
(`:839-840`). `try_clone_reader` copies both (`:892-893`). A V2 writer must read
back what it writes, so it requires the dual reader:
`admit_physical_source` refuses `RetentionWriterUnreadable{writer, reader}` for
a V2 writer with the V1-only reader (`durable/admission.rs:1855-1863`), after the
profile check and before the store is pinned, so the refusal writes nothing.
Physical recovery runs first, so content that the V1-only reader cannot read is
refused there, as before.

| Appointed source | Writer | Reader | Cold admission | Successor | Generation checkpoint |
|---|---|---|---|---|---|
| V1 | V1 | either | admitted, unchanged | a V1 manifest of $`96 + 56(n + 1)`$ bytes | V1: the retained union and the manifest |
| V1 | V2 | dual | admitted, unchanged | the migration, then V2 commits | V1, until the first successor |
| V2 | V1 | dual | admitted | refused `RetentionFormatNotWritable(V2)` before sealing | refused `SourceRetentionFormatNotWritable(V2)` |
| V2 | V2 | dual | admitted, with the metadata domain seeded | a path-local commit | V2: the retained union and $`M`$ |
| V1 | V2 | V1-only | refused `RetentionWriterUnreadable` after physical recovery | not reached | not reached |
| V2 | either | V1-only | refused in physical recovery, as before | not reached | not reached |

Since the activation commit, an opened directory is row 2 or row 4, by its
appointed format. Every other row needs a test-build setter, the V1 writer's or
the V1-only reader's, and those setters stand in for older binaries.

**Old readers after the first V2 successor.** The first V2 successor of a
directory is a one-way change for it:

- a V1-only reader refuses the directory in physical recovery and writes
  nothing: at a V2 checkpoint descriptor with `InvalidDomain`, or, when the last
  checkpoint is V1 and a migration transaction follows it, at that transaction's
  first anchor or radix node with `UnknownObjectDomain`;
- the V1 writer under the dual reader still adopts the directory, with the report
  and authority that the V2 writer adopts, but refuses to extend it
  (`RetentionFormatNotWritable(V2)`) or to checkpoint it
  (`SourceRetentionFormatNotWritable(V2)`, `RV-28`);
- nothing exports V2 back to V1.

No reader adopts content that it does not support, which is the property
`NoReaderAdoptsUnsupportedContent` of the upgrade model
(`RSpaceRetentionV2Upgrade.tla`). This task does not change that model; the
directory tests below realize its refusal witnesses on images that the writer
produced.

**Checkpoints.** Under the V2 writer, a generation checkpoint of a V2 source
declares as DATA the retained union and the metadata closure.
`checkpoint_descriptor` (`durable/admission.rs:1150-1177`) builds it with
`GenerationCheckpointDescriptorV1::from_retained_union_and_metadata`
(`checkpoint.rs:509-526`), whose inventory (`checkpoint.rs:273-301`) traverses
the closure ledger once in digest order and visits every object counted in the
retained or the metadata domain (`try_visit_retained_union_and_metadata`,
`closure.rs:507-527`); the domains are disjoint, so no object is counted in
both. The copy visitor (`try_visit_checkpoint_references`,
`durable/admission.rs:1179-1198`) uses the same traversal
(`try_visit_checkpoint_references_v2`, `checkpoint.rs:469-476`). This is the
production builder that the format section left to this task; the test-only
`from_test_ordered_v2` still builds the fixtures. Cold admission then compares
$`\mathrm{DATA} - R_{\mathrm{decl}}`$ with $`M`$ exactly, as before. A V1 source
keeps its V1 checkpoint under the V2 writer until its first successor. Because a
checkpoint carries $`M`$ completely, compaction may reclaim the pack that held
superseded radix nodes, and the next restart still adopts the anchor and its
closure.

**Theorems.** `RetentionV2Editor.v` proves sixteen capstones without axioms, and
the module passes independent kernel checking:

| # | Theorem (`RetentionV2Editor.`…) | Statement |
|---|---|---|
| T1 | `canonical_radix_exists_and_is_unique` | For strictly sorted entries of width $`w`$, the partition build is a canonical root that denotes exactly those entries, and every canonical root that denotes them is that build. |
| T1m | `unary_inner_mutant_breaks_uniqueness` | Negative control: a shape rule that admits unary inner nodes accepts two distinct images of one entry, a unary root above a leaf and the leaf alone, and the unary one is not canonical. |
| T2 | `radix_edit_batch_is_the_canonical_rebuild` | On a canonical root, a batch of width-$`w`$ CAS edits is refused exactly when some CAS does not match the entry it meets, and otherwise yields the partition build of the updated entries, which is canonical, denotes them, and is the empty tree when none remain. |
| T2m | `unpruned_empty_child_mutant_is_not_the_rebuild` | Negative control: evicting one of two entries with a finalizer that does not prune leaves an empty child, so the result is neither the partition build of the remaining entry nor canonical. |
| T3 | `radix_edit_changes_only_the_edited_paths` | For a key absent from sorted width-$`w`$ entries, inserting it into the partition build and evicting it again move between the two partition builds; both search paths have at most $`w + 1`$ nodes; a node only in the larger image is on its search path or is the one node that the insertion splits off and the eviction merges away; a node only in the smaller image is on its search path; and every node has at most $`\beta(w)`$ bytes. |
| T3m | `flat_manifest_mutant_rewrites_bytes_proportional_to_history` | Negative control: the V1 manifest of $`n`$ entries has $`96 + 56n`$ bytes, grows by 56 bytes per entry, passes every bound for some $`n`$, and passes $`(w + 2)\,\beta(w)`$ once $`n`$ is at least that bound. |
| T4 | `anchor_step_realizes_the_retention_entry_step` | The anchor of a valid V1 manifest reads back as that manifest; a commit of a target above the current entry and an eviction of a historical entry yield the anchors of the stepped manifests, which read back as them and whose composition entries are the `step_entries` of the composition section; and the metadata root change, $`[A']`$ in and $`[A]`$ out, is authorized and moves the root multiplicity from $`A`$ to $`A'`$. |
| T4m | `inserted_object_count_mutant_undercounts_an_aliased_leaf` | Negative control: a commit into a history of two entries that share one leaf object inserts no leaf object, so counting inserted leaves gives 2, while the CAS count gives the 3 entries of the result. |
| T5 | `canonical_radix_store_is_admitted_by_the_cold_reader` | For an injective naming and a store that holds a canonical image, the image is named by the empty node exactly when it has no entries; otherwise the plain count and the memoized count, with a sound memo, return its number of entries, and the unfold yields its entries in strict key order. |
| T5m | `unmerged_root_mutant_is_refused_by_the_cold_reader` | Negative control: evicting one of two entries with a finalizer that does not merge the root leaves a unary root with the same entries; the faithful result counts 1, while the count, the unfold and the memoized count all refuse the unmerged root. |
| T6 | `metadata_trial_work_is_the_changed_objects` | From a valid predecessor whose roots are the occurrences of $`A`$, a trial of $`[A']`$ in and $`[A]`$ out ends in the seed of $`A'`$, prepares $`\lvert M' \rvert`$ objects, expands exactly the $`\lvert M \,\triangle\, M' \rvert`$ changed objects in $`2 + \sum_{v \in M \triangle M'} (1 + 2\,\lvert \mathrm{children}(v) \rvert)`$ cursor steps, and for one insertion or eviction satisfies $`2 + \mathrm{expansions} \le 2w + 7`$. |
| T6m | `deactivate_first_mutant_expands_the_shared_history` | Negative control: for a commit into an aliased history, the faithful trial expands the 5 changed objects, while deactivating first expands $`\lvert M \rvert + \lvert M' \rvert = 9`$. |
| T7 | `metadata_ledger_seed_is_the_cold_ledger` | The seed of an anchor with occurrence counts is active exactly on the objects reachable from the anchor, is a credited cold build, agrees count for count with every credited cold build, and is a valid trial base. |
| T7m | `set_seed_mutant_drops_a_shared_radix_child` | Negative control: when an eviction removes one alias of a leaf shared by two parents, the occurrence seed counts the leaf 2 and the trial keeps it with count 1, while a seed of ones deactivates it, although a live node still references it and the cold ledger of the successor has it active. |
| T8 | `v1_migration_is_the_canonical_rebuild_of_its_history` | For a valid V1 manifest, inserting its history into the empty tree yields its anchor's canonical root and count, the anchor denotes the migrated manifest and reads back as it, and migrating the committed manifest equals committing on the migrated anchor. |
| T8m | `current_in_history_migration_mutant_duplicates_the_current_entry` | Negative control: a migration that also inserts the current entry reads back with the current entry in the history, which V1 refuses, and counts that entry twice among the retained roots. |

**Executable bindings.** Every property compares the production writer with the
independent test model of the two sections above: `build_radix` and
`model_anchor_bytes` for images and anchors; the reference unfold, the memoized
count and the depth-keyed count for admission; and a closure and occurrence
oracle that walks the rebuilt image for the ledger. No property compares the
writer with itself. Each row gives its number of generated cases.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| RV-30 | On 128 generated histories of up to 24 entries with up to 8 detour keys, one sorted batch from the empty root and a seeded order of single insertions, followed by the evictions of the detours, both end at the independent rebuild's root with exactly its reachable objects. Splitting a generated leaf into a unary inner node is refused `CompressibleUnaryNode` by the node constructor, and the stored unary image is refused `Body(CompressibleUnaryNode)` by the cold reader and by the reference unfold, while the rebuild is admitted. | `canonical_radix_images_are_determined_by_their_entries` |
| RV-31 | On 256 generated histories with batches of up to 10 CAS edits on present or fresh keys, consistent or stale, a consistent batch yields the rebuild of the updated entries, stages only rebuild objects byte for byte, reports one mutation per edit, and yields and stages the empty node when it empties the history; a stale batch is refused `CompareAndSwap` with the model's failure (domain `RetentionHistory`, key digest, expected and actual endpoints) and stages nothing; both outcomes occur. An eviction finalized without pruning keeps an empty child, which the node constructor refuses `EmptyNode` and the cold reader and the reference unfold refuse `Body(EmptyNode)`. | `radix_edits_equal_the_independent_rebuild` |
| RV-32 | On 128 generated manifests of up to 16 entries, one commit and one eviction each report exactly the oracle's work from the two search paths: loaded nodes and bytes, emitted nodes and bytes, reused child references and staged objects. Every object that appears is staged, at most one removed object lies beside the old path and none for a commit, at most $`w + 2`$ objects are loaded or staged, the emitted bytes are within $`(w + 2)\,\beta(w)`$ and every node within $`\beta(w)`$. A commit into the empty history loads 0, emits 2 and stages 1; one that splits a leaf root loads 1, emits 3 and stages 3; one whose split remainder equals the new leaf emits 3 and stages 2; an eviction that merges into the root loads 3, emits 1 and stages 1; one that empties the history stages exactly the empty node. At $`2^4`$ to $`2^{14}`$ entries with dense, gapped and aliased generations, a commit and an eviction report exactly the oracle's work within the same bounds, the V1 manifest has exactly $`96 + 56n`$ bytes, and it passes the path-local bound exactly when $`n \ge 9\,258`$. | `radix_edit_work_is_the_changed_path_under_geometric_growth` |
| RV-33 | On 128 generated manifests, a commit's anchor is byte for byte the model anchor of the committed history with count $`n + 1`$, the cold reader reads it back as the committed entries, and those entries form a valid V1 manifest with the target; an eviction's anchor is the model anchor of the evicted history and reads back as it. On two historical entries that share one leaf object, the committed entry's leaf is that object again: the CAS count, 3, is admitted, while the stored-leaf count, 2, is refused `CountMismatch` with 2 declared and 3 counted. | `anchor_steps_realize_retention_entry_steps` |
| RV-34 | On 96 generated manifests of up to 6 entries followed by 1 to 9 commits and evictions, the cold reader admits the image after every step: the anchor's root is the rebuild's, the entries, the metadata closure and its bytes are the oracle's, and the reference unfold, the memoized count and the depth-keyed count all give $`n`$; a step that empties the history appoints and stages the empty node, and some sequence does. An eviction that leaves the root with one child, finalized without the merge, is refused `CompressibleUnaryNode` by the cold reader, the reference unfold, the memoized count and the depth-keyed count, while the faithful merged root is admitted with the remaining entry. | `editor_images_are_admitted_by_the_cold_reader` |
| RV-35 | On 64 generated manifests of up to 12 entries, for one commit and one eviction, the metadata trial from the seeded predecessor expands exactly $`\lvert M \,\triangle\, M' \rvert`$ objects with 2 plus the out-degrees of those objects as references, stays within $`2w + 7`$ root events and expansions, reads exactly the $`\lvert M \setminus M' \rvert`$ departing objects from the store, ends with the inventory of $`M'`$ and the cold ledger's count for every object of $`M \cup M'`$, and moves the root multiplicity from $`A`$ to $`A'`$. At $`2^4`$ to $`2^{14}`$ entries with the three generation shapes, the bound holds for a commit and an eviction. Deactivating first expands exactly $`\lvert M \rvert + \lvert M' \rvert`$ objects with the same final inventory and roots, more than $`n`$ for dense and gapped histories. | `metadata_trial_work_is_the_changed_objects_under_geometric_growth` |
| RV-36 | On 128 generated manifests, the reader's occurrence counts equal the oracle's, the root multiplicity plus one per edge of a distinct closure object, and the seeded ledger equals the ledger that activating the anchor from an empty domain builds: the same counted objects, root multiplicities, metadata inventory and count of every object. On a history whose two aliased entries share one leaf, the leaf's occurrence count is 2, and evicting one alias keeps it at 1 with the successor's exact inventory, while a seed of ones drops it to 0 and undercounts the inventory by that object and its bytes. | `seeded_metadata_ledger_equals_the_cold_ledger` |
| RV-37 | On 128 generated manifests, the migration reads no object, loads no node, makes $`n + 1`$ mutations, and stages exactly the rebuild's objects and the model anchor of the history with the current entry inserted; its anchor equals a commit of the target on the V2 twin of the source; the cold reader reads it back; and both V1 readers refuse every staged object. An anchor that appoints the migrated image while keeping the old current entry current is refused `RetentionContainsCurrent` by the cold reader and by the V1 manifest constructor. | `v1_migration_is_the_canonical_rebuild_and_refused_by_old_readers` |

**Directory tests.** Four tests in `durable_publication.rs` run the writer
through real directories. They select the V2 writer with the test setter, and
track the expected history with `RetentionOracle` (`durable_publication.rs:4944`),
which keeps the entries without the editor and builds the expected anchor and
closure with the test model:

- `v2_writer_successors_appoint_the_retention_entry_step_across_restarts` refines
  `RV-33` and `RV-34`. Three successors over a migrated V2 source, with two
  restarts: each successor appoints the oracle's anchor, and cold admission adopts
  exactly the oracle's metadata closure. At each restart the V1-only reader
  refuses at the V2 descriptor under either writer and writes nothing, and the V2
  writer adopts the same authority and report.
- `v2_writer_migrates_a_v1_source_at_its_first_successor` refines `RV-37`. The
  V2 writer adopts a V1 source unchanged, migrates at its first successor and
  edits the migrated radix at its second. The V1-only reader refuses the
  migration transaction in recovery with `UnknownObjectDomain`. The V1 writer
  adopts the migrated directory with the same report and authority, refuses to
  extend it with `RetentionFormatNotWritable(V2)`, and leaves it unchanged.
- `v2_writer_checkpoints_carry_the_retained_union_and_the_metadata_closure`
  refines `RV-08` and `RV-24`. The writer's publication, the writer's compaction
  and the publication authority each publish a checkpoint of a V2 source with
  the same state and retained union, whose DATA is the retained union and the
  metadata closure. The pack that held the edited radix nodes is reclaimed, and
  each restart adopts the oracle's anchor and closure. A V1 source keeps its V1
  checkpoint under the V2 writer, and the V1-only reader adopts it.
- `v2_writer_requires_the_dual_reader_and_is_the_default` (named
  `v2_writer_requires_the_dual_reader_and_is_not_the_default` until the
  activation commit). An opened directory has the V2 writer and the dual reader.
  The V2 writer with the V1-only reader is refused `RetentionWriterUnreadable` on
  a V1 directory and in recovery on a V2 directory, neither refusal changes the
  directory, and the V1 writer is admitted under both readers.

`model_property_manifest_is_unique_and_exhaustive` now checks 187 bindings,
these eight among them. It requires each of the sixteen `Print Assumptions`
capstones of `RetentionV2Editor.v` to be cited by an RV row of the derivation
matrix, and the integrated runner compiles the theory expecting sixteen closed
capstones.

**Boundary.**

- Activation. The V2 writer became the default in the commit after this one,
  with a release note, because after a directory's first V2 successor an older
  binary either cannot read it or cannot extend it, and nothing exports it back
  to V1; `RV-28` now describes the V1 writer only
  ([activation](#retention-v2-writer-activation-2026-10-26)).
- Eviction has no production caller; the retention policy that evicts is task
  7756.
- The theory walks every edit of a batch from the root. `RadixEditor::apply`
  (`rewrite.rs:792-816`) instead resumes at the last cursor step that ends
  within the longest common prefix of the previous and the current key. That
  step's node path is the current key's prefix of the same length, so a walk
  from the root passes through the same node at the same position, and resuming
  there skips only nodes the batch has already opened. The theory does not
  mechanize this equivalence; the multi-edit batches of `RV-30`, `RV-31` and
  `RV-37` check it executably against the independent rebuild.
- The cold-checked publication re-admits every successor cold, so a successor
  costs $`O(n)`$ end to end on that path. Since task 11946 the trusted
  publication replaces re-admission on ordinary commits (the trusted successor
  publication section). Comparing that successor with cold admission after
  every step is task 7928 (the faulted trusted histories section), and the
  scaling evidence is the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29):
  thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history.
- Superseded radix nodes stay in their packs until compaction or the garbage
  collector reclaims them; the collector must treat the metadata closure of
  every retained anchor as live (tasks 7943 to 7945).
- The third domain adds 8 bytes to each digest's state in the ledger, and
  8 bytes to each root's multiplicities.
- The TLA+ models are unchanged: crash atomicity is the commit's WAL transaction
  (`RSpaceAtomicCheckpointAuthority`), and the old-reader fence is
  `RSpaceRetentionV2Upgrade`.

Cited works:

- D. R. Morrison, "PATRICIA — Practical Algorithm To Retrieve Information Coded
  in Alphanumeric", *Journal of the ACM* 15(4):514–534, 1968,
  [doi:10.1145/321479.321481](https://doi.org/10.1145/321479.321481), for the
  path-compressed radix tree, whose unary inner nodes the shape rules forbid.
- V. Leis, A. Kemper and T. Neumann, "The adaptive radix tree: ARTful indexing
  for main-memory databases", *Proceedings of the 29th IEEE International
  Conference on Data Engineering (ICDE)*, 38–49, 2013,
  [doi:10.1109/ICDE.2013.6544812](https://doi.org/10.1109/ICDE.2013.6544812),
  for the Node4, Node16, Node48 and Node256 child tables.
- J. R. Driscoll, N. Sarnak, D. D. Sleator and R. E. Tarjan, "Making data
  structures persistent", *Journal of Computer and System Sciences*
  38(1):86–124, 1989,
  [doi:10.1016/0022-0000(89)90034-2](<https://doi.org/10.1016/0022-0000(89)90034-2>),
  for path copying: an update copies its search path and shares every other
  node.
- G. E. Collins, "A method for overlapping and erasure of lists",
  *Communications of the ACM* 3(12):655–657, 1960,
  [doi:10.1145/367487.367501](https://doi.org/10.1145/367487.367501), for
  reference counting.
- R. C. Merkle, "A digital signature based on a conventional encryption
  function", *Advances in Cryptology — CRYPTO '87*, Lecture Notes in Computer
  Science 293, 369–378, 1988,
  [doi:10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32), for
  naming every node by the digest of its content, which makes identical
  subtrees one object.

The writer evidence in the verification record was taken at these sources:

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/RetentionV2Editor.v` | `878da37f174f3f8f8ffe46b206b345704617f350ef24387647ab21a42d9fd2b0` |
| `scripts/check-stack-safe-pda-proofs.sh` | `e54c18c4bb08abe690c66a72054a9569805ca58ab84ef4cece7f7edc9d23ae02` |
| `rspace++/src/rspace/persistence/retention/v2_writer.rs` | `a3af7d4dc6caf186aebced776eb6044378fc0e26f4b57060485322ebe2b609ab` |
| `rspace++/src/rspace/persistence/retention/v2_reader.rs` | `cc4b66d2a49e397b8093575d8fa147e7bbd1cbaac226f68f70910866c34d32f3` |
| `rspace++/src/rspace/persistence/retention.rs` | `e5971ed87b6973bdbc09fd02609507c2a8dc8af4459f8bb973df59e2782529de` |
| `rspace++/src/rspace/persistence/rewrite.rs` | `638c62e1299377d50480022b0e57a25b2a566cfe57e1580bf14d29a4216fde73` |
| `rspace++/src/rspace/persistence/object.rs` | `b1bd27345892c3a3c0a1aba611b33e93e57a2fcbdb7c7d70fe4eae638c93937a` |
| `rspace++/src/rspace/persistence/store.rs` | `3f79a99fa14ca15c36269e21ff17e32aa7d64ac3c14b470f8d2fc0f40b694a29` |
| `rspace++/src/rspace/persistence/closure.rs` | `768b54c28e8a0e4c68826ff67d8510228345a62bef7d3559ca92027b977feec6` |
| `rspace++/src/rspace/persistence/closure/ledger_trial.rs` | `b5d887bf057c63d13b6b37a158d2ceaec29139192a521044d26a01be9c65ffc3` |
| `rspace++/src/rspace/persistence/checkpoint.rs` | `c3805f569d926d3a64a8ff08db73172a056b200aa796251e66ee5dce3c5547ff` |
| `rspace++/src/rspace/persistence/transaction.rs` | `7361e447a0114837dfb3f24f57a63e4d3fbbbba7ccda19f9dd48bda78f02c21b` |
| `rspace++/src/rspace/persistence/wal/codec.rs` | `02a30b3a057d635dd6bcfa91cc4f4b40635300a1a5b5340dd45dafed5181e427` |
| `rspace++/src/rspace/persistence/durable/io.rs` | `4cc5db7ef8b2fed0d952cdace30495f61cb6673f22e3af4b790df435ccc47157` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `d0c10d21705bfe5889a7aa49e4f9d5ee5b97579d46bcb6c1f80300de55eb02c1` |
| `rspace++/src/rspace/persistence/durable/publication.rs` | `6b22101d11e5d795a8f2843bf8e94695b07dafd1bbe3f17e7e9d981c5396ab37` |
| `rspace++/tests/persistence/retention_v2_editor_properties.rs` | `6d3c70af410a660afcf748e146c2f151e3f6ab14ed5c07429c72607d618aba0f` |
| `rspace++/tests/persistence/retention_v2_format_properties.rs` | `fba70440c3b860db27ad705a09f41ad663805a44d10870b57069a9c232468d1c` |
| `rspace++/tests/persistence/durable_publication.rs` | `4203049158834abb07fbff5787d6516bf951049806d1326bcb806cc1993bc8d0` |
| `rspace++/tests/persistence/cow_reachability_ledger_properties.rs` | `0e3317fa328a823147b829be8dc6be5943ca5132293bbec5707c875ab47e28bd` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `e5aebfbb8bb7d655fd334a17abb3492e977d8e4eff891aba7fd0ace101100993` |
| `rspace++/tests/persistence/property_support.rs` | `46573d3b4365f648d39bb1512dafb8e89bf2e9350e3b68662637cdf010946aa4` |
| `rspace++/tests/persistence/mod.rs` | `ffda73dca27f38fbf177a8358635a9b10156313a347d97f47eb84dc17d8462c9` |

### Retention V2 writer activation (2026-10-26)

The writer section above kept the V1 writer for every opened directory, because
the first V2 successor of a directory is a one-way change for that directory,
and the user decided on 2026-10-26 to activate the V2 writer after that change's
gates. Its full gate passed at `e6c784d90`, and the activation commit makes V2
the default: `DurableDirectoryV1::open` selects the V2 writer beside the dual
reader (`durable/io.rs:839-840`). That constant is the whole production change,
apart from one extraction that changes no behavior: `recover_read_authority`
(`durable/mod.rs:502-512`) opens its directory and calls the new
`recover_read_authority_from_directory` (`durable/mod.rs:514-531`), with which a
test admits a read authority under a writer it selected.

![A durable directory under the default V2 retention writer. A directory starts as a V1 directory, in amber, which appoints a V1 retention manifest: every binary opens, admits and checkpoints it, and binaries built before the activation extend it with V1 successors, so it stays V1. Its first successor under the default V2 writer, in purple, migrates its history once, as one sorted batch from the empty radix, and appoints an anchor; this step is one-way. The V2 directory, in green, takes path-local commits and V2 checkpoints from this and later binaries. Opened by a binary that reads V2 but writes V1, built at 21912f662 or e6c784d90, in grey, it is adopted, but a successor is refused RetentionFormatNotWritable(V2) and a checkpoint SourceRetentionFormatNotWritable(V2). Opened by a binary built before the dual reader, in red, it is refused in physical recovery, with InvalidDomain at a V2 checkpoint descriptor or UnknownObjectDomain at the first anchor or radix node of a migration transaction. Both older binaries write nothing, so the directory stays V2, and no transition leads back to V1](figures/retention-v2-writer-activation.svg)

**Coherence.** By its appointed format, an opened directory is row 2 or row 4
of the writer and reader table above. Cold admission refuses only the V2 writer
with the V1-only reader (`RetentionWriterUnreadable`,
`durable/admission.rs:1855-1863`), and no production path selects the V1-only
reader. Admission reads the writer only for that refusal and keeps it in the
admitted source (`:2072`), so every admission report is the same under both
writers. The V1 writer and the V1-only reader are now selected only by the
test-build setters (`durable/io.rs:852-856`, `:864-868`), where they stand in
for older binaries: the V1-only reader with the V1 writer for a binary built
before the dual reader, and the dual reader with the V1 writer for one built
with the reader or the writer of the two sections above. Nothing is removed:
the V1 writer and the V1-only reader remain as those stand-ins.

**Release note.** The first V2 successor of a directory, which migrates its V1
history once, is one-way for that directory:

- a binary built before the dual reader (before `21912f662`) refuses the
  directory in physical recovery and writes nothing: `InvalidDomain` at a V2
  checkpoint descriptor, or `UnknownObjectDomain` at the first anchor or radix
  node of a migration transaction;
- a binary that reads V2 but writes V1 (built at `21912f662` or `e6c784d90`)
  adopts the directory but refuses to extend it
  (`RetentionFormatNotWritable(V2)`) or to checkpoint it
  (`SourceRetentionFormatNotWritable(V2)`), and leaves the directory unchanged;
- nothing exports V2 back to V1.

Opening, admitting and checkpointing a V1 directory before its first successor
keep it V1, so installing the new binary is not itself the one-way point. These
statements rest on the stand-ins, whose V1-only reader is the V1 classifier
(`RV-20`, `RV-21`), and on the directory refusals of `RV-27`, `RV-28` and the
directory tests of the writer section; no older build was run. The
consensus-change register records the note as a continuation of CBR-053.

**RV-28.** `RV-28` now describes the V1 writer only. Its test selects that
writer explicitly, and the journal owner's checkpoint runs over a directory
opened with it. The V2 writer's successors and checkpoints are `RV-33` to
`RV-37` and the directory tests of the writer section.

**Executable adaptations.** With only the default flipped, 8 of the 457 selected
persistence tests failed (engineering journal J-250). None exposed a production
defect; each encoded the old default:

| Test | What the default changed | Adaptation |
|---|---|---|
| `incremental_inventory_tracks_replacement_removal_sharing_and_reuse` (`SI-01`, `SI-03`) and `incremental_inventory_matches_cold_admission_at_every_history_step` (`SI-02`) | the V2 writer seals anchors and radix nodes, which the semantic oracle store refuses with `UnknownObjectDomain` | both writers; the metadata objects are kept out of the oracle store, and under the V2 writer the sealed anchor and the appointed closure are checked against `RetentionOracle` |
| `v2_admitted_sources_refuse_every_successor_before_durable_mutation` (`RV-28`) | the default writer extends and checkpoints a V2 source | selects the V1 writer, also for the journal owner's directory |
| `v2_cold_admission_matches_v1_admission_and_charges_metadata_reads_exactly` (`RV-25`), `v2_restart_fixtures_are_refused_by_old_readers_and_adopted_by_dual_readers` (`RV-27`), `v2_writer_checkpoints_carry_the_retained_union_and_the_metadata_closure` | the V1-only reader met the default V2 writer: `RetentionWriterUnreadable` | cold admission with the V1-only reader, the stand-in for an older binary, also selects the V1 writer |
| `v2_writer_migrates_a_v1_source_at_its_first_successor` | its V1 writer was the default | selects the V1 writer |
| `v2_writer_requires_the_dual_reader_and_is_not_the_default` | asserted the V1 default | asserts the V2 writer and the dual reader; renamed `v2_writer_requires_the_dual_reader_and_is_the_default` |

**SI under both writers.** `SI-01` to `SI-03` do not depend on the writer: they
compare the sealed target, the catalog's incremental inventory and cold
admission after a restart with an oracle store of the semantic objects. They
now run under both writers, from both sources, on the fixed history and on
every generated one. That store admits only semantic objects, so under the V2
writer it receives every sealed object except the anchor and the radix nodes.
The retained states are then the current state and the history of
`RetentionOracle`: the sealed anchor must equal the oracle's, and after each
restart cold admission must appoint the oracle's anchor and metadata closure
(`assert_appoints`). The retained union stays the semantic union, so the
comparison with the catalog's retained bytes is unchanged; keeping the metadata
out of it is `SI-03` for V2 metadata. Under the V1 writer the checks are as
before, and no metadata object may be sealed.

**Boundary.**

- The durable backend is still dormant: no running node opens a durable
  directory, and the authority backends are `Memory` and `Lmdb`. The release
  note binds the backend's activation, task 7931, which must re-enter the
  consensus-change register.
- The writer section's cost remarks now apply by default: the first successor of
  every V1 directory migrates its whole history once, and below roughly two
  hundred entries a V2 commit can write more bytes than the V1 manifest it
  replaces (derived, not measured). The
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29)
  measures the median V2 step at 1,538 to 4,088 bytes for 256 to 65,536
  retained states, so the V1 manifest exceeds it from 26 to 72 entries on.
- The TLA+ and Rocq sources are unchanged. `RSpaceRetentionV2Upgrade` lets a
  publisher choose either format until the first V2 publication and only V2
  after it (`AppointableFormats`, `RSpaceRetentionV2Upgrade.tla:81-93`). A
  publisher that always chooses V2 therefore has a subset of the model's
  behaviors, so every invariant the model checks holds for it; no theorem names
  the default.
- Earlier sections that say the V2 writer is not yet the default record those
  changes as they landed and are not rewritten.

The activation evidence in the verification record was taken at these sources:

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/io.rs` | `7811573c32e8cf8942eed3530c52b3f7a620f664880e517204ac258ffda05cf7` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `b0a55402c7dae5369607a0de15a7771a6601d50a55a19ea3343074de60f5021d` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `d0c10d21705bfe5889a7aa49e4f9d5ee5b97579d46bcb6c1f80300de55eb02c1` |
| `rspace++/tests/persistence/durable_publication.rs` | `4a7e31cbfd7b3b2c5ce78369fc4c6e96383dc2a628b5a2cc0b3a8ba2998e190a` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `e8dd5af97091573848fd1b1e3fc37263a4bfc5f9b14874ba5093932775dcd01a` |
| `rspace++/tests/persistence/retention_v2_format_properties.rs` | `fba70440c3b860db27ad705a09f41ad663805a44d10870b57069a9c232468d1c` |
| `rspace++/tests/persistence/property_support.rs` | `46573d3b4365f648d39bb1512dafb8e89bf2e9350e3b68662637cdf010946aa4` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `e5aebfbb8bb7d655fd334a17abb3492e977d8e4eff891aba7fd0ace101100993` |

### Trusted successor capability (2026-10-27)

Task 7927 replaces the physical recovery and cold admission that follow every
ordinary incremental publication with a *trusted successor*, as engineering
journal J-067 rules: the publication returns the successor core that it derived
before writing, without re-reading the store. This section records the first of
the task's five slices, task 11942: the formal models, and the test-model
extraction of every property that the production refinement must satisfy. The
slice changed no production code: every incremental publication then ran
physical recovery and cold admission, and every TS row below was a test-only
model extraction. The durable backend stays dormant until task 7931. The other
slices are the exact shape summary (task 11943), the split of the authority
core from cold-checked admission evidence (task 11944), the trusted bind under
an explicit hot policy (task 11945), and trusted publication with the switch
of the journal owner (task 11946), which completed this section: every TS row
now checks production code (the trusted successor publication section).

**The successor rule.** Let $`s`$ be a predecessor authority and $`t`$ the
successor that a sealed receipt names. Write

- $`A_s`$ for *$`s`$ is an exact authority*: its current, retained and metadata
  ledgers are the coherent quiescent ledgers of its retention entries and its
  metadata roots, its retained-state maxima are the componentwise maxima of the
  cold per-state inventories, its validity token is its current manifest, and
  its object index is the cold rebuild of its index records;
- $`D_{s,t}`$ for *the receipt is delta-sound*: it turns a semantically valid
  base into a valid target inside the semantic object universe, and every
  metadata root that it adds or removes lies in the metadata universe;
- $`R_{s,t}`$ for *the trusted publication of the receipt returned $`t`$*;
- $`C_{t,L}`$ for *cold execution evidence*: physical recovery and cold
  admission of $`t`$ succeeded under the incoming admission limits $`L`$.

J-067 keeps the authority apart from the cold evidence:

```math
A_s \wedge D_{s,t} \wedge R_{s,t} \;\Longrightarrow\; A_t
\qquad\text{and}\qquad
A_t \;\not\Longrightarrow\; C_{t,L}.
```

The implication is the theorem `trusted_successor_rule`. The non-implication
has two witnesses: cold demand is not a function of the inventory summary that a
trusted core carries (`cold_demand_is_not_a_function_of_the_inventory_summary`),
and the TLA+ model reaches a trusted success under incoming limits that cold
admission of the same catalog refuses (`NeverTrustedReady`). A trusted successor
therefore never carries a cold report, and a fresh checkpoint from a trusted
core measures its cold demand anew. Delta soundness is a premise of the rule:
the theorems that need it take it as an explicit argument. Cold startup, import
and fresh-checkpoint preflight stay exact.

**The publication.** Both models describe one publication of a sealed
transaction. Its *bind* checks the receipt's base token and profile against the
predecessor core, runs one failure-atomic ledger trial per inventory domain, in
the order current, retained, metadata, on the shared three-count ledger, and
checks the final ceilings of the hot policy. It writes nothing. The sealed
transaction carries the publication mode, trusted or cold-checked, and the
policy. The *preflight* requires the call's mode, and in the trusted mode its
policy, to equal the sealed ones, and the catalog to hold the predecessor image
that the core's token names. The trusted mode then

1. assembles the successor's object index in memory, so that a duplicate or
   conflicting record refuses before any write;
2. appends the transaction, with the same writes as the cold-checked mode;
3. stages the successor token and pins the store;
4. swaps the catalog slot and syncs the catalog directory;
5. resolves the reclamation sidecar;
6. adopts the successor core after an exact re-read of the catalog image.

The cold-checked mode instead runs physical recovery, which also resolves the
sidecar, and then cold admission under the incoming limits. A refusal precedes
the first durable write. A failure before the slot write is *definitely not
published*, a failure during it is *indeterminate*, and a failure once the
catalog file is durable is *published with an error*.

**Artifacts.** Four artifacts implement the slice:

- the Rocq theory `formal/rocq/stack_safe_pda/theories/TrustedSuccessorCapability.v`
  (module `TrustedSuccessorCapability`), with eleven positive capstones and five
  negative controls, all sixteen closed under the global context. `_CoqProject`
  lists it, and the integrated runner (`scripts/check-stack-safe-pda-proofs.sh`)
  compiles it and requires that closed count;
- the TLA+ model `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.tla`
  (Lamport, 1994). Its base configuration checks nine invariants, each of its
  eight negative-control configurations must violate its named invariant, and
  each of its six non-vacuity witnesses must be reached; the integrated runner
  checks all fifteen configurations;
- an extension of the journal owner's model `RSpaceJournalPublicationOwner.tla`
  with tightened cold limits, the empty checkpoint, and no cold evidence on
  trusted adoption: two invariants (`ColdRecoveryHonorsIncomingLimits`,
  `TrustedSuccessorHasNoColdEvidence`), two negative controls, and two witnesses
  (`NeverEmptyCheckpointUnderTightenedColdLimits`,
  `NeverAwaitingAckRetryUnderTightenedColdLimits`), which
  `scripts/check-rspace-journal-owner-model.sh` checks;
- the test model `rspace++/tests/persistence/trusted_successor_properties.rs`,
  with the four owner rows bound in `durable_rspace_authority.rs`.

**The test model.** The test model has three layers.

1. A transliteration of the TLA+ next-state relation, action by action, in
   which a `CASE` is decided by its first true guard, as TLC decides it.
   Breadth-first exploration from all 64 initial scenarios must reach exactly
   the 588 distinct states that TLC reaches; every reachable state must satisfy
   the nine invariants; every witness must fail in some state; and each of the
   eight negative controls must violate its named invariant. A bridge maps each
   trace of the second layer, with the catalog image that persisted, to a model
   behavior, so every generated fault is replayed in the model.
2. A mirror of the theory's bind and publication, built on the composed ledger
   trials of `incremental_reachability_properties.rs`, with four
   negative-control variants: skipping the retained trial, skipping the
   metadata trial, ignoring the sealed policy at the preflight, and appending
   before the trials. A trace records every decision and every durable write.
   A trusted trace passes the thirteen hooks
   `FORMAL_PUBLICATION_PHASE_ORDER[2..=14]` (`durable_publication.rs:74-93`); a
   cold-checked trace also passes the physical-recovery hook and the two
   semantic-admission hooks, sixteen in all.
3. A mirror of the theory's journal owner, with the empty checkpoint, the
   awaiting-acknowledgement retry, recovery and the trusted checkpoint, and
   probes that count cold checks and publication attempts.

Histories are generated over a semantic DAG of one to seven objects of zero to
four bytes each and a metadata DAG of one to four objects of one to four bytes
each. Every object has up to three children, two in the metadata DAG, among the
lower-numbered objects, with repeats allowed, so every graph is acyclic. A
history starts from a cold-admitted core, with a current manifest, a metadata
anchor, a shape summary, a profile from 0 to 2 and up to two index records, and
makes one to four commits. Each commit chooses a target manifest, a metadata
anchor, a shape summary and up to three index records, under a cursor schedule
of one to eight choices. The *exact policy* of a step sets every trial work axis
to its measured value and every final ceiling to the successor's value; its 32
axes are the 21 work axes (seven per domain) and the 11 state ceilings.

| ID | Required property and negative-control boundary | Test |
|---|---|---|
| TS-01 | On 256 generated histories, every trusted successor is an exact authority, and it equals, field by field, the core that cold admission derives from the same retention entries, metadata roots, shape, profile and index records. Its entries are the predecessor's with the receipt's commit, its metadata roots are the predecessor's with the receipt's delta applied, its retained-state count is one more, its validity token is the receipt's target, its profile is the predecessor's, and its index records are the predecessor's followed by the receipt's plan. The transliteration satisfies `AdoptedSuccessorIsExactCatalog` in all 588 states, and adoption without the catalog re-read violates it. | `trusted_successor_matches_cold_admission_at_every_history_step` |
| TS-02 | On 256 generated pairs of up to five prefix records and up to five appended records under capacities from 0 to 7, appending to the prefix's index gives exactly the cold rebuild of all the records: the same records, the same count $`n`$ and the tail $`64 + 72n`$, or the same refusal (a duplicate record, a digest bound to two references, or a count above the capacity), and all four outcomes occur. On 64 generated histories, a plan that repeats an existing record, conflicts with one, repeats a fresh record or conflicts with a fresh record, and a plan for an index already at its capacity, are refused at assembly with the cold rebuild's record and reason, definitely not published, before any durable write. | `trusted_successor_index_frontier_equals_cold_recovery` |
| TS-03 | On a fixture that commits a new manifest and replaces the metadata anchor, skipping the retained trial keeps 2 retained objects where cold traversal counts 3, and skipping the metadata trial keeps 1 metadata object where it counts 2. On 128 generated histories, at every step the production order prepares exactly the composed trial's evidence and successor; the metadata trial gives the same result after the two semantic trials as on the predecessor; lowering any positive measured trial axis of any domain by one refuses at that domain and axis, as the composed trial does, and both keep the predecessor; and a publication under the exact policy lowered by one on a work axis is refused at the bind after exactly the trials up to that domain, with the predecessor unchanged. Refusals in all three domains, and divergence of both skipped-trial variants from cold traversal, occur. | `trusted_bind_runs_domains_in_order_and_keeps_the_predecessor_on_first_refusal` |
| TS-04 | Each of the seven ceilings (state objects and bytes, retained states, logical nodes, endpoints, logical path bytes and payload bytes) accepts at the successor's value and refuses one below on its own axis. On 64 generated histories, the exact policy binds the cold successor's ledger and maxima and seals that policy. Lowering any positive one of its 32 axes by one is refused with the expected trial or ceiling refusal, by the bind and by a publication, which is definitely not published with only the bind in its trace, writes nothing, and leaves the predecessor unchanged; a state ceiling that the current state attains is refused by the current trial, which runs first. Ceiling and trial refusals both occur. The transliteration satisfies `TrustedRefusalPrecedesAllMutation` and reaches a trusted policy refusal; the late-refusal and append-before-trials controls violate it. | `trusted_policy_accepts_exact_and_refuses_one_below_on_every_axis` |
| TS-05 | On a swap of one root for another, the union peak is 2 objects and the final inventory 1: a union budget of 2 with a final limit of 1 prepares, the control that checks the union against the final limit refuses on union objects, and a final limit of 0 refuses on final objects. On 128 generated histories, every domain's measured union peak equals the cold inventory of its base roots plus the added roots. Where a domain's peak exceeds its final size, lowering its union budget to the final size refuses on that union axis, and such steps occur; under the exact policy the control refuses on a union axis that exceeds the final limit and otherwise agrees with the faithful trial. | `trusted_policy_permits_budgeted_union_scratch_above_final_state_size` |
| TS-06 | On a fixture, a publication called with a policy other than the sealed one is refused `TrustedPolicyMismatch` by the preflight before any durable write; the control that ignores the policy publishes a successor above the changed policy's union ceiling, which the faithful bind refuses under that policy. On 64 generated histories, the sealed exact policy passes the preflight, and changing any of its 32 axes by one, up or down, is refused `TrustedPolicyMismatch` with only the preflight in the trace. A cold-checked binding is refused `PublicationModeMismatch` by the trusted preflight, and a trusted binding by the cold preflight; the cold binding passes its own preflight and refuses each of its three demand axes raised by one (`AdmissionPolicyMismatch`). The transliteration satisfies `PublishedPolicyIsSealedPolicy`, and the changed-policy control violates it. | `trusted_policy_and_mode_cannot_change_after_preparation` |
| TS-07 | In the transliteration a stale authority is refused without a durable write while the catalog holds a foreign image, and a re-read mismatch is reachable; the append-before-trials control violates `TrustedRefusalPrecedesAllMutation`, and in the test model it swaps the catalog for a successor that the faithful bind refuses. On 128 generated histories, a receipt with a foreign base, with an earlier validity token, replayed against its own successor, or with another profile is refused at the receipt check under each of the six fault settings (none, a pin failure, a catalog mismatch, an interrupted catalog write, a directory-sync failure and a sidecar failure), with only that check in the trace and the authority unchanged. Under the unbounded, exact and one-below policies and each fault setting, every refused publication writes nothing, and every first durable write follows the trusted safe prefix with equal bind and publication policies. Stale and replayed receipts and writing publications occur. | `trusted_binding_refuses_foreign_stale_and_resequenced_authority_before_mutation` |
| TS-08 | On a four-object fixture, two states with the same inventory of 2 objects and 15 bytes have cold demands of 2 and 1 references. Under limits of 1 reference, preflight with the measured demand accepts the single-reference state and refuses the shared-child state, which preflight with the predecessor's demand and preflight with the summary's demand both accept. On 256 generated histories, the measured demand of every successor equals that of a fresh cold traversal of its current manifest, and preflight accepts at it and refuses one below on each positive axis; each of the two controls accepts a state that the measured preflight refuses on some generated step. | `fresh_checkpoint_from_a_trusted_core_measures_new_cold_demand` |
| TS-09 | On 128 generated histories, every trusted publication passes exactly the thirteen trusted hooks, with no physical recovery and no cold admission. The cold-checked publication of the same receipt under the successor's measured demand returns the same core with that demand as its evidence, passes the sixteen cold-checked hooks, and runs physical recovery and cold admission once each. The transliteration satisfies `NoPhysicalRecoveryOnTrustedSuccess` and `TrustedSuccessorHasNoColdEvidence` and reaches a trusted success under tight cold limits (`NeverTrustedReady`); the trusted-recovery and fabricated-evidence controls violate the two invariants. | `trusted_publication_performs_no_physical_recovery_or_cold_admission` |
| TS-10 | On 128 generated histories, for each reclamation sidecar (absent, deferred because it is pinned, and reclaimable), the trusted and cold-checked publications of a step make the same durable writes, ending with the same sidecar resolution (nothing, keeping the intent bytes, and removing the superseded files), and both return the cold successor; the three sidecars give pairwise different writes. The transliteration satisfies `AdoptionFollowsDirectorySyncAndResolution`, and adoption before the directory sync violates it. | `trusted_and_cold_checked_publication_write_identical_bytes` |
| TS-11 | The transliteration satisfies all nine invariants and reaches an indeterminate catalog write, a published failure of the sidecar resolution and a staging failure before the swap. Every one of the thirteen trusted hooks has a failure behavior, every such behavior replays in the model to its production class (definitely not published from the prepare write through the swap's verification, indeterminate at the slot write, published with an error from the catalog file sync on) with every invariant intact, and the fault-free path replays to ready. On 64 generated histories, every fault setting of every step, with the old, torn or new catalog image persisted, replays to the publication's outcome class; a changed policy replays to the policy refusal; and a retry publishes the same successor. At every trusted hook, every interrupted pack image (each prefix of the transaction between its synced and its written end, with and without a torn tail, under each catalog image that the hook allows) recovers the appointed prefix: the predecessor's entries under an old, torn or foreign catalog, the successor's under the new one. A retry from that prefix rebuilds exactly the uninterrupted pack. | `trusted_incremental_publication_recovers_and_retries_every_phase` |
| TS-12 | On 256 generated owner requests with an empty journal, a checkpoint under the incoming cold limits and under tightened ones (all zero, or loose with one axis at zero) accepts the current authority with no change, makes no cold check and attempts no publication. | `owner_empty_checkpoint_under_tightened_cold_limits_performs_no_cold_check` |
| TS-13 | On 256 generated owners awaiting acknowledgement, reconciliation and the checkpoint retry, which reconciles before it checkpoints, acknowledge the pending successor exactly once and make no cold check, with the same outcome under the incoming and the tightened cold limits. Recovery, by contrast, consults the cold limits once: it accepts the recovered authority under loose limits and refuses under zero limits. | `owner_awaiting_ack_retry_under_tightened_cold_limits_acknowledges_once_without_cold_check` |
| TS-14 | On 128 generated histories, for every positive axis of the successor's measured cold demand, the cold-checked publication under limits one below on that axis runs cold admission and is refused after publication with the measured demand, while the trusted publication of the same receipt returns the successor; such axes occur. The transliteration satisfies `ColdModeSuccessIncludesColdAdmission`, reaches cold success (`NeverColdReady`) and trusted success under tight limits, and the skipped-admission control violates the invariant. | `trusted_success_does_not_imply_cold_admission_under_incoming_limits` |
| TS-15 | On 64 generated histories, for every positive axis of the exact policy lowered by one, the owner's checkpoint of a captured journal is refused and stays ready, emits no event, makes no cold check, and makes exactly one publication attempt, which writes nothing. The transliteration reaches a trusted policy refusal. | `owner_trusted_policy_refusal_leaves_files_journals_and_authority_unchanged` |
| TS-16 | On a four-object fixture, two retention histories whose maxima are both 1 object and 3 bytes have maxima of 1 object and 3 bytes, and of 1 object and 2 bytes, after evicting the same entry, so the maxima cannot follow an eviction without a recomputation. On 256 generated histories, every successor's maxima equal the componentwise maxima of the cold per-state inventories, and the componentwise maximum of the predecessor's maxima and the target's inventory; per-state preflight accepts at the maxima and refuses one below on each positive component. | `trusted_retained_state_maxima_equal_cold_requirements` |
| TS-17 | On a corner chain of seven states and on 64 generated chains of two to five states, every successor's shape summary, derived from its predecessor's summary, the mutation batch and the rewrite report, equals field by field the summary that the recording cold walk measures on the target: logical nodes, endpoints, the entries of each namespace, source payload bytes and the key-length multiset, hence the maximum logical path. Each target is also checked against the independent rebuild of the model state, and the recording walk returns exactly the report of the plain walk. Generated states hold datums of four lengths on three channels that share one source channel, so their subtrees repeat, and continuations of three bodies in four channel groups, one of them with two channels, each with a natural or an independent source and the joins that its group requires; one batch can insert, delete and update entries of all three namespaces. The corner chain shares subtrees, so the walk counts more logical nodes than node objects; removes a hub join and updates its sibling, so the rewrite absorbs an edited child into its parent and emits more nodes than it places, the case that counting emitted minus loaded nodes gets wrong; deletes the unique longest key, which lowers the maximum; deletes one of two tied longest keys, which keeps it, and then the other, which lowers it; replaces a continuation's source; and empties the state. Through durable publication, every published successor's cold admission and every restart admission equal the derived summary. The companion test `shape_summary_successor_refuses_inconsistent_batches_and_reports` refuses a report whose mutation count differs from the batch, a logical-node underflow, the release of a key length that the multiset does not hold, and a source reference shorter than the 64-byte blob envelope. | `trusted_shape_summary_matches_cold_semantic_walk` |
| TS-18 | On 256 generated histories, every successor's validity token is the receipt's target and its profile is the predecessor's; the bind refuses two foreign bases (`ForeignBase`) and two foreign profiles (`ProfileMismatch`), and refuses the next step's receipt against the predecessor whenever their tokens differ, which occurs. | `trusted_validity_is_bound_to_the_exact_predecessor_and_receipt` |
| TS-19 | On 256 generated histories started from a V1 source, which has no metadata root, only the first successor migrates: its receipt removes no root, and every later receipt removes the previous anchor. Every successor's metadata roots are its anchor alone, its metadata ledger is the cold traversal of that anchor, and it is an exact authority. Migrating a migrated source a second time keeps the old anchor counted and diverges from the cold seed; later extensions occur. | `trusted_successor_migrates_a_v1_source_once_and_matches_the_cold_seed` |

`model_property_manifest_is_unique_and_exhaustive` checks these nineteen
bindings. It reads the TLA+ model with its base configuration and the two Rocq
theories, `TrustedSuccessorCapability.v` and `ExactStateShapeSummary.v`, as part
of its sources. It requires each of the nine base invariants and each of the
thirty-two `Print Assumptions` capstones of the two theories to be cited by a TS
row of the derivation matrix, every witness that a TS row cites to have its
configuration and to be checked by the integrated runner, and the runner to
compile both theories. The journal owner's model stays outside the manifest, as
the journal-owner rows do: the qualifications of TS-12 and TS-13 name its two
witnesses, which its own checker runs. TS-17, the shape summary of task 11943,
is described in the next section.

**Boundary.**

- These results hold for the models and the test model. TS-17 checks
  production code (next section), and since task 11945 the test functions of
  TS-01, TS-03 to TS-07, TS-16, TS-18 and TS-19 also check the production bind
  (the trusted successor bind section), so their bindings are unchanged. Since
  task 11946 the test functions of TS-02, TS-08 to TS-15 and TS-17 also check
  the production publication and journal owner, and those of TS-03, TS-04,
  TS-06 and TS-07 the publication halves (the trusted successor publication
  section).
- The maxima are exact only while retention appends
  (`retained_state_maxima_is_cold_componentwise_max`); an eviction needs a
  recomputation (`eviction_requires_recomputation_counterexample`), which is the
  concern of task 7756.
- No scaling is claimed here; the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29)
  measures it (thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history,
  with gate G4 failing on allocated bytes). Task 11946 checks the publication's
  faults in-process on real files; crossing generated histories with
  failpoints at every hook is task 7928 (the faulted trusted histories
  section).
- Damage to the catalog after adoption is the trade that J-067 accepts: the next
  cold boundary finds it. The re-read before adoption (step 6 above) reports
  damage that precedes it as published with an error.

Cited works:

- K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing
  of Haskell programs", *Proceedings of the Fifth ACM SIGPLAN International
  Conference on Functional Programming (ICFP '00)*, 268–279, 2000,
  [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266), for the
  generated properties.
- L. Lamport, "The temporal logic of actions", *ACM Transactions on Programming
  Languages and Systems* 16(3):872–923, 1994,
  [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726).

The trusted successor evidence in the verification record was taken at these
sources. The production sources are those of the parent commit, which this
slice does not change.

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/TrustedSuccessorCapability.v` | `a1319b94208910b243967fe272c4c5da88e0a098c68b507fd3db092f15d9a579` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.tla` | `659975b286d258d374c9a1f15b02675a25bd1c3963cdeb5f458d8c52362d7ea4` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.cfg` | `c86314b6494792e1711951c52a482893389574f87c14e84546b0e7678ee557df` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationLateTrustedRefusalMutant.cfg` | `e506bb205c3997796f98291ee674ce23138b9b40f80c4ae40b3f7fd23b89d153` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationFabricatedColdEvidenceMutant.cfg` | `09f299e54d1efe4e11578d29e115789ea186f174e25eac71d94ba93ea3f1158c` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationChangedPolicyMutant.cfg` | `df1af8eea5ad1a27a79f1a024141fb5e3a4e61ac62396ffbdc5032a7972ad906` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationPublicationBeforeTrialsMutant.cfg` | `6c705c540d4fbc18834bb18d8b28790647dd5bf7dbb017275ffe2708b483dd25` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationTrustedRecoveryMutant.cfg` | `2d175d58199554c3ab74edaa155cb17fb2f7077e094a806ab860a567d3f7f593` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationEarlyAdoptionMutant.cfg` | `4b693def03869b41e020eec93afb6998cce2934c8360bdebd79d533e840dc0ef` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationWrongCatalogAdoptionMutant.cfg` | `66629356c4f6e0e4ae31cea1f3e79a8f197fc0549de3017a6f02a125f6157197` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationSkippedColdAdmissionMutant.cfg` | `5a48d569c7bfb779b31e043c805c9c5284f25e650a06066d7d0c1fe9ad593c91` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverTrustedReady.cfg` | `09103f34e95e749a20775c5cf50ab85aa0470ae9faab0de24968280e132411e5` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverColdReady.cfg` | `f6e212351baae6426ceaedca7c7f5de47069287837988183b0e510ba4b8af027` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverTrustedRefusal.cfg` | `906b06db17a39a151f72644fbb6fc5d7bca3f55967831520078d7066370ca29a` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverPublishedResolutionFailure.cfg` | `4e155535373706f2209655cb883be985ff678a9acc023db126d4a4c8cb4786f4` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverStageFailureBeforeCas.cfg` | `58cceea4ee9b5b073623b1548318a9cc7660362f8e2c72403d6a7cbde9487e10` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublicationNeverIndeterminate.cfg` | `fd653a1dfbcb5d660948eb57752a2ff735e871e317de04a5a5cf86f7372e6ac2` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla` | `bacb3ac1e05f23a3b6e01b263aa1cb8556f21ca04775fe2be9d320f3d4a88ed0` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.cfg` | `d14a572fe87c35e54cc29d99b721e2f5968c267537b7d111e3e245e01dbdd6a3` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerColdLimitIgnoredMutant.cfg` | `3b1923659d581a7c3c95d713b3503a9cb32b3d137c6005dd6435e9c05a8a3518` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerFabricatedColdMutant.cfg` | `e5f2fe985b1474abf0d8d781423d869064ef5a854ea761befe3f1fdc9ffaa317` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerNeverEmptyCheckpointUnderTightenedColdLimits.cfg` | `75e86f27aaf1a59add53d1cc55ddd2566a55426f0b6f56f65449d85eb6fe5b98` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerNeverAwaitingAckRetryUnderTightenedColdLimits.cfg` | `dcfe841466e135afdf9967098cc32c745958cde26c3bf45c4e91fb34c5cc58ef` |
| `scripts/check-rspace-journal-owner-model.sh` | `ba1009d1c6bea5a106f67f308e264e29cd811db74c3386691fac151a35a345ca` |
| `scripts/check-stack-safe-pda-proofs.sh` | `87ab5ce4cd1d047e1aa7964791c40bbe4639a0221f1e4ffc21fab16b9af603db` |
| `rspace++/tests/persistence/trusted_successor_properties.rs` | `14c95644aecd4094bbf6a32e896ee0c66221a85bf34482a4c3379c7d38ea772e` |
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `a8fef80b962ba87aeae939cd1e3eedfe3632448166478f8de91fc439960913bd` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `a71cfde7b9c82aef3bdd44b7b972652ee76892028f7225ee7430cc4d4afd09a4` |
| `rspace++/tests/persistence/property_support.rs` | `df451a1dc45ce9b6d7c0d395b2b9d0de25fa8aafdf7da43e957233201222eb61` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `489d9344c520a6fa4c7577e57609fcbb7498aa775b0e74daf48c5494809a14cc` |

### Exact current-state shape summary (2026-10-27)

Task 11943, the second slice of task 7927, gives a trusted successor the exact
shape of its current state without walking that state. J-067 requires the hot
policy to keep explicit ceilings on the current state's logical nodes,
endpoints, logical path bytes and payload bytes, and it warns that an exact
local summary needs a proof, because shared logical occurrences are not unique
objects (the per-field policy mapping of J-067 in the engineering journal). The
trusted bind of task 11945 checks those ceilings, so the summary it checks must
equal what cold admission would measure. This slice proves the derivation,
implements it, and checks it against the cold walk (TS-17). It does not change
what a publication returns.

**What the cold walk measures.** Semantic admission walks each of the three
namespace tries, the data bag, the continuation map and the join bag, with an
explicit worklist (`walk_trie`, `semantic.rs:804-912`). A node that is
reachable along two paths, for example a subtree that two channels share, is
visited once per path. For a state $`S`$ write

- $`N(S)`$ for the *logical nodes*: the number of visits, counting the root of
  each namespace even when it is empty;
- $`E(S)`$ for the *endpoints*: the visits that end at a present claim, a
  nonzero count or a content reference; and $`E_x(S)`$ for those of namespace
  $`x`$, where $`x`$ is $`d`$, $`c`$ or $`j`$ for the data bag, the
  continuation map and the join bag;
- $`P(S)`$ for the *source payload bytes*: the sum, over the continuation
  endpoints, of the payload length of the source blob that each endpoint
  references, with no deduplication;
- $`K(S)`$ for the multiset of the endpoints' key lengths, so that $`E(S)`$ is
  its size;
- $`M(S)`$ for the *maximum logical path*: the greatest path length that the
  walk observes.

`CurrentShapeSummaryV1` holds $`N`$, $`E`$, $`E_d`$, $`E_c`$, $`E_j`$, $`P`$
and $`K`$, and reads $`M`$ from $`K`$. The successor state is $`t`$ and its
predecessor $`s`$.

**Logical positions.** The rewrite edits each namespace trie along the paths of
its batch. It *loads* a node when it opens the root of a namespace whose batch
is nonempty, when it expands a child reference, and when it merges the only
child of a node that kept neither an endpoint nor a second child. It *emits* a
node when it builds a final node, and it *places* a node when it stages a final
node into the store. A merged child that was itself edited is emitted and then
absorbed into its parent, so it is never placed. Every load removes one logical
position of the predecessor, and every placement creates one position of the
successor. With $`\ell`$, $`p`$, $`e`$ and $`a`$ the loaded, placed, emitted
and absorbed counts,

```math
N(t) + \ell = N(s) + p,
\qquad
e = p + a .
```

The task required this to be decided from the code rather than by trial. The
existing load counter `loaded_nodes` counts per position: the root load
(`rewrite.rs:672`), an expansion (`rewrite.rs:1031`) and a merge load
(`rewrite.rs:1248`). The emit counter does not, because an absorbed child
(`rewrite.rs:1246`) is emitted but not placed. The report therefore gains
`placed_nodes`, counted by `stage_final` (`rewrite.rs:1400`), which places every
kept child (`rewrite.rs:1340`) and the root (`rewrite.rs:1187`). The counter
lives in the in-memory diagnostics, and no persisted byte depends on it.

![The compression merge of the theory's negative control, in one namespace; paths are lists of edge bytes. The predecessor, in amber, has four logical nodes, and the rewrite loads all four: the root R, which has no endpoint, when it opens the namespace; the leaf A at the key [1], which it expands and deletes; the node B at the key [2], which it expands; and B's child C at the key [2, 3], which it expands. The batch deletes the key [1] and updates the key [2, 3] from 1 to 2. The rewrite emits C' with the endpoint 2, and B' with the endpoint 1 and the one child C'. The root is then left with no endpoint and the one child B', so it merges with that child: B', in purple, is emitted and absorbed, never placed, and the merged root R'' takes its prefix [2], its endpoint and its child. The successor, in green, has two logical nodes, R'' and C', both emitted and placed. A note gives the counts, 4 loaded, 3 emitted, 2 placed and 1 absorbed; the identity that the final logical nodes plus the loaded nodes equal the initial logical nodes plus the placed nodes, here 2 plus 4 and 4 plus 2; the identity that the emitted nodes are the placed plus the absorbed ones, here 3 and 2 plus 1; and, in red, the negative control's miscount: the initial nodes plus the emitted minus the loaded nodes give 3, not 2](figures/shape-summary-absorbed-merge.svg)

**Endpoints, entries and payload.** A batch entry carries a before-value and an
after-value that differ. The entry removes an endpoint when its before-value
exists and adds one when its after-value exists, so an update does both. With
$`A_x`$ and $`R_x`$ the endpoints that the batch adds to and removes from
namespace $`x`$,

```math
E_x(t) = E_x(s) + A_x - R_x,
\qquad
E(t) = E(s) + \sum_{x} A_x - \sum_{x} R_x .
```

The payload of a continuation source is the encoded length
$`\lVert r \rVert`$ of its reference $`r`$ minus the 64-byte blob envelope: a
32-byte object header, the 16-byte kind domain, the blob version and the
payload length. With $`\mathcal{A}`$ and $`\mathcal{B}`$ the present after- and
before-references of the batch's continuation entries,

```math
P(t) = P(s) + \sum_{r \in \mathcal{A}} \bigl(\lVert r \rVert - 64\bigr)
            - \sum_{r \in \mathcal{B}} \bigl(\lVert r \rVert - 64\bigr).
```

Each total $`v`$ is updated as $`(v + \mathit{added}) - \mathit{removed}`$
with checked arithmetic, so a batch or a report that removes more than the
summary holds is refused instead of wrapping.

**The maximum and the multiset.** Structural admission refuses an empty
non-root node (`closure.rs:1294-1299`) and a node without an endpoint that has
exactly one child (`object.rs:318-319`), the path compression of a PATRICIA
trie (Morrison, 1968). An admitted trie therefore has no empty leaf, and every
visit lies on the path to some endpoint. The greatest path that the walk
observes is the longest key, or zero for an empty state:

```math
M(S) = \max\bigl(K(S) \cup \{0\}\bigr).
```

A running maximum cannot follow the deletion of the longest key, so the summary
keeps the whole multiset. Each removed endpoint releases its key length, and a
length that the multiset does not hold is refused; each added endpoint records
its length. The maximum is the largest length with a nonzero count.
`KeyLengthHistogramV1` stores the multiset as a copy-on-write `PathMap<u64>`
keyed by the big-endian length, so that byte order is numeric order. It holds
only nonzero counts, a clone shares its structure, and the maximum is the last
path in that order.

**The derivation.** `CurrentShapeSummaryV1::successor`
(`semantic.rs:318-423`) derives the successor's summary from the
predecessor's summary, the mutation batch and the rewrite report in five steps.

1. ⟨Bind the report to the batch⟩. Refuse `SemanticCoherence` unless the
   report's mutation count equals the number of the batch's entries in the
   three namespaces (`semantic.rs:323-332`).
2. ⟨Clone the multiset⟩. Clone $`K`$. The clone shares the predecessor's
   structure, and the predecessor's summary is unchanged whatever the outcome
   (`semantic.rs:334`).
3. ⟨Track the endpoints⟩. For each entry of namespace $`x`$, in the data,
   continuation and join batches in turn: when its before-value exists,
   release the key's length from $`K`$, refusing `SemanticCoherence` for a
   length that $`K`$ does not hold, and count one removal in $`R_x`$; when its
   after-value exists, record the length and count one addition in $`A_x`$
   (`semantic.rs:335-374`, with `track` at `semantic.rs:433-456`).
4. ⟨Track the payload⟩. For each continuation entry, add
   $`\lVert r \rVert - 64`$ of a present before-reference $`r`$ to the removed
   payload, and the same of a present after-reference to the added payload,
   refusing `ObjectLengthTooSmall` for a reference shorter than the envelope
   (`semantic.rs:351-364`, with `blob_payload_len` at `semantic.rs:459-464`).
5. ⟨Apply the deltas⟩. Replace each total $`v`$ by
   $`(v + \mathit{added}) - \mathit{removed}`$: $`N`$ with $`p`$ and
   $`\ell`$, $`E`$ with the sums of $`A_x`$ and $`R_x`$, each $`E_x`$ with its
   own, and $`P`$ with the payloads. An overflow refuses `CounterOverflow` and
   an underflow `SemanticCoherence` (`semantic.rs:376-422`, with
   `checked_delta` at `semantic.rs:466-479`).

The maximum is not stored: `maximum` reads the last path of $`K`$ and decodes
it, or returns zero for an empty multiset (`semantic.rs:224-234`). A refusal
returns before any field of the successor exists, so no partial summary
escapes.

![The derivation of a successor's shape summary. The inputs, in blue, are the predecessor's summary, the mutation batch, and the rewrite report with its mutation count and its loaded and placed nodes. A report whose mutation count is not the batch's number of entries is refused, in red. The key-length multiset is cloned, in amber, and the clone shares its structure. For each entry of the data, continuation and join batches in turn, a present before-value releases its key length from the multiset, refusing a length that the multiset does not hold, and counts one removal in its namespace; a present after-value records the length and counts one addition. For a continuation entry, in blue, the payload of each present reference, its encoded length less the 64-byte envelope, joins the removed or the added payload, and a reference shorter than the envelope is refused. Every total then becomes the total plus its additions minus its removals: the logical nodes with the placed and the loaded nodes, the endpoints with the sums over the namespaces, each namespace's entries with its own counts, and the payload; an overflow or an underflow is refused. The result, in green, is the successor summary, whose maximum logical path is the last length of the multiset, or zero. A grey note states the check of TS-17 against the recording cold walk, and why the derivation is exact](figures/shape-summary-derivation.svg)

The cold summary comes from the walk that admission already performs. The
recording walk (`verify_rspace_semantics_after_structural_with_shape`,
`semantic.rs:695-714`) runs the plain walk's three trie walks and records each
endpoint's key length after the endpoint validates (`semantic.rs:754-777`, with
`record_key_length` at `semantic.rs:785-795`). `from_cold`
(`semantic.rs:280-300`) builds the summary from its report and multiset, and
refuses a multiset whose size is not the walk's endpoint count or whose maximum
is not the walk's maximum logical path.

**The theory.** `formal/rocq/stack_safe_pda/theories/ExactStateShapeSummary.v`
(module `ExactStateShapeSummary`) proves the derivation. `_CoqProject` lists
it, and the integrated runner compiles it, requires all sixteen capstones to be
closed under the global context, and checks it with the kernel checker.

| Capstone | Statement |
|---|---|
| `admitted_tries_have_no_empty_leaf` | An admitted trie has no empty leaf. |
| `maximum_logical_path_is_the_longest_key` | On an admitted root, the walk's path maximum is the length of the longest key. |
| `combined_maximum_is_the_walk_maximum` | Over three admitted namespace roots, the largest of the three path maxima is the largest key length. |
| `batch_weight_delta_is_exact` | A successful batch moves any additive weight of the entries by the weights of its after-images minus those of its before-images. |
| `endpoint_existence_delta_is_exact` | The endpoint count moves by the added minus the removed entries, both as a checked delta and as a compare-and-swap count. |
| `payload_delta_is_exact` | The payload bytes move by the after-images' encoded lengths less 64 each, minus the same for the before-images. |
| `key_length_multiset_tracks_the_batch` | Applying the batch to a multiset that is the predecessor's key lengths together with some other lengths succeeds, and gives the successor's key lengths together with the same other lengths. |
| `largest_nonempty_bucket_is_the_longest_key` | In such a multiset, every length with a nonzero count is at most the largest length, the largest length has a nonzero count when the multiset is not empty, and the largest length is the greater of the longest key and the largest other length. |
| `rewrite_moves_logical_positions_exactly` | For one namespace, the final positions plus the loaded nodes equal the initial positions plus the placed nodes, and the emitted nodes are the placed nodes plus the absorbed ones. |
| `absorbed_reports_move_positions_exactly` | The same identity holds for the sum of the three namespace reports. |
| `counted_rewrite_is_the_radix_edit` | On an admitted root, the counted rewrite computes the radix editor's result and refuses exactly when the editor refuses. |
| `trusted_shape_summary_equals_the_cold_walk` | For an admitted namespace root and a counted rewrite whose admitted result lists the batch applied to the root's entries, the cold walk's logical nodes, endpoints and payload bytes, updated by the batch and the rewrite's loaded and placed counts, are those of the cold walk of the result; the updated multiset is the result's key lengths together with the other lengths, and its largest length is the greater of the result's path maximum and the largest other length. |

Four negative controls are closed counterexample theorems.
`emitted_minus_loaded_mutant_miscounts_a_compression_merge` counts
$`e - \ell`$ and miscounts a merge that absorbs an edited child.
`digest_dedup_counter_mutant_undercounts_equal_leaves` counts distinct objects
and misses equal leaves, which are distinct positions.
`running_maximum_mutant_keeps_a_deleted_longest_key` keeps a running maximum
without the multiset. `empty_leaf_mutant_breaks_the_path_maximum` shows that a
trie with an empty leaf has a path maximum above its longest key. The composed
theorem takes as an explicit argument that the rewrite's result lists the batch
applied to the predecessor's entries; TS-17 checks that premise against an
independent rebuild.

**Production code.**

- `DagRewriteReport::placed_nodes` (`rewrite.rs:115`), counted by
  `stage_final` (`rewrite.rs:1401-1410`) and summed across the three
  namespaces with the other counters (`rewrite.rs:149`).
- `KeyLengthHistogramV1` (`semantic.rs:167-266`) and `CurrentShapeSummaryV1`
  (`semantic.rs:268-479`), with `successor` and `from_cold` as described
  above.
- The recording walk, as described above.
  `verify_rspace_semantics_after_structural` (`semantic.rs:720-730`) runs the
  same walk and records nothing, so its report is unchanged.
- Durable cold admission records the histogram in the walk that it already
  performs (`durable/admission.rs:2033-2037`) and keeps the summary on the
  admitted source (`durable/admission.rs:2074`), read through `shape()` on the
  source and on the writer (`durable/admission.rs:1124`,
  `durable/admission.rs:1450`).

The histogram is neither persisted nor used for preflight. It is not the
cold-summary index of task 7955: nothing reads it before admission completes,
and it adds no object read and no walk.

**Boundary.**

- Task 11945 checks the hot policy's shape ceilings against the derived
  summary at the bind, and since task 11946 the trusted publication returns it
  (the trusted successor publication section). TS-17 checks both the
  cold-checked publication's re-admitted summary and the trusted publication's
  returned one against the derivation.
- The summary describes the current state. Retained states keep their
  per-state maxima (TS-16).
- No persisted byte, identity or format changes.

Cited work: D. R. Morrison, "PATRICIA—Practical Algorithm To Retrieve
Information Coded in Alphanumeric", *Journal of the ACM* 15(4):514–534, 1968,
[doi:10.1145/321479.321481](https://doi.org/10.1145/321479.321481).

The shape summary evidence in the verification record was taken at these
sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/rewrite.rs` | `24447a36c1909f9b3691bbbf72272dad4f46682165ceaa5b48911232b077bb79` |
| `rspace++/src/rspace/persistence/semantic.rs` | `863810a22e07988258b8e9f7493ae277764b2a9ed53eb6af7e0ef96b2fc90fe1` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `f9eb0bdc6a2959234bd48b5dddcd6803b060d9859cc786f9b1affce9d14a5707` |
| `formal/rocq/stack_safe_pda/theories/ExactStateShapeSummary.v` | `2cb514702837fae8cd5c5952e2d9e732010e183d55705d79bb6fefe4fc905e11` |
| `formal/rocq/stack_safe_pda/_CoqProject` | `51fe4fccca6c555b7f5b25577565b1e91fb5ab80d2c23738e7b583c7c33e4478` |
| `scripts/check-stack-safe-pda-proofs.sh` | `66b3c6408769a0a61db122c60e29e853611c7dad98d39da3633d74cc0f5dbbc6` |
| `rspace++/tests/persistence/shape_summary_properties.rs` | `58017465f4214612305a75111baa5cb42cbde8f142a98472e35f041dc74319fb` |
| `rspace++/tests/persistence/path_exact_closure_properties.rs` | `d01821542887567fb7d11170cce24f9c76feb8bd05d1e3f57c1339b160f54d6c` |
| `rspace++/tests/persistence/property_support.rs` | `1f1cbfec63a29976479af2070c67763e3a351f3e0fb513b4f1bc0e15d2165f6e` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `b52a6ec4c3416b0c7a684bbde17fef1f6f7873c19b32da01924984367c4cb70e` |

### Durable authority core and cold-checked evidence (2026-10-27)

Task 11944, the third slice of task 7927, separates by type what a trusted
successor may carry from what only cold admission can measure. Engineering
journal J-067 writes $`A_s`$ for the authority of a state $`s`$, its semantic
validity and exact inventory, and $`C_{s,L}`$ for a successful cold admission
of $`s`$ under the cold limits $`L`$, together with the demand that the
admission measured. It rules that one *authority core* carries $`A_s`$, that a
*cold-checked wrapper* adds the cold observations, and that nothing converts
the first into the second (the capability contract of J-067 in the engineering
journal). Until this slice one type, `AdmittedCheckpointSourceV1`, held both,
so no code could hold an authority without also holding cold evidence, which a
trusted successor never has. The slice splits that type, gives the core a
semantic validity token in place of the cold semantic report, and adds a
cold-checked acquisition that turns a core back into a cold source. It changes
no persisted byte, identity, report, refusal or publication outcome. When it
landed, every incremental publication ran physical recovery and cold
admission, and only the tests below called the new conversions; tasks 11945
and 11946 then bound and published trusted successors.

**The split.** `AdmittedCheckpointSourceV1` (`durable/admission.rs:1224-1230`)
now owns a `DurableAuthorityCoreV1` (`durable/admission.rs:1083-1099`) and the
cold evidence beside it.

| Field | Part | What it holds |
|---|---|---|
| `store` | core | the pinned, indexed object store of the admitted generation |
| `registry` | core | the generation-lease registry that a writer's lease must share |
| `authority` | core | the catalog token: the exact catalog image of two 336-byte slots, 672 bytes in all, and the slot that it selects |
| `version` | core | the version of the current state |
| `retention_manifest_bytes`, `retention_format` | core | the appointed retention manifest and its format |
| `validity` | core | the semantic validity token, described below |
| `shape` | core | the exact current-state shape summary of task 11943 |
| `retained_closure` | core | the shared closure ledger, with its current, retained and metadata domains |
| `retained_states` | core | the number of retained states: the historical states and the current one |
| `retained_state_maxima` | core | the componentwise maxima of the per-state structural inventories, in distinct objects and canonical bytes |
| `retention_writer` | core | the retention format in which this writer seals, fixed by its directory |
| `report` | evidence | the cold admission report |
| `admission_requirements` | evidence | the demand that cold admission measured, which a fresh checkpoint preflights |
| `retention` | evidence | the admitted retention, with its admitted V2 metadata |
| `semantic` | evidence | the cold semantic admission token, which carries the semantic report |

Cold admission builds both parts at once (`durable/admission.rs:2168-2191`).
The core is the production image of the theory's `authority_core` record
(`TrustedSuccessorCapability.v:655-666`):

- `core_state` and `core_metadata_roots` are the retention entries and the
  metadata anchor that the version and the appointed manifest name;
- `core_ledger` is `retained_closure`, into which admission also seeds the
  metadata domain (`durable/admission.rs:2118-2120`);
- `core_retained_states`, `core_maxima`, `core_shape` and `core_validity` are
  `retained_states`, `retained_state_maxima`, `shape` and `validity`;
- `core_profile` is the type parameter `P`;
- `core_index_records` and `core_index` are the store's pinned object index.

The cold evidence has no counterpart in the record: the theory keeps cold
demand in a separate record (`cold_demand`, `TrustedSuccessorCapability.v:782`).
The shape summary that task 11943 kept on the admitted source now lives on the
core; the previous section's citations refer to its pinned sources.

**The validity token.** `SemanticValidityV1` (`transaction.rs:109-145`) holds
the manifest of a state whose semantics are valid. Cold admission mints it from
the semantic admission token (`from_admission`, `durable/admission.rs:2176`).
`successor` advances it along a sealed transaction in three steps.

1. ⟨Check the profile⟩. Refuse `SemanticProfileMismatch` unless the receipt's
   profile is that of `P`. Sealing already refuses any other profile
   (`transaction.rs:568-574`), so the check keeps the rule total rather than
   deciding anything today.
2. ⟨Check the base⟩. Refuse `SemanticCoherence` unless the receipt's base names
   the token's manifest.
3. ⟨Advance⟩. Return the token of the receipt's target manifest.

The two checks are the guards of the theory's `trusted_bind`
(`TrustedSuccessorCapability.v:724-725`), and the result is the conclusion
`core_validity successor = receipt_target receipt` of `trusted_successor_rule`
(`TrustedSuccessorCapability.v:1636-1650`), which TS-18 checks on the test
model. Like the semantic admission token, a validity token names no store and
no profile; only the core pairs it with its own store, through `from_validity`.

**The evidence parameter.** `VerifiedDagState` gains a type parameter `E` for
the evidence that it carries, whose default is the cold evidence
(`transaction.rs:146-162`). The parameter follows the typestate idea (Strom
and Yemini, 1986): the type records what has been established about a value,
and the operations that the value admits depend on it.

- `ColdAdmittedV1` carries the cold semantic report. Only `admit` and
  `from_completed_admission` build it, both from a completed semantic
  admission, and `admission_report` exists only for it (`transaction.rs:224`).
  The report therefore stays cold-checked, never optional and never
  fabricated, as J-067 requires of `admission_report`.
- `ValidityOnlyV1` is zero-sized and carries no report. `from_validity` builds
  it from a store, a version and a validity token, and refuses
  `SemanticCoherence` unless the token's manifest is the version's manifest
  (`transaction.rs:230-247`).
- Every other method is generic over `E`. A signature that names
  `VerifiedDagState` without `E` names the cold evidence, and `admit` and
  `from_completed_admission` exist only for it, so every existing caller and
  test compiles and behaves unchanged.
- `OpenDagTransaction::new` accepts either evidence and keeps a validity-only
  witness of its store and version (`transaction.rs:383-402`); no transaction
  phase reads a report. The private `validity_only` (`transaction.rs:255-262`)
  drops the evidence of a state that one of the three checked constructors
  built, and it is the only other constructor of `ValidityOnlyV1`.

**No coercion.** Two conversions exist, both in the direction that J-067
allows: `core()` borrows the core of a cold source, and `into_core()` keeps the
core and drops the evidence (`durable/admission.rs:1233-1235`). No function
takes a core and returns cold evidence. The core holds no report, no measured
demand, no admitted retention and no cold semantic token, and its
`verified_state` returns validity-only evidence
(`durable/admission.rs:1112-1117`). This is the type-level form of the
trusted successor section's non-implication

```math
A_t \;\not\Longrightarrow\; C_{t,L}.
```

The theory witnesses it with
`cold_demand_is_not_a_function_of_the_inventory_summary`
(`TrustedSuccessorCapability.v:2011`), and the TLA+ model requires that a
trusted successor carry no cold evidence (`TrustedSuccessorHasNoColdEvidence`,
`RSpaceTrustedIncrementalPublication.tla:295-296`), which the control that
adopts with fabricated evidence violates (`AdoptWithFabricatedEvidence`,
`RSpaceTrustedIncrementalPublication.tla:381-386`). A fresh checkpoint needs
the cold current inventory (`checkpoint_descriptor`,
`durable/admission.rs:1286-1313`) and the measured demand, so a core must
acquire a cold source before it publishes one (TS-08).

**Cold-checked acquisition.** `acquire_cold_checked(lease, core, recovery,
wal, admission)` (`durable/admission.rs:1919-1932`) turns a core back into a
cold source of the same catalog image in three steps.

1. ⟨Recover⟩. Recover the directory under the borrowed lease
   (`recover_physical_from_lease_v1`), which also resolves a pending
   reclamation sidecar.
2. ⟨Admit⟩. Admit the recovered state cold under the admission limits
   (`admit_physical_source`). It is the only constructor of a cold source, so
   the result carries freshly measured evidence.
3. ⟨Compare⟩. Require the core's catalog image to equal the admitted one in
   all 672 bytes (`matches_exactly`, `catalog.rs:500-502`), and refuse
   `AuthorityChanged` otherwise, reusing the existing variant. The comparison
   covers the whole image, not only the selected slot: an image whose other
   slot changed is refused although it selects the same slot.

The lease is only borrowed, so after every refusal the caller still holds it
and its writer lock. The first two steps are those of the journal owner's
`reconcile` (`durable/rspace_authority.rs:302-306`).

`SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)`
(`durable/admission.rs:1549-1555`) binds a lease to a cold source. It runs the
writer preflight first, which refuses `AuthorityMismatch` unless the source
shares the generation-lease registry of the lease's opened directory, so a
source admitted through one opened directory is never bound to the lease of
another. A fresh checkpoint from a core therefore
acquires a cold source, binds it, and calls
`publish_generation_checkpoint_v1`, whose admission preflight
(`durable/publication.rs:770-772`) still precedes the cleanup of unpublished
artifacts (`durable/publication.rs:784`).

![The durable authority core and its cold-checked admission evidence. In grey, admit_physical_source, which walks the current and every retained closure and then the current semantics under the admission limits, is the only constructor of the amber cold source, AdmittedCheckpointSourceV1. The cold source owns the core and the cold evidence: the admission report, the measured demand, the admitted retention and the cold semantic token. Only the cold source reports, preflights a fresh checkpoint against the measured demand, describes a checkpoint from the cold current inventory, visits and copies checkpoint objects, seals and binds durable transactions, which was the journal owner's bind until task 11946, and returns a verified state with the cold evidence. In green, the core DurableAuthorityCoreV1 holds the store, the registry, the exact 672-byte catalog image, the version, the retention bytes and format, the validity token, the shape summary, the retained closure ledger, the retained-state count and maxima, and the retention writer; it preflights writers, prepares transactions and returns a verified state with validity-only evidence. The green validity token holds the current manifest and advances only along a sealed receipt whose base is that manifest. In blue, the evidence parameter of VerifiedDagState: the default, ColdAdmittedV1, carries the cold report and alone has admission_report; ValidityOnlyV1 carries none, and from_validity requires the token's manifest to be the version's; OpenDagTransaction::new accepts either and keeps a validity-only witness. In purple, core() borrows the core and into_core() drops the evidence, the only conversion; acquire_cold_checked recovers, admits and compares all 672 bytes of the catalog image, refusing a mismatch as AuthorityChanged while the caller keeps the lease; and a fresh checkpoint needs the cold source, which from_parts binds to its own lease. In red and dashed, the coercion from the core to cold evidence, which does not exist. A grey note states that the default evidence keeps every caller unchanged; that task 11944 changed no behavior, because the cold source delegates to its core and only tests then called core(), into_core(), acquire_cold_checked and from_parts; and that since task 11946 the journal owner keeps only the core, from_view and reconcile calling into_core() and the trusted publication returning a core, while acquire_cold_checked and from_parts remain test-only](figures/durable-authority-core-split.svg)

**Supporting refactors.** Four further changes prepare tasks 11945 and 11946,
and each preserves behavior.

- The current part of the successor inventory narrows to the two counts that
  its readers use, `ClosureInventoryV1`: distinct objects and canonical bytes
  (`durable/admission.rs:1456-1462`, `durable/admission.rs:1887-1891`). The
  full report still decides `CurrentOutsideRetainedUnion` first, and the two
  readers take only the canonical bytes (`durable/admission.rs:1751`,
  `durable/publication.rs:889`).
- The retention V2 attachment becomes a free function of the sealed
  transaction, the staging store, the shared ledger, the anchor, the
  predecessor anchors and the trial limits (`attach_retention_v2`,
  `durable/admission.rs:2194-2220`), so that a bind can run it without a cold
  source. Its ledger trial, attachment and errors are those of the former
  method.
- Startup's resolution of the reclamation sidecar becomes
  `resolve_reclamation_candidate_v1` (`durable/startup.rs:303-324`), which
  `recover_physical_from_lease_v1` calls with the same candidate and the
  selected slot (`durable/startup.rs:299`). Its three outcomes run in the same
  order with the same errors, and task 11946's trusted publication reuses it
  (the trusted successor publication section).
- The incremental append takes a prebuilt plan, `IncrementalAppendPlanV1`
  (`durable/publication.rs:1042-1080`). `append_incremental_prepare_and_index`
  keeps its signature, builds the plan with the same derivations, and hands it
  to `append_planned_incremental_prepare_and_index`
  (`durable/publication.rs:1097-1173`), which makes the same writes. The plan
  carries the index append together with the addresses derived beside it, so
  the two cannot be mismatched.

**Tests.** `rspace++/tests/persistence/durable_authority_core.rs` adds four
tests. They are not property rows: they add no ID, and the
catalog/derivation/binding registry stays at 211. Two thread-local counters,
compiled only into test builds, make cold work observable. `cold_admissions`
counts the entries into `admit_physical_source`
(`durable/admission.rs:1934-1951`), and `cold_closure_walks` counts the entries
into `verify_structural_closure_in_ledger_with_credit`
(`closure.rs:633-656`), through which every structural closure walk passes.
libtest runs each test on its own thread and nextest in its own process, and
no admission or walk spawns a thread, so each test sees only its own counts.

| Test | What it shows |
|---|---|
| `authority_core_projects_the_cold_source_without_cold_work` | One cold admission walks one closure per retained state. The core and the cold source then agree: one version, one catalog token, one object store (the same reference), the same shape, retention bytes and format, the validity token of the current manifest, and the retained-state count of the cold report. A checkpoint preflight accepts at the core's maxima and refuses one below on each component, naming that component, the lowered limit and the maxima, so the maxima are those of the cold requirements. Sealing one batch through the cold source and through the core's validity-only state gives equal receipts and objects. Neither the projections, nor the sealing, nor `into_core` admits or walks again. |
| `semantic_validity_advances_only_along_its_own_sealed_receipt` | The core's token advances along a receipt of its own state to the target's manifest, and the advanced token refuses the same receipt again. `from_validity` binds the token to its own version, refuses it for the target, and refuses the advanced token for the base. |
| `cold_checked_acquisition_is_bound_to_the_exact_catalog_image` | Acquisition from a core admits cold once and walks one closure per retained state, and its source equals the core in authority, retention bytes, shape and version. Publishing a batch incrementally through that source admits cold once more, as every incremental publication still does, and yields the catalog image that a freshly admitted writer publishes for the same batch in another directory. Acquisition from the stale core then refuses with `AuthorityChanged`, and acquisition from the successor's core succeeds. A catalog image rewritten so that its other slot changes while it selects the same slot is refused with `AuthorityChanged` as well. After each refusal the lease still holds the writer lock. |
| `rebound_cold_checked_writer_publishes_the_same_fresh_generation` | Two directories installed from the same checkpoint hold equal catalog images. A source acquired under one lease is refused with `AuthorityMismatch` when bound to the other lease. A source acquired again and bound to its own lease publishes a fresh generation of the same state, whose catalog image equals the one that a freshly admitted writer publishes. |

**Boundary.**

- This slice refines no TS row in production. The trusted bind of task 11945
  takes the core and its validity token, and the trusted publication of task
  11946 returns a successor core and switches the journal owner to it.
- Cold-checked behavior is unchanged. Every cold-checked incremental
  publication recovers and admits cold, startup admits cold, and a fresh
  checkpoint preflights its measured demand before cleanup.
- An acquisition may write before it refuses: its recovery may resolve the
  reclamation sidecar and rebuild the object index (`durable/startup.rs:581-620`),
  as every startup may. Both are idempotent and keyed to the selected catalog,
  exactly as in the owner's `reconcile`.
- A catalog image names a store, not a directory: two directories installed
  from one checkpoint hold byte-identical images. The comparison therefore
  accepts the lease of any directory whose image equals the core's, and the
  result is a complete cold admission of the leased directory. `from_parts`
  refuses to bind that source to the lease of any other opened directory,
  which the fourth test checks with two such directories.
- No persisted byte, identity, cold report or refusal changes.

Cited work: R. E. Strom and S. Yemini, "Typestate: A programming language
concept for enhancing software reliability", *IEEE Transactions on Software
Engineering* SE-12(1):157–171, 1986,
[doi:10.1109/TSE.1986.6312929](https://doi.org/10.1109/TSE.1986.6312929).

The authority core evidence in the verification record was taken at these
sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/transaction.rs` | `85147658e02a85537bd31fbd08770f76f883aaea90f402fb9a15a20b7767788d` |
| `rspace++/src/rspace/persistence/closure.rs` | `1a3ecb59031c71d880d9b30eb625fa775516c7ef2be263578bb9bbec9f86aa70` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `5513991d1a2a6562e7ed360c4044c4b796433c802d297b54c5f703fb679a6d1d` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `0229bda8a53f969e2574c7611b13e3abef1d2e4be77e0b1421618acf99d6e468` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `3a1da653d1206d0dcc9563c0cd14b6197cd49081cc605d78aed3c6cc4db863d6` |
| `rspace++/src/rspace/persistence/durable/publication.rs` | `51a9916b467a5c6cfda135c9e140c169a8d213cff2bbe4ae79e3d11d0a9b9062` |
| `rspace++/tests/persistence/mod.rs` | `11a4a73ec62d6f09868b0bb3aaeb44af2ba65c9bcf057d2f1e99ce42ebf016f3` |
| `rspace++/tests/persistence/durable_authority_core.rs` | `bd3d405f249f3440386399d6cf8aef314cd97f744062b655b6b7cabe502e6f32` |
| `docs/design/pathmap/figures/durable-authority-core-split.puml` | `d13c7192bcc2e0d90bd16cd16bfd4e9df62896b433fb02e0a6beb88771f3f4a1` |

### Path-exact incremental closure (2026-10-27)

Task 11957 corrects the incremental closure verifier,
`verify_incremental_closure` (`closure.rs:927-1210`), whose only production
caller is the transaction seal (`transaction.rs:524`). The verifier proves both
directions of a portable delta without walking an untouched base subtree: every
occurrence in the target is either inherited from the verified base at its
exact path or staged by the transaction, and every staged object belongs to the
delta. The shape summary property of task 11943 found that it refused a valid
transition with `MissingObject`. This section defines the walk, the defect and
its correction, the model, and the properties PX-01 to PX-05. It corrects
Algorithm 3 of J-037 in the engineering journal, where J-251 records the
discovery and the measurements.

**Terms.**

- An *occurrence* is a reference $`r`$ together with its *expectation* $`e`$
  and its exact absolute logical path $`p`$. The expectation says what the walk
  expects to find: the manifest, a node of one namespace, or a content blob at
  an endpoint of one namespace. A node entered at path $`p`$ with prefix $`q`$
  enters its child on edge byte $`b`$ at $`p \cdot q \cdot [b]`$ and its content
  at $`p \cdot q`$, where $`\cdot`$ concatenates byte strings. Equal subtrees
  are one object, so one object can have many occurrences.
- The *staging overlay* $`O`$ holds every object that the transaction emitted,
  even when the committed store already holds equal bytes: insertion consults
  only the overlay (`store.rs:230-270`), and so does the verifier's lookup
  (`store.rs:274-296`).
- A *certificate* is an inherited boundary $`(n, k, p, r)`$: a namespace, a
  boundary kind (`NodeEntry` for a node, `ContentAtEndpoint` for content), an
  exact path and a reference. The private rewrite records certificates in two
  steps. Each base node that it loads makes its content reference, at the
  node's path, and each child reference, at the child's path, *eligible*
  (`rewrite.rs:1415-1449`, called at `rewrite.rs:1565-1567`). Each node that it
  builds promotes an eligible boundary to *inherited* when it holds the same
  reference at the same path (`rewrite.rs:1451-1483`, called at
  `rewrite.rs:1362-1370`). A namespace that the batch leaves unmutated
  certifies its root at the empty path (`rewrite.rs:652-665`). The verifier
  consults only inherited boundaries (`rewrite.rs:388-390`). The manifest has
  no boundary kind, so it is never certified (`closure.rs:1212-1222`).
- An occurrence is *coincident* when it is certified and its reference is also
  staged.

**The defect.** Algorithm 3 of J-037 looked a reference up in the overlay
first, and consulted a certificate only for a reference that was not staged. A
node that the rewrite emits at one path can be byte-identical to an untouched
base subtree that the rewrite certified at another path. Content addressing
makes the two one object, so the untouched subtree's reference is staged. The
old walk reached the untouched subtree at its certified position, found its
reference staged and descended. The rewrite never loaded that subtree, so its
descendants are neither staged nor certified at their own paths, and the walk
refused a valid delta with `MissingObject`.

That property shrank its failure to one transition. The base holds three
continuations: $`A`$, on the channels alpha and beta with the body `body-0` and
an independent source $`S_0`$; $`B`$, on alpha and beta with `body-the-second`
and a natural source; and $`C`$, on gamma and alpha with `body-0` and the same
independent source $`S_0`$; together with the joins that their groups require.
The batch replaces $`B`$'s source with its independent source, inserts $`D`$,
on gamma and alpha with `body-the-second` and a natural source, and raises both
joins of gamma and alpha from 1 to 2. Inserting $`D`$ splits $`C`$'s leaf, and
the split leaf keeps the same remaining key bytes and the same content $`S_0`$
as $`A`$'s untouched leaf, so it is the same object. Replacing $`B`$'s source
rebuilds $`A`$'s parent, which certifies $`A`$'s leaf at $`A`$'s position. The
walk reached $`A`$'s leaf there, found it staged and descended, and found
$`S_0`$ at $`A`$'s key neither staged nor certified. The seal refused with
`MissingObject` naming exactly $`S_0`$'s digest; the target holds 15 logical
nodes in 14 node objects, against the base's 13 in 13 (pgmcp artifact 1123).

![The coincidence counterexample of the theory, in one namespace; numbers are references and paths are lists of edge bytes. The staged manifest 1 enters the staged root 2 at the empty path. The root has edge 1 to the leaf 3, drawn in amber, whose occurrence at path 1 is certified as a NodeEntry, and edge 2 to the staged node 4, which split the base node C. Node 4 has edge 3 to the same leaf 3, at path 2, 3, where it is not certified, and edge 4 to the staged leaf 5, whose content is the staged blob 7. The leaf 3 has an empty prefix and the content 6, drawn in grey: a base blob that is not staged, certified as ContentAtEndpoint at path 2, 3 and neither staged nor certified at path 1. A note compares the walks: the staged-first walk descends into the leaf 3 at path 1 and refuses MissingObject 6 for every fuel; the certificate-first walk stops at the certificate at path 1, descends into the leaf 3 at path 2, 3, stops at the content's certificate, and accepts with 8 visits, reaching 1, 2, 4, 5, 7 and 3](figures/path-exact-closure-coincidence.svg)

**The correction.** The corrected walk consults the exact certificate first. It
starts by pushing the target manifest at the empty path, and every occurrence
$`(r, e, p)`$ that it pops takes these steps:

1. ⟨Count the visit⟩. Count one logical visit, refusing `ResourceLimit` above
   `max_logical_visits` (`closure.rs:963-968`).
2. ⟨Stop at the exact certificate⟩. If $`e`$ has a namespace $`n`$ and a
   boundary kind $`k`$, and $`(n, k, p, r)`$ is an inherited boundary, mark the
   occurrence completed and go on to the next one: the base owns this
   occurrence and everything below it (`closure.rs:971-981`).
3. ⟨Read the staged object⟩. Otherwise read $`r`$ from the overlay, refusing
   `MissingObject` if it is absent (`closure.rs:982-989`).
4. ⟨Check and expand⟩. Unchanged: authenticate $`r`$ once per digest against
   its declared length and digest, check for a cycle, parse $`r`$ as $`e`$
   expects at $`p`$, validate the node context and the endpoint policy, record
   $`r`$ as reached, and push its content at $`p \cdot q`$ and each child at
   $`p \cdot q \cdot [b]`$.

When the worklist is empty, one step remains:

5. ⟨Check the delta⟩. Refuse `SemanticCoherence` ("transaction staged an
   object outside the target delta closure") if the walk did not reach some
   staged reference (`closure.rs:1199-1208`).

Algorithm 3 ran step 3 before step 2, and step 2 only for a reference that step
3 did not find.

![One occurrence of the corrected walk. After the target manifest is pushed at the empty path, the walk pops an occurrence of reference r, expectation e and exact path p while work remains. If e is a node or a content blob and a certificate names exactly its namespace, its kind, p and r, the walk stops at an inherited boundary, drawn in green: the base owns the occurrence. Otherwise it looks r up in the staging overlay, drawn in amber. An absent r is refused as MissingObject, in red. A staged r is authenticated once per digest against its declared length and digest and checked for a cycle, parsed as e expects at p with the node context and the endpoint policy validated, both in grey, and recorded as reached, with its content and children pushed at their exact paths, in blue. When no work remains, the walk accepts the delta, in green, if every staged object was reached, and otherwise refuses it, in red, as staged outside the target delta closure. A note states the defect of Algorithm 3 of J-037, which consulted the overlay first, and the correction recorded in J-251](figures/path-exact-closure-occurrence.svg)

The two orders decide alike at every occurrence that is not coincident, so the
correction changes two outcomes, both at coincident occurrences:

- a valid delta that has a coincident occurrence is accepted, where Algorithm 3
  refused it with `MissingObject`;
- a staged object that the walk reaches only at certified occurrences is
  refused in step 5, where Algorithm 3 descended into it there and accepted.
  Such an object is not part of the delta: the base owns each of its
  occurrences in the target.

**What stays equal.** Write $`R_{\mathrm{cf}}`$ and $`R_{\mathrm{sf}}`$ for the
references that the certificate-first and the staged-first walk reach. When
both accept a transaction,

```math
R_{\mathrm{cf}} \subseteq R_{\mathrm{sf}} \subseteq \operatorname{dom} O \subseteq R_{\mathrm{cf}} .
```

The first inclusion holds because the staged-first walk descends wherever the
certificate-first walk does, and also at coincident occurrences; the second
because both walks descend only into staged objects; the third because the
certificate-first walk passed step 5. The three sets are therefore equal, and
so are the report fields that count first visits per digest: distinct objects,
node objects, payload objects and canonical bytes. The counts that follow
occurrences (the references, the maximum worklist and the logical visits) can
only fall, and only when a coincident occurrence is met; without one, both walks
make the same decision everywhere and return identical outcomes and reports. The
verifier writes nothing: the sealed objects, the receipt and the target
manifest come from the rewrite and the overlay. The WAL reads only the closure
report's canonical bytes, through an accessor that nothing calls yet
(`wal/codec.rs:421-427`). A transaction that becomes sealable writes its
coincident object as `Reuse`, because an equal body is already committed
(`wal/disposition.rs:12-20`).

**The theory.** `formal/rocq/stack_safe_pda/theories/PathExactIncrementalClosure.v`
(module `PathExactIncrementalClosure`) models the walk as a recursive descent
with fuel over parsed objects, with one classifier for each order. `_CoqProject`
lists it, and the integrated runner rejects placeholders in it, compiles it,
requires all nine capstones to be closed under the global context, and checks
it with the kernel checker. Write $`\mathrm{cert}(o)`$ when the certificate set
$`C`$ holds the exact boundary of an occurrence $`o`$, $`r_o`$ for the
reference of $`o`$, $`O(r) = x`$ when the overlay stages the object $`x`$ under
$`r`$, and $`\mathrm{ch}(o, x)`$ for the child occurrences of $`x`$ at $`o`$;
every rule below that reads $`x`$ also requires $`x`$ to have the kind that
$`o`$ expects. *Coverage*, the contract of exact-path provenance, is the least
predicate closed under

```math
\frac{\mathrm{cert}(o)}{\mathrm{covered}(o)}
\qquad\qquad
\frac{O(r_o) = x \qquad \forall o' \in \mathrm{ch}(o, x).\ \mathrm{covered}(o')}{\mathrm{covered}(o)}
```

and the *delta closure* $`o \leadsto t`$, the references reached through
uncertified staged objects, is the least relation closed under

```math
\frac{\neg\,\mathrm{cert}(o) \qquad O(r_o) = x}{o \leadsto r_o}
\qquad\qquad
\frac{\neg\,\mathrm{cert}(o) \qquad O(r_o) = x \qquad o' \in \mathrm{ch}(o, x) \qquad o' \leadsto t}{o \leadsto t} .
```

With $`o_m`$ the manifest $`m`$ at the empty path, a delta is valid when the
manifest is covered and the delta closure reaches every staged object:

```math
\mathrm{valid}(O, C, m) \iff \mathrm{covered}(o_m) \wedge \forall s \in \operatorname{dom} O.\ o_m \leadsto s .
```

| Capstone | Statement |
|---|---|
| `certificate_first_reached_is_delta_closure` | When the certificate-first walk accepts an occurrence $`o`$, the references that it reaches are exactly those $`t`$ with $`o \leadsto t`$. |
| `certificate_first_verifier_sound` | An accepted certificate-first verification is a valid delta. |
| `certificate_first_verifier_complete` | The certificate-first verifier accepts every valid delta once its fuel reaches the depth of the delta. |
| `staged_first_verifier_sound` | An accepted staged-first verification covers the manifest: Algorithm 3 never accepts an uncovered occurrence. |
| `verifiers_agree_without_coincidence` | When no certificate names a staged reference, the two verifiers return identical outcomes for every fuel and manifest, including the visit count, the reached references and the refusal. |
| `staged_first_refuses_a_valid_delta_at_a_coincident_certificate` | The delta of the coincidence fixture (the first figure of this section) is valid; the certificate-first verifier accepts it with 8 visits, reaching 1, 2, 4, 5, 7 and 3; the staged-first verifier refuses it with `MissingObject 6`, and no fuel makes it accept. |
| `digest_only_certificate_mutant_admits_an_uncovered_occurrence` | Negative control. A staged node occurs at two paths, and its unstaged child is certified below only one of them. A certificate lookup that ignores the path accepts; the exact verifier refuses with `MissingObject`, and the manifest is not covered. |
| `skipped_reachability_mutant_admits_an_unreached_staged_object` | Negative control. Without step 5 the walk accepts a delta that stages a blob that no occurrence references; the verifier refuses it with `StagedOutsideClosure`, and the delta is not valid. |
| `staged_first_accepts_a_staged_object_owned_only_at_inherited_positions` | The second outcome change. A staged node whose two occurrences, and the content below each, are certified is accepted by the staged-first verifier and refused by the certificate-first verifier in its final check; the delta is not valid. |

Object authentication, the completed-key memo, the cycle check, node-context
and endpoint-policy validation, the payload kind of content and the resource
limits are outside the model. A staged object is modelled by its parsed form;
the canonical decoder makes child edges strictly increasing, so distinct
occurrences have distinct keys and the memo never skips one. Fuel bounds the
descent depth where the Rust walk relies on the finite, acyclic overlay. Exact
paths are byte lists: the path arena gives two paths the same identity exactly
when their bytes are equal.

**The properties.**

| ID | Property | Test |
|---|---|---|
| PX-01 | The coincidence fixture in canonical bytes. The base holds a continuation leaf with an empty prefix over a source; the staged root holds that leaf at edge 1 and a staged branch at edge 2, whose children are the same leaf at edge 3 and a new leaf over a new source at edge 4; the certificates name the leaf as a `NodeEntry` at 1 and the base source as `ContentAtEndpoint` at 2, 3. The verifier stops at the exact certificate and verifies the emitted leaf at 2, 3: it accepts with 7 distinct objects, 5 node objects, 1 payload object and 9 references, accepts within 10 logical visits (the model's 8 and the two empty namespace roots), and with 9 refuses `ResourceLimit` for logical visits, 10 attempted. | `a_staged_node_equal_to_an_inherited_subtree_stops_at_its_exact_certificate` |
| PX-02 | On the same fixture, a leaf certificate that is absent, or that differs from the exact one only in its path (1, 0), its namespace (the data bag), its kind (`ContentAtEndpoint`) or its reference (the sibling leaf), does not stop the walk at 1: all five are refused with `MissingObject` naming exactly the base source. | `an_inexact_boundary_does_not_stop_a_coincident_staged_node` |
| PX-03 | A data-bag leaf staged with the bytes of a base leaf, whose only target occurrence is certified as a `NodeEntry` at `roota`, is refused in the final check with `SemanticCoherence`: "transaction staged an object outside the target delta closure". | `an_object_staged_only_where_it_is_inherited_is_outside_the_delta` |
| PX-04 | The shrunk transition of the defect seals. Its target manifest equals the independent rebuild of the target state; the certificate-first model walk accepts and reaches exactly the staged objects; at least one occurrence is coincident; and the staged-first model walk reproduces the measured refusal, a missing content naming the shared source. | `shrunk_coincident_leaf_transition_seals_and_the_staged_first_control_refuses` |
| PX-05 | On 256 generated transitions, every coherent transition seals, its target manifest equals the independent rebuild, and its sealed objects are published and the target is cold-admitted. The certificate-first model walk accepts and reaches exactly the staged objects; the staged-first walk accepts or misses a node or a content; and the two walks are identical whenever no occurrence is coincident. Some generated transition makes the staged-first walk miss a node, and another a content. | `generated_transitions_with_shared_subtrees_seal_to_the_independent_rebuild` |

**The test model of PX-04 and PX-05.** A model state holds datums of three
texts on three channels, and continuations of three bodies in four channel
groups (alpha; beta; alpha and beta; gamma and alpha), each with a natural or
an independent source, together with the joins that each group requires. A
transition installs the base state, records the difference of two states as a
mutation batch, stages the changed continuation sources and seals. Its target
is compared with an independent rebuild of the target state, never with the
production rewrite. Three generators bias the transitions toward coincidence:

- *uniform*: a random state and one to four edits, each of which sets one slot
  or copies one channel's or one group's slots onto another;
- *node alias*: the texts `datum-pair-a` and `datum-pair-b` share their first
  eleven bytes, so each channel holds a node over its two pair datums whose
  bytes depend only on their multiplicities. The batch makes the second
  channel's pair node equal to the first's and changes the first channel's
  remaining datum, so the first channel's untouched pair node is certified
  under its rebuilt root while an equal node is emitted. The channel's identity
  hash sits in each channel subtree's root prefix, so only such shared inner
  structure, not a copied channel, can alias a node;
- *content alias*: the groups of alpha and beta and of gamma and alpha both
  hold `body-0` with the same independent source; the batch edits the first
  group and inserts a body into the second, which reproduces the shrunk
  transition and its variants.

The two alias generators add up to two random edits. The model walk derives its
certificates independently of the rewrite: it collects every occurrence of the
base image, and a target occurrence is certified exactly when it is a base
occurrence whose parent occurrence is not, which makes the certificates the
maximal shared occurrences of the target. A namespace root counts as having an
uncertified parent, because the manifest has no boundary kind.

`model_property_manifest_is_unique_and_exhaustive` checks these five bindings.
It reads `PathExactIncrementalClosure.v` among its Rocq sources and requires
each of the theory's nine `Print Assumptions` capstones to be cited by a PX row
of the derivation matrix, and the integrated runner to compile the theory. The
PX rows cite no TLA+ symbol: the walk is a sequential function of one sealed
transaction.

**Boundary.**

- The certificates are an input of the theory: how the rewrite records them is
  not modelled, and neither is the claim that every rewrite output is a valid
  delta. PX-05 checks both through outcomes only: the production seal accepts
  every generated transition, and the model walk over independently derived
  certificates accepts it and reaches exactly the staged objects.
- The second outcome change cannot refuse a rewrite's own delta (derived, not
  mechanized). The rewrite stages only the nodes that it builds and the changed
  continuation sources. A built node is never the base occurrence at its own
  path: either the base has no node entered there, as for the remainder of a
  split, or the keys below it changed, and a PATRICIA trie (Morrison, 1968) is
  determined by its keys and values. A changed source sits at an endpoint whose
  value changed. Every generated transition and every persistence test seals.
- Digests are taken as injective on the objects in play: the overlay compares
  full bytes and refuses `ContentCollision` (`store.rs:230-270`).
- Authentication, the memo, the cycle check, node-context and endpoint-policy
  validation and the limits stay with the existing structural tests and the
  path-provenance tests; PX-01 binds the logical-visit limit to the model's
  count.
- No persisted byte, identity, format or wire message changes.

Cited works:

- K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing
  of Haskell programs", *Proceedings of the Fifth ACM SIGPLAN International
  Conference on Functional Programming (ICFP '00)*, 268–279, 2000,
  [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266), for the
  generated properties.
- R. C. Merkle, "A Digital Signature Based on a Conventional Encryption
  Function", *Advances in Cryptology — CRYPTO '87*, Lecture Notes in Computer
  Science, 369–378, Springer, 1988,
  [doi:10.1007/3-540-48184-2_32](https://doi.org/10.1007/3-540-48184-2_32), for
  hash trees, in which equal subtrees are one object.
- D. R. Morrison, "PATRICIA—Practical Algorithm To Retrieve Information Coded
  in Alphanumeric", *Journal of the ACM* 15(4):514–534, 1968,
  [doi:10.1145/321479.321481](https://doi.org/10.1145/321479.321481).

The path-exact closure evidence in the verification record was taken at these
sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/closure.rs` | `5a792b11ab3fa440b9b252fb05829a63c2b4afd810f52b191ae4a93b7af3ce47` |
| `formal/rocq/stack_safe_pda/theories/PathExactIncrementalClosure.v` | `8731049247321418344bf2e9594e9c8e3b835a13b9b9f191d1ebe8563fe8f42d` |
| `formal/rocq/stack_safe_pda/_CoqProject` | `920645613396146c9992f71957ab9ff1cdca895aee840a702a3730008683366f` |
| `scripts/check-stack-safe-pda-proofs.sh` | `69935d0491c349218ecd08bb4869b12c8699bd7d3ca274e43cfeff4e4d279263` |
| `rspace++/tests/persistence/path_provenance.rs` | `fb6cb724340b6d73b8cdda9442f7180adabc8e8343e3c7a344793ab940d208f6` |
| `rspace++/tests/persistence/path_exact_closure_properties.rs` | `29cb16b87b66901af416d39257f884d470072d978281af3f9a618dac8a1956ad` |
| `rspace++/tests/persistence/property_support.rs` | `c45b844dfbe4fa550a799c8be47501a277ff9d0fdeccc1b8433645f7fadb91e5` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `d7c993b31f1289d04376975d869651a1a6d2d7a24b297454ba4936ce261876f6` |

### Trusted successor bind (2026-10-27)

Task 11945, the fourth slice of task 7927 (engineering journal J-067), binds a
sealed transaction to a durable authority core under a *trusted successor
policy*, deriving the successor's authority without physical recovery or cold
admission. The trusted successor section above states the rule
(`trusted_successor_rule`, `TrustedSuccessorCapability.v:1636-1650`), and the
authority core section gives the type it acts on. This slice adds the bind,
records in each sealed transaction which of the two publication modes it was
bound for, and adds production checks to nine of the TS rows: TS-01, TS-03 to
TS-07, TS-16, TS-18 and TS-19. It changes no persisted byte, identity, cold
report or cold-checked outcome. No publication path called the bind when it
landed, and every incremental publication still ran physical recovery and
cold admission: the trusted publication and the journal owner's switch to it
are task 11946 (the trusted successor publication section). Like the rest of
the persistence substrate, the bind is exercised only by tests until task 7931
wires the substrate into RSpace (`rspace/mod.rs:11-18`).

**The policy.** A trusted successor policy, `TrustedSuccessorPolicyV1`
(`durable/admission.rs:108-199`), bounds everything that the bind derives. It
has 32 axes, the 32 of the trusted successor section's exact policy.

- Eleven state ceilings, `TrustedStateCeilingsV1`: the state's distinct
  objects and canonical bytes, the retained states, the retained union's
  objects and bytes, the metadata closure's objects and bytes, the logical
  nodes, the endpoints, the longest logical path in bytes, and the source
  payload bytes.
- Three times seven work budgets, `TrustedTrialWorkV1`, one per closure domain
  in the order current, retained, metadata: the peak count, the union peak in
  objects and in bytes, the scratch peak, the references, the expansions and
  the quoted bytes of that domain's ledger trial.

`trial_limits(domain)` (`durable/admission.rs:174-198`) gives a domain's
ledger trial its seven work budgets and, as the trial's final limits, the
domain's pair of state ceilings: the state objects and bytes for the current
domain, the union pair for the retained domain, and the metadata pair for the
metadata domain. The policy is a plain value, and `UNBOUNDED` sets every axis
to `u64::MAX`.

**The bind.** `bind_trusted_transaction(lease, sealed, policy)`
(`durable/admission.rs:1322-1509`) is a method of the core. It writes no file:
it stages the sealed objects in an in-memory overlay of the core's pinned
store, and every derived value lives in the returned transaction. Its steps
run in this order, and the first refusal ends the bind.

1. ⟨Check the writer and the receipt⟩. The writer preflight refuses
   `AuthorityMismatch` unless the lease shares the core's generation-lease
   registry. The receipt check refuses `AuthorityMismatch` unless the
   receipt's base is the core's state and its profile is `P`
   (`durable/admission.rs:1330-1332`). A foreign, stale or replayed receipt
   names another base, so this one comparison refuses all three.
2. ⟨Check the writer's format⟩. A V2 source under the V1 writer is refused
   `RetentionFormatNotWritable(V2)` (`durable/admission.rs:1246-1253`), as in
   the cold-checked bind.
3. ⟨Run the two semantic trials⟩. The retained-state count is the
   predecessor's plus one, with checked addition. After staging the sealed
   objects, `run_domain_trials` (`closure/ledger_trial.rs:163`) runs the
   current trial, which adds the
   target manifest and removes the base manifest, and then the retained trial,
   which adds the target manifest, each under its `trial_limits`. A refusal
   becomes `TrustedTrial { domain, refusal }`. The retained union must then
   contain the current closure (`require_current_within_retained_union`,
   `durable/admission.rs:1677-1689`), the check that cold sealing makes.
4. ⟨Derive the successor's fields⟩. The maxima are the componentwise maximum
   of the predecessor's maxima and the target's inventory,

   ```math
   m' = \bigl(\max(m_{\mathrm{obj}},\ \mathrm{inv}_{\mathrm{obj}}(t)),\ \max(m_{\mathrm{bytes}},\ \mathrm{inv}_{\mathrm{bytes}}(t))\bigr),
   ```

   where $`m = (m_{\mathrm{obj}}, m_{\mathrm{bytes}})`$ is the predecessor's
   pair of maxima, in distinct objects and canonical bytes, and
   $`\mathrm{inv}(t) = (\mathrm{inv}_{\mathrm{obj}}(t), \mathrm{inv}_{\mathrm{bytes}}(t))`$
   is the same pair for the target state $`t`$, which the current trial
   measured. The shape summary advances along
   the parsed mutation batch and the rewrite report (the shape summary
   section's `successor`).
5. ⟨Write the retention⟩. A V1 source under the V1 writer writes a V1
   manifest whose current state is the target and whose history is the
   predecessor's history followed by the predecessor's current state, and
   runs no metadata trial. A V1 source under the V2
   writer migrates its history once and runs the metadata trial, which adds
   the new anchor and removes none. A V2 source under the V2 writer commits
   the target to its radix and runs the metadata trial, which adds the new
   anchor and removes the predecessor's. A metadata refusal becomes
   `TrustedTrial { domain: Metadata, .. }` (`trusted_retention_error`,
   `durable/admission.rs:2637-2647`).
6. ⟨Check the ceilings⟩. Seven ceilings are checked in this order: state
   objects and state bytes against the maxima, retained states, logical
   nodes, endpoints, the longest logical path and the payload bytes. The bind
   refuses `TrustedCeiling { axis, limit, attempted }` at the first axis whose
   attempted value exceeds its limit (`durable/admission.rs:1443-1485`). The
   union and metadata ceilings need no check here: they are the final limits
   of their trials. A state ceiling below the target's own inventory is
   refused earlier, by the current trial's final limit.
7. ⟨Seal the binding⟩. The validity token advances to the receipt's target,
   and the bind returns the sealed transaction with the binding
   `Trusted { policy, successor }`. The successor, a
   `PreparedTrustedSuccessorV1` (`durable/admission.rs:1806-1813`), holds the
   successor's retention manifest bytes, validity token, shape summary,
   closure ledger, retained-state count and maxima: the fields of the
   successor core that the bind derives. The successor shares the store and
   the registry, its retention format is the writer's, and only publication
   fixes its version and catalog token.

The refusals that other steps can raise keep their existing variants: a
staging failure and the union check (`Inventory`); a mutation batch that does
not parse (`Format`); a counter overflow, a batch that the shape summary's
`successor` refuses, and the validity token's own profile and base checks,
which step 1 already decides (`Canonical`); and a failed retention edit
(`RetentionWrite`).

![The trusted bind of a sealed transaction to a durable authority core. The bind writes no file. In grey, the checks that existed before this slice: the writer preflight, which refuses AuthorityMismatch unless the lease shares the core's registry, the union check, and the unchanged preparation. In red, every refusal: AuthorityMismatch for a receipt whose base is not the core's state or whose profile is not P, which covers foreign, stale and replayed receipts; RetentionFormatNotWritable for a V2 source under the V1 writer; TrustedTrial for the current, retained and metadata trials; TrustedCeiling at the first of the seven state ceilings exceeded; and, at the trusted preflight, AuthorityMismatch, PublicationModeMismatch and TrustedPolicyMismatch. In blue, the derivations: the retained-state count plus one, the in-memory staging, the componentwise maxima, the shape summary, the V1 append and the validity token. In amber, the steps that the trusted policy bounds: the current trial under the first work budget and the state ceilings, the retained trial under the second and the union ceilings, the migration or commit with its metadata trial under the third and the metadata ceilings, and the seven state ceilings. In green, the sealed binding Trusted with the prepared successor, and the accepting preflight. A grey note lists the refusals that keep their existing variants. A purple note states that each mode has its own preflight, so a transaction cannot change its mode or policy after it is sealed, and how the production checks derive the exact policy](figures/trusted-successor-bind.svg)

**The binding.** `SealedDurableTransactionV1` replaces its admission limits
with a `SuccessorBindingV1` (`durable/admission.rs:1834-1850`): either
`ColdChecked { limits }`, which the cold-checked bind seals
(`durable/admission.rs:2234`), or `Trusted { policy, successor }`. Each mode
has its own preflight, and each runs after the authority check, which refuses
`AuthorityMismatch` unless the transaction's predecessor authority and
registry are the publisher's (`preflight_authority`,
`durable/admission.rs:1918-1928`).

| Preflight | `ColdChecked { limits }` | `Trusted { policy, .. }` |
|---|---|---|
| `preflight(limits')` | accepts if $`\mathit{limits}' = \mathit{limits}`$, else `AdmissionPolicyMismatch` | `PublicationModeMismatch` |
| `preflight_trusted(policy')` | `PublicationModeMismatch` | accepts if $`\mathit{policy}' = \mathit{policy}`$, else `TrustedPolicyMismatch` |

The cold-checked row is the former preflight's behavior; the trusted row is
the guard of the theory's `sealed_policy_binding_is_exact`. Because the
cold-checked publication calls `preflight`, a transaction bound for trusted
publication is refused by it before any durable write.

**Production refinement.** The production checks live in
`rspace++/tests/persistence/trusted_successor_production.rs`. Each TS test
function runs its model checks first and then its production check at 64
cases, so the property bindings of the trusted successor section are
unchanged. These maps carry the test model's vocabulary to production.

| Model | Production |
|---|---|
| `Domain`: current, retained, metadata | `ClosureDomainV1` |
| `BudgetAxis`: the seven work axes and the two final axes | `LedgerTrialAxisV1` |
| `CeilingAxis`: the seven ceilings | `TrustedCeilingAxisV1` |
| `PolicyAxis`: the 32 policy axes | one field of `TrustedSuccessorPolicyV1`: a state ceiling, or work budget $`a`$ of `work[i]`, with $`i \in \{0, 1, 2\}`$ for the current, retained and metadata domains |
| `expected_refusal(axis, current, maxima)` | the expected `BindOutcome`: a work axis lowered by one is refused by its domain's trial on that axis; the union and metadata ceilings by the retained and metadata trials on their final axes; a state ceiling by the current trial on its final axis when the target's own inventory equals the maxima, and otherwise by the ceiling check; the other five ceilings by the ceiling check |

**Generated histories.** Every production check runs on generated histories in
four scenarios, the V1 and V2 writers on each of two checkpoint fixtures,
each fixture having the other as its foreign source. A history is up to eleven
operations, each setting the count of one of four datums on one channel to a
value from 0 to 4; an operation that leaves the count unchanged is skipped.
Each step is sealed and bound through the production API and is then published
by the cold-checked incremental publication; since task 11946 the checks of
TS-03, TS-04, TS-06 and TS-07 publish it by the trusted publication instead
and re-admit the successor cold (the trusted successor publication section).
The published source becomes the next step's predecessor.

**Independent measurement.** The exact policy is measured without the bind.
The check runs production `run_domain_trials` and, when the writer is V2,
`run_ledger_trial` for the metadata domain, all under unbounded limits, on the
predecessor's ledger. They read through a test overlay that finds the bound
transaction's sealed objects by digest and otherwise reads the core's indexed
store. The deltas are built in the test from the receipt: the current domain
adds the target and removes the base, the retained domain adds the target, and
the metadata domain adds the new anchor and removes the predecessor's (V2 to
V2), adds it alone (V1 to V2), or is not run (the V1 writer). The *exact
policy* then sets each work budget to the measured usage, each union and
metadata ceiling to the measured final inventory, and the other seven ceilings
to the successor's maxima, retained-state count and shape. Under the V1 writer
the metadata usage and inventory are zero. TS-04 and TS-16 compare those
values with the cold successor after publication, so the policy does not take
the bind's word for them. A *positive
axis* is an axis whose exact value is above zero; only those can be lowered by
one. Every check that expects a refused bind or publication (TS-03 to TS-07
and TS-18) takes a *witness* before the refused calls, the bytes of every file
in the directory except the writer lock together with the core's catalog token
and ledger, and requires it unchanged after them; TS-19 compares the directory
bytes around its refused bind.

| Row | Production check | What it requires at every step |
|---|---|---|
| TS-01 | `production_trusted_successor_equals_cold_successor` | The trusted and the cold-checked binds of one batch, both unbounded, give the same receipt, sealed object bytes, retention manifest and successor inventory. After the cold-checked publication, the trusted successor's version, validity, retention bytes, shape, retained-state count, maxima and per-domain ledger equal those of the cold successor. |
| TS-03 | `production_trials_refuse_in_domain_order` | The trusted ledger equals the independently measured one. Under the V2 writer, the metadata trial alone on the predecessor's ledger has the same usage and the same metadata domain as after the two semantic trials. The exact policy binds the same ledger. One positive work axis is chosen per domain, rotating with the step; lowering any contiguous run of those axes by one is refused by the first lowered domain, on its axis. Refusals occur in all three domains. |
| TS-04 | `production_policy_accepts_exact_and_refuses_one_below` | The exact policy binds. Each positive axis lowered by one is refused as `expected_refusal` predicts, with the lowered value as the limit and the exact value as the attempted one. After publication, the policy's ceilings are the cold successor's maxima, retained-state count and shape, and the measured ledger is the cold ledger. Ceiling and trial refusals both occur. |
| TS-05 | `production_union_budget_admits_scratch_above_final_size` | The exact policy, whose union budgets are the measured peaks, binds. In each domain the union peak is at least the final size in objects and in bytes, and where it exceeds the final size, lowering that union budget to the final size is refused on that union axis: the bind admits scratch above the final size exactly within the budget. Such steps occur. |
| TS-06 | `production_binding_fixes_publication_mode_and_policy` | A trusted-bound transaction passes `preflight_trusted` with its sealed exact policy, is refused `TrustedPolicyMismatch` with any of the 32 axes changed by one up or, where positive, down, and is refused `PublicationModeMismatch` by `preflight`. A cold-bound transaction is refused `PublicationModeMismatch` by `preflight_trusted`, passes `preflight` with its own limits, and is refused `AdmissionPolicyMismatch` under each of three changed limits, which lower one per-state structural limit (distinct objects, canonical bytes or references) from the unbounded $`2^{64} - 1`$ to $`2^{64} - 2`$. The cold-checked publication of a trusted-bound transaction returns `PublicationModeMismatch`, definitely not published. |
| TS-07 | `production_bind_refuses_foreign_stale_and_replayed_receipts` | A receipt sealed in a foreign directory, bound with this directory's lease, and this directory's receipt bound with the foreign lease, are refused `AuthorityMismatch`. The receipt that produced the current state, replayed against it, and an earlier step's receipt (stale), each resealed on the current store, are refused `AuthorityMismatch`. All four cases occur. |
| TS-16 | `production_maxima_equal_cold_requirements` | The trusted maxima are the componentwise maximum of the predecessor's maxima and the target's structural inventory, and equal the cold successor's maxima. The cold successor's per-state checkpoint preflight accepts at the trusted maxima and refuses one below on each positive component, naming that component, the lowered limit and the maximum. |
| TS-18 | `production_validity_binds_the_exact_predecessor_and_receipt` | A receipt with a forged profile is refused `AuthorityMismatch`. The successor's validity token names the target manifest and equals the cold successor's. A batch sealed on the successor is refused against the predecessor and binds against the successor. |
| TS-19 | `production_retention_migrates_once_and_matches_the_cold_seed` | The V1 writer has no metadata domain. Under the V2 writer, the successor's metadata roots are its anchor alone with count 1, its metadata inventory is the measured one, migration happens exactly at the first step, and the cold successor's format and metadata domain are the trusted ones. A V2 directory reopened under the V1 writer refuses a bind with `RetentionFormatNotWritable(V2)`. Migration, V2 successors and that refusal occur. |

**Negative controls.** Each control applies one mutation to the production
sources of an export of the candidate tree, runs the one TS test that names
it, and requires the test to fail inside its production check. The model
checks run first in each test and pass, so every failure below is the
production check's.

| Control | Mutation | Killed by, with the observed failure |
|---|---|---|
| The V2 ledger skips the metadata trial | the V2 commit arm returns the ledger of the two semantic trials | TS-01: the trusted successor differs from the cold successor |
| The retained trial runs first | `run_domain_trials` swaps the domain order | TS-03: lowering the current and retained count budgets is refused by the retained trial, not the current one |
| A ceiling refuses its exact value | the ceiling check compares with $`\geq`$ instead of $`>`$ | TS-04: the exact policy is refused `TrustedCeiling(StateObjects, 3, 3)` |
| The union budget is capped at the final size | the union-objects budget becomes the minimum of the budget and the final limit | TS-05: the exact policy is refused by the current trial on union objects |
| The preflight ignores the sealed policy | `preflight_trusted` accepts any policy | TS-06: a changed policy passes the trusted preflight |
| The base is not checked | the receipt check compares only the profile | TS-07: a foreign receipt is refused only by the current trial (`UnauthorizedRemoval`), after staging |
| The maxima forget the history | the maxima's objects are the target's alone | TS-16: the maxima fall from 3 objects to 2 |
| The profile is not checked | the receipt check compares only the base | TS-18: a forged profile is refused only by the validity token's own check (`Canonical`), after every trial |
| The migration removes the V1 manifest | the migration arm's metadata trial removes the predecessor's V1 manifest | TS-19: the metadata trial refuses that removal (`UnauthorizedRemoval`) under the unbounded policy |

All nine controls were killed: each test exited with status 101, failing in
its production check after 0.03 to 0.32 s. The base and profile controls show
why the receipt check comes first: without it the bind still refuses, but only
after staging the objects, or after every trial, and with another variant.

**Boundary.**

- The bind derives the successor; it does not publish or adopt it. Rows TS-02
  and TS-08 to TS-15, and the publication halves of TS-03, TS-04, TS-06 and
  TS-07, are task 11946 (the trusted successor publication section).
- Each step's successor becomes the next predecessor by cold-checked
  publication or cold re-admission, so these checks compare a trusted
  successor with the cold one at every step; task 11946 adds chains of trusted
  successors.
- The production checks run 64 cases of four scenarios each. Each case opens,
  installs, admits and publishes real directories, so they run fewer cases
  than most model checks.
- No persisted byte, identity, cold report or cold-checked refusal changes.

The trusted successor bind evidence in the verification record was taken at
these sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/transaction.rs` | `9b71c3c51c02ef7eeb1a8157417e0b455158d3a1aa7f8befa316ff89ac26456b` |
| `rspace++/src/rspace/persistence/closure.rs` | `362010373e2d62084a731bddc1c0428de61e23f9b8bea46f9ca3928c86418692` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `13c1360d35d5aadde8735ddd9878fee09cccbf8dafb5a337c3b348fb534be869` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `77bec4e4db3e7bd0a6469596e9bab912d34882463c8eb137fc0fa2cba9319fc4` |
| `rspace++/tests/persistence/mod.rs` | `e063cd4cbdda3daf06397b9e3d1a4155622c97431e56b99ca3c364204657fafe` |
| `rspace++/tests/persistence/durable_publication.rs` | `2036a6a6a492a3e7e7307ec033ea69cfdbe66f836c79f7ebcb60ee4e08488165` |
| `rspace++/tests/persistence/trusted_successor_properties.rs` | `735ebd79949630fab031efd571c0728c528dad164e61b5de566c517e87c8e443` |
| `rspace++/tests/persistence/trusted_successor_production.rs` | `886701b7fbce6607bbb910d471ca1c1620917fcd3ef4184b4d3a40967ffffb9f` |
| `docs/design/pathmap/figures/trusted-successor-bind.puml` | `f8cd102bcec4630ced25f7b966b63741a1e54f2d280dcbb7f74292f1e9542ab5` |

### Trusted successor publication and owner (2026-10-27)

Task 11946, the fifth and last slice of task 7927 (engineering journal J-067),
publishes a trusted-bound transaction and adopts the successor core that the
publication assembled, without physical recovery, an index rebuild or cold
admission, and it switches the journal owner from cold-checked to trusted
publication. The trusted successor section above gives the theory and the
model's six publication steps, the authority core section gives the type that
the publication returns, and the trusted successor bind section gives the bind
whose sealed successor it publishes. This slice adds production checks to
TS-02, TS-08 to TS-15 and TS-17, and to the publication halves of TS-03,
TS-04, TS-06 and TS-07, so every TS row now checks production code. It changes
no persisted byte or identity: a trusted publication writes the bytes that the
cold-checked publication writes (TS-10). The cold-checked publication, startup
admission and the fresh checkpoint keep their behavior and their cold checks.
Like the rest of the persistence substrate, the publication and the owner are
exercised only by tests until task 7931 wires the substrate into RSpace
(`rspace/mod.rs:11-18`).

**The hooks.** Publication passes named test points, compiled only into test
builds, at which a test can record a trace, inject a failure or change a file.
This section numbers them by their position, from zero, in the 18-entry table
`FORMAL_PUBLICATION_PHASE_ORDER`
(`rspace++/tests/persistence/durable_publication.rs:75-94`).

| Hooks | Test points | Passed by |
|---|---|---|
| 0, 1 | `AfterPreflightBeforeCleanup`, `AfterCleanup` | no incremental publication |
| 2 to 5 | `AfterPrepareWrite`, `AfterPrepareSync`, `AfterIndexWrite`, `AfterIndexSync` | both incremental modes |
| 6 to 8 | `AfterCommitWrite`, `AfterCommitSync`, `AfterArtifactDirectorySync` | both incremental modes |
| 9 to 14 | `BeforeCatalogCompareAndSwap`, `AfterCatalogCasVerifiedBeforeSlotWrite`, `AfterCatalogSlotWrite`, `AfterCatalogFileSyncBeforeDirectorySync`, `AfterCatalogDirectorySync`, `AfterCatalogSyncBeforeRecovery` | both incremental modes |
| 15 to 17 | `AfterPhysicalRecovery`, `BeforeSemanticAdmission`, `AfterSemanticAdmission` | the cold-checked mode only |

A trusted publication therefore passes the thirteen hooks 2 to 14, and a
cold-checked one the sixteen hooks 2 to 17.

**The publication.** `publish_trusted_incremental_v1(lease, core, prepared,
policy, retain_terminal)` (`durable/publication.rs:909-970`) takes the
writer's lease, the predecessor core, a transaction that
`bind_trusted_transaction` sealed and `prepare_transaction` prepared, the
trusted successor policy, and a callback that retains the terminal catalog
slot. It returns the successor's `DurableAuthorityCoreV1`. Its steps run in
this order, and the first failure ends the publication.

1. ⟨Preflight the writer and the binding⟩. The writer preflight refuses
   `AuthorityMismatch` unless the lease shares the core's generation-lease
   registry. `preflight_trusted` (`durable/admission.rs:1971-1990`) then
   refuses `AuthorityMismatch` unless the transaction's predecessor authority
   and registry are the core's, `PublicationModeMismatch` unless the sealed
   binding is trusted, and `TrustedPolicyMismatch` unless the call's policy is
   the sealed one. It returns the sealed `PreparedTrustedSuccessorV1`.
2. ⟨Require the base⟩. `require_incremental_base`
   (`durable/publication.rs:972-994`) refuses `BaseStateMismatch` unless the
   receipt's base is the current state, `ProfileMismatch` unless the receipt's
   profile is `P`, `MissingRetentionManifest` unless the transaction names a
   retention manifest, and `CatalogCompareAndSwap` unless the catalog file
   holds exactly the core's 672-byte image.
3. ⟨Plan the append and assemble the index⟩. `IncrementalAppendPlanV1::new`
   (`durable/publication.rs:1167-1192`) derives, as the cold-checked append
   does, the PREPARE record's address at the active pack's appointed tail, the
   index generation, the index records of the transaction's new objects and
   the new index tail $`\mathit{tail}'`$. `trusted_successor_index`
   (`durable/admission.rs:1511-1532`) clones the core's committed index,
   inserts each planned record, and requires the tail that the result implies
   to be the planned one,

   ```math
   64 + 72\,n = \mathit{tail}',
   ```

   where $`n`$ is the successor index's record count and 64 and 72 are the
   lengths in bytes of the index header and of one index record. A record
   already indexed is refused `DuplicateData`, a digest already indexed under
   another reference `DigestReferenceConflict`, a product or sum that
   overflows `RecordCountOverflow`, and an unequal tail `CanonicalStreamTail`;
   a planned record that fails to decode passes its own error through. Each
   becomes `TrustedSuccessorAssembly`, and nothing has been written.
4. ⟨Append⟩. The active pack is truncated to its appointed tail
   (`truncate_incremental_pack`, `durable/publication.rs:996-1008`), and the
   planned PREPARE and index records are written and synced (hooks 2 to 5).
5. ⟨Commit⟩. `commit_incremental_successor`
   (`durable/publication.rs:1010-1088`) derives the successor slot and the
   successor catalog image, passes the slot to `retain_terminal`, writes and
   syncs the commit record, and syncs the artifact directory (hooks 6 to 8).
6. ⟨Assemble the successor core⟩. `assemble_trusted_successor`
   (`durable/admission.rs:1534-1577`) selects the catalog token from the
   successor image and refuses `AuthorityChanged` unless it names the
   committed slot. It pins the store over the new index, which refuses
   `PackTail` when an appointed pack is shorter than the slot authorizes,
   binds the bind's validity token to the slot's state, and builds the core
   from the prepared successor's retention bytes, validity token, shape
   summary, closure ledger, retained-state count and maxima. A failure becomes
   `TrustedSuccessorAssembly`, and the catalog still names the predecessor.
7. ⟨Swap the catalog⟩. `swap_incremental_catalog`
   (`durable/publication.rs:1090-1115`) compares the catalog file with the
   core's image again, refusing `CatalogCompareAndSwap`, writes and syncs the
   inactive slot, and syncs the catalog directory (hooks 9 to 14).
8. ⟨Resolve the sidecar⟩. The publication reads the reclamation sidecar and
   resolves it against the successor slot with
   `resolve_reclamation_candidate_v1` (`durable/startup.rs:303-324`), the
   resolution that physical recovery runs.
9. ⟨Re-read the catalog⟩. The publication reads the catalog image again and
   returns the successor core only if the image is byte-equal to the core's;
   otherwise it reports `CatalogAuthorityChanged`.

These are the model's six steps at a finer grain: steps 1 and 2 are the
model's preflight, step 3 its index assembly, steps 4 and 5 its append, step
6 its staging and pin, step 7 its swap, step 8 its sidecar resolution and
step 9 its adoption. Each failure falls into one outcome class.

| Outcome class | Errors | Raised at |
|---|---|---|
| Definitely not published | `Preparation`, `BaseStateMismatch`, `ProfileMismatch`, `MissingRetentionManifest`, `CatalogCompareAndSwap`, `TrustedSuccessorAssembly`, and the append's and commit's `Append`, `Io`, `LogPack` and `Durable` | steps 1 to 6, and the comparison of step 7 |
| Indeterminate | `CatalogCommitIndeterminate` | the slot write and the file sync of step 7 |
| Published with an error | `PublishedCatalogDirectory` | the catalog directory sync of step 7 |
| Published with an error | `PublishedRecovery` | steps 8 and 9 |

The new variant `TrustedSuccessorAssembly` is definitely not published
(`durable/publication.rs:285`, `:310`): when it is raised, the catalog file
still holds the predecessor's image. TS-11 shows that recovery after such a
failure selects the predecessor and that a retry writes the reference bytes.

The cold-checked publication `publish_incremental_with_terminal_v1`
(`durable/publication.rs:842-907`) now runs through the same helpers: step 1
with `preflight` in place of `preflight_trusted`, then steps 2, 4, 5 and 7,
and then physical recovery, which resolves the sidecar and rebuilds the
index, cold admission under the incoming limits, and the metadata inventory
comparison. It assembles no index and no core, and its writes, hooks and
errors are unchanged.

![The trusted publication of a bound transaction. publish_trusted_incremental_v1(lease, core, prepared, policy, retain_terminal) returns the successor core that it assembled before the catalog swap, with no physical recovery, no index rebuild and no cold admission. In grey, the steps shared with the cold-checked publication: the writer preflight, which refuses Preparation(AuthorityMismatch) unless the lease shares the core's generation-lease registry; require_incremental_base, which refuses BaseStateMismatch, ProfileMismatch, MissingRetentionManifest or CatalogCompareAndSwap unless the receipt's base is the current state, its profile is P, it names a retention manifest and the catalog file holds the core's image; the append plan of the PREPARE address, the index generation, the new objects' index records and the new index tail; the truncation of the active pack to its appointed tail and the writes and syncs of the PREPARE and index records, at hooks AfterPrepareWrite to AfterIndexSync; and the commit step, which stages the successor slot and its image, retains the terminal, writes and syncs the commit record and syncs the artifact directory, at hooks AfterCommitWrite to AfterArtifactDirectorySync. In purple, preflight_trusted, which refuses AuthorityMismatch unless the sealed predecessor authority and registry are the core's, PublicationModeMismatch unless the binding is trusted and TrustedPolicyMismatch unless the policy is the sealed one, and otherwise borrows the prepared trusted successor that the bind sealed. In blue, the trusted derivations: trusted_successor_index, which clones the core's index, inserts each new record and requires the derived tail 64 + 72n to equal the planned tail, refusing TrustedSuccessorAssembly with DuplicateData, DigestReferenceConflict, RecordCountOverflow or CanonicalStreamTail before anything is written; assemble_trusted_successor, which selects the token from the successor image, pins the store over the new index, binds the validity token to the slot's state and builds the core from the prepared successor, refusing TrustedSuccessorAssembly with AuthorityChanged or PackTail while the catalog still names the predecessor; the resolution of the reclamation sidecar against the successor slot, which fails as PublishedRecovery; and the re-read of the 672-byte catalog image, which fails as PublishedRecovery with CatalogAuthorityChanged unless it is byte-equal to the core's image. In amber, the catalog swap: the comparison with the core's image at BeforeCatalogCompareAndSwap and AfterCatalogCasVerifiedBeforeSlotWrite, which refuses CatalogCompareAndSwap; the write and sync of the inactive slot at AfterCatalogSlotWrite and AfterCatalogFileSyncBeforeDirectorySync, whose failure is CatalogCommitIndeterminate; and the sync of the catalog directory at AfterCatalogDirectorySync and AfterCatalogSyncBeforeRecovery, whose failure is PublishedCatalogDirectory. Each red failure names its outcome class: definitely not published, indeterminate, or published with an error. In green, the returned successor core with its authority, store, validity, shape, ledger, retained states and maxima, which carries no cold evidence. Three notes state that a failure before the slot write is definitely not published, one during the slot write or its sync indeterminate, and one after the catalog file is durable published with an error, which the journal owner resolves by cold recovery; that the cold-checked publication runs every grey and amber step with the same bytes, skips the blue ones, and after the directory sync runs physical recovery, cold admission and the metadata inventory check, sixteen hooks against the trusted thirteen; and that the model adopts a successor only after an exact re-read of the catalog (AdoptTrusted, AdoptedSuccessorIsExactCatalog), which costs one 672-byte read per publication](figures/trusted-successor-publication.svg)

**The re-read.** Decision 3 of the reviewed plan for task 7927 (pgmcp
artifact 1114) rejected a re-read of the catalog after the swap: J-067 does
not require one, and damage after the swap is found at the next cold
boundary. The model of task 11942 nevertheless requires one: its adoption
action `AdoptTrusted` re-reads the catalog and adopts the successor only when
the catalog holds the swapped image, otherwise failing as published with an
error; the invariant
`AdoptedSuccessorIsExactCatalog` states it; and the configuration
`RSpaceTrustedIncrementalPublicationWrongCatalogAdoptionMutant.cfg` must
violate it. Step 6 of the trusted successor section's publication is the same
rule. The production publication follows the model, so the model supersedes
decision 3. The re-read costs one 672-byte read per publication and narrows
the trade that J-067 accepts: a catalog changed between the swap and the
re-read is reported as published with an error (TS-07), and only a change
after the publication returns waits for the next cold boundary.

**The journal owner.** `DurableRSpaceAuthorityV1`
(`durable/rspace_authority.rs`) now keeps a `DurableAuthorityCoreV1` in place
of a cold-checked source. `from_view` (`:138-161`) keeps only the view's core
(`into_core`, `:155`), and the owner's limits gain the trusted policy:
`DurableRSpacePublicationLimitsV1` holds the transaction, recovery, wal,
admission and trusted limits (`:30-37`). A checkpoint (`:202-233`) takes the
operation gate's write permit, validates the base and captures the pending
journal. `publish_captured` (`:235-289`) then

1. returns at once, dropping the capture, when the journal names an empty
   batch (`:245-249`);
2. opens the transaction over the core's validity-only verified state under
   the transaction limits, stages the pending continuation sources, seals it,
   and binds it with `bind_trusted_transaction` under the trusted policy
   (`:252-261`);
3. prepares it under the wal limits and revalidates the journal
   (`:262-268`);
4. enters `RecoveryRequired` and publishes with
   `publish_trusted_incremental_v1` under the same trusted policy, retaining
   the terminal slot (`:269-276`);
5. refuses `UnexpectedTerminal` unless the returned core's slot is the
   retained terminal (`:277-283`), and enters `AwaitingAck` with the
   successor core.

A refusal before `RecoveryRequired` drops the capture (`:227-230`), and
nothing has been written. `acknowledge_successor` (`:335-386`) pins the
successor's generation, acknowledges the journal, adopts the core and
releases the permit. `reconcile` (`:291-333`) keeps its decisions: from
`AwaitingAck` it acknowledges (`:298-300`); from `RecoveryRequired` it runs
physical recovery and cold admission under the recovery, wal and admission
limits and keeps only the recovered core (`:302-308`), which is either the
predecessor, adopted at once, or the retained terminal, acknowledged.
Production calls `into_core` only at `:155` and `:308`, and only tests call
`acquire_cold_checked` and `SemanticallyAdmittedDurableWriterV1::from_parts`.

| Branch | Limits consulted | Hooks | Cold checks |
|---|---|---|---|
| Empty journal | none | none | 0 |
| Non-empty journal, published | transaction, wal, trusted | 2 to 14 | 0 |
| Refused at the trusted bind | transaction, trusted | none | 0 |
| Awaiting acknowledgement, retried | none | none | 0 |
| Recovery required, reconciled | recovery, wal, admission | none | 1 |

![The journal owner's checkpoint over trusted successors, a state diagram of DurableRSpaceAuthorityV1, which keeps only a DurableAuthorityCoreV1. In green, Ready: from_view enters it when the hot journal names the view's authority, keeping only the core. A checkpoint takes the operation gate's write permit, validates the base and captures the pending journal, entering Captured, in blue. From Captured, an empty batch validates the base and drops the capture, returning to Ready with no limit consulted, no hook and no cold check; a refusal before publication begins, at the mutation, the transaction, the trusted bind or the preparation, drops the capture with nothing written, no hook and no cold check. Otherwise the transaction is prepared under the wal limits and the journal is revalidated, entering RecoveryRequired, in amber. From there, publish_trusted_incremental_v1 returns the successor core at the retained terminal under the trusted policy, with thirteen hooks and no cold check, entering AwaitingAck, in purple; a failed publication stays in RecoveryRequired, keeping the capture, the permit and the retained terminal; and reconcile runs cold recovery, which either selects the predecessor, adopting its core and returning to Ready under the recovery, wal and admission limits with one cold check, or selects the retained terminal and enters AwaitingAck with one cold check. From AwaitingAck, acknowledge_successor pins the successor, acknowledges the journal, adopts the core and releases the permit, returning to Ready with no limit consulted, no hook and no cold check; when the pin or the acknowledgement fails the owner stays there, and a retry acknowledges once with no hook and no cold check. In red, Invalidated: Ready moves there when the hot journal no longer names the owner's authority, and back when it is restored. A note on RecoveryRequired states that the trusted publication retains its terminal before the swap, and that a successor at another slot, or a recovered slot that is neither the predecessor nor that terminal, is refused UnexpectedTerminal and stays there. A note on Invalidated states that a checkpoint there is refused by the base validation before it captures anything](figures/journal-owner-trusted-checkpoint.svg)

**The fresh checkpoint.** A trusted core carries no cold evidence, so nothing
derived from it can stand for a cold demand. A fresh generation checkpoint
still starts from a cold-checked source:
`SemanticallyAdmittedDurableWriterV1::from_parts(lease, source)` takes one,
and `publish_generation_checkpoint_v1` preflights that source's measured
demand before cleanup, both unchanged. A core reaches them only through
`acquire_cold_checked`, which recovers, admits under the caller's limits and
compares all 672 bytes of the catalog image. TS-08 shows that the demand so
measured is the new state's: it differs from the predecessor's wherever the
state grew, and from a demand derived from the core's summary wherever
subtrees are shared.

**Generated histories.** The production checks run on the generated histories
of the bind section: four scenarios, the V1 and V2 writers on each of two
checkpoint fixtures, and up to eleven operations, each setting the count of
one of four datums on one channel to a value from 0 to 4. Two drivers publish
them.

- `ProductionHistory::publish_trusted` publishes each step by the trusted
  publication and then re-admits the successor cold with
  `acquire_cold_checked` under unbounded limits. It requires the re-admitted
  core to equal the published one, and the re-admitted source becomes the
  next predecessor.
- `TrustedChain` keeps the published core itself as the next predecessor, so
  its histories are chains of trusted successors. The checks compare them
  with a cold-checked history of the same operations.

Each trusted publication of either driver records its hook trace and two
counters, the cold admissions and the constructions of a committed index from
disk. A trusted publication must show hooks 2 to 14 and zero of each; TS-09
also requires a cold-checked one to show hooks 2 to 17 and one of each.

**Production refinement.** The production checks live in
`rspace++/tests/persistence/trusted_successor_production.rs`, except the
owner's, which live in `rspace++/tests/persistence/durable_rspace_authority.rs`,
the crash check, in `durable_publication.rs`, and TS-17's, in
`shape_summary_properties.rs`. Each TS test function runs its model checks
first and then its production checks, so the property bindings of the trusted
successor section are unchanged. The case counts are those of the TS test
functions; each case runs all four scenarios on real directories.

| Row | Production check | What it requires |
|---|---|---|
| TS-02 | `production_trusted_index_equals_cold_recovery`, 64 cases | At every step the core's committed index holds exactly the appointed records on disk. Against those records, `trusted_successor_index` gives what a cold rebuild gives for five plans: an indexed record (`DuplicateData`), an indexed digest under a widened reference (`DigestReferenceConflict`), a fresh record twice (`DuplicateData`), a fresh record and its widened twin (`DigestReferenceConflict`), and a fresh record, which yields one more record. A planned tail one record too long is refused `CanonicalStreamTail`, a record that fails to decode passes its error through, and the predecessor's index is unchanged. After the trusted publication, the appointed records extend the predecessor's, the appointed tail is $`64 + 72n`$, and the published core and its cold re-admission index exactly the appointed records. |
| TS-03 | `production_trials_refuse_in_domain_order`, 64 cases | The bind half of the bind section. The trusted publication under the exact policy then publishes, and the published core's ledger is the measured one. |
| TS-04 | `production_policy_accepts_exact_and_refuses_one_below`, 64 cases | The bind half of the bind section. Each positive axis lowered by one and passed to the publication of the exactly bound transaction is refused `TrustedPolicyMismatch` by its preflight, definitely not published, with no hook. The exact policy publishes; the ceilings then equal the cold successor's maxima, retained-state count and shape, and the measured ledger the cold ledger. |
| TS-06 | `production_binding_fixes_publication_mode_and_policy`, 64 cases | The bind half of the bind section. The trusted publication of a cold-bound transaction is refused `PublicationModeMismatch`, and that of the trusted-bound transaction with any of the 32 axes changed by one up or, where positive, down is refused `TrustedPolicyMismatch`, each definitely not published, with no hook. The exact policy publishes. |
| TS-07 | `production_bind_refuses_foreign_stale_and_replayed_receipts`, 64 cases | The bind half of the bind section. With the catalog's inactive slot changed, the trusted publication is refused `CatalogCompareAndSwap` with no hook; under a foreign lease it is refused `AuthorityMismatch`. On even steps it publishes the bind's successor. On odd steps a hook at `AfterCatalogSyncBeforeRecovery` restores the predecessor's catalog image: the publication passes hooks 2 to 14 and reports `PublishedRecovery(Io(CatalogAuthorityChanged))` for the directory, published with an error; once the published image is restored, cold recovery and admission adopt the bind's successor. All six events occur: the foreign base, the foreign lease, the replayed and the stale receipt, the publication and the restored catalog. |
| TS-08 | `production_fresh_checkpoint_measures_new_cold_demand`, 32 cases | Each step publishes trustedly and measures the successor's eighteen-axis cold demand from its cold re-admission. Nine axes equal the trusted core's own values: the maxima's objects and bytes, the retained states, the retained union's objects and bytes, the logical nodes, the endpoints, the longest logical path and the payload bytes. The exact demand passes the checkpoint preflight, and each positive axis lowered by one is refused on that axis with the lowered limit and the demand. Limits that reuse the predecessor's demand on the four per-state axes are refused at the first of those axes that grew, if any. Limits whose per-state references are derived from the summary as if no subtree were shared, the maxima's objects less one, are refused exactly when the measured references exceed that value. After the history, a fresh checkpoint under the demand lowered by one on a rotating axis is refused `AdmissionPreflight` on that axis with no hook and no file changed; after a restart, cold admission measures the same demand, and a fresh checkpoint under it publishes. The reused-demand refusal, the summary refusal and the fresh checkpoint all occur. |
| TS-09 | `production_trusted_publication_skips_cold_admission`, 32 cases | A trusted chain and a cold-checked history publish the same operations. Every trusted publication shows hooks 2 to 14, no cold admission and no index construction; every cold-checked one hooks 2 to 17 and one of each; and the two cores are equal at every step. |
| TS-10 | `production_trusted_and_cold_publication_write_identical_bytes`, 16 cases | A trusted chain and a cold-checked history start from equal directories after a fresh checkpoint whose reclamation sidecar is absent, deferred by a pin, or reclaimable. After every step the two directories hold identical bytes and the two cores are equal. The first step resolves the sidecar as the test model's `resolve_sidecar` predicts: an absent sidecar leaves every file, a deferred one keeps its intent and the superseded files, and a reclaimable one removes the superseded index, the genesis pack and `reclaim.v1`. The three resolutions differ, and each occurs. |
| TS-11 | `production_trusted_publication_faults_recover_and_retry`, 16 cases, and `production_trusted_crash_recovers_and_retries_every_phase` | A faulted and a reference trusted chain start from a deferred sidecar. A changed endpoint ceiling is refused `TrustedPolicyMismatch` with no hook and no file changed. One fault per step, in rotation: the active pack shortened by one byte at `AfterArtifactDirectorySync` stops the publication after hook 8 with `TrustedSuccessorAssembly(PackTail)`, whose available length is one below the authorized one, definitely not published, after which recovery and a trusted retry publish; a failed catalog directory sync stops it after hook 12 with `PublishedCatalogDirectory`, published; and the reclamation intent corrupted at `AfterCatalogSyncBeforeRecovery` stops it after hook 14 with `PublishedRecovery` (`InvalidData`), published. After each fault and its recovery, the faulted chain's core equals the bound successor and the reference chain's core, and the two directories hold identical bytes. All three faults occur. The crash check runs the incremental crash harness in trusted mode at each of hooks 2 to 14 and at every proper prefix of the catalog slot write. |
| TS-12 | `production_empty_checkpoint_consults_no_limit` | An owner's empty checkpoint, under the normal limits and under minimal limits that set every limit to its least value, returns the current state with no hook and no cold check, leaving the owner ready, the files unchanged and the gate free. With populated journals, the minimal limits refuse the checkpoint with no hook and no cold check, leaving the owner ready at the same state and the files and journals unchanged. |
| TS-13 | `production_awaiting_retry_acknowledges_once_without_cold_check` | A generation-lease counter at $`2^{64} - 1`$ makes the successor's pin overflow, so the owner awaits acknowledgement. A retry under the normal or the minimal limits, which reconciles before it checkpoints, acknowledges the published terminal with no hook, no cold check and no file changed, releases the predecessor's pin and holds the successor's; a second retry is an empty checkpoint at the same state; and both limit sets acknowledge the same state. A failure at `AfterCatalogSyncBeforeRecovery` leaves the owner in `RecoveryRequired`; reconciliation under zero admission limits is refused after one cold check, keeping the terminal and the permit, and reconciliation under unbounded limits acknowledges the terminal after one cold check. |
| TS-14 | `production_trusted_success_does_not_imply_cold_admission`, 32 cases, and `production_trusted_owner_success_skips_incoming_admission_limits` | A trusted chain publishes each step under the unbounded policy. The cold demand of its successor, lowered by one on a rotating positive axis, gives the incoming limits, under which a cold-checked history refuses the same step: at its bind (`AdmissionPreflight`, `Inventory` or `RetentionWrite`), or after its publication (`PublishedAdmission`, published). Both occur, and the two cores stay equal. In the owner witness, a checkpoint of populated journals under zero admission limits publishes trustedly with hooks 2 to 14 and no cold check; cold admission of the same directory under those limits is refused, and under unbounded limits it admits the same state. |
| TS-15 | `production_trusted_policy_refusal_changes_nothing` | A twin directory measures the exact policy of the owner's journal mutation. Every positive axis lowered by one is refused at the owner's trusted bind as `expected_refusal` predicts, with no hook and no cold check, leaving the owner ready at the same state, the files and journals unchanged and the gate free; ceiling and trial refusals both occur. The exact policy publishes the measured target with hooks 2 to 14 and no cold check. |
| TS-17 | `check_trusted_durable_chain`, the corner chain | Each corner transition binds and publishes trustedly under the unbounded policy. The published core's shape equals the summary derived at the bind and the model state's cold shape; the cold-checked acquisition of each trusted core before its step, and the restart admission at the end, agree with the trusted summary. |

**Reachability.** A publication never plans a duplicate record. The planner
marks an object that the store already holds as reused
(`LogPackObjectPlanV1::classify`, `wal/disposition.rs:93-152`), the index
append skips reused objects (`PreparedIndexAppendV1::records`,
`durable/index.rs:734-750`), and the planned tail is the old tail plus 72
bytes per new object (`durable/index.rs:704-717`). TS-02 therefore reaches the
duplicate, conflict and tail refusals by calling `trusted_successor_index`
directly. `RecordCountOverflow` needs more than

```math
\left\lfloor \frac{2^{64} - 1 - 64}{72} \right\rfloor = 256\,204\,778\,801\,521\,549
```

records, about $`2.56 \times 10^{17}`$, which no test can build; the
derivation uses checked arithmetic (`durable/admission.rs:1520-1524`).

**The test model's faults.** The test model's six publication faults
(`PublicationFault`, `trusted_successor_properties.rs:519-526`, whose
outcomes `fault_outcome` gives at `:701-717`) map to these production checks.

| Fault | Model outcome | Production check |
|---|---|---|
| `NoFault` | `TrustedReady` | every trusted publication above |
| `PinFault` | definitely not published, at the pin | TS-11: a pack one byte short is refused `TrustedSuccessorAssembly(PackTail)` after hook 8 |
| `CatalogMismatch` | definitely not published, at the catalog | TS-07: a changed catalog is refused `CatalogCompareAndSwap` with no hook; production compares the catalog at step 2 and again at step 7 |
| `CatalogInterrupted` | indeterminate | TS-11's crash check: a failure at hook 11 and every torn slot prefix are indeterminate |
| `DirectorySyncFault` | published, `DirectorySyncFailed` | TS-11: `PublishedCatalogDirectory` after hook 12 |
| `SidecarFault` | published, `SidecarResolutionFailed` | TS-11: `PublishedRecovery` from a corrupted intent after hook 14 |

The re-read's failure, which the TLA+ model reaches by `ExternalCatalogChange`
before `AdoptTrusted`, is TS-07's restored catalog.

**Negative controls.** Each control applies one mutation to the production
sources of an export of the candidate tree, runs the one TS test that names
it, and requires the test to fail inside its production check. The model
checks run first in each test and pass, so every failure below is the
production check's.

| Control | Mutation | Killed by, with the observed failure |
|---|---|---|
| The index tail is not checked | `trusted_successor_index` returns the index without comparing tails | TS-02: a planned tail one record too long yields a four-record index instead of `CanonicalStreamTail { expected: 352, appointed: 424 }` |
| The successor keeps the predecessor's ledger | `assemble_trusted_successor` copies the predecessor core's ledger | TS-03: the cold re-admission of the published successor differs from the published core |
| The publication ignores the policy | the guard of `preflight_trusted` compares the sealed policy with itself | TS-04: a lowered policy passes the publication's preflight and runs thirteen hooks |
| The publication checks only the state ceilings | `preflight_trusted` compares only the policies' state ceilings | TS-06: a policy with a changed work budget passes `preflight_trusted` |
| No re-read | the publication returns the core without re-reading the catalog | TS-07: the publication accepts the restored predecessor catalog |
| The fresh checkpoint skips its preflight | `publish_generation_checkpoint_v1` drops its measured-demand preflight | TS-08: the checkpoint under a lowered demand runs publication hooks |
| Cold re-admission after the swap | the trusted publication runs physical recovery and cold admission after the swap | TS-09: the trusted publication shows one cold admission and one index construction |
| The sidecar is not resolved | the publication skips `resolve_reclamation_candidate_v1` | TS-10: the trusted and cold-checked directories hold different `catalog.v1` and sidecar bytes |
| Assembly after the swap | `assemble_trusted_successor` runs after `swap_incremental_catalog` | TS-11: the shortened pack stops the publication after hook 14 instead of hook 8 |
| The empty checkpoint runs a cold check | the empty-batch branch calls `acquire_cold_checked` | TS-12: the empty checkpoint runs one cold check |
| An awaiting retry reconciles cold | `reconcile` drops its awaiting-acknowledgement branch | TS-13: the retry runs one cold check |
| The owner cold-checks its successor | `publish_captured` calls `acquire_cold_checked` on the published core under the incoming limits | TS-14: the owner's checkpoint under zero admission limits is refused `AdmissionReadLimit` on object-read bytes (limit 0, attempted 136) |
| The owner binds unbounded | `publish_captured` binds and publishes under the unbounded policy | TS-15: the policy with the state-objects ceiling lowered by one is not refused, and the checkpoint publishes |
| The successor keeps the predecessor's shape | `assemble_trusted_successor` copies the predecessor core's shape | TS-17: the published shape has 3 logical nodes where the derived summary has 19 |

All fourteen controls were killed: after the clean export passed all fourteen
tests, each mutant's test exited with status 101, failing in its production
check after 0.02 to 2.57 s (pgmcp artifact 1136). The ledger, shape and
re-read controls show that the comparisons with cold re-admission, not the
types, keep a trusted successor honest: each mutant compiles and publishes.

**Boundary.**

- With this slice task 7927 is complete. Every TS row checks production code,
  the trusted publication returns a core, and the journal owner publishes
  trusted successors and runs a cold check only when it reconciles.
- TS-11 injects one of three faults at each step of its histories, and its
  crash check covers every trusted hook at a single step, all in-process on
  real files. Crossing generated histories with failpoints at every hook,
  eviction and reset-target references, pinned readers and cancellation is
  task 7928 (the faulted trusted histories section), and the scaling
  measurement against geometrically growing history is the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29):
  thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history.
- No persisted byte, identity, cold report or cold-checked outcome changes.
- The owner stays out of RSpace until task 7931 wires the substrate in.
- A catalog damaged after the publication returns is the trade that J-067
  accepts: the next cold boundary finds it.
- The production checks run 64, 32 or 16 cases of four scenarios each, fewer
  than most model checks, because every case publishes real directories.

The trusted successor publication evidence in the verification record was taken
at these sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/publication.rs` | `801348d8e8a547db88596136103bf45dab03d645208c2e0658445b5e49c30de6` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `eef3657433921ca66c94d3e24c028d903f7599de18d9c4510e797b419c9b1671` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `143f15ce54a0d20d1fe64b21c3f6a207331f1e75c1464ed4895dd7d2c35143e8` |
| `rspace++/src/rspace/persistence/durable/rspace_authority.rs` | `9c5acd88d7ac44719ddce6d7f6cd846a1c592bde831417c630767f8237921e70` |
| `rspace++/tests/persistence/durable_publication.rs` | `76e39365802ec016e9a7379445313e5ae2685743c1ab8fb44f9728bdb0c5e373` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `bb4b3d4ef20837cbd4d6945abd81002288d2027abb934c7a7a0afef82ccca9c5` |
| `rspace++/tests/persistence/shape_summary_properties.rs` | `5189ecea7cf747fce9cb3697817d3a46fdb87b1a1a8199e96f6bf997a9b65625` |
| `rspace++/tests/persistence/trusted_successor_production.rs` | `a0fa6f8144c5a296485af125bb3497d208908841d3f78aa2ff97abc163b4a5b9` |
| `rspace++/tests/persistence/trusted_successor_properties.rs` | `bc1cea246be68a645622cc88d3d960a3166556e5af93c40e99f42582a3285d12` |
| `docs/design/pathmap/figures/trusted-successor-publication.puml` | `59b2f5878fa1ef73c1f0fc01dde8b95471960d789e3c5a7e2c5de189b0609a3c` |
| `docs/design/pathmap/figures/journal-owner-trusted-checkpoint.puml` | `162838f6d92c9316cd545a563b2a1a03ba9a7f61b66ea55b4cf1acd57cfbc418` |
| `docs/design/pathmap/figures/durable-authority-core-split.puml` | `15269e5faa5610ab3689bab10393453caea73dd9faa759e727c04ff3343283ba` |

### Faulted trusted histories and cold restart equivalence (2026-10-27)

Task 7928 checks that a history of trusted successors keeps one exact
authority, equal to independent cold admission, while its publications fail,
its writer restarts, a reader pins its generations and the environment binds a
stale copy to the store's pathname. Its reviewed plan (pgmcp artifact 1140)
splits the task into four slices. This section records the first, task 11966:
the formal models, and the test-model extraction of a new property family,
TH-01 to TH-08. Task 11967 adds the production halves of TH-01 to TH-07 on
real files, task 11968 the production half of TH-08 through the journal owner,
and task 11969 the production negative controls, the timing and the completion
of this section and of engineering journal J-253. The slice changes no
production code, so until those tasks every TH row below is a test-only model
extraction, as the TS rows were after the trusted successor section's first
slice.

**Scope.** The task asks for the comparison with cold admission across
replacement, removal, sharing, retention, eviction and reset-target
references; under faults at the physical failpoints, overflow, scratch and
final limit errors, cancellation, ambiguous catalog publication and
acknowledgement retry; with one sole authority that never replays the journal
and never reopens the store by pathname. The reviewed plan settles what each
of these means against the code.

- *Eviction.* Retention keeps all history. Every sealed transaction computes
  its retention step with `commit_retention_v2` or `migrate_retention_v1`
  (`durable/admission.rs:1397`, `:1418`, `:2258`, `:2277`); a fresh generation
  checkpoint rewrites the admitted retained closure and takes no retention
  parameter (`durable/publication.rs:563-571`); and `evict_retention_v2` is
  reachable only from tests (`retention/v2_writer.rs:62-67`). No production
  history can therefore evict a retained root. The family covers physical
  eviction instead: reclamation of a superseded generation, deferred while a
  reader pins it and completed after the unpin, crossed with faults, including
  a crash in the middle of reclamation. At every generated step it also checks
  the premise under which the trusted maxima are exact (TS-16): the
  successor's history holds every predecessor entry and the predecessor's
  current state, one entry more than the predecessor's history. A semantic
  eviction would need the maxima recomputed
  (`eviction_requires_recomputation_counterexample`) and an extension of this
  family.
- *Reset-target references.* A reset publishes an earlier retained state at a
  fresh generation. Generated histories reach that target without a reset:
  steps whose next state equals a retained earlier state, readers that pin and
  read retained generations, and reset-shaped binds from retained states,
  which must be refused before any mutation after faults and restarts. The
  alias lookup, reset identity and activation remain tasks 7931 and 7756.
- *Overflow, scratch and final limits.* Overflow is the generation-lease
  counter's: at the owner's successor pin it leaves the owner awaiting
  acknowledgement, and at a reader's pin it is refused with nothing changed.
  The scratch limits are the trial axes `Scratch`, `UnionObjects` and
  `UnionBytes`, and the final limits are `FinalObjects` and `FinalBytes`
  (`closure/ledger_trial.rs:19-26`) and the six state ceilings; both appear as
  refusals interleaved in generated histories. Three overflows need a 64-bit
  counter to overflow, which no test can bring about without a production
  seam: the canonical counters' `CanonicalFormatError::CounterOverflow`, the
  retained-root registry's `RetentionRegistryError::CounterOverflow`, and the
  reclamation's pack count and range (`DurableIoError::SuccessorOverflow`,
  `durable/io.rs:1283-1302`). They are listed here, and no seam is added.
- *Cancellation.* The operation gate is the owner's only await
  (`durable/rspace_authority.rs:213`), and the trusted publication is
  synchronous, so a checkpoint can be cancelled only while it waits for the
  gate; generated owner histories cancel it there.
- *Independent cold admission.* The comparison at every step admits a byte
  copy of the store in a fresh directory, written with plain writes, mode
  0o600 and no hard links, under its own lease. An in-process reopen is not
  independent: the generation-lease registry is keyed by the directory's
  device and inode (`retention.rs:574-598`), and the writer lock refuses a
  second writer of the live directory (`durable/io.rs:907-915`). Restart is a
  generated action, which drops the writer and reopens the store by path, and
  every history ends with one.
- *Kills at every hook.* Every hook that a trusted publication passes is
  killed in a child process at a later history step, after an earlier fault
  and its recovery. The child aborts at the hook, without unwinding; the parent
  reopens the store by path, admits it cold, requires the predecessor or the
  exact successor and the committed appointed prefix, retries, and compares the
  result with a reference chain. No destructor in the persistence substrate
  writes a file (`durable/io.rs:1012-1021`, `retention.rs:464-475`,
  `:714-723`), so an in-process failure followed by its drops leaves the files
  that a kill at the same hook leaves; the child processes check it. Power
  loss is not modelled.
- *Defects.* A defect that a TH check finds is diagnosed from a model
  counterexample first and fixed, under its own bug item, before task 7928
  closes; no TH test is left failing or filtered.

**The history model.** `RSpaceTrustedIncrementalHistory.tla` instantiates the
single-publication model `RSpaceTrustedIncrementalPublication.tla` as `P` and
composes its publications into histories of at most `MaxSteps` commits. It adds
what happens between publications: the writer settles each outcome, recovers
through its held lease after an ambiguous one and restarts by path; one reader
pins the appointed prefix; and the environment binds a stale copy, a *decoy*,
to the store's pathname, replaces the live catalog with an image that the
writer did not write, and repairs it. A history record abstracts the store.

| Field | Meaning |
|---|---|
| `durable` | the number of commits that the live catalog appoints |
| `log` | the commits in the appointed prefix, a ghost sequence in which commit $`k`$ is the integer $`k`$ and `Orphan`, 0, a commit left behind the appointed tail |
| `suffix` | whether bytes exist behind the appointed tail |
| `core` | the generation that the writer's in-process authority names, or `NoGen`, $`\mathit{MaxSteps} + 1`$, when it holds none |
| `mode` | `idle`, `publishing`, `recovery` or `damaged` |
| `pre` | the predecessor generation of the current or last attempt |
| `pinned`, `pinnedLog` | the reader's pinned generation, or `NoGen`, and the prefix that it pinned |
| `restarts` | the restarts so far, at most `MaxRestarts` |
| `decoy` | whether a decoy is bound to the store's pathname |
| `foreign` | whether the live catalog holds an image that this writer did not write |
| `atBegin` | `log`, `suffix` and `durable` at the last `Begin` |
| `last` | the tag of the last event |

| Action | Effect |
|---|---|
| `Begin` | From an idle writer whose authority names the live catalog's generation, below `MaxSteps` commits and with no foreign image, starts a publication from one of five scenarios: the sealed trusted publication under the sealed policy, or one that the bind's trials refuse (a margin one below the measurement), that the preflight's policy check refuses (a changed policy), that its mode check refuses (a cold call), or that the successor index assembly refuses (a conflict). |
| `Publish` | Takes one step of the single-publication model, its faults and its external catalog change included, while the run is not terminal. |
| `Settle` | Applies the terminal outcome. Ready adopts the successor and appends it to the log. A definite non-publication keeps the predecessor authority and leaves a suffix if the run appended. An indeterminate catalog write appoints the successor only if the new image persisted, and a published error appoints it. Both leave the writer without authority and recovering, or damaged after a published error that left another image in the live catalog. |
| `Recover` | Held-lease recovery through the pinned directory adopts the generation that the live catalog selects; it reads neither the suffix nor the pathname. |
| `RecoverRefused` | Recovery against a foreign image leaves the writer damaged. |
| `ReplaceCatalog`, `Repair` | The environment replaces the live catalog with an image that the writer did not write, and later restores the writer's last image. |
| `Restart` | Drops the writer and its reader and reopens the store by path, never under a decoy. A publication abandoned during the slot write leaves the old, a torn or the new image; one abandoned elsewhere leaves the image that its run left. A restart that finds a foreign image leaves the writer damaged. |
| `PinReader`, `UnpinReader` | The reader pins the idle writer's generation and prefix, and releases them. |
| `PlaceDecoy`, `RemoveDecoy` | The environment binds a decoy while the writer is idle or recovering, and removes it. |

Ten invariants hold in the base configuration.

| Invariant | Statement |
|---|---|
| `HistoryTypeInvariant` | The single-publication type invariant, and the types of the history record. |
| `HistorySoleAuthority` | An idle writer's authority names exactly the generation that the live catalog appoints. |
| `HistoryAuthorityIsCatalogSelected` | After a recovery or a restart the authority is the catalog's selection, the predecessor or the successor of the last attempt. |
| `HistoryRecoveryRequiredAfterAmbiguity` | After an indeterminate or published-error outcome the writer holds no authority and is recovering or damaged. |
| `HistoryAppointedPrefixIsCommitted` | The appointed prefix holds commits 1 to `durable`, in order. |
| `HistoryCommitsAtMostOnce` | No commit appears twice in the appointed prefix. |
| `HistoryPinnedPrefixStable` | The reader's pinned prefix is a prefix of the appointed one. |
| `HistoryRefusalPreservesFiles` | A refused publication leaves the log, the suffix and the appointed count as they were at its `Begin`. |
| `RunOutcomeBoundaryIsExact` | The single-publication model's outcome boundary, `TrustedOutcomeBoundaryIsExact`, at every history step. |
| `RunAdoptedSuccessorIsExactCatalog` | The single-publication model's exact re-read, `AdoptedSuccessorIsExactCatalog`, at every history step. |

Each negative control adds one action to `Next`, and its configuration checks
the type invariant and the invariant that it must violate.
`MisclassifiedCatalogWrite` is the first negative control of
`TrustedOutcomeBoundaryIsExact`, which no configuration of the
single-publication model violated.

| Negative control | Defect | Violates |
|---|---|---|
| `AdoptPreparedOnIndeterminate` | an indeterminate catalog write whose image stayed old or torn adopts the prepared successor | `HistorySoleAuthority` |
| `SuffixReplay` | recovery adopts the synced commit left behind the appointed tail | `HistoryAuthorityIsCatalogSelected` |
| `PathnameRecovery` | recovery reopens the store by path and, under a decoy, adopts the stale copy's generation | `HistoryAuthorityIsCatalogSelected` |
| `PathnameReread` | adoption re-reads the catalog by path, where a decoy holds the expected new image while the live catalog holds another | `RunAdoptedSuccessorIsExactCatalog` |
| `StaleWriterRestoration` | an ambiguous outcome hands the predecessor authority back to the writer | `HistoryRecoveryRequiredAfterAmbiguity` |
| `AppendWithoutTruncation` | a publication that began over a suffix appends behind it | `HistoryAppointedPrefixIsCommitted` |
| `StaleCoreRetry` | after an ambiguous outcome that appointed the successor, a retry from the stale authority skips the base check and commits the transaction again | `HistoryCommitsAtMostOnce` |
| `TruncateBelowAppointed` | truncating a suffix also cuts the last appointed commit | `HistoryPinnedPrefixStable` |
| `TruncateOnRefusal` | the suffix is truncated before the preflight, so a refused publication changes the files | `HistoryRefusalPreservesFiles` |
| `MisclassifiedCatalogWrite` | a failed slot write whose new image persisted is reported as definitely not published | `RunOutcomeBoundaryIsExact` |

Each witness must be violated by the faithful specification, which shows that
the histories over which the invariants quantify occur.

| Witness | History that violates it |
|---|---|
| `NeverRecoveredSuccessorAfterAmbiguity` | recovery selects the successor after an ambiguous outcome |
| `NeverRecoveredPredecessorAfterAmbiguity` | recovery selects the predecessor after an ambiguous outcome |
| `NeverRetriedAfterSuffix` | a publication that began over a suffix becomes ready |
| `NeverReaderAcrossFault` | the reader stays pinned across a definite non-publication or an ambiguous outcome |
| `NeverRestartedMidPublication` | a restart abandons a publication |
| `NeverRecoveredUnderDecoy` | recovery completes while a decoy is bound |
| `NeverRefusedWithSuffix` | a refused publication leaves an existing suffix in place |
| `NeverRepairedForeignCatalog` | the environment repairs a foreign catalog image |

**Model checking.** Under `MaxSteps = 3` and `MaxRestarts = 1`, TLC explored
the base configuration from its one initial state, generating 19,420 states,
**6,841** of them distinct, to depth 46, with the ten invariants satisfied.
Each of the ten negative controls violated its named invariant, and each of the
eight witnesses was reached.
`bash scripts/check-rspace-trusted-history-model.sh` runs the base
configuration and the eighteen checks with the shared bounded TLC helper (a
512 MiB heap, two workers, a 2 GiB memory cap and 120 s) and requires every
counterexample to name its invariant; the integrated stack-safe runner calls it
before the journal owner's checker.

The history needs the single-publication model's initial run as a value, so
that model now defines
`InitialRun(sealedMode, callMode, margin, callPolicy, assembly, coldLimit)`,
the record that its `Init` wrote out, and its `Init` quantifies over the
arguments. The refactor changes no behavior: TLC again generated 664 states,
**588** of them distinct, to depth 13, with its nine invariants satisfied, and
its eight negative controls and six witnesses passed, as the TS rows'
transliteration requires.

**The journal owner model.** `RSpaceJournalPublicationOwner.tla` gains the
decoy. `PlaceDecoy` and `RemoveDecoy` bind and unbind it while an operation is
open, and `CrashAndReopen`, a reopen by path, is disabled while it is bound.
Two faults misuse it.

| Fault | Defect | Violates |
|---|---|---|
| `journal-replay` | `CrashAndReopen` keeps the volatile journal | `ReopenDiscardsVolatileJournal` (JO-11), whose first negative control it is |
| `pathname-recovery` | `Recover` accepts the old report as exact while a decoy is bound | `RecoveryAcceptsOnlyExactTerminal` (JO-09) |

The witness `NeverReconciledUnderDecoy` shows that an exact recovery accepted
while a decoy is bound is reachable. TLC explored the extended model from its
four initial states, generating 1,716,908 states, **64,080** of them distinct,
to depth 21 (44,272 before the decoy), with its seventeen invariants
satisfied, and its nineteen causal mutants and eleven witnesses passed their
designated checks (`bash scripts/check-rspace-journal-owner-model.sh`). The
executable publication-owner composition table binds the two faults.

**The Rocq theory.** `TrustedSuccessorCapability.v` gains a theory of faulted
histories.

- `admissible_recovery` relates a faulted publication's outcome to the cores
  that recovery may select. A definite non-publication keeps the predecessor.
  An indeterminate commit recovers the predecessor or the successor of the
  fault-free publication of the same receipt, whichever the catalog appoints.
  A published failure recovers that successor, whose catalog swap is durable
  although a later sync or the sidecar resolution failed. A ready publication
  adopts its successor. `every_faulted_publication_admits_a_recovery` shows
  that no outcome leaves a history stuck.
- `faulted_history` composes faulted publications, each followed by an
  admissible recovery. `faulted_history_is_trusted_history` and
  `trusted_history_is_faulted_history` show that faulted and fault-free
  histories from the same start reach the same cores: a step that recovers its
  predecessor is a stutter.
- `faulted_pack_history` adds the pack, a list of bytes of which the catalog
  appoints a prefix. A step refused before its first mutation writes nothing;
  any other step that recovers its predecessor leaves what it wrote, torn or
  whole, behind the appointed prefix; and a step that recovers its successor
  truncates the pack to the appointed prefix, appends its transaction, and has
  the catalog appoint it.

| Capstone | Statement |
|---|---|
| `faulted_history_keeps_an_exact_sole_authority` | Every admissible recovery is the predecessor or the fault-free successor; every faulted history is a trusted history; and from a cold-admitted base the final core is an exact authority equal to cold admission of its final state (`trusted_inventory_equals_cold_admission`). |
| `faulted_history_appoints_each_commit_once` | The appointed prefix of a faulted history's pack is the initial pack followed by the committed steps' transactions, in order, each once and never an orphan, and the final core's index records are the start's followed by the same plans. |
| `ambiguous_adoption_mutant_breaks_sole_authority` | On the skipped-trial fixture an interrupted catalog swap is indeterminate. When the old slot persisted, recovery from the catalog holds the predecessor, which is admissible, while adopting the prepared successor holds a core that the catalog never appointed. |
| `append_without_truncation_mutant_appoints_an_orphan` | After a torn write kept the first byte of a two-byte transaction, a retry that appends behind it appoints the orphan byte and the transaction's first byte, while one that truncates first appoints exactly the transaction. |

With these four capstones the theory compiles with **20** capstones closed
under the global context, sixteen before.

**Transliteration.** `rspace++/tests/persistence/trusted_history_properties.rs`
transliterates the history model. `HistoryState::step` implements each action,
the negative controls' actions included, and its `Publish` steps the
single-publication transliteration's `PublicationRun`. A breadth-first search
from the initial state reaches exactly TLC's 6,841 states. The first two TH
test functions divide the ten invariants, ten negative controls and eight
witnesses between them by row: in every reachable state each invariant holds,
each negative control reaches a state that violates its invariant, and each
witness reaches a state that violates it.

**Generated model chains.** Each test function then runs a generated chain on
the trusted successor section's test model, over its generated histories of
one to four steps. A step's eight fault choices are the six publication
faults, the interrupted catalog swap with each of the old, a torn and the new
image. The authority and pack chains replay every publication's trace through
the transliteration, so each of their steps is also a history step that keeps
the invariants.

- *The authority chain*, 256 cases. At every step, from a state with a pinned
  reader and a decoy, it settles all eight fault choices, recovers or repairs,
  and requires the recovered core to be the predecessor or the fault-free
  successor, as the persisted image selects, and to equal the test model's
  cold admission of its state; the drawn choice then advances the chain from
  its recovered core. Successor and predecessor recoveries after ambiguity,
  readers pinned across faults, recoveries under a decoy, repaired catalogs
  and adoption controls, in which an indeterminate swap whose image stayed old
  or torn recovers a core other than the prepared successor, all occur.
- *The pack chain*, 256 cases. At every step a changed-policy refusal must
  write nothing and leave the pack, its suffix and the appointed count as they
  were, and a restart at each image that an interrupted trusted hook leaves
  must recover the prefix that the model selects. After the drawn fault the
  prefix recovered from the pack must equal the committed transactions, and,
  for a commit that began over a suffix, appending without truncation must
  recover a different prefix. Retries over a suffix, refusals with a suffix,
  restarts mid-publication and orphaned attempts all occur.
- *The owner histories*, 128 cases. At every step, all eight fault choices
  crossed with four interleavings run through the owner mirror: uninterrupted,
  a recovery under tightened cold limits, an unwind after the acknowledgement,
  and a restart. A checkpoint publishes once with no cold check and is
  accepted only without a fault, acknowledging the successor once. Otherwise
  the owner requires recovery, and one cold check then acknowledges the
  successor that the catalog appoints, or recovers the predecessor and
  publishes again. A tightened recovery is refused after one cold check,
  publishing nothing; an awaiting retry acknowledges once with no cold check
  and no publication; and a restart reopens the owner with an empty journal,
  whose checkpoint changes nothing. No generation is acknowledged twice in a
  history. Successor reconciliations, predecessor retries, awaiting retries,
  tightened refusals and discarded journals all occur.

**Corrections to the owner mirror.** Composing the trusted successor section's
owner mirror (`trusted_successor_properties.rs`) into histories exposed two
departures from production, both corrected here.

1. A definite non-publication now depends on where it occurred. A refusal at
   the trusted bind returns the owner to ready and drops the capture
   (`durable/rspace_authority.rs:225-231`). One at the preflight, the
   successor assembly, the pin or the catalog leaves the owner requiring
   recovery, because production enters `RecoveryRequired` before it publishes
   (`:268-276`). The mirror had returned every definite non-publication to
   ready.
2. The mirror names an owner state by the generation of its current retention
   entry (`owner_state`), not by the core's validity token. The generated
   histories draw each step's manifest from a few states, so two generations
   can share a token, and acknowledging each generation at most once needs one
   name per generation.

The model halves of TS-12 to TS-15 pass with the corrected mirror.

**The TH rows.**

| ID | Required property | Test |
|---|---|---|
| TH-01 | Sole authority. After every faulted step of a generated history the writer holds exactly one authority, the predecessor or the exact successor that the live catalog selects. An ambiguous outcome leaves it none until held-lease recovery, and an indeterminate swap whose catalog kept its old or a torn slot never adopts the prepared successor. | `faulted_trusted_histories_match_independent_cold_admission_at_every_step` |
| TH-02 | Independent cold admission. At every step the authority equals cold admission of the same state by an admission that shares no lease, registry or process state with the writer: the test model's cold admission in the model half, and admission of a byte copy in a fresh directory in the production half. | `faulted_trusted_histories_match_independent_cold_admission_at_every_step` |
| TH-03 | Each commit appointed once. A failure at any later history step recovers the appointed prefix of the committed transactions, and the next publication truncates a pack suffix that an earlier failure left behind it and refuses an index suffix, definitely not published, until held-lease recovery rebuilds the index, so no commit is appointed twice and no orphan is appointed. | `every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps` |
| TH-04 | Refusals change no file. A publication refused before its first mutation leaves the pack, any suffix behind the appointed prefix, the index and the catalog as they were. | `every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps` |
| TH-05 | Pinned readers and physical eviction. A reader's pinned prefix stays a prefix of the appointed one across faults, recoveries and restarts, and reclamation of a superseded generation waits while a reader pins it and completes after the unpin, after a crash in the middle of reclamation too. | `faulted_trusted_histories_match_independent_cold_admission_at_every_step` |
| TH-06 | Recovery after ambiguity, restart anywhere. After an indeterminate or published-error outcome the writer acts again only after recovery, and a restart at any point, mid-publication included, adopts the generation that the catalog selects. | `every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps` |
| TH-07 | No pathname authority. Recovery, the re-read after the swap and the base check read through the pinned directory, so a decoy bound to the pathname is never adopted, and a foreign catalog image is refused until it is repaired. | `faulted_trusted_histories_match_independent_cold_admission_at_every_step` |
| TH-08 | Owner histories. Across faults, recoveries under tightened limits, acknowledgement unwinds and restarts, the journal owner publishes each step once, holds its gate until the outcome is resolved, accepts recovery only of the exact terminal, acknowledges each generation once, and reopens after a restart with an empty journal, never replaying it. | `owner_histories_resolve_ambiguity_acknowledge_once_and_never_replay_the_journal` |

Each test function checks its rows' model halves now: TH-01, TH-02, TH-05 and
TH-07 through the transliteration and the authority chain, TH-03, TH-04 and
TH-06 through the transliteration and the pack chain, and TH-08 through the
owner histories. Tasks 11967 and 11968 add the production halves to the same
test functions, so the bindings do not change.

`model_property_manifest_is_unique_and_exhaustive` checks these eight
bindings. It reads the history model with its base configuration, and the
journal owner and compaction models without theirs, so that the citations of
TH-05 and TH-08 resolve while only the history's ten base invariants join the
exhaustive rule; the owner's invariants stay outside it, as the journal-owner
rows' boundary states. It requires every witness that a TH row cites to have
its configuration and to be checked by
`scripts/check-rspace-trusted-history-model.sh`, and the integrated runner to
call that script. TH-01 and TH-03 cite the four new capstones.

**Boundary.**

- These results hold for the models and the test model; this slice changes no
  production code. The production halves are tasks 11967 and 11968, and the
  production negative controls, the timing and the completion of J-253 are
  task 11969.
- Semantic eviction does not exist under keep-all retention; a future one
  needs the maxima recomputed and this family extended.
- Reset identity and activation remain task 7756, and the substrate stays out
  of RSpace until task 7931 wires it in.
- No scaling is claimed here; the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29)
  measures it (thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history,
  with gate G4 failing on allocated bytes). No power-loss claim is made.
- No persisted byte, identity, format, wire message or production outcome
  changes.

Cited works:

- K. Claessen and J. Hughes, "QuickCheck: a lightweight tool for random testing
  of Haskell programs", *Proceedings of the Fifth ACM SIGPLAN International
  Conference on Functional Programming (ICFP '00)*, 268–279, 2000,
  [doi:10.1145/351240.351266](https://doi.org/10.1145/351240.351266), for the
  generated chains.
- L. Lamport, "The temporal logic of actions", *ACM Transactions on Programming
  Languages and Systems* 16(3):872–923, 1994,
  [doi:10.1145/177492.177726](https://doi.org/10.1145/177492.177726), for the
  history model and its instantiation of the publication model.

The faulted-history model evidence in the verification record was taken at
these sources. This slice changes no production source, so the production
sources are those of its parent commit.

| Pinned source | SHA-256 |
|---|---|
| `formal/rocq/stack_safe_pda/theories/TrustedSuccessorCapability.v` | `10362cfb85c4d483aaf521367462690d60bb1d62ad72d8242f85b7e4ba0389da` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistory.tla` | `0d2ffd08fed463e4dac7ab9957eb37f41c3a53869225f1aa751fdae70aab7a8e` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistory.cfg` | `a013eced00f1450ef071534542d19f71d480f555c3bf74b528453eee7bfddefd` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryAdoptPreparedOnIndeterminateMutant.cfg` | `56fbf98fc93030c39e3d445b9fa7a1641e09eba3cd92883721bcfefbf6e1e75b` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryAppendWithoutTruncationMutant.cfg` | `e333606122c3c1657349028d0930f75d0c9cd86470e4dcbf9fc52cf1ce889081` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryMisclassifiedCatalogWriteMutant.cfg` | `db71a3d64ee86a91d17b6815b3234c0219935ad6adc9027211665e806897ba2a` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryPathnameRecoveryMutant.cfg` | `cf9e3522ffa3d314e0d028a363d97a42f8fadf7aba30b2c1f7cbd20c44ac8e97` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryPathnameRereadMutant.cfg` | `e0eb1ba202e5f3f3aea7104ccec7e8448984cd2c34d8aec81ba98ef81672656d` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryStaleCoreRetryMutant.cfg` | `20615e8d801b4d24b48ae014ac555521a4b71c783aaafbda503c9fe1536819f7` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryStaleWriterRestorationMutant.cfg` | `b524ab4650ba7f4a0dd370426dac8c8c41f63da6e9b0bfcf1d9bb62d2111fc16` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistorySuffixReplayMutant.cfg` | `caa05838f0c0a564f8bf03c946bd4f4cc36ee8be1e4266ea32b05acf836def70` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryTruncateBelowAppointedMutant.cfg` | `a3fab5bacf12f5f3448f8e3a8c88fda19c9d84483e5c1007730b67fb5b53bbb7` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryTruncateOnRefusalMutant.cfg` | `b57071d6455dec6a52a4073c203c2d70f0873a871fb6bff0644051897dd41455` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverReaderAcrossFault.cfg` | `cdc2ff5148aafc3936c4ce331c045434a05f8695027d7b2062e259b2e6c978d1` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRecoveredPredecessorAfterAmbiguity.cfg` | `090a88fcd4e41bfb2a81d5f84bc44c44f9edaff87bf366468f73913957f5685b` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRecoveredSuccessorAfterAmbiguity.cfg` | `b4bb1867ea01e5d97b036e3950794036a0a39bb8a34eea6dc3c9a94ee3d58c24` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRecoveredUnderDecoy.cfg` | `23a2eea0b0812dc51e916e0a08d1aacfca867af1cc671fde7c1f2ffb66e583a1` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRefusedWithSuffix.cfg` | `36959684c68ff8810795fff59836428cd74a89e2dffba0e474c67214caaabfa2` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRepairedForeignCatalog.cfg` | `499f397e48c8834a3d3294d0d859973fda7f6a5d48bcc2f4361489810028ccbd` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRestartedMidPublication.cfg` | `63e1a03bddc8fb29fb2db8385c014929538a4fac8a5f31f2e10619dd43694f56` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalHistoryNeverRetriedAfterSuffix.cfg` | `d82d9a183b412ba6407fe0e0db32eb632b08b02126bd8d8f729ebc2a0d57812b` |
| `formal/tlaplus/stack_safe_pda/RSpaceTrustedIncrementalPublication.tla` | `f1b1a9c3f5ad00a056b1d421a2bfbde086208f14adbac73112474765d7c78851` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla` | `fa9a5ae423c4513d4b3474ef2c9c480c1a11542cef1b92179693f52e07fe866c` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerJournalReplayMutant.cfg` | `066d4d739978c045ccce0853fa037bbd7545476a4b039aaf0f0b9ce6c4de0240` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerPathnameRecoveryMutant.cfg` | `0d698ccd5481610418cbb86fdbf40b5408f434ecf609018af5aa1537e632bb77` |
| `formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwnerNeverReconciledUnderDecoy.cfg` | `8b577c829fe29be31cc0bc05567e2d807f0246c1be5a92fff3172f44583c4526` |
| `scripts/check-rspace-trusted-history-model.sh` | `c39e94b593e2ff6ed6088620e64d90e097e76fb832dbb9473732062c7fe3f543` |
| `scripts/check-rspace-journal-owner-model.sh` | `a3bd4547837de39389550f850527c11b96674fa27f48386e747aa280332157ac` |
| `scripts/check-stack-safe-pda-proofs.sh` | `03a816749ce02ffec16217c8a463d713de94a682a0c66642fc3923a356a52ff5` |
| `rspace++/tests/persistence/trusted_history_properties.rs` | `3c28c5bf0f1e6b9ffd15195428d8bf3676908192bb5c57814089b1ce8fdd7e39` |
| `rspace++/tests/persistence/trusted_successor_properties.rs` | `d7647265b2bd05d10fd1d7c58e6af0dcd88d9a7c95002f3d5cfc0648c039f2b6` |
| `rspace++/tests/persistence/property_support.rs` | `d34553b7b9dc6032b9691629ae7e8bd3092afeb66e0c65ec467a53479d25fe90` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `b9afcc8d401861152c8f0d599242f0f4b1a4dc047e1116bbf08aedf8b4f21a55` |
| `rspace++/tests/persistence/mod.rs` | `9843c8f00ea540f9c2b3a37bba38f74b63ed27b9519dc00aa50923f685015a01` |

### Faulted trusted histories on real files (2026-10-28)

Task 11967, the second slice of task 7928, adds the production halves of
TH-01 to TH-07. It runs histories of trusted publications on real files,
crosses their later steps with every fault that the trusted path can meet,
kills a child process at each of the thirteen trusted hooks, and judges every
outcome by cold admission of a byte copy of the store. It changes no
production source.

**Production refinement.** The production halves are in
`rspace++/tests/persistence/trusted_history_production.rs`, which is compiled
on unix only. Each TH test function runs its model checks first and then calls
one driver, so the bindings of the TH rows do not change.

- `faulted_trusted_histories_match_independent_cold_admission_at_every_step`
  (TH-01, TH-02, TH-05 and TH-07) calls
  `production_faulted_trusted_histories(16)`. For each retention writer, V1
  and V2, over the structural fixture, it runs three seeded histories of
  eight, nine and six steps, then sixteen generated histories of one to nine
  steps. Each history runs on a fresh scoped thread, so a failed case cannot
  leave a failpoint or a hook armed for the next one.
- `every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps`
  (TH-03, TH-04 and TH-06) calls `production_every_failpoint_at_later_steps()`.
  It runs a scripted sweep of 96 items in each of four scenarios, the two
  writers crossed with the structural and the semantic fixture, one scenario
  per thread, and then the thirteen kill children of
  `trusted_history_kill_child`.

**One step.** A trial holds two chains in separate directories. The live chain
takes each step's interleave and fault; the reference chain publishes the same
transitions with neither. An in-memory canonical object store, the oracle,
holds every model state that the trial installs, so a walk from a manifest
gives the closure that the store must serve, byte for byte. A step draws a
transition (a jump to a slot configuration, a path edit, or a revert to an
earlier adopted state), a fault and an interleave, and runs in seven stages.

1. *Normalize.* The drawn fault and interleave are reduced to what the settled
   store admits. A corrupt sidecar needs a pending reclamation intent, a
   foreign catalog needs none, an unpin needs a reader, and a refusal needs a
   non-empty transition. The seeded histories and the sweep run strictly, so
   any normalization there fails the test: each of their steps runs as
   written.
2. *Interleave.* At the settled point, before the publication, one of:
   refusals; a reader pin or unpin; a lease counter at its maximum; a fresh
   generation checkpoint; a crash in the middle of reclamation; an idle
   foreign catalog; a restart; a bind of a past version; or a decoy at the
   store's pathname.
3. *Publish.* The live writer binds and prepares the transition, the fault is
   armed, and `publish_trusted_incremental_v1` runs. Its hook trace must be
   the publication order up to the fault, with no cold admission and no index
   construction, and the error determines the outcome class.
4. *Judge.* The store is copied byte for byte into a fresh directory, and the
   copy is admitted cold with its own writer lease and lease registry. It must
   select the slot that the outcome class requires (figure below), and the
   live core that settles the fault must equal the copy's, observation for
   observation. The comparison uses observations, never the copy's bytes after
   admission, because the copy's fresh registry may reclaim what the live
   writer's reader still defers.
5. *Stale retry.* After an ambiguous or published-error outcome, the same
   prepared publication is retried from the stale core and must be refused.
6. *Settle.* The fault's own settlement: a retry over a pack suffix,
   held-lease recovery, a restart and cold admission by path in place of
   recovery, or a repaired catalog.
7. *Close.* The catalog file holds the core's image; the manifest is the
   oracle's installation of the model state; the current closure and a pinned
   reader's closure read the oracle's bytes; the retention history is the
   previous one plus the previous current state; and once the successor is
   adopted, the live and reference stores hold the same files and equal
   cores.

![One step of a faulted trusted history on real files, as an activity diagram. In grey, the settled point, where the live catalog file holds the core's image, the index length is its appointed tail and the live and reference stores hold the same files and cores with equal observations, and the normalization of the drawn fault and interleave to what the settled store admits. In purple, the interleave at the settled point: refusals that change no file, a reader pin or unpin, a lease counter at its maximum, a fresh generation checkpoint, a crash in the middle of reclamation, an idle foreign catalog, a restart, a bind of a past version, or a decoy bound to the store's pathname. In blue, the transition that the reference chain publishes without a fault, and the byte copy of the store admitted cold with its own writer lease and lease registry, whose current closure must read the oracle's bytes and which refuses a corrupt sidecar or a foreign catalog. In amber, the armed fault and publish_trusted_incremental_v1 on the live chain, whose hook trace is the publication order up to the fault with no cold admission and no index construction. The outcome class decides the slot that the copy must select: the successor after a success, in green; the predecessor after a definite non-publication, with no new object readable; the slot that the persisted image selects after an indeterminate write, the old slot or the exact terminal; and the exact retained terminal slot after a published error, in red. In red too, the stale retry from the stale core after an ambiguous or published-error outcome, refused at the catalog compare or at the index length check, and the settling: a retry over a pack suffix, held-lease recovery, a restart with cold admission by path, or a repaired catalog. In green, the comparisons that close the step: the settled core against the copy's, the catalog file, the manifest against the oracle, the closures, the pinned reader's prefixes and the keep-all retention history, and finally the live store against the reference store](figures/trusted-history-step.svg)

**What each row requires.** The "What it requires" column states the
production checks that the row's test makes, in addition to its model half.

| Row | Production check | What it requires |
|---|---|---|
| TH-01 | `production_faulted_trusted_histories`, three seeded and sixteen generated histories per writer | After every step, the copy admitted cold selects the predecessor after a definite non-publication (hooks 2 to 10, a physical write or sync failure, a short pack, a catalog changed at the compare), the slot that the persisted image selects after an indeterminate write (hook 11, a slot write or slot sync failure, a torn slot prefix), and the exact retained terminal slot after a published error (hooks 12 to 14, a directory sync failure). The copy of a store with a corrupt sidecar or a foreign catalog is refused, a copy taken under the forged re-read follows the rolled-back image, and in every other case the settled core equals the copy's. After a definite non-publication, and after an indeterminate write that kept the old slot, no new object reads through the settled core or the copy. All four outcome classes occur for each writer. |
| TH-02 | `production_faulted_trusted_histories` | The copy lives in a fresh directory with its own writer lease, and its registry is not the live writer's. Every adopted step's core equals the trusted observation of the bound transaction; the catalog file equals the core's image; the manifest equals the oracle's installation of the model state; the current closure reads the oracle's bytes through the core and through the copy; the retention history grows by exactly the previous current state, and the retained closure exceeds the current one. At every restart and at the end, the ledger's retained domain equals the union of the adopted current closures, and its current domain equals the current walk. |
| TH-03 | `production_every_failpoint_at_later_steps`, four scenario sweeps | From step 2 on, each of hooks 2 to 14, the five I/O failpoints, the fourteen torn slot prefixes at both slot parities, a short pack, a catalog changed at the compare and a failed sync of the truncation is injected. Right after each of these faults, the appointed pack prefix of the persisted image equals the predecessor's or the reference chain's. A retry over a pack suffix truncates it. A retry over an index suffix, after each of hooks 4 to 10, is refused at the index length check (`publication.rs:1254`) until held-lease recovery rebuilds the index, and that recovery survives each of its six rebuild failpoints. A commit abandoned by an indeterminate write that kept the old slot is truncated by the next publication, and its new objects never become readable. After settling, the live store holds the reference store's files. |
| TH-04 | `production_every_failpoint_at_later_steps` | At a clean point, a bind under each lowered axis of the measured policy evidence, a publication under a changed policy, a publication of a cold-bound transaction and a publication after a changed inactive slot are refused before any write. Over a pack suffix one of each kind is refused, and over a pack and an index suffix all but the cold-bound transaction, whose cold acquisition would rebuild the index. The store's files, suffixes included, are byte-identical before and after each refusal. |
| TH-05 | `production_faulted_trusted_histories` | A pinned reader holds a generation lease in the registry that the writer shares. After every step, its pinned pack and index prefixes are unchanged when read by name, and each object of its closure lies, through the pinned index prefix, at a pack location that holds the oracle's bytes. This holds across faults, recoveries, fresh generation checkpoints and a restart that keeps the reader. A fresh checkpoint under the reader leaves the superseded generation's files and the reclamation intent in place, and after an unpin the next publication completes the reclamation. A crash in the middle of reclamation, with or without the old index and with or without a restart, completes and retires the old key. A lease counter at its maximum refuses a pin and changes no file and no active lease. |
| TH-06 | `production_every_failpoint_at_later_steps` and `trusted_history_kill_child` | After every ambiguous or published-error outcome, a publication from the stale core is refused: at the catalog compare, with an empty hook trace and no file changed, when the catalog image changed; and at the index length check, after truncating the abandoned commit, when an indeterminate write kept the old image. A restart in place of recovery, after a definite non-publication and after indeterminate writes that kept the old or the new slot, adopts the catalog's selection. Each of the thirteen trusted hooks is killed in a child process at step 3, after step 2 met a fault of one of the four outcome classes and settled. Cold admission then yields the step-2 state for hooks 2 to 10, where a retry publishes step 3 with the trusted effects, and the step-3 state for hooks 11 to 14; either way the store ends with the reference store's files. |
| TH-07 | `production_faulted_trusted_histories` | A decoy, a copy of the store's initial files bound to its pathname while the live directory sits under another name, is placed before the base check, by hook 14 before the re-read, before held-lease recovery, and as a forged re-read. The publication, the re-read and the recovery go through the pinned directory descriptor, so the decoy is never adopted and its files are unchanged when it is removed. A foreign catalog image, a valid catalog of the same store that names the other fixture's checkpoint, is refused by the re-read after the swap, by held-lease recovery, by the base check of an idle publication and by cold admission after a restart, all with the store's files unchanged, until the saved image is written back. |

**Reachability.** Four coverage items of the reviewed plan for task 7928
(pgmcp artifact 1140, section 4.3) cannot be met as written, and the slice
replaces them.

1. *Sharing.* "References exceed distinct objects" holds before any step:
   references are counted per root, child edge and content edge, and the empty
   fixture state points all three roots at one empty node. The slice counts
   instead a node with at least two distinct parent nodes in the oracle's walk,
   and a blob with at least two distinct parent leaves.
2. *Duplicate child edges.* A valid node cannot hold two edges with the same
   byte: non-strict edges are refused at encoding and at parsing, and a
   256-way node stores one bit per edge. The slice instead counts a node that
   reaches one object through two edges, and requires the count to be zero:
   a channel child begins with its own identity, so sibling edges never reach
   one object for the fixture's alphabet.
3. *Equal epochs.* The selection branch for two slots with one epoch is
   unreachable for the writer, because
   `CatalogAuthorityTokenV1::successor_image` requires the next epoch
   (`durable/catalog.rs:507-521`).
4. *Lease-counter overflow.* It is reachable in this family only through a
   reader's pin, through the counter seam at `retention.rs:601-603`. The
   owner's own successor pin belongs to TH-08.

Append errors are matched only by their variant, `Append(_)`:
`IncrementalAppendErrorV1` is declared in the private module `publication`
and is not re-exported (`durable/mod.rs:25`, `:81-94`). The length detail of
a retry over an index suffix is therefore checked in the error's debug text,
as `expected: Exact(old_tail)` beside the index's length.

**Coverage and kills.** The sweep alone must meet every coverage counter
(`assert_complete`). Each writer's three seeded histories alone, and then its
seeded and generated histories together, must meet the authority subset
(`assert_authority`): the four outcome classes, a reader across a fault and
across a restart, a fresh checkpoint, deferred and completed reclamation, both
crashes in the middle of reclamation, a pin overflow, the four decoys, both
foreign catalogs, an idle foreign catalog, an idle restart, both past binds, a
revert, a corrupt sidecar, a shared blob and a retained closure above the
current one. The table gives the counts. The sweep is scripted, so its counts
are fixed. The writer counts are those of the seeded and generated histories
together under `PROPTEST_RNG_SEED=20261027`, which drew the same histories for
V1 and V2; the two writers differ only in which check refused the lowered
axes. A zero in the writer column is either a counter outside the authority
subset, which the sweep meets, or the count of nodes that reach one object
through two edges, which must be zero. An adopted step is a step whose
successor became the live core.

| Counter | Entries, in order | Four sweeps | Each writer |
|---|---|---|---|
| Fault at a trusted hook | hooks 2 to 14 | 4, 4, 4, 12, 8, 8, 4, 4, 4, 12, 4, 4, 4 | 1, 1, 2, 1, 1, 1, 2, 1, 0, 1, 3, 4, 1 |
| I/O failpoint | physical write, physical sync, catalog slot write, catalog sync, catalog directory sync | 4, 4, 8, 4, 4 | 1, 1, 2, 2, 1 |
| Failed sync of the truncation | one entry | 4 | 0 |
| Torn slot prefix at a fixed length | the lengths 0, 1, 2, 7, 8, 63, 64, 65, 127, 128, 255, 256, 334 and 335, each written into either catalog slot as the inactive one | 4 at each of the 28 | 1, at length 335 written into slot 0 |
| Effect of a torn slot write | image unchanged; image changed with the old slot selected; new slot selected | 40, 56, 16 | 1, 8, 1 |
| Short pack | one entry | 4 | 4 |
| Corrupt sidecar | one entry | 4 | 1 |
| Foreign catalog met by a publication | repaired under the held lease; repaired after a restart | 4, 4 | 3, 5 |
| Idle foreign catalog | one entry | 4 | 7 |
| Catalog changed at the compare | one entry | 4 | 6 |
| Retry over a pack suffix without recovery | after hooks 4 to 10 | 4 each | 0, 0, 0, 2, 0, 0, 0 |
| Index rebuild failpoint | candidate write, candidate length, candidate sync, before the replace, after the replace and before the directory sync, directory sync | 4 each | 2, 0, 0, 2, 0, 1 |
| Outcome class | definitely not published; indeterminate with the old slot; indeterminate with the new slot; published with an error | 132, 104, 32, 32 | 29, 11, 4, 19 |
| Stale retry refused | at the compare after a new slot or a published error; at the compare after a changed image that kept the old slot; at the index length check after an unchanged image | 60, 56, 48 | 22, 8, 3 |
| Abandoned commit truncated by the next publication | one entry | 4 | 0 |
| Refusal over a pack suffix | lowered axis, changed policy, cold-bound transaction, changed inactive slot | 4 each | 0 each |
| Refusal over a pack and an index suffix | lowered axis, changed policy, changed inactive slot | 4 each | 0 each |
| Refusal at a clean point | lowered axis, changed policy, cold-bound transaction, changed inactive slot | 102, 4, 4, 4 | 3, 3, 1, 3 |
| Lowered axis refused by | the ledger trial on an axis other than the final totals; the ledger trial on the final object or byte total; a ceiling | 62, 12, 36 | V1: 2, 0, 1; V2: 3, 0, 0 |
| Pinned reader checked after a faulted step | one entry | 12 | 13 |
| Pinned reader checked after a restart | one entry | 4 | 1 |
| Fresh generation checkpoint | one entry | 12 | 7 |
| Reclamation deferred under a reader | one entry | 12 | 4 |
| Deferred reclamation completed | one entry | 12 | 4 |
| Crash in the middle of reclamation | without a restart; with a restart | 4, 4 | 1, 1 |
| Pin refused at the lease counter's maximum | one entry | 4 | 6 |
| Decoy | before the base check, before the re-read, during recovery, forged re-read | 4 each | 2, 1, 1, 1 |
| Idle restart | one entry | 8 | 26 |
| Restart in place of recovery | after a definite non-publication; after an indeterminate write that kept the old slot; after one that selected the new slot, or after a published error | 8, 4, 4 | 0, 0, 2 |
| Bind of a past version | after a clean step; after a faulted step | 4, 8 | 2, 2 |
| Adopted step whose state equals an earlier adopted state | one entry | 336 | 9 |
| Adopted step with a node of two distinct parent nodes | one entry | 292 | 91 |
| Adopted step with a blob of two distinct parent leaves | one entry | 92 | 32 |
| Slot replaced by an adopted step | one entry | 576 | 324 |
| Slot removed by an adopted step | one entry | 1492 | 291 |
| Adopted step whose retained closure exceeds its current one | one entry | 384 | 105 |
| Node that reaches one object through two edges | must be zero | 0 | 0 |
| Kill child | hooks 2 to 14 | 1 each | 0 each |

The thirteen kill hooks are not counted by hand: they are
the hook trace of the sweep's first clean step, which must equal the
publication order from hook 2 to hook 14 in all four scenarios. Hook `h` goes
to scenario `h mod 4`, whose step 2 meets a failure at hook 6, hook 11 or
hook 13, or a torn slot prefix of 200 bytes, so every kill follows an earlier
fault and its recovery. Each child aborts at its hook and must end by
`SIGABRT` after writing its marker; a core dump is suppressed.

**Boundary.**

- No production source changes, and no persisted byte, identity, format, wire
  message or production outcome; the consensus-change register gets no note.
- A torn or zero-filled reclamation intent stops held-lease recovery, cold
  admission and fresh checkpoints permanently. The reviewed design found it
  by reading the code, and it is filed as bug 11970, to be fixed model-first
  before task 7928 closes. No step here faults a fresh generation checkpoint,
  so no test exercises it and none is filtered. The fix is described in
  [Torn reclamation intents](#torn-reclamation-intents-bug-11970-2026-10-28).
- A catalog rolled back to an older valid image of the same store is adopted
  by recovery, and the next publication truncates the overwritten successor's
  bytes as a pack suffix (`publication.rs:996-1008`), so those commits are
  lost. The forged re-read shows it: the copy taken under it selects the
  predecessor. Production cannot detect a rollback without external monotonic
  state, and the history model's environment writes only a foreign image
  (`RSpaceTrustedIncrementalPublication.tla:251-256`).
- `publish_trusted_incremental_v1` resolves the reclamation intent against
  the in-memory successor (`publication.rs:956`) before the re-read
  (`:957-968`) confirms the catalog, so a rollback inside that window would
  delete files that the restored catalog names. Recovery resolves against the
  verified selection (`startup.rs:299`). The tests stay out of the window: a
  replaced catalog and the forged re-read require that no intent is pending.
- No power-loss claim is made: a killed child leaves its writes with the
  operating system, and nothing here models a lost page cache.
- A reader kept across a restart, decoys placed in the middle of a
  publication, and the forged re-read go beyond the history model. Its
  restart drops every reader
  (`RSpaceTrustedIncrementalHistory.tla:216`), it places a decoy only while
  the writer is idle or recovering (`:240-244`), and its environment never
  writes an older image of the store.
- The production halves compile on unix only; elsewhere the two TH functions
  run their model halves alone.

The production evidence in J-253 was taken at these sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/publication.rs` | `801348d8e8a547db88596136103bf45dab03d645208c2e0658445b5e49c30de6` |
| `rspace++/src/rspace/persistence/durable/io.rs` | `7811573c32e8cf8942eed3530c52b3f7a620f664880e517204ac258ffda05cf7` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `3a1da653d1206d0dcc9563c0cd14b6197cd49081cc605d78aed3c6cc4db863d6` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `eef3657433921ca66c94d3e24c028d903f7599de18d9c4510e797b419c9b1671` |
| `rspace++/src/rspace/persistence/durable/reclaim.rs` | `9b1c05dce7fcc5280db2cd255f99c6fc286ec94144779bffc167a9ca6635485e` |
| `rspace++/src/rspace/persistence/durable/catalog.rs` | `bec65f927d812bb73198f0d1267de344cbc831d5024992e44e31582b63d81eaf` |
| `rspace++/src/rspace/persistence/retention.rs` | `e5971ed87b6973bdbc09fd02609507c2a8dc8af4459f8bb973df59e2782529de` |
| `rspace++/tests/persistence/trusted_history_production.rs` | `c13278b6edc9249c250df8295748e4b8b96cb0752fb1afbf069ae95684c68a9a` |
| `rspace++/tests/persistence/trusted_history_properties.rs` | `a532881c9d6a087ef8e87940b1b28c05880382c12b3ae016e83c23d4b7e81b26` |
| `rspace++/tests/persistence/trusted_successor_production.rs` | `57557ac33cd057613cde2d683290d78b7fd95bc1226662ef55f9fb7ae58ca9c2` |
| `rspace++/tests/persistence/path_exact_closure_properties.rs` | `62c6d34d6cf5f9ade05273d739d4d58878bdb0a4e1b886ce4c697c9291fc5a4f` |
| `rspace++/tests/persistence/mod.rs` | `7f97eb6dd116d9c1d3c3efd327d6e0e5ca14a1ccd7b5b0b8c123cfc0cd45fd6a` |
| `docs/design/pathmap/figures/trusted-history-step.puml` | `327b7f7aeb731261802aa1fe1ed1e163cd8dc7bc57d1d3c6e7f0a23e0b04a10e` |

### Journal-owner histories on real files (2026-10-28)

Task 11968, the third slice of task 7928, adds the production half of TH-08.
It drives the real journal owner, `DurableRSpaceAuthorityV1`, over real files
through histories of hot-store edits, checkpoints, faults, reconciliations,
acknowledgement retries, cancellations and restarts. After every step it
judges the store by cold admission of a byte copy and by a hot store
bootstrapped from that copy. It changes no production source.

**Production refinement.** The production half is in
`rspace++/tests/persistence/owner_history_production.rs`, which is compiled
on unix only. The TH-08 test function runs its model half and then calls
`production_owner_histories(32)`, so the row's binding does not change. The
driver works in three parts:

1. It runs four seeded histories of 18, 14, 16 and 11 steps, 59 in all, and
   requires the coverage counters below from them alone.
2. It runs 32 generated histories of one to eight steps, each step with zero
   to three edits, adds their counts to the seeds' and requires the counters
   again.
3. It spawns one child process, through the existing
   `authority_unwind_child` dispatcher, that unwinds the owner past its
   acknowledgement.

Each history runs on a fresh scoped thread, so a failed case cannot leave a
failpoint or a hook armed for the next one. Only the V2 retention writer is
used: production opens every store with it (`durable/io.rs:835-841`), the
owner's publication has no writer branch, and task 11967 already runs both
writers over the same trusted publication.

**The model.** A history keeps a model of the owner,

```math
M = (H, D, d, \varphi, a, s, t),
```

whose components are:

- $`H`$, the contents that the owner's hot store must hold;
- $`D`$, the contents of the durable state that the catalog selects;
- $`d`$, the number of publications committed since the trial opened;
- $`\varphi`$, the owner's phase;
- $`a`$, the catalog slot whose state the owner has adopted;
- $`s`$, the catalog slot that the live catalog selects;
- $`t`$, the terminal that an open publication retained, if any.

The *open phases* are Captured, RecoveryRequired and AwaitingAck: exactly
the phases in which the owner holds a pending publication, and with it the
operation gate's write permit. The contents range over a small fixed
universe: two data channels with three values each, kept as a bag so that
multiplicities count; two continuation groups, of one and of two channels,
with three bodies each; and the three joins of those groups. A step is
*changed* when $`H \neq D`$, and an owner in RecoveryRequired is *kept* when
$`s = a`$, that is, when its publication's outcome left the predecessor
selected. With $`g_0`$ the state that the trial's first open admits and
$`\mathrm{gen}`$ the generation of a slot, every step must leave

```math
\mathrm{gen}(s) = \mathrm{gen}(g_0) + d,\qquad
\varphi = \mathrm{Ready} \;\Rightarrow\; a = s,\qquad
\varphi \neq \mathrm{Ready} \;\Leftrightarrow\; \text{the operation gate is held}.
```

**One step.** A step runs in six stages.

1. *Observe.* At the settled point the trial records the catalog image and
   its selection, the predecessor's appointed pack prefix, whether the active
   pack holds a suffix beyond its appointed tail, and $`d`$.
2. *Edit.* Each edit runs only while the gate's read side is free, as an
   RSpace operation would: a datum put or removal, a continuation put with
   the joins of its group, a continuation removal, or a join put or removal.
   In an open phase the owner holds the gate, so the edit is blocked and
   counted. A join removal while its group holds a continuation is refused
   by the hot store, as intended, and counted.
3. *Normalize.* The drawn action becomes one that the phase admits. A fault,
   a successor pin overflow or a cancellation needs Ready, and the first two
   also a changed step. An explicit or zero-limit reconcile needs an open
   phase. A minimal checkpoint needs an unchanged Ready step or a pending
   acknowledgement. A reconcile under a decoy needs RecoveryRequired with
   $`\mathrm{gen}(a) > \mathrm{gen}(g_0)`$ or $`s \neq a`$, so that recovery
   by pathname, which would find $`g_0`$'s slot in the decoy, could not pass
   for correct recovery. A seeded history runs strictly: any normalization of
   its edits or actions fails the test.
4. *Call.* The owner call runs under `observed_effects`. A fault is armed
   inside the owner's one-shot `AfterPreparation` hook, so it binds to the
   publication that the owner actually runs, and the hook must have fired.
   The call's hook trace, cold admissions and index constructions must equal
   the action's expected effects (the table below).
5. *Settle.* The outcome is checked as the table below states, and then the
   settled point: the phase; the gate, held exactly in the open phases; the
   pins, with the adopted generation pinned and a superseded generation or an
   unacknowledged terminal unpinned; the selected slot; the predecessor's
   appointed prefix; and at most one new generation. In Ready the hot store,
   read under the gate's read side with its five journals compared before and
   after the reads, must hold $`H`$.
6. *Judge.* The store is copied byte for byte into a fresh directory, and the
   copy is admitted cold with its own writer lease and lease registry. The
   copy must select $`s`$, have generation $`\mathrm{gen}(g_0) + d`$ and
   enumerate exactly $`D`$ through its authenticated data, continuation and
   join enumerators. In Ready the owner's state and pin key must equal the
   copy's; in an open phase the owner must report its phase as unresolved. A
   hot store bootstrapped from the copy must have five empty journals and
   hold $`D`$. The judge's admission and bootstrap run outside every
   `observed_effects` window, so they never count toward a step's effects.

![One step of a journal-owner history on real files, as an activity diagram. In grey, the settled point: the owner's phase, the operation gate, the generation pins, the slot that the catalog selects, the predecessor's appointed pack prefix, any pack suffix, the files and the five journals. In purple, the step's edits to the hot store under the gate's read side, blocked while an open phase holds the gate. In amber, the action normalized to what the phase admits, which a seeded history must keep as written, and the fault armed inside the owner's AfterPreparation hook: a failpoint at one of the hooks 2 to 14, an I/O failpoint or a torn slot prefix. In blue, the owner call under observed_effects: a checkpoint, a minimal checkpoint, a checkpoint cancelled at the gate, a reconcile, also under zero admission limits or under a decoy, or a restart by path, whose hook trace, cold admissions and index constructions must match the action. A switch on the phase after the call: Ready in green, with the journals empty or unchanged, the gate free and only the adopted generation pinned; RecoveryRequired in red, after a faulted checkpoint whose outcome class keeps the predecessor or selects the exact terminal, or after a refused zero-limit reconcile, with the gate held and the journals unchanged; AwaitingAck in red, after the successor pin overflowed, with the terminal retained and unpinned. In green, the settled checks and the judge: a byte copy admitted cold with its own lease registry, whose slot, generation and contents equal the model's durable state, and a hot store bootstrapped from it with empty journals](figures/owner-history-step.svg)

**Outcomes and effects per action.** The effects are counted per call: the
hook trace of the trusted publication, the cold admissions and the index
constructions. "Trusted order" is the trace of hooks 2 to 14, and "one
recovery" is one cold admission with one index construction and no trace.

| Phase and step before the call | Action | Result | Effects | Model |
|---|---|---|---|---|
| Ready, changed | checkpoint | the next generation is published and acknowledged, and the five journals are empty | trusted order | commit, then adopt the new slot |
| Ready, unchanged | checkpoint, or checkpoint under minimal limits | the adopted state, with no file and no journal changed | none | unchanged |
| Ready, changed | faulted checkpoint | a publication error whose outcome class keeps the predecessor or selects the exact retained terminal; RecoveryRequired with the journals unchanged | the trusted order up to the fault | commit the terminal when the class selects it |
| Ready, changed | successor pin overflow | the publication commits, then the exhausted lease counter refuses the successor's pin; AwaitingAck with the terminal retained and the journals unchanged | trusted order | commit the terminal |
| Ready | cancellation at the gate | while a reader holds the gate, the checkpoint waits before capturing and is dropped; Ready, with no file and no journal changed and the gate free afterwards | none | unchanged |
| RecoveryRequired | reconcile, also under a decoy at the store's pathname | the predecessor with the journals unchanged when kept, or the terminal with the journals empty; the decoy's files are unchanged | one recovery | adopt the selected slot |
| RecoveryRequired | checkpoint | when kept, recovery and then a publication of the journal; otherwise recovery adopts the terminal | one recovery, with the trusted order when kept | adopt, and when kept commit and adopt the new slot |
| RecoveryRequired | reconcile under zero admission limits | refused at admission after the physical recovery; RecoveryRequired, with the terminal, the gate and the journals unchanged and the files changed at most by the index rebuild | one recovery | unchanged |
| AwaitingAck | checkpoint, checkpoint under minimal limits, reconcile, or reconcile under zero admission limits | the terminal's state, acknowledged once; the journals are empty and the files unchanged | none | adopt the terminal |
| any | restart by path | the owner and its hot store are dropped, which frees the gate, and reopened: Ready at the selected slot, with five empty journals and the hot store holding $`D`$ | one recovery | $`H := D`$, adopt the selected slot |

Every recovery, the refused one included, rebuilds the selected slot's index
file to its appointed tail when the file is longer, and changes no other
file; only a publication truncates a pack suffix.

**The unwind past the acknowledgement.** The child process publishes once,
faults its second publication at hook 13, a published error that retains the
terminal, and then reconciles. A one-shot owner hook at
`AfterAcknowledgement` checks that the gate is held, that the five journals
are empty and that both the predecessor's and the terminal's pins are
active, and then panics. While the stack unwinds, a drop guard checks that
the owner is still in AwaitingAck with the gate held, the journals empty and
both pins active, and retries the checkpoint. The retry returns the
terminal's state with no hook, no cold admission and no index construction,
leaves the files unchanged, releases the predecessor's pin and the gate, and
the next checkpoint publishes exactly one generation, which the judge
confirms. The journal is therefore acknowledged once:

1. the first call committed the acknowledgement before the hook ran;
2. nothing changed the hot store between the panic and the retry;
3. with an empty journal, the retry's acknowledgement can only find the
   committed marker, which changes nothing.

In `RSpaceJournalPublicationOwner.tla` this panic is a stutter between
`Acknowledge` and `FinishAcknowledgement`, not the model's `Unwind`, which
moves to RecoveryRequired. Production thus refines a subset of the model's
behaviours (an inference from the code: the owner has no drop guard that
resets its phase).

**What the row requires.** The "What it requires" column states the
production checks that TH-08's test makes, in addition to its model half.

| Row | Production check | What it requires |
|---|---|---|
| TH-08 | `production_owner_histories`, four seeded and thirty-two generated histories, and `run_owner_history_unwind_child` through `authority_unwind_child` | `ReopenDiscardsVolatileJournal`: after every restart by path, from every phase that a history reaches, the reopened owner's five journals are empty and its hot store holds the durable contents, and after every step a hot store bootstrapped from a cold copy does the same. `RecoveryAcceptsOnlyExactTerminal`: every reconcile, explicit, implicit (the one that a checkpoint retry runs before it checkpoints) or under a decoy bound to the store's pathname, adopts exactly the predecessor after an outcome class that keeps it and exactly the retained terminal after one that selects it; the decoy is never adopted or written; a reconcile under zero admission limits is refused and keeps RecoveryRequired, the terminal, the gate and the journals. `AtMostOneAcknowledgement`: each of the four acknowledgement retries after a successor pin overflow acknowledges with no hook, no cold admission and no index construction, and after an unwind past the acknowledgement the retry acknowledges through the committed marker and the next checkpoint publishes one generation. `GateHeldUntilResolution`: the operation gate is held exactly in the open phases, edits are blocked there, and a checkpoint cancelled while a reader holds the gate captures and publishes nothing and leaves the gate free. |

**Reachability.** Some behaviours of the owner model cannot be reached, or
mean nothing, in production histories, and the slice replaces them.

1. *Cancellation in an open phase.* The model's `CancelCall` cancels a call
   in progress. The owner's only await, for the gate's write permit
   (`rspace_authority.rs:213`), comes before the capture (`:216`), while
   `reconcile` and the acknowledgement are synchronous, so a call can be
   cancelled only at the gate. The histories cancel there; the unwind child
   covers a panic past the acknowledgement; and the existing boundary
   children cover panics at capture, preparation, both pins and the
   acknowledgement. In an open phase the test cannot hold the gate's read
   side, so a cancellation there normalizes to a checkpoint.
2. *A torn slot write that selects the new slot.* Whether a torn prefix
   leaves a valid new slot depends on the bytes it overlays. The class that
   selects the new slot is met deterministically by hook 11 and by a failed
   catalog sync, and the torn write's own class is counted with the outcome
   classes.
3. *Captured and Invalidated after a step.* Captured exists only inside a
   checkpoint call, and Invalidated needs a foreign rebinding of the hot
   store. The existing capture, preparation and acknowledgement unwinds and
   the invalidated-base regressions (JO-08) cover them.
4. *A restart in the middle of a publication.* The publication runs within
   one poll, so an in-process history cannot restart inside it. Task 11967's
   kill children (TH-06) cover it.
5. *Vacuous actions.* A reconcile under a decoy in AwaitingAck returns
   through the acknowledgement shortcut before any recovery, and a
   zero-limit reconcile in Ready returns the state; both normalize.
6. *A publication over an index suffix.* Every route out of
   RecoveryRequired passes through a recovery that rebuilds the index first,
   so the owner never publishes over an index suffix. The refusal at
   `publication.rs:1254` stays covered by task 11967.

**Coverage.** The four seeds alone, and then the seeds together with the
generated histories, must meet every counter except the informational
normalization counts (`assert_authority`). The seeds run strictly, so their
counts are fixed; the generated counts are those of
`PROPTEST_RNG_SEED=20261027`. The torn slot write of the seeds kept the old
slot, one of the two outcomes that the design predicted for it.

| Counter | Entries, in order | Seeds | Seeds and generated |
|---|---|---|---|
| Faulted checkpoint at a trusted hook | hooks 2 to 14 | 1 each | 1, 1, 1, 4, 4, 1, 1, 1, 1, 1, 1, 1, 2 |
| Faulted checkpoint at an I/O failpoint | physical write, physical sync, catalog slot write, catalog sync, catalog directory sync | 1, 2, 1, 1, 1 | 2, 4, 1, 2, 2 |
| Site of a failed physical sync | the sync after the prepare write; the sync after truncating a pack suffix | 1, 1 | 3, 1 |
| Torn slot write | one entry | 1 | 6 |
| Outcome class of a faulted checkpoint | definitely not published; indeterminate with the old slot; indeterminate with the new slot; published with an error | 12, 2, 2, 4 | 21, 7, 3, 6 |
| Recovery path | explicit, to the predecessor; explicit, to the terminal; implicit, to the predecessor and a republication; implicit, to the terminal | 6, 2, 6, 2 | 10, 3, 13, 3 |
| Reconcile under a decoy | to the predecessor; to the terminal | 1, 1 | 1, 1 |
| Reconcile refused under zero admission limits | one entry | 1 | 2 |
| Successor pin overflow | one entry | 5 | 8 |
| Acknowledgement retry | checkpoint, minimal checkpoint, reconcile, zero-limit reconcile | 1, 1, 1, 1 | 3, 1, 1, 1 |
| Checkpoint cancelled at the gate | one entry | 1 | 9 |
| Empty checkpoint | normal limits; minimal limits | 1, 1 | 37, 8 |
| Restart by path | Ready, unchanged; Ready, with a discarded journal; RecoveryRequired, kept; RecoveryRequired, with the terminal selected; AwaitingAck | 1 each | 4, 4, 1, 2, 1 |
| Edit blocked by the held gate | one entry | 3 | 25 |
| Durable commit that changed a namespace | data, continuations, joins | 12, 5, 6 | 47, 25, 31 |
| Durable commit that removed an entry | data, continuations, joins | 2, 2, 2 | 5, 2, 2 |
| Join removal refused while its group holds a continuation | one entry | 1 | 5 |
| Durable commit that leaves a datum with multiplicity at least two | one entry | 3 | 8 |
| Normalized, informational | edits, actions | 0, 0 | 77, 54 |

The faulted checkpoints of the seeds and generated histories together number
37 by trigger (20 at hooks, 11 at I/O failpoints, 6 torn) and 37 by outcome
class, as they must.

**Boundary.**

- Each history runs one process, one owner and one thread. RSpace operations
  are emulated through the gate's read side; there is no concurrent producer
  or consumer.
- Faults are injected in-process, and the operating system keeps every
  write; no power-loss claim is made.
- The production half compiles on unix only; elsewhere the TH-08 function
  runs its model half alone.
- Only the V2 retention writer is used.
- In an open phase the judge reads the durable copy only. The owner's hot
  store is read in Ready alone, where an RSpace operation could read it too.
- A restart drops every handle of the owner first. The directory registry
  can hand a surviving clone the state of the old registry for the same
  directory, so the test keeps no clone across a restart (an inference from
  `retention.rs:575-598`).

The production evidence in J-253 was taken at these sources.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/rspace_authority.rs` | `9c5acd88d7ac44719ddce6d7f6cd846a1c592bde831417c630767f8237921e70` |
| `rspace++/src/rspace/hot_store.rs` | `4b0ea9cdde34385b4f13f62cd9872fcc8ce4319a581428c9009fb759f2986460` |
| `rspace++/src/rspace/persistence/durable/publication.rs` | `801348d8e8a547db88596136103bf45dab03d645208c2e0658445b5e49c30de6` |
| `rspace++/src/rspace/persistence/durable/io.rs` | `7811573c32e8cf8942eed3530c52b3f7a620f664880e517204ac258ffda05cf7` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `3a1da653d1206d0dcc9563c0cd14b6197cd49081cc605d78aed3c6cc4db863d6` |
| `rspace++/src/rspace/persistence/durable/recovery.rs` | `60c375acd1eeefd8f821bea60ffec84fa9b1eb51b0856541d01059c8423fcfb8` |
| `rspace++/src/rspace/persistence/durable/admission.rs` | `eef3657433921ca66c94d3e24c028d903f7599de18d9c4510e797b419c9b1671` |
| `rspace++/src/rspace/persistence/durable/index.rs` | `28d5ae62109e70dee01a9e63044bb26f269fd4adfcf2d79c2da5ae98161ccfc0` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `143f15ce54a0d20d1fe64b21c3f6a207331f1e75c1464ed4895dd7d2c35143e8` |
| `rspace++/src/rspace/persistence/retention.rs` | `e5971ed87b6973bdbc09fd02609507c2a8dc8af4459f8bb973df59e2782529de` |
| `rspace++/tests/persistence/owner_history_production.rs` | `b9047bdfee02e99f91fbb2d2e14e2f6d7cd6901e1562a6457e0b4230bfb99b69` |
| `rspace++/tests/persistence/trusted_history_properties.rs` | `986fd173fa171ba90c95a5702bbe8322d9443a77e3aa568f373bfc0b4c2f3f74` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `6d8d8d64652eeb9e46341a250700d2c1863f39b9e4405f07c940be1df0c3e8cb` |
| `rspace++/tests/persistence/trusted_history_production.rs` | `61cb02a4c26bd4868bbf464df9b9a202d9119604184018626f37536ada9df82e` |
| `rspace++/tests/persistence/mod.rs` | `217c64ad3bf68e72acbcf87848fda631cc126899a63e3ff0ab8e55ab069b33e4` |
| `docs/design/pathmap/figures/owner-history-step.puml` | `4e12fffd099800ef669eea6b096641487d741c567259881298100dd134f7239e` |

### Torn reclamation intents (bug 11970, 2026-10-28)

The reclamation intent is the 88-byte record `reclaim.v1` that a checkpoint
writes before its catalog compare-and-swap (CAS). It names the superseded
generation whose pack and index the checkpoint may delete once no reader pins
them. Bug 11970, found while reviewing the design of task 11967, concerned how
that record was written. `write_reclamation_candidate` created `reclaim.v1`
and then set its length, wrote it and synchronized it in place. A failure or a
crash between the creation and the synchronization left an empty file, 88 zero
bytes or a partial record under the final name. Every reader refuses such a
record (`Length` or `InvalidData`), and recovery reads the intent before
anything else, so held-lease recovery, cold admission and every later
checkpoint failed for good.

**The fix.** The intent is now published atomically:

1. The writer creates `reclaim.v1.tmp` exclusively (`O_EXCL`), sets its
   length, writes the record and synchronizes the file.
2. It authenticates the file against its handle, as every durable file is
   authenticated before it is named.
3. It renames the temporary to `reclaim.v1` without replacing an existing
   record: `renameat2` with `RENAME_NOREPLACE` on Linux with glibc, and
   `renameatx_np` with `RENAME_EXCL` on Apple platforms. Other platforms
   refuse the rename as `Unsupported`.
4. It synchronizes the directory.

Recovery first removes a leftover `reclaim.v1.tmp`, after authenticating it
against its handle, and synchronizes the directory. The writer refuses a
leftover temporary and never removes one itself.

The removal is safe for this reason. The intent is written before the CAS,
and the writer returns only after the rename and the directory
synchronization. A temporary that is still present therefore means that no CAS
happened for its checkpoint, so it carries no deletion obligation.

The final name, the 88-byte format and the reader do not change. A corrupt
complete record under the final name stays fail-closed, as TS-11 and TH-01
require. The
[reclamation-intent contract](gc-pending-reclaim-intent-2026-10-08.md#torn-intent-publication-bug-11970)
diagrams the in-place writer beside the atomic one.

**The model.** `RSpaceGcPendingReclaimIntent.tla` gains the following.

- An action `TearIntent`: a failure anywhere from the creation to the rename,
  or a rename lost to a power failure, while a checkpoint's files are staged.
  It ends the writer process as `Crash` does. With the constant
  `AtomicIntentPublication` it adds the temporary `IntentTemp` to the files.
  Without it, which is the in-place writer, it sets the intent to `"torn"`.
- `Recover` refuses a torn intent, as production does. With the constant
  `RemoveIntentTempOnRecover` it removes the temporary.
- `PersistAB` and `PersistBC`, the intent writes, require that no temporary
  exists. This is the writer's exclusive creation.

In the model, $`\mathit{live}`$ holds while a writer process runs,
$`\mathit{intent}`$ is the record under the final name (`"none"`, `"AB"`, `"BC"`
or `"torn"`), and $`\mathit{files}`$ is the set of files in the store directory.
The three new invariants are:

```math
\begin{aligned}
\mathit{RecoveryAvailable} &\triangleq \lnot\mathit{live} \Rightarrow \mathrm{ENABLED}\ \mathit{Recover} \\
\mathit{TornIntentNeverPublished} &\triangleq \mathit{intent} \neq \texttt{"torn"} \\
\mathit{IntentTempNeverLive} &\triangleq \mathit{live} \Rightarrow \mathit{IntentTemp} \notin \mathit{files}
\end{aligned}
```

- The first says that a stopped store can always recover.
- The second says that the final name never holds a torn record.
- The third says that no temporary survives into a running writer.

The base configuration checks all three together with the earlier invariants.
It generates 4,533 states, 1,700 of them distinct, with no error; one worker
reaches depth 28. The count is the same as before the guards were added,
because the temporary exists only while no writer runs, so the guards never
bind on a reachable state. Adding `TearIntent` raised the distinct count from
1,683.

**Negative controls.** Each new invariant has a configuration that must violate
it. The model's runner, `scripts/check-rspace-gc-pending-reclaim-intent-model.sh`,
requires each control's trace to contain the listed actions in order and to end
at the last one. With two workers a counterexample need not be the shortest, so
the runner checks order rather than an exact trace.

| Configuration | Constant changed | Violated invariant | Actions required in the trace |
|---|---|---|---|
| `RSpaceGcPendingReclaimIntentTornIntentMutant.cfg` | `AtomicIntentPublication = FALSE` | `RecoveryAvailable` | `TearIntent` |
| `RSpaceGcPendingReclaimIntentTornIntentPublishedMutant.cfg` | `AtomicIntentPublication = FALSE` | `TornIntentNeverPublished` | `TearIntent` |
| `RSpaceGcPendingReclaimIntentKeepIntentTempMutant.cfg` | `RemoveIntentTempOnRecover = FALSE` | `IntentTempNeverLive` | `TearIntent`, then `Recover` |

The model's four earlier controls still fail as before:

- the dropped intent on an ordinary advance violates
  `SelectedDeletionObligationCovered`;
- the overwritten pending intent violates the same invariant;
- the pre-CAS witness reaches `NoPreCasRecoveryOrphan`;
- the retry witness reaches `NoRetryAfterPendingAdvanceAndRestart`.

The integrated proof runner calls the model's runner.

**Production controls.** RI-04 plants a record at `reclaim.v1` after the
checkpoint has resolved the previous intent and before it writes its own. The
publication must then fail without replacing the planted record, so a rename
with flags 0, which replaces, is a mutation that RI-04 kills. If recovery no
longer calls `remove_reclamation_temporary`, a leftover temporary survives.
RI-01 and RI-03 then find a reclamation name after recovery, and PUB-16's retry
is refused by the exclusive creation; these tests kill that mutation.

**The RI rows.**

| ID | Required property | Test |
|---|---|---|
| RI-01 | Every tear recovers. The intent write is torn at each of 94 points, with the predecessor pinned or not. The points are the failpoints at creation, at the length, at the file synchronization, before the rename and at the directory synchronization with the rename kept or lost, and a torn write of each prefix length from 0 to 87 bytes. Each publication is definitely not published and keeps the old catalog. Cold admission then succeeds at the old catalog image and leaves neither `reclaim.v1` nor `reclaim.v1.tmp`, the predecessor's generation can still be acquired, and the retry publishes the expected successor. A pinned predecessor keeps the retry's intent until the pin is dropped. After the final reclamation the predecessor's pack and index are gone and its generation is retired. | `every_reclamation_intent_tear_recovers_pre_cas_authority_and_retries` |
| RI-02 | Generated tears. Sequences of one to three tears, with the predecessor pinned or not, never leave a torn record under the final name, and each sequence recovers and retries as in RI-01. | `generated_reclamation_intent_tears_never_publish_a_torn_record` |
| RI-03 | Held-lease recovery. The existing physical-write failpoint interrupts the intent write. Recovery under the held lease then succeeds at the old catalog image. The final name is absent, and the temporary holds the 88 zero bytes of its set length. The retry publishes the expected successor and unlinks the successor pack that the failed attempt left before it writes its own. | `interrupted_reclamation_intent_write_recovers_pre_cas_authority` |
| RI-04 | No replacement. A record planted at the final name after the previous intent was resolved is never replaced. The publication is definitely not published and is refused as `AlreadyExists`, and the planted bytes stay unchanged. Cold admission refuses them as `InvalidData`, which is fail-closed. After their removal the store admits with no temporary left, and the retry publishes the expected successor. | `reclamation_intent_publication_never_replaces_an_existing_record` |

`model_property_manifest_is_unique_and_exhaustive` checks these four bindings:

- each RI ID is bound once in `PROPERTY_BINDINGS`, to a test function that
  exists;
- each has one derivation row, whose TLA+ symbols are defined in
  `RSpaceGcPendingReclaimIntent.tla`;
- the integrated proof runner calls the model's runner.

The test reads the model as source only. The model's configurations list their
invariants in an `INVARIANTS` block, which the test's exhaustive rule for
single `INVARIANT` lines does not parse, so the model's runner checks the
baseline and its seven controls instead. PUB-16 gains the five intent
failpoints as cases: each publication is definitely not published, and a
restart keeps the old authority.

RI-01 compares the directory taken right after each failure with these
expectations. "Complete" means the full 88-byte record that the retry later
publishes.

| Tear point | `reclaim.v1` after the failure | `reclaim.v1.tmp` after the failure |
|---|---|---|
| Creation | absent | absent |
| Length | absent | empty |
| Torn write of a prefix of $`k`$ bytes | absent | the first $`k`$ bytes of the record, then zeros to 88 bytes |
| File synchronization | absent | complete |
| Before the rename | absent | complete |
| Directory synchronization, rename kept | complete | absent |
| Directory synchronization, rename lost | absent | complete |

A prefix of 84 to 87 bytes is already the complete record, because the
record's last four bytes are always zero: the checksum is a `u32` encoded as a
`u64`.

**Boundary.**

- Faults are injected in-process, and the operating system keeps every write.
  RI-01 simulates one power-loss outcome, the lost rename, by renaming the
  record back to the temporary. The synchronizations before and after the
  rename are covered by the model and by review only.
- A foreign or unsafe entry at `reclaim.v1.tmp` makes recovery fail closed:
  a symbolic link, a hard link, or the wrong owner or mode. Authenticating and
  classifying such residue belongs to the deferred filesystem-adversary
  hardening (tasks 6365, 6369, 6370, 6371 and 6374). So does the window between
  the handle check and the rename or unlink.
- A file system or kernel without the no-replace rename refuses every
  checkpoint as definitely not published (`EINVAL` or `ENOSYS`), and no data
  is lost; the next recovery removes the temporary. Platforms other than Linux
  with glibc and Apple refuse the rename as `Unsupported`, and the Apple
  branch is not compiled in CI.
- A torn record written by an earlier binary stays fail-closed. None is
  expected to exist, because the durable backend has no production caller yet
  (an inference).
- The record under the final name keeps its name, its 88 bytes and its
  format, and no identity changes. The one new name, `reclaim.v1.tmp`, exists
  only before the CAS; a binary built before the fix never opens it, because
  nothing in the durable code lists the directory. The consensus-change
  register records the changed outcomes of the dormant store as a CBR-053
  living continuation (2026-10-28).
- The RI tests compile on unix only.

The evidence in J-253 was taken at these sources. `publication.rs` and
`reclaim.rs` are unchanged and are pinned because the intent is written and
decoded there.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/io.rs` | `9debf4b1f1c84e4c566db9436c9d453a0865121236276f0b5fc0109ac265063c` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `e1a2c065e7d32f7293820eb312b236b225651304a853c216b5a1fdcdc04d2641` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `833060b7173fdc8140b900cb26b94c2768cd220ee32bde80e98236bf4c084e54` |
| `rspace++/src/rspace/persistence/durable/publication.rs` | `801348d8e8a547db88596136103bf45dab03d645208c2e0658445b5e49c30de6` |
| `rspace++/src/rspace/persistence/durable/reclaim.rs` | `9b1c05dce7fcc5280db2cd255f99c6fc286ec94144779bffc167a9ca6635485e` |
| `rspace++/tests/persistence/durable_publication.rs` | `e34bfa801b8341caac780f6bde1208cbacb0c8da3faaa1c4b7ff8460a51f3524` |
| `rspace++/tests/persistence/property_support.rs` | `aa22b29ad05a5d9b0807fa64b97d510c3c34c164b8ecadbd31ee2d125b598ebf` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `75c3194a426c1a463dec7e9b0a08ee5946976b450ac6be15895a666dd7c63d51` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntent.tla` | `3063fc43cde6fe8d22d8d6eb3d1ca5a67c0a73edf2ed730efd571c4b17861e6f` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntent.cfg` | `c792d3b7fffb69dce3f50f4ba2bc2c911d5573aa9edb21055205b3c47f0ecffa` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentDropOnAdvanceMutant.cfg` | `aa201d8df4b0e6bd1dcaf39abc9f55d577eb9da03854a763ec96f1934114d114` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentKeepIntentTempMutant.cfg` | `2d93bff3b20424060db7a8aaddf249ef274b9bea47558a1b606c5996a1400160` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentOverwriteMutant.cfg` | `2806b81b10963554b863f044ff17d322e31458ba851dc48021de2ba8b821586b` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentPreCasWitness.cfg` | `ace030fe91afdf4cda3b20d209326fff8fd83fcfec5a30b4cd63dfea1558142f` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentRetryWitness.cfg` | `05e52558e803faac2245edc1be9004e3a52d47b1805f6ba3fda5db863390d4d0` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentTornIntentMutant.cfg` | `c5b68113c0f675557f9a8687aa56c9eeb95127f4b882545844619abf635252bb` |
| `formal/tlaplus/stack_safe_pda/RSpaceGcPendingReclaimIntentTornIntentPublishedMutant.cfg` | `695ee2a01ee7020cf4246f760896468a94fa9ca79507710863bf302045e9aa0a` |
| `scripts/check-rspace-gc-pending-reclaim-intent-model.sh` | `aacc8ca9ef9da433c7889a421e841536802416ee3a6c5c61f3f18a30af1404ff` |

### Faulted-history production controls and timing (2026-10-28)

Task 11969, the last slice of task 7928, adds no test and changes no source.
It applies the reviewed plan's nine production negative controls, each in an
export of the candidate tree and never in the working tree; it times the
persistence selection with a fixed seed against the tree before task 7928; it
reruns every model that the task changed; and it completes the evidence of the
[faulted trusted histories section](#faulted-trusted-histories-and-cold-restart-equivalence-2026-10-27)
and of engineering journal J-253. With it, task 7928 is complete.

**Negative controls.** A script fixed, before any run, each control's
mutation as exact replacements in one production file, the TH test that must
kill it, and the message with which that test's production check must fail.
Each replacement had to occur exactly once in the clean export, and the clean
export had to pass the killer tests, TS-11 and the persistence selection
first. Then, for each control, the clean tree was exported again, the control
was applied, and three runs recorded whether it discriminates: its killer
test, which had to exit with status 101 and print the predicted message;
TS-11; and the whole persistence selection. Every run was a debug build on
one test thread with `PROPTEST_RNG_SEED=20261027`, so each property draws the
same sequence of cases in every run; a mutated run differs from the clean one
only by its mutation and by the failing cases saved during the run, which the
end of this part discusses. The reviewed plan calls C5, C8 and C9
regression controls, because existing tests were expected, or for C9
suspected, to kill them too; they ran and are recorded like the others
(review decision 10).

The failpoint sweep runs its four scenarios in parallel and reports the first
failing one in a fixed order, and every test stops at its first failing check.
So the table names the check that reported, and other checks may fail under
the same control too.

| Control | Mutation | Killed by, with the observed failure | TS-11 | Other tests that fail in the selection |
|---|---|---|---|---|
| C1 | The catalog commit maps a failed synchronization of `catalog.v1` to `Io`, which means definitely not published, instead of `CatalogCommitIndeterminate` (`publication.rs`) | TH-03, in the failpoint sweep at the catalog synchronization: "expected Indeterminate, got Io(Io { operation: SyncFile, … })" (`trusted_history_production.rs:1144`) | passes | TH-01 and TH-08, with the same failure |
| C2 | The re-read after the swap opens the catalog by pathname, through `DurableDirectoryV1::open`, instead of through the lease's pinned directory (`publication.rs`) | TH-07, in a seed history with a decoy at the re-read: the publication fails as `PublishedRecovery(Io(CatalogAuthorityChanged { … }))` where it should succeed | passes | TH-03 |
| C3 | The base check reads the catalog by pathname (`publication.rs`) | TH-07, in a seed history with a decoy at the base check: the publication runs no hook where all thirteen trusted hooks were expected (`trusted_history_production.rs:2161`) | passes | TH-03 |
| C4 | Held-lease recovery opens its reader by pathname instead of cloning the lease's reader (`startup.rs`) | TH-07, in a seed history with a decoy before held-lease recovery: "held-lease recovery failed: Startup(Io(Io { operation: OpenFile, … NotFound … }))" | passes | TH-03, and TH-08 with "a reconcile was refused: Recovery(AuthorityChanged)" |
| C5 | The publication keeps a pack suffix beyond the appointed prefix instead of truncating it (`publication.rs`) | TH-03, in the sweep's retry of the prepared successor: no hook ran where the whole trusted trace was expected (`trusted_history_production.rs:2730`) | fails: "the trusted publication failed: Append(Io(Length { … expected: Exact(1299), actual: 3017 }))" | TH-01, TH-08, five publication tests and three authority tests |
| C6 | The trusted publication truncates the pack at its entry, before the preflight can refuse, instead of after the preflight (`publication.rs`) | TH-06, in the sweep's stale retry: the refused retry had truncated the pack, "a stale retry changed the store: differing files [(\"pack-00000000000000000001.rsp\", Some(48113), Some(43293))]" (`trusted_history_production.rs:688`) | fails: the crash check's torn-slot case "Torn(1)" (`durable_publication.rs:874`) | TH-01 and TS-07 |
| C7 | The catalog commit ignores a failed synchronization of `catalog.v1` and adopts the successor (`publication.rs`) | TH-03, in the failpoint sweep at the catalog synchronization: the hook trace runs on to `AfterCatalogSyncBeforeRecovery` where it should end at the failed synchronization (`trusted_history_production.rs:2161`) | passes | TH-01 and TH-08 |
| C8 | Reclamation deletes a superseded generation's files although a reader still pins them (`io.rs`) | TH-05, in a seed history: "a fresh checkpoint left a mixed reclamation: (false, true)", that is, no reclamation intent while a reader is attached | fails: "read the deferred reclamation intent: … NotFound" (`trusted_successor_production.rs:2857`) | TH-03, RI-01, RI-02, TS-10 and five publication tests |
| C9 | Held-lease recovery rebuilds the index only when it is shorter than the appointed tail, so it keeps an index suffix (`startup.rs`) | TH-03, in the sweep's rebound retry: the retry stops after `AfterPrepareSync` where the whole trusted trace was expected (`trusted_history_production.rs:2739`) | fails: "the trusted publication failed: Append(Io(Length { … expected: Exact(280), actual: 496 }))" on the index | TH-01, TH-08, five publication tests, three authority tests and three startup tests |

The older tests that fail under C5, C8 and C9, besides TS-11, are these:

- In `rspace++/tests/persistence/durable_publication.rs`:
  - under C5 and C9:
    - `borrowed_incremental_publication_recovers_and_retries_every_phase`
    - `borrowed_incremental_publication_retains_authority_for_every_torn_slot_prefix`
    - `borrowed_incremental_publication_unwind_keeps_the_outer_owners_capabilities`
    - `incremental_terminal_capture_precedes_commit_and_survives_callback_unwind`
  - under C5 only: `borrowed_incremental_publication_trims_only_under_exact_store_authority`
  - under C9 only: `selected_successor_damage_never_falls_back_and_repairs_safe_index`
  - under C8:
    - `checkpoint_reclamation_waits_for_older_epoch_pin_from_independent_open`
    - `compaction_keeps_transitive_retained_closure_through_old_pack_lease`
    - `pending_reclamation_retries_partial_deletion_after_restart`
    - `pending_reclamation_survives_incremental_epoch_and_blocks_second_checkpoint`
    - `pending_reclamation_waits_for_selected_successor_verification`
- In `rspace++/tests/persistence/durable_rspace_authority.rs`, under C5 and C9:
  - `cold_restart_before_acknowledgement_selects_old_or_new_without_delta_replay`
  - `owner_checkpoint_histories_preserve_multiplicities_across_retry_and_cold_restart`
  - `owner_uncertain_old_and_new_publications_retain_the_gate_and_original_writer`
- In `rspace++/tests/persistence/durable_startup.rs`, under C9:
  - `every_recoverable_index_shape_is_replaced_by_the_same_canonical_bytes`
  - `postrename_sync_failures_leave_only_a_complete_recoverable_index`
  - `prepublication_io_failures_leave_the_official_index_byte_identical`

All nine controls were killed. The clean export first passed the two killer
functions and TS-11, then the whole selection: 493 passed and four ignored.
Under each control, the killer exited with status 101 after 0.61 to 0.84 s and
printed the predicted message.

TS-11 is the trusted publication's crash and retry test, whose production half
task 11946 added. It passes under C1, C2, C3, C4 and C7, and under those five
controls no test
outside the TH family fails. Those five faults are therefore caught only by
the tests that task 7928 added. They are a misclassified catalog
synchronization, the three reads by pathname, and an adopted catalog that was
never synchronized.

TS-11 fails under C5, C6, C8 and C9, so older tests catch those four too.
The reviewed plan had expected C6 to survive the existing tests, but TS-11 and
TS-07 kill it. The predictions recorded before the run named every test that
failed under C1, C2, C3, C4, C6 and C7. Under C5, C8 and C9 they missed twenty
failing tests: seven under C5, four under C8 and nine under C9. The four under
C8 include RI-01 and RI-02. RI-01 failed 94 of its 188 tear cases, which is
exactly the number of cases with a reader pinned. Without a pin the mutated
code behaves like the original, so presumably only the pinned cases failed (an
inference from the mutation; the test lists only its first three failures, all
pinned).

Proptest saves each failing case to a regression file, and every property test
compiled from the same source file replays that file's cases first. Nine cases
were saved during the run:

- three in the TS-11 runs of C5, C8 and C9;
- six in the selection runs of C1 and C5 to C9.

Each fresh export restored the one tracked regression file and removed the
others. Within a control, a saved case could reach a later run only through a
test that shares its file. Each such test passed, failed first in a fixed seed
history, is not a property test, or reads another file. So no outcome above
depends on a replayed case.

**Timing.** The persistence selection, every rspace++ library test under
`rspace::persistence` plus the kind-1 streaming manifest test, was timed on two
trees: the baseline, the tree of task 11946's commit `68e61c0f9`, the last
before task 7928, and the candidate, the tree of commit `7823b8db9`, which this
slice documents. Each tree's test binaries were built unpinned; each run was
pinned to one CPU with `taskset -c 8`, used one test thread, the seed
`PROPTEST_RNG_SEED=20261027` and libtest's per-test times, and ran in the order
baseline, candidate, candidate, baseline in each profile, so that a linear
drift of the host cancels between the trees. The seed makes every property
whose strategy is unchanged draw the same cases on both trees. Write $`B`$ and
$`A`$ for the sets of tests timed in both runs of the baseline and of the
candidate, and $`\bar b_t`$ and $`\bar a_t`$ for a test's mean time over its
two runs on each tree. The change in the summed per-test time then decomposes
exactly into the common tests' changes, the candidate's new tests and the
baseline's removed tests:

```math
\sum_{t \in A} \bar a_t - \sum_{t \in B} \bar b_t
  = \sum_{t \in A \cap B} \left(\bar a_t - \bar b_t\right)
  + \sum_{t \in A \setminus B} \bar a_t
  - \sum_{t \in B \setminus A} \bar b_t .
```

A common test's spread is the sum of its two within-tree run-to-run
differences, and the test is flagged when its change exceeds both 0.1 s and
twice its spread, the rule of task 11946 (J-252).

| Profile | Baseline runs | Candidate runs | Summed per-test means | The 486 common tests (summed spread) | The seven new tests |
|---|---|---|---|---|---|
| debug | 78.02 s, 76.45 s | 93.98 s, 92.56 s | 77.18 s to 93.21 s, +16.03 s | −0.66 s (5.45 s) | +16.68 s |
| release | 35.24 s, 34.69 s | 43.26 s, 43.07 s | 34.92 s to 43.12 s, +8.20 s | −0.49 s (3.20 s) | +8.69 s |

Every run passed: the baseline ran 486 tests with three ignored, the candidate
493 with four ignored, the fourth being the kill child that the failpoint sweep
starts. No common test was flagged in either profile. The largest increase,
+0.30 s in debug for
`incremental_inventory_matches_cold_admission_at_every_history_step`, is
less than twice its spread of 0.17 s, and in release the same test was 0.29 s
faster; the largest change in magnitude, −0.33 s in debug, lies within its
spread of 0.71 s. So the whole rise is the seven test functions that task 7928
added, and no existing test became measurably slower or faster:

| Test function | Rows | Debug | Release |
|---|---|---|---|
| `every_failpoint_recovers_to_the_appointed_prefix_at_later_history_steps` | TH-03, TH-04, TH-06 | 10.98 s | 6.31 s |
| `faulted_trusted_histories_match_independent_cold_admission_at_every_step` | TH-01, TH-02, TH-05, TH-07 | 3.34 s | 1.37 s |
| `owner_histories_resolve_ambiguity_acknowledge_once_and_never_replay_the_journal` | TH-08 | 1.47 s | 0.40 s |
| `every_reclamation_intent_tear_recovers_pre_cas_authority_and_retries` | RI-01 | 0.72 s | 0.50 s |
| `generated_reclamation_intent_tears_never_publish_a_torn_record` | RI-02 | 0.16 s | 0.11 s |
| `interrupted_reclamation_intent_write_recovers_pre_cas_authority` | RI-03 | 0.004 s | 0.003 s |
| `reclamation_intent_publication_never_replaces_an_existing_record` | RI-04 | 0.004 s | 0.003 s |

The failpoint sweep is the one parallel test among them: it runs its four
scenarios on four threads, then four kill threads that start its thirteen kill
children at the same time (`trusted_history_production.rs:3330-3447`). Pinned
to one CPU, they share that CPU, so the pinned time approaches the sweep's
summed CPU time. Run alone, seeded, in the order unpinned, pinned, pinned,
unpinned:

| Profile | Unpinned | Pinned to one CPU | Ratio |
|---|---|---|---|
| debug | 3.99 s, 3.62 s | 10.93 s, 10.90 s | 2.9 |
| release | 1.85 s, 1.91 s | 6.27 s, 6.28 s | 3.3 |

The pinned times lie within 1% of the sweep's times inside the selection. The
unpinned means, 3.80 s and 1.88 s, differ from those at task 11968's commit
(3.65 s and 1.90 s) by less than the 0.37 s and 0.06 s between their own two
runs. So neither the selection around the sweep nor bug 11970's fix changed it
measurably. With the sweep's unpinned means in place of its pinned ones, the
seven new tests add 9.51 s in debug and 4.26 s in release to an unpinned run
of the selection (an inference from these runs).

The reviewed plan estimated that the TH tests would add 13 to 27 s in debug
and 6 to 12 s in release to the single-threaded selection, measured unpinned at
task 11946. Pinned, the three TH functions take 15.79 s and 8.07 s, inside both
ranges. With the sweep's unpinned time, the condition of that baseline, they
take about 8.62 s and 3.65 s, below both (an inference). The four RI
functions, which bug 11970 added and the estimate did not cover, take another
0.89 s and 0.62 s.

**Formal reruns.** Every model that task 7928 changed was checked again on
the export of the candidate tree, with its negative controls and witnesses.
The three models with their own runner scripts ran through those scripts with
the integrated runner's TLC settings (one worker, a 512 MiB heap, 768 MiB of
high memory, 900 s), except for a 4 GiB memory ceiling in place of its 1 GiB.
A ceiling decides only whether a run completes, never its counts. The
single-publication model ran a copy of the integrated runner's block for it
(`scripts/check-stack-safe-pda-proofs.sh:1775-1833`), which checks the same
configurations against the same invariants, on two workers:

| Model | Result |
|---|---|
| `RSpaceTrustedIncrementalHistory.tla`, `scripts/check-rspace-trusted-history-model.sh` | one initial state; 19,420 states generated, **6,841** distinct, depth 46; each of the ten causal mutants violated its named invariant and each of the eight witnesses was reached |
| `RSpaceJournalPublicationOwner.tla` with the decoy, `scripts/check-rspace-journal-owner-model.sh` | four initial states; 1,716,908 generated, **64,080** distinct, depth 21; the nineteen causal mutants and eleven witnesses passed their designated checks |
| `RSpaceGcPendingReclaimIntent.tla`, `scripts/check-rspace-gc-pending-reclaim-intent-model.sh` | one initial state; 4,533 generated, **1,700** distinct, depth 28; each of the five mutants violated its named invariant with its actions in order, and both witnesses were reached |
| `RSpaceTrustedIncrementalPublication.tla`, the integrated runner's block | 64 initial states; 664 generated, **588** distinct, depth 13; each of the eight causal mutants violated its named invariant and each of the six witnesses was reached |
| `TrustedSuccessorCapability.v` | compiled with its eight-file dependency closure; all **20** capstones print "Closed under the global context", and `coqchk` accepts the module |

Every count equals the one recorded when the model last changed. The
reclamation-intent model's file and configuration hashes also equal the pins
of the [torn reclamation intents section](#torn-reclamation-intents-bug-11970-2026-10-28).

**Boundary.**

- With this slice task 7928 is complete. Every TH row checks production code,
  and nine production faults that the reviewed plan chose are each killed by
  their named TH test. Five of them only by the TH tests. These nine are the
  controls the plan chose, not every possible fault.
- The slice changes no source. Each control existed only in an export and was
  never committed. No persisted byte, identity, format, wire message or
  production outcome changes, so the consensus-change register gets no note
  (review decision 11).
- Semantic eviction does not exist under keep-all retention; a future one
  needs the maxima recomputed and the TH family extended.
- Reset identity and activation remain task 7756, and the substrate stays out
  of RSpace until task 7931 wires it in.
- No scaling is claimed here; the
  [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29)
  measures it (thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history,
  with gate G4 failing on allocated bytes). Faults are injected in-process and the
  operating system keeps every write, so no power-loss claim is made.
- The timing holds for one seed, one host and pinning to one CPU. The
  unpinned figures are inferences from the sweep's own unpinned runs.

The evidence in J-253 was taken at these sources, which this slice leaves
unchanged. The models' files are those pinned in the faulted trusted histories
section and the torn reclamation intents section, and they are unchanged too.

| Pinned source | SHA-256 |
|---|---|
| `rspace++/src/rspace/persistence/durable/publication.rs` | `801348d8e8a547db88596136103bf45dab03d645208c2e0658445b5e49c30de6` |
| `rspace++/src/rspace/persistence/durable/startup.rs` | `e1a2c065e7d32f7293820eb312b236b225651304a853c216b5a1fdcdc04d2641` |
| `rspace++/src/rspace/persistence/durable/io.rs` | `9debf4b1f1c84e4c566db9436c9d453a0865121236276f0b5fc0109ac265063c` |
| `rspace++/src/rspace/persistence/durable/mod.rs` | `833060b7173fdc8140b900cb26b94c2768cd220ee32bde80e98236bf4c084e54` |
| `rspace++/src/rspace/persistence/retention.rs` | `e5971ed87b6973bdbc09fd02609507c2a8dc8af4459f8bb973df59e2782529de` |
| `rspace++/tests/persistence/mod.rs` | `217c64ad3bf68e72acbcf87848fda631cc126899a63e3ff0ab8e55ab069b33e4` |
| `rspace++/tests/persistence/property_support.rs` | `aa22b29ad05a5d9b0807fa64b97d510c3c34c164b8ecadbd31ee2d125b598ebf` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `75c3194a426c1a463dec7e9b0a08ee5946976b450ac6be15895a666dd7c63d51` |
| `rspace++/tests/persistence/trusted_history_properties.rs` | `986fd173fa171ba90c95a5702bbe8322d9443a77e3aa568f373bfc0b4c2f3f74` |
| `rspace++/tests/persistence/trusted_history_production.rs` | `61cb02a4c26bd4868bbf464df9b9a202d9119604184018626f37536ada9df82e` |
| `rspace++/tests/persistence/owner_history_production.rs` | `b9047bdfee02e99f91fbb2d2e14e2f6d7cd6901e1562a6457e0b4230bfb99b69` |
| `rspace++/tests/persistence/trusted_successor_properties.rs` | `d7647265b2bd05d10fd1d7c58e6af0dcd88d9a7c95002f3d5cfc0648c039f2b6` |
| `rspace++/tests/persistence/trusted_successor_production.rs` | `57557ac33cd057613cde2d683290d78b7fd95bc1226662ef55f9fb7ae58ca9c2` |
| `rspace++/tests/persistence/durable_publication.rs` | `e34bfa801b8341caac780f6bde1208cbacb0c8da3faaa1c4b7ff8460a51f3524` |
| `rspace++/tests/persistence/durable_rspace_authority.rs` | `6d8d8d64652eeb9e46341a250700d2c1863f39b9e4405f07c940be1df0c3e8cb` |
| `rspace++/tests/persistence/durable_startup.rs` | `c5f0cceebe61777c783f42462d5733655c41d3f0e4267b95d4a995109db61188` |

### Fixed-delta trusted commit scaling (2026-10-29)

Task 7929 asks whether a trusted commit costs only what it changes. The
trusted successor path seals a mutation, binds it under an exact trusted
policy, prepares it and publishes it with no physical recovery and no cold
admission (the
[trusted successor publication section](#trusted-successor-publication-and-owner-2026-10-27)).
Its cost should therefore depend on the keys that a commit changes, not on the
size of the current state or on the number of retained states. The task's
first slice writes this section. It states the cost that the code implies, the
fixture, the oracles and three properties, SW-01 to SW-03. They check, at
geometrically growing sizes, that each trusted commit's work equals an oracle
computed independently from the objects that the commit changes. The second
slice adds the release-mode harness, its driver and SW-04, which checks the
harness itself (the harness subsection below). The third slice (task 11990)
runs the preregistered measurement and records its results and its limits at
the end of this section, after the SW rows.

**Terms.**

- $`B`$ and $`T`$ are the semantic manifests of a commit's base and target
  states, and $`\mathrm{cl}(S)`$ is the set of objects reachable from the
  manifest $`S`$, the manifest included.
- $`U`$ is the retained union before the commit: the objects reachable from the
  manifest of any retained state.
- A radix image is the set of radix nodes that encode the retained history's
  keys. $`I`$ and $`I'`$ are the radix images before and after the commit, and
  $`a`$ and $`a'`$ are the old and new retention anchors.
- $`N`$ is the number of data keys in the current state, $`H`$ the number of
  historical states, and $`m`$ the number of keys that a commit deletes.
- $`\mathrm{outdeg}(x)`$ is the number of out-references of the object $`x`$,
  counted with multiplicity: 3 for a semantic manifest; its children, plus one
  for a content endpoint, for a DAG node; 0 for a blob; 1, its radix root, for a
  retention anchor; and its children for a radix node.
- $`\mathrm{len}(x)`$ is the length of the canonical bytes of $`x`$.

A ledger trial charges one reference for each root that it adds or removes and
one for each out-reference that it transfers. It expands an object, reading it
and transferring its out-references, only when the object's count in the
trial's domain crosses between 0 and 1, and it quotes the object's bytes then.
The activation phase runs before the deactivation phase
(`ledger_trial.rs:142-143`).

**Cost implied by the code.** The measurement confirms the logical work below:
the verify routine asserts it at every commit, and every verify run of the
decision population completed (the results at the end of this section). The
table does not price the bytes that each `states` insert copies, the cost that
gate G4 measures.

| Step | Work per trusted commit | Source |
|---|---|---|
| Current-domain trial | Adds $`T`$ and removes $`B`$, and expands only the objects whose current count crosses between 0 and 1 | `run_domain_trials` (`admission.rs:1345`); the phases at `ledger_trial.rs:142-143` |
| Retained-domain trial | Adds $`T`$ only, because keep-all retention never removes a retained root | `admission.rs:1345` |
| Ledger maps | A clone of a PathMap, which copies only its root reference; then one `states` insert per consumed event and one roots insert per root delta | `ledger_trial.rs:135`, `:335`, `:212` and `:221` |
| V2 metadata | A path-local edit of the radix image, a new anchor, and a metadata trial that deactivates the predecessor anchor | `commit_retention_v2` (`v2_writer.rs:50`); `attach_retention_v2` (`admission.rs:1425`) |
| V1 metadata, the control | Rewrites the whole manifest: $`96 + 56n`$ bytes for $`n`$ historical entries | `admission.rs:1388` |
| Object index | A clone of a PathMap plus one record per fresh object | `index.rs:200` |
| Live packs | One, because the generation publisher admits exactly one live pack and an incremental commit keeps the first live and active packs | `publication.rs:1145` and `:1045-1046` |
| Cold path | Absent: no physical recovery, no cold admission and no index construction | TS-09 |

**Fixture.** Each key is a datum on its own channel, the channel and datum
texts are fixed-width hexadecimal (`chan-000000`, `datum-000000` and so on),
and every key starts with multiplicity 1. The history holds generations 1 to
$`H`$. Each historical state is a manifest that shares the current roots and
differs only by its legacy root, and the current state is generation $`H+1`$.
The V2 twin replaces the V1 manifest by an anchor over the radix image of the
same history (`migrate_checkpoint_source_to_v2`). A replacement at commit $`c`$,
counted from 0, gives a key the fresh multiplicity $`2 + c`$, and a deletion
removes a key. Every key, text and multiplicity is new and retention keeps
every state, so no object that a commit seals is already stored. The verify
routine checks this, which makes every sealed object a fresh data record in the
write-ahead log.

**Oracles.** Neither oracle uses production code. `walk`, which keeps an
explicit stack, computes closures over an in-memory store, where the test
installs each target state with the independent canonical rebuild.
`build_radix` is the test model of the radix image. In each domain $`D`$, one
of current, retained and metadata, the expected work is

```math
\begin{aligned}
X_{\mathrm{current}} &= \mathrm{cl}(T) \mathbin{\triangle} \mathrm{cl}(B), &
\mathrm{refs}_{\mathrm{current}} &= 2 + \sum_{x \in X_{\mathrm{current}}} \mathrm{outdeg}(x), \\
X_{\mathrm{retained}} &= \mathrm{cl}(T) \setminus U, &
\mathrm{refs}_{\mathrm{retained}} &= 1 + \sum_{x \in X_{\mathrm{retained}}} \mathrm{outdeg}(x), \\
X_{\mathrm{metadata}} &= (I' \mathbin{\triangle} I) \cup \{a, a'\}, &
\mathrm{refs}_{\mathrm{metadata}} &= 2 + \sum_{x \in X_{\mathrm{metadata}}} \mathrm{outdeg}(x),
\end{aligned}
```

with $`\mathrm{expansions}_D = \lvert X_D \rvert`$ and
$`\mathrm{quoted}_D = \sum_{x \in X_D} \mathrm{len}(x)`$. The constants count
root events: the current trial adds $`T`$ and removes $`B`$, the retained trial
adds $`T`$, and the metadata trial adds $`a'`$ and removes $`a`$. The V1 writer
runs no metadata trial, so its metadata work is zero. Each set holds exactly
the objects whose count crosses between 0 and 1, because objects are
content-addressed: a child of an object in $`\mathrm{cl}(B)`$ is itself in
$`\mathrm{cl}(B)`$, so each object of $`\mathrm{cl}(T) \setminus \mathrm{cl}(B)`$
is reached only through objects that cross too.
`IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work`
proves this law for root-delta execution.

**The deactivate-first control.** A current trial that removed $`B`$ before it
added $`T`$ would expand $`\lvert \mathrm{cl}(B) \rvert + \lvert \mathrm{cl}(T) \rvert`$
objects, which exceeds $`\lvert X_{\mathrm{current}} \rvert`$ by exactly
$`2 \lvert \mathrm{cl}(B) \cap \mathrm{cl}(T) \rvert`$. SW-01 runs that order
with two ledger trials before every commit, checks the excess against the
trusted commit's measured expansions, and requires the smallest excess to grow
strictly from each key count to the next. An implementation that deactivated
first would therefore fail SW-01's work check.

**The verify routine.** SW-01 to SW-03 share one routine. Before each commit it
checks that the core's retained history and retention object equal the
model's. Then it checks the following.

1. **Work.** In each domain, the exact trusted policy's references, expansions
   and quoted bytes equal the oracle. The policy is measured on one seal of the
   mutation and bound to a second, identical seal.
2. **Sealed objects.** The bound transaction's objects equal
   $`X_{\mathrm{retained}}`$ plus the new metadata, byte for byte: for V2, the
   radix nodes of $`I' \setminus I`$ and the anchor $`a'`$, and for V1 the new
   manifest. None of them is already stored.
3. **Metadata step.** For V2, $`2 + \lvert X_{\mathrm{metadata}} \rvert \le 103`$,
   the new radix nodes total at most 518,500 bytes, and each is at most 10,370
   bytes; these are the RV bounds. For V1, the manifest is $`96 + 56n`$ bytes.
4. **Effects.** The publication passes hooks 2 to 14 with no cold admission and
   no index construction, as TS-09 observes.
5. **Files.** The active pack and the index generation stay the same. The pack
   grows by exactly the prepare header, the mutation, a 48-byte record header
   and the bytes of each fresh object, the 8-byte trailer and the commit
   record. The index grows by 72 bytes per fresh object, and each file ends at
   its durable tail.
6. **Ledger.** The new ledger entries are exactly the fresh digests:
   $`X_{\mathrm{retained}}`$ and, for V2, the radix nodes of $`I' \setminus I`$
   and $`a'`$. The zero-count entries grow by exactly the radix nodes of
   $`I \setminus I'`$ and $`a`$, which stay in the ledger with count zero.
   Every entry that changes is a root of the commit or a child of an expanded
   object, at most $`\sum_D \mathrm{refs}_D`$ entries change, and the root
   multiplicities change only by the commit's root deltas.

The count of logical ledger writes is $`\sum_D \mathrm{refs}_D`$ plus the root
deltas, 5 for V2 and 3 for V1: one `states` insert per consumed event and one
roots insert per root delta. It is derived from those lines, not measured.

**Cells.**

| Property | Writer | Sizes | Commits per chain |
|---|---|---|---|
| SW-01 | V2 | $`N \in \{2^4, 2^6, 2^8, 2^{10}\}`$ and $`H = 2^4`$ | 3, replacing 1, 4 and 1 keys, in 8 generated schedules per key count |
| SW-02 | V2, and V1 as the control | $`H \in \{2^4, 2^6, 2^8, 2^{10}\}`$ and $`N = 2^4`$ | 3, each replacing one key |
| SW-03 | V2 | $`N = 2^8`$ kept keys and 16 reserve groups of $`m \in \{1, 4, 16\}`$ keys at aligned offsets | 3, each deleting one reserve group, in 8 generated schedules per group size |
| SW-04 | V2 | $`N = 2^4`$ and $`H = 2^4`$; the L cell adds 16 reserve keys in groups of $`m = 2`$ | U: 2 warm-up and 4 timed replacements of one key. E: 2 warm-up, an eviction checkpoint, 2 deferred, 1 reclaim and 2 timed replacements. L: 2 warm-up and 4 timed deletions of one group. Each cell runs one build, two verify and one time child |

A generated schedule is an offset and an odd stride. In SW-01 the key replaced
at position $`p`$, counted across the chain's commits, is
$`(\mathit{offset} + p \cdot \mathit{stride}) \bmod N`$, so no key is replaced
twice. In SW-03 the group deleted at commit $`c`$ is
$`(\mathit{offset} + c \cdot \mathit{stride}) \bmod 16`$.

**The harness (task 11989).** SW-01 to SW-03 check exact work at small sizes
in the ordinary test build. The measurement needs larger cells, release builds,
clocks and resource counters, and a fresh process per cell, so a harness runs
each cell in child processes of the test binary. The scale child is the ignored
test `scale_child` in `trusted_scaling_resource.rs`. The driver,
`scripts/bench/rspace-fixed-delta-scaling.py`, starts it as
`<test binary> --exact rspace::persistence::tests::trusted_scaling_resource::scale_child --ignored --nocapture --quiet --test-threads=1`
with the environment variables `RSPACE_SCALE_CELL`, `RSPACE_SCALE_MODE`,
`RSPACE_SCALE_FIXTURE` and, except in build mode, `RSPACE_SCALE_SCRATCH`.
`--quiet` selects libtest's terse formatter, which prints nothing when a test
starts, so every record begins its own line. A line is a record exactly when it
starts with `SCALE_`.

A cell is twelve comma-separated `key=value` pairs, in this order and no other.
Every integer is canonical decimal, with no sign and no leading zero, so each
cell has one spelling.

| Key | Meaning | Constraint |
|---|---|---|
| `axis` | U, R, D, L or E; a label that the driver groups strata by | one of the five letters |
| `keys` | $`N`$, the kept data keys | $`N \ge 1`$ |
| `reserve` | reserve keys that only deletions remove | a multiple of $`m`$ when $`m > 0`$, else 0 |
| `history` | $`H`$, the historical states | $`H \ge 1`$ |
| `delta` | $`d`$, the keys that a commit replaces | exactly one of $`d`$ and $`m`$ is positive |
| `deletion` | $`m`$, the keys that a commit deletes | |
| `writer` | `v2` or `v1` | |
| `case` | `ordinary` or `evict` | $`K > 0`$ exactly for `evict` |
| `warmup` | $`W`$, the warm-up commits | |
| `deferred` | $`K`$, the commits between the eviction checkpoint and the reclaim commit | |
| `timed` | $`T`$, the timed commits | $`T \ge 1`$ |
| `seed` | the schedule seed, a `u64` | |

A process runs $`C`$ commits, counted from 0: $`W`$ labeled `warmup`; for
`evict`, a generation checkpoint with a reader pin, $`K`$ labeled `deferred`
and one labeled `reclaim`, run after the pin is dropped; then $`T`$ labeled
`timed`. So $`C = W + K + T`$, plus one for `evict`. The schedule draws an
offset and a stride from a SplitMix64 stream seeded with `seed`, over a range of
length $`n`$: $`\mathit{offset} = s_1 \bmod n`$ and
$`\mathit{stride} = (s_2 \bmod n) \mathbin{|} 1`$, where $`s_1`$ and $`s_2`$
are the first two outputs and $`\mathbin{|}`$ is bitwise or, and then the
stride grows by 2 until it is coprime with $`n`$. The map
$`p \mapsto (\mathit{offset} + p \cdot \mathit{stride}) \bmod n`$ is then a
bijection on $`\{0, \dots, n - 1\}`$. A replacement cell uses $`n = N`$, and
commit $`c`$ replaces the keys at positions $`c d`$ to $`(c + 1) d - 1`$, from
multiplicity 1 to $`2 + c`$. A deletion cell uses the $`G = \mathit{reserve} / m`$
reserve groups, $`n = G`$, and commit $`c`$ deletes the $`m`$ keys of group
$`(\mathit{offset} + c \cdot \mathit{stride}) \bmod G`$, from multiplicity 1
to 0. The parser refuses $`C d > N`$ and $`C > G`$, so no key is edited twice.

The child runs one of three modes.

- **Build** builds the fixture with $`N + \mathit{reserve}`$ keys and $`H`$
  historical states, installs its catalog, pack and index in the empty fixture
  directory, and prints the fixture record.
- **Verify** rebuilds the fixture in memory and refuses a fixture directory
  that does not hold exactly those three files, byte for byte. It installs the
  fixture in the empty scratch directory, admits it cold under the scale
  limits, and runs every commit through the verify routine. A probe wraps the
  routine's four phases: the second seal, the bind, the prepare and the
  publication. Around each phase it reads `/proc/self/io`, and inside it counts
  allocations with the test build's thread-local counting allocator. A commit's
  record is printed only after the whole routine passed, so a failed check
  leaves its record missing.
- **Time** copies the fixture files into the empty scratch directory and admits
  them cold on a worker thread with an 8 MiB stack, behind a ready and start
  handshake. The commits run with no oracle, no allocation counting and no
  `/proc/self/io` read, each bound under the unbounded trusted policy. Six
  readings of the thread CPU, process CPU and monotonic wall clocks bound five
  phases: seal, bind, prepare, publish, and release, which drops the prepared
  and bound transactions and the predecessor core. The samples wait in a
  preallocated buffer and are printed after the window.

In the evict case, after the warm-up commits the child pins the current
generation with a bare generation lease, which suffices because lease
registries are process-global per directory identity (`retention.rs:575-598`).
It then publishes a generation checkpoint from the trusted core, by the
cold-checked acquire and the generation publisher, as the trusted history tests
do. The $`K`$ deferred commits run with the pin held, then the pin is dropped
and the reclaim commit runs. In verify mode each record carries the names of
the files that the commit unlinked. Only the reclaim commit may unlink files,
and it must unlink exactly the superseded generation's packs, its index and
`reclaim.v1`, the three kinds that reclamation removes (`io.rs:1390-1429`).

The window is every post-warm-up commit. It opens just before the first of
them, after the eviction checkpoint and pin, by writing 5 to
`/proc/self/clear_refs`, which resets VmHWM to VmRSS.

**Records.** A record is tab-separated: the tag, the version `v1`, then
`key=value` fields in a fixed order. The driver refuses an unknown tag, a wrong
version, a missing, extra or reordered key, a non-canonical integer and a
duplicate commit index.

| Record | Printed | Fields, in order |
|---|---|---|
| `SCALE_HEADER` | first, by every child | `mode`, `cell` |
| `SCALE_FIXTURE` | once, by build | `keys_total`, `state_objects`, `state_bytes`, `history`, `union_objects`, `pack_bytes`, `index_bytes`, `records`, `build_ns` |
| `SCALE_VERIFY` | once per commit, by verify | `commit`, `label`, `target`; references, expansions and quoted bytes of the current, retained and metadata trials; `logical_writes`, `metadata_bytes`, `fresh_objects`, `fresh_bytes`, `pack_delta`, `index_delta`, `live_packs`, `old_path`, `new_path`, `deleted_objects`; the rewrite's loaded, edited and emitted nodes and exact path chunks; for each of seal, bind, prepare and publish, the allocation count, bytes, peak live bytes and largest allocation and the `syscr`, `syscw`, `rchar` and `wchar` deltas; `unlinked` |
| `SCALE_SAMPLE` | once per commit, by time | `commit`, `label`, `target`; thread, process and wall nanoseconds of each of seal, bind, prepare, publish and release; `rss_after` |
| `SCALE_PROCESS` | last, by verify and time | `mode`, `commits`, `admission_ns`, `rss_start`, `rss_end`, `hwm_window`, `hwm_reset`, `swap`, `live_packs`, `filesystem`, then the six probe calibrations |

`target` is the target manifest's digest in lowercase hexadecimal. In a verify
record $`\mathit{old\_path} + \mathit{deleted\_objects} = \lvert \mathrm{cl}(B) \setminus \mathrm{cl}(T) \rvert`$
and $`\mathit{new\_path} = \lvert \mathrm{cl}(T) \setminus \mathrm{cl}(B) \rvert`$,
so the current expansions equal their sum. `deleted_objects` is SW-03's deleted
closure, computed only when a commit removes keys, and 0 for a replacement.
`unlinked` is the sorted, semicolon-joined list of file names present before
the commit and absent after it, or `-`. `hwm_reset` is 1 when the reset brought
VmHWM within one page of VmRSS; when it is 0, `hwm_window` is the whole
process's peak.

**Calibrations.** `probe_syscr`, `probe_syscw`, `probe_rchar` and
`probe_wchar` are the cost of one `/proc/self/io` read, the difference of two
back-to-back reads. A phase's delta includes exactly one such read, the one
before the phase, so the driver subtracts the calibration once per phase. The
read's own `rchar` is the length of the file, which grows by one byte each time
a counter gains a digit, so a net `rchar` can exceed the phase's true reads by
the digits gained since the calibration, a few bytes, and never falls below
them. `probe_alloc_count` and `probe_alloc_bytes` are the allocations of the
effects observer's trace buffer, which grows inside the publication in verify
mode: 13 hook points pushed onto an empty vector of one-byte values, 2
allocations and 24 bytes. The child replays the same pushes under the same
counter before its first commit, and the driver subtracts them once per
commit. Time mode runs no observer and reads no `/proc/self/io`, so it reports
six zeros.

**Probes fail closed.** A probe that cannot measure stops the child; it never
reports a zero.

| Probe | Source | Refused |
|---|---|---|
| I/O | `LinuxIo::parse` of `/proc/self/io`, read whole into a 512-byte stack buffer | a missing counter, or a read that fills the buffer |
| Memory | `status_bytes` of `VmRSS`, `VmHWM` and `VmSwap` in `/proc/self/status`, read whole into an 8 KiB stack buffer | a missing field, or a read that fills the buffer |
| Clocks | `clock_gettime` of `CLOCK_THREAD_CPUTIME_ID` and `CLOCK_PROCESS_CPUTIME_ID`, and `Instant` for wall time | a failed call, or a phase whose reading does not advance |
| Allocations | the counting allocator's thread-local measurement | a probed phase that counts no allocation |
| Scratch | the `statfs` magic of the fixture and scratch directories | tmpfs or ramfs |

The `/proc` readers allocate nothing when they succeed, so a probe never
perturbs the allocation counts that it brackets.

**Scale limits.** Cold admission, the cold-checked acquire and the generation
publisher's preflight are the only checks of the durable limits; a trusted
commit checks none. Let $`P`$ and $`I`$ be the fixture's pack and index bytes,
$`R = \lfloor I / 72 \rfloor`$, which is at least its record count,
$`k = \max(d, m)`$, and $`c = C + 1`$, counting the eviction checkpoint as one
more whole-state write. One commit writes at most $`R + 104`$ records and
$`b = 4P + 518{,}500 + 2^{23} + 4096k`$ pack bytes: $`4P`$ covers a
shape-preserving rewrite of every state object, each at most twice its size,
and a V1 manifest of at most $`P + 56(c + 1)`$ bytes; 518,500 is the V2
path-local bound; $`2^{23}`$ covers anchors, record headers and the prepare and
commit records; and $`4096k`$ covers the mutation, whose keys are under 64
bytes each. The scale limits are $`R + c(R + 104)`$ records and index records,
$`I + 72c(R + 104)`$ index bytes, $`P + cb`$ pack and comparison bytes, scan
and physical reads of the pack bound plus the index bound, $`c + 1`$
transactions and 8 live packs, each raised to at least the test default. Limits
are refusal caps that allocate nothing, and a refusal still fails the child.

**The driver.** `scripts/bench/rspace-fixed-delta-scaling.py` collects one
population into a new directory and then analyzes it: the decision population,
a pilot (`--pilot`), or one negative control (`--controls NAME`). It refuses to
collect unless `SCALE_CAPPED=1` says that it runs inside the prescribed
`systemd-run` memory envelope and, except for a pilot, unless the measured
CPU's governor reads `performance`. It also refuses an output directory on
tmpfs or ramfs.

- **Children.** Before each child the driver takes one-second `mpstat`
  snapshots, at most 30, until the measured core's SMT siblings are each at
  least 90% idle and at least three physical cores have every sibling more than
  half idle. A host that stays busy stops the collection. Each child runs under
  `taskset` on the measured CPU with a per-child timeout. The driver creates the
  fixture and scratch directories with mode 0700, because the durable directory
  refuses a root that the group or others can write, and it removes each
  scratch directory when its child exits.
- **Order.** One build per cell, then the verify runs, then the time processes:
  3 warm-up rounds and 31 decision rounds, each visiting every cell in an order
  shuffled with seed 7929. Decision data use two verify runs; a pilot or a
  control uses one.
- **Failures.** A nonzero exit, a timeout, a SIGKILL (reported as an
  out-of-memory kill) and a record set other than exactly the one the contract
  prescribes for the mode and cell are each kept in `failures.tsv`, and any of
  them makes the population unqualified.
- **Summaries.** `fixtures.tsv`, `verify.tsv` (each verify record beside its
  process record), `samples.tsv`, `process-summary.tsv` and `failures.tsv`,
  with `metadata.json` (host facts, the sha256 of the binary and the sources,
  and the frozen protocol) and `manifest.json` (the sha256 of every file). A
  time process's statistic for a clock is the median, over its $`T`$ timed
  commits, of the five phases' per-commit sum.

The analysis reads only `metadata.json`, `fixtures.tsv`, `verify.tsv` and
`process-summary.tsv`, so `--analyze-only` and `--check` reproduce it from the
committed summaries. `--check` also verifies the manifest's digests and that
the recomputed analysis equals `analysis.json`. Every exponent is the
stratified log-log slope of `paired_benchmark_statistics.py`, with 10,000
resamples and seed 7929, against the axis's stratum: $`N`$ on U, $`H`$ on R,
$`d`$ on D and $`m`$ on L. A net value below zero fails its gate; it is never
clamped.

| Gate | Samples | Passes when |
|---|---|---|
| H1 | per decision time process, the thread CPU statistic | the exponent's upper 95% bound is at most 0.25 on U and on R |
| G1 | every verify record | each run has one record per commit, in order and labeled as scheduled, with $`\mathit{cur\_exp} = \mathit{old\_path} + \mathit{new\_path} + \mathit{deleted\_objects}`$ and no deleted closure in a replacement |
| G2 | the verify records and the time processes | the verify runs agree in every field except the per-phase allocation and I/O counters, and each time process's chain of targets equals the verify run's |
| G3 | per decision time process, the wall statistic | as H1 |
| G4 | per verify run, the $`T`$ timed commits' allocation count and bytes, net of the trace calibration once per commit | the upper 95% bound is at most 0.25 on U and on R, in every run |
| G5 | the same for `syscr`, `syscw`, `rchar` and `wchar`, summed over the four phases, each net of one I/O calibration | as G4, and every verify run is complete, because check 5 of the verify routine asserts the pack and index growth |
| G6 | per decision time process, `rss_end` minus `rss_start` | a two-sample bootstrap of the largest stratum's median minus the smallest's (10,000 resamples, seed 7929) has an upper 97.5% bound of at most 1 MiB on U and on R; no child reports swap and none is killed |
| G7 | every verify record's `metadata_bytes` | V2 commits write at most 518,500 bytes with $`2 + \mathit{meta\_exp} \le 103`$, a V1 commit $`c`$ writes exactly $`96 + 56(H + c + 1)`$ bytes, and every verify run is complete, because check 2 asserts the writer model's bytes |
| G8 | the thread CPU statistic on D and L, and the L verify records | the exponent's upper 95% bound is at most 1.25 against $`d`$ and against $`m`$, and every L commit deletes a nonempty closure equal to its excess current expansions |
| G9 | the E cells | each verify run has one reclaim commit, only it unlinks files, they are superseded packs, one index and `reclaim.v1`, and every verify run is complete, because check 4 asserts the trusted effects; each process's deferred median and reclaim commit, minus its timed median, are reported with 95% bootstrap intervals and no threshold |
| G10 | a control population | the control's expected signal is detected |

A decision population is accepted when it is qualified and H1 and G1 to G9
pass. A control population is accepted when G10 detects it, and a pilot is
never accepted. Descriptively, the analysis reports each cell's per-phase
medians with 95% bootstrap intervals, the sealing counters and admission time,
and $`c^*`$: the smallest $`c`$ on the grid $`2^{j/4}`$,
$`j \in \{-40, \dots, 80\}`$, such that adding $`c \cdot x`$ to every process
statistic at stratum $`x`$ lifts H1's bound above 0.25 (0 when the bound
already exceeds it).

`--self-test` checks the parser against 27 malformed records and 21 incomplete
record sets, the cells and guards, idle admission, the bootstraps, the
monotonicity of $`c^*`$, the net-value rules and 22 faulted children. Synthetic
collections at exponents 0, 0.1 and 1 recover each exponent exactly; an
incomplete verify run fails G5, G7 and G9; and a scratch directory that the
group or others can open fails the synthetic child.

**The SW rows.**

| ID | Required property | Test |
|---|---|---|
| SW-01 | Current growth. At each key count every commit passes the verify routine, and the deactivate-first control expands exactly $`\lvert \mathrm{cl}(B) \rvert + \lvert \mathrm{cl}(T) \rvert`$ objects, $`2 \lvert \mathrm{cl}(B) \cap \mathrm{cl}(T) \rvert`$ more than the trusted commit. The smallest excess grows strictly from each key count to the next. | `trusted_commit_work_is_the_changed_frontier_under_geometric_current_growth` |
| SW-02 | History growth. Every commit passes the verify routine, and each commit's current and retained work is the same at every history size and for both writers. The test prints each commit's metadata bytes for both writers, and from 64 historical states up the V1 manifest is larger than the V2 step at every commit. | `trusted_commit_work_is_unchanged_by_geometric_history_growth` |
| SW-03 | Last-reference deletion. The deleted closure is the part of $`\mathrm{cl}(B)`$ off the manifest, the namespace roots and every kept key's search path. It is disjoint from $`\mathrm{cl}(T)`$, holds each deleted key's leaf, and has between $`m`$ and $`2m - 1`$ objects. The retained frontier is exactly the new path $`\mathrm{cl}(T) \setminus \mathrm{cl}(B)`$, and after the commit every object of $`\mathrm{cl}(B) \setminus \mathrm{cl}(T)`$ has current count zero and a positive retained count. Every commit passes the verify routine. | `last_reference_deletion_expands_exactly_the_deleted_closure` |
| SW-04 | Harness determinism and fail-closed probes. A `/proc/self/io` copy that lacks any of `rchar`, `wchar`, `syscr`, `syscw`, `read_bytes` and `write_bytes`, or is truncated anywhere before the last of those values, is refused, as is a status copy that lacks `VmRSS`, `VmHWM` or `VmSwap`, and a `/proc` read that fills its buffer. Equal or decreasing clock readings, a zero allocation count, and tmpfs and ramfs are refused. 27 malformed cells are refused, and the schedule permutation is a bijection for every length up to 64 under four seeds. For each of the U, E and L cells, a build child prints a header and a fixture record; two verify children print the same records except the 32 per-phase probe fields and the process record; and a time child prints one sample per commit, with the labels in order, every nanosecond field positive, zero probes and the verify children's target chain. In E only the reclaim commit unlinks files, and they include `reclaim.v1`. | `scaling_harness_is_deterministic_and_fails_closed` |

**The results (task 11990).** The preregistered measurement is pgmcp experiment
446, and it asks the question that opens this section. Its decision population,
`decision-20261101T101751Z`, was collected at commit `941ff07d` on one pinned
CPU and ended on 2026-11-03. It has 21 cells, each with one build, two verify
runs and 3 warm-up and 31 decision time processes. A process runs
$`W = 4`$ warm-up commits and $`T = 32`$ timed commits; the E cells run
$`W = 4`$, $`K = 8`$, one reclaim commit and $`T = 16`$.

| Axis | Measures | Keys $`N`$ | History $`H`$ | Per-commit change |
|---|---|---|---|---|
| U | current-state growth | $`2^{10}, 2^{12}, 2^{14}, 2^{16}, 2^{18}`$ | $`2^{10}`$ | replace 1 key |
| R | retained-history growth | $`2^{12}`$ | $`2^{8}, 2^{10}, 2^{12}, 2^{14}, 2^{16}`$ | replace 1 key |
| D | delta size | $`2^{14}`$ | $`2^{10}`$ | replace $`d \in \{1, 4, 16, 64, 256\}`$ keys |
| L | last-reference deletion | $`2^{14}`$, plus $`36m`$ reserve keys | $`2^{10}`$ | delete one aligned group of $`m \in \{1, 4, 16, 64, 256\}`$ keys |
| E | physical eviction | $`2^{10}, 2^{14}, 2^{18}`$ | $`2^{10}`$ | replace 1 key, with deferred, reclaim and timed commits |

The axes share two cells: U at $`2^{12}`$ keys is R at $`2^{10}`$ states, and
U at $`2^{14}`$ keys is D at $`d = 1`$. The first figure below shows what one
trusted commit touches and where the bytes that G4 measures are copied. The
second shows the experiment as it ran.

![The work of one trusted commit. Sealing the mutation, binding it through the current, retained and metadata ledger trials, preparing it and publishing it touch only the objects, references and records that the commit changes, not the N keys or the H retained states; the bytes copied per touched ledger node do grow with the ledger, the cost that gate G4 found](figures/fixed-delta-commit-work.svg)

![The fixed-delta scaling experiment as it ran: the preregistration before any sample, the pilots, which never decide, the four negative controls, each its own population and run first, the decision population of 21 cells, the separate diagnostics and the durable records; amendments may change only feasibility and order, never thresholds, metrics, arms, seeds or the analysis](figures/fixed-delta-scaling-protocol.svg)

The committed [measurement record](../stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03.md) defines every column and key and
derives its four data files from the pgmcp archives by a program that it
carries: the [process summary](../stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03-process-summary.tsv), the
[verify records](../stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03-verify.tsv), the [analysis](../stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03-analysis.json) and the
[artifact manifest](../stack-safety/measurements/rspace-fixed-delta-scaling-2026-11-03-manifest.json), which pins the sha256 of every archived
population. Unless a row says otherwise, the values below come from the
analysis, and each exponent is its estimate with the 95% interval in brackets.

**The decision.** Result 483 of experiment 446, decided on 2026-11-08, accepts
its one hypothesis, H1: the statistic, the larger upper 95% bound of the two
thread CPU exponents, is 0.2086 against the threshold 0.25. The verdict covers
H1 only. The task's preregistered rule also requires a qualified decision
population, G1 to G9 and every control detected. The population is qualified,
with no retained failure, and all four controls were detected, but gate G4
fails on allocated bytes, so the rule is not met: the analysis reports
`qualified` as `true` and `accepted` as `false`.

| Gate | Verdict | Measurements |
|---|---|---|
| H1 | pass | Thread CPU exponent. U 0.1964 [0.1881, 0.2086]; R 0.1363 [0.1333, 0.1395]. Bound 0.25. |
| G1 | pass | 2 verify runs per cell, 0 problems. Exact work held at every verify commit. |
| G2 | pass | The two verify runs agree on all 27 compared fields; 0 time-chain mismatches. |
| G3 | pass | Wall time exponent. U 0.1586 [0.1508, 0.1699]; R 0.1098 [0.1059, 0.1138]. Bound 0.25. |
| G4, allocation count | pass | U 0.1091 [0.1050, 0.1130]; R 0.0529 [0.0479, 0.0576]. Bound 0.25. Identical in both verify runs. |
| G4, allocated bytes | **fail** | U 0.4130 [0.4086, 0.4174]; R 0.3186 [0.3137, 0.3234]. Both upper bounds exceed 0.25. Identical in both verify runs. |
| G5 | pass | Exponents in verify run 1: `syscr` U 0.0366 [0.0366, 0.0366], R 0; `syscw` 0 on U and R; `rchar` U 0.0500 [0.0423, 0.0575], R 0.0291 [0.0212, 0.0368]; `wchar` U 0.0437 [0.0381, 0.0492], R 0.0229 [0.0170, 0.0286]. The largest upper bound over both runs is 0.0575, against 0.25. The pack and index growth equal the encoded appends. |
| G6 | pass | `rss_end` minus `rss_start`, largest stratum's median minus the smallest's, with its upper 97.5% bound, at most 1 MiB. U: $`-315{,}392`$ B, bound $`-151{,}552`$ B. R: 700,416 B, bound 757,760 B. No swap and no out-of-memory kill. |
| G7 | pass | 0 problems: every V2 commit wrote at most 518,500 bytes with $`2 + \mathit{meta\_exp} \le 103`$, and the metadata objects equal the writer model's bytes. The NC-V1 control checks the V1 manifest law. The crossover is the table below. |
| G8 | pass | Thread CPU exponent. D 0.6527 [0.6493, 0.6564]; L 0.2772 [0.2746, 0.2801]. Bound 1.25. Every L commit deleted a nonempty closure equal to its excess current expansions. |
| G9 | pass | Each verify run has one reclaim commit, only it unlinks files, they are the superseded packs, one index and `reclaim.v1`, and the effects are trusted. No threshold applies; the differences are the table below. |
| G10 | not applicable here | Each control is its own population with its own G10, the control table below. |

Among the G4 and G5 exponents, only `rchar` on axis U differs between the two
verify runs, by less than 0.0002. A value printed as 0 is floating-point noise
below $`3 \times 10^{-16}`$. For scale, the median over the 31 time processes of
a commit's thread CPU time rises from 433,658.5 ns at $`N = 2^{10}`$ to 1,131,847 ns at $`N = 2^{18}`$, and from
471,453 ns at $`H = 2^{8}`$ to 916,373 ns at $`H = 2^{16}`$.

Per R stratum, over its timed verify commits, this table compares V2's median
metadata bytes with the V1 manifest of the same commits,
$`96 + 56(H + c + 1)`$ bytes at commit $`c`$. The crossover is the smallest
retained-entry count whose V1 manifest exceeds the measured V2 median.

| $`H`$ | Median V2 metadata bytes | Median V1 manifest bytes | V1 over V2 | Crossover entries |
|---|---|---|---|---|
| $`2^{8}`$ | 1,538 | 15,580 | 10.13 | 26 |
| $`2^{10}`$ | 1,661 | 58,588 | 35.27 | 28 |
| $`2^{12}`$ | 2,392 | 230,620 | 96.41 | 42 |
| $`2^{14}`$ | 4,088 | 918,748 | 224.74 | 72 |
| $`2^{16}`$ | 1,538 | 3,671,260 | 2,387.04 | 26 |

These are the medians at the measured history sizes, so the crossover at 26 to
72 entries sits below the derived worst case of the writer section (about 184
entries), which assumes a full last inner node.

Gate G9's within-process thread CPU differences, in microseconds, are medians
over 31 processes with 95% bootstrap intervals:

| E cell keys | Deferred minus timed | Reclaim minus timed |
|---|---|---|
| $`2^{10}`$ | 7.4 [4.2, 11.2] | 181.1 [175.8, 185.2] |
| $`2^{14}`$ | 5.2 [$`-0.3`$, 10.5] | 571.2 [547.0, 593.8] |
| $`2^{18}`$ | 14.8 [$`-8.4`$, 46.3] | 7,827.3 [7,375.5, 8,134.4] |

$`c^*`$ is 1.4142 ns per key on U (the grid point $`j = 2`$), which is 32.75%
of the largest stratum's median of 1,131,847 ns, and 16 ns per state on R (the
grid point $`j = 16`$), which is 114.43% of the median of 916,373 ns.

**The negative controls.** Each control is its own population, collected before
the decision population with five time processes of eight timed commits on the
smallest, middle and largest strata. All four are qualified and detected.

| Control | Mutation | Population (pgmcp artifact) | Expected signal, as measured |
|---|---|---|---|
| NC-SCAN | before `authorized_roots`, the trial loads every predecessor ledger entry's object (`ledger_trial.rs:129`) | `controls-nc-scan-20261031T232114Z` (1625) | CPU: U 0.845 [0.841, 0.849], R 0.597 [0.572, 0.613]. `rchar`: U 0.831 [0.830, 0.833], R 0.555 [0.554, 0.555]. G1 still passes, which is why the timing and I/O gates exist. |
| NC-CLONE | the $`O(1)`$ ledger map clone becomes an entry-by-entry rebuild (`ledger_trial.rs:135`) | `controls-nc-clone-20261101T014638Z` (1626) | CPU: U 0.835 [0.826, 0.852], R 0.653 [0.648, 0.658]. `alloc_count`: U 0.836 [0.836, 0.837], R 0.628 [0.628, 0.629]. `alloc_bytes`: U 0.799 [0.798, 0.800], R 0.596 [0.596, 0.597]. |
| NC-ORDER | the deactivation phase runs before the activation phase (`ledger_trial.rs:142-143` swapped) | `controls-nc-order-20261101T065818Z` (1630) | G1 fails at every cell. CPU: U 0.971 [0.968, 0.974]. SW-01, run against the mutant export, fails (pgmcp artifact 1253). |
| NC-V1 | committed code, the V1 writer (`admission.rs:1388`), a natural control | `controls-nc-v1-20261101T092116Z` (1631) | On R, metadata bytes equal $`96 + 56n`$. CPU: R 0.666 [0.662, 0.671]. |

**The missed G4 prediction.** G4 predicted an exponent of at most 0.25 for the
bytes allocated per commit, and the pilot had already forecast the miss: the
control-pilot addendum (pgmcp artifact 1178, observation O2) reported a pilot
interval of [0.380, 0.390] on U. Hypothesis H-O2 (pgmcp progress notes 25705
and 25707) is that the bytes come from copy-on-write copies of the
widening 256-way level-1 nodes of the ledger's PathMap. `run_ledger_trial`
clones the predecessor's `PathMap<DigestState>` and inserts one 56-byte state
for each touched reference (`ledger_trial.rs:135` and `:335`). The first
insertion under a shared node copies that node's whole slot vector, and one
slot, an `OrdinaryCoFree` of pathmap 0.3.4 with its optional inline
`DigestState`, is 72 bytes. For a ledger of $`\ell`$ digests a level-1 node
holds about $`\min(\ell / 256, 256)`$ slots, so copying one costs about
$`72 \min(\ell / 256, 256)`$ bytes. The model matched the pilot's measured bind
bytes to within 15%. The section 5.9 diagnostics, run on the pilot's fixtures,
confirm the mechanism on axis U: heaptrack attributes 93.2%, 97.9% and 99.41%
of the bind bytes at $`2^{10}`$, $`2^{14}`$ and $`2^{18}`$ keys to
`run_ledger_trial`, and its bind totals equal the verify run's counter exactly
(pgmcp artifacts 1277 and 1281). The refined width model's predictions,
registered before the largest trace was read, hold. Heaptrack ran on axis U
only, so axis R has the model's prediction and no attribution. The heaptrack,
strace and uProf outputs are pgmcp artifact 1303, and the strace and uProf
outcome is artifact 1304. Allocation counts stay logarithmic
and thread CPU and wall time pass, so this is a cost in copied bytes per touched
node: memory bandwidth and allocator pressure that grow with the ledger, not
extra allocations and not a correctness failure. The defect is bug item
`rspace-ledger-trial-cow-allocation-growth` (pgmcp item 13988), which holds the
evidence and the consensus-change assessment. The measurement records it and
does not redesign the ledger.

**What the measurement does not claim.**

- It holds for one host and one core.
- The binary is the release library test binary, which still contains the
  `cfg(test)` hooks and a toggled counting allocator, not a production build.
- The keys are data-only: each is a datum on its own channel.
- The history shares the current roots: each historical state differs from the
  current one only by its legacy root (the fixture above).
- fsync was measured on one filesystem, ext4.
- It makes no power-loss claim and no concurrency claim.
- It is not the LMDB verdict (task 5693). It measures no semantic eviction,
  because no eviction policy exists under keep-all retention, and axis E
  reclaims only physical files. It activates nothing (task 7931).
- $`c^*`$ bounds only what the timing gate excludes by itself: an extra cost per
  key or per state below $`c^*`$ would pass H1.

### Fallible legacy state-authority facets (2026-11-01)

Task 7931 wires the durable PathMap DAG owner into RSpace as a third
state-authority backend, beside the in-memory and LMDB backends. The authority
hands out three legacy facets, which are views of its objects in the legacy
byte layout: the history facet (radix history nodes keyed by digest), the cold
facet (cold leaves keyed by physical address) and the roots facet (the
verified-root catalog, read-only). They come from
`RSpaceStateAuthority::history_store`, `cold_store` and
`roots_store_for_inspection` in
[`rspace_state_authority.rs`](../../../rspace++/src/rspace/state/rspace_state_authority.rs).
A durable backend does not keep that layout as its authority, so it may be
unable to serve a facet. Until this slice the accessors returned a bare
`Arc<dyn KeyValueStore>`, so the only ways to fail were a panic or a
fabricated view.

This slice, the fifth leaf of the operation gate, makes the three accessors
fallible. Each now returns
`Result<Arc<dyn KeyValueStore>, AuthorityBackendMismatch>`, where
`AuthorityBackendMismatch` names the unavailable facet: `History`, `Cold` or
`Roots`. All three call one private `legacy_facet(kind)`, whose `match` over the
backend names the in-memory and LMDB backends explicitly and returns the
existing view for both. No behavior, persisted byte, root or wire message
changes. When the durable backend is added, that `match` stops compiling until
the new arm states which facets the backend serves, so a missing facet is a
decision recorded in the code rather than an accident.

Each caller passes the refusal on with `?` into the error type it already
returns. There is one conversion per error type, and no call site maps the
mismatch itself, so these conversions are the only places where the facet that
a refusal names could be lost:

| Caller's error type | Callers | Conversion |
|---|---|---|
| `HistoryError` | `commit_local_checkpoint`, `RadixHistory::create_authorized` | the new variant `HistoryError::BackendMismatch` |
| `HistoryRepositoryError` | `HistoryRepositoryInstances::repository` | `HistoryRepositoryError::HistoryError` around the `HistoryError` conversion |
| `StateExportError` | the exporter's `get_page`, `get_history_items` and `get_data_items` | the new variant `StateExportError::BackendMismatch` |
| `StateImportError` | `seal_import`, and the importer's `begin_import` and `get_history_item` | the new variant `StateImportError::BackendMismatch` |
| `heed::Error` | `store` of the in-memory, LMDB and LMDB-directory store managers | `heed::Error::Io` holding an I/O error of kind `Other` whose source is the mismatch itself, so a caller can recover it by downcasting |

The exporter and the importer take their facets when they are built, and their
constructors, `RSpaceExporterStore::create` and `RSpaceImporterStore::create`,
return the store itself rather than a `Result`. Both are public, and a Casper
test builds an exporter through one, so instead of changing their signatures,
each store keeps the facet's `Result` and refuses at the first operation that
needs the facet. The exporter refuses every page and history request while the
history facet is unavailable, and every data request while the cold facet is.
The importer refuses `begin_import` while either facet is unavailable, before
it stages anything, and `get_history_item` while the history facet is
unavailable. An empty request is refused too, so a refusal never depends on
what was asked for.
`every_export_operation_refuses_an_unavailable_legacy_facet` and
`every_import_operation_refuses_an_unavailable_legacy_facet` pin this wiring,
and the second also checks that a refused import leaves the current root
unchanged. They test the stores' wiring rather than a property, so they are not
registered.

**The OG rows.** OG is the operation gate's family. Its numbers come from the
gate's property plan, so rows are added in the order in which their leaves
land, not in numeric order.

| ID | Required property | Test |
|---|---|---|
| OG-12 | Typed refusal of an unavailable legacy facet. For each of the three facets, the mismatch converts into `HistoryError`, into `HistoryRepositoryError` (directly and through `HistoryError`), into `StateExportError` and into `StateImportError` as the `BackendMismatch` variant holding the same facet, and each converted message contains the mismatch's own. It converts into `heed::Error::Io` holding an I/O error of kind `Other` whose message equals the mismatch's and whose source downcasts to the same mismatch. The in-memory authority and an initialized LMDB authority still serve all three facets: the history and cold facets read as empty maps, and the roots facet holds the current-root record. | `legacy_facet_mismatch_conversions_keep_their_typed_source` |

### Fallible RSpace constructors (2026-11-01)

`RSpace::create` and `RSpace::create_with_replay` in
[`rspace.rs`](../../../rspace++/src/rspace/rspace.rs) build a space over an
`RSpaceStore`, which wraps the state authority. Both return
`Result<_, HistoryRepositoryError>`, and both begin with `create_history_repo`,
which opens the history repository over the authority and builds the hot store
from the history at the current root. Until this slice each constructor called
`.unwrap()` on that result, so a store that the repository refused made the
caller panic, although the signature promised a `Result`. The replay reader that
`create_with_replay` opens next already passed its refusal on with `?`.

This slice, the fourth leaf of the operation gate, replaces both `.unwrap()`
calls with `?`. Every refusal of `create_history_repo` now reaches the caller as
the `HistoryRepositoryError` it already was, with its variant and message
intact. The signatures do not change, and a store that the repository accepts
builds the same space as before, so no successful construction, persisted byte,
root or wire message changes. The durable backend that task 7931 adds will
refuse some stores with a typed error when the repository is opened, and a
constructor that panics would turn each such refusal into a process abort.

The refusals that now reach the caller are those of the steps of
`create_history_repo`:

| Step | What it refuses | Arrives as |
|---|---|---|
| `RSpaceStateAuthority::initialize_for_records` | an LMDB legacy state it cannot complete, or a current-root record it cannot decode | `HistoryRepositoryError::RootError` |
| `RSpaceStateAuthority::current_root` | an authority with no current root | `HistoryRepositoryError::RootError` |
| `HistoryInstances::create_authorized` | a current root that is not verified, an unavailable history facet, or a root node it cannot load | `HistoryRepositoryError::HistoryError` |
| `RSpaceStateAuthority::cold_store` | an unavailable cold facet (OG-12) | `HistoryRepositoryError::HistoryError` holding `HistoryError::BackendMismatch` |
| `HistoryRepository::get_history_reader` | a history reader it cannot open at the current root | `HistoryRepositoryError::HistoryError` |

`rspace_constructors_propagate_a_failing_store_as_an_error` builds each space
over an in-memory authority whose current-root record is the 17 bytes
`malformed-current`. A current-root record is 61 bytes: the 21-byte versioned
prefix `rspace-current-root\0\x01`, an 8-byte generation and a 32-byte root.
`initialize_for_records` refuses the 17-byte record because it has neither that
length nor that prefix. An in-memory store needs no files, so the refusal is
deterministic, and the record is the one that
`lmdb_rejects_malformed_current_and_membership_records` writes into an LMDB
authority. Both constructors must return
`HistoryRepositoryError::RootError(RootError::CorruptRootCatalog(..))` with the
message `current-root value has an unknown version or invalid length`. With
either `.unwrap()` restored, the test panics instead.

**The OG row.**

| ID | Required property | Test |
|---|---|---|
| OG-39 | Constructor refusal. Over a store whose history repository cannot be opened, `RSpace::create` and `RSpace::create_with_replay` each return `Err` holding the repository's own error instead of panicking. For an in-memory authority whose current-root record is malformed, that error is `HistoryRepositoryError::RootError(RootError::CorruptRootCatalog(..))` with the message `current-root value has an unknown version or invalid length`. | `rspace_constructors_propagate_a_failing_store_as_an_error` |

### Generation-leased durable read handle (2026-11-01)

Task 7931 reads a durable generation lazily, one channel at a time, while the
owner goes on publishing successors. The view seam that provides those reads
is task N2. Before this slice the only reader of an admitted generation was
`DurableAuthorityViewV1` in
[`mod.rs`](../../../rspace++/src/rspace/persistence/durable/mod.rs), and
`into_rspace_authority` consumes the view. Once the owner took over, nothing
could read the generation that the view had admitted.

This slice, the first leaf of the view seam, separates reading from owning.
The admitted checkpoint source in
[`admission.rs`](../../../rspace++/src/rspace/persistence/durable/admission.rs)
now keeps its `DurableAuthorityCoreV1` in an `Arc`. The core holds, among
other things:

- the pinned object index;
- open handles to every pack that the catalog names;
- the catalog authority token;
- the semantic validity token.

The view, the owner and any number of read handles share this one core, and
nothing is copied. Publication still borrows the core as
`&DurableAuthorityCoreV1`, so it is unchanged. The owner's `source` field and
its pending successor are `Arc`s too. The successor that publication returns
is wrapped once, when the owner records it as pending acknowledgement.

`DurableAuthorityViewV1::read_handle` returns a `DurableViewReadHandleV1`. The
handle holds a clone of the `Arc` and its own `GenerationLeaseV1` for the
view's exact catalog generation, and it releases the lease when it is
dropped. The two parts do different jobs:

| Part | What it keeps | Who relies on it |
|---|---|---|
| The shared core | The pinned index and the open pack handles of one generation, in this process. | The handle's own reads. |
| The generation lease | A registry entry that names the generation's pack range. | Reclamation. `reclaim_reclamation_candidate` deletes a superseded generation's packs and index only after `retire_pack_range_if_unleased` finds no lease that overlaps their range. That one registry operation also refuses every later lease on the range. |

The `Arc` alone therefore keeps the core alive, but only the lease keeps its
generation from being reclaimed. Without the lease, reclamation could delete
the generation's files, and no later reader could lease the generation again.

`read_handle` also carries a compile-time assertion that the handle is
`Send + Sync + 'static` for every semantic profile. A handle can therefore
move to another thread or task and outlive both the view and the owner.

None of the remaining changes alters behavior:

- The accessors that now dereference the `Arc` are no longer `const fn`:
  - the admitted source's `core`, `shape`, `authority`,
    `retention_manifest_bytes`, `retention_format` and `retained_closure`;
  - the admitted writer's `catalog`, `shape`, `authority`,
    `retention_manifest_bytes` and `retained_closure`;
  - the view's `catalog`.
- `into_core` returns the `Arc`. Five test modules that kept an owned core
  now keep the `Arc`, and wrap the successor that publication returns:
  `shape_summary_properties`, `trusted_history_production`,
  `trusted_scaling_properties`, `trusted_scaling_resource` and
  `trusted_successor_production`.
- The lease registry exposes no count of live leases. It gains a test-only
  `active_lease_count_for_test`.

No persisted byte, root or wire message changes.

**Relation to the caller registry.** VR-01 is the first executable witness,
through the durable view, of two capstones of the caller-authority contract
below:

- CA-21, `pinned_generation_cannot_be_reclaimed`;
- CA-22, `owner_teardown_preserves_retained_readers`.

Its derivation row is nevertheless executable-only, like the OG rows. Listing
`RSpaceCallerAuthority.v` among the manifest test's Rocq sources would oblige
a derivation row here for every capstone of that file, and those capstones
belong to the caller registry, not to this matrix.

`read_handle_pins_its_generation_across_a_successor_and_owner_teardown`
proceeds as follows:

1. It recovers a view of an installed checkpoint image and takes a handle
   before `into_rspace_authority`.
2. The owner then publishes and acknowledges a successor that adds a datum, a
   continuation and a join.
3. The owner is dropped, and a second recovery reads the successor's roots.
4. Reclamation's registry decision is then made for the old generation.

It requires all of the following:

- the handle adds exactly one lease;
- the handle reads the old semantic roots both after the successor and after
  teardown, and those roots differ from the successor's;
- the old generation's exact lease key stays active after the owner's own
  lease has moved to the successor;
- reclamation refuses the old pack range;
- the handle's lease is the only one left, and none is left after the handle
  is dropped.

The red control is a handle whose lease is released as soon as it is
acquired. That handle leaves the lease count unchanged, loses the old key and
lets the range be retired. One assertion compares the three observations, so
the control's failure reports all of them.

**The VR row.** VR is the view seam's family.

| ID | Required property | Test |
|---|---|---|
| VR-01 | Generation-leased read handle. A handle taken from a durable view before `into_rspace_authority` adds exactly one lease to the directory's lease registry. After the owner publishes and acknowledges a successor, the handle still reads its own generation's semantic roots, which differ from the successor's, and that generation's exact lease key is still active although the owner's lease has moved to the successor. After the owner is torn down, `retire_pack_range_if_unleased` refuses the generation's pack range, and the handle's lease is the only one left. Once the handle is dropped, none is left. | `read_handle_pins_its_generation_across_a_successor_and_owner_teardown` |

### Durable owner incarnations and close (2026-11-01)

Task 7931 will let several RSpace views share one durable owner, and it must
be able to close that owner and open another on the same directory. The view
extension of
[`RSpaceJournalPublicationOwner.tla`](../../../formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla)
states what close and reopen must guarantee. Before this slice the owner in
[`rspace_authority.rs`](../../../rspace++/src/rspace/persistence/durable/rspace_authority.rs)
had no identity and no close: it lived until it was dropped. A view could
remember only the owner's base, the catalog slot the owner was current at.
After a close and a reopen with no publication in between, the new owner is
current at the same slot, so the base cannot tell a view of the closed owner
from a view of its successor.

This slice, leaf G07 of task 7931, adds three things.

- **Incarnation.** An `OwnerIncarnationV1` is a `u64` that names one owner
  for its whole life. `OwnerIncarnationAllocatorV1::allocate` advances an
  `AtomicU64` with `fetch_update` and `checked_add(1)`, and returns the value
  it replaced. An allocator that starts at $`n`$ therefore hands out
  $`n, n+1, \ldots, 2^{64}-2`$, each once and in increasing order. Once the
  counter holds $`2^{64}-1`$, every later call returns the typed refusal
  `OwnerIncarnationExhaustedV1`, and the counter never wraps.
  - `from_view` draws from `OWNER_INCARNATIONS`, one allocator that every
    owner in the process shares and that starts at 0. `from_view_with` takes
    the allocator as an argument, so a test can exhaust an allocator of its
    own.
  - When the allocator refuses, the constructor returns its view with
    `DurableRSpaceTransactionError::IncarnationExhausted`, as it already
    returns the view with its other refusal. `into_rspace_authority`
    therefore reports `DurableRSpaceTransactionError` instead of
    `RSpaceError`, and a journal-authority refusal arrives as its `Journal`
    variant.
- **Attachment.** `attach` returns a `ViewAttachmentV1 { incarnation, base }`
  from a live owner that has no pending publication and whose journal base is
  valid. `validate_attachment` checks an attachment in the model's order,
  given in the table below.
- **Close.** `close` refuses with `Unresolved(phase)` while a publication is
  pending, which is the model's `Quiescent` precondition. Otherwise it
  replaces the owner's live state with `Closed`, which keeps only the
  generation key. Replacing the live state drops three things:
  - the incarnation;
  - the writer lease, which releases the directory's writer lock;
  - the owner's own generation pin.

  A closed owner then refuses every operation:
  - `attach`, `state`, `checkpoint` and `reconcile` return
    `DurableRSpaceTransactionError::Closed`, and so does a second `close`;
  - `phase` reports the new `JournalPublicationPhaseV1::Closed`;
  - `validate_attachment` refuses every attachment as `OwnerClosed`.

  The hot store keeps its journals, but the owner is its only route to
  publication, so nothing that a closed owner's hot store holds can be
  published. Reopening is not a method. It is recovering a new view from the
  directory and constructing a new owner, which draws a fresh incarnation.

The owner's life, and what three close and reopen cycles leave behind:

```text
recovered view ──▶ from_view ──▶ ┌──────────────────┐       close        ┌───────────────────────┐
                   allocates i   │ Live             │ ─────────────────▶ │ Closed                │
                   from the      │  incarnation i   │ only when no       │  generation key only  │
                   shared        │  writer lease    │ publication is     │  writer lock free     │
                   counter       │  generation pin  │ pending            │  own pin released     │
                                 └────────┬─────────┘                    │  read handles pinned  │
                                          │ attach                       └───────────────────────┘
                                          ▼
                                      (i, base)

owner 0, incarnation i0       attaches a0 = (i0, b)
   └─ close, recover, reopen
owner 1, incarnation i1 > i0  attaches a1 = (i1, b)    refuses a0          as StaleIncarnation
   └─ close, recover, reopen
owner 2, incarnation i2 > i1  attaches a2 = (i2, b)    refuses a0, a1      as StaleIncarnation
   └─ close, recover, reopen
owner 3, incarnation i3 > i2  attaches a3 = (i3, b)    refuses a0, a1, a2  as StaleIncarnation
```

`validate_attachment` decides in this order:

| Order | Check | Refusal when it fails |
|---|---|---|
| 1 | The owner is live. | `OwnerClosed` |
| 2 | The attachment's incarnation is the owner's. | `StaleIncarnation { attached, current }` |
| 3 | The attachment's base is the owner's current catalog slot. | `StaleBase { attached, current }` |

The model's `CheckRequest` checks the caller's permit first, then
`incarnationOk`, then `baseOk`. The permit check belongs to the slice that
passes the attachment and the permit into the checkpoint path; this slice has
no permit to check. Nothing in production calls `attach` or
`validate_attachment` yet.

**Relation to the model.**

| Model | This slice |
|---|---|
| `CloseOwner` requires `Quiescent`, sets `live` to false and increments the incarnation. | `close` refuses while a publication is pending, and replaces the live state, incarnation included, with `Closed`. |
| `ReopenOwner` takes the incarnation that `FreshIncarnation` allows: one more than the value `CloseOwner` left. | A new owner draws the next value of the shared counter. That value is greater than every incarnation an earlier owner held in the process, so it differs from each of them, which is all `incarnationOk` needs. It need not be exactly one more, because every owner in the process draws from the same counter. |
| `CheckRequest` returns `refused-incarnation` before `refused-base`. | `validate_attachment` returns `StaleIncarnation` before `StaleBase`. |
| `StaleIncarnationRefused`: a caller that publishes, acknowledges or applies holds an attachment from the live owner's own session. The mutants `CloseNoBump` and `ReopenReusesIncarnation` violate it. | OG-06 and OG-07, and the red control that gives each owner a counter of its own. |
| The witness `NeverRefusedRetainedIncarnation` is violated: a live owner refuses an attachment kept from an earlier session. | OG-07 observes that refusal in each of its three cycles. |

**Relation to the caller registry.** Close is a teardown of the owner without
a drop. CA-22, `owner_teardown_preserves_retained_readers`, requires a
teardown to revoke the owner's mutation permits and to leave retained readers
pinned. Close drops the writer lease, which is the owner's only mutation
permit, and the owner's own generation pin, and it touches no read handle.
Each handle holds its own lease on its generation and its own reference to the
shared core, as the generation-leased durable read handle section above
describes. OG-08 observes both halves. Its derivation row is executable-only
for the reason that VR-01's is.

**The tests.** All five tests are in
[`owner_incarnation.rs`](../../../rspace++/tests/persistence/owner_incarnation.rs).
Each recovers a view of an installed checkpoint image and opens owners on it.

- `close_revokes_old_attachments_without_mutation` (OG-06) attaches to an
  owner, populates its hot store with a datum, a continuation and a join, and
  records the state, the three bag journals and the store files. Before close,
  the attachment is accepted and the writer lock is held. After close it
  requires all of the following:
  - the attachment is refused as `OwnerClosed`;
  - `attach`, `checkpoint_view`, `reconcile`, `state` and a second `close`
    each return `Closed`, and `phase` is `Closed`;
  - the view gate is free, the journals and the store files are unchanged, the
    writer lock can be taken, and a fresh recovery reads the recorded state.
- `reopened_owners_refuse_every_earlier_incarnation` (OG-07) runs three cycles
  of close, recover and reopen, attaches to each new owner, and keeps every
  closed owner alive. After cycle $`k`$, the new owner must accept its own
  attachment and refuse each of the $`k`$ earlier ones as `StaleIncarnation`,
  naming that attachment's incarnation and its own. The four incarnations must
  increase, and the four attachments must name the same base, so only the
  incarnation separates them.
- `pinned_reads_survive_owner_close_until_the_last_reader_releases` (OG-08)
  takes a read handle before `into_rspace_authority`, then closes the owner.
  It requires all of the following:
  - two leases before close, the owner's and the handle's, and one after;
  - the writer lock can be taken while the handle is alive;
  - the handle still reads the generation's semantic roots;
  - the generation's exact key is still active, and
    `retire_pack_range_if_unleased` refuses its pack range;
  - once the handle is dropped, no lease is left and the range can be retired.
- `concurrent_incarnation_allocation_is_unique_monotonic_and_never_wraps`
  starts an allocator 8,192 values below $`2^{64}-1`$ and lets eight threads
  allocate until each is refused. The values granted must be exactly those
  8,192, each thread's values must increase, and later calls must still be
  refused. A new allocator must grant 0, then 1.
- `an_exhausted_allocator_refuses_the_owner_and_returns_its_view` constructs an
  owner with an allocator that starts at $`2^{64}-1`$. It requires the refusal
  `IncarnationExhausted`, a returned view at the original state, and an owner
  opened from that view at the same state.

The last two tests are bound to no ID. They check the allocator, and no model
action describes it.

**Red controls.** Four production controls are each applied to an export of
the candidate tree. Each must make exactly one test of this file fail, and no
durable persistence test:

- `from_view` with a counter of its own that starts at 0 gives every owner
  incarnation 0. The earlier attachments at the unchanged base are then
  accepted, and OG-07 fails.
- `validate_attachment` accepting every attachment on a closed owner makes
  OG-06 fail.
- `close` leaking one lease on its generation makes OG-08 fail.
- `checkpoint` without its closed-owner check makes OG-06 fail.

One assertion compares all the observations of a test, so each control's
failure reports every one of them.

**The OG rows.**

| ID | Required property | Test |
|---|---|---|
| OG-06 | Close revocation. Closing a quiescent durable owner refuses its earlier attachment as `OwnerClosed`, and `attach`, `checkpoint_view`, `reconcile`, `state` and a second `close` each return `Closed`, with `phase` reporting `Closed`. Close leaves the view gate free and the writer lock available, changes neither the hot store's journals nor the store files, and a fresh recovery reads the state recorded before close. | `close_revokes_old_attachments_without_mutation` |
| OG-07 | Fresh incarnation on reopen. Over three cycles of close, recovery and reopen at an unchanged base, each new owner accepts only its own attachment and refuses every earlier one as `StaleIncarnation` naming both incarnations. The owners' incarnations increase. | `reopened_owners_refuse_every_earlier_incarnation` |
| OG-08 | Pinned reads across close. A read handle taken before `into_rspace_authority` survives the owner's close. Close leaves exactly the handle's lease and frees the writer lock, while the handle still reads its generation's semantic roots and keeps the generation's exact key active, so `retire_pack_range_if_unleased` refuses its pack range. Once the handle is dropped, no lease is left and the range can be retired. | `pinned_reads_survive_owner_close_until_the_last_reader_releases` |

No persisted byte, root, identity or wire message changes.

### Durable owner checkpoint views under caller-held permits (2026-11-01)

`RSpace::create_checkpoint` in
[`rspace.rs`](../../../rspace++/src/rspace/rspace.rs) holds its operation
gate's write permit while it checkpoints. Before this slice the durable owner
in
[`rspace_authority.rs`](../../../rspace++/src/rspace/persistence/durable/rspace_authority.rs)
kept that gate as a field, and its `checkpoint` took a write permit of the
same gate itself, with `write_owned().await`. A tokio `RwLock` is not
reentrant: the second write request waits for the first permit, which the
waiting caller holds, so a `create_checkpoint` that called the owner would
never return. Nothing calls the owner from `RSpace` yet, so the deadlock was
latent; task 7931 is the task that adds the call. The view extension of
[`RSpaceJournalPublicationOwner.tla`](../../../formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla)
reproduces it. Its fault `retake-gate` makes the owner take the view gate
again while it holds the owner mutex. `RetakeGateMutant` reports the
violation of `LockOrder`, one state before the deadlock, and
`RetakeGateDeadlockMutant`, which checks no invariant, reports the deadlock.

This slice, leaf G08 of task 7931, replaces `checkpoint(limits)` with
`checkpoint_view(permit, attachment, gate, hot, limits)`. The caller takes the
permit and passes it in, and the owner takes no lock.

- **Arguments.**
  - `permit`, an `OwnedRwLockWriteGuard<()>` taken by value, is a write permit
    of the caller's view gate.
  - `attachment` is the `ViewAttachmentV1` that the caller obtained from
    `attach`.
  - `gate` is the caller's view gate, an `Arc<tokio::sync::RwLock<()>>`.
  - `hot` is the caller's hot store, whose journal the checkpoint captures.
  - `limits` are the publication limits, as before.
- **The owner keeps neither the gate nor the hot store.** Its fields are now
  its lifecycle, the generation-lease registry, its durable core and its
  pending publication. `from_view` and `from_view_with` take the hot store by
  reference only to validate its journal base, and `attach`, `state` and
  `phase` take the caller's hot store. The pending record keeps the permit and
  a reference to the store that captured, so `reconcile` and the
  acknowledgement act on that store, and `reconcile` keeps its signature.
- **Two refusals.** `DurableRSpaceTransactionError` gains `ForeignPermit` and
  `Attachment(ViewAttachmentRefusalV1)`. Converting a
  `ViewAttachmentRefusalV1` maps `OwnerClosed` to the existing `Closed` and
  the other two refusals to `Attachment`, so `checkpoint_view`, given a permit
  of its gate, refuses a closed owner with `Closed`, as every other operation
  does.

`checkpoint_view` decides in this order, before it captures anything:

| Order | Check | Refusal when it fails |
|---|---|---|
| 1 | The permit was issued by `gate`: `Arc::ptr_eq(OwnedRwLockWriteGuard::rwlock(&permit), gate)`. | `ForeignPermit` |
| 2 | The owner is live. | `Closed` |
| 3 | The attachment's incarnation is the owner's. | `Attachment(StaleIncarnation { attached, current })` |
| 4 | The attachment's base is the owner's current catalog slot. | `Attachment(StaleBase { attached, current })` |
| 5 | No publication is pending. | `Unresolved(phase)` |
| 6 | The hot store's journal names the owner's authority. | `Journal(..)` |

Checks 2 to 4 are `validate_attachment`, unchanged since the incarnation slice
above, and check 6 is the base validation that `checkpoint` already made. A
refusal drops the permit when the call returns, so the caller's gate is free
again, and nothing has been captured. Once every check passes, the capture
moves the permit into the pending record, and from there:

- an empty journal, or a refusal before publication begins, drops the record
  and the permit with it, as before;
- a publication whose outcome is unknown keeps the record in
  `RecoveryRequired`, and the permit inside it keeps the gate held until
  `reconcile` resolves the outcome, which is the base model's
  `GateHeldUntilResolution`;
- a successful publication is acknowledged in the same call:
  `acknowledge_successor` adopts the successor and pins its generation, and
  only then drops the permit.

The permit is therefore held from the call until acknowledgement or refusal,
and, when the outcome is unknown, until reconciliation. The caller protocol
of this slice, and the owner's decisions:

```text
caller of view v
  1. permit ← gate_v.write_owned().await         the view gate first
  2. owner  ← owner_mutex.lock().await            the owner mutex second
  3. a      ← owner.attach(&hot)
  4. owner.checkpoint_view(permit, a, &gate_v, &hot, limits)
        │
        ├─ permit issued by gate_v? ────── no ──▶ ForeignPermit
        ├─ owner live? ─────────────────── no ──▶ Closed
        ├─ incarnation current? ────────── no ──▶ Attachment(StaleIncarnation)
        ├─ base current? ───────────────── no ──▶ Attachment(StaleBase)
        ├─ nothing pending? ────────────── no ──▶ Unresolved(phase)
        ├─ journal names the authority? ── no ──▶ Journal(..)
        │                                         each refusal drops the permit
        ▼
     capture: the pending record takes the permit and the capturing store
        │
        ├─ empty journal, or refused before publication ──▶ record dropped, gate free
        ├─ outcome unknown ──▶ RecoveryRequired: record kept, gate held until reconcile
        ▼
     AwaitingAck ──▶ acknowledge_successor: successor adopted and pinned,
                     then the permit dropped ──▶ Ok(state), gate free
```

**A pending publication.** The owner no longer reconciles inside a
checkpoint. While a publication is pending, its record holds the permit of the
gate that captured it, so no caller of that gate can take a new permit, and
`checkpoint_view` refuses any other caller with `Unresolved(phase)`. A caller
resolves the publication first:

1. It locks the owner mutex, holding no gate, and calls `reconcile(limits)`,
   which needs no permit and acts on the store kept in the record. It takes no
   gate while it holds the mutex, so the lock order holds.
2. If reconciliation adopted the retained terminal, the journal is published
   and acknowledged, and the checkpoint is done. If it kept the predecessor,
   the journal is still in the hot store, and the caller checkpoints it again
   with a new permit and a new attachment.

That is what `checkpoint` did inside the owner. The tests that drove
`checkpoint` now follow this protocol through `checkpoint_with` in
[`durable_rspace_authority.rs`](../../../rspace++/tests/persistence/durable_rspace_authority.rs).
When the owner's phase is `Captured`, `RecoveryRequired` or `AwaitingAck`, the
helper records the retained terminal, reconciles, and returns the reconciled
state if it is the terminal's. Otherwise it takes a write permit with
`write_owned().await`, attaches and calls `checkpoint_view`. It returns early
exactly when `checkpoint` did:

- `checkpoint` returned early when the reconciled state differed from the
  state before reconciliation. That state was the predecessor's, because a
  pending owner adopts its successor only at acknowledgement.
- A successful `reconcile` of a pending publication returns one of two states:
  the predecessor's, when recovery finds the catalog unchanged, or the
  retained terminal's, once the terminal is acknowledged.
- The terminal's generation is the predecessor's plus one. A sealed
  transaction's target is the state at its mutation batch's target generation
  ([`transaction.rs`](../../../rspace++/src/rspace/persistence/transaction.rs)).
  `RSpaceMutationBatchV1::new` in
  [`mutation.rs`](../../../rspace++/src/rspace/persistence/mutation.rs) sets
  that generation to the base generation plus one, and
  `require_contiguous_generation` refuses every parsed batch whose target is
  another generation.
- The reconciled state therefore differs from the predecessor's exactly when
  it is the terminal's, so both forms return early in the same cases, and both
  return an error of `reconcile` unchanged.
- A closed owner holds no pending publication, because `close` refuses while
  one is pending. Its phase is `Closed`, so the helper does not reconcile, and
  `attach` refuses with `Closed`, the refusal that `checkpoint` returned
  first.

A checkpoint is now cancelled at the helper's `write_owned().await`, where its
caller waits, because the owner no longer waits anywhere.
`cancelling_a_checkpoint_waiting_for_the_exact_gate_does_not_capture_or_publish`
and the cancellation step of the owner histories cancel it there while a
reader holds the gate, and observe what they observed before: the first poll
is pending, nothing is captured or published, the store is unchanged, and the
journals keep their entries.

**Lock order.** The caller takes its view gate before the owner mutex, as
`CallCheckpoint` precedes `TakeMutex` in the model, and the owner takes
neither: `checkpoint_view` is `async` for its callers but contains no await
point. No path through the owner can therefore wait for a gate while the owner
mutex is held, which is `LockOrder`.

**Relation to the model.**

| Model | This slice |
|---|---|
| `CallCheckpoint(view, permit)` takes `viewGate[permit]` for the caller. | The caller takes `gate.write_owned()` and passes the guard by value. |
| `TakeMutex(view)`. Under the fault `retake-gate` the owner then takes the view gate again (`OwnerAcquiresGate`), which `LockOrder` forbids. | The caller holds the owner mutex, the `&mut self` of `checkpoint_view`, and the owner takes no gate. |
| `CheckRequest(view)` checks `permitOk`, then `incarnationOk`, then `baseOk`. `CloseOwner` increments the incarnation, so on a closed owner every attachment fails `incarnationOk`. | Checks 1, 3 and 4 in the model's order. Check 2 refuses a closed owner first, as `Closed`, which names the reason that `incarnationOk` fails there. |
| `CheckRequest` is enabled only when `pending = NoPending`; a view that holds the mutex over a pending record takes `ReconcileFirst` instead. | Check 5 refuses with `Unresolved(phase)`, and the caller reconciles first, as described above. |
| The capture sets `pending.permit` to the request's permit and marks `viewGate[permit]` as `"pending"`, so `ReturnCall` does not free it. | The pending record owns the guard, so returning from the call does not release the gate. |
| `PublishView(view, outcome)`: the outcome `new` enters `AwaitingAck` for the caller to acknowledge; any other outcome enters `RecoveryRequired` and reports `uncertain`, with the gate still marked. | `publish_captured`: a success enters `AwaitingAck`, and `checkpoint_view` acknowledges at once; an unknown outcome returns its error and keeps the record and its permit. |
| `AcknowledgeView(view, completes)` frees `viewGate[pending.permit]` and sets the view's attachment base to the new root; an incomplete acknowledgement keeps the gate. | `acknowledge_successor` adopts the successor and drops the permit last; a failed acknowledgement keeps the record and the permit. The attachment is a value that the caller holds, so the caller attaches again to obtain the new base. |
| `ReconcilePending`, with the mutex free, takes `Resolve`: a kept predecessor frees the gate and clears `pending`; a changed root enters `AwaitingAck`, whose resolution sets the attachment base and frees the gate. | `reconcile(limits)`, under the owner mutex alone: a kept predecessor drops the record and its permit; the retained terminal is acknowledged, which drops them once the successor is adopted. |
| `ReconcileFirst(view)`: a view that holds the mutex resolves the pending record first, even one captured through another view's gate; the witness `NeverReconciledByOtherView` reaches that case. | Not taken. With one gate per owner, the only configuration before the view layer of task 7931, a pending record holds the caller's own gate, so no caller reaches `ReconcileFirst`, as in the model with one view. Whether a view may resolve a publication captured through another view's gate is decided with that view layer; until then `checkpoint_view` refuses it with `Unresolved(phase)`, which the unbound test below pins. |
| `PermitProvenance`: a pending record holds its own view's permit. `ForeignPermitMutant` violates it. | Check 1 admits only a guard of `gate`, and the record keeps that guard. OG-09, and the red control without check 1. |
| `NoViewMutationWhilePending`: while a record exists, its view's gate is marked pending. | The record owns the guard, so the gate stays held while the record exists. OG-09's second owner and the unbound test observe the gate still held after a refusal. |
| `ExactBaseOrRefuse`: a captured record's base is the owner's root, and a `refused-base` result names an attachment whose base differs from that root. `SkipBaseMutant` violates it. | Check 4, and the capture takes its predecessor from the owner's current slot. OG-10, and the red control without check 4. |
| The witness `NeverRefusedStaleBase` is violated: a stale base is refused. | OG-10 observes that refusal. |
| `LockOrder`: no view holds the mutex while it waits to take its gate again. `RetakeGateMutant` violates it. | The owner takes no gate. OG-11, and the red control that takes the gate again. |

**Relation to the legacy repository.** OG-10 asks that a stale base be refused
in the same class as the legacy route. `checkpoint_at` in
[`history_repository_impl.rs`](../../../rspace++/src/rspace/history/history_repository_impl.rs)
compares the caller's expected root with the authority's current verified root
before it writes, and refuses a difference with
`RootError::RootCatalogConflict { expected_generation, actual_generation }`.
Both refusals come before any write, both name the caller's base and the
current one, and both are retried by taking the current base again. OG-10
drives the two routes through the same history and compares those three
observations.

**Relation to the caller registry.** At the owner, OG-09 and OG-10 witness two
rows that task 7932 still owes through the caller routes: CA-01,
`permit_requires_exact_authority`, since neither a permit of another gate nor
an attachment of another base authorizes a capture, and CA-08,
`publication_invalidates_all_old_view_permits`, since an attachment taken
before a publication is refused after it. Their derivation rows do not cite
those rows, for the reason given at VR-01.

**The tests.** The three bound tests and one unbound test are in
[`owner_checkpoint_view.rs`](../../../rspace++/tests/persistence/owner_checkpoint_view.rs).
Each opens an owner with the fixture of the durable owner tests and populates
its hot store. Below, "no effects" means no publication hook, no cold
admission and no index construction.

- `a_foreign_gate_permit_is_refused_before_capture_without_mutation` (OG-09)
  passes a write permit of a second, free gate, in two cases.
  - On a ready owner with a current attachment, it requires the refusal
    `ForeignPermit`, no capture hook and no effects; the phase `Ready`, with
    the state, the generation key, the three bag journals and the store files
    unchanged; both gates free; and the attachment still accepted by
    `validate_attachment`.
  - On an owner whose checkpoint failed at `AfterCatalogSyncBeforeRecovery`,
    so that its publication awaits recovery, it requires the refusal
    `ForeignPermit` and no effects; the phase `RecoveryRequired`, with the
    retained terminal, the journals and the store files unchanged; and the
    view gate held by the pending publication while the second gate is free.
    Reconciling then returns the terminal's state and frees the view gate.
- `a_stale_base_is_refused_like_a_legacy_root_conflict_without_mutation`
  (OG-10) attaches and checkpoints once, so that the attachment is one
  generation behind, then attaches again at the new base and puts one more
  datum into the hot store.
  - With a permit of the view gate, the stale attachment must be refused as
    `Attachment(StaleBase { attached, current })`, naming the two catalog
    slots, with no effects; the phase `Ready`, with the state, the generation
    key, the journals and the store files unchanged; and the gate free. The
    fresh attachment must then publish the next generation.
  - An in-memory legacy repository runs the same history through
    `checkpoint_at`. After one checkpoint advances its root, a checkpoint at
    the earlier root must be refused with `RootCatalogConflict` naming both
    generations, the root must be unchanged, and a checkpoint at the current
    root must publish the next generation.
- `checkpointing_under_the_held_view_gate_and_owner_mutex_completes` (OG-11)
  runs on a tokio runtime with the owner in a `tokio::sync::Mutex`. Within a
  ten-second timeout it takes the view gate's write permit, then the mutex,
  attaches and awaits `checkpoint_view`, and a capture hook records whether
  the gate and the mutex are held at capture. It requires the next generation
  within the timeout; both locks held at capture and both free afterwards;
  the phase `Ready` at the next generation; the store files changed; and the
  hot store's journals empty.
- `a_pending_publication_refuses_another_gates_own_permit` leaves a
  publication awaiting recovery, as OG-09's second case does, then calls
  `checkpoint_view` with a second gate and that gate's own permit. The permit
  passes check 1 and the attachment is still current, so check 5 must refuse
  with `Unresolved(RecoveryRequired)` and no effects, leaving the terminal,
  the journals and the store files unchanged, the view gate held and the
  second gate free.

The last test is bound to no ID. It pins the refusal that stands for the
model's `ReconcileFirst` until the view layer decides that case, as described
above. Each test compares all of its observations in one assertion, so a
failure reports every one of them.

**The migrated tests.** Four test files called `checkpoint`, `attach`, `state`
or `phase` without a gate or a hot store, and now pass them:

- [`durable_rspace_authority.rs`](../../../rspace++/tests/persistence/durable_rspace_authority.rs)
  defines `checkpoint_with`, and every checkpoint of its tests goes through it;
- [`owner_history_production.rs`](../../../rspace++/tests/persistence/owner_history_production.rs),
  which runs the owner histories of TH-08, passes the history's gate and hot
  store at every step and gives a reopened owner a new gate;
- in [`owner_incarnation.rs`](../../../rspace++/tests/persistence/owner_incarnation.rs),
  OG-06 calls `checkpoint_view` on the closed owner with a permit of the free
  view gate and requires `Closed`;
- [`durable_read_handle.rs`](../../../rspace++/tests/persistence/durable_read_handle.rs)
  checkpoints under a gate of its own.

Apart from OG-11 and the two cancellations, the tests poll each checkpoint
once with `poll_once` and require that poll to complete. An uncontended
checkpoint has nothing to wait for, and a checkpoint that waits fails its test
instead of hanging it, which is how the first red control below fails. Inside
a tokio task every polled acquisition of a tokio lock spends one unit of the
task's cooperative budget, and once the budget is spent even a free lock
reports `Pending`. `poll_once` therefore polls under
`tokio::task::unconstrained`, which exempts the future from the budget. No
expected value of a migrated test changed. One test polled `checkpoint` by
hand and matched `Poll::Ready(Err(Lease(CounterOverflow)))`; it now matches
`Err(Lease(CounterOverflow))` from `poll_checkpoint_with`, which polls once
and requires the poll to be ready.

**Earlier sections.** Three rows describe a checkpoint retry that reconciled
inside `checkpoint`. The retry now reconciles before it checkpoints, and the
rows say so. Their tests check what they checked before; only the place of the
reconciliation moved.

- TS-13 in the trusted successor capability section: reconciliation and the
  checkpoint retry, which reconciles before it checkpoints, acknowledge the
  pending successor exactly once.
- TS-13 in the trusted successor publication section: a retry, which
  reconciles before it checkpoints, acknowledges the published terminal.
- TH-08 in the journal-owner histories on real files section: an implicit
  reconcile is the one that a checkpoint retry runs before it checkpoints.

OG-06's row, and the description of its test in the owner incarnation section
above, now name `checkpoint_view`, which the test calls on the closed owner.
The rest of that section records that slice on its own tree and is unchanged,
including its red control without the closed-owner check of `checkpoint`; the
same check is now check 2 of `checkpoint_view`. Also unchanged, because they
describe `checkpoint` as it was when they were written and measured, are the
journal owner's description in the trusted successor publication section, the
cancellation paragraph of the faulted trusted histories section, which names
the gate as the owner's only await, and the measurement tables that count
checkpoint retries.

**Red controls.** Four production controls are each applied to an export of
the candidate tree. Each runs the red selection of 137 tests: the four of this
file, the five incarnation tests and the 128 durable persistence tests whose
module names begin with `durable_`.

- The owner takes the view gate again, with
  `Arc::clone(gate).write_owned().await` after `validate_attachment`, as the
  model's fault `retake-gate` does. Every checkpoint then waits for the permit
  that its own caller holds: `poll_once` reports the pending first poll, and
  OG-11's timeout expires. 26 tests fail: the four of this file, the twenty
  durable owner tests, the read-handle test of VR-01, and the V2 durable
  publication test that checkpoints through the owner. The incarnation tests
  pass, because OG-06's closed owner is refused before the second wait.
- Without check 1, OG-09's ready owner captures under the foreign permit and
  publishes the next generation through all thirteen publication hooks, and
  its pending owner refuses with `Unresolved(RecoveryRequired)` instead of
  `ForeignPermit`. OG-09 fails.
- Without the base comparison in `validate_attachment`, OG-10's stale
  attachment is accepted, and the owner publishes the next generation from
  its current base. OG-10 fails.
- Without check 5, the other gate's permit captures again over the pending
  publication. The new record replaces the pending one, so the retained
  terminal is lost and the view gate is freed while the outcome is still
  unknown. The unbound test fails.

**The OG rows.**

| ID | Required property | Test |
|---|---|---|
| OG-09 | Permit provenance. `checkpoint_view` refuses a write permit of another gate as `ForeignPermit` before it captures. On a ready owner the refusal runs no capture and no publication, and leaves the phase `Ready`, the state, the generation key, the hot store's journals and the store files unchanged, both gates free, and the attachment still valid. On an owner whose publication awaits recovery, the refusal runs no publication and leaves `RecoveryRequired`, the retained terminal, the journals and the store files unchanged, the view gate held by the pending publication and the other gate free; reconciliation then reaches the terminal and frees the view gate. | `a_foreign_gate_permit_is_refused_before_capture_without_mutation` |
| OG-10 | Exact base. An attachment taken one generation before the owner's current base is refused as `Attachment(StaleBase { attached, current })`, naming both catalog slots, before capture, with no publication and with the phase `Ready`, the state, the generation key, the journals, the store files and the gate unchanged; an attachment at the current base then publishes the next generation. The legacy repository, through the same history, refuses a stale expected root with `RootCatalogConflict` naming both generations, keeps its root, and publishes the next generation from the current root. | `a_stale_base_is_refused_like_a_legacy_root_conflict_without_mutation` |
| OG-11 | Caller-held permit. A caller that takes the view gate's write permit and then the owner's `tokio::sync::Mutex`, attaches and calls `checkpoint_view` completes within ten seconds and publishes the next generation, with both locks held at capture and both free afterwards, the phase `Ready`, the store files changed and the hot store's journals empty. | `checkpointing_under_the_held_view_gate_and_owner_mutex_completes` |

No persisted byte, root, identity or wire message changes.

### View gates, owner mutex and incarnations (2026-11-01)

Leaf G01 of task 7931 (commit `d971fad64`) models how callers reach the
durable owner before any caller is wired to it. The module
[`RSpaceJournalPublicationOwner.tla`](../../../formal/tlaplus/stack_safe_pda/RSpaceJournalPublicationOwner.tla)
gains a second specification, `ViewSpec`, beside `Spec`. Its vocabulary:

- A *view* is one RSpace object that shares the durable owner. The model has
  two views.
- Each view has a *gate*, the lock that its checkpoints and resets take. A
  held gate is a *permit*.
- The owner has one *mutex*, and at most one *pending record*: a publication
  that has been captured and not yet resolved.
- An *attachment* is what a view keeps in order to mutate through the owner:
  its *incarnation*, which names one lifetime of the owner, and its *base*,
  the root that the view's changes extend.

**Two specifications in one module.**

| | `Spec` | `ViewSpec` |
|---|---|---|
| Models | The single-checkpoint publication protocol. | The view layer: gates, mutex, pending record, attachments, close and reopen. |
| Moves | The protocol record. | The six view variables below. |
| Holds constant | The six view variables, at `ViewInit`. | The protocol record, at `FrozenState`, one initial state of `Init`. |
| Next-state relation | `Next` | `ViewNext` |
| Fairness | None. | Weak fairness of `OwnerAction` and of `ReconcilePending`. |
| Configuration and deadlock checking | RSpaceJournalPublicationOwner.cfg, off. | RSpaceJournalPublicationOwnerViews.cfg, on. |
| Properties | Seventeen invariants. | Eight invariants, `ViewTypeInvariant` and the seven below, and `PendingResolves`. |

`Spec` keeps the single-checkpoint protocol. The view variables are constant
under it, so its state graph, its seventeen invariants and its thirty red
controls are unchanged. `ViewSpec` holds the protocol record still and moves
only the view variables. The phases of its pending record, "Captured",
"RecoveryRequired" and "AwaitingAck", abstract the protocol of `Spec`, with at
most one acknowledgement.

**The view-layer state.** Six variables, set initially by `ViewInit`:

| Variable | Contents |
|---|---|
| *owner* | The owner's incarnation, its live flag, its current root and a ghost session counter (premise P3 below). |
| *attachment* | For each view, the incarnation and base that it holds and the ghost session that it attached in. |
| *viewGate* | For each view, "free", the view whose caller holds the gate, or "pending" when a pending record owns it. |
| *caller* | For each view, the call in progress: its stage, its operation, its permit, its target root and its result. |
| *ownerMutex* | "free", or the view whose call holds the mutex. |
| *pending* | The pending record's view, permit, base, phase and acknowledgement count, or `NoPending`. |

**One owner operation.** A caller takes its gate before it calls the owner,
and the owner mutex after the gate. The owner takes no gate while it holds the
mutex. Before it captures, the owner reconciles any pending record and checks
the permit, the incarnation and the base.

![One owner operation of the view-gate model, as an activity diagram, with close and reopen below it. In blue, the caller takes a gate first, through CallCheckpoint, CallReset or CallSpawn, and then the owner mutex through TakeMutex. In amber, while a publication is pending the mutex holder can only resolve it through ReconcileFirst: a RecoveryRequired record whose root is unchanged is dropped and frees its gate, one whose root has moved enters AwaitingAck, and an AwaitingAck record sets the view's base, frees the gate and is cleared. In purple, the decisions of CheckRequest, in order: a checkpoint's permit must be its own view's gate, the attachment's incarnation must be the owner's, and a checkpoint's base must be the owner's root. In red, the refusals refused-permit, refused-incarnation and refused-base, each before anything is captured. In green, a checkpoint's capture, which moves the permit into the pending record and marks its gate pending, the publication, which moves the root and enters AwaitingAck on the outcome new, and the acknowledgement, which sets the view's base to the new root and, when complete, frees the gate and clears the record; and, for a reset or a spawn, ApplyActivation. In amber again, the outcomes unknown-new and unknown-old enter RecoveryRequired and leave the gate held, as does an incomplete acknowledgement, each reporting uncertain. In grey, ReturnCall frees the mutex and the gates that the call holds, while a gate marked pending stays with its record. In teal, below, CloseOwner retires the incarnation of a live, quiescent owner, and ReopenOwner installs a fresh incarnation, advances the session and attaches one view at the current root.](figures/owner-view-gate-protocol.svg)

The figure's source is
[`owner-view-gate-protocol.puml`](figures/owner-view-gate-protocol.puml).

| Step | Action | Effect |
|---|---|---|
| Take a gate | `CallCheckpoint(view, permit)`, `CallReset(view, target)`, `CallSpawn(view)` | The caller takes a free gate: its permit's gate for a checkpoint, its own for a reset, both for a spawn with both callers idle. |
| Take the mutex | `TakeMutex(view)` | The view waits for the free mutex and then holds it. |
| Reconcile first | `ReconcileFirst(view)` | While a record is pending the holder can only resolve it, even a record captured through the other view's gate. |
| Check | `CheckRequest(view)` | With no record pending, in this order: a checkpoint's permit must be its own view's gate, else "refused-permit"; the attachment's incarnation must be the owner's, else "refused-incarnation"; a checkpoint's base must be the owner's root, else "refused-base". |
| Capture | `CheckRequest(view)` | An admitted checkpoint records its view, permit and base in phase "Captured" and marks the permit's gate "pending". |
| Publish | `PublishView(view, outcome)` | "new" moves the root and enters "AwaitingAck". "unknown-new" moves the root, "unknown-old" keeps it, and both enter "RecoveryRequired" and report "uncertain". |
| Acknowledge | `AcknowledgeView(view, completes)` | The view's base becomes the new root. A complete acknowledgement frees the gate and clears the record. An incomplete one keeps the record with one acknowledgement, and the gate stays "pending". |
| Activate | `ApplyActivation(view)` | An admitted reset moves the root to its target and reattaches the view there. An admitted spawn moves the root to the source view's base and attaches the other view to the same owner at that base. |
| Return | `ReturnCall(view)` | The mutex and the gates that this call holds are freed. A gate marked "pending" stays with its record. |

`Resolve` is the abstract reconciliation, shared by `ReconcileFirst(view)` and
by `ReconcilePending`, which acts with the mutex free. A "RecoveryRequired"
record whose base is still the root keeps the predecessor: its gate is freed
and the record is cleared. A "RecoveryRequired" record whose root has moved
enters "AwaitingAck". An "AwaitingAck" record sets its view's base to the root,
unless it was already acknowledged once, frees the gate and is cleared.

Close and reopen act only on a `Quiescent` owner: the mutex is free, no record
is pending and every caller is idle. `CloseOwner` makes the owner not live and
retires the incarnation by incrementing it, which refuses every attachment
taken before the close. `ReopenOwner(view, incarnation)` takes an incarnation
that `FreshIncarnation` allows, exactly one more than the value that close
left. It makes the owner live, advances the ghost session and attaches one view
at the current root. A view that kept its old attachment is refused.

**The seven invariants.**

| Invariant | Required behavior |
|---|---|
| `ExactBaseOrRefuse` | A "Captured" record's base is the owner's root, and a view whose call returned "refused-base" holds a base other than the root. |
| `NoViewMutationWhilePending` | While a record is pending, its view's gate is marked "pending". |
| `AckOnce` | A record is acknowledged at most once. Once acknowledged it is "AwaitingAck" with its view's base at the owner's root, so a retry only completes it. |
| `StaleIncarnationRefused` | A view that holds the mutex at the publish, acknowledge or activation stage has a live owner and an attachment from the owner's current session. |
| `PermitProvenance` | A pending record's permit is its own view's gate. |
| `LockOrder` | No view holds the mutex while it waits to take a gate again. |
| `SpawnSharesOwner` | A view whose call returned "spawned" has the owner's incarnation and root, and the other view's attachment has the same incarnation, session and base. |

`PendingResolves` requires that a pending record is eventually cleared. It is
checked under two weak-fairness conditions: `OwnerAction`, which is the steps
of the mutex holder and `TakeMutex`, and `ReconcilePending`. Callers, close and
reopen have no fairness, so the property never relies on a client calling. The
control runs with deadlock checking on.

The statement in
[Publication owner composition (2026-09-06)](#publication-owner-composition-2026-09-06)
that the model does not establish fairness or eventual repair of permanent I/O
faults concerns the protocol layer `Spec`, which has no fairness condition.
`PendingResolves` is bounded evidence about `ViewSpec`. It assumes that the
abstract `Resolve` always succeeds, so permanent I/O faults stay out of its
scope too.

**Five faults and three witnesses.** The constant *Fault* selects one defect
at a time. Each configuration named below is the cfg file with the module's
name as its prefix. "States" counts the states of TLC's counterexample,
including the initial state.

| Fault | Defect | Configuration | TLC reports | States |
|---|---|---|---|---|
| "retake-gate" | `TakeMutex` sends a checkpoint to `OwnerAcquiresGate`, so the owner waits for a gate that its caller holds. Leaf G08 removed this defect from the owner. | RetakeGateMutant | Invariant `LockOrder` is violated | 3 |
| "retake-gate" | The same fault, with deadlock checking on and no invariant. | RetakeGateDeadlockMutant | Deadlock reached | 4 |
| "skip-base" | `CheckRequest` passes any base. | SkipBaseMutant | Invariant `ExactBaseOrRefuse` is violated | 9 |
| "foreign-permit" | `CheckRequest` accepts a permit of another view's gate. | ForeignPermitMutant | Invariant `PermitProvenance` is violated | 4 |
| "close-no-bump" | `CloseOwner` keeps the incarnation, so old attachments still pass. | CloseNoBumpMutant | Invariant `StaleIncarnationRefused` is violated | 5 |
| "reopen-reuses-incarnation" | `FreshIncarnation` asks only for inequality with the closed incarnation, so a reopen can reuse one that a retained attachment holds. | ReopenReusesIncarnationMutant | Invariant `StaleIncarnationRefused` is violated | 6 |

The last fault is the model's negative control for the freshness premise of
the [caller-authority contract](#exact-contract-and-realization-premises):
"inequality with only the previous incarnation is insufficient".

Three witnesses are invariants whose violation shows that a case is reachable.
Each runs with no fault.

| Witness | The reachable case | States |
|---|---|---|
| `NeverRefusedStaleBase` | A checkpoint is refused for a stale base. | 9 |
| `NeverRefusedRetainedIncarnation` | A live owner refuses an attachment retained across a close and a reopen. | 6 |
| `NeverReconciledByOtherView` | A view that holds the mutex finds the other view's record pending, the case that `ReconcileFirst` resolves. | 8 |

**What TLC measured at commit `d971fad64`.** The runner
`bash scripts/check-rspace-journal-owner-model.sh` made 41 TLC runs (pgmcp
artifact 1587): the two controls, the thirty red controls of `Spec` and the
nine of `ViewSpec`, each passing its designated check.

| Specification | Initial states | States generated | Distinct states | Depth | Outcome |
|---|---|---|---|---|---|
| `Spec` | 4 | 1,716,908 | **64,080** | 21 | No error. The counts are those recorded before the view layer. |
| `ViewSpec` | 1 | 15,945 | **7,962** | 28 | Eight invariants and `PendingResolves` satisfied over the complete state space, with no deadlock. |

TLC's two-worker search is not level-synchronous, so the depth that a run
prints can differ: one scratch run of `ViewSpec` printed 29, and a one-worker
run and the gate's two-worker run printed 28 (pgmcp artifact 1586).

**Bounded scope.** This is bounded evidence over two views (`Views`), roots 0
and 1 (`Roots`) and two close and reopen cycles (`MaxSession`). Generations are
abstracted: the base check compares roots. Read handles (CA-22) and
soft-checkpoint restores, which task 7931 forbids refusing by incarnation, are
outside the model. The model is protocol evidence, not a proof of the Rust
owner.

**Design premises.** The model rests on five decisions.

- **P1, liveness is checked on `ViewSpec`.** `Spec` has no fairness and has
  behaviors that need not resolve: `Unwind` and `Recover` can repeat forever,
  `TightenColdLimits` can make `Recover` refuse permanently, and `PlaceDecoy`
  and `RemoveDecoy` can disable `CrashAndReopen` intermittently. Its recorded
  evidence must stay valid, so `Spec` is left as it was and `PendingResolves`
  is checked on `ViewSpec`. The runner fails unless TLC printed its
  temporal-check line.
- **P2, two configurations for "retake-gate".** TLC stops at its first error,
  and the two errors occur at different depths. `LockOrder` fails at the third
  state, after `CallCheckpoint` and `TakeMutex`. The first deadlock is at the
  fourth, after both views have called and one has taken the mutex: it waits
  for its own gate, and the other waits for the mutex. No single run can show
  both, so the runner has two entries for one fault.
- **P3, ghost sessions.** Under the two incarnation faults the incarnation
  comparison passes by construction, so an attachment of incarnation and base
  alone cannot falsify `StaleIncarnationRefused`. The model adds ghost session
  counters on the owner and on attachments. Rust carries no session, and its
  obligation stays the incarnation check.
- **P4, roots, not generations.** The model compares roots, as the legacy radix
  history's refusal ("is stale relative to authority root") does, so a root
  that recurs passes. `validate_attachment` compares whole catalog slots, which
  include the generation, so it refuses a superset. The capture's base stays
  exact, and a refusal then means that the slots differ, possibly only in their
  generation.
- **P5, no live test in the checks.** `CheckRequest` does not test the live
  flag. The retired incarnation of a closed owner is what refuses every old
  attachment, which makes "close-no-bump" causal: a live test there would
  refuse the closed owner without the incarnation ever mattering.
  `StaleIncarnationRefused` still names the live flag, so a closed owner cannot
  proceed unnoticed. The Rust owner refuses a closed owner first, at check 2 of
  [`checkpoint_view`](#durable-owner-checkpoint-views-under-caller-held-permits-2026-11-01).
  A closed owner holds no incarnation to compare, so that refusal is the Rust
  form of the failed incarnation test.

**Executable obligations.** The leaves of task 7931 own the tests that refine
these properties, and each adds its OG rows when it lands.

| Model property | Obligation | Owning leaf |
|---|---|---|
| `StaleIncarnationRefused`, `NeverRefusedRetainedIncarnation` | OG-06 and OG-07 | G07, in [owner incarnations](#durable-owner-incarnations-and-close-2026-11-01) |
| `PermitProvenance`, `NoViewMutationWhilePending` | OG-09 | G08, in [checkpoint views](#durable-owner-checkpoint-views-under-caller-held-permits-2026-11-01) |
| `ExactBaseOrRefuse`, `NeverRefusedStaleBase` | OG-10 | G08, in the same section |
| `LockOrder` | OG-11 | G08, in the same section; leaf G15 adds the regression on the hard checkpoint path when it routes that path through the owner |
| `AckOnce` | JO-13 | G08 migrated the owner's acknowledgement tests |
| `SpawnSharesOwner` | None yet | Leaf G19 adds its row when spawn attaches through the owner |
| `PendingResolves`, `NeverReconciledByOtherView` | None yet | The owner's handling of another view's record is decided with the view layer, as the row for `ReconcileFirst` in the checkpoint views section records |

### Lazy durable view reader (2026-11-11)

The [generation-leased read handle](#generation-leased-durable-read-handle-2026-11-01)
keeps a durable generation readable after the owner has taken over, but it
reads nothing beyond the semantic roots. This slice, the second leaf of the
view seam (task 12396), reads channel state through such a handle, one key at
a time. `DurableViewReaderV1`, in
[`view_reader.rs`](../../../rspace++/src/rspace/persistence/durable/view_reader.rs),
implements `HistoryReaderBase`, the trait through which the hot store reads
channel state that it does not hold in memory. A reader holds three parts:

| Part | What it provides |
|---|---|
| A `DurableViewReadHandleV1` | The shared core of one generation, that is, its pinned object index and open pack handles, and the lease that keeps the generation from reclamation. |
| Explicit `SemanticRoots` | The namespace roots of the state that the reader reads: the generation's current state or any state that the generation retains. |
| `DurableHotStoreBootstrapLimits` | The startup budgets of [`hot_store.rs`](../../../rspace++/src/rspace/hot_store.rs). `new` leaves them unbounded, so that they never bind on a state that the legacy store accepts; `with_limits` sets them. |

Each call visits one prefix of one namespace and decodes the records under
it:

| Method | Namespace root | Prefix | Budgets |
|---|---|---|---|
| `get_data_canonical(channel)` | `data_bag` | `RhoPathV1::datum_prefix(channel)` | Steps and endpoints. |
| `get_continuations_canonical(channels)` | `continuation_map` | `RhoPathV1::continuation_prefix(channels)` | Steps, endpoints, and the source payload bytes of each continuation. |
| `get_joins_canonical(channel)` | `join_bag` | `RhoPathV1::join_prefix(channel)` | Steps, endpoints and join occurrences. |

The budgets have no field for datum occurrences, and the eager bootstrap
binds none, so the data read passes an unbounded occurrence limit. Expanding
a datum's multiplicity still fails with a typed allocation error when the
memory cannot be reserved, and with a typed limit error when the count of
expanded values would pass `u64::MAX`, as described after the rulings below.
The reader reads no manifest after it is built: its results depend only on
its roots and on the immutable objects of its generation, so a successor
that the owner publishes later cannot change them.

**One decode path.** The bodies of the view's `data_for_channel`,
`continuations_for_channels` and `joins_for_channel` in
[`mod.rs`](../../../rspace++/src/rspace/persistence/durable/mod.rs) moved
unchanged into the private functions `data_for_channel_at`,
`continuations_for_channels_at` and `joins_for_channel_at`, which take the
verified state and the roots as arguments. The view methods keep their
signatures and pass their own verified state and the roots of their
manifest; the reader passes its handle's shared core and its pinned roots.
A data or join record expands to as many values as its multiplicity, and a
continuation record loads its source blob through the pinned index, which
checks the blob's length and digest before the blob is decoded.

**Rulings D1 and D2** (pgmcp note 27611) fix two choices:

- D1, literal groups. A receive group is read at the prefix of its channels in
  the given order. `(a,b)` and `(b,a)` are therefore different groups, and a
  group that repeats a channel, such as `(a,a)`, is a group of its own. In the
  RhoPathV1 key, a zero byte inside a channel frame is written `00 FF`, each
  frame ends with `00 00`, and the group ends with `00 01`, so no group's
  prefix is a prefix of another group's records: reading `(c,a)` does not see
  the records of `(c,a,b)`. There is no alias resolution, key map or facet.
- D2, fail closed. A read or limit error panics with the typed error, as the
  legacy history reader's `.expect` calls do. An empty `Vec` means only that
  the authenticated prefix holds no record.

**The expansion count.** The data and join reads count the values that they
have expanded through one private function of `mod.rs`,
`expansion_total(expanded, count, limit, resource)`. It adds a record's
multiplicity `count` to the number `expanded` of values already expanded
with `checked_add` and returns the sum when the sum is within `limit`. A sum
above `limit` is refused with the typed `ResourceLimit`, whose `attempted`
is the sum, and a sum past `u64::MAX` with `ResourceLimit` whose `attempted`
is `u64::MAX`: the true sum exceeds every representable limit, so `u64::MAX`
is a lower bound of it, and not even an unbounded limit admits it. The
`resource` is the read's existing name, "durable data materialization" or
"durable join materialization", and a `#[cfg(test)]` re-export lets the
tests call the function. The hot store accepts, and the owner publishes, a
channel whose datum multiplicities sum past `u64::MAX`, for instance one
record of multiplicity 1 and one of multiplicity `u64::MAX`. When the
larger record is visited second, the view's `data_for_channel` returns the
typed `ResourceLimit`, and the reader panics with it, in every build. Until
task 14335 the moved bodies counted with `result.len() as u64 + count`,
which overflowed there: a build with overflow checks, such as the debug test
profile, panicked with Rust's untyped "attempt to add with overflow", and a
release build wrapped the sum, let a bounded limit pass unchecked and failed
later with the typed `AllocationFailure`. When the larger record is visited
first, the sum is `u64::MAX`, which the unbounded limit admits, and the read
fails at the allocation with the typed `AllocationFailure`, as it did
before.

Two tests of the module check this behaviour, and like tests 4 and 5 below
they check ruling D2 and are bound to no ID.
`a_multiplicity_total_past_u64_max_is_a_typed_limit_error_in_either_visit_order`
puts those two records on two channels, one in each visit order, and
requires the typed error of each channel from the view and in the reader's
panic message, with the same assertions in debug and in release.
`expansion_total_is_exact_at_the_limit_and_typed_past_it` checks, for both
resources, a sum equal to the limit, a sum one above it and a sum past
`u64::MAX`, under two bounded limits and under the limit `u64::MAX`.

`ClosedDurableHistoryReader`, the reader that the eager bootstrap installs in
the hot store and that answers every query with an empty `Vec`, is now
`pub(crate)`, so that a test can put the same query to both readers. Only
unit tests construct the new reader until task 12399 bootstraps the hot store
from it. The RhoPathV1 key grammar is unchanged, and no persisted byte, root
or wire message changes.

**Tests.** The module
[`durable_view_reader.rs`](../../../rspace++/tests/persistence/durable_view_reader.rs)
builds each durable state by putting records into the hot store of a fresh
durable owner, publishing them and recovering a view of the published state:

1. `lazy_reads_equal_the_eager_enumerations_of_every_key` (VR-02) uses the
   channels `a`, `b` and `c`, the values `v0` to `v2` and the bodies `k0` and
   `k1`. A state puts up to seven data and up to five continuations on the
   groups `(a)`, `(b)`, `(a,b)`, `(b,a)`, `(a,a)` and `(c,a,b)`, each
   continuation with a join for every distinct channel of its group. Three
   seeded states and 64 generated ones are checked. For each state, the eager
   enumerations must equal the records put, so that neither side is empty by
   accident. The reader's results for every channel, for the six groups and
   for the unstored groups `(c)`, `(c,a)` and `(a,b,c)` must equal the eager
   `data_entries`, `continuation_entries` and `join_entries` restricted to
   that key, with multiplicities expanded, in the same order. The test
   asserts that each of these cases occurred: an empty state, a channel with
   two datum records, a datum of multiplicity two, a group with two
   continuations, both orders of one pair, a group that repeats a channel, a
   stored group that extends the probed group `(c,a)`, and a channel that is
   in only one of the data and join namespaces.
2. `lazy_reads_return_the_pinned_state_after_a_successor_is_published`
   (VR-03) takes a reader from a recovered view before
   `into_rspace_authority`. The owner then publishes and acknowledges a
   successor that removes and adds records in all three namespaces. The
   reader must return the old state's eager enumerations while the owner
   holds the successor and after the owner is torn down. The successor's
   enumerations must differ from them in every namespace, and a reader on the
   successor's generation with the old roots, which the successor retains,
   must return the old state too.
3. `a_receive_group_is_read_only_at_its_literal_ordered_prefix` (VR-07)
   publishes records under `(a,b)` only, under `(b,a)` only, and under both
   with different bodies. Each group must return exactly its own records,
   and `(a)`, `(b)` and `(a,a)` must return none.
4. `a_corrupted_continuation_source_panics_with_its_typed_error_where_the_closed_reader_is_empty`
   flips the last byte of a continuation's source blob in its pack file after
   the view has admitted the generation. `ClosedDurableHistoryReader` returns
   an empty `Vec` for the group. The view's own read returns the typed
   `DigestMismatch` whose expected digest is the reference's and whose actual
   digest is that of the corrupted bytes, and the reader panics with exactly
   that error in its message.
5. `every_reader_limit_binds_its_read_and_panics_with_the_typed_error` finds,
   for each read and each budget that applies to it, the least budget with
   which the view's own read succeeds. The reader must return the unbounded
   result at that budget, and must panic one below it, with the view's typed
   error at that budget in its message.

Tests 4 and 5 check ruling D2 and are bound to no ID.

**The VR rows.**

| ID | Required property | Test |
|---|---|---|
| VR-02 | Lazy reads equal the eager enumerations. For a published durable state, a `DurableViewReaderV1` over a read handle and the state's own roots returns, for every channel, the datums of the eager `data_entries` on that channel; for every ordered channel group, the continuations of the eager `continuation_entries` under exactly that group; and, for every channel, the joins of the eager `join_entries` on that channel; with multiplicities expanded and in the eager order. A key without records returns an empty `Vec`. | `lazy_reads_equal_the_eager_enumerations_of_every_key` |
| VR-03 | Pinned reads. A reader built from a durable view's read handle and that view's roots before `into_rspace_authority` returns the view's state while the owner holds a published successor and after the owner is torn down, although the successor's state differs in the data, continuation and join namespaces. A reader on the successor's generation with the retained predecessor's roots returns the predecessor's state. | `lazy_reads_return_the_pinned_state_after_a_successor_is_published` |
| VR-07 | Literal ordered groups. A continuation read visits only the prefix of its channels in the given order. With records under `(a,b)` only, reading `(b,a)` returns an empty `Vec`; with records under `(b,a)` only, reading `(a,b)` returns an empty `Vec`; with records under both, each group returns exactly its own. A group that repeats a channel of the pair, and either channel alone, return none. | `a_receive_group_is_read_only_at_its_literal_ordered_prefix` |

### Durable state roots (2026-11-11)

Task 7931 lets a caller name a durable state by a root and read it back. By
the user's ruling of 2026-11-11, the durable state root is the DAG's own
identity: no LMDB root is reproduced, and no mapping table from another kind
of root is kept. This slice, task 14293, defines that root and resolves it to
a retained state. Its code is
[`state_root.rs`](../../../rspace++/src/rspace/persistence/durable/state_root.rs).

**The root.** `DurableStateRootV1` is the 32-byte digest of the state's
`SemanticStateManifestV1`, the digest in the manifest's `object_ref()`. That
digest is the Blake2b-256 hash of the manifest's canonical bytes, so the root
already is a Blake2b-256 value. It converts to a `Blake2b256Hash` by copying
the 32 bytes, and back by checking the length and copying again; nothing is
hashed a second time. A `Blake2b256Hash` of any other length is refused with
`RootError::InvalidRootHash`.

A manifest holds the schema epoch, the semantic profile, the three namespace
roots and an optional legacy history root. The namespace roots are the
digests of canonical radix DAGs, and a canonical radix DAG is a function of
the set of endpoints it holds, not of the order in which they were inserted.
The legacy history root is different: it records where an imported state came
from, which is provenance and not content. Two manifests with the same
namespace roots and different legacy roots have different digests.

**Resolution.** `DurableAuthorityCoreV1::resolve(root)`, and the view's
`resolve`, which delegates to it, take these steps:

1. Look the digest up once in the admitted core's closure ledger
   (`StructuralClosureLedgerV1::counts_semantic_root`). Unless its current or
   its retained count is positive, refuse with `RootError::UnknownRootError`.
   No object is read. A root that only the metadata domain counts, such as a
   retention V2 anchor, is not a state and is refused here.
2. Read the object once, bounded by `SEMANTIC_MANIFEST_MAX_CANONICAL_LEN`
   (241 bytes). A store error is `KvStoreError`. An absent object is
   `CorruptRootCatalog`, because the admitted ledger and the object index
   disagree.
3. Parse it as a semantic manifest, which hashes the bytes again, and require
   that hash to be the root. A parse failure or another digest is
   `CorruptRootCatalog`.
4. Refuse a manifest that carries a legacy history root with
   `UnknownRootError`. Such a manifest is not a durable RSpace state, and
   refusing it makes every resolvable root a function of the contents alone.
5. Refuse a manifest of another semantic profile with `UnknownRootError`.
   Cold admission checks the profile of the current state only, so without
   this step a retained manifest of another profile would be read under this
   profile's types.
6. Return the manifest's reference and its `SemanticRoots`.

The cost is one probe of the ledger's 32-byte key, one index lookup, one
positional read of at most 241 bytes and one Blake2b-256 of those bytes. It
does not depend on the number of retained states or on the size of any
state, and nothing is enumerated. The object reads are measured: a counting
store observes exactly one read per resolution, of the resolved manifest, for
retained states of 1, 16 and 256 datums, the largest of which has a closure of
more than 256 objects, and no read for a refused digest. The single ledger
probe is read from the code, not measured.

**Read handles at a resolved state.** `DurableAuthorityViewV1::read_handle_at`
takes a resolved state, probes this view's ledger once more, so that a state
resolved in another store is refused unless this store also retains it, and
then acquires a lease on the view's catalog generation exactly as
`read_handle` does. A refusal takes no lease. The handle now records the
manifest it reads: `read_handle` records the view's current manifest, which
keeps its behavior, and `read_handle_at` records the resolved one.

No persisted byte, record kind, root or wire message changes, and no table is
added.

**The tests.**

- `durable_state_root_is_a_function_of_contents_across_insertion_orders_and_reopen`
  builds the same seven insertions (three datums, two continuations on one
  channel pair, and the pair's two joins) in fourteen fresh stores. Eight
  schedules insert them in one checkpoint: the seven rotations and the
  reverse. Six schedules publish three units, one checkpoint per unit, in
  every order of the units; their first states have three distinct roots.
  Every final root is equal. Each store is reopened, and the recovered root
  is the published one and resolves to the same roots in every store.
- `every_retained_state_resolves_to_its_own_roots_and_nothing_else_resolves`
  installs a checkpoint followed by an incremental successor whose retention
  manifest no longer retains the checkpoint state, so the checkpoint's
  manifest stays in the object index but leaves the ledger. The owner then
  publishes two more states. After a reopen, each of the three retained
  states resolves to exactly the roots that the view reported while it was
  current. Seven digests are refused: one absent from the store, and six that
  name objects in the store (the empty trie node, a data-bag node, both V1
  retention manifests, the current retention anchor, and the unretained
  checkpoint manifest, which parses as a manifest of this profile without a
  legacy root).
- `states_that_differ_in_one_endpoint_have_different_roots` publishes, in one
  store, a datum, a second copy of that datum, a continuation and a join, one
  per state. Each pair of consecutive states differs in exactly one endpoint,
  only that endpoint's namespace root differs, and the state roots differ.
- `a_manifest_with_a_legacy_history_root_does_not_resolve` installs one store
  whose two historical manifests carry legacy roots, and one whose current
  manifest carries one. Every such manifest is retained and parses with its
  legacy root, and every one is refused with the legacy-root reason; the
  current manifest without a legacy root resolves.
- Four further tests cover the rest of the slice: the `Blake2b256Hash`
  conversion for every length from 0 to 64 bytes, the measured reads of
  resolution, refusal of a retained manifest of another profile, and
  `read_handle_at`. That test requires one lease per handle, each handle
  reading its own state's roots after the view is dropped, no lease for a
  refused foreign state, and no lease left once the handles are dropped.

The red control resolves by object-index presence instead of the ledger's
root count. It accepts the unretained checkpoint manifest, which fails the
second test.

**The DI rows.** DI is the durable state identity family.

| ID | Required property | Test |
|---|---|---|
| DI-01 | Content-determined root. The same contents, built by every insertion order and checkpoint schedule exercised, publish one durable state root, and each store recovers that root on reopen and resolves it to the same semantic roots. | `durable_state_root_is_a_function_of_contents_across_insertion_orders_and_reopen` |
| DI-02 | Exact resolution of retained states. In a store that retains several states, each resolves to exactly its own manifest and semantic roots. An absent digest, and the digest of every object in the store that is not a current or retained manifest, including a manifest that the ledger no longer retains, is refused with `UnknownRootError`. | `every_retained_state_resolves_to_its_own_roots_and_nothing_else_resolves` |
| DI-03 | One endpoint, another root. Two states that differ in exactly one endpoint, whether added or changed in multiplicity, have different durable state roots, and only that endpoint's namespace root differs. | `states_that_differ_in_one_endpoint_have_different_roots` |
| DI-04 | Legacy provenance does not resolve. A retained manifest that carries a legacy history root, current or historical, is refused with `UnknownRootError` for that reason, while a manifest without one resolves. | `a_manifest_with_a_legacy_history_root_does_not_resolve` |

### Empty durable store genesis (2026-11-11)

Task 14292 adds the production constructor of a fresh durable store. Before
it, a durable directory could only be opened, and only tests installed store
images. Opening a directory that holds no `catalog.v1` fails with the startup
error `Io` (`NotFound` for `catalog.v1`); a catalog whose two slots are both
empty fails with `NoCatalogAuthority`.
`create_empty_store_v1::<P>(directory, limits)` in
[`startup.rs`](../../../rspace++/src/rspace/persistence/durable/startup.rs)
writes the first generation of an empty store and returns the catalog slot
that it made authoritative.

**What genesis writes.** Every byte comes from an existing encoder; no record
kind, byte layout, magic number or file name is new.

| Object or file | Content |
|---|---|
| Empty root | `CanonicalDagNode::empty()`, the root of all three namespaces. |
| Manifest | Schema 1, the profile `P::ID`, the three empty roots and `legacy_history_root = None`. The store's state is generation 1 of this manifest. |
| Retention anchor | A V2 anchor that retains that state with the canonical bytes of its closure (the manifest and the empty root, each once), no history, and the empty radix root. |
| Pack 1 | The genesis pack header, one generation-checkpoint PREPARE whose DATA objects are the empty root, the manifest, the anchor and the empty radix node in digest order, and its COMMIT, whose previous commit digest is zero. |
| Index generation 1 | The index header and one record per DATA object. |
| `catalog.v1` | Slot 0 names that COMMIT at catalog epoch 1; slot 1 stays empty. |

The constructor computes the whole image in memory before it opens the
directory: the empty-state closure and its V2 metadata closure, the
checkpoint descriptor, the PREPARE bytes, the index bytes, the recovery-limit
preflight that checkpoint publication applies, the COMMIT and the catalog
slot. A format or limit error therefore leaves the directory untouched.

**Write and sync order.** Genesis then takes the writer lease and performs the
order of checkpoint publication in
[`publication.rs`](../../../rspace++/src/rspace/persistence/durable/publication.rs),
without the steps that need an older generation (the reclamation candidate
and the catalog compare-and-swap read). Each row is one test injection point
of `DurableStoreGenesisStepV1`, placed immediately before its operation.

| # | Point | Operation |
|---|---|---|
| 1 | `CreatePack` | Create pack 1 (`O_CREAT`, `O_EXCL`, `O_NOFOLLOW`, owner-only). |
| 2 | `WritePackHeader` | Write the pack header at offset 0. |
| 3 | `WritePrepare` | Write the PREPARE at offset 128; require the exact PREPARE end. |
| 4 | `SyncPrepare` | Synchronize the pack file. |
| 5 | `CreateIndex` | Create index generation 1. |
| 6 | `WriteIndex` | Write the index image at offset 0; require the exact index tail. |
| 7 | `SyncIndex` | Synchronize the index file. |
| 8 | `WriteCommit` | Reopen pack 1 and require the exact PREPARE end, then write the COMMIT there; require the exact COMMIT end. |
| 9 | `SyncCommit` | Synchronize the pack file. |
| 10 | `SyncArtifactDirectory` | Synchronize the directory, which makes the pack and index names durable. |
| 11 | `CreateCatalog` | Create `catalog.v1` with `O_EXCL` and extend it to 672 zero bytes. |
| 12 | `WriteCatalogSlot` | Write the slot at offset 0. |
| 13 | `SyncCatalog` | Synchronize the catalog file. |
| 14 | `SyncCatalogDirectory` | Synchronize the directory, which makes the catalog name durable. |

**Why a crash at each point is safe.** A crash at a point means that the
operation at that point does not happen, every earlier operation does, and
the call returns without cleanup.

- Point 1: no store file exists. Recovery finds no `catalog.v1`, and the next
  genesis creates the store.
- Points 2 to 11: pack 1 (from point 6 also index 1) exists and `catalog.v1`
  does not, so recovery fails before it reads any pack.
- Inside the operation of point 11, between the create and the extension:
  `catalog.v1` exists with no bytes, and recovery fails the catalog's
  exact-length check. The injection point lies before the operation, so the
  test does not produce this state; it follows from `open_catalog`, which
  requires exactly 672 bytes.
- Point 12: `catalog.v1` holds 672 zero bytes, which select no authority
  (`NoCatalogAuthority`).
- A torn slot write of fewer than 336 bytes: an empty prefix is point 12. A
  prefix that leaves a nonzero byte of the slot unwritten fails the slot's
  CRC32C check, so selection fails with `NoValidCatalog`. A prefix whose
  unwritten suffix is all zero already equals the whole slot, and the store
  opens with exactly the genesis state; that is always the case from 332 bytes
  on, because the CRC32C field is eight bytes wide and its last four bytes are
  zero.
- Points 13 and 14: the complete slot is in the file, and the store opens
  with exactly the genesis state.

From point 2 on, the directory holds a store file, and the next genesis
refuses it. Under power loss, points 4, 7, 9 and 10 make every pack and index
byte and both names durable before the catalog exists; the catalog can then
survive only absent, short, all zero, torn or complete, and only the complete
slot opens, naming bytes that are already durable. Every create uses
`O_EXCL`, so genesis never replaces an existing file.

**Refusal.** Genesis lists the directory through the pinned directory handle's
path, requiring before and after the listing that the path still names the
same device and inode, and refuses with `DirectoryHoldsStoreFiles` when any
name is one that a durable store creates: `catalog.v1`, `reclaim.v1`,
`reclaim.v1.tmp`, or a pack, index or rebuild-index name that the existing
name functions produce. The error lists those names, and nothing is written.
The advisory `writer.lock.v1` is not a store file, and names that no durable
store creates do not block genesis.

**The SG rows.** SG is the store genesis family.

| ID | Required property | Test |
|---|---|---|
| SG-01 | Empty store genesis. `create_empty_store_v1` on an empty directory creates exactly `catalog.v1`, pack 1 and index generation 1 beside the writer lock, and its catalog file is the returned slot followed by an empty slot. `recover_read_authority` reopens the store with that slot: generation 1 of the manifest with schema 1, the store's profile, three empty namespace roots and no legacy history root; catalog epoch 1, pack 1 as first live and active pack, index generation 1 verified without a rebuild, and the V2 anchor of that state as retention manifest with two metadata objects. The data, continuation and join namespaces enumerate nothing. | `an_empty_store_reopens_at_generation_one_with_empty_namespaces` |
| SG-02 | Deterministic empty state. Two stores created independently in two directories have different store identities and the same empty-state manifest digest, which equals the digest of an independently encoded manifest with the three empty roots and no legacy history root. | `independently_created_empty_stores_share_the_empty_state_manifest_digest` |
| SG-03 | Crash-safe genesis. The injection points are taken from a traced genesis run and each is exercised once. A crash at a point stops genesis there; recovery then either fails without changing the directory or opens exactly the genesis state, and it opens exactly when the point follows the catalog slot write. A second genesis then refuses the directory, listing exactly the store files present, and changes nothing; only a crash before the first create leaves no store file, and then the second genesis creates the store. Every torn catalog-slot prefix from 0 to 335 bytes likewise opens exactly the genesis state or no authority, and both outcomes occur. | `a_crash_at_any_genesis_write_or_sync_point_never_opens_a_different_state` |

## Formal-to-executable derivation matrix

Classification is explicit: **D** is directly entailed at the model's abstraction level, **P** is partially entailed with concrete executable refinement obligations, and **E** is executable-only. `EXEC_ONLY` is deliberate rather than a missing citation. A campaign-complete report may contain D, P, and E rows, but no unresolved mismatch class.

| ID | TLA+ symbols | Rocq theorem or lemma symbols | Class | Qualification |
|---|---|---|---|---|
| IR-01 | `EXEC_ONLY` | `IncrementalReachability.incoming_replace_absent`, `IncrementalReachability.incoming_replace_present`, `IncrementalReachability.occurrences_positive`, `IncrementalReachability.incoming_positive` | P | Test-only model extraction; production refinement remains open. Each reachable parent contributes once; repeated outgoing references each contribute. Distinct-parent enumeration is required. |
| IR-02 | `EXEC_ONLY` | `IncrementalReachability.transferring_one_cursor_occurrence_preserves_pending` | P | Test-only model extraction; production refinement remains open. Transfer exactly one cursor occurrence and preserve every vertex's pending count. |
| IR-03 | `EXEC_ONLY` | `IncrementalReachability.discarding_an_exhausted_cursor_preserves_pending` | P | Test-only model extraction; production refinement remains open. Only an exhausted cursor may disappear without transferring its residual contributions. |
| IR-04 | `EXEC_ONLY` | `IncrementalReachability.increment_preserves_coherence`, `IncrementalReachability.increment_preserves_activation_balance` | P | Test-only model extraction; production refinement remains open. Activation uses the pre-update count and installs outgoing work only at zero-to-one. |
| IR-05 | `EXEC_ONLY` | `IncrementalReachability.pending_decrement_cannot_underflow`, `IncrementalReachability.decrement_preserves_coherence` | P | Test-only model extraction; production refinement remains open. Balance funds every legal decrement; coherence alone does not reject natural-number saturation at zero. |
| IR-06 | `EXEC_ONLY` | `IncrementalReachability.decrement_preserves_deactivation_balance` | P | Test-only model extraction; production refinement remains open. Deactivation installs outgoing work only at one-to-zero; repeated references are not collapsed. |
| IR-07 | `EXEC_ONLY` | `IncrementalReachability.activation_starts_with_added_root_occurrences`, `IncrementalReachability.activation_initial_configuration_is_valid` | P | Test-only model extraction; production refinement remains open. Initialize against old roots plus every added occurrence, with added vertices in the graph domain. |
| IR-08 | `EXEC_ONLY` | `IncrementalReachability.deactivation_starts_with_authorized_root_occurrences`, `IncrementalReachability.deactivation_initial_configuration_is_valid` | P | Test-only model extraction; production refinement remains open. Authorize removals against root multiplicity, not incoming count; preserve domain support. |
| IR-09 | `EXEC_ONLY` | `IncrementalReachability.drained_activation_is_quiescent`, `IncrementalReachability.drained_deactivation_is_quiescent`, `IncrementalReachability.add_before_remove_agrees_with_cold_ledger` | P | Test-only model extraction; production refinement remains open. Complete activation before deactivation; completed replacement agrees pointwise with the independently computed quiescent cold ledger. |
| IR-10 | `EXEC_ONLY` | `IncrementalReachability.complete_child_cursor_is_authenticated`, `IncrementalReachability.advancing_a_cursor_preserves_its_authenticated_suffix`, `IncrementalReachability.increment_preserves_cursor_authentication`, `IncrementalReachability.decrement_preserves_cursor_authentication`, `IncrementalReachability.authenticated_cursor_head_is_an_edge`, `IncrementalReachability.cursor_suffix_does_not_establish_processed_history` | P | Test-only model extraction; production refinement remains open. Suffix validity does not prove processed history or authenticate real object bytes/provenance. A skipped prefix is a limitation witness. |
| IR-11 | `EXEC_ONLY` | `IncrementalReachability.root_reachability_implies_positive_count`, `IncrementalReachability.member_rank_is_bounded`, `IncrementalReachability.positive_count_implies_root_reachability`, `IncrementalReachability.quiescent_support_is_exact_root_reachability` | P | Test-only model extraction; production refinement remains open. Positive support equals root reachability at quiescence on the finite ranked graph, not during draining. |
| IR-12 | `EXEC_ONLY` | `IncrementalReachability.incoming_extensional`, `IncrementalReachability.quiescent_ledger_is_unique` | P | Test-only model extraction; production refinement remains open. Any two coherent quiescent ledgers for the same acyclic graph and roots have identical counts and membership. |
| IR-13 | `EXEC_ONLY` | `IncrementalReachability.cursor_step_preserves_configuration`, `IncrementalReachability.cursor_execution_preserves_configuration`, `IncrementalReachability.drained_execution_has_exact_root_reachability` | P | Test-only model extraction; production refinement remains open. Every permitted microstep preserves balance, coherence, domain support and cursor suffixes; a completed run has exact support. |
| IR-14 | `EXEC_ONLY` | `IncrementalReachability.no_roots_no_reachability`, `IncrementalReachability.rootless_cycle_has_a_spurious_quiescent_ledger` | P | Test-only model extraction; production refinement remains open. A rootless self-cycle admits a positive quiescent ledger; omitting acyclicity makes the intended reachability claim false. |
| IR-15 | `EXEC_ONLY` | `IncrementalReachability.selected_sum_extensional`, `IncrementalReachability.selected_sum_replace_absent`, `IncrementalReachability.selected_sum_replace_present`, `IncrementalReachability.increment_releases_exact_expansion_credit`, `IncrementalReachability.decrement_releases_exact_expansion_credit`, `IncrementalReachability.every_cursor_step_spends_exactly_one` | P | Every generated microstep checks the exact ghost-potential identity; checked production arithmetic and runtime cost refinement remain open. |
| IR-16 | `EXEC_ONLY` | `IncrementalReachability.cursor_machine_progress`, `IncrementalReachability.valid_cursor_steps_are_well_founded`, `IncrementalReachability.measured_execution_erases`, `IncrementalReachability.every_execution_has_a_step_count`, `IncrementalReachability.measured_execution_prepend`, `IncrementalReachability.execution_conserves_exact_work`, `IncrementalReachability.every_valid_machine_reaches_drainage` | P | Actual transitions terminate and counted executions conserve work; independent executable action counters include repeated occurrences and empty-frame disposal. External stuttering is excluded. |
| IR-17 | `EXEC_ONLY` | `IncrementalReachability.cursor_step_cannot_restore_expansion_credit`, `IncrementalReachability.cursor_execution_cannot_restore_expansion_credit`, `IncrementalReachability.selected_sum_partition`, `IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work` | P | Untouched credit cancels from the exact drained-work equation. Unrelated-graph extension checks abstract locality, not PathMap allocation or byte-access costs. |
| IR-18 | `EXEC_ONLY` | `IncrementalReachability.drained_execution_has_exact_changed_frontier_work` | P | General initial configurations account for all residual cursor occurrences and disposal steps; the root-count specialization requires initially empty frames. |
| IR-19 | `EXEC_ONLY` | `IncrementalReachability.every_valid_machine_reaches_drainage`, `IncrementalReachability.rootless_cycle_has_a_spurious_quiescent_ledger` | P | The self-cycle witness separates termination from rooted-support correctness; production acyclic admission remains mandatory. |
| IR-20 | `EXEC_ONLY` | `IncrementalReachability.zero_pending_work_is_exactly_drained`, `IncrementalReachability.drained_machine_cannot_step` | P | Only pending work must vanish; positive unspent credit is allowed at drainage. |
| IR-21 | `EXEC_ONLY` | `IncrementalReachability.every_cursor_step_spends_exactly_one` | P | Wrong-cost and transition negative controls discriminate the exact model law; they are not an alternative implementation proof. |
| CR-01 | `EXEC_ONLY` | `CheckedReachability.checked_add_is_exact`, `CheckedReachability.checked_add_refuses_exactly_above_limit`, `CheckedReachability.checked_increment_refines_natural_update`, `CheckedReachability.checked_increment_refuses_exactly_at_capacity`, `CheckedReachability.checked_increment_preserves_bounded_counts` | P | Symbolic limit specializes to `u64::MAX`; generated one-below and exact-maximum counts check the executable refusal boundary. |
| CR-02 | `EXEC_ONLY` | `CheckedReachability.checked_sub_is_exact`, `CheckedReachability.checked_sub_refuses_exactly_underflow`, `CheckedReachability.checked_decrement_refines_natural_update`, `CheckedReachability.checked_decrement_refuses_exactly_zero`, `CheckedReachability.checked_decrement_preserves_bounded_counts`, `CheckedReachability.checked_updates_preserve_coherence` | P | Zero and positive counts distinguish checked refusal from natural-number saturation; a refused trial preserves its input. |
| CR-03 | `EXEC_ONLY` | `CheckedReachability.increment_changes_unique_inventory`, `CheckedReachability.checked_inventory_add_is_exact`, `CheckedReachability.checked_inventory_add_refuses_exactly_above_limit` | P | A symbolic unchanged background reaches one-below and exact `u64::MAX` distinct-object counts; generated large byte weights and duplicate references independently check byte refusal and zero-crossing behavior. The zero-byte-weight abstract case isolates arithmetic overflow, not production object admission. |
| CR-04 | `EXEC_ONLY` | `CheckedReachability.decrement_changes_unique_inventory`, `CheckedReachability.deactivated_inventory_is_funded`, `CheckedReachability.checked_inventory_remove_is_exact`, `CheckedReachability.exact_inventory_removal_cannot_underflow` | P | Last-reference removal subtracts one unique contribution; repeated-edge removal and invalid underflow are negative controls. |
| LT-01 | `EXEC_ONLY` | `CheckedLedgerTrial.checked_attempts_are_exactly_scheduled_cursor_steps` | P | Every accepted test-model step is the executable cursor machine's scheduled step, observed at every generated microstep; the production trial refines the event-first schedule (CL-03), and the trusted bind runs it in each domain (TS-03, task 11945). |
| LT-02 | `EXEC_ONLY` | `CheckedLedgerTrial.checked_step_is_failure_atomic`, `CheckedLedgerTrial.accepted_event_reservations_are_checked_arithmetic` | P | The natural-number reservations are refined by the test model's `u64` usage with `u128` candidates and by the Verus counters; the production trial reserves the same way and matches the test model's usage (CL-03). |
| LT-03 | `EXEC_ONLY` | `CheckedLedgerTrial.every_trial_retains_the_bit_identical_predecessor`, `CheckedLedgerTrial.checked_step_is_failure_atomic` | P | Retained predecessors are compared by value in the test model; in production the trial borrows its predecessor immutably, and CL-04 compares the predecessor's observation before and after every trial. |
| LT-04 | `EXEC_ONLY` | `CheckedLedgerTrial.prepared_trial_agrees_with_cold_ledger`, `CheckedLedgerTrial.union_peak_is_the_add_phase_cold_inventory`, `CheckedLedgerTrial.every_valid_authorized_trial_prepares_without_limits` | P | The cold oracle is an independent traversal of the test-only vector graph; production ledgers built over authenticated canonical objects match it in CL-01 and CL-03. |
| LT-05 | `EXEC_ONLY` | `CheckedLedgerTrial.union_peak_is_the_add_phase_cold_inventory`, `CheckedLedgerTrial.final_union_conflation_mutant_refuses_exact_final_limit` | P | Union and final inventories are separate abstract axes; the current, retained and metadata domains compose in DC-01 to DC-08, and the production current and retained domains are checked in CL-01 to CL-06. |
| LT-06 | `EXEC_ONLY` | `CheckedLedgerTrial.live_scratch_is_bounded_by_its_reserved_peak`, `CheckedLedgerTrial.uncharged_phase_initialization_mutant_overruns_scratch` | P | Scratch counts pending events and suspended frames; byte-level scratch accounting remains open. |
| LT-07 | `EXEC_ONLY` | `CheckedLedgerTrial.live_scratch_is_bounded_by_its_reserved_peak`, `CheckedLedgerTrial.scratch_allocation_conflation_mutant_refuses_admissible_trial` | P | The peak-versus-allocation distinction is checked on abstract scratch; production allocation accounting remains open. |
| LT-08 | `EXEC_ONLY` | `CheckedLedgerTrial.sound_quotes_bound_consumed_read_bytes`, `CheckedLedgerTrial.unsound_quote_admits_unbounded_consumption` | P | Quote soundness is a premise, and the unsound-quote witness shows it cannot be dropped; production quotes are declared lengths, which `load_reference` authenticates, so they are sound by construction (CL-03). |
| LT-09 | `EXEC_ONLY` | `CheckedLedgerTrial.trial_accepts_at_exact_measured_limits`, `CheckedLedgerTrial.trial_refuses_one_below_on_that_axis` | P | Exact boundaries are checked per axis on generated trials; production limit configuration remains open. |
| LT-10 | `EXEC_ONLY` | `CheckedLedgerTrial.checked_trial_is_the_guarded_unbounded_trial`, `CheckedLedgerTrial.refused_trial_retries_from_identical_predecessor` | P | Retry equivalence is checked under generated limits near the measured evidence; the production retry policy remains open. |
| LT-11 | `EXEC_ONLY` | `CheckedLedgerTrial.partial_commit_mutant_replaces_the_predecessor`, `CheckedLedgerTrial.skipped_cursor_contribution_mutant_is_not_quiescent`, `CheckedLedgerTrial.duplicated_cursor_contribution_mutant_is_not_quiescent` | P | Negative controls: closed fixture theorems, plus generated detections whose counts must both be positive. |
| DC-01 | `EXEC_ONLY` | `CheckedDomainComposition.composed_trial_is_the_conjunction_of_domain_trials` | P | First-refusal composition and per-domain limits are checked for all four variants on generated histories; the trusted bind realizes the production domain order and per-domain limits (TS-03 and TS-04, task 11945). |
| DC-02 | `EXEC_ONLY` | `CheckedDomainComposition.composed_trial_agrees_with_cold_domains`, `CheckedDomainComposition.canonical_composed_step_preserves_entry_domains` | P | Successor bases are compared with independent cold traversals after every generated step; production per-domain counts are compared with the same oracles in CL-01 and CL-06. |
| DC-03 | `EXEC_ONLY` | `CheckedDomainComposition.canonical_retention_deltas_realize_entry_roots` | P | Signed deltas are derived from test-model entries; production retention entries are only appended today: the cold-checked bind (`durable/admission.rs:2248-2299`) and the trusted bind (`durable/admission.rs:1386-1442`) append the target state in every retention format. |
| DC-04 | `EXEC_ONLY` | `CheckedDomainComposition.current_closure_is_within_retained_union` | P | Inclusion and the exact difference are checked on cold ledgers; the production current and union inventories equal the cold oracle's in CL-01. |
| DC-05 | `EXEC_ONLY` | `CheckedDomainComposition.evicting_one_root_alias_keeps_retained_support` | P | Alias evictions are generated in the test model; production has no eviction today. |
| DC-06 | `EXEC_ONLY` | `CheckedDomainComposition.data_inventory_is_disjoint_retained_plus_metadata`, `existing_digest_is_rejected` | P | Disjoint universes are a premise; production refuses a shared digest with `DuplicateInventoryDigest` where the retained union meets the retention manifest. |
| DC-07 | `EXEC_ONLY` | `CheckedDomainComposition.pins_protect_physically_without_changing_semantic_evidence` | P | A model pin protects the pinned object itself; the closure a production pin protects is the garbage collector's contract (tasks 7943 to 7945). |
| DC-08 | `EXEC_ONLY` | `CheckedDomainComposition.retained_sign_mutant_drops_the_predecessor_closure`, `CheckedDomainComposition.deduplicated_alias_mutant_drops_a_shared_closure`, `CheckedDomainComposition.pinned_root_mutant_inflates_the_retained_union` | P | Negative controls: closed fixture theorems, plus generated detections whose counts must all be positive. |
| CL-01 | `EXEC_ONLY` | `CreditedClosureLedger.credited_epoch_is_quiescent_for_added_roots`, `CreditedClosureLedger.credited_build_is_quiescent`, `CreditedClosureLedger.credited_cold_build_realizes_entry_domains`, `CreditedClosureLedger.credited_epoch_agrees_with_cold_ledger` | P | The theorems characterize credited epochs. That the production walk performs exactly a credited epoch is the no-unexpanded-crossing lemma, which is argued in prose and checked on generated canonical stores, not mechanized. |
| CL-02 | `EXEC_ONLY` | `CreditedClosureLedger.credited_epoch_inventory_grows_by_crossed_objects`, `CreditedClosureLedger.credited_crossings_are_newly_reachable_objects` | P | The inventory growth is entailed; equal reports, reads and limit errors against a retained-only build are executable observations. |
| CL-03 | `EXEC_ONLY` | `CheckedLedgerTrial.checked_attempts_are_exactly_scheduled_cursor_steps`, `CheckedLedgerTrial.prepared_trial_agrees_with_cold_ledger`, `CheckedLedgerTrial.checked_trial_is_the_guarded_unbounded_trial` | P | Production runs only the event-first schedule; the mirror maps digests to vertices and declared lengths to weights, quotes and consumed reads. |
| CL-04 | `EXEC_ONLY` | `CheckedLedgerTrial.every_trial_retains_the_bit_identical_predecessor`, `CheckedLedgerTrial.refused_trial_retries_from_identical_predecessor` | P | The immutable borrow of the predecessor enforces retention; load and parse refusals have no model counterpart. |
| CL-05 | `EXEC_ONLY` | `CheckedLedgerTrial.trial_accepts_at_exact_measured_limits`, `CheckedLedgerTrial.trial_refuses_one_below_on_that_axis`, `CheckedDomainComposition.composed_trial_is_the_conjunction_of_domain_trials` | P | Exact boundaries are checked per axis and domain on generated trials; the trusted bind's per-domain limits are checked at their exact values and one below (TS-04, task 11945). |
| CL-06 | `EXEC_ONLY` | `CheckedDomainComposition.composed_trial_agrees_with_cold_domains`, `CheckedDomainComposition.canonical_composed_step_preserves_entry_domains`, `CheckedDomainComposition.evicting_one_root_alias_keeps_retained_support` | P | The deltas come from the test model's entries; the trusted bind derives them from the receipt (TS-01 and TS-03, task 11945), and the metadata domain belongs to tasks 7925 and 7926. |
| CL-07 | `EXEC_ONLY` | `IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work`, `CheckedLedgerTrial.union_peak_is_the_add_phase_cold_inventory` | P | Work independence is checked on small generated cases. SW-01 to SW-03 check exact work at larger generated sizes, and the [fixed-delta trusted commit scaling section](#fixed-delta-trusted-commit-scaling-2026-10-29) measures the scaling against geometrically growing history: thread CPU exponents 0.1964 in the key count and 0.1363 in the retained history, upper 95% bounds 0.2086 and 0.1395, while gate G4 fails on allocated bytes at 0.4130 and 0.3186. |
| CL-08 | `EXEC_ONLY` | `EXEC_ONLY` | E | Stack bounds are platform evidence; both walks keep their work on the heap. |
| CL-09 | `EXEC_ONLY` | `CreditedClosureLedger.epoch_wide_credit_mutant_double_counts_a_shared_child`, `CreditedClosureLedger.unexpanded_crossing_mutant_leaves_an_uncredited_child`, `CheckedLedgerTrial.partial_commit_mutant_replaces_the_predecessor`, `CheckedDomainComposition.retained_sign_mutant_drops_the_predecessor_closure`, `CheckedDomainComposition.deduplicated_alias_mutant_drops_a_shared_closure` | P | Negative controls: closed fixture theorems, plus generated detections whose counts must all be positive. |
| RV-01 | `EXEC_ONLY` | `RetentionV2Format.retention_key_is_an_exact_48_byte_bijection` | D | Identity validity in the theory is the production `ObjectRefV1` length rule; the production parser and the model decoder are also compared on arbitrary 48-byte strings. |
| RV-02 | `EXEC_ONLY` | `RetentionV2Format.retention_key_order_is_entry_order`, `RetentionV2Format.little_endian_key_mutant_inverts_generation_order` | D | Entry order is the V1 `(generation, ObjectRefV1)` order; agreement with the strictness check of the V1 manifest parser is executable. |
| RV-03 | `EXEC_ONLY` | `RetentionV2Format.lossy_key_mutants_collapse_v1_aliases`, `RetentionV2Format.retention_key_is_an_exact_48_byte_bijection` | D | That the V1 manifest parser accepts each kind of alias pair is checked executably. |
| RV-04 | `EXEC_ONLY` | `EXEC_ONLY` | E | Byte goldens are executable evidence; the theory models domains, widths and lengths, not digests. |
| RV-05 | `EXEC_ONLY` | `RetentionV2Format.radix_count_denotes_entry_occurrences` | P | The theory abstracts the DAG body; the body parser's canonical-shape refusals and the typed refusal of each rule are executable, and the production reader refines the reference decoder (RV-22). |
| RV-06 | `EXEC_ONLY` | `RetentionV2Format.radix_count_denotes_entry_occurrences`, `RetentionV2Format.distinct_leaf_count_mutant_undercounts_shared_entries` | P | Sharing is generated over product histories; the production reader memoizes the same pairs of digest and width (RV-23). |
| RV-07 | `EXEC_ONLY` | `RetentionV2Format.domain_separation_excludes_cross_universe_digest_aliases`, `RetentionV2Format.anchor_only_checkpoint_mutant_is_not_reference_closed` | P | Closure completeness is checked on the test model's images; the stack bound is platform evidence. |
| RV-08 | `EXEC_ONLY` | `RetentionV2Format.v2_checkpoint_inventory_is_retained_plus_metadata`, `RetentionV2Format.plus_one_rule_mutant_rejects_every_v2_inventory`, `RetentionV2Format.anchor_only_checkpoint_mutant_is_not_reference_closed` | P | The codec checks the V2 rule's minimum (RV-21) and cold admission the exact metadata closure (RV-24); under the V2 writer the production builder `CheckpointObjectInventoryV1::from_retained_union_and_metadata` produces the inventory, and `v2_writer_checkpoints_carry_the_retained_union_and_the_metadata_closure` publishes it through all three checkpoint publishers. |
| RV-09 | `NoReaderAdoptsUnsupportedContent` | `RetentionV2Format.retention_domains_are_pairwise_distinct` | P | The model's format check is realized by the domain refusal of every V1 object reader. |
| RV-10 | `EXEC_ONLY` | `RetentionV2Format.plus_one_rule_mutant_rejects_every_v2_inventory`, `RetentionV2Format.retention_domains_are_pairwise_distinct` | P | The V1 codec checks the domain before the inventory; both refusals and their order are exercised in descriptors, prepare records and commit records. |
| RV-11 | `ReadersSelectTheSameSlot`, `RetentionV2TypeInvariant` | `EXEC_ONLY` | P | The model's envelope is the 336-byte slot outside the retention reference and the CRC. |
| RV-12 | `AdoptionIsNewestPublished` | `EXEC_ONLY` | P | Each executable envelope change realizes the model's changed envelope, and the observed fallback is the negative control's stale adoption. |
| RV-13 | `AdoptionIsNewestPublished`, `NeverOldReaderRefusesV2`, `CrashRecoverySelectsOnlyValidAuthority` | `EXEC_ONLY` | P | Terminal refusal is the existing no-fallback recovery; old readers refuse real V2 directory images in RV-27. |
| RV-14 | `EXEC_ONLY` | `RetentionV2Format.retention_domains_are_pairwise_distinct`, `RetentionV2Format.domain_separation_excludes_cross_universe_digest_aliases`, `RetentionV2Format.shared_node_domain_mutant_aliases_empty_metadata_and_semantic_roots` | D | Collision checking is the duplicate-digest refusal of the production inventory builder. |
| RV-15 | `NeverDualReaderAdoptsV2` | `RetentionV2Format.v1_migration_preserves_entries_order_count_and_roots` | P | The test model's radix builder migrates the fixtures; the production migration `migrate_retention_v1` is bound by RV-37 and by `v2_writer_migrates_a_v1_source_at_its_first_successor`. |
| RV-16 | `EXEC_ONLY` | `RetentionV2Format.retention_format_binding_separates_representations_of_one_state`, `RetentionV2Format.state_version_only_binding_mutant_resumes_across_formats` | P | The binding is a contract; its realization in the transfer protocol is tasks 5690 and 7938. |
| RV-17 | `EXEC_ONLY` | `RetentionV2Format.v1_migration_preserves_entries_order_count_and_roots` | P | Root preservation is checked on parsed semantic manifests; consensus identity remains the contract of task 5691. |
| RV-18 | `EXEC_ONLY` | `EXEC_ONLY` | E | Totality is executable evidence over arbitrary and perturbed bytes. |
| RV-19 | `NeverAdoptsBehindTornSuccessor` | `EXEC_ONLY` | D | The existing torn-slot test realizes the witness behavior at the byte level. |
| RV-20 | `NoReaderAdoptsUnsupportedContent` | `RetentionV2Format.retention_domains_are_pairwise_distinct` | P | The model's format check is realized by domain dispatch in every reader; that the dual capability differs from the V1 readers only on the two metadata domains is executable. |
| RV-21 | `EXEC_ONLY` | `RetentionV2Format.v2_checkpoint_inventory_is_retained_plus_metadata`, `RetentionV2Format.plus_one_rule_mutant_rejects_every_v2_inventory` | P | The codec checks only the V2 minimum, because it never reads radix bytes; the exact metadata count is the admission check of RV-24. |
| RV-22 | `EXEC_ONLY` | `RetentionV2Format.radix_count_success_unfolds_with_that_many_leaves`, `RetentionV2Format.shape_free_count_mutant_counts_an_empty_node_below_the_root`, `RetentionV2Format.radix_count_denotes_entry_occurrences` | P | The theory's count and unfold abstract the DAG body and the store; the production reader's agreement with the reference decoder, refusal class by refusal class, is executable. |
| RV-23 | `EXEC_ONLY` | `RetentionV2Format.memoized_radix_count_agrees_with_radix_count`, `RetentionV2Format.digest_only_memo_mutant_accepts_a_misplaced_shared_leaf`, `RetentionV2Format.radix_count_success_unfolds_with_that_many_leaves` | P | The memo theorem holds for every memo whose entries are correct; read counts, the refusal of an impossible reservation and the stack bound are executable and platform evidence. |
| RV-24 | `EXEC_ONLY` | `RetentionV2Format.count_and_inclusion_authenticate_the_disjoint_union`, `RetentionV2Format.overlapping_count_mutant_accepts_an_unauthenticated_object`, `RetentionV2Format.domain_separation_excludes_cross_universe_digest_aliases` | P | Inclusion is realized by the store pinned to the checkpoint pack, disjointness by the domain refusals of both walks, and distinctness by digest-keyed sets; the verdict function is checked exhaustively on three objects. |
| RV-25 | `EXEC_ONLY` | `RetentionV2Format.v1_migration_preserves_entries_order_count_and_roots`, `RetentionV2Format.memoized_radix_count_agrees_with_radix_count` | P | Each V2 source is the migration of its V1 twin; equal reports, the read equations and the limit boundaries are executable. |
| RV-26 | `EXEC_ONLY` | `EXEC_ONLY` | E | Resource boundaries are executable evidence; no limit was added. |
| RV-27 | `AdoptionIsNewestPublished`, `NoReaderAdoptsUnsupportedContent`, `NeverOldReaderRefusesV2`, `NeverDualReaderAdoptsV2`, `NeverAdoptsBehindTornSuccessor` | `EXEC_ONLY` | P | Directory images realize the model's refusal and adoption witnesses; which layer refuses each corruption is executable. |
| RV-28 | `EXEC_ONLY` | `EXEC_ONLY` | E | Successor refusal is the executable guard of the V1 writer, which only the test-build setter selects since the V2 writer became the default, standing in for a binary that reads V2 but writes V1; the test selects it explicitly, and RV-33 to RV-37 cover the V2 writer. |
| RV-29 | `EXEC_ONLY` | `EXEC_ONLY` | E | Totality is executable evidence over arbitrary bytes and arbitrary graphs. |
| RV-30 | `EXEC_ONLY` | `RetentionV2Editor.canonical_radix_exists_and_is_unique`, `RetentionV2Editor.unary_inner_mutant_breaks_uniqueness` | P | The theory names nodes by an injective function; byte-identical images under every edit order, and the refusal of a unary node by the constructor and the reader, are executable. |
| RV-31 | `EXEC_ONLY` | `RetentionV2Editor.radix_edit_batch_is_the_canonical_rebuild`, `RetentionV2Editor.unpruned_empty_child_mutant_is_not_the_rebuild` | P | The theory's editor follows `RadixEditor` step by step but walks every edit from the root, where `RadixEditor` resumes at a cursor step within the common prefix of adjacent keys (writer section, boundary); that resumption, the staged bytes, the typed CAS failure and the empty node are executable. |
| RV-32 | `EXEC_ONLY` | `RetentionV2Editor.radix_edit_changes_only_the_edited_paths`, `RetentionV2Editor.flat_manifest_mutant_rewrites_bytes_proportional_to_history` | P | The theory bounds the changed nodes and their bytes; the exact report counters, the fixed cases, the growth to $`2^{14}`$ entries and the crossover at 9 258 entries are executable. |
| RV-33 | `EXEC_ONLY` | `RetentionV2Editor.anchor_step_realizes_the_retention_entry_step`, `RetentionV2Editor.inserted_object_count_mutant_undercounts_an_aliased_leaf` | P | Anchor bytes and the refusal of the undercounted anchor are executable; `v2_writer_successors_appoint_the_retention_entry_step_across_restarts` publishes the steps in a directory. |
| RV-34 | `EXEC_ONLY` | `RetentionV2Editor.canonical_radix_store_is_admitted_by_the_cold_reader`, `RetentionV2Editor.unmerged_root_mutant_is_refused_by_the_cold_reader` | P | The production reader refines the theory's counts and unfold (RV-22, RV-23); admission after every step, by the reader and the three model passes, is executable. |
| RV-35 | `EXEC_ONLY` | `RetentionV2Editor.metadata_trial_work_is_the_changed_objects`, `RetentionV2Editor.deactivate_first_mutant_expands_the_shared_history` | P | The theory counts cursor steps by expansion credit; the production trial's expansion, reference and read counters are compared exactly with the oracle's changed objects. |
| RV-36 | `EXEC_ONLY` | `RetentionV2Editor.metadata_ledger_seed_is_the_cold_ledger`, `RetentionV2Editor.set_seed_mutant_drops_a_shared_radix_child` | P | The reader's occurrence counts realize the theory's seed; its equality with the ledger of an activation from an empty domain is executable. |
| RV-37 | `NoReaderAdoptsUnsupportedContent` | `RetentionV2Editor.v1_migration_is_the_canonical_rebuild_of_its_history`, `RetentionV2Editor.current_in_history_migration_mutant_duplicates_the_current_entry` | P | Both V1 readers refuse every migrated object, which realizes the model's format check; `v2_writer_migrates_a_v1_source_at_its_first_successor` shows recovery refusing the migration transaction. |
| TS-01 | `AdoptedSuccessorIsExactCatalog` | `TrustedSuccessorCapability.trusted_successor_rule`, `TrustedSuccessorCapability.trusted_inventory_equals_cold_admission`, `CheckedDomainComposition.canonical_composed_step_preserves_entry_domains` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section). The successor is compared field by field with the core that cold admission derives from the same history; the transliteration adopts only the exactly re-read catalog image. |
| TS-02 | `EXEC_ONLY` | `TrustedSuccessorCapability.appended_index_is_cold_rebuild` | P | Model extraction; the same test also checks production (task 11946; the trusted successor publication section). The appended index is compared with the cold rebuild on generated record lists, including the duplicate, conflict and capacity refusals, before any durable write. |
| TS-03 | `EXEC_ONLY` | `TrustedSuccessorCapability.production_order_is_run_composed`, `CheckedDomainComposition.composed_trial_is_the_conjunction_of_domain_trials`, `TrustedSuccessorCapability.skipped_domain_trial_mutant_breaks_cold_agreement` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section) and publication (task 11946; the trusted successor publication section). The production order is the theorem's projection onto the shared ledger; the two skipped-trial variants realize the mutant theorem. |
| TS-04 | `TrustedRefusalPrecedesAllMutation` | `TrustedSuccessorCapability.ceiling_decisions_are_exact`, `CheckedLedgerTrial.trial_accepts_at_exact_measured_limits`, `CheckedLedgerTrial.trial_refuses_one_below_on_that_axis` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section) and publication (task 11946; the trusted successor publication section). Exact acceptance and one-below refusal are checked on all 32 axes of the exact policy, by the bind and by a publication that writes nothing. |
| TS-05 | `EXEC_ONLY` | `CheckedLedgerTrial.union_peak_is_the_add_phase_cold_inventory`, `CheckedLedgerTrial.final_union_conflation_mutant_refuses_exact_final_limit` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section). Each domain's union peak is compared with the cold inventory of its base and added roots; the conflation control realizes the mutant theorem. |
| TS-06 | `PublishedPolicyIsSealedPolicy` | `TrustedSuccessorCapability.sealed_policy_binding_is_exact`, `TrustedSuccessorCapability.changed_policy_mutant_accepts_unbound_limits` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section) and publication (task 11946; the trusted successor publication section). Policy and mode equality at the preflight realize the sealed binding; the policy-ignoring variant realizes the changed-policy mutant. |
| TS-07 | `TrustedRefusalPrecedesAllMutation` | `TrustedSuccessorCapability.trials_precede_mutation`, `TrustedSuccessorCapability.publication_before_trials_mutant_publishes_refused_successor` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section) and publication (task 11946; the trusted successor publication section). Every receipt refusal is checked before the first durable write under all six fault settings; the append-before-trials variant realizes the mutant theorem. |
| TS-08 | `EXEC_ONLY` | `TrustedSuccessorCapability.cold_demand_is_not_a_function_of_the_inventory_summary`, `TrustedSuccessorCapability.reused_predecessor_demand_mutant_fabricates_cold_evidence`, `TrustedSuccessorCapability.trusted_source_preflight_mutant_accepts_minus_one` | P | Model extraction; the same test also checks production (task 11946; the trusted successor publication section). Cold demand is measured by a fresh traversal of the successor; the predecessor-demand and summary-demand controls realize the two mutant theorems. |
| TS-09 | `NoPhysicalRecoveryOnTrustedSuccess`, `TrustedSuccessorHasNoColdEvidence`, `NeverTrustedReady` | `EXEC_ONLY` | P | Model extraction; the same test also checks production (task 11946; the trusted successor publication section). No theorem models the publication hooks; the test model's hook trace and production's are compared with `FORMAL_PUBLICATION_PHASE_ORDER`. |
| TS-10 | `AdoptionFollowsDirectorySyncAndResolution` | `EXEC_ONLY` | P | Model extraction; the same test also checks production (task 11946; the trusted successor publication section). The two modes' durable writes are compared in the test model and, byte for byte, on real files under the three sidecars. |
| TS-11 | `TrustedOutcomeBoundaryIsExact`, `TrustedPublicationTypeInvariant`, `NeverIndeterminate`, `NeverPublishedResolutionFailure`, `NeverStageFailureBeforeCas` | `CatalogAppointedPrefix.retry_after_suffix_discard_preserves_appointed_bytes` | P | Model extraction; the same test also checks production (task 11946; the trusted successor publication section; generated histories crossed with failpoints at every hook are task 7928, the faulted trusted histories section). Every trusted hook's failure is replayed in the transliteration and on real files, and every interrupted pack image recovers the appointed prefix. |
| TS-12 | `EXEC_ONLY` | `TrustedSuccessorCapability.empty_and_awaiting_ack_branches_do_not_consult_cold_limits` | P | Model extraction; the same test also checks the production owner (task 11946; the trusted successor publication section). The owner mirror and the production owner count cold checks; the owner model's witness `NeverEmptyCheckpointUnderTightenedColdLimits`, checked by `scripts/check-rspace-journal-owner-model.sh`, reaches the same branch. |
| TS-13 | `EXEC_ONLY` | `TrustedSuccessorCapability.empty_and_awaiting_ack_branches_do_not_consult_cold_limits` | P | Model extraction; the same test also checks the production owner (task 11946; the trusted successor publication section). The owner mirror and the production owner count cold checks and acknowledgements; the owner model's witness `NeverAwaitingAckRetryUnderTightenedColdLimits` reaches the same branch. |
| TS-14 | `ColdModeSuccessIncludesColdAdmission`, `NeverColdReady` | `TrustedSuccessorCapability.cold_demand_is_not_a_function_of_the_inventory_summary` | P | Model extraction; the same test also checks production and the production owner (task 11946; the trusted successor publication section). The cold-checked refusal and the trusted success of one receipt realize the non-implication on every positive demand axis. |
| TS-15 | `NeverTrustedRefusal` | `TrustedSuccessorCapability.trials_precede_mutation` | P | Model extraction; the same test also checks the production owner (task 11946; the trusted successor publication section). The checkpoints of the owner mirror and of the production owner are refused by the bind before any write, so the owner emits no event and stays ready. |
| TS-16 | `EXEC_ONLY` | `TrustedSuccessorCapability.retained_state_maxima_is_cold_componentwise_max`, `TrustedSuccessorCapability.eviction_requires_recomputation_counterexample` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section). The maxima are checked while retention appends; the eviction counterexample is reproduced on its fixture, and eviction is task 7756. |
| TS-17 | `EXEC_ONLY` | `ExactStateShapeSummary.admitted_tries_have_no_empty_leaf`, `ExactStateShapeSummary.maximum_logical_path_is_the_longest_key`, `ExactStateShapeSummary.combined_maximum_is_the_walk_maximum`, `ExactStateShapeSummary.batch_weight_delta_is_exact`, `ExactStateShapeSummary.endpoint_existence_delta_is_exact`, `ExactStateShapeSummary.payload_delta_is_exact`, `ExactStateShapeSummary.key_length_multiset_tracks_the_batch`, `ExactStateShapeSummary.largest_nonempty_bucket_is_the_longest_key`, `ExactStateShapeSummary.rewrite_moves_logical_positions_exactly`, `ExactStateShapeSummary.absorbed_reports_move_positions_exactly`, `ExactStateShapeSummary.counted_rewrite_is_the_radix_edit`, `ExactStateShapeSummary.trusted_shape_summary_equals_the_cold_walk`, `ExactStateShapeSummary.emitted_minus_loaded_mutant_miscounts_a_compression_merge`, `ExactStateShapeSummary.digest_dedup_counter_mutant_undercounts_equal_leaves`, `ExactStateShapeSummary.running_maximum_mutant_keeps_a_deleted_longest_key`, `ExactStateShapeSummary.empty_leaf_mutant_breaks_the_path_maximum` | P | Production refinement of the derivation and of the trusted publication that returns it (task 11946; the trusted successor publication section). The production summary is compared with the production cold walk in memory, with durable cold admission after every publication and with restart admission, on shared subtrees, an absorbing merge, deletion of the unique and of tied longest keys, and batches across all three namespaces; the composed theorem's premise that the rewrite lists the batch applied to the entries is checked against an independent rebuild. |
| TS-18 | `EXEC_ONLY` | `TrustedSuccessorCapability.trusted_successor_rule` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section). The rule's token and profile premises are checked as refusals at the receipt check. |
| TS-19 | `EXEC_ONLY` | `RetentionV2Editor.v1_migration_is_the_canonical_rebuild_of_its_history`, `RetentionV2Editor.metadata_ledger_seed_is_the_cold_ledger` | P | Model extraction; the same test also checks the production bind (task 11945; the trusted successor bind section). The first successor's migration and the seeded metadata ledger are compared with the cold traversal; the double-migration control keeps the old anchor counted. |
| TH-01 | `HistorySoleAuthority`, `HistoryAuthorityIsCatalogSelected`, `NeverRecoveredSuccessorAfterAmbiguity`, `NeverRecoveredPredecessorAfterAmbiguity` | `TrustedSuccessorCapability.faulted_history_keeps_an_exact_sole_authority`, `TrustedSuccessorCapability.ambiguous_adoption_mutant_breaks_sole_authority` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). Every recovery that the authority chain settles is the predecessor or the fault-free successor, as the persisted image selects, which realizes the capstone's admissible recoveries; the adoption controls realize the mutant theorem. On real files, after every step of the seeded and generated histories, a byte copy admitted cold selects the predecessor after a definite non-publication, the exact retained terminal slot after a published error, and the slot that the persisted image selects after an indeterminate write, and the settled core equals that copy's; a copy that holds a corrupt reclamation intent or a foreign catalog is refused instead, and a copy taken under the forged re-read follows the rolled-back image to the predecessor. |
| TH-02 | `HistoryTypeInvariant`, `RunOutcomeBoundaryIsExact` | `TrustedSuccessorCapability.faulted_history_keeps_an_exact_sole_authority`, `TrustedSuccessorCapability.trusted_inventory_equals_cold_admission` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). Every recovered core equals the test model's cold admission of its state, and every publication's outcome is replayed through the single-publication transliteration. On real files, every step's settled core equals, observation for observation, cold admission of a byte copy in a fresh directory with its own lease and registry, except where the copy must be refused or follows a forged image; the current closure reads the bytes of an independent rebuild of the model state through the core and through the copy, a pinned closure through the core and through the pinned files by name, and the ledger's retained domain equals the union of the adopted current closures at every restart and at the end. |
| TH-03 | `HistoryAppointedPrefixIsCommitted`, `HistoryCommitsAtMostOnce`, `NeverRetriedAfterSuffix` | `TrustedSuccessorCapability.faulted_history_appoints_each_commit_once`, `TrustedSuccessorCapability.append_without_truncation_mutant_appoints_an_orphan`, `CatalogAppointedPrefix.retry_after_suffix_discard_preserves_appointed_bytes` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). The pack chain's appointed prefix equals the committed transactions after every drawn fault, and appending over a suffix without truncation recovers a different prefix, which realizes the mutant theorem. On real files, hooks 2 to 14, the five I/O failpoints, torn slot prefixes at both parities, a short pack and a catalog changed at the compare are injected from step 2 on; a retry over a pack suffix truncates it, a retry over an index suffix is refused at the index length check until held-lease recovery rebuilds the index, which also survives each of its six rebuild failpoints, a commit abandoned by an indeterminate write that kept the old slot is truncated by the next publication rather than replayed, and the appointed prefix equals the reference chain's. |
| TH-04 | `HistoryRefusalPreservesFiles`, `NeverRefusedWithSuffix` | `TrustedSuccessorCapability.trials_precede_mutation` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). A changed-policy refusal at every step writes nothing and leaves the pack, any suffix and the appointed count as they were. On real files, a bind under each lowered axis, a changed policy, a cold-bound transaction and a changed inactive slot are refused with no hook at a clean point; one of each is refused over a pack suffix, and all but the cold-bound transaction, whose cold acquisition would rebuild the index, over an index suffix; every file is unchanged. |
| TH-05 | `HistoryPinnedPrefixStable`, `NeverReaderAcrossFault`, `LeaseSafety`, `NoDeletionBeforePublication` | `CatalogAppointedPrefix.discarding_unselected_suffix_preserves_appointed_bytes` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). The compaction model's lease invariants are cited, not remodelled; the authority chain keeps a reader pinned across faults and their recoveries. On real files, a reader's pinned generation files keep their pinned prefix and serve its objects by name across faults, recoveries, fresh checkpoints and restarts; reclamation is deferred while the reader pins and completes after the unpin, also after a crash in the middle of reclamation, and a lease counter at its maximum refuses the pin with nothing changed. |
| TH-06 | `HistoryRecoveryRequiredAfterAmbiguity`, `NeverRestartedMidPublication` | `TrustedSuccessorCapability.faulted_history_keeps_an_exact_sole_authority` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). The transliteration requires recovery after every ambiguous outcome, and the pack chain restarts at every image that an interrupted trusted hook leaves and recovers the prefix that the model selects. On real files, a publication from the stale predecessor after an ambiguous outcome is refused, at the catalog compare with no hook when the catalog changed and at the index length check when an indeterminate write kept the old image; each of the thirteen trusted hooks is killed in a child process at step 3, and restarts mid-publication adopt the catalog's selection. |
| TH-07 | `HistoryAuthorityIsCatalogSelected`, `RunAdoptedSuccessorIsExactCatalog`, `NeverRecoveredUnderDecoy`, `NeverRepairedForeignCatalog` | `EXEC_ONLY` | P | Model extraction; the same test also checks production (task 11967; the faulted trusted histories on real files section). No theorem models the pathname; the authority chain recovers under a decoy to the catalog's selection, and repaired catalogs occur. On real files, decoys bound to the pathname at the base check, at the re-read, during held-lease recovery and at a forged re-read are never adopted or written, and a foreign catalog image is refused by the re-read, by held-lease recovery and by a restart until it is repaired. |
| TH-08 | `ReopenDiscardsVolatileJournal`, `RecoveryAcceptsOnlyExactTerminal`, `AtMostOneAcknowledgement`, `GateHeldUntilResolution` | `TrustedSuccessorCapability.empty_and_awaiting_ack_branches_do_not_consult_cold_limits` | P | Model extraction; the same test also checks production (task 11968; the journal-owner histories on real files section), including the operation gate that the mirror does not model. On real files, after every step of four seeded and thirty-two generated owner histories, a byte copy admitted cold and its bootstrapped hot store match the model. Restarts replay no journal, every acknowledgement retry runs no publication phase and no cold check, and a retry after an unwind past the acknowledgement acknowledges exactly once. The owner mirror counts cold checks, publications and acknowledgements across eight fault choices and four interleavings; the owner model's journal-replay and pathname-recovery faults and its witness `NeverReconciledUnderDecoy`, checked by `scripts/check-rspace-journal-owner-model.sh`, reach the same branches. |
| PX-01 | `EXEC_ONLY` | `PathExactIncrementalClosure.staged_first_refuses_a_valid_delta_at_a_coincident_certificate`, `PathExactIncrementalClosure.certificate_first_verifier_complete` | P | Production refinement on the theory's coincidence fixture in canonical bytes, with the two certificates admitted directly; the report's counts and the logical-visit limit bind the model's eight visits. |
| PX-02 | `EXEC_ONLY` | `PathExactIncrementalClosure.digest_only_certificate_mutant_admits_an_uncovered_occurrence`, `PathExactIncrementalClosure.certificate_first_verifier_sound` | P | Production refinement of exactness: a certificate that differs from the exact one in its path, namespace, kind or reference does not stop the walk, the executable side of the digest-only mutant. |
| PX-03 | `EXEC_ONLY` | `PathExactIncrementalClosure.staged_first_accepts_a_staged_object_owned_only_at_inherited_positions`, `PathExactIncrementalClosure.skipped_reachability_mutant_admits_an_unreached_staged_object` | P | Production refinement of the second outcome change: the final reachability check refuses a staged object that the walk reaches only at a certified occurrence. |
| PX-04 | `EXEC_ONLY` | `PathExactIncrementalClosure.staged_first_refuses_a_valid_delta_at_a_coincident_certificate` | P | Production refinement on the measured shrunk transition: the production seal accepts it, and the staged-first model walk reproduces the measured refusal. The rewrite's recording of certificates is checked only through this outcome. |
| PX-05 | `EXEC_ONLY` | `PathExactIncrementalClosure.certificate_first_reached_is_delta_closure`, `PathExactIncrementalClosure.certificate_first_verifier_complete`, `PathExactIncrementalClosure.verifiers_agree_without_coincidence`, `PathExactIncrementalClosure.staged_first_verifier_sound` | P | Production refinement on generated transitions with shared subtrees: the production seal accepts every coherent transition, the model walk over independently derived certificates reaches exactly the staged objects, and the two orders agree without coincidence. The rewrite's recording rule remains an executable premise. |
| RI-01 | `RecoveryAvailable`, `IntentTempNeverLive` | `EXEC_ONLY` | P | Each model `TearIntent` is realized at every concrete tear point of the atomic writer, pinned or not, and each recovery leaves no temporary. The model ignores the temporary's contents, which recovery never reads, so an empty, a torn and a complete temporary are one model state; the directory snapshot checks the concrete bytes. |
| RI-02 | `TornIntentNeverPublished`, `RecoveryAvailable`, `IntentTempNeverLive` | `EXEC_ONLY` | P | Generated sequences of tears and recoveries realize repeated `TearIntent` and `Recover` steps from the pre-CAS slot; the final name never holds a torn record. |
| RI-03 | `RecoveryAvailable` | `EXEC_ONLY` | P | Held-lease recovery, not only cold admission, realizes `Recover` after an interrupted intent write. |
| RI-04 | `SelectedDeletionObligationCovered` | `EXEC_ONLY` | P | The model has no foreign writer. A record planted after the gate is the executable premise that the no-replace rename never overwrites an intent, so no deletion obligation is lost; the record stays fail-closed. |
| JR-01 | `EXEC_ONLY` | `JournalOwnerReadiness.readiness_requires_exact_registry_and_catalog` | P | The abstract pair is refined by existing generation-registry identity and complete catalog equality; constructor refusal returns the original view. |
| JR-02 | `EXEC_ONLY` | `JournalOwnerReadiness.invalid_binding_retains_the_original_pending_receipt` | P | Invalidated-base refusal must retain the original pending record, permit, writer and pin without recovery I/O. |
| JR-03 | `EXEC_ONLY` | `JournalOwnerReadiness.valid_base_allows_abort_independently_of_receipt_revision` | P | A same-base stale journal may abort and be recaptured; it cannot be published using its old receipt. |
| HJ-01 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.acknowledgement_preserves_live_state` | P | Abstract live/journal separation; concrete resident records and installs are checked in Rust. |
| HJ-02 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.exact_acknowledgement_clears_all_five_journals` | P | All five populated Rust journals must clear in debug and release. |
| HJ-03 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.stale_acknowledgement_changes_nothing` | P | Concrete all-domain conflict preflight remains an executable obligation. |
| HJ-04 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.successful_acknowledgement_requires_exact_journal` | P | Exact abstract equality; Rust receipt identity is separately required by the owner model. |
| HJ-05 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.post_state_addition_recovers_base` | P | Natural arithmetic; checked Rust count inversion is executable. |
| HJ-06 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.post_state_removal_recovers_base` | P | Natural subtraction premise; zero and nonzero removals are generated. |
| HJ-07 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.one_occurrence_removal_requires_unit_delta` | P | Single-removal producer semantics require the concrete multiplicity property. |
| HJ-08 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.missing_count_is_distinct_from_captured_zero` | P | Rust must preserve missing evidence separately from an observed zero. |
| HJ-09 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.journal_lookup_uses_exact_native_record_key` | P | Native byte-key framing and repeated channel positions are executable. |
| HJ-10 | `EXEC_ONLY` | `HotStoreJournalAcknowledgement.reframed_join_key_misses_native_record` | P | The concrete native-key test detects the extra-framing producer error. |
| JV-01 | `EXEC_ONLY` | `JournalVersionFreshness.advancing_never_reuses_current_version` | P | Rust advances the revision in place; rollover must allocate a fresh identity while prior receipts retain their identity tokens. |
| JV-02 | `EXEC_ONLY` | `JournalVersionFreshness.arbitrary_nonempty_history_cannot_return_to_old_version` | P | The unbounded abstract history law assumes fresh identities; generated Rust histories retain and compare every prior version. |
| JV-03 | `EXEC_ONLY` | `JournalVersionFreshness.rollover_uses_a_fresh_identity` | P | Explicit `u64::MAX` refinement changes the identity and resets the local counter without an artificial lifetime limit. |
| SI-01 | `EXEC_ONLY` | `ExactSuccessorInventory.replacement_conserves_inventory`, `ExactSuccessorInventory.exact_replacement_inventory` | P | Finite partition accounting; Rust must derive exact current membership from the sealed target. |
| SI-02 | `EXEC_ONLY` | `ExactSuccessorInventory.removed_inventory_is_bounded` | P | Histories exercise removed/shared membership and checked Rust inventory arithmetic. |
| SI-03 | `EXEC_ONLY` | `ExactSuccessorInventory.staged_only_inventory_counterexample` | P | Concrete old arithmetic is a negative control; semantic/physical metadata classification is executable. |
| SI-04 | `EXEC_ONLY` | `EXEC_ONLY` | E | Exact catalog/registry/sequence binding is tested; the partition lemmas do not prove Rust capability ownership. |
| SI-05 | `EXEC_ONLY` | `EXEC_ONLY` | E | Inventory-budget refusal and changed-policy nonmutation are concrete preflight properties. |
| IP-01 | `EXEC_ONLY` | `CatalogAppointedPrefix.discarding_unselected_suffix_preserves_appointed_bytes` | P | Existing catalog authority must authenticate the exact retained prefix before discarding only an unselected tail. |
| IP-02 | `EXEC_ONLY` | `CatalogAppointedPrefix.retry_after_suffix_discard_preserves_appointed_bytes` | P | Incremental failure/retry must retain the original writer and all selected bytes. |
| INV-01 | `OrderedInventoryIsExact` | `successful_merge_is_exact_permutation` | D | Exact ordered disjoint merge. |
| INV-02 | `DescriptorInventoriesAreExact` | `successful_merge_adds_exactly_one_member`, `refined_descriptor_carries_exact_six_inventories` | D | Exact cardinality. |
| INV-03 | `DescriptorInventoriesAreExact` | `successful_merge_adds_exactly_declared_bytes`, `refined_descriptor_carries_exact_six_inventories` | D | Exact canonical bytes. |
| INV-04 | `OrderedInventoryIsExact` | `successful_merge_membership_is_exact`, `successful_merge_preserves_strict_digest_order` | D | Under sorted unique premises. |
| INV-05 | `OrderedInventoryIsExact` | `successful_merge_membership_is_exact`, `successful_merge_preserves_strict_digest_order` | D | Under manifest-disjoint premise. |
| INV-06 | `OrderedInventoryIsExact` | `successful_merge_preserves_strict_digest_order` | P | No-production-sort/allocation behavior is executable. |
| INV-07 | `OrderedInventoryIsExact` | `existing_digest_is_rejected` | P | Exact two-length diagnostic is executable. |
| INV-08 | `OrderedInventoryIsExact`, `DescriptorInventoriesAreExact` | `successful_merge_is_exact_permutation` | P | Current-subset relation is TLA+ and executable. |
| DES-01 | `DescriptorInventoriesAreExact` | `refined_descriptor_carries_exact_six_inventories` | D | Six exact scalars. |
| DES-02 | `DescriptorInventoriesAreExact`, `OrderedInventoryIsExact` | `refined_descriptor_carries_exact_six_inventories` | P | Codec round trip is executable. |
| DES-03 | `DescriptorInventoriesAreExact` | `EXEC_ONLY` | P | Authenticated mutation rejection is executable. |
| DES-04 | `DescriptorInventoriesAreExact` | `refined_descriptor_carries_exact_six_inventories` | D | Disjoint-union equations. |
| DES-05 | `DescriptorInventoriesAreExact` | `EXEC_ONLY` | P | Arbitrary numeric boundaries are executable. |
| DES-06 | `OrderedInventoryIsExact` | `EXEC_ONLY` | P | Hash algorithm/domain bytes are executable. |
| DES-07 | `CheckpointAndIndexExactlyAdmitted`, `EverySingleBindingMutationIsRejected` | `successful_merge_membership_is_exact`, `every_single_binding_mutation_is_rejected` | P | Concrete parser/authentication mutations are executable. |
| DES-08 | `EXEC_ONLY` | `EXEC_ONLY` | E | TLA+/Rocq naturals do not model `u64` overflow. |
| WIR-01 | `EXEC_ONLY` | `EXEC_ONLY` | E | Exact 216-byte codec size. |
| WIR-02 | `WireLayoutIsExact` | `refined_descriptor_has_exact_wire_layout` | D | PREPARE extent. |
| WIR-03 | `WireLayoutIsExact` | `refined_descriptor_has_exact_wire_layout` | D | Index tail. |
| WIR-04 | `WireLayoutIsExact` | `refined_descriptor_has_exact_wire_layout` | D | COMMIT address. |
| WIR-05 | `WireLayoutIsExact` | `refined_descriptor_has_exact_wire_layout` | D | Final pack tail. |
| WIR-06 | `EXEC_ONLY` | `EXEC_ONLY` | E | Legacy ordinary-wire compatibility. |
| WAL-01 | `CheckpointAndIndexExactlyAdmitted` | `reachable_prepare_members_are_exact_after_write`, `reachable_index_members_are_exact_after_write` | D | Exact shared membership. |
| WAL-02 | `ReceiptBindingsAreExact` | `every_single_binding_mutation_is_rejected` | P | Concrete receipt parsing remains executable. |
| WAL-03 | `EverySingleBindingMutationIsRejected`, `ReceiptBindingsAreExact` | `every_single_binding_mutation_is_rejected` | P | All 16 abstract facts plus typed Rust boundaries. |
| WAL-04 | `EXEC_ONLY` | `EXEC_ONLY` | E | DATA/REUSE disposition is absent abstractly. |
| WAL-05 | `EXEC_ONLY` | `EXEC_ONLY` | E | Total checksummed byte parsing. |
| CAT-01 | `CrashRecoverySelectsOnlyValidAuthority` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | P | Raw prefixes/checksums are executable. |
| CAT-02 | `NewAuthorityRequiresRestartGradeGeneration`, `CatalogSelectionRequiresDurableCompleteSlot` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | P | Durable files are executable. |
| CAT-03 | `CrashRecoverySelectsOnlyValidAuthority`, `RepairableIndexRecoveryPreservesSuccessorAuthority` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | P | Physical repair/reject shapes are executable. |
| CAT-04 | `CatalogWriteUsesExactOldImage` | `EXEC_ONLY` | P | Exact 672-byte CAS is executable. |
| CAT-05 | `CrashRecoverySelectsOnlyValidAuthority` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | D | Abstract old/new/reject partition. |
| CAT-06 | `NewAuthorityRequiresRestartGradeGeneration`, `CatalogSelectionRequiresDurableCompleteSlot`, `PhysicalRecoveryRequiresCatalogDirectory` | `selected_new_has_exact_members_and_all_durable_barriers`, `physical_recovery_requires_post_catalog_directory_barrier` | D | Two-stage selection/directory contract. |
| PUB-00 | `AtMostOneMutableAuthority`, `AuthorityIsContinuous` | `reachable_authority_is_affine_and_continuous` | D | Unique initial lease. |
| PUB-01 | `AtMostOneMutableAuthority`, `AuthorityIsContinuous` | `reachable_authority_is_affine_and_continuous` | P | OS-lock lifetime is executable. |
| PUB-02 | `PhaseFollowsAffineProtocol` | `reachable_certificates_are_exact` | D | Exact phase predecessors. |
| PUB-03 | `PhaseFollowsAffineProtocol` | `reachable_certificates_are_exact` | P | Real-operation hook correspondence is executable. |
| PUB-04 | `PreflightAndCleanupPrecedeMutation`, `ResourceRefusalPrecedesAllMutation` | `refused_resource_preflight_has_no_mutation_or_new_authority` | P | Orphan/catalog bytes are executable evidence. |
| PUB-05 | `NewAuthorityRequiresRestartGradeGeneration` | `selected_new_has_exact_members_and_all_durable_barriers` | P | Filesystem barriers are executable. |
| PUB-06 | `CatalogWriteUsesExactOldImage` | `EXEC_ONLY` | P | Full-image byte comparison is executable. |
| PUB-07 | `OutcomeBoundaryIsExact` | `EXEC_ONLY` | D | Abstract outcome boundary. |
| PUB-08 | `OutcomeBoundaryIsExact`, `CrashRecoverySelectsOnlyValidAuthority` | `EXEC_ONLY` | P | Exact retry is executable. |
| PUB-09 | `OutcomeBoundaryIsExact`, `CrashRecoverySelectsOnlyValidAuthority` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | P | All 336 positional prefixes are executable. |
| PUB-10 | `OutcomeBoundaryIsExact`, `CrashRecoverySelectsOnlyValidAuthority` | `EXEC_ONLY` | D | Abstract complete-slot uncertainty. |
| PUB-11 | `OutcomeBoundaryIsExact` | `EXEC_ONLY` | D | Uncertainty starts at slot-write attempt. |
| PUB-12 | `PhysicalRecoveryRequiresCatalogDirectory` | `physical_recovery_requires_post_catalog_directory_barrier` | D | Directory barrier precedes same-process recovery. |
| PUB-13 | `AcknowledgementRequiresRestartPathReadmission`, `SemanticStateIsPreserved` | `acknowledgement_requires_recovery_readmission_and_same_semantics` | P | Concrete restart-path identity is executable. |
| PUB-14 | `OutcomeBoundaryIsExact`, `RestartCompletionPreservesOriginalOutcome` | `EXEC_ONLY` | P | Portable physical failure/repair is executable. |
| PUB-15 | `OutcomeBoundaryIsExact`, `RestartCompletionPreservesOriginalOutcome` | `EXEC_ONLY` | P | Admission-specific typed failure is executable. |
| PUB-16 | `TypeInvariant`, `OutcomeBoundaryIsExact` | `EXEC_ONLY` | P | Exhaustive failpoint coverage is executable. |
| PUB-17 | `CatalogWriteUsesExactOldImage` | `EXEC_ONLY` | P | Exact external bytes are executable. |
| PUB-18 | `DescriptorInventoriesAreExact`, `SemanticStateIsPreserved` | `acknowledgement_requires_recovery_readmission_and_same_semantics` | P | Generated DAG/report equality is executable. |
| PUB-19 | `RestartCompletionPreservesOriginalOutcome` | `EXEC_ONLY` | D | Outcome value is immutable across restart. |
| PUB-20 | `PreflightAndCleanupPrecedeMutation` | `EXEC_ONLY` | P | Physical successor evidence is executable. |
| SEM-01 | `CheckpointPreservesSemanticAccounting` | `checkpoint_publication_preserves_seeded_runtime_budget` | P | Production-adapter capability is executable. |
| SEM-02 | `PhysicalAccountingIsSeparated` | `checkpoint_publication_preserves_seeded_runtime_budget` | P | Physical-layout and future typed-ledger separation are executable. |
| SEM-03 | `SemanticStateIsPreserved`, `AcknowledgementRequiresRestartPathReadmission` | `acknowledgement_requires_recovery_readmission_and_same_semantics` | P | Canonical reports/roots are executable. |
| RES-01 | `EXEC_ONLY` | `EXEC_ONLY` | E | Parser panic-freedom and typed errors. |
| RES-02 | `PreflightRequiresExactResourceAdmission`, `ResourceRefusalPrecedesAllMutation`, `UnderprovisionedResourceCannotReachCleanup`, `ExactResourceBoundaryIsAccepted`, `ZeroDemandResourceBoundariesAreExact` | `exact_resource_boundary_is_accepted`, `required_minus_one_is_refused`, `refused_resource_preflight_has_no_mutation_or_new_authority` | P | Concrete exact requirement certificate is executable. |
| RES-03 | `EXEC_ONLY` | `EXEC_ONLY` | E | Stack/RSS/scratch/FD bounds are platform evidence. |
| NV-01 | `NeverAcknowledged` | `EXEC_ONLY` | D | Expected TLC counterexample. |
| NV-02 | `NeverRecoveredOld` | `EXEC_ONLY` | D | Expected TLC counterexample. |
| NV-03 | `NeverRecoveredNew` | `EXEC_ONLY` | D | Expected TLC counterexample. |
| NV-04 | `NeverPublishedFailure` | `EXEC_ONLY` | D | Expected TLC counterexample. |
| NV-05 | `NeverIndeterminate` | `EXEC_ONLY` | D | Expected TLC counterexample. |
| NV-06 | `NeverDurableResourceRefusal` | `required_minus_one_is_refused` | D | Durable limit refusal witness. |
| NV-07 | `NeverInvalidResourceConfiguration` | `required_minus_one_is_refused` | D | Durable constructor-minimum witness. |
| NV-08 | `NeverWalResourceRefusal` | `required_minus_one_is_refused` | D | WAL refusal witness. |
| NV-09 | `NeverAdmissionResourceRefusal` | `required_minus_one_is_refused` | D | Admission refusal witness. |
| NV-10 | `NeverDurableExactCleanup` | `exact_resource_boundary_is_accepted` | D | Durable exact-limit progress. |
| NV-11 | `NeverWalExactCleanup` | `exact_resource_boundary_is_accepted` | D | WAL exact-limit progress. |
| NV-12 | `NeverAdmissionExactCleanup` | `exact_resource_boundary_is_accepted` | D | Admission exact-limit progress. |
| NV-13 | `NeverRepairableIndexRecovery` | `EXEC_ONLY` | D | Expected TLC repair counterexample. |
| NV-14 | `NeverRejectedDamagedSuccessor` | `raw_slot_recovery_selects_old_complete_new_or_rejects` | D | Expected TLC rejection counterexample. |
| SW-01 | `TrustedSuccessorHasNoColdEvidence`, `NoPhysicalRecoveryOnTrustedSuccess` | `IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work`, `IncrementalReachability.add_before_remove_agrees_with_cold_ledger`, `RetentionV2Editor.deactivate_first_mutant_expands_the_shared_history` | P | Production refinement at geometric current sizes (task 7929; the fixed-delta trusted commit scaling section). The trusted effects realize the two TLA+ invariants. The work in each domain equals the changed-frontier oracle, which realizes the root-delta theorem in the add-before-remove order that the agreement theorem covers. The committed deactivate-first control realizes the mutant theorem in the current domain. |
| SW-02 | `EXEC_ONLY` | `RetentionV2Editor.metadata_trial_work_is_the_changed_objects`, `RetentionV2Editor.anchor_step_realizes_the_retention_entry_step`, `RetentionV2Editor.flat_manifest_mutant_rewrites_bytes_proportional_to_history` | P | Production refinement at geometric history sizes (task 7929). The metadata trial's work is the changed radix objects plus the two anchors; the new anchor and radix nodes equal the model's retention entry step byte for byte; and the V1 control's manifest grows by 56 bytes per historical entry, as the flat-manifest mutant states. |
| SW-03 | `EXEC_ONLY` | `IncrementalReachability.root_delta_execution_has_exact_changed_frontier_work`, `CheckedReachability.decrement_changes_unique_inventory` | P | Production refinement of last-reference deletion (task 7929). The current deactivation expands each object of the deleted closure once, and each object that loses its last current reference leaves the current inventory while its retained count stays positive. |
| SW-04 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 11989): the measurement harness's record contract, schedule and fail-closed probes. No model states a property of the harness; the trusted commit's work that it measures is SW-01 to SW-03's. |
| OG-06 | `CloseOwner`, `StaleIncarnationRefused` | `EXEC_ONLY` | P | Partially entailed (task 7931): `CloseOwner` fires only from `Quiescent` and leaves the owner not live, and `StaleIncarnationRefused` requires a live owner for every caller that publishes, acknowledges or applies. The writer lock, the view gate, the journals and the store files are not in the model; their preservation across close is the executable refinement obligation. |
| OG-07 | `StaleIncarnationRefused`, `NeverRefusedRetainedIncarnation` | `EXEC_ONLY` | P | Partially entailed (task 7931): `StaleIncarnationRefused` requires the caller's attachment to come from the live owner's session, and the counterexample to the witness `NeverRefusedRetainedIncarnation` reaches the refusal of a retained attachment. The model's `FreshIncarnation` is exactly one more than the closed owner's value; the refinement obligation is that the shared counter's greater value differs from every earlier incarnation at an unchanged base. |
| OG-08 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 7931): the lease count, writer lock, reads, key activity and reclamation decision of a read handle across the owner's close. It witnesses the caller registry's CA-22, `owner_teardown_preserves_retained_readers`, which this matrix does not cite, for the reason given at VR-01. |
| OG-09 | `PermitProvenance`, `NoViewMutationWhilePending` | `EXEC_ONLY` | P | Partially entailed (task 7931): `PermitProvenance` requires a pending record to hold its own view's permit, and `NoViewMutationWhilePending` requires that view's gate to stay marked while the record exists; `ForeignPermitMutant` violates the first. The guard's identity, the capture hook, the journals and the store files are not in the model; refusing another gate's guard before capture, and preserving them, is the executable refinement obligation. |
| OG-10 | `ExactBaseOrRefuse`, `NeverRefusedStaleBase` | `EXEC_ONLY` | P | Partially entailed (task 7931): `ExactBaseOrRefuse` requires a captured record's base to be the owner's root and a `refused-base` result to name an attachment whose base differs from that root, and the counterexample to the witness `NeverRefusedStaleBase` reaches that refusal; `SkipBaseMutant` violates the invariant. The catalog slots that the refusal names, the effects it leaves unchanged and the legacy repository's `RootCatalogConflict` are not in the model; they are the executable refinement obligation. |
| OG-11 | `LockOrder` | `EXEC_ONLY` | P | Partially entailed (task 7931): `LockOrder` forbids a view that holds the owner mutex from waiting to take its gate again; `RetakeGateMutant` violates it one state before the deadlock, and `RetakeGateDeadlockMutant` reaches the deadlock. That `checkpoint_view` contains no await point, and that a tokio caller holding both locks completes, are the executable refinement obligation. |
| OG-12 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 7931): the result types of the legacy facet accessors and the conversion of an unavailable facet into each caller's error type. No model states a property of Rust accessor results or error types. |
| OG-39 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 7931): the result of the RSpace constructors when the history repository refuses the store. No model states a property of Rust constructor results or panics. |
| VR-01 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 12397): the lease count, lease key activity and reclamation decision of a durable read handle across a successor and owner teardown. It witnesses the caller registry's CA-21, `pinned_generation_cannot_be_reclaimed`, and CA-22, `owner_teardown_preserves_retained_readers`, which this matrix does not cite: listing `RSpaceCallerAuthority.v` among its Rocq sources would oblige a row here for every capstone of that file. |
| VR-02 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 12398): the per-key results of a lazy durable reader against the eager enumerations of the same published state. No model states a property of the reader's Rust results, and both sides decode through the same functions. |
| VR-03 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 12398): the results of a lazy durable reader after a successor is published, after owner teardown, and on the successor's generation with the retained predecessor's roots. Like VR-01, it witnesses the caller registry's CA-22, `owner_teardown_preserves_retained_readers`, which this matrix does not cite, for the reason given at VR-01. |
| VR-07 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 12398): the literal ordered prefix of a continuation read, ruling D1. This matrix cites no model of RhoPathV1 group prefixes, and no model states a property of the reader's Rust results. |
| DI-01 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14293): equality of the durable state root across insertion orders, checkpoint schedules and reopen. No model represents the canonical encoding of a state's contents or the digest of its manifest. |
| DI-02 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14293): resolution of a digest against the admitted ledger's current and retained root counts, and the refusal of every other digest. No model states a lookup of a retained state by its manifest digest. |
| DI-03 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14293): different roots for states that differ in one endpoint. The property rests on the canonical encoding and on Blake2b-256 over it, which no model represents. |
| DI-04 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14293): the refusal of manifests that carry a legacy history root. No model represents legacy provenance. |
| SG-01 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14292): the files, catalog slot, recovered state and empty namespaces of a store that `create_empty_store_v1` creates. The publication model and its Rocq theory start from an admitted old generation (phase `OldAdmitted`, selected authority `old`), so no model creates the first generation. |
| SG-02 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14292): the empty state's manifest digest depends only on the schema, the profile and the empty root, not on the store identity. No model states a property of manifest bytes. |
| SG-03 | `EXEC_ONLY` | `EXEC_ONLY` | E | Executable-only (task 14292): crash safety of genesis at each create, write and sync point and at every torn catalog-slot prefix. The crash selection that `raw_slot_recovery_selects_old_complete_new_or_rejects` states maps a torn slot to the old authority, and a genesis directory has none: there a torn slot selects no authority, and the next genesis refuses the directory. |

## Generator discipline

1. Generate unique references as an ordered map keyed by digest; generate the manifest from a disjoint digest strategy.
2. Generate `$L$` as a dependent subset of `$R$`; never filter invalid independent pairs with `prop_assume!`.
3. Represent corruptions with typed enumerations (`InventoryMutation`, `ReceiptMutation`, `CatalogMutation`, and `FailurePoint`) so every mutation family is counted and reviewable.
4. Bias arithmetic toward boundary values and use checked oracle arithmetic.
5. Use 256 cases for pure in-memory properties, 64 for codec properties, and 16–32 for filesystem/crash properties unless measurement justifies a different bound.
6. Keep generated DAGs small but nontrivial: one to eight canonical objects and one to four retained states with deliberate shared subgraphs.
7. Never use the production builder as the oracle for its own result. Mathematical folds, independent hash construction, parsed byte extents, and reopened filesystem authority are the respective oracles.

## Verification record

The checkpoint was developed in the required refinement order. The TLA+ and Rocq models, all negative controls, and all non-vacuity witnesses were completed and checked first. The original 74 stable IDs were bound to executable targets by `PROPERTY_BINDINGS` before the production refinement changed. The registry now contains 118 IDs after the separately owned incremental-cursor extension. `model_property_manifest_is_unique_and_exhaustive` checks exact ID equality among the current catalog, derivation matrix, and binding registry, resolves every cited formal symbol, requires every base invariant and Rocq capstone to be cited, and checks every non-vacuity configuration and gate entry. The current-source manifest and all 18 focused checkpoint property tests passed on 2026-10-08.

Two implementation controls were deliberately observed red before their fixes:

1. Admission `required−1` initially reached `Published` rather than `DefinitelyNotPublished`, proving that semantic-admission limits were enforced after mutation. The admitted source now owns a derived, opaque requirements certificate. All 18 applicable semantic-admission axes, nine durable axes, and seven checkpoint-WAL axes are decided before successor cleanup; exact boundaries succeed and every positive `required−1` case preserves byte-identical old authority and seeded successor orphans. Per-state structural requirements use the componentwise maximum over current and all retained historical states.
2. A duplicated writer-lock file description initially kept the OS lock after the affine Rust lease was dropped. `DurableWriterLeaseV1::drop` now explicitly unlocks the shared open file description before descriptor destruction. This makes model invariant PUB-01 true even across the transient descriptor inheritance of a concurrent process spawn; the deterministic duplicate-descriptor control changed from red to green, and the formerly intermittent parallel publication tests became stable.

At the original 74-ID boundary, the focused executable gate passed traceability, the complete 34-axis resource-boundary matrix, and selected-successor damage/repair without old-authority fallback. The parallel `rspace_plus_plus` library gate passed **392/392** in **52.20 s** under a 10 GiB hard memory cap, zero swap, and a 12-core quota. The complete package gate passed **817/817** across every registered library and integration target in **4 min 36.055 s**, peaking at **5.4 GiB** inside a 10 GiB hard cap with zero swap and a ten-core quota. The integrated formal gate passed Rocq, Verus, Z3, TLC, and mCRL2 under explicit per-tool memory ceilings. A bounded TLC run on 2026-09-03 checked the unchanged publication model with 8,344 generated states, 7,896 distinct states, depth 22, and no invariant error under a 2 GiB Java heap and 4 GiB RSS cap. These are historical results. On 2026-10-08, the current-source focused gate compiled the Rocq module with **112** closed capstones, passed Coq kernel checking, checked the checkpoint TLA+ base invariant over **7,896** distinct states, rejected all **28** named mutants on their intended invariants, reached all **14** non-vacuity witnesses, and passed all **18** focused checkpoint property tests. This was a checkpoint-only gate, not a rerun of the full integrated Verus/Z3/mCRL2 suite. Ignored `scratch/` transcripts retain the local measurement detail; the tracked scripts, models, configurations, properties, and this catalog are the reproducible authority. The physical write/sync seam and its generated publication matrix are covered by commits `018620a20`, `92c603126`, and `8c8f570fe`.

On 2026-09-03, commit `94513d0f0` added `every_checkpoint_pack_truncation_fails_before_authority`. It exercises every shorter prefix of a valid checkpoint pack, including all byte boundaries in the header, checkpoint `PREPARE`, checkpoint `COMMIT`, and indexed object bodies. The fully qualified test passed under a `MemoryMax=4G`, zero-swap, `CPUQuota=600%` systemd scope. Commit `a6f373419` additionally verifies that the positioned-write helper resumes correctly after a positive short write. Commit `c94efc7de` adds a real durable-pack truncation test through `DurableFileV1::read_exact_at`, asserting the typed positional short-read contract after the file is physically shortened. Commit `018620a20` adds test-only failpoints at the actual positioned-write and file-sync boundaries, with a focused test proving typed path-aware failures and no file extension after a failed write. Commit `92c603126` connects those failpoints to generation publication and restart admission, proving both physical failures remain definitely unpublished and restart selects the old authority. Commit `8c8f570fe` adds both physical failpoints to the existing table-driven publication outcome matrix; the matrix passes with the expected trace prefixes and old-authority restart result. The separate exhaustive crash-property and physical fault-seam tasks are tracked as claimed done. This leaf's current-source focused formal and property gates passed on 2026-10-08; independent acceptance review remains.

The 2026-09-06 incremental cursor extension adds `IR-01` through `IR-21`; the
current catalog/derivation/binding registry contains **118** IDs. After the twenty
work/termination lemmas were compiled and independently kernel-checked, the focused
gate passed **16/16** in debug (0.09 s) and release (0.02 s): fifteen test-only
cursor-model checks plus the central traceability check. All 94 assumption reports
are closed. The original 256-case DAG property checks exact per-step potential
loss and both complementary schedules; resumed/cyclic/unused-credit/cost controls
distinguish the theorem boundaries. The remaining 635 library tests were not
selected. These results do not update the earlier full-package counts or establish
production successor refinement. See engineering journal J-065 for caps, evidence
hashes and remaining checked-resource/implementation obligations.

The 2026-10-25 ledger-trial extension adds `LT-01` through `LT-11`. The
catalog/derivation/binding registry now contains **133** IDs: the 118 above, the
four checked-counter rows `CR-01` through `CR-04`, and the eleven trial rows.
On an isolated export of the candidate tree, `IncrementalLedgerTrial.v` compiled
with all **20** capstones closed under the global context and passed independent
kernel checking. The Verus model verified **23** functions with `--no-cheating`,
and each of its three reservation mutants failed at its named postcondition. The
six trial properties, the seventeen earlier reachability properties, this registry
test and the streaming-catalog test passed **25/25** single-threaded in debug
(0.39 s) and release (0.05 s). These results cover the abstract trial and its test
model only. See engineering journal J-244
for caps, evidence hashes and the remaining production obligations.

The 2026-10-25 composition extension adds `DC-01` through `DC-08`. The
catalog/derivation/binding registry now contains **141** IDs: the 133 above and
the eight composition rows. On an isolated export of the candidate tree,
`IncrementalDomainComposition.v` compiled with all **11** capstones closed under
the global context, its trial dependency again closed all **20**, and both
passed independent kernel checking. The three composition properties, the
twenty-three earlier reachability properties, this registry test and the
streaming-catalog test passed **28/28** single-threaded in debug (0.59 s) and
release (0.09 s). These results cover the abstract composition and its test
model only. See engineering journal J-245 for caps, evidence hashes and the
remaining production obligations.

The 2026-10-26 copy-on-write ledger extension adds `CL-01` through `CL-09`. The
catalog/derivation/binding registry now contains **150** IDs: the 141 above and
the nine production-ledger rows. On an isolated export of the candidate tree,
`CreditedClosureLedger.v` compiled with all **8** capstones closed under the
global context, its trial and composition dependencies again closed all **20**
and **11**, and the publication, trial, composition and credited-ledger modules
passed independent kernel checking. All 418 selected rspace++ library tests
under `rspace::persistence` passed single-threaded in debug (19.65 s) and
release (8.46 s), with the three unwind-child helpers ignored by design. They
include the seven CL properties, the twenty-six reachability properties, this
registry test, the streaming-catalog test, and the closure, semantic, admission
and publication tests that run the credited walk. Unlike the two preceding
extensions, these results cover production code: the counted ledger, the
credited cold construction, and the trial and composition engines. See
engineering journal J-246 for caps, evidence hashes and the remaining
obligations.

The 2026-10-26 retention-format extension adds `RV-01` through `RV-19`. The
catalog/derivation/binding registry now contains **169** IDs: the 150 above and
the nineteen retention rows. On an isolated export of the candidate tree,
`RetentionV2Format.v` compiled with all **15** capstones closed under the global
context, and the module passed independent kernel checking. TLC checked
`RSpaceRetentionV2Upgrade` with `MaxEpoch = 4`: the base configuration found no
error in 349 generated and 92 distinct states at depth 7, each of the four
negative controls violated its named
invariant, and each of the three non-vacuity witnesses was reached. All
434 selected rspace++ library tests under `rspace::persistence` passed
single-threaded in debug (20.85 s) and release (8.40 s),
with the three unwind-child helpers ignored by design. They include the thirteen
format tests, the checkpoint-record test and the two catalog tests, the two
existing tests bound as `RV-13` and `RV-19`, and this registry test. These
results freeze the format and its compatibility contract; no production code
writes or reads V2 yet. See engineering journal J-247 for caps, evidence hashes
and the remaining obligations.

The 2026-10-26 cold-reader extension adds `RV-20` through `RV-29`. The
catalog/derivation/binding registry now contains **179** IDs: the 169 above and
the ten reader rows. On an isolated export of the candidate tree,
`RetentionV2Format.v` compiled with all **21** capstones closed under the global
context, and the module passed independent kernel checking; the TLA+ model did
not change. All 445 selected rspace++ library tests under
`rspace::persistence` passed single-threaded in debug (20.79 s) and
release (8.49 s), with the three unwind-child helpers ignored by
design. They include the ten reader properties, the overflow regression test,
the format, ledger, reachability, closure, semantic, admission and publication
tests, and this registry test. Unlike the format extension, these results cover
production code that reads V2: the dual dispatch, the versioned descriptor, the
two-phase radix reader and the new cold-admission steps. No production code
writes V2. See engineering journal J-248 for caps, evidence hashes and the
remaining obligations.

The 2026-10-26 writer extension adds `RV-30` through `RV-37`. The
catalog/derivation/binding registry now contains **187** IDs: the 179 above and
the eight writer rows. On an isolated export of the candidate tree,
`RetentionV2Editor.v` compiled with all **16** capstones closed under the global
context, and the module passed independent kernel checking; the TLA+ models did
not change. All 457 selected rspace++ library tests under `rspace::persistence`
passed single-threaded in debug (25.89 s) and release (9.66 s), with the three
unwind-child helpers ignored by design. They
include the eight writer properties, the four directory tests, the format,
reader, ledger, reachability, closure, semantic, admission and publication
tests, and this registry test. Unlike the reader extension, these results cover
production code that writes V2: the editor under the retention domain, the
commit and the migration, the metadata trial, the successor binding, the V2
checkpoint builder and the post-publication check. At this extension only the
test setter selected the V2 writer, and opened directories wrote V1 until the
activation commit below. See engineering journal J-249 for caps, evidence hashes
and the remaining obligations.

The 2026-10-26 writer activation makes V2 the default writer of opened
directories. It adds no ID, so the catalog/derivation/binding registry still
contains **187**, and no Rocq or TLA+ source changed. On an isolated export of
the candidate tree, all 457 selected rspace++ library tests under
`rspace::persistence` passed single-threaded in debug (31.83 s) and
release (13.57 s), with the three unwind-child helpers ignored by
design. They include `RV-28` under the V1 writer, which its test now selects,
`SI-01` to `SI-03` under both writers, the four directory tests, and every
admission, publication and recovery test that opens a directory by default and
therefore now runs the V2 writer. See engineering journal J-250 for the
activation probe, caps, evidence hashes and the release note.

The 2026-10-27 trusted successor models add `TS-01` through `TS-16`, `TS-18`
and `TS-19`. The catalog/derivation/binding registry now contains **205** IDs:
the 187 above and the eighteen trusted rows. `TrustedSuccessorCapability.v`
compiled with all **16** capstones closed under the global context, and the
module passed independent kernel checking (pgmcp artifact 1116). TLC explored
`RSpaceTrustedIncrementalPublication` from its 64 initial states, generating 664
states, **588** of them distinct, to depth 13, with the nine invariants
satisfied; each of the eight negative controls violated its named invariant, and
each of the six witnesses was reached (artifact 1117). The integrated runner's
block for the model passed again on the export of the candidate tree (artifact
1120). The extended journal owner model generated 865,820 states, **44,272** of
them distinct, to depth 21 (13,784 distinct before the extension), with the
seventeen invariants satisfied, and its seventeen causal mutants and ten
witnesses passed their designated checks (artifact 1118). On that export, all
475 selected rspace++ library tests under `rspace::persistence` passed
single-threaded in debug (45.54 s) and release (16.38 s), with the three
unwind-child helpers ignored by design (artifact 1119). They include the
eighteen trusted rows, whose transliteration reaches exactly TLC's 588 states,
and this registry test. These results cover the models and the test model only:
no production code changed, and every TS row remained a test-only extraction
until tasks 11943 to 11946 refined it.

The 2026-10-27 path-exact closure correction adds `PX-01` through `PX-05`. The
catalog/derivation/binding registry now contains **210** IDs: the 205 above and
the five closure rows. `PathExactIncrementalClosure.v` compiled with all **9**
capstones closed under the global context, and the module passed independent
kernel checking (pgmcp artifact 1124); the integrated runner's block for the
theory passed again on the export of the candidate tree. On that export, all 480
selected rspace++ library tests under `rspace::persistence` passed
single-threaded in debug (35.28 s) and release (14.89 s), with the three
unwind-child helpers ignored by design. They include the five closure rows, the
ten existing path-provenance tests, unchanged, and this registry test. The
candidate's export gate also passed: `cargo check` of all targets, clippy, all
4,418 release nextest tests of `models`, `rspace_plus_plus`, `rho-pure-eval` and
`rholang`, and the release doc tests. Unlike the trusted successor models, these
results cover a production change: the verifier consults the exact certificate
before the staging overlay. See engineering journal J-251 for the discovery, the
failed proof strategies, caps and evidence hashes.

The 2026-10-27 shape summary adds `TS-17`. The catalog/derivation/binding
registry now contains **211** IDs: the 210 above and the shape summary row.
`ExactStateShapeSummary.v` compiled with all **16** capstones closed under the
global context, its four negative controls among them, and the module passed
independent kernel checking (pgmcp artifact 1129); the integrated runner's block
for the theory passed on the export of the candidate tree. On that export, all
482 selected rspace++ library tests under `rspace::persistence` passed
single-threaded in debug (34.50 s) and release (15.37 s), with the three
unwind-child helpers ignored by design. They include `TS-17`, its companion
refusal test, the five closure rows and this registry test. `TS-17`'s generated
chains first exposed bug 11957, whose correction is recorded above, and they
pass with it. The candidate's export gate also passed: `cargo check` of all
targets, clippy, all 4,420 release nextest tests of `models`,
`rspace_plus_plus`, `rho-pure-eval` and `rholang`, and the release doc tests.
The gate's first run, on an earlier candidate, refused a catch-all arm in the
key-length histogram's equality; the arm now enumerates its residue, which
changes no outcome. Unlike the trusted successor models, these results cover a
production change: the rewrite counts the nodes it places, and durable cold
admission records the key-length multiset in its existing walk. No persisted
byte, identity or cold report changes. See pgmcp artifact 1130 for caps and
evidence hashes.

The 2026-10-27 authority core split adds no ID, so the catalog/derivation/binding
registry still contains **211** IDs, and it changes no Rocq or TLA+ source, so
no model was re-run. On an export of the candidate tree, all 486 selected
rspace++ library tests under `rspace::persistence` passed single-threaded in
debug (35.36 s) and release (16.82 s), with the three unwind-child helpers
ignored by design. They include the four authority-core tests, every existing
test unchanged, and this registry test. The candidate's export gate also
passed: `cargo check` of all targets, clippy, all 4,424 release nextest tests
of `models`, `rspace_plus_plus`, `rho-pure-eval` and `rholang`, and the release
doc tests. These results cover a production change that changes no behavior:
the durable authority is split by type into its core and its cold evidence,
and no persisted byte, identity, cold report or refusal changes. See pgmcp
artifact 1132 for caps and evidence hashes.

The 2026-10-27 trusted successor bind adds no ID, so the
catalog/derivation/binding registry still contains **211** IDs, and it changes
no Rocq or TLA+ source, so no model was re-run. On an export of the candidate
tree, all 486 selected rspace++ library tests under `rspace::persistence`
passed single-threaded in debug (68.07 s) and release (28.87 s), with the three
unwind-child helpers ignored by design. They include the nine TS test
functions with their production checks at 64 cases, every other test
unchanged, and this registry test. The fourteen TS tests took 33.79 s in debug,
nearly all of it in the production checks, which raise the suite's debug time
from 35.36 s. Each of the nine production negative controls was killed by its
row's production check (pgmcp artifact 1134). The candidate's export gate also
passed: `cargo check` of all targets, clippy, all 4,424 release nextest tests
of `models`, `rspace_plus_plus`, `rho-pure-eval` and `rholang`, and the release
doc tests. These results cover a production change: the core gains the trusted
bind, and every sealed transaction records its publication mode. No
publication called the bind until task 11946 (the trusted successor
publication section), and no persisted byte, identity, cold report or
cold-checked outcome changes. See pgmcp artifact 1135 for caps and evidence
hashes.

The 2026-10-27 trusted successor publication adds no ID, so the
catalog/derivation/binding registry still contains **211** IDs, and it changes
no Rocq or TLA+ source, so no model was re-run. On an export of the candidate
tree, all 486 selected rspace++ library tests under `rspace::persistence`
passed single-threaded in debug (86.17 s) and release (37.97 s), with the three
unwind-child helpers ignored by design. They include the fourteen TS test
functions with their production checks, the owner checks of TS-12 to TS-15, the
trusted crash check, the existing tests with their assertions unchanged, and
this registry test.
The debug suite took 18.10 s longer than in the previous commit's gate
(68.07 s). With a fixed proptest seed, the summed per-test times rose by
17.21 s, of which six test functions to which this change adds production
checks account for 14.99 s; no other test changed beyond run-to-run noise, as
the engineering journal's J-252 records. Each of the fourteen production
negative controls was killed by its row's production check (pgmcp artifact
1136). The candidate's export gate also passed: `cargo check` of all targets,
clippy, all 4,424 release nextest tests of `models`, `rspace_plus_plus`,
`rho-pure-eval` and `rholang`, and the release doc tests. These results cover a
production change: the trusted publication returns the successor's core, and
the journal owner publishes every non-empty checkpoint through it. No persisted
byte, identity or cold-checked outcome changes; the owner's accepted outcomes
change as the consensus-change register's CBR-053 continuation of 2026-10-27
records. See pgmcp artifact 1137 for caps and evidence hashes, and artifact
1138 for the timing attribution with its script.

The 2026-10-27 faulted-history models add `TH-01` through `TH-08`. The
catalog/derivation/binding registry now contains **219** IDs: the 211 above and
the eight history rows. `TrustedSuccessorCapability.v` compiled with all **20**
capstones closed under the global context, the four faulted-history capstones
among them, and the module passed independent kernel checking. On the export of
the candidate tree, TLC explored `RSpaceTrustedIncrementalHistory`, generating
19,420 states, **6,841** of them distinct, to depth 46, with its ten invariants
satisfied, and each of its ten causal mutants and eight witnesses passed its
designated check. The journal owner model with the decoy generated 1,716,908
states, **64,080** of them distinct (44,272 before), and its nineteen causal
mutants and eleven witnesses passed. The refactored single-publication model
still has **588** distinct states, and its fourteen controls passed. All 489
selected rspace++ library tests under `rspace::persistence` passed
single-threaded in debug (86.01 s) and release (43.03 s), with the three
unwind-child helpers ignored by design. They include the three TH test
functions, whose transliteration reaches exactly TLC's 6,841 states, the model
halves of TS-12 to TS-15 with the corrected owner mirror, and this registry
test. The TH tests took 1.48 s in debug and 0.31 s in release. The release suite
took 5.06 s longer than in the previous commit's gate (37.97 s), and the rest of
that rise is not attributed here; task 11969 times the suite with a fixed seed.
The candidate's export gate also passed: `cargo check` of all targets, clippy,
all 4,427 release nextest tests of `models`, `rspace_plus_plus`, `rho-pure-eval`
and `rholang`, and the release doc tests. These results cover the models and the
test model only: no production code changed, and every TH row remains a
test-only extraction until tasks 11967 and 11968 add its production half. See
engineering journal J-253 and pgmcp artifact 1141 for caps and evidence hashes.

The 2026-10-28 fix of bug 11970 adds `RI-01` through `RI-04`. The
catalog/derivation/binding registry now contains **223** IDs: the 219 above and
the four reclamation-intent rows. On the export of the candidate tree, the
model's runner checked the extended `RSpaceGcPendingReclaimIntent`: its
baseline generated 4,533 states, **1,700** of them distinct (1,683 before
`TearIntent`), with no invariant error, each of its five mutants violated its
named invariant, and both witnesses were reached. All 493 selected rspace++
library tests under `rspace::persistence` passed single-threaded in debug
(90.24 s) and release (43.14 s), with the three unwind children and the
trusted-history kill child ignored by design. They include the four RI test
functions, the five reclamation-intent cases of PUB-16, the existing tests with
their assertions unchanged, and this registry test. The RI tests took 1.13 s in
debug and 0.69 s in release. On the in-place writer, RI-01, RI-02, RI-03 and
PUB-16 had failed as predicted, and two code mutations, a rename that replaces
and a recovery that keeps the temporary, were each killed by their named tests.
The candidate's export gate also passed: `cargo check` of all targets, clippy,
all 4,431 release nextest tests of `models`, `rspace_plus_plus`,
`rho-pure-eval` and `rholang`, and the release doc tests. These results cover a
production change: the reclamation intent is published through a synchronized
temporary and a rename that never replaces an existing record, and recovery
first removes a leftover temporary. The record under the final name keeps its
bytes, and a corrupt complete record still fails closed; the consensus-change
register's CBR-053 living continuation of 2026-10-28 records the changed
outcomes of the dormant store. See engineering journal J-253 and pgmcp artifact
1150 for caps and evidence hashes.

The 2026-10-28 production controls and timing of task 11969 add no ID, so the
catalog/derivation/binding registry still contains **223** IDs, and they
change no source or model. On exports of the candidate tree, each of the nine
production negative controls was killed by its named TH test, and TS-11, the
trusted publication's crash and retry test, survived five of them, which only
the TH tests kill. Seeded and pinned to one CPU, the persistence selection's
summed per-test time rose by 16.03 s in debug and 8.20 s in release against
the tree before task 7928, all of it in the seven test functions that task
7928 added. No existing test changed measurably, so the release rise of
5.06 s that the 2026-10-27 faulted-history models paragraph above leaves
unattributed was not a lasting change in the existing tests (an inference:
that rise was measured unseeded and unpinned, at an intermediate tree). The
history, journal owner, reclamation-intent and single-publication models
reproduced their recorded state counts with every negative control and
witness, and `TrustedSuccessorCapability.v` again closed its **20** capstones
and passed the kernel check. The workspace gate also passed on the export of
the candidate with this task's documentation in draft: `cargo check` of all
targets, clippy, all 4,431 release nextest tests of `models`,
`rspace_plus_plus`, `rho-pure-eval` and `rholang`, and the release doc tests.
These results add evidence only: no production code, persisted byte or outcome
changed. See the faulted-history production controls and timing section,
engineering journal J-253 and pgmcp artifact 1154 for caps and evidence
hashes.

The 2026-10-29 fixed-delta trusted commit scaling properties of task 11988 add
SW-01, SW-02 and SW-03, so the catalog/derivation/binding registry now
contains **226** IDs. On an export of the candidate tree, the three tests
passed in 3.46 s in debug and in 1.76 s in release with
`PROPTEST_RNG_SEED=20261029`, and they printed the same values in both builds.
At every commit of every cell, the work of the three domain trials equaled the
changed-frontier oracle and the rest of the verify routine held. At 16, 64, 256
and 1,024 keys, the smallest deactivate-first excess was 26, 122, 530 and 2,162
expansions, while the largest retained frontier of a commit was 6, 10, 10 and
14 objects. At 16, 64, 256 and 1,024 historical states, the V1 manifest wrote
exactly $`96 + 56n`$ bytes per commit for $`n`$ historical entries, from 1,048
to 57,608, while the V2 step wrote between 545 and 3,003 bytes. The V2 step was largest at 64 states. It
rewrites the radix node that receives the new generation's key, and since the
key leads with the big-endian generation, that node holds one child for each
retained generation that shares the new one's high-order bytes: about the
generation modulo 256, whatever the history size (derived from the key
layout). The seeded persistence selection passed with 496 tests in debug
(95.92 s) and in release (44.61 s), and the manifest test passed. The workspace
gate also passed on the export: `cargo check` of all targets, clippy, all
4,434 release nextest tests of `models`, `rspace_plus_plus`,
`rho-pure-eval` and `rholang`, and the release doc tests. These results add
evidence only: no production code, persisted byte or outcome changed. See the
fixed-delta trusted commit scaling section and pgmcp artifact 1156 for
caps and evidence hashes.

The 2026-10-29 fixed-delta scaling harness and driver of task 11989 add SW-04,
so the catalog/derivation/binding registry now contains **227** IDs. On an
export of the candidate tree, SW-01 to SW-04 passed in 3.90 s in debug and in
1.52 s in release with `PROPTEST_RNG_SEED=20261029`, and SW-01 to SW-03
printed the same values as under task 11988 in both builds. The seeded
persistence selection passed with 497 tests in debug (98.99 s) and in release
(44.67 s), and the manifest test passed. `scale_child` is ignored by design,
like the three unwind children and the trusted-history kill child, because
only a parent process runs it. The driver's self-test passed in 15.9 s and
refused two mutants of the driver: one that creates a scratch directory with
mode 0777, and one whose child-asserted gate conditions pass on an incomplete
verify run. A hand run of the E cell followed the record contract: the verify
child printed seven records labeled in schedule order, only the reclaim commit
unlinked files (the superseded pack and index and `reclaim.v1`), the I/O
calibration was 2 read calls and 105 bytes, and the time child's chain of
targets equaled the verify child's. A V1 child with $`H = 16`$ historical
states wrote exactly $`96 + 56(H + c + 1)`$ metadata bytes at each commit
$`c`$ from 0 to 5, from 1,048 to 1,328. The workspace gate also passed on the
export: `cargo check` of all targets, clippy, all 4,435 release
nextest tests of `models`, `rspace_plus_plus`, `rho-pure-eval` and `rholang`,
and the release doc tests. These results add tooling and evidence only: no
production code, persisted byte or outcome changed, and the section still
claims no scaling result. See the fixed-delta trusted commit scaling section
and pgmcp artifact 1158 for caps and evidence hashes.

The 2026-11-01 fallible legacy facet slice of task 7931 adds OG-12, the first
OG row, so the catalog/derivation/binding registry now contains **228** IDs. On
an export of the candidate tree, OG-12 and the two wiring tests passed in under
0.01 s in debug and in release. Four production controls each made exactly one
of the three tests fail: a conversion into `HistoryError::ActionError`, and a
`heed::Error` whose source is the mismatch's message rather than the mismatch,
each failed OG-12; an exporter refusal retyped as `InvalidPath` and an importer
refusal retyped as `InternalInvariant` each failed that store's wiring test.
The library tests passed with 893 tests in release (10 ignored), the `mod`
target with 179, and every rspace++ test target with 1,453 tests in 28 binaries
(13 ignored); the manifest test passed. `cargo check` of every workspace target
passed, with eight warnings that all lie in files this slice does not touch,
and clippy passed on `rspace_plus_plus` with warnings denied. The affine-import
and atomic-checkpoint obligation checkers pin assertion text from two of the
edited tests, the malformed-page import test and the root-visibility helper, so
their pinned fragments now include the facet's `expect`, and both registries
were regenerated. No persisted byte, root, identity or wire message changed:
the in-memory and LMDB backends serve every facet as before. See the fallible
legacy state-authority facets section and pgmcp artifacts 1565 and 1566 for
the gate and measurement logs.

The 2026-11-01 constructor slice of task 7931 adds OG-39, so the
catalog/derivation/binding registry now contains **229** IDs. On an export of
the candidate tree, OG-39 and the 27 other atomic-state-authority tests passed
in release and in debug. Two production controls each made OG-39, and only
OG-39, fail among those 28 tests: the `.unwrap()` restored in `RSpace::create`,
and the `.unwrap()` restored in `create_with_replay`. In each, the constructor
panicked instead of returning the refusal. The library tests passed with 894
tests in release (10 ignored), the `mod` target with 179, and every rspace++
test target with 1,454 tests in 28 binaries (13 ignored); the manifest test
passed. `cargo check` of every workspace target passed, with the same eight
warnings as before, all in files this slice does not touch, and clippy passed
on `rspace_plus_plus` with warnings denied. The atomic-checkpoint and
workspace-regression registries hash the edited test file, so both were
regenerated, and the PathMap aggregate takes their new manifest digests. No
successful construction, persisted byte, root, identity or wire message
changed. See the fallible RSpace constructors section and pgmcp artifacts 1572
and 1573 for the gate and measurement logs.

The 2026-11-01 durable read handle slice of task 12396 adds VR-01, the first VR
row, so the catalog/derivation/binding registry now contains **230** IDs. On
an export of the candidate tree, VR-01, the 20 durable owner tests, the 21
trusted-chain tests and the two shape-summary tests passed in release, and
VR-01 and the owner tests passed in debug. One production control released
the handle's lease as soon as it was acquired. It made VR-01, and only VR-01,
fail among the 128 tests of the durable persistence modules, with the
observations `(0, false, true)` against the required `(1, true, false)`: no
added lease, the old key no longer active, and the old pack range retirable.
The library tests passed with 895 tests in release (10 ignored), the `mod`
target with 179, and every rspace++ test target with 1,455 tests in 28
binaries (13 ignored); the manifest test passed. `cargo check` of every
workspace target passed, with the same eight warnings as before, all in files
this slice does not touch, and clippy passed on `rspace_plus_plus` with
warnings denied. The workspace-regression registry hashes the manifest test's
file and the persistence test module, so it was regenerated, and the PathMap
aggregate takes its new manifest digest. No persisted byte, root, identity or
wire message changed. See the generation-leased durable read handle section
and pgmcp artifacts 1576 and 1577 for the gate and measurement logs.

The 2026-11-01 owner incarnation slice of task 7931 adds OG-06, OG-07 and
OG-08, so the catalog/derivation/binding registry now contains **233** IDs. On
an export of the candidate tree, the five owner-incarnation tests and the 20
durable owner tests passed in release and in debug, and the 128 tests of the
durable persistence modules and the 21 trusted-chain tests passed in release.
Four production controls each made exactly one of the 133 tests of the
owner-incarnation and durable persistence modules fail, as predicted:

- A counter of its own for each owner gave every owner incarnation 0. OG-07
  then observed every earlier attachment accepted and incarnations that do not
  increase.
- `validate_attachment` accepting every attachment on a closed owner made
  OG-06 observe `Ok(())` where `OwnerClosed` is required, with every other
  observation unchanged.
- `close` leaking one lease on its generation made OG-08 observe
  `(2, 2, true, true, true, false, 1, false)` against the required
  `(2, 1, true, true, true, false, 0, true)`: two leases after close, one
  after the handle is dropped, and a range that cannot be retired.
- `checkpoint` without its closed-owner check took the view gate, captured the
  journals into a pending record and refused with `Closed` only at
  publication. A closed owner reports the `Closed` phase, not `Captured`, so
  the refusal kept the record and the gate permit it holds: OG-06 observed the
  gate held, with every other observation unchanged.

The library tests passed with 900 tests in release (10 ignored), the `mod`
target with 179, and every rspace++ test target with 1,460 tests in 28
binaries (13 ignored); the manifest test passed. `cargo check` of every
workspace target passed, with the same eight warnings as before, all in files
this slice does not touch, and clippy passed on `rspace_plus_plus` with
warnings denied. An earlier gate of this slice found two
`clippy::result_large_err` warnings in the new test file, because the
stale-base refusal carried two catalog slots, at least 496 bytes; that refusal
now boxes them, and clippy of every rspace++ test target lists the same 127
findings as before this slice, none new. The workspace-regression registry
hashes the manifest test's file and the persistence test module, so it was
regenerated, and the PathMap aggregate takes its new manifest digest. No
persisted byte, root, identity or wire message changed. See the durable owner
incarnations and close section and pgmcp artifacts 1604 and 1607 for the gate
and measurement logs, 1608 for the locations of the check's warnings, and 1605
and 1606 for the earlier gate and its full clippy output.

The 2026-11-01 owner checkpoint view slice of task 7931 adds OG-09, OG-10 and
OG-11, so the catalog/derivation/binding registry now contains **236** IDs. On
an export of the candidate tree, the four tests of the new file, the five
owner-incarnation tests and the 20 durable owner tests passed in release and
in debug, and the 128 tests of the durable persistence modules, the 21
trusted-chain tests and the owner-history test of TH-08 passed in release.
Four production controls were each applied to the export, and each made
exactly the predicted tests fail among the 137 tests of the new file, the
owner-incarnation module and the durable persistence modules:

- The owner taking the view gate again made 26 fail: the four tests of the new
  file, the 20 durable owner tests, VR-01 and the V2 durable publication test.
  Every checkpoint except OG-11's was still pending at its first poll, and
  OG-11 observed `(None, None, Ready, Some(7), true, true)` against the
  required `(Some(Some(8)), Some((true, true)), Ready, Some(8), true, false)`:
  no completion within ten seconds, no capture and nothing published.
- `checkpoint_view` without check 1 made OG-09, and only OG-09, fail. Its
  ready owner published generation 8 through all thirteen publication hooks
  under the foreign permit, and its pending owner refused with
  `Unresolved(RecoveryRequired)` instead of `ForeignPermit`.
- `validate_attachment` without its base comparison made OG-10, and only
  OG-10, fail. The stale attachment was accepted, and the owner published
  generation 9 from its current base.
- `checkpoint_view` without check 5 made the unbound test, and only it, fail.
  The other gate's permit captured again over the pending publication, which
  lost its retained terminal and freed the view gate.

The export was then restored to the candidate blobs, and the 137 tests passed
again. The library tests passed with 904 tests in release (10 ignored), the
`mod` target with 179, and every rspace++ test target with 1,464 tests in 28
binaries (13 ignored); the manifest test passed. `cargo check` of every
workspace target passed, with the same eight warnings as before, all in files
this slice does not touch, and clippy passed on `rspace_plus_plus` with
warnings denied; clippy of every rspace++ test target lists the same 127
findings as before this slice, none new. The 17 obligation checkers passed
with `--check` and `--self-test`, and the PathMap aggregate with `--check`.
The workspace-regression registry hashes the manifest test's file and the
persistence test module, so it was regenerated, and the PathMap aggregate
takes its new manifest digest. No persisted byte, root, identity or wire
message changed. See the durable owner checkpoint views section and pgmcp
artifacts 1610 and 1611 for the gate and measurement logs, and 1612 for the
full output of the first control.

The 2026-11-11 lazy durable view reader slice of task 12398 adds VR-02, VR-03
and VR-07, so the catalog/derivation/binding registry now contains **239**
IDs. In a worker copy of the candidate tree, the five tests of the new module
passed in debug and in release, and so did VR-01, the 20 durable owner tests,
the 17 durable startup tests and the manifest test. VR-02 checked the three
seeded states and 64 generated ones, and the seeded states alone covered
every case that the test requires. Three production faults were each applied
to the worker copy, run against the new module and removed, and each made
the predicted tests fail:

- The reader reading the roots of its handle's own generation instead of the
  roots it was given made VR-03, and only VR-03, fail among the five tests: a
  reader on the successor's generation, given the predecessor's roots, did
  not return the predecessor's state.
- Sorting a group's channels before its prefix is built made VR-07 fail,
  because reading `(b,a)` with records under `(a,b)` only returned the
  `(a,b)` continuation, and made VR-03 fail at its first comparison, because
  its pinned state holds a `(b,a)` record. The corrupted-source and limit
  tests passed. VR-02 was skipped in this run, so that its property runner
  would not persist a regression file into the tree.
- The reader's continuation read answering an empty `Vec` on any error, as
  `ClosedDurableHistoryReader` does, instead of panicking made the
  corrupted-source and limit tests, and only them, fail: the reader returned
  an empty `Vec` for the corrupted group, and an empty result with a step
  budget one below the requirement.

Each faulted file was restored and compared byte for byte with its saved
copy. A temporary test, run once in debug and once in release and then
removed, showed the overflow of the expansion count that task 14335 later
made a typed limit error, as described in the lazy durable view reader
section. Clippy of `rspace_plus_plus` with every target and warnings denied
lists no finding in the files of this slice, and `rustfmt --check` passes on
its Rust files. No persisted byte, root, identity or wire message changed.

The 2026-11-11 durable state root slice of task 7931 (task 14293) adds DI-01
to DI-04, the first DI rows, so the catalog/derivation/binding registry now
contains **243** IDs. On an export of the candidate tree, which already holds
the lazy durable view reader slice, the eight tests of the new file, the five
tests of the lazy reader, VR-01, the 20 durable owner tests and the 17 durable
startup tests (each family has one more, ignored, test) and the manifest test
passed in debug and in release, and the whole library test target passed with
923 tests in both profiles (10 ignored). One production control removed the
ledger probe from resolution and decided by object-index presence instead. It
made exactly two of the eight tests fail. DI-02 reported the unretained
checkpoint manifest as resolved, while the other six candidates stayed
refused. The unbound measurement test observed its own unretained manifest
resolve after one object read, against the required refusal with no read. The
resolver was then restored from the candidate tree, and the eight tests
passed again. Clippy of `rspace_plus_plus` with every target lists no finding
in the files of this slice, and `rustfmt --check` passes on its Rust files.
The workspace-regression registry hashes the manifest test's file and the
persistence test module, both of which change, so it was regenerated, and the
PathMap aggregate takes its new manifest digest. No persisted byte, record
kind, root, identity or wire message changed.

The 2026-11-11 empty durable store genesis slice of task 14292 adds SG-01,
SG-02 and SG-03, the first SG rows, so the catalog/derivation/binding registry
now contains **246** IDs. On an export of the candidate tree, which already
holds the lazy durable view reader and durable state root slices, the five
tests of the new file passed in debug and in release, and so did the durable
persistence modules, the checkpoint WAL module and the generation-checkpoint
property module with the manifest test; the whole library test target passed
with 928 tests in both profiles (10 ignored). In a worker copy with temporary
prints, which were removed afterwards and the file compared byte for byte, a
traced genesis reached the 14 points of the table in order. Crashes at the
first 12 left a directory that does not open, and crashes at the last two a
store that opens with the genesis state; of the 336 torn slot prefixes, 332
did not open, and the four of 332 to 335 bytes opened with the genesis state.
Three production controls were each applied to the export and run against the
five tests of the new file:

- Omitting the retention anchor, that is neither staging the anchor and the
  empty radix node nor admitting and seeding the metadata closure, made all
  five fail at their first genesis, before the directory is opened, with
  `Checkpoint(RetentionMetadataBelowMinimum { data_objects: 2,
  retained_objects: 2, data_bytes: 259, retained_bytes: 259 })`. The V2
  inventory rule refuses a checkpoint without its anchor, so such a store is
  never written, and SG-01 fails at construction rather than at admission.
- Counting the anchor but leaving its bytes out of the empty-state source
  made all five fail the same way, with `Canonical(MissingObject(..))` for the
  anchor's digest from the PREPARE writer.
- Writing the anchor but appointing the manifest in its place in the catalog
  slot made SG-01, SG-02, SG-03 and the foreign-names test fail at their first
  reopen through `recover_read_authority`, which refused with
  `Startup(CatalogVerification(Catalog(CatalogCommitPublication { .. })))`:
  the catalog's publication names the 209-byte manifest where the COMMIT's
  names the 136-byte anchor. The refusal test, which never reopens, passed.

The export was then restored, and the five tests passed again. Clippy of
`rspace_plus_plus` with every target lists no finding in the files of this
slice, and `rustfmt --check` passes on its Rust files. The
workspace-regression registry hashes the manifest test's file and the
persistence test module, both of which change, so it was regenerated, and the
PathMap aggregate takes its new manifest digest. No persisted record kind,
byte layout, magic number or file name is new. See the empty durable store
genesis section.

## Caller-authority contract (2026-09-06)

**Status: verified abstract contract; legacy caller observations exercised; durable
caller refinement remains required in tasks 7931 and 7932.** Task 7930 owns this
section and `RSpaceCallerAuthority.v`, not a second publication implementation.
The existing publication, journal-owner, affine-import and record/replay models
remain authoritative for their respective transition protocols.

### Source-backed caller inventory

An *authority* selects one backend and owns its current publication and authenticated
metadata. A *view* carries a particular readable history plus mutable hot state.
A historical state identity, publication generation, physical storage generation,
and owner incarnation are different coordinates; equality of one cannot substitute
for another.

| Caller boundary | Current source and observation | Required durable refinement |
|---|---|---|
| Current and known roots | `history/history_repository.rs` and `state/rspace_state_authority.rs`: exact `CurrentRoot`; private known-root capability bound to the authority. `HistoryRepository::root()` is view-local. | One backend-discriminated authority; preserve explicit view-local versus backend-current distinction. Never consult an independently current legacy repository. |
| Hard checkpoint and reset | `rspace.rs::create_checkpoint/reset`, `replay_rspace.rs`, `history_repository_impl.rs::checkpoint_at/reset`. Reset activates a known root at a fresh publication generation; no-op checkpoint preserves generation. | Consume the existing exact-base journal-owner protocol. Historical reset semantically admits the selected target before publishing a fresh generation. Reuse the existing no-op and ABA proofs/tests. |
| Clear | `RSpace::clear` and replay clear reset to the legacy empty-root identity. | Resolve the agreed empty-state bridge under the selected authority; never manufacture an empty legacy history as a durable fallback. Root/epoch policy is task 7941's contract, not silently selected here. |
| Spawn and runtime pools | `rspace.rs::spawn`, `replay_rspace.rs::spawn`, and `casper/src/rust/util/rholang/runtime_manager.rs::spawn_runtime/spawn_replay_runtime`. Spawn calls reset on the source view's root and therefore advances shared publication generation. Its new hot view currently has a separate gate. | Model full spawn as historical activation then view attachment. Reuse the one writer/commit owner; do not acquire an independent writer lease. Attachment preserves owner identity and source history, but full spawn does not preserve the previous publication generation. |
| Soft capture and restoration | `checkpoint.rs::SoftCheckpoint`, both space implementations, `reporting_rspace.rs`, and `rho_runtime.rs`. Cache, log, produce counter and five journal domains are captured. Restore uses the receiving view's current history, not the capture-time durable root. | Transferable retained payload is data, not mutation authority. Fresh receiving-owner permission, an authenticated receiving-view reader and complete journal refinement are required. No public same-base or same-generation precondition is added. |
| Replay and reporting | Casper `rholang/runtime.rs`, `replay_runtime.rs`, and `ReportingRSpace` delegate reset/checkpoint/soft operations. `RuntimeOps::with_soft_transaction` rolls back the explicit false result, but its `?` error return does not itself promise rollback. | Preserve actual error, replay-log, COMM and post-state behavior. Historical target equality alone does not authorize replay against a stale base. |
| Readers, exports and imports | `HistoryRepository::get_history_reader`, its legacy-concrete `get_history_reader_struct`, `exporter/importer`, and the state-manager APIs. | Replace the required concrete legacy seam; reader/export snapshots pin their exact objects. Imports use the existing affine seal/admission/publication protocol; an export is not current-state activation. |
| Installs and teardown | Space constructors, `install`, checkpoint/reset install restoration, and escapable `get_store()` handles. | Preserve installs in the new hot view. Close revokes mutable authority across retained aliases; immutable readers with their own leases remain readable. A retained hot handle must not mutate after whole-owner teardown. |
| Node startup and CLI/REPL | `node/src/rust/runtime/setup.rs` creates eval, play/replay, reporting and state-manager paths. `api/repl_grpc_service.rs::evaluate` uses the storage printer; `rho_runtime.rs` exposes `to_map`. | All constructors select the same appropriate backend per store. Existing commands and RSpace printing remain supported; no unreviewed root/API format change. |
| FFI | Active functions in `rholang/src/lib.rs` serialize/restore soft payloads and expose hard checkpoint/reset/map observations. `rspace++/libs/rspace_rhotypes/src/lib.rs` exposes reset/spawn/history/export/import and replay checkpoint paths. | Audit active ABI paths, not commented-out functions. Snapshot bytes contain no owner-incarnation authority token. Keep owned return-buffer lifetimes and existing supported behavior; versioning requires the separate identity decision. |

RSpace-relative paths in the table are under `rspace++/src/rspace/`.
This inventory is bounded to the persistence campaign; it is not authorization
for unrelated Casper or contract rewrites.

### Exact contract and realization premises

**DERIVED.** Durable admission currently verifies structural closure and inventory
for retained historical states, but calls semantic admission only for the selected
current state. Retention membership therefore cannot mint a historical reset
capability by itself. `admit_historical_root` consults the selected owner's retained catalog and
requires an independent semantic witness. Activation preserves backend identity
and advances publication generation, rather than restoring a historical generation.

**DECISION.** The model's owner is an already authenticated capability. Its retained
roots and alias bindings are authoritative metadata, not caller-provided evidence.
Alias resolution requires exact owner-appointed bindings, domain equality and
one unambiguous result. An unchanged authenticated alias catalog need not be
reissued merely because publication generation advances. An inherited manifest
legacy root is provenance; it does not rename every successor. Alias creation,
cryptographic verification, transactionally publishing mappings, and any epoch/API
selection remain the root-identity and migration tasks.

**DERIVED.** A cached empty channel and a channel not in the captured cache differ.
For a captured cache, restoring the former hides receiving-history values; restoring
the latter reads the receiving history. The model retains arbitrary value/log/counter
types, and its five-journal record keeps data, continuations, content transitions,
source bytes and joins distinct.

The count-rebase subproof uses base count $`b`$, target count $`t`$, addition
$`a`$ and removal $`r`$:

```math
a = \max(t-b, 0), \qquad r = \max(b-t, 0), \qquad (b-r)+a=t.
```

This is a **pointwise arithmetic lemma**, not a production full-store scan or
proof of complete cross-base journal reconstruction. Reusing an addition of one
captured over base one against a new base three yields four, not the intended two.
The runtime refinement must preserve the complete captured-cache/current-reader
overlay and all source/continuation metadata using the existing native trie and
journal machinery. It must not publish old-base deltas merely because their bytes
were captured legitimately.

The following are explicit realization premises, not proved implementation facts:

- The observable-incarnation set contains every still-observable old mutation
  capability. The allocator must guarantee freshness against that entire set,
  including repeated close/reopen histories; inequality with only the previous
  incarnation is insufficient. Naturals are ghost identities here, not a new
  wrapping integer allocator.
- The owner appointment and receiving-view history are authenticated. The receiving
  view may retain a historical reader; it is not forced to equal the backend's
  most recent publication. Checkpointing such a view still needs its own exact-base
  validation/reconciliation.
- `complete_journal_refinement` relates all five journals to that receiving history.
  The conditional restoration theorem does not prove this relation is inhabited
  for every concrete payload. Task 7931 must establish it before runtime cutover,
  including a justified same-base fast path and supported cross-base cases.
- The immutable reader owns its exact physical-generation lease. The proof keeps
  the lease set unchanged during owner close; actual Rust retention and release
  across teardown are task 7932's executable obligation.
- The model assumes the already verified publication/acknowledgement transition is
  atomic at this boundary. It does not replace WAL write/sync/recovery proofs,
  prove Rust extraction, solve generation overflow, or authorize a second writer.

The model uses lists for finite mathematical catalogs, not a prescription to turn
PathMap state into vectors. No format, storage layout, consensus root, production
algorithm or dependency is changed by this proof/test checkpoint.

### Exhaustive caller capstone extraction

All 30 exported `Print Assumptions` capstones are listed below. **Contract** means
the abstract lemma and controls pass; **legacy witness** means an actual pre-cutover
caller test passes. Neither means the planned durable caller implementation is
already verified. New caller IDs are separate from the existing publication
registry and do not relabel its 118 verified bindings.

| ID | Formal capstone in `RSpaceCallerAuthority` | Executable binding or remaining required obligation |
|---|---|---|
| CA-01 | `permit_requires_exact_authority` | Existing `prepared_checkpoint_cannot_cross_an_authority_boundary`; 7931/7932 additionally vary backend, profile, epoch and incarnation in actual caller routes. |
| CA-02 | `not_ready_refuses_mutation` | Existing durable-owner nonready/refusal/cancellation tests; 7932 must reach the same gate from every mutable caller. |
| CA-03 | `closed_owner_revokes_every_mutation_permit` | Contract; 7932 closes the whole owner with each old HotStore/intent/receipt alias retained and requires nonmutation. |
| CA-04 | `reopened_owner_rejects_old_incarnation` | Contract; 7932 repeats stale-permit attempts after reopening the same root/generation. |
| CA-05 | `fresh_permit_requires_live_ready_owner` | Existing owner constructor/readiness tests are substrate evidence; 7931/7932 bind actual permit issuance. |
| CA-06 | `reopen_rejects_every_observable_incarnation` | Repeated-incarnation ABA control; 7932 varies retained aliases over multiple close/reopen cycles. |
| CA-07 | `spawn_attachment_reuses_owner_and_preserves_view_history` | Legacy witness `spawn_reactivates_the_source_view_root_on_the_shared_authority`; 7931/7932 must not mint a second writer. |
| CA-08 | `publication_invalidates_all_old_view_permits` | Existing `same_generation_plans_conflict_without_partial_visibility`; 7932 exercises spawned/shared caller views. |
| CA-09 | `root_aba_does_not_revive_a_permit` | Existing `aba_root_reactivation_does_not_revive_an_old_plan`; 7932 repeats through durable caller APIs. |
| CA-10 | `retained_membership_does_not_skip_semantic_admission` | Retained-but-refused control; 7931/7932 require a structurally retained, semantically invalid historical target to refuse before activation. |
| CA-11 | `unknown_target_cannot_be_admitted` | Existing missing-root rejection; 7932 verifies no empty/current/legacy fallback through each caller. |
| CA-12 | `activation_requires_exact_semantically_admitted_target` | Existing known-root authority isolation; 7931/7932 bind exact historical semantic evidence, domain and still-retained membership. |
| CA-13 | `historical_activation_has_fresh_publication_generation` | Existing `target_equal_replay_requires_reactivation_and_reuses_exact_object_closure` and LMDB reopen/known-root tests. |
| CA-14 | `activation_is_deterministic` | Contract; 7932 compares actual reset result/current catalog for identical authenticated requests. |
| CA-15 | `full_spawn_activates_history_before_attaching` | Legacy witness `spawn_reactivates_the_source_view_root_on_the_shared_authority`; 7932 extends to play/replay/reporting and retained source histories. |
| CA-16 | `alias_resolution_is_exact_and_unambiguous` | Contract; identity/migration tasks supply canonical single-valued mapping; 7932 varies exact and duplicate mappings. |
| CA-17 | `missing_or_ambiguous_alias_refuses` | Missing/duplicate controls; 7932 rejects before mutation without provenance fallback. |
| CA-18 | `foreign_catalog_cannot_resolve` | Foreign-domain control; 7932 varies store/profile/epoch at an actual alias caller. |
| CA-19 | `unappointed_alias_catalog_cannot_resolve` | Same-domain forged-catalog control; 7932 rejects a stale or unauthenticated mapping despite an apparently valid target. |
| CA-20 | `provenance_is_not_an_alias_certificate` | Inherited/absent-alias controls; identity task must keep successor identity separate from migration provenance. |
| CA-21 | `pinned_generation_cannot_be_reclaimed` | Existing generation-lease substrate; 7932 retains old reader/export handles through replacement and reclamation. |
| CA-22 | `owner_teardown_preserves_retained_readers` | Contract; 7932 closes owner, rejects writes, and reads the exact old leased generation until last-reader release. |
| CA-23 | `every_legacy_overlay_with_refined_journals_has_a_permitted_restore` | New `soft_restore_caller_overlay_matches_legacy_observations` covers legacy observations; 7931 must prove complete-journal existence, not simply add a new refusal. |
| CA-24 | `soft_restore_preserves_durable_authority_log_and_counter` | New cross-hard and transferred-payload tests compare exact current authority, restored log and counter. |
| CA-25 | `soft_restore_requires_receiving_history_and_complete_journals` | Foreign-history/missing-journal controls; 7931 binds all five domains and 7932 tests subsequent checkpoint/reopen, not just immediate reads. |
| CA-26 | `captured_values_override_current_history` | Cross-hard and transferred-payload fixtures plus generated multiplicities, including captured empty. |
| CA-27 | `uncaptured_values_use_receiving_history` | The same fixtures require uncaptured channels to read the receiving current history rather than captured history. |
| CA-28 | `payload_transfer_never_transfers_mutation_authority` | Old-permit restore control; 7932 validates fresh receiver permits and rejects stale writer/receipt resurrection. |
| CA-29 | `rebased_journal_reconstructs_exact_restored_count` | `caller_contract_count_rebase_is_exact_over_full_u64_domain` checks the scalar refinement; complete trie/journal reconstruction remains 7931. |
| CA-30 | `rebased_journal_has_no_underflow_or_intermediate_overflow` | The same arbitrary-u64 property uses checked subtraction/addition; production checked arithmetic and all-domain reconstruction remain mandatory. |

The twenty controls are in `RSpaceCallerAuthorityMutants.v`; the normal proof
runner requires exactly 30 closed reports for the model and 20 for the controls,
then independent `coqchk` success and placeholder rejection. The controls include
root ABA, owner reopen ABA, wrong backend/store/profile/epoch, nonready/closed
owners, retained-but-invalid/unknown targets, missing/ambiguous/forged aliases,
provenance confusion, pinned reclamation, empty/missing cache, cross-base payload
transfer, stale write permits, foreign history, dropped journals and wrong-base
count deltas. The fixture journal relation is exact equality in a synthetic
five-field record, not a proof that copying captured journals works across bases.

The concrete legacy tests are in `rspace++/tests/storage_actions_test.rs`; the
atomic-root and durable-owner substrate tests remain in their existing files.
No alternate in-memory RSpace test implementation was introduced. The complete
durable lifecycle gates and full-workspace CI remain required after implementation.

## Preserved legacy consensus-identity contract (2026-09-07)

**Status: policy selected and abstract contract verified; concrete bridge construction,
caller refinement and activation remain required.** Task 7941 owns this contract.
It selects preserved legacy history/post-state identity, not direct semantic-manifest
identity or a new consensus epoch. Tasks 7942 and 7931 implement and qualify the bridge.

### Evidence and the rejected projection hypothesis

The legacy group hash sorts individual channel hashes, while RhoPath continuation
prefixes preserve ordered channel identities. The relevant sources are
[`stable_hash_provider.rs`](../../../rspace++/src/rspace/hashing/stable_hash_provider.rs),
[`rho_path.rs`](../../../rspace++/src/rspace/rho_path.rs),
[`record_index.rs`](../../../rspace++/src/rspace/record_index.rs), and the cold lookup in
[`rspace_history_reader_impl.rs`](../../../rspace++/src/rspace/history/instances/rspace_history_reader_impl.rs).
The input normalizer orders syntactic sources, whereas evaluation substitutes those
sources without reordering binds. No reviewed check establishes that every supported
resolved group or admitted DAG state excludes order aliases.

Two deterministic legacy tests freeze the distinction between duplicate-key refusal
inside one checkpoint and successful last-writer updates across checkpoints. A third
test, `ordered_endpoint_insertions_commute_but_legacy_group_checkpoints_do_not`, uses
actual `OpenDagTransaction` sealing and full semantic readmission under the fixture's
`TestSemanticProfile` (string channels, patterns, data and continuations). Both orders insert
the same two ordered continuation endpoints with identical final join records.
Their exact DAG manifests, versions and three namespace roots agree; their legacy
roots and aliased cold continuation reads do not. The fixture pins both legacy roots:

| Checkpoint order | Frozen legacy root, hexadecimal |
|---|---|
| Forward group, then reversed group | `8e7af529977c2d5cb4e85c199482ccabcf5c9b83d77a90ff3dd76f9e0e8ef89b` |
| Reversed group, then forward group | `384b28f6f87a7f079c4759dff7abaa1010218a4f01957d4160c934fd92fdd7f7` |

This refutes an unconditional function from the three ordered namespace roots to the
legacy root **for the tested insertion correspondence**. It is not an end-to-end
Rholang execution, a proof of all supported trace reachability, or permission to
normalize channel order, reject previously successful calls, or alter hash preimages.

### Selected contract and realization premises

A *coupled binding* authenticates the exact semantic manifest, compatibility facet,
store/profile/schema domain, bridge version and certified legacy root. A *compatibility
facet* is the recoverable information required to preserve the frozen legacy commitment
and observable lookup behavior. It is mandatory authority data when it cannot be
derived from the three semantic roots; it is not an accelerator, cache, inherited
provenance field, second current pointer or independent backend acknowledgement.

Let $`S`$ denote semantic state, $`F`$ compatibility information, $`C=(S,F)`$ the coupled
state, and $`H`$ legacy history. The required relation $`\operatorname{Refines}(C,H)`$
must establish matching supported cold/reset reads, successful transitions and refusal
behavior. The bridge must preserve that relation on every supported transition and
construct an admitted coupled successor whenever the frozen legacy operation succeeds.
Neither semantic admission alone nor the abstract predicate proves that construction.

The model's `frozen_history_bytes` is a mathematical sequence describing the frozen
history interpretation, **not a prescribed flat runtime buffer or a second serialization
of all RSpace**. Concrete design must use native PathMap sharing, incremental updates,
existing byte-native canonical leaves and checked streaming where applicable. It must
prove compatibility without restoring a separately current LMDB/RadixHistory backend.
Canonical layout, incremental maintenance and space/time bounds are still implementation
obligations; this section does not silently add a fourth namespace to existing files.

[`RSpaceConsensusIdentity.v`](../../../formal/rocq/stack_safe_pda/theories/RSpaceConsensusIdentity.v)
uses exact reference tokens and abstract history sequences. `binding_lookup` means
authenticated exact lookup under the selected owner; its functional result prevents
one reference from certifying two different bindings. `frozen_root`, `frozen_read`,
`legacy_refinement` and `successful_refined_transition` are explicit realization
parameters. The model proves neither BLAKE2b collision resistance nor codec extraction,
disk authentication, total import, or full Rholang transition equivalence.

Publication installs one complete current/retained/alias catalog in one abstract step.
It requires authenticated **post-publication** lookup, retained target and a resolvable
coupled-equivalent representative, and proves the successor is selected and usable.
The existing WAL/publication protocol must realize atomic installation, cancellation,
crash ordering and preservation of all still-required roots/aliases. Recovery rechecks
the exact binding and representative and composes the caller model's full observable
incarnation freshness rule; the registry's completeness remains an implementation premise.

Aliases are not bijections. Several exact manifests/provenances may have equivalent
coupled observations; a manifest can also require different compatibility information
after different histories. One legacy lookup appoints exactly one retained representative.
Changing that representative requires equal coupled observations, not merely equal
semantic roots, and does not retarget existing reader/export pins. The replacement
relation proves candidate equivalence and unchanged pin identity, not lease persistence,
catalog mutation or garbage-collection correctness.

### Exhaustive capstone-to-property extraction

The following 28 rows exhaust the model's named capstones. **Future** entries are binding
executable acceptance targets for the indicated existing task, not currently passing
runtime evidence. This distinction prevents a checked abstract contract from being
reported as a completed bridge implementation.

| ID | Capstone | Concrete evidence or required executable property |
|---|---|---|
| CI-01 | `ordered_insertions_commute` | Actual sealed/readmitted DAG fixture compares both exact final manifests and all three roots. |
| CI-02 | `legacy_alias_insertions_do_not_commute` | Actual legacy repository fixtures compare successful sequential roots and both ordered cold reads. |
| CI-03 | `no_unconditional_ordered_state_projection` | Combined coherent-join DAG/legacy fixture; future 7942 adds full supported runtime trace correspondence. |
| CI-04 | `checked_requires_exact_authentication_and_refinement` | Missing-authentication and missing-refinement formal controls; future 7942 independently corrupts each required certificate/closure component. |
| CI-05 | `authenticated_reference_identifies_exact_binding` | Same-reference/different-facet formal control; future 7942 exercises digest/full-byte equality checks and exact lookup. |
| CI-06 | `checked_preserves_domain_profile_schema_and_version` | New generated manifest-field property and unsupported-schema property; future 7942 adds bridge/store/session-domain mutations. |
| CI-07 | `checked_root_comes_from_frozen_compatibility` | Frozen serializer/root goldens and manifest-as-root formal control; future 7942 derives roots from actual coupled compatibility state. |
| CI-08 | `absent_exact_authentication_refuses` | Missing-facet control; future 7942 deletes/truncates each binding/facet dependency without fallback. |
| CI-09 | `absent_refinement_refuses` | False-refinement control; future 7942 rejects invented certificates while proving all supported successful transitions construct valid ones. |
| CI-10 | `direct_manifest_identity_is_not_activated` | Formal version separation; future 7942 rejects unapproved direct-manifest consensus identities. |
| CI-11 | `unknown_binding_version_is_not_activated` | Formal unsupported tag; future 7942 varies every unsupported bridge version. |
| CI-12 | `mixed_session_cannot_resolve` | Formal session mismatch; future 7942/7938–7940 qualify peer negotiation, resume and unknown identities. |
| CI-13 | `alias_requires_appointed_retained_coupled_target` | Reuses caller alias contract; future 7931/7942 resolve only owner-appointed retained coupled targets. |
| CI-14 | `unappointed_provenance_cannot_resolve` | Generated manifest test shows provenance changes exact manifest bytes; future 7942 forbids using that hint as an alias certificate. |
| CI-15 | `alias_resolution_identifies_one_exact_binding` | Reuses exact singleton alias resolution and functional binding lookup; future 7942 injects ambiguous and conflicting aliases. |
| CI-16 | `unretained_binding_cannot_resolve` | Formal retained-membership guard; future 7942/7943–7944 test removed aliases and retained closures. |
| CI-17 | `equivalent_checked_bindings_preserve_root_and_reads` | Exact compatibility-byte equivalence theorem; future 7942 compares concrete canonical reads/roots across provenance variants. |
| CI-18 | `replacement_preserves_coupled_observations_and_exact_pin` | Formal replacement relation; future 7943–7944 keep old leased targets readable through atomic representative replacement. |
| CI-19 | `publication_selects_one_binding_and_exposes_legacy_root` | Positive publication witness and manifest-as-root control; future 7931/7942 exercise actual checkpoint acknowledgement/root APIs. |
| CI-20 | `completed_publication_is_closed_under_selection` | Prepublication-only lookup control; future 7931 immediately authenticates and uses the acknowledged successor. |
| CI-21 | `completed_publication_has_a_usable_equivalent_alias` | Positive publication witness; future 7942 resolves every acknowledged external root after commit and restart. |
| CI-22 | `publication_requires_refined_transition_not_just_semantic_admission` | Explicit transition premise; future 7942 differentially generates successful/refused legacy and coupled operations, including ordered aliases. |
| CI-23 | `publication_invalidates_old_permit` | Reuses caller generation rules; future 7931/7932 reject stale operations even when the legacy root returns to an old value. |
| CI-24 | `recovery_preserves_exact_selected_binding_and_legacy_observations` | Positive recovery witness; future 7932/7942 terminate the OS process at each durable boundary and compare exact resumed state/root/reads. |
| CI-25 | `missing_recovery_binding_cannot_fall_back` | Missing-facet and restart-reauthentication controls; future 7942 refuses missing selected authority rather than falling back to provenance or LMDB. |
| CI-26 | `recovered_owner_rejects_every_observable_permit` | Raw-reopen incarnation control; future 7931/7932 retain old handles across multiple restarts and reject each obsolete incarnation. |
| CI-27 | `recovered_owner_has_a_usable_equivalent_alias` | Positive recovery witness; future 7932/7942 immediately reset/read/export using recovered legacy roots. |
| CI-28 | `same_semantic_manifest_does_not_imply_coupled_equivalence` | Distinct-facet formal witness plus the coherent DAG/legacy fixture; future 7942 never overwrites identity mappings solely by manifest equality. |

The companion `RSpaceConsensusIdentityMutants.v` contains three positive witnesses
(checked binding, completed publication, checked restart) and seven negative controls.
The proof runner requires 28 and 10 closed-context reports, independent `coqchk` success,
and placeholder rejection. The controls use a deliberately simple root/read abstraction;
they are not an independent implementation of the frozen legacy hash algorithm.

### Implementation ownership and completion boundary

Task 7942 owns concrete coupled identity/facet construction, complete successful/refused
transition refinement and consensus qualification; 7931 owns the actual operation gate;
7932 owns caller lifecycles. Tasks 7934–7936 import all retained roots and shadow/cutover;
7938–7940 transfer/version/resume the full coupled closure; 7943–7944 retain/reclaim facets,
aliases and pins. They remain required and reachable under campaign 4131. Task 7942 must
decompose independent implementation algorithms before broadening its execution, without
introducing a dependency cycle with operation-gate task 7931.

No physical format, current backend, Rholang match/normalization rule, legacy root preimage,
blessed contract, compute/storage charge, or consensus/API epoch changes in this checkpoint.
If exact preserved behavior cannot be realized, evidence and an explicit versioned decision
are required; silently dropping valid states or adding new refusals is not an implementation
of this contract.

## Legacy retained-root migration baseline (2026-09-07)

Task 7933 freezes the supported legacy migration observations before the PathMap-DAG
adapter and cutover. The baseline is the unchanged production implementation at
`53d7c57be`, exercised by the added
`legacy_lmdb_retained_fixture_survives_process_exit_and_frozen_root_replay` test in
[`atomic_state_authority.rs`](../../../rspace++/tests/state/atomic_state_authority.rs).
All fixtures are synthetic; no production database was scanned or rewritten.

### Fixture and exact identities

The fixture uses production `HistoryRepository<String, String, String, String>` with
the concrete record validator, not the permissive validator of some low-level atomicity
controls. It records two copies of a nonpersistent datum on the first channel, one
persistent two-channel continuation with peek position zero, and a join record for each
channel. The next checkpoint adds one persistent datum on the second channel. The
non-latest first checkpoint is the fixed rollback point. The complete fixture catalog
contains exactly three verified roots and one current-root record; the current generation
is two. Historical reads compare canonical datum, continuation/source and join bytes.

| Role | Frozen legacy root |
|---|---|
| Empty retained state | `0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8` |
| Non-latest rollback checkpoint | `10842f8676716425a2fd9a60a4fd748b8225e4f6aa6c02578fc4fb79ded15c5d` |
| Latest checkpoint after the recorded suffix | `d430aa653eceb0c0df6e821ee8941bcb3f5e235584048a7279ebc48b49fe06b6` |

The test independently pins a fingerprint for each sorted logical key/value inventory
and for the **ordered** export page transcript at page sizes one, two and five over all
three roots. Each transcript retains start cursor, next cursor, derived terminal flag,
ordered history items and ordered cold items; it does not deduplicate away repeated
emissions. These fingerprints are `Blake2b256(bincode::serialize(fixture_value))` under the
existing locked dependencies. They are test-evidence fingerprints, not new consensus
hash domains, LMDB file hashes or prescribed production serialization algorithms.

| Fixture component | Frozen fingerprint |
|---|---|
| Logical history key/value inventory | `96d2d98d20f9e5e7c77f0d826384360a6f7a8a02c03fed072106db7e9174eeb2` |
| Typed cold key/value inventory | `4cc917977696dcbc3c729117d3c85f69891e9244f2e0430614389bfdc6173c5c` |
| Verified/current catalog records | `18957323e133547de68f8828cc9e7bc3c18612e6ef4f3795531e8d3152184097` |
| Complete ordered page transcript | `c5bdca40adc5acc4d86117fb25309c16568265bef18441755189a536ddeaec12` |
| Combined current/catalog/history/cold/pages snapshot | `fafc11d0fb8accd3b1b3f3f2f26c07dc6aeb143aae9847dd502ef2f1a6f60afa` |

The in-memory reference and actual LMDB reader must have identical snapshots as well as
matching these literals. The literals were captured with production code unchanged, then
pinned and rerun. Comparing two live implementations alone would not freeze a baseline.

### Process termination, rollback and protocol boundaries

A dedicated child process creates the LMDB fixture, verifies its reads, then calls
`process::exit(73)` while repository/environment handles remain live. The parent requires
that exact exit status, so a misspelled filter or zero-test child cannot pass. It then
opens the directory for the first time in that reader process and compares all retained
state, catalog bytes and page transcripts. This bypasses writer-side Rust destructors and
the process-global environment cache; it is stronger than same-process drop/reopen.
It is **not** a power-failure, torn-sector, arbitrary-instruction crash or fsync-fault test.

The reader exports only the older rollback root and imports it, through the existing
affine staging/sealing/publication API, into a fresh LMDB destination. The destination
must not know the later root yet, and the later datum is absent. Replaying the recorded
typed checkpoint-action suffix then produces the exact latest root and identical historical
reads/page transcripts. The destination's current authority must select that root with exactly
one generation advance; a freshly constructed repository must select it too. Merely retaining
the root while leaving current authority at the rollback point cannot pass. This is
frozen-root-plus-recorded-suffix replay, not a Rholang
deploy/replay execution, a post-cutover rollback implementation or an instant switch to
a newer current legacy backend. The original source snapshot remains unchanged.

Stateless cursor continuation is distinct from durable import-session resumption. Existing
invalid-page retry keeps the same process-local session; it does not recover its capability
after termination. Tasks 7938–7940 still implement/qualify the new transfer/resume protocol.
Actual retained-root DAG import, shadow ownership and cutover remain tasks 7934–7936.

### Baseline assertions and reused formal obligations

| ID | Required observation | Evidence and downstream binding |
|---|---|---|
| LM-01 | Exact current root/generation and complete verified-root inventory | New fixture checks all catalog members and literal snapshot; 7934 imports every retained root, not just current. |
| LM-02 | Membership and historical reads do not change current authority | New snapshot checks current before/after inspection and source unchanged after rollback/replay; existing atomic tests cover activation separately. |
| LM-03 | Multiplicity, persistence, peek/source metadata and joins remain canonical | New typed historical-read comparisons plus existing serializer goldens and continuation/join lifecycle corpus. |
| LM-04 | Exact ordered page transcript, root-bound continuation and terminal cursor | New literal transcript at three page sizes; existing export/import tests additionally cover skip/cursor and rejected-page retry. |
| LM-05 | Committed state remains available to a fresh process without writer destructors | New child exit-status witness and reader's exact LMDB snapshot; power-loss and new-DAG byte-progress faults remain separate gates. |
| LM-06 | Rollback begins at the designated older root, not an invented current successor | Fresh destination imports only the pinned rollback root and rejects membership of the later root until suffix replay. |
| LM-07 | Replaying the recorded checkpoint suffix restores exact latest observations | New returned/current/fresh-repository root equality, exact generation advance, historical-read and ordered transcript comparison; full runtime/genesis/COMM replay remains the existing integration track. |
| LM-08 | Complete publication couples history, cold objects, membership and current root | Existing `RSpaceAtomicCheckpointAuthority.tla`: `CurrentRootIsVerified`, `VerifiedRootsHaveCompleteClosure`, `ContentKeysAreImmutable`, `NoUnpublishedCheckpointObjects`; concrete atomic-state suite and split/trust-target controls. |
| LM-09 | Activation is generation-bound; stale/failing/no-op operations have exact effects | Same model's `TypeInvariant`, `GenerationAdvancesExactlyOnActivation`, `CommitUsesExactBase`, `FailedCommitLeavesAuthorityUnchanged`, `NoOpLeavesAuthorityUnchanged`; existing atomic-state suite and root-only control. |
| LM-10 | Import stages privately, seals once and publishes through its exact capability | Existing `RSpaceAffineImportLifecycle.tla`, import-session/state tests and the new real-LMDB rollback destination; no claim of process-persistent staging. |
| LM-11 | Legacy namespace adoption preserves exact validated source identity | Existing `legacy_state_migration.rs` tests cover split/v2/eval, namespace-bound markers, divergent sources and malformed records; their same-process retry is not subprocess evidence. |
| LM-12 | Physical migration does not silently replace legacy consensus identity | CI-01..CI-28 and the #7941 ordered-alias fixtures remain binding for 7934–7936; metadata/provenance is not a legacy-root certificate. |

The unchanged atomic model was rerun: 357 states generated, 127 distinct states,
complete depth seven, no remaining queue or invariant failure. Its split-publication,
root-only-base and trust-target controls each fail the intended invariant. This finite
abstract model is reused, not presented as a proof of the LMDB engine, host filesystem
or future PathMap persistence implementation.

### Verification result and reproduction

The reviewed source passed 45 state tests and six export/import tests in release mode,
with no failures or ignored tests in those selections. Nextest separately reran the new
subprocess fixture successfully; that is 51 distinct tests, not 52. The commands were:

```sh
cargo test --release --locked -j4 -p rspace_plus_plus --lib rspace::state:: -- --test-threads=2
cargo test --release --locked -j4 -p rspace_plus_plus --test mod export_import_tests:: -- --test-threads=2
CARGO_BUILD_JOBS=4 cargo nextest run --release --locked -p rspace_plus_plus --lib --test-threads 2 -E 'test(legacy_lmdb_retained_fixture)' --no-fail-fast
```

Execution used a systemd scope with 8 GiB hard/6 GiB high memory limits, swap disabled,
four CPU-equivalents and two test threads. Model checking used a separate 2 GiB hard/
1 GiB high scope with swap disabled, a 512 MiB Java heap and two workers. These are
configured ceilings, not measured peak RSS or a performance comparison. Existing compiler
warnings remain outside this baseline capture; these selections do not replace the full
workspace release, new-backend fault or default-cutover gates.

### Byte-identical versus versioned surfaces

Preserve logical legacy history-node bytes, cold leaf hash preimages and roots, canonical
records, multiplicities, historical observations, selected continuation/COMM behavior and
consensus-visible post-state identity. The fixed old pagination transcripts define the
legacy protocol baseline. The new backend may use a separately reviewed/versioned transfer
protocol, but it may not reinterpret an old cursor or import session silently.

Physical LMDB pages/inodes, local database mappings, typed cold addresses, catalog encodings,
future compatibility-facet representation, ACT/index/pack encodings and new transfer/resume
metadata are not thereby declared consensus preimages. Their format, migration and rollback
compatibility must still be explicit. Storage metering integration remains separately tracked;
this fixture does not select a charging representation or change compute/storage costs.

## Source-bound DAG transfer and resume contract (2026-11-01)

**Status: abstract contract model-checked; implementation and qualification remain
required.** Task 7938 owns this contract. Task 7939 implements the parent-first streaming
export of the full coupled closure, affine private staging, the persisted resume frontier
and full cold admission before publication. Task 7940 owns the node and FFI consumers;
negotiating a DAG transfer version is wire-visible and stays non-default until the user
confirms it. This checkpoint changes no physical format, wire message, consensus identity
or production source.

### Session, token and coordinates

Write $`\beta`$ for the binding digest, $`g`$ for the catalog generation, $`b`$ for the
bridge version, $`t`$ for the transfer version, $`p`$ for the profile, and $`f`$ for the
acknowledged frontier: the number of leading objects of the closure, in the export order
of $`t`$, that the importer has staged durably and then acknowledged. A transfer session
$`s`$ and its resume token $`\tau`$ are

```math
s = (\beta,\ g,\ b,\ t,\ p), \qquad \tau = (s,\ f), \qquad 0 \le f \le n,
```

where $`n`$ is the number of objects in the closure that $`\beta`$ names.

The binding digest is abstract. It names the exact coupled semantic-plus-compatibility
closure (CI-05, CI-06) and the physical representation that the source published at one
catalog generation, so two representations of one state have different digests, as
theorems 14 and 15 of `RetentionV2Format.v` require of the retention binding (RV-16). Its
durable carrier and the read path are pending rulings and are not modelled; whichever
carrier is chosen must keep the digest injective over closure, representation and
generation. The bridge version is the identity version of `RSpaceConsensusIdentity.v`,
where only `PreservedLegacyV1` resolves (CI-10, CI-11, CI-12). The catalog generation and
the transfer version are physical transfer coordinates: they never enter consensus
identity and are not epochs (RV-17, LM-12).

### Model

`formal/tlaplus/stack_safe_pda/RSpaceDagStateTransfer.tla` extends the unchanged
`RSpaceAffineImportLifecycle.tla` (LM-10). The affine session chosen as the transfer target
is the importer's adoption capability; only cold admission (its terminal page and closure
verification), seal and publication touch it, so a refused or aborted transfer leaves it
unused. The other two affine sessions keep the inherited lifecycle and race the transfer.
The inherited `Spec` is checked as a property of the transfer specification, so every
step is an affine step or leaves the affine state unchanged, and all seven inherited
invariants are checked on the composed specification.

The source publishes a second generation that restamps the same state, reclaims an
unpinned superseded generation and may lose its pins. A reconnection meets one of five
peers: legacy-only, DAG transfer version 1, an upgraded peer speaking only version 2, a
peer presenting an unactivated bridge identity, and a peer with a foreign profile. The
durable importer speaks versions 1 and 2 and never the legacy page protocol, because a
durable node cannot bootstrap from a legacy-only peer (plan 4.5). A frontier is meaningful
only under the export order of one transfer version.

The model is a contract, not an implementation. One importer runs one transfer at a time.
The source is honest but may crash, restart or change software; page and cursor validation
against a malicious peer remains task 6093's authenticated cursor contract and the closure
verifier. Abort and admission release the pin, standing for release or lease expiry; a
lingering pin only delays reclamation. A crash after admission is not modelled because the
affine capability is process-local (LM-10); recovery returns to the complete frontier,
from which admission needs no source.

The domains are bounded: a closure of two objects, a root and its child (so $`n = 2`$);
two source generations; the five peers; the two transfer versions the importer speaks,
whose export orders are reversed; and two profiles. Write $`\mathrm{obj}(\beta, t, i)`$
for the object at position $`i`$ of the export order of version $`t`$ over the closure
that $`\beta`$ names, and $`\beta_s`$ and $`t_s`$ for the binding digest and the transfer
version of a session $`s`$. The objects that the token $`(s, f)`$ acknowledges are

```math
\mathrm{Ack}(s, f) = \{\, \mathrm{obj}(\beta_s, t_s, i) : 1 \le i \le f \,\}.
```

With $`\mathit{staged}`$ the importer's private staging and $`v`$ the tuple of all model
variables, the frontier discipline is the invariant `FrontierIsContiguous` together with
the action property `FrontierAdvancesByOne`:

```math
\mathrm{Ack}(s, f) \subseteq \mathit{staged} \subseteq \mathrm{Ack}\big(s, \min(f + 1, n)\big),
\qquad
\Box\big[\, s' = s \Rightarrow f' \in \{f,\ f + 1\} \,\big]_{v}.
```

The invariant allows at most the next object beyond the acknowledged prefix to be staged,
which is what a crash between staging an object and persisting the frontier leaves
behind; the property forbids a frontier that regresses or skips within one session.

![State diagram of one source-bound DAG transfer session. In grey, no session: no token, no staging, no stream, and the adoption capability unused; a negotiation with a legacy-only peer, an unactivated bridge identity or a foreign profile is refused, in red, and opens nothing. In blue, an accepted negotiation binds the session, made of the binding digest, catalog generation, bridge, transfer version and profile, to the source's current generation and pins it; the source serves the next object from the pinned generation in the export order of the session's transfer version, and the importer persists the frontier only once that object is staged. In amber, a disconnect keeps the token, the staging and the pins; a reconnection resumes the stream, in blue, only with the same transfer version, bridge and profile while the source still pins the exact session, and is otherwise refused, in red, with the token kept. In dark grey, abort discards the staging and the token and releases the pin. In green, full cold admission at the complete frontier over the whole closure, then the inherited seal and publication as a known or current state; in red, the stale activation that a racing activator beat at the generation compare-and-swap. In purple, the source restamps generation 1 as generation 2 without moving a pin, reclaims generation 1 only while no transfer pins it, and may lose its pins while the link is down. Notes name the invariants that hold on a live stream, without a session, and at publication](figures/dag-transfer-session.svg)

### Configurations

`scripts/check-rspace-dag-state-transfer-model.sh` runs every configuration and requires
the stated result. It refuses a module that no longer extends the affine model and a
faithful configuration that drops an inherited invariant or either property, requires the
faithful model to complete with no error, and requires each of the 30 controls to fail
with its exact expected text. The stack-safe proof runner calls it with a 120 s TLC limit
per run. The numbers below come from the runner's gate on the tree of commit `48546c256`
(TLC 2.19, two workers, a 4 GiB scope). With two workers TLC's breadth-first search is not
level-synchronous, so a control's generated and distinct counts depend on scheduling; every
trace nevertheless has the minimal length predicted by hand before the run, and the
faithful model's 47,072 distinct states at depth 20 equal the hand count. A single-worker
run, as the stack-safe proof runner makes, reports the same faithful-model numbers.

| ID | Configuration suffix | Checked | Expected | Generated | Distinct | Depth | Trace (states) |
|---|---|---|---|---:|---:|---:|---:|
| — | (base) | 18 invariants, 2 properties | no error | 390,674 | 47,072 | 20 | — |
| DT-M01 | `CrossVersionResumeMutant` | `StreamMatchesSessionIdentity` | violated | 176 | 104 | 4 | 4 |
| DT-M02 | `SkippedFrontierMutant` | `FrontierIsContiguous` | violated | 32 | 32 | 3 | 3 |
| DT-M03 | `MixedSessionMutant` | `StagingIsSessionBound` | violated | 2,202 | 812 | 6 | 6 |
| DT-M04 | `StateVersionOnlyResumeMutant` | `SessionBindingIsExact` | violated | 772 | 332 | 5 | 5 |
| DT-M05 | `ResumeWithoutPinMutant` | `ServingIsPinned` | violated | 752 | 328 | 5 | 5 |
| DT-M06 | `ReclaimPinnedMutant` | `PinnedGenerationsAreRetained` | violated | 194 | 117 | 4 | 4 |
| DT-M07 | `LegacyFallbackMutant` | `SessionIdentityIsSupported` | violated | 20 | 20 | 2 | 2 |
| DT-M08 | `OlderAuthorityMutant` | `StreamMatchesSessionIdentity` | violated | 20 | 20 | 2 | 2 |
| DT-M09 | `EarlyAdmissionMutant` | `ColdAdmissionRequiresCompleteClosure` | violated | 32 | 32 | 3 | 3 |
| DT-M10 | `FrontierBeforeObjectsMutant` | `FrontierIsContiguous` | violated | 32 | 32 | 3 | 3 |
| DT-M11 | `AffineIncompleteAdoptionMutant` | `AdoptionRequiresAdmittedClosure` | violated | 20 | 20 | 2 | 2 |
| DT-M12 | `AbortAfterAdmissionMutant` | `NoSessionHoldsNothing` | violated | 13,556 | 3,507 | 8 | 8 |
| DT-W01 | `NeverAdoptsCurrent` | `NeverAdoptsCurrent` | violated | 29,139 | 6,529 | 9 | 9 |
| DT-W02 | `NeverAdoptsKnown` | `NeverAdoptsKnown` | violated | 29,035 | 6,515 | 9 | 9 |
| DT-W03 | `NeverRefusesLegacyOnlyPeer` | `NeverRefusesLegacyOnlyPeer` | violated | 8 | 8 | 2 | 2 |
| DT-W04 | `NeverRefusesUnsupportedBridge` | `NeverRefusesUnsupportedBridge` | violated | 10 | 10 | 2 | 2 |
| DT-W05 | `NeverRefusesProfileMismatch` | `NeverRefusesProfileMismatch` | violated | 12 | 12 | 2 | 2 |
| DT-W06 | `NeverResumesAfterDisconnect` | `NeverResumesAfterDisconnect` | violated | 154 | 86 | 4 | 4 |
| DT-W07 | `NeverRefusesResumeOnLegacyOnlyPeer` | `NeverRefusesResumeOnLegacyOnlyPeer` | violated | 158 | 90 | 4 | 4 |
| DT-W08 | `NeverRefusesResumeOnChangedTransferVersion` | `NeverRefusesResumeOnChangedTransferVersion` | violated | 156 | 88 | 4 | 4 |
| DT-W09 | `NeverRefusesResumeOnBridgeChange` | `NeverRefusesResumeOnBridgeChange` | violated | 208 | 118 | 4 | 4 |
| DT-W10 | `NeverRefusesResumeOnProfileChange` | `NeverRefusesResumeOnProfileChange` | violated | 162 | 94 | 4 | 4 |
| DT-W11 | `NeverRefusesResumeAfterPinLapse` | `NeverRefusesResumeAfterPinLapse` | violated | 739 | 326 | 5 | 5 |
| DT-W12 | `NeverResumesAcrossSourcePublication` | `NeverResumesAcrossSourcePublication` | violated | 632 | 282 | 5 | 5 |
| DT-W13 | `NeverStagesBeyondFrontier` | `NeverStagesBeyondFrontier` | violated | 144 | 78 | 4 | 4 |
| DT-W14 | `NeverAdoptsAfterAbort` | `NeverAdoptsAfterAbort` | violated | 106,429 | 18,505 | 11 | 11 |
| DT-W15 | `NeverRejectsTransferActivation` | `NeverRejectsTransferActivation` | violated | 143,605 | 23,216 | 12 | 12 |
| DT-W16 | `NeverReclaimsSupersededGeneration` | `NeverReclaimsSupersededGeneration` | violated | 102 | 64 | 3 | 3 |
| DT-P01 | `LifecyclePropertyMutant` | `AffineLifecycleIsPreserved` against `AffineIncompleteAdoptionMutantSpec` | violated | 20 | 20 | 2 | 2 |
| DT-P02 | `FrontierPropertyMutant` | `FrontierAdvancesByOne` against `SkippedFrontierMutantSpec` | violated | 32 | 32 | 3 | 3 |

DT-M01 to DT-M03 are the three mutants that plan section 4.6 names. DT-P01 and DT-P02 are
the negative controls of the two properties: each runs a mutant specification against one
property alone, so a property that held vacuously would let its control pass. TLC names
the inherited `[][Next]_vars` of `Spec`, which DT-P01 violates, by its source location,
line 95 of `RSpaceAffineImportLifecycle.tla`.

### Peer and interruption matrix

| # | Scenario | Faithful behavior | Covered by |
|---|---|---|---|
| E1 | Durable importer, legacy-only peer | refuse; no session, no fallback | DT-02; M07; W03 |
| E2 | Durable importer, DAG peer v1 or v2 | complete; current or known adoption | W01, W02; DT-08, DT-09 |
| E3 | Durable importer, unactivated bridge | refuse; no older authority | DT-02, DT-03; M08; W04 |
| E4 | Durable importer, foreign profile | refuse | DT-02; W05 |
| E5 | Legacy importer, DAG peer (pages from the facet) | unchanged legacy affine import; not a transfer client here | DT-11 (inherited invariants on the free sessions); executable owners N1 B09 and 7940 |
| E6 | Legacy importer, legacy peer | unchanged | DT-11; the three affine mutants |
| E7 | Mixed: DAG session reconnects to legacy-only | resume refused, token kept | DT-03; M01; W07 |
| E8 | Changed transfer version (v1 then upgraded v2) | resume refused; abort; fresh session completes | DT-03; M01; W08, W14 |
| E9 | Mixed: reconnect to unactivated-bridge peer | resume refused | DT-03; M01; W09 |
| E10 | Mixed: reconnect to foreign-profile peer | resume refused | DT-03; M01; W10 |
| E11 | Crash or disconnect, same peer, pin held | resume at frontier | DT-04, DT-07; W06 |
| E12 | Crash between staging and frontier persistence | at most one page beyond the frontier; idempotent re-stage | DT-07; M10; W13 |
| E13 | Changed source generation (restamp), pin held | pinned generation still served; resume accepted | DT-01, DT-04; M04; W12 |
| E14 | Source restart or pin lapse | resume refused; abort; restart binds the current generation | DT-04; M05; W11, W14 |
| E15 | Superseded generation reclaimed | only when unpinned | DT-05; M06; W16 |
| E16 | New session after refusal or abort | empty staging, frontier 0 | DT-06, DT-10; M03 |
| E17 | Out-of-order page | rejected | DT-07; M02; P02 |
| E18 | Admission or adoption before complete closure | blocked | DT-08, DT-09; M09, M11; P01 |
| E19 | Abort after admission | forbidden; consume through the FFI abort | DT-10; M12 |
| E20 | Racing activation wins | transfer Activate rejected by CAS | inherited CAS invariants; W15 |

In the last column, M*nn*, W*nn* and P*nn* abbreviate the configurations DT-M*nn*, DT-W*nn*
and DT-P*nn* above.

### Contract rows

DT-01 to DT-10 exhaust the transfer invariants of the model; DT-11 names the inherited
lifecycle. **Future** entries are binding executable acceptance targets for the indicated
task, not currently passing runtime evidence.

| ID | Model symbol | Negative control | Required executable property |
|---|---|---|---|
| DT-01 | `SessionBindingIsExact` | DT-M04 | Future 7939: a session records the exact binding digest the source published at its catalog generation; with one state restamped into a second representation, a token is refused against the restamp, and refreshing its generation while keeping its digest is refused. |
| DT-02 | `SessionIdentityIsSupported` | DT-M07 | Future 7939 and 7940: negotiating with a legacy-only peer, an unactivated bridge identity or a foreign profile refuses before any page is staged or token written, with no legacy-page fallback (plan 4.5). |
| DT-03 | `StreamMatchesSessionIdentity` | DT-M01, DT-M08 | Future 7939: a token resumes only on a stream speaking its transfer version, bridge and profile; upgraded, legacy-only, epoch and foreign-profile peers refuse, and an unactivated bridge is never answered with the older `PreservedLegacyV1` authority (CI-12, CI-25). |
| DT-04 | `ServingIsPinned` | DT-M05 | Future 7939: the exporter serves a session only under an authority-held pin of that exact session; after a restart or lease expiry drops it, resume is refused. |
| DT-05 | `PinnedGenerationsAreRetained` | DT-M06 | Future 7939 with 7943–7944: reclamation never removes a generation a transfer pins (CA-21). |
| DT-06 | `StagingIsSessionBound` | DT-M03 | Future 7939: staging and frontier never cross sessions; a session opened after a refusal or abort starts at frontier 0 with empty private staging. |
| DT-07 | `FrontierIsContiguous`, `FrontierAdvancesByOne` | DT-M02, DT-M10, DT-P02 | Future 7939: a page beyond the next position is rejected; the frontier is persisted only after the objects it covers are durable; crash injection between the two leaves at most one page beyond the frontier, and replay is idempotent. |
| DT-08 | `ColdAdmissionRequiresCompleteClosure` | DT-M09 | Future 7939: full cold admission (closure walk, retention metadata closure, history rules) runs only at the complete frontier over the complete closure of the session binding. |
| DT-09 | `AdoptionRequiresAdmittedClosure` | DT-M11 | Future 7939 and 7940: `PreserveCurrent` and `Activate` publish only an admitted, sealed closure of a supported session; a stale activation is rejected by the generation CAS. |
| DT-10 | `NoSessionHoldsNothing` | DT-M12 | Future 7939 and 7940: abort before admission discards staging and token and leaves the capability unused; after admission the capability is consumed through the FFI abort (`AbortConsumesWithoutPublishing`), never by dropping the token. |
| DT-11 | the seven inherited invariants, `AffineLifecycleIsPreserved` | the three affine mutants; DT-W15; DT-P01 | Existing affine evidence (LM-10); no new property. |

## Refinement completion rule

The runtime may be declared conformant only when:

- every catalog row has a non-ignored executable property or a documented measurement gate;
- every historically red target is green;
- every negative control is known to make at least one corresponding property fail;
- all 14 non-vacuity witnesses execute;
- the integrated formal gate and the focused Rust property/integration suites pass under bounded resources;
- the stack-safety report, consensus change register, and PathMap engineering journal record the final wire, durability, compatibility, and measurement evidence.

## Resolved LMDB scope isolation (2026-09-07)

**Status: governing model compiled and independently kernel-checked before Rust
implementation; all 23 new fixture/control statements are also checked;
executable refinement remains in progress.**
Required task 8283 repairs the database-name override bypass introduced by the
authority adapter in task 6094. Baseline task 5636 depends on this repair: its
fresh-process positive genesis/workload replay passes, but three concurrent
negative-control tests encounter another scope's published root during setup.
The pre-change authority suite passes 16 tests and does not cover that mapping.

A *physical slot* is the pair of a resolved environment identity and an exact
database name. Each authority owns three live slots in its history environment;
the configured cold environment may instead designate a read-only migration
source. Different name triples alone do not establish isolation: their slots
must be disjoint. An unrelated environment may legitimately contain a database
named `rspace-history` without that name acquiring authority semantics.

`RSpaceCallerAuthority.v` defines `scope_invariant_id`, its exact twenty-entry
catalog, and a theorem binding every entry to its proposition. The following is
the complete extraction of that catalog, not a claim that the Rust properties
already pass. Property families are planned in the existing
`rspace++/tests/state/atomic_state_authority.rs` test source.

| ID | Model invariant | Required executable evidence |
|---|---|---|
| SA01 | `IndependentNameResolution` | Arbitrary independent history, roots and cold overrides; absent overrides use logical IDs. |
| SA02 | `UniqueRoleResolution` | Repeated acquisition and reopened mappings select the same exact layout; mapping order cannot change selection. |
| SA03 | `ExactlyOneLogicalRole` | Missing and duplicate logical-role mappings refuse. |
| SA04 | `HistoryRootsConfiguration` | Different history/root environments or incompatible environment settings refuse. |
| SA05 | `PhysicalNamesRejectNul` | Each physical-name NUL mutation refuses before opening. |
| SA06 | `CheckedFieldLengths` | Every namespace field uses checked `u64` length conversion; aggregate manifest length and allocation arithmetic are checked. No giant allocation is needed to exercise a modeled arithmetic boundary. |
| SA07 | `DistinctPhysicalRoles` | Every pairwise collision among the three live role names refuses. |
| SA08 | `OrdinaryAliasExclusion` | Ordinary-first and authority-first aliases of live slots refuse, including the live cold destination. |
| SA09 | `ValidationBeforeDatabaseOpen` | Invalid configurations leave the target directory/database inventory unchanged. |
| SA10 | `ExactU64FrameCancellation` | An independent framing decoder consumes exactly one eight-byte length and its declared payload. |
| SA11 | `ExactFourFieldEncoding` | Property-generated name quadruples round-trip without delimiter ambiguity. |
| SA12 | `LegacyNamespaceCompatibility` | Default RSpace/Eval namespace and migration-marker bytes remain unchanged. |
| SA13 | `FullNamespaceInjectivity` | Default/composite and composite/composite encodings distinguish unequal admitted quadruples. |
| SA14 | `DisjointPhysicalIsolation` | Independently prepared checkpoints and generated operation sequences preserve the other scope's exact history/cold/root maps. |
| SA15 | `SameLayoutReopen` | Reopening exact arbitrary names preserves contents, current root and generation. |
| SA16 | `LiteralMigrationColdSource` | Split-cold migration reads the mapped environment and literal cold database name. |
| SA17 | `NoScopedAmbientMigration` | Scoped layouts ignore unrelated generic `db` and unscoped v2 sources. |
| SA18 | `LegacyColdCompatibility` | Default layouts retain their existing named-cold then `db` fallback and applicable v2 discovery. |
| SA19 | `MigrationMarkerBinding` | Transplanted markers from another resolved namespace refuse. |
| SA20 | `UnrelatedEnvironmentLiteralNames` | Reserved-looking physical names in unrelated environments remain ordinary literal stores. |

**DERIVED algorithm.** Resolve all logical-role mappings exactly once, then resolve
their physical environments without creating directories. Validate configurations,
live footprints and migration-source footprints before any opening. Ordinary stores
use a literal-name route after validation; authority roles use the same resolved
names and paths. Repeated manager-local acquisition reuses its authority.

For default names, the migration namespace is the unchanged logical prefix.
Otherwise it is `b"\0rspace-authority-layout\0\x01"` followed by four UTF-8 fields
(logical prefix, history name, roots name, cold name), each preceded by its checked
eight-byte big-endian length. The proof reuses `ChannelImage.v`'s existing exact
`u64` round trip. NUL-free physical default names separate the legacy namespace
from the composite tag; framing establishes injectivity without a hash-collision
assumption. These bytes remain local migration metadata, not consensus preimages.

**Refinement boundaries.** Physical environment IDs in the model stand for paths
resolved by Rust. Existing symlink resolution, missing-tail handling, identical
checked/opened paths, actual LMDB transactions, read-only diagnostic routing,
allocation failure and migration-source immutability require executable evidence.
Managed paths must remain stable during opening; this repair is not a new
filesystem-confinement protocol. Protect eligible migration sources against
writable aliases while allowing the intended same-authority in-place cold case.
The mathematical finite lists are not a replacement for resident PathMap tries.
The existing stale-root check, replay expectations and archived pins must remain
unchanged. Previously collapsed unscoped state cannot be assigned to an arbitrary
scope or deleted automatically.

### Executable scope bindings and current evidence

The following non-ignored tests are implemented in `atomic_state_authority.rs`.
All ten `scope_layout_` tests pass in the broader **55/55** release state gate;
the two generated properties each execute 32 cases. The separate fresh-process
concurrent Casper archive gate at `9308e7685` passes **13 tests, 0 failures and
1 explicit capture ignored** in 541.57 seconds (artifact 500). Its test-only causal
diagnostic recognizer is qualified by the RA09 controls in the existing proof
report; no production replay expectation, fixture byte or root pin changes.
These are local acceptance results, not independent CI verification.

| Invariants | Executable binding |
|---|---|
| SA01, SA02, SA15 | `scope_layout_independent_names_and_reopen`, `scope_layout_disjoint_checkpoints_and_reopen`: exact independent names, cached authority identity, read-only facets, and retained root/generation on reopening. |
| SA03–SA09 | `scope_layout_preflight_refuses_before_open`: sixteen invalid-layout cases, each exercised authority-first and ordinary-first; no target directory appears. SA06 additionally uses the explicit checked conversions in the encoder; oversized natural lengths in the formal control cannot be represented by a Rust `usize` on supported 32/64-bit targets, so no artificial giant allocation is attempted. |
| SA10–SA13 | `scope_layout_namespace_matches_framed_reference`: independent framed decoding of generated UTF-8 names, per-field distinctions, and exact default RSpace/Eval bytes. |
| SA14 | `scope_layout_generated_mutations_preserve_other_scopes`: generated checkpoint sequences preserve every other-scope history, cold and catalog byte. The deterministic two-prepared-checkpoint case was red on the former adapter. |
| SA16, SA18, SA19 | `scope_layout_named_migration_and_marker_binding`: exact named split source, unchanged source bytes, default `db` compatibility, exact namespace bytes, marker transplantation refusal, and reopen stability. |
| SA17 | `scope_layout_scoped_migration_ignores_ambient_sources`: unrelated malformed v2 state is not imported; explicitly supplying v2 to a scoped private opener refuses before creating its destination. The named-source test separately proves that an unrelated populated `db` is ignored. |
| SA20 | `scope_layout_ordinary_literal_and_alias_routing`: an ordinary reserved-looking name in another environment remains writable and does not become an authority. |
| Physical-identity realization | `scope_layout_resolves_environment_aliases`: existing symlinks, missing/parent traversal, writable alias refusal before opening, same physical history/root environment, and dangling-symlink refusal. |
| Reusable-manager ownership | `scope_layout_custom_ownership_survives_shutdown`: all custom authority roles remain read-only after shutdown, with and without a retained old authority; already exposed ordinary handles prevent later authority acquisition. |

Model uniqueness quantifies over one exact mapping list; it is not a separate
permutation theorem. Model reopening means equal lookup over the same abstract
contents; the executable reopening tests and pre-existing process-exit legacy
fixture supply additional implementation evidence. Migration-source alias
protection is a separate concrete check, not silently attributed to the
live-destination alias theorem. The existing stale-root guard, archive fixtures,
and consensus pins are unchanged.
