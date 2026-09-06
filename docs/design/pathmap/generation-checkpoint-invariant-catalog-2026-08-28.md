# Generation checkpoint publication invariant catalog

Date: 2026-08-28

Status: formal phase model and executable phase-level refinement verified; checkpoint-pack truncation boundaries covered; per-write crash-I/O refinement remains open

Scope: fresh PathMap-DAG generation checkpoints for durable RSpace

## Purpose

This report is the traceability authority between the verified publication-phase models and their executable Rust refinement. It prevents an implementation or test suite from silently covering only the successful publication path. Every obligation at this abstraction level has a stable identifier, a property-based test target, and a negative control where one is meaningful. The phase model treats each pre-catalog file write and synchronization operation atomically; it does not claim the separately tracked byte-progress refinement for short writes, zero progress, interruptions, synchronization uncertainty, or loss of unsynchronized names after a power failure.

The required refinement order is:

1. verify the formal model and its controls;
2. extract every invariant into this catalog and executable property tests;
3. implement the Rust refinement;
4. require all proofs, properties, differentials, and focused integration tests to pass.

No Rust implementation claim is justified by the abstract model alone. Filesystem durability, checksums, bounded parsing, partial writes, and semantic admission are executable proof obligations.

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

## Formal-to-executable derivation matrix

Classification is explicit: **D** is directly entailed at the model's abstraction level, **P** is partially entailed with concrete executable refinement obligations, and **E** is executable-only. `EXEC_ONLY` is deliberate rather than a missing citation. A campaign-complete report may contain D, P, and E rows, but no unresolved mismatch class.

| ID | TLA+ symbols | Rocq theorem or lemma symbols | Class | Qualification |
|---|---|---|---|---|
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

The checkpoint was developed in the required refinement order. The TLA+ and Rocq models, all negative controls, and all non-vacuity witnesses were completed and checked first. The 74 stable IDs in this report were then bound to executable targets by `PROPERTY_BINDINGS`; `model_property_manifest_is_unique_and_exhaustive` mechanically proves exact ID equality among the catalog, derivation matrix, and binding registry, resolves every cited formal symbol, requires every base invariant and Rocq capstone to be cited, and verifies every non-vacuity configuration and gate entry. Only after that traceability gate was green was the production refinement changed.

Two implementation controls were deliberately observed red before their fixes:

1. Admission `required−1` initially reached `Published` rather than `DefinitelyNotPublished`, proving that semantic-admission limits were enforced after mutation. The admitted source now owns a derived, opaque requirements certificate. All 18 applicable semantic-admission axes, nine durable axes, and seven checkpoint-WAL axes are decided before successor cleanup; exact boundaries succeed and every positive `required−1` case preserves byte-identical old authority and seeded successor orphans. Per-state structural requirements use the componentwise maximum over current and all retained historical states.
2. A duplicated writer-lock file description initially kept the OS lock after the affine Rust lease was dropped. `DurableWriterLeaseV1::drop` now explicitly unlocks the shared open file description before descriptor destruction. This makes model invariant PUB-01 true even across the transient descriptor inheritance of a concurrent process spawn; the deterministic duplicate-descriptor control changed from red to green, and the formerly intermittent parallel publication tests became stable.

The focused executable gate passes exact 74-ID traceability, the complete 34-axis resource-boundary matrix, and selected-successor damage/repair without old-authority fallback. The parallel `rspace_plus_plus` library gate passes **392/392** in **52.20 s** under a 10 GiB hard memory cap, zero swap, and a 12-core quota. The complete package gate passes **817/817** across every registered library and integration target in **4 min 36.055 s**, peaking at **5.4 GiB** inside a 10 GiB hard cap with zero swap and a ten-core quota. The integrated formal gate passes Rocq, Verus, Z3, TLC, and mCRL2 under explicit per-tool memory ceilings. A fresh bounded TLC run on 2026-09-03 also checked the unchanged publication model with 8,344 generated states, 7,896 distinct states, depth 22, and no invariant error under a 2 GiB Java heap and 4 GiB RSS cap. Ignored `scratch/` transcripts retain the local measurement detail; the tracked scripts, models, configurations, properties, and this catalog are the reproducible authority. These results do not discharge unrelated open campaign tasks; the physical write/sync seam and its generated publication matrix are now covered by commits `018620a20`, `92c603126`, and `8c8f570fe`.

On 2026-09-03, commit `94513d0f0` added `every_checkpoint_pack_truncation_fails_before_authority`. It exercises every shorter prefix of a valid checkpoint pack, including all byte boundaries in the header, checkpoint `PREPARE`, checkpoint `COMMIT`, and indexed object bodies. The fully qualified test passed under a `MemoryMax=4G`, zero-swap, `CPUQuota=600%` systemd scope. Commit `a6f373419` additionally verifies that the positioned-write helper resumes correctly after a positive short write. Commit `c94efc7de` adds a real durable-pack truncation test through `DurableFileV1::read_exact_at`, asserting the typed positional short-read contract after the file is physically shortened. Commit `018620a20` adds test-only failpoints at the actual positioned-write and file-sync boundaries, with a focused test proving typed path-aware failures and no file extension after a failed write. Commit `92c603126` connects those failpoints to generation publication and restart admission, proving both physical failures remain definitely unpublished and restart selects the old authority. Commit `8c8f570fe` adds both physical failpoints to the existing table-driven publication outcome matrix; the matrix passes with the expected trace prefixes and old-authority restart result. These changes close the checkpoint-pack truncation, physical exact-read, physical write/sync interception, publication integration, generated matrix, and pure helper-loop slices; final formal artifact/trace documentation and the focused completion gate remain open.

## Refinement completion rule

The runtime may be declared conformant only when:

- every catalog row has a non-ignored executable property or a documented measurement gate;
- every historically red target is green;
- every negative control is known to make at least one corresponding property fail;
- all 14 non-vacuity witnesses execute;
- the integrated formal gate and the focused Rust property/integration suites pass under bounded resources;
- the stack-safety report, consensus change register, and PathMap engineering journal record the final wire, durability, compatibility, and measurement evidence.
