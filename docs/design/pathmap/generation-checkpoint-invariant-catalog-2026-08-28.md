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
counterexample's named invariant, so a parse error or resource failure cannot count as a pass.
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
| SI-01 | Exact replacement removes predecessor-only objects and counts shared objects once. | `incremental_inventory_tracks_replacement_removal_sharing_and_reuse` compares every sealed target and cold successor to full traversal. |
| SI-02 | Repeated transitions preserve the exact distinct-object partition, including empty states and reused digests. | `incremental_inventory_matches_cold_admission_at_every_history_step` generates count updates and removals with a fresh union ledger and physical reopen at every step. |
| SI-03 | Current semantic closure and retained semantic union are separate from retention metadata. | Both history checks compare manifest/current/retained fields independently; the existing DATA-inventory laws continue to account for the separate retention-manifest object. |
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
replacement for cold object/semantic admission. The same model will require separately
bound current-state, retained-semantic and metadata instances.

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
| `rspace++/tests/persistence/incremental_reachability_properties.rs` | `b5e97ee5226238566bbcb537f6dfede875ec21981baa078c91e9bd29722ef797` |
| `rspace++/tests/persistence/property_support.rs` | `b8e2aa87da27c9a8bfdc8c611a4e9245fb412475406a3398a5f26a8a28d0588b` |
| `rspace++/tests/persistence/generation_checkpoint_properties.rs` | `2f4080420cdd91c233e0137c2941abd67864d1190ec1cb6483f528138b65388b` |

These are abstract checked-arithmetic and test-model bindings. They do not yet
prove that a production successor maintains the exact ledger, authenticates
objects, or performs a failed copy-on-write trial without mutation.

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
unique-byte/kind inventories, or cold-resource certificates. Abstract termination
and the exact changed-frontier step formula are proved; runtime initialization,
byte access, lookup, validation, allocation and CoW costs need separate refinement.
The test-only graph scans and vector operations are not production complexity
evidence. `authenticated_cursor` means only
an exact suffix of an abstract parent's children; digest/length/kind, source identity
and namespace/exact-path authorization require their own concrete refinement.
Acyclicity and complete drainage remain essential for exact rooted support.

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
