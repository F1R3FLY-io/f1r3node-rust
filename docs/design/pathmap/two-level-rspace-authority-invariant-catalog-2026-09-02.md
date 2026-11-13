# Two-level RSpace authority invariant catalog

Date: 2026-09-02

Status: in-memory authority partially verified; direct RBA1 persistence remains an implementation gap

Scope: the selected two-level RSpace representation, its operations and lifecycle, authoritative matching, and direct RBA1 checkpoint refinement

## Purpose

This living engineering report is the non-duplicative traceability authority between the committed Rocq models and executable Rust properties for the two-level RSpace design. The outer `PathMap` stores one exact endpoint per loaded channel. Each real endpoint owns an independently guarded, channel-relative `Bag`, represented by `PathMap<Multiplicity>`. A loaded channel with an empty Bag remains observably different from an unloaded channel.

The original ten-module formal catalogs contain **171 distinct obligations**. They are normalized below into ten executable property families. The ordered-receive extension in TLRS-PROP-11 separately maps 22 additional capstones from task 8008, 15 from task 8009, 26 from task 8010 and 7 from task 8011. Each formal invariant has exactly one primary executable owner even when several formal modules derive related consequences. This prevents duplicated green tests from obscuring an untested seam.

`VERIFIED` means current Rust tests exercise the production behavior represented by the family. `PARTIAL` means useful executable evidence exists but one or more material refinements remain open. `GAP` means the production surface or its semantic property test does not yet exist.

Source-string checks, registry-name comparisons, documentation parsers, and `include_str!` assertions establish traceability only. They are **not executable semantic evidence** and cannot move a property from GAP or PARTIAL to VERIFIED.

## Mechanically verified formal inventory

The constructor counts below are taken from the `*_invariant_id` inductives in the committed sources.

| Formal artifact | Catalog | Count |
|---|---|---:|
| `TwoLevelRSpaceAuthority.v` | `two_level_invariant_catalog` | 23 |
| `TwoLevelRSpaceOperations.v` | `two_level_operation_invariant_catalog` | 22 |
| `TwoLevelRSpaceRepresentation.v` | `two_level_representation_invariant_catalog` | 13 |
| `TwoLevelRSpaceDelta.v` | `two_level_delta_invariant_catalog` | 13 |
| `TwoLevelRSpaceGuardedOwnership.v` | `guarded_ownership_invariant_catalog` | 40 |
| `TwoLevelRSpaceMatchTransaction.v` | `match_transaction_invariant_catalog` | 14 |
| `TwoLevelRSpaceCheckpointCodec.v` | `rba1_codec_invariant_catalog` | 13 |
| `TwoLevelRSpaceCheckpointStreaming.v` | `rba1_streaming_invariant_catalog` | 14 |
| `TwoLevelRSpaceCheckpointCompatibility.v` | `rba1_compatibility_invariant_catalog` | 13 |
| `TwoLevelRSpaceCheckpointRecovery.v` | `rba1_recovery_invariant_catalog` | 6 |
| **Total** |  | **171** |

The corresponding mutant companions provide formal causal witnesses. They do not by themselves prove that a Rust mutation is detected. A mutant is executable evidence only when a concrete Rust red control changes production behavior and the owning property fails for the intended reason.

## Normalized executable property catalog

### TLRS-PROP-01 — Exact channel and loaded-empty semantics

Status: **VERIFIED for the in-memory semantic boundary**

Formal owner set:

- Authority: exact target replacement, other-channel preservation, prefix-related isolation, loaded-empty preservation, and exact target multiplicity.
- Operations: exact upsert/delete, other-channel preservation, loaded/absent behavior, and absent-delete idempotence.
- Representation: projected exact lookup and prefix-related exact lookup.
- Match: loaded-empty preservation and exact outer lookup consequences.

Production surface: `RSpaceDataIndex` exact outer lookup, `replace_channel_at_marker`, `count_path`, `remove_path`, and `RSpaceDataStore::{is_loaded,set_channel}`.

Current executable evidence:

- `rspace++/tests/data_index/two_level_channel_authority.rs`: `exact_outer_endpoint_distinguishes_unloaded_from_loaded_empty`, `prefix_related_channel_identities_are_exactly_independent`, `replacing_one_channel_preserves_the_sibling_inner_anchor`, `removing_the_last_datum_preserves_loaded_empty_authority`, `absolute_count_and_remove_split_at_the_exact_channel_boundary`, and `operation_sequences_refine_an_exact_channel_multiset_oracle`.
- The generated `generated_exact_channel_relations_keep_absent_and_loaded_empty_authorities_distinct` property covers equal aliases and ancestor, descendant, and neighboring raw channel identities while rotating populated, loaded-empty, and absent roles.
- `rspace++/tests/data_index_spec.rs`: `data_store_distinguishes_unloaded_from_loaded_empty_channels`, `channel_replacement_is_transactional_and_removes_the_old_subtrie`, `channel_replacement_preserves_other_channels_and_loaded_empty_authority`, `removing_an_expanded_index_decrements_only_its_multiplicity`, and `removing_an_invalid_expanded_index_is_non_mutating`.

Remaining qualification: the generated exact-channel seam is now executable, but its formal causal mutants are not yet compiled as Rust red controls; the family therefore remains PARTIAL rather than VERIFIED.

### TLRS-PROP-02 — Aggregate metadata and multiplicity conservation

Status: **PARTIAL**

Formal owner set:

- Authority: target distinct count, target cardinality, total distinct conservation, and total cardinality conservation.
- Representation: real endpoint count, loaded count, `u64` multiplicity validity, and saturated cardinality laws.
- Operations and Delta: loaded-channel counts and well-formed published metadata.

Production surface: `RSpaceDataIndex::{loaded_channel_count,distinct_len,exact_cardinality,cardinality}` and checked aggregate preflight in exact and multi-channel replacement.

Current executable evidence: the generated operation-sequence oracle, repeated-identity tests, successful multi-channel delta, and underflow/refusal tests check important projections. `aggregate_metrics_preserve_loaded_distinct_exact_and_saturated_domains` crosses the `u64::MAX` boundary and directly verifies the private exact `u128` aggregate before and after a removal.

Remaining qualification: the small generated sequence oracle compares every public metric and the boundary fixture verifies the exact private aggregate above saturation. A generated large-multiplicity model and executable causal controls for each aggregate-field mutation remain open.

### TLRS-PROP-03 — Canonical authority, uniqueness, and candidate order

Status: **PARTIAL**

Formal owner set:

- Authority: canonical replacement, unique datum authority, flattened-oracle refinement, and candidate order.
- Representation: public projection exists exactly when all slots are real and every observable endpoint is real.
- Delta: canonical prepared output.
- Match: canonical candidate order.

Production surface: channel replacement preparation, canonical identity-byte catalog construction, candidate sorting, uniqueness validation, and byte-native pool access.

Current executable evidence:

- `canonical_candidate_order_is_independent_of_insertion_order`.
- `candidate_pool_uses_identity_bytes_for_order_and_decodes_on_first_access`.
- `decoded_entries_and_encoded_pool_have_the_same_exact_order_and_addresses`.
- `candidate_pool_keeps_multiplicity_in_the_subtrie`.
- The malformed two-level identity test in `data_index.rs`.
- `canonical_order_combines_insertion_permutation_multiplicity_prefixes_and_hash_collision` compares reversed construction orders across prefix-related channels under a forced digest collision, checks strict independent identity-byte order, preserves multiplicities, and proves decoded candidate-pool positions and stored addresses equal ordered entries.

Remaining qualification: lazy decoding is measured by the separate decode-count test rather than within the combined generated property, and the formal ordering mutants are not yet Rust red controls.

### TLRS-PROP-04 — Atomic operations, deltas, and publication

Status: **PARTIAL**

Formal owner set:

- Authority: accepted and refused batch atomicity and canonical publication.
- Operations: unique batch targets, canonicality, untouched channels, failure atomicity, deterministic transition, exactly one accepted publication, and one generation advance.
- Delta: all 13 delta invariants, including canonical preparation, exact channel partition, applicability, untouched preservation, failure atomicity, catalog effects, and exactly one root publication.

Production surface: `replace_channels_at_markers` and `RSpaceDataStore::apply_delta`, which stage channel-local Bags and catalogs before assigning the candidate index.

Current executable evidence: multi-channel success and failure tests, empty-delta identity preservation, malformed-path rejection, iterator-panic rollback, repeated/distinct atomic delta, and underflow rollback. `delta_publication_identity_distinguishes_success_empty_refusal_and_underflow` proves a successful nonempty delta installs a new outer root and that empty, cancelled, malformed, and underflowing deltas preserve outer and inner root identities. `catalog_lock_poison_precedes_clear_publication` proves `clear` cannot publish a half-cleared authority when derived-catalog access fails.

Open executable obligation: with existing publication instrumentation, prove successful nonempty operations publish exactly once and empty, malformed, underflowing, panicking, or otherwise refused operations publish zero times while authority bytes, metrics, catalogs, and issued snapshots remain unchanged. If the authoritative assignment cannot be observed, exactly-one publication is an implementation-observability gap.

### TLRS-PROP-05 — Catalog authority, freshness, and issued snapshots

Status: **PARTIAL**

Formal owner set:

- Authority and Operations: touched-channel locality, exact touched invalidation, untouched preservation, and issued snapshot preservation.
- Delta: every touched catalog invalidated or updated consistently.
- Match: exact selected-authority freshness, touched invalidation, untouched channel/catalog isolation, and issued-pool preservation.

Production surface: `RSpaceDataStore::candidate_catalogs`, incremental catalog metadata transitions, `needs_refresh`, immutable `CandidatePool` snapshots, and copy-on-write catalog updates.

Current executable evidence: candidate availability sharing, incremental catalog updates, target-only invalidation, old-pool validity, replay filtering, issued-pool immutability, and successful/refused multi-channel delta catalog observations. `tlrs_prop_05_exact_live_selection_not_catalog_generation_controls_freshness` drives the production enumeration-to-commit path across multiplicities one through four. Its positive arm changes the same exact channel's catalog with an unrelated identity and proves the held exact selection still commits; its causal negative arm exhausts the selected stored path and proves refusal preserves the complete hot-store snapshot and emits no COMM.

Formal correction: `TwoLevelRSpaceMatchTransaction.v` now treats request catalog identities as diagnostic provenance and gates preparation on positive live multiplicity for the exact selected path. `TwoLevelRSpaceMatchTransactionMutants.v` proves that trusting an issued snapshot after the selected occurrence is exhausted violates the invariant. This matches the production optimistic protocol under the hot-store write lock and deliberately rejects a whole-catalog generation gate: unrelated same-channel changes must not invalidate an otherwise live exact selection. Both modules verify axiom-free. Catalog rebuild counts and pointer identity remain optimization measurements, not semantic freshness requirements.

### TLRS-PROP-06 — Authoritative stack-safe match transaction

Status: **PARTIAL**

Formal owner set: all 14 MatchTransaction invariants. Exact lookup and loaded-empty consequences are exercised through TLRS-PROP-01; order through TLRS-PROP-03; catalog consequences through TLRS-PROP-05. This family exclusively owns the end-to-end composition: exact outer lookup, canonical inner candidates, stack-safe PDA match, positive multiplicity, selected `BagDelta`, strict scan progress, exactly one accepted publication, and refusal atomicity.

Current executable evidence: candidate continuation, reservation, catalog, and delta tests exercise components independently. The normal (non-feature-gated) hot-store property
`an_accepted_match_atomically_removes_the_selected_datum_continuation_and_join` now drives
`HotStore::apply_accepted_match` through a successful one-channel transaction and checks the
committed outcome, selected datum consumption, one-shot continuation consumption, and empty-join
cleanup. `an_accepted_multichannel_match_preserves_unconsumed_datum_multiplicity` adds a
two-channel transaction with repeated identity and verifies that one multiplicity remains on the
repeated channel while the single occurrence is consumed on the other. The companion cross-group
stale-capability property checks refusal atomicity. This is concrete production-entry-point
evidence, but it is not yet the exhaustive generated differential required by the formal
transaction capstone. `an_insufficient_datum_delta_is_stale_and_failure_atomic` and
`a_missing_required_persistent_path_is_stale_before_any_mutation` add causal underflow and missing
witness controls; both compare the complete hot-store state and checkpoint projection before and
after refusal. `an_accepted_match_preserves_a_persistent_continuation` covers the complementary
successful lifecycle rule: persistent continuation and join state survive while the selected datum
is consumed. `an_accepted_match_can_remove_the_selected_datum_and_publish_a_new_arrival` verifies
the same atomic successor can remove the selected datum and publish a distinct fresh arrival.
`a_datum_only_accepted_match_commits_without_a_continuation_capability` covers the optional-
continuation branch and requires a committed datum-only successor.
`replaying_a_committed_one_shot_match_is_stale_and_nonmutating` verifies that replaying the exact
proposal after its first commit is rejected by freshness/version checks without changing the
published state or checkpoint projection.
`a_continuation_source_mismatch_is_stale_before_a_nonempty_delta` adds the independent source-
metadata witness check and proves it also precedes mutation.

Open implementation and executable obligation: bind the actual hot-store accepted-match path to a generated property with an independent shallow recursive oracle. Assert selected identity, multiplicity-aware delta, untouched authority/catalog snapshots, loaded-empty outcome, strict PDA progress, and exactly one or zero publication. Reuse TLRS-PROP-05's exact-live-selection positive and exhausted-selection causal controls; do not reintroduce whole-catalog generation equality.

### TLRS-PROP-07 — Copy-on-write and guarded lifecycle

Status: **PARTIAL**

Formal owner set: all 40 GuardedOwnership invariants, including outer and inner clone/drop isolation, registered fresh anchors, structural preflight/disjointness, exact real-slot handoff, barrier separation, two-phase teardown, canonical-empty fast drop, generated PDA progress, constant control stack, zipper identity, lease/publication rules, and fail-stop atomicity.

Production surface: `GuardedPathMap<ChannelSlot, ChannelSlotDisposer, ChannelSlotBarrier>`, independently guarded inner Bags, copy-on-write clones, and generated iterative teardown.

Current executable evidence: sibling-anchor preservation, candidate authority sharing, incremental catalog anchor retention, clone splitting, cloned-store isolation, issued-pool isolation, and deep outer/inner clone-and-drop on a small stack.

Additional resource qualification (2026-09-17): `bag_allocation_spec` now checks
warm pool allocation independence over the finite distinct-count, width,
prefix-sharing, sibling, multiplicity and retained-pool matrix. Experiment 344
records 1,980 cold/warm observations and headless allocation profiles in J-167
of the PathMap engineering journal. Every warm issue requests six allocations
and 355 bytes, with no value decoding before the oracle. This is whole guarded
anchor sharing, not the old independent per-catalog Bag rebuild. Allocation
traffic evidence does not discharge the remaining lifecycle obligations below;
allocator-warm RSS differences are not proof of zero retained memory.

Open executable obligations:

1. generated unique/shared outer and inner clone/drop orders with exact observer equality;
2. a disposer ledger proving every real inner Bag is handed off exactly once and a synthetic barrier is never treated as a real slot;
3. deep normal, canonical-empty, and failure-path teardown under a small native stack;
4. fault injection at preflight, candidate construction, and publication boundaries proving no partial publication, borrowed disposal, or recursive raw drop.

### TLRS-PROP-08 — Direct RBA1 codec and compatibility

Status: **GAP**

Formal owner set: all 13 CheckpointCodec invariants and all 13 CheckpointCompatibility invariants.

Required behavior: direct canonical outer and inner ACT objects, exact decode/encode identity and injectivity, rejection of trailing bytes and invalid magic/version/barriers/counts/order, loaded-empty distinction, domain-separated hashing, authoritative RhoPath semantics, RCI2 shadow-only status, unchanged legacy hash/protobuf/bincode behavior, failure-atomic migration, one accepted migration publication, and current-format precedence.

Current executable evidence: none against a production RBA1 codec. Generic checkpoint and legacy codec tests do not refine this direct two-level format.

Open implementation and executable obligation: implement the production RBA1 codec/compatibility adapter, then add arbitrary canonical round trips, typed corruption and hostile-byte properties, byte-distinct loaded-empty/unloaded fixtures, direct ACT frame checks, domain separation, legacy compatibility, and migration atomicity. Direct RBA1 codec and compatibility remain GAP.

### TLRS-PROP-09 — Direct RBA1 streaming and resource bounds

Status: **GAP**

Formal owner set: all 14 CheckpointStreaming invariants.

Required behavior: exact chunk concatenation, chunked/contiguous decoder and sink equivalence, streamed/contiguous hash equivalence, no whole-RSpace materialization, peak ownership of at most one largest ACT object, exact frame count, loaded-empty frame preservation, no frame for unloaded channels, truncation rejection, constant control state, and no artificial traversal-depth limit.

Current executable evidence: generic canonical-object streaming properties establish useful infrastructure but do not exercise the RBA1 outer/inner stream or its PathMap resource claims.

Open implementation and executable obligation: after TLRS-PROP-08 exists, test arbitrary chunk partitions and truncations against the contiguous RBA1 oracle; verify exact outer/inner frame counts; instrument peak retained bytes/objects and control frames. Direct RBA1 streaming remains GAP.

### TLRS-PROP-10 — Direct RBA1 recovery and restart authority

Status: **GAP**

Formal owner set: all 6 CheckpointRecovery invariants and their recovery-seam causal mutants.

Required behavior: exact validation subject, exact object-inventory binding, independent physical and semantic generation binding, exact accepted-match successor generation, no partial authority before acknowledged readmission, and exact restart preservation including loaded-empty.

Current executable evidence: `rba1_checkpoint_recovery_properties.rs` verifies catalog traceability and explicitly records the production adapter as unimplemented. Generic durable recovery tests do not admit and compare a direct RBA1 two-level authority.

Open implementation and executable obligation: exact-subject mutation matrix, inventory substitution, independent generation mutation, crash cutpoints around object/install/catalog acknowledgement, accepted/refused semantic successor checks, invalid-current-format precedence, and loaded-empty restart equivalence. Direct RBA1 recovery remains GAP.

### TLRS-PROP-11 — Ordered receive fields and occurrence identity (2026-09-06)

Status: **PARTIAL — structural proof, existing-code codec witnesses, the
borrowed byte view (task 8009), the derived-index specification (task 8010) and
the production receive accelerator (task 8011) verified; selection and
qualification remain tasks 8012 and 8013**

The required proof slice, task 8008, extends the existing
[`TwoLevelRSpaceContinuationAuthority.v`](../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceContinuationAuthority.v)
with `OrderedReceivePositions`. Its 22 additional capstones are separate from
the original ten-module, 171-constructor inventory above. The companion adds 17
executable formal controls. When task 8008 landed, the proof runner required 33
closed assumption reports from each module, including their prior 11 and 16
reports. Independent `coqchk` checks the compiled modules and dependencies; no
admitted lemma or additional axiom is used.

**Byte-view extension (task 8009).** The same file adds the module
`OrderedReceiveByteView`. It models the canonical continuation record together
with its field bounds, the record's admission, and LegacyV1 and Rci2V2 receive
views over it, and proves 15 capstones, ORV-RC-01 to ORV-RC-15. The companion
module `OrderedReceiveByteViewMutants` refutes twelve wrong implementations,
ORV-MUT-01 to ORV-MUT-12, by closed computation. Its bridge lemma
`production_checks_are_admit_record` proves by reflexivity that the production
admission checks give `admit_record`, so each mutant differs from production in
exactly one check. The proof runner now requires 54 closed reports from the
authority file and 47 from its companion, up from 39 and 34, and its
`--ordered-receive-view` flag runs this family. Each formal mutant has a
production-source analogue, ORV-RS-01 to ORV-RS-12, generated by
[`scripts/generate-ordered-receive-view-mutant.py`](../../../scripts/generate-ordered-receive-view-mutant.py)
and applied only to an export of the tree; each was killed by its named test
(table below). The registry
[`formal/ordered_receive_view_obligations.generated.json`](../../../formal/ordered_receive_view_obligations.generated.json),
maintained by `scripts/check-ordered-receive-view-obligations.py` and aggregated
as the `ordered_receive_view` family, binds every capstone and mutant to its
executable owner. The registry is traceability; the semantic evidence is the
named tests. The design, including the hot-store read-lock contract, is the
integration doc's
[ordered receive byte view](../rspace/pathmap-bag-rspace-integration.md#ordered-receive-byte-view-task-8009)
subsection, and the evidence is journal entry
[J-254](pathmap-engineering-journal-2026-08-23.md#j-254--2026-10-29-the-ordered-receive-byte-view-task-8009).

**Derived occurrence index (task 8010).** Two new files specify the receive
occurrence relation that every derived receive index must equal.
[`TwoLevelRSpaceReceiveAccelerator.v`](../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAccelerator.v)
derives the relation from the ContinuationMap root and the versioned installed
input; the DataBag is not an input, the JoinBag is a checked projection, and the
installed input is not a fourth root. The model binds every accelerator to a
revision of fresh generation, store and installed-version tokens, sorts each
accelerator into one of six classes (Absent, Building, Foreign, Stale,
Incomplete and Coherent), and proves 26 capstones, RAC-RC-01 to RAC-RC-26: the
relation's soundness, completeness and positional factoring; exact receive
identifiers that are neither digests nor ordinals; maintenance equal to a
rebuild; revision soundness; a read rule that answers from the accelerator only
when it is Coherent and otherwise derives; failure atomicity; publication of the
catalog, occurrence and automaton artifacts at one revision; commit only through
the task-6013 accepted-match resolution; and a semantic successor that does not
depend on the accelerator. Its companion
[`TwoLevelRSpaceReceiveAcceleratorMutants.v`](../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorMutants.v)
proves a bridge lemma and refutes fifteen wrong implementations, RAC-MUT-01 to
RAC-MUT-15, so the proof runner requires 26 closed reports from the model and 16
from the companion. The TLA+ model
[`RSpaceReceiveAcceleratorCoherence.tla`](../../../formal/tlaplus/stack_safe_pda/RSpaceReceiveAcceleratorCoherence.tla)
checks eleven invariants, RAC-TLA-01 to RAC-TLA-11, over snapshot readers,
private builds, aborts, crashes, adoption at a reset and per-group tickets, and
each of its ten mutant configurations, RAC-TMUT-01 to RAC-TMUT-10, violates its
target invariant. No production accelerator exists before task 8011, so the
executable owner checks a test-only reference oracle,
[`rspace++/tests/support/receive_occurrence_reference.rs`](../../../rspace++/tests/support/receive_occurrence_reference.rs),
against real `HotStore` and `RSpace` instances, in 31 specification tests, five
Rholang tests and one persisted-manifest library test. Its 25 source controls
fall into two classes (table below). The seventeen harness controls change the
reference oracle or its adoption module and are classified `concrete_witness`; they are not production
evidence. The eight production controls change hot-store and RSpace source on
which the relation already depends and are classified `executable_refinement`.
The registry
[`formal/receive_accelerator_obligations.generated.json`](../../../formal/receive_accelerator_obligations.generated.json),
maintained by `scripts/check-receive-accelerator-obligations.py` and aggregated
as the `receive_accelerator` family, records the 87 identifiers: 26 RC, 15 MUT,
11 TLA, 10 TMUT and 25 RS. The registry is traceability; the semantic evidence
is the named tests. The runner's `--receive-accelerator` flag runs this family.
The design is the integration doc's
[receive occurrence index coherence](../rspace/pathmap-bag-rspace-integration.md#receive-occurrence-index-coherence-task-8010)
subsection, and the evidence is journal entry
[J-255](pathmap-engineering-journal-2026-08-23.md#j-255--2026-11-01-the-generation-bound-receive-occurrence-index-task-8010).

**Concrete field boundary.** Each position owns a complete `BindPattern`:
ordered terms, optional remainder and signed `free_count: i32`. It is not the
aggregate `Receive.bind_count`. The model uses mathematical integers for this
field; Rust representability is the full signed-i32 interval, not nonnegativity.
The body denotes the complete `TaggedContinuation`, including guard and random
state. Source-free identity consists of ordered patterns, body, persistence and
the raw signed peek set within one exact ordered channel group. The first
representative's `Consume` source is separately observable.

An empty aligned logical view has no bind. This does not change admission:
consume rejects empty groups, install permits them when arity agrees, and the
current RhoPathV2 continuation constructor rejects empty groups. Task 8009 must
preserve those distinct boundaries rather than making the constructor's
restriction a universal receive rule.

**Invariant-to-executable traceability.** Theorem names in this table are
within `OrderedReceivePositions`, except those carrying an ORV-RC identifier,
which are capstones of `OrderedReceiveByteView`. The primary Rust codec owner is
[`models/tests/cold_store_records.rs`](../../../models/tests/cold_store_records.rs).
Its task-8008 position witnesses composed canonical field bytes with
`RhoPathV2::channel_frames`; task 8009 re-pointed them to the production view,
through `admit_rci2_v2` over real `rho_path_v2_continuation_key` images and
through LegacyV1 store views of `Par` channels. The byte-view owner is
[`rspace++/tests/ordered_receive_view_spec.rs`](../../../rspace++/tests/ordered_receive_view_spec.rs).
Each row lists every capstone and source mutant it owns. Rows TLRS-RECEIVE-18 to
TLRS-RECEIVE-28 belong to task 8010: their RAC-RC theorems are in
`TwoLevelRSpaceReceiveAccelerator.v`, their RAC-TLA invariants are in
`RSpaceReceiveAcceleratorCoherence.tla`, and their owner is
[`rspace++/tests/receive_accelerator_spec.rs`](../../../rspace++/tests/receive_accelerator_spec.rs),
whose tests check the test-only reference oracle over real `HotStore` and
`RSpace` instances. The Rholang integration witnesses are in
[`rholang/tests/receive_accelerator_rholang.rs`](../../../rholang/tests/receive_accelerator_rholang.rs).
Those rows name their controls, whose kill results are in the receive
accelerator control table below. The typing invariant RAC-TLA-01 has no
executable owner, and the TLA+ mutants are proof-only controls of the
invariants beside which they are listed. Rows TLRS-RECEIVE-29 to
TLRS-RECEIVE-33 belong to task 8011, the production accelerator. Row 29 holds
the channel-scope capstones RAC-CS-01 to RAC-CS-07 of
[`TwoLevelRSpaceReceiveAcceleratorChannelScope.v`](../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorChannelScope.v)
and the refutations RAC-CSMUT-01 to RAC-CSMUT-04 of its companion
[`TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants.v`](../../../formal/rocq/stack_safe_pda/theories/TwoLevelRSpaceReceiveAcceleratorChannelScopeMutants.v),
which are proof-only. Rows 30 to 33 bind every task-8010 capstone and every
invariant from RAC-TLA-02 to RAC-TLA-11 to production witnesses. Their primary
owner is
[`rspace++/tests/receive_accelerator_production_spec.rs`](../../../rspace++/tests/receive_accelerator_production_spec.rs),
which checks the production accelerator over real `HotStore` and `RSpace`
instances; the others are the occurrence-index, binding and revision-token
library tests, the production test of the persisted-manifest module and
[`rholang/tests/receive_accelerator_production_rholang.rs`](../../../rholang/tests/receive_accelerator_production_rholang.rs).
Their controls, RAC-RS-26 to RAC-RS-50, are in the production control table
below, and the refinement argument is the
[production receive accelerator](../rspace/pathmap-bag-rspace-integration.md#production-receive-accelerator-task-8011)
subsection of the integration design.

| ID | Formal capstones | Executable witness and remaining refinement |
|---|---|---|
| TLRS-RECEIVE-01 | `positions_exact`, `indexed_bind_correspondence`; ORV-RC-08 `view_positions_are_exact_image_positions` | `ordered_receive_codec_preserves_arbitrary_signed_fields` compares every returned position against an independent ordered-channel oracle, including absent queries. Task 8009 applies the same relation to the borrowed view: `rci2_views_return_exact_positions_over_explicit_images` over explicit RCI2 keys, and `ordered_receive_legacy_store_views_match_typed_par_positions` over LegacyV1 store views of `Par` channels, whose positions equal the typed `Par` positions on its fixture. A universal equality of byte and typed positions is not claimed; it stays with task 8012. |
| TLRS-RECEIVE-02 | `positions_are_bounded`, `positions_are_unique`, `positions_are_increasing` | The same property compares the complete position vector and checks bounds and strict order. Formal controls detect dropped, duplicated and reordered positions. The view yields positions in one ascending walk over the channel frames, and the TLRS-RECEIVE-01 tests compare complete position vectors. |
| TLRS-RECEIVE-03 | `bind_at_preserves_all_fields`, `same_channel_does_not_erase_distinct_pattern_fields`; ORV-RC-07 `admitted_view_binds_refine_bind_at`; ORV-RC-14 `free_count_is_trailing_signed_word` | `ordered_receive_signed_fields_survive_repeated_channel_permutations` covers `[a,b,a]` and `[a,a,b]` with distinct terms/remainders and signed count extrema. `ordered_receive_fixture_detects_erased_and_shifted_fields` detects observer-insensitive fixtures. Task 8009: `view_binds_align_channel_pattern_and_effective_peek_by_position` checks that each bind pairs the channel frame, pattern bytes, exact signed free count and effective peek of one position, and `ordered_receive_view_admits_every_corpus_pattern_and_continuation` reads every corpus pattern's free count through `ReceivePatternLayout for BindPattern`. Source mutant ORV-RS-04 is killed. |
| TLRS-RECEIVE-04 | `bind_at_exists_exactly_in_range`, `mismatched_arity_refuses_every_position`; ORV-RC-02 `admitted_receive_refuses_mismatched_arity` | Existing `consume_arity_refusal` covers play/replay/install admission. The view refuses `ArityMismatch` before any position or bind can be observed: `admission_refuses_mismatched_arity_before_any_position`. Source mutant ORV-RS-01 is killed. The store keeps records whose pattern count differs from their channel count, and only the visitor reports them. |
| TLRS-RECEIVE-05 | `effective_peek_is_signed_membership`, `wide_position_does_not_wrap_to_signed_peek`; ORV-RC-15 `wide_view_position_does_not_wrap` | The positional fixture preserves asymmetric raw peeks and compares each effective flag. A position outside signed-i32 range must not wrap when testing membership; the general theorem has no traversal-depth bound. Task 8009: `ContinuationRecordView::effective_peek` is false above $`2^{31}-1`$ and otherwise binary-searches the ascending peek words; the unit tests `wide_positions_convert_without_wrap` and `effective_peek_search_matches_linear_membership` check the conversion and compare the search with linear membership. |
| TLRS-RECEIVE-06 | `coarse_peek_preserves_raw_nonemptiness` | `ordered_receive_raw_peeks_and_source_suffix_are_not_erased` keeps `{-1,7}` at arity three: all effective flags are false, but the coarse nonempty flag is true. Removing those raw peeks changes identity. The view keeps the raw words: `raw_peeks` and `coarse_peek` are separate from the effective flags. |
| TLRS-RECEIVE-07 | `source_replacement_preserves_identity_and_positions`, `distinct_source_remains_observable` | The same source-suffix test compares the exact common identity prefix and separately serialized source suffix, and checks both complete records through the decoder. The byte-view refinement is TLRS-RECEIVE-16. |
| TLRS-RECEIVE-08 | `first_identity_retains_head_source`, `appending_aliases_preserves_first_identity` | Existing `record_index::continuation_identity_lookup_retains_first_representative_and_loaded_empty` and the continuation/join lifecycle differential bind first-match retention to the current PathMap store. The model's record list is a ghost observation of one exact group, not a proposed list-backed implementation. The byte-view refinement is TLRS-RECEIVE-16. |
| TLRS-RECEIVE-09 | `installed_and_resting_positions_are_distinct` | `hot_store_spec::byte_equal_installed_and_resting_continuations_remain_distinct` keeps two byte-identical records in separate occurrence classes, refuses installed removal and removes only the resting occurrence. It reuses the existing `ResolvedTarget` constructors and hot-store API. Task 8009: `installed_continuations_are_not_visited_as_resting_records` shows that the resting visit never lends an installed continuation, even when a byte-identical resting record is visited. |
| TLRS-RECEIVE-10 | `empty_aligned_receive_has_no_bind`; ORV-RC-11 `empty_aligned_view_has_no_bind_or_position` | Existing empty-pattern codec fixture and arity admission tests. Task 8009: `empty_aligned_view_has_no_bind_and_v2_empty_group_stays_refused` admits an empty LegacyV1 group as an arity-0 view with no bind and no position, while the RCI2 parser still refuses an empty group with `EmptyChannelGroup`. No persisted empty-group format is added. Source mutant ORV-RS-11 is killed. |
| TLRS-RECEIVE-11 | `escaped_channel_occurrences_are_exact`; ORV-RC-09 `prefix_related_images_are_not_aliases` | Existing RhoPath all-byte, malformed-frame and count tests; `v2_wide_repeated_channel_positions_preserve_every_occurrence` tests prefix-related binary channels at arities 65, 257 and 4097. The formal lemma reuses the verified zero-escape frame round trip. Task 8009: `legacy_store_views_return_exact_ordered_positions_for_repeated_and_prefix_related_channels` checks that frame equality never aliases a channel with a prefix of its bytes. Source mutant ORV-RS-09 is killed. |
| TLRS-RECEIVE-12 | `checked_position_is_exact`, `successful_position_codes_cannot_alias`, `representable_arity_has_representable_occurrences` | Formal boundary controls accept zero, signed-i32-plus-one and unsigned-u64 maximum, and refuse negative/overflow values without wrap or saturation. Task 8009: the production position type `RhoPathReceivePosition(u64)` converts with the checked `index`, `signed_peek_key` and `at_index`, tested by `wide_positions_convert_without_wrap`. Source mutant ORV-RS-08 is killed. The reducer's `sources.len() as i32` peek keys (`rholang/src/rust/interpreter/reduce.rs:1460`) are consensus-visible and stay unchanged, flagged to tasks 8012 and 6021. |
| TLRS-RECEIVE-13 | ORV-RC-01 `encoded_record_admits_exact_fields`; ORV-RC-03 `admitted_bounds_are_ordered_and_inside_record`; ORV-RC-05 `admitted_peeks_are_canonical_signed`; ORV-RC-06 `admitted_persist_is_boolean` | Record admission over the encoder's field bounds. The 64-case property `prepared_record_bytes_equal_the_frozen_encoders` checks that prepared records equal the public encoders and that every field slice is exact. `malformed_field_bounds_and_peek_tails_are_refused` refuses nine malformed bound vectors and twelve malformed records (header, persist byte, peek-count tail, peek order and truncation) and admits two well-formed ones, including the signed-ascending peeks `(-1, 1)`. `malformed_framing_is_refused` covers ten LegacyV1 and five RCI2 framing errors. Source mutants ORV-RS-03, ORV-RS-07 and ORV-RS-12 are killed. |
| TLRS-RECEIVE-14 | ORV-RC-04 `admitted_peek_tail_is_checked_exactly` | The peek-count tail is computed in checked arithmetic before any peek word is read. After bodies of 0 and 4096 bytes, `peek_count_overflow_is_checked_without_wrap` refuses the counts $`2^{64}-1`$, $`2^{62}`$, $`2^{62}-1`$ and the smallest overflowing count with `PeekCountOverflow`, and reports the largest fitting count, $`2^{61}`$ and 1 as `PeekTailMismatch`. Source mutant ORV-RS-02 is killed. |
| TLRS-RECEIVE-15 | ORV-RC-10 `cross_profile_query_is_refused` | A view is admitted under exactly one channel profile, LegacyV1 or Rci2V2, and `positions` refuses a query of the other profile with `ProfileMismatch`: `profile_queries_are_explicit`. Source mutant ORV-RS-10 is killed. Store views are LegacyV1; RCI2 views come only from explicit keys until task 6021. |
| TLRS-RECEIVE-16 | ORV-RC-12 `source_suffix_is_exact_and_identity_is_source_free`; ORV-RC-13 `first_view_identity_retains_head_source_suffix` | The source suffix is cut at the admitted identity end, and the store derives the identity path, the primary path, the source reference and the source bytes from the admitted view. `prepared_record_bytes_equal_the_frozen_encoders` checks the split. `first_representative_and_exact_source_suffix_are_retained` shows that a second put of the same identity returns `Some(false)` and that the visit lends the first record's identity and source. `visit_order_reconstructs_historical_order_and_unloaded_groups_are_not_hydrated` rebuilds the historical order from the visited orders and shows that an unloaded group returns `Unloaded` without hydration. Source mutants ORV-RS-05 and ORV-RS-06 are killed. |
| TLRS-RECEIVE-17 | none: resource evidence | `rholang/tests/ordered_receive_view_rholang.rs` counts allocations with a counting global allocator. `view_admission_and_iteration_do_not_allocate` finds zero allocations at arities 1, 65 and 4097, `resting_visit_allocations_do_not_grow_with_group_size` finds equal allocations for 64 and 4096 resting records, and `deep_pattern_views_fit_a_256_kib_stack` admits and iterates a pattern nested 20000 levels deep on a 256 KiB stack. The field-bounds sidecar costs exactly $`16 + 8(n+2)`$ requested bytes per stored continuation of arity $`n`$ ([measurement](../stack-safety/measurements/ordered-receive-field-bounds-sidecar-2026-10-29.txt)). |
| TLRS-RECEIVE-18 | RAC-RC-01 `derived_occurrence_is_sound`; RAC-RC-02 `derived_occurrence_is_complete`; RAC-RC-06 `prefix_aliased_channels_have_disjoint_occurrences` | Association. Over generated groups in real `HotStore` instances, `reference_occurrences_are_sound_over_generated_groups` checks that every reference occurrence names a supported group, the exact nonempty positions of its channel and a target of that group, and `reference_occurrences_are_complete_over_generated_groups` checks the converse. `prefix_aliased_channel_images_never_share_occurrences` uses the prefix-alias images of TLRS-RECEIVE-11 and finds no occurrence shared by a channel and a byte prefix of it. The production index of task 8011 must pass the same tests through `AcceleratorUnderTest`. |
| TLRS-RECEIVE-19 | RAC-RC-03 `derived_positions_factor_through_group`; RAC-RC-04 `repeated_channel_is_one_occurrence_with_all_positions`; RAC-RC-05 `occurrence_capacity_is_position_multiplicity` | Factoring and capacity. `occurrence_positions_factor_through_the_group` checks that the relation is the union over the channel's groups and that every occurrence of a group carries that group's positions. `repeated_channels_yield_one_occurrence_with_every_position` covers `[a, b, a]` and `[a, a, b]`; control RAC-RS-07. The Rholang owner of RAC-RC-05, `compiled_join_capacity_agrees_with_occurrence_multiplicity`, shows that one datum on the repeated channel of a compiled join gives no COMM and two give one, with the bindings compared by position in both arrival directions, so the capacity equals the demand of the production join. The Rholang witness `compiled_repeated_joins_derive_exact_occurrences` derives the occurrences of a compiled repeated join whose binds are asymmetric and which peeks only at position 0. |
| TLRS-RECEIVE-20 | RAC-RC-07 `derivation_ignores_data_and_runtime_order`; RAC-RC-08 `join_root_projection_is_exact` | Roots. `derivation_ignores_data_and_authority_orders` changes the data and renumbers the authority orders through `set_state(snapshot())`, and the relation is unchanged. `join_bag_projection_matches_derived_groups` compares the JoinBag's groups of each channel with the derived groups. The JoinBag can transiently diverge from the ContinuationMap, because `remove_continuation` does not remove joins; RAC-RC-08 therefore assumes that the joins cover the targets, and the test compares only groups that have targets. |
| TLRS-RECEIVE-21 | RAC-RC-09 `derivation_is_order_neutral`; RAC-RC-15 `insertion_update_equals_rebuild`; RAC-RC-16 `nonfinal_removal_update_equals_rebuild`; RAC-RC-17 `final_removal_update_equals_rebuild`; RAC-RC-18 `install_and_uninstall_update_equals_rebuild` | Order neutrality and maintenance. `construction_order_does_not_change_the_relation_or_ids` builds equal roots in different orders and finds equal relations and identifiers, and its ordered lookup equals the historical order of `get_continuations`; control RAC-RS-09. `incremental_insertion_equals_rebuild`, `incremental_nonfinal_removal_equals_rebuild`, `incremental_final_removal_equals_rebuild_and_clears_the_group` and `install_and_generation_uninstall_equal_rebuild` compare the reference's maintained update with a rebuild after each operation; harness controls RAC-RS-02 and RAC-RS-08, and production controls RAC-RS-17, RAC-RS-18 and RAC-RS-19, which keep the group's epoch after an insertion, an install and a removal. The Rholang witness `both_matching_directions_maintain_the_derived_relation` checks the relation after COMMs triggered by a produce and by a consume. |
| TLRS-RECEIVE-22 | RAC-RC-10 `receive_id_resolution_is_exact`; RAC-RC-11 `distinct_records_have_distinct_receive_ids`; RAC-RC-12 `hash_bucket_resolution_is_collision_checked`; RAC-RC-13 `stale_revision_never_resolves` | Receive identifiers. `receive_ids_resolve_only_to_byte_identical_records`; `metadata_distinct_records_keep_distinct_ids`, over peeks, persistence, random state and source, control RAC-RS-15; `colliding_buckets_resolve_by_full_bytes`, control RAC-RS-01; and `ids_from_a_prior_revision_or_generation_never_resolve`, production control RAC-RS-22, in which `set_state` copies the old base epoch. The Rholang witness `installed_and_resting_receives_derive_distinct_occurrences` installs a receive beside a byte-identical resting consume and finds two targets. A production identifier may be stored compressed, but it must resolve to the full identity and source bytes (task 8011). |
| TLRS-RECEIVE-23 | RAC-RC-14 `first_identity_representative_is_preserved` | First source. `first_source_representative_survives_alias_insertion`: a second put of a known identity with another source returns `Some(false)`, keeps the first source and leaves the group's version unchanged; control RAC-RS-13. It lifts TLRS-RECEIVE-08 and TLRS-RECEIVE-16 to the derived relation. |
| TLRS-RECEIVE-24 | RAC-RC-19 `history_load_and_byte_equal_replacement_preserve_derivation`; RAC-RC-20 `reset_issues_fresh_generation_and_rebuilds`; RAC-RC-21 `same_revision_same_derivation` | Revision. `history_load_and_duplicate_put_are_relation_and_version_neutral`; `clear_and_set_state_invalidate_every_held_version_and_rebuild_equal`, production control RAC-RS-21, in which `clear` keeps the generation; and `equal_versions_imply_equal_group_relation`, over random traces and every group, production control RAC-RS-20, in which an accepted removal keeps the group's epoch. The Rholang witness `checkpoint_reset_and_soft_revert_rebuild_identical_occurrences` rebuilds an unchanged relation after a checkpoint, a reset and a soft revert. Production has no store-level revision, so the reference observes the whole group universe; a store-level revision is task 8011's first obligation. TLRS-RECEIVE-28 refines the adoption clause of RAC-RC-20. |
| TLRS-RECEIVE-25 | RAC-RC-22 `read_never_hides_an_enabled_receive`; RAC-TLA-03 `CoherentAcceleratorEqualsDerivation` (mutant RAC-TMUT-02); RAC-TLA-04 `AnswerIsSound` (RAC-TMUT-03); RAC-TLA-05 `NoEnabledReceiveHidden` (RAC-TMUT-04); RAC-TLA-06 `StaleAcceleratorNeverAnswers` (RAC-TMUT-05); RAC-TLA-07 `IncompleteAcceleratorNeverAnswers` (RAC-TMUT-06); RAC-TLA-08 `ForeignAcceleratorNeverAdopted` (RAC-TMUT-07) | Fail-closed reads. `reference_reads_equal_derivation_in_every_accelerator_state` owns RAC-RC-22 and RAC-TLA-05, control RAC-RS-03; `incremental_maintenance_equals_rebuild_over_generated_traces` owns RAC-TLA-03; `reference_answers_never_include_absent_receives` owns RAC-TLA-04, control RAC-RS-16; `stale_accelerators_fall_back_after_every_mutation` owns RAC-TLA-06, control RAC-RS-04; `unloaded_history_groups_make_the_accelerator_incomplete`, with a history group reported as `Unloaded`, owns RAC-TLA-07, control RAC-RS-06; and `foreign_generation_or_manifest_accelerators_are_never_adopted` owns RAC-TLA-08, control RAC-RS-05. Production selection that reads through this rule is task 8012's. |
| TLRS-RECEIVE-26 | RAC-RC-23 `failed_or_stale_update_publishes_nothing`; RAC-RC-24 `published_artifacts_share_one_revision`; RAC-TLA-02 `PublishedArtifactsShareOneRevision` (RAC-TMUT-01); RAC-TLA-10 `FailedUpdateIsNonMutating` (RAC-TMUT-09) | Publication and atomicity. `failed_update_leaves_store_and_accelerator_unchanged` owns RAC-RC-23 and RAC-TLA-10, control RAC-RS-11; `catalog_occurrence_and_automaton_publish_at_one_revision` owns RAC-TLA-02, control RAC-RS-10; and `same_count_group_replacement_never_reuses_a_stale_automaton` owns RAC-RC-24. That test is the first executable witness for the retained-set automaton cache predicate (`rspace.rs:1165-1167`); the `rspace.retained_automaton.cache_hit` metric confirms that the cached path was taken, and production control RAC-RS-24 removes the version comparison. Joining the production accelerator root to the private copy-on-write replacement of task 6079 is task 8011's. |
| TLRS-RECEIVE-27 | RAC-RC-25 `accelerator_candidate_commit_refines_accepted_match`; RAC-RC-26 `semantic_successor_is_accelerator_free`; RAC-TLA-09 `StaleCandidateNeverCommits` (RAC-TMUT-08); RAC-TLA-11 `SemanticIdentityIsAcceleratorFree` (RAC-TMUT-10) | Commit and semantic identity. `stale_accelerator_candidates_never_commit` owns RAC-RC-25 and RAC-TLA-09: an old ticket makes `apply_accepted_match` return `Stale` and leaves the snapshot unchanged; harness control RAC-RS-14, and production control RAC-RS-23, which deletes the version comparison at `hot_store.rs:1518`. `enabled_and_disabled_acceleration_select_identically` owns RAC-RC-26 and RAC-TLA-11, control RAC-RS-12. Selection through an accelerator is task 8012's. |
| TLRS-RECEIVE-28 | none of its own: it refines the adoption clause of RAC-RC-20 and RAC-TLA-08 | Persisted-manifest adoption (decision R1 of journal entry J-255). The library test module [`rspace++/tests/persistence/receive_accelerator_manifest_binding.rs`](../../../rspace++/tests/persistence/receive_accelerator_manifest_binding.rs) builds two real semantic state manifests from the canonical encoding of two different continuation states, stores each as a real object with a real `ObjectRefV1`, and binds an `AcceleratorManifestV1` to each through the production encode and parse round trip. It checks that the reference adoption rule adopts exactly when the bound `semantic_manifest` equals the current one as an `ObjectRefV1`, digest and encoded length, and that otherwise the accelerator is Foreign and the rebuilt relation equals the derivation; harness control RAC-RS-25 makes adoption ignore the bound manifest. Production adoption is task 8011's. |
| TLRS-RECEIVE-29 | RAC-CS-01 `join_coverage_completes_the_channel_scope`; RAC-CS-02 `complete_channel_scope_lookup_is_derivation`; RAC-CS-03 `exact_channel_read_is_current_sound_and_group_complete` and `covered_exact_channel_read_is_the_derivation`; RAC-CS-04 `global_coherence_implies_channel_coherence`; RAC-CS-05 `exact_channel_read_contains_the_joins_derivation`; RAC-CS-06 `framed_occurrence_keys_enumerate_exactly_the_channel_groups`; RAC-CS-07 `equal_store_tokens_imply_equal_revisions`; refutations RAC-CSMUT-01 `skipped_join_scope_check_is_refuted`, RAC-CSMUT-02 `skipped_lookup_scope_check_is_refuted`, RAC-CSMUT-03 `unframed_occurrence_keys_are_refuted` and RAC-CSMUT-04 `live_scope_join_check_is_refuted` | Channel scope (decisions D3 and D6 of task 8011). The model refines the task-8010 Incomplete test to the groups of the channel being read: a read answers only when the live join view of the channel is loaded (J1), every join group of the channel is loaded in the handle's own snapshot (J2), and every group visited under the channel's frame is loaded there (J3). The capstones are proof-only, and `Print Assumptions` reports the eight capstone theorems, the bridge and the four refutations closed. Their executable counterparts run on the production accelerator. `production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated` reads a channel through each of the three gaps and finds `Incomplete` for a current handle and `Exact` after hydration at the same revision; production controls RAC-RS-33 (J1), RAC-RS-31 (J2) and RAC-RS-32 (J3, the analogue of RAC-CSMUT-02). `production_held_handles_never_answer_for_groups_loaded_after_they_were_taken` and its library twin `held_handles_never_answer_for_groups_loaded_after_they_were_taken` check the handle-scope rule of D3 that RAC-CSMUT-04 refutes; control RAC-RS-50. `production_history_groups_without_joins_are_invisible_to_both_paths` checks RAC-CS-05 where the joins do not cover the targets: a history group whose joins were never written is invisible to the accelerator and to the joins derivation until it is loaded, and afterwards the accelerator answers every occurrence, a superset of the joins derivation. The library test `occurrence_keys_are_exact_under_prefix_aliased_images` checks the framed keys of RAC-CS-06; control RAC-RS-45, the analogue of RAC-CSMUT-03. `store_token_equality_implies_equal_receive_state_across_generated_traces` checks the conclusion of RAC-CS-07 on generated traces. |
| TLRS-RECEIVE-30 | RAC-RC-01 to RAC-RC-14 (rows TLRS-RECEIVE-18 to TLRS-RECEIVE-23) | The production accelerator over association, factoring, roots, order, identifiers and first source. `production_answers_equal_the_derivation_over_generated_stores` owns RAC-RC-01 and RAC-RC-02; `production_positions_come_from_the_group_prefix` owns RAC-RC-03; `production_repeated_channels_yield_one_occurrence_per_target` owns RAC-RC-04 and RAC-RC-05, control RAC-RS-34; `production_prefix_aliased_images_never_share_occurrences` and the library test `occurrence_keys_are_exact_under_prefix_aliased_images` own RAC-RC-06, control RAC-RS-45; `production_answers_ignore_data_and_authority_orders` owns RAC-RC-07; `production_exact_groups_are_the_join_groups_with_targets` and `production_history_groups_without_joins_are_invisible_to_both_paths` own RAC-RC-08; `production_construction_order_does_not_change_answers_or_ids` owns RAC-RC-09, control RAC-RS-36; `production_receive_ids_resolve_only_to_byte_identical_records` owns RAC-RC-10 and RAC-RC-12, control RAC-RS-26, and the library test `forged_alias_source_targets_never_resolve_at_current_tokens` also owns RAC-RC-10, control RAC-RS-40; `production_metadata_distinct_records_keep_distinct_ids` owns RAC-RC-11, controls RAC-RS-35 and RAC-RS-42; `production_ids_and_handles_from_a_prior_revision_or_generation_never_answer` owns RAC-RC-13, control RAC-RS-41; and `production_alias_puts_keep_the_revision_answers_and_first_source` owns RAC-RC-14. The Rholang witness `production_compiled_join_capacity_agrees_with_occurrence_multiplicity` also owns RAC-RC-05, and `production_compiled_repeated_joins_answer_exact_occurrences` and `production_installed_and_resting_receives_answer_distinct_occurrences` repeat the Rholang witnesses of rows TLRS-RECEIVE-19 and TLRS-RECEIVE-22 against the published accelerator. A production target shares the record's `Arc<StoredRecord>` or the installed slot's `Arc` and returns the full identity and source bytes, so the compressed identifier that TLRS-RECEIVE-22 allows resolves to the full bytes. |
| TLRS-RECEIVE-31 | RAC-RC-15 to RAC-RC-21 (rows TLRS-RECEIVE-21 and TLRS-RECEIVE-24) | Maintenance and revision. Four of the five mutators of `RSpaceContinuationStore`, `set_group`, `put_prepared`, `remove_path` and `install`, maintain the index in the same critical section as the mutation, and the fifth, `clear`, leaves it Absent. `production_insertions_maintain_the_published_accelerator` (RAC-RC-15), `production_nonfinal_removals_maintain_the_published_accelerator` (RAC-RC-16), control RAC-RS-27, `production_final_removal_clears_the_maintained_group` (RAC-RC-17), control RAC-RS-43, `production_installs_maintain_and_set_state_retires_the_published_accelerator` (RAC-RC-18) and `production_history_loads_extend_the_maintained_scope` (RAC-RC-19), control RAC-RS-44, compare the maintained index with a rebuild after each operation, and the library test `maintained_index_equals_a_fresh_build_across_generated_mutator_sequences` does so after every step of generated mutator sequences. `production_clear_and_set_state_retire_held_handles_and_rebuilds_equal_the_derivation` (RAC-RC-20), control RAC-RS-30, retires every held handle on `clear` and `set_state` and rebuilds the derivation; `production_equal_group_versions_imply_equal_group_answers` (RAC-RC-21) compares answers at equal group versions; and the library test `every_continuation_change_issues_the_bumped_group_epoch_as_a_fresh_store_token` (RAC-RC-21), control RAC-RS-49, checks that every continuation change issues a fresh store token $`\sigma`$, the store-level revision that TLRS-RECEIVE-24 left to task 8011 (decision D1). The Rholang witnesses `production_checkpoint_reset_and_soft_revert_rebuild_identical_occurrences` (RAC-RC-20) and `production_both_matching_directions_keep_the_published_accelerator_equal_to_a_rebuild` repeat those of rows TLRS-RECEIVE-24 and TLRS-RECEIVE-21 against the published accelerator. |
| TLRS-RECEIVE-32 | RAC-RC-22 to RAC-RC-26 and RAC-TLA-02 to RAC-TLA-11 (rows TLRS-RECEIVE-25 to TLRS-RECEIVE-27) | Fail-closed reads, publication, commit and semantic identity. `production_reads_equal_derivation_in_every_accelerator_state` owns RAC-RC-22 and RAC-TLA-05, control RAC-RS-28; `production_maintenance_equals_rebuild_over_generated_traces` owns RAC-TLA-03 and drives the production accelerator through the harness's `check_trace`; `production_answers_never_name_absent_receives` owns RAC-TLA-04; `production_held_handles_fall_back_after_every_mutation` owns RAC-TLA-06, control RAC-RS-29; RAC-TLA-07 is owned by the channel-scoped witnesses of row TLRS-RECEIVE-29, controls RAC-RS-31, RAC-RS-32, RAC-RS-33 and RAC-RS-50; `production_foreign_generation_or_manifest_accelerators_are_never_adopted` owns RAC-TLA-08, control RAC-RS-47; `production_refused_updates_leave_the_store_and_accelerator_unchanged` and the library tests `faulted_maintenance_publishes_nothing`, control RAC-RS-38, and `publication_refuses_a_stale_base_or_a_history_load`, control RAC-RS-46, own RAC-RC-23 and RAC-TLA-10; `production_index_and_catalog_publish_at_one_revision` owns RAC-RC-24 and RAC-TLA-02, control RAC-RS-37, and `production_same_count_group_replacement_never_reuses_a_stale_answer_or_handle` also owns RAC-RC-24; `production_stale_tickets_ids_and_handles_never_commit` owns RAC-RC-25 and RAC-TLA-09; and `production_accelerator_never_changes_semantic_identity` owns RAC-RC-26 and RAC-TLA-11, control RAC-RS-39. Production refines the model's answers but not its class tags: a handle may answer for one channel where the model's global test calls the snapshot Incomplete, so the witnesses of RAC-TLA-07 are channel-scoped. A failed maintenance leaves the index Absent instead of rolling the mutation back, so the witnesses of RAC-TLA-10 check that a refused update changes nothing and that a failed maintenance publishes nothing. Selection through this rule is task 8012's. |
| TLRS-RECEIVE-33 | none of its own: it refines the adoption clause of RAC-RC-20 and RAC-TLA-08 | Production adoption. `receive_accelerator_adoption` adopts exactly when the bound manifest equals the current one, and `persisted_receive_accelerator_adoption` parses a stored `AcceleratorManifestV1` and compares the `semantic_manifest` it binds with the current `ObjectRefV1`; bytes that do not parse are Foreign. The second test of the persisted-manifest module, `production_adoption_requires_the_stored_bound_semantic_manifest`, runs the production rule over the real sealed states and stored manifests of TLRS-RECEIVE-28. An accelerator bound to its own state, or to an identical state sealed in another order, is adopted and answers exactly as its bound state; the four other stored bindings, two across the states, one with a forged manifest length and one with a flipped digest, are Foreign; a truncated manifest, one with a trailing byte and a semantic manifest in its place never parse and are Foreign; and the production rebuild equals the current derivation. Control RAC-RS-48 compares only the digests and is killed by the forged length. `production_foreign_generation_or_manifest_accelerators_are_never_adopted` refuses an accelerator of another generation or manifest, control RAC-RS-47, the analogue of RAC-RS-25, and the Rholang witness `production_checkpoint_reset_and_soft_revert_rebuild_identical_occurrences` rebuilds identical occurrences after a checkpoint, a reset and a soft revert. Only unit tests call the persisted form until task 7931 adopts persisted accelerator manifests. |

The 17 new formal controls cover dropped/duplicated/reordered positions, channel
permutation, prefix aliasing, shifted patterns, erased terms/remainder/signed
count, shifted peeks, raw-peek normalization, arity truncation, last-source-wins,
source erasure, installed/resting collapse and both fixed-width boundary cases.
Rust field-mutant fixtures are observer-sensitivity controls: they demonstrate
that the asserted observations distinguish changed fields. They are not a claim
that mutation testing rewrote the production implementation.

**Production source mutants (task 8009).** The twelve ORV-RS mutants do rewrite
production source. Each applies one change to
`rspace++/src/rspace/ordered_receive.rs` in an export of the tree, never
committed, rebuilds, and runs the specification tests (the module's unit tests
for ORV-RS-08); the run must fail the named test. All twelve were killed on tree
`0a9310de` in a debug build with seed `41318009`, and the unmutated controls
passed 13 of 13 specification tests and 2 of 2 unit tests. The run log is
`target/perf/logs/8009/11987-negctl-20261029T153723Z.log` (SHA-256
`f07867e4b8ed28a53d47c14424086c1c301b48f32600bf03243dcbfcc2af7ba4`), and each
mutant's own log is identified below. Logs under `target/` are not durable; the
pgmcp artifact of task 11987 holds this table.

| Mutant | Formal mutant | Production change | Killing test | Log SHA-256 |
|---|---|---|---|---|
| ORV-RS-01 | ORV-MUT-01 | the alignment compares the channel count with itself | `admission_refuses_mismatched_arity_before_any_position` | `06e243304c99890264d8cd545a5f2ebf2bcd1740fde2b4d380c2c5bdbde938ff` |
| ORV-RS-02 | ORV-MUT-02 | the expected identity end wraps instead of refusing an overflowing peek count | `peek_count_overflow_is_checked_without_wrap` | `e730b8f3efe1cfef1410602258a554d2adbe06b2d6ec84d87cbd2423907dbdd3` |
| ORV-RS-03 | ORV-MUT-03 | the header's pattern count is compared modulo $`2^{32}`$ | `malformed_field_bounds_and_peek_tails_are_refused` | `b40e99c4614cfaaf9f5764585c48dd4a57845afe9cca85a303bc344482af9d6f` |
| ORV-RS-04 | ORV-MUT-04 | every pattern slice ends one byte early | `view_binds_align_channel_pattern_and_effective_peek_by_position` | `0481d286227198d0cca7577d707c65425846a3d57560173bed3a0e886ea106f2` |
| ORV-RS-05 | ORV-MUT-05 | the identity includes the source | `first_representative_and_exact_source_suffix_are_retained` | `2a04a7c803e9c5fb820f69d38a3fa8fdd6d9f893046c12daff28b176c62dfdca` |
| ORV-RS-06 | ORV-MUT-06 | the source suffix starts at the body end | `prepared_record_bytes_equal_the_frozen_encoders` | `a552d2cab17b382046b5ae4cd711793416200052d1ae70dcdde223af04122101` |
| ORV-RS-07 | ORV-MUT-07 | the ascending-peek check accepts every pair | `malformed_field_bounds_and_peek_tails_are_refused` | `50a2679696169b79eb5e0c0a013bd4453c6ed95f95d68663d6ece499487fb5e7` |
| ORV-RS-08 | ORV-MUT-08 | a position wraps to a signed 32-bit peek key | `wide_positions_convert_without_wrap` | `c5d0876c7fa1fffb0add298e31c1cff47040b0575c427379184c7683e43951e4` |
| ORV-RS-09 | ORV-MUT-09 | a frame also matches a query that is only its prefix | `legacy_store_views_return_exact_ordered_positions_for_repeated_and_prefix_related_channels` | `ceb7d34adce3f4998f2f26171f6184543e6dc80d848df32457a799b9e1f6a894` |
| ORV-RS-10 | ORV-MUT-10 | a query of the other profile reads the view's frames | `profile_queries_are_explicit` | `fba3ea157fe6c1cba52967d3a6e175a3e8868021970dd5985a1b917b48487422` |
| ORV-RS-11 | ORV-MUT-11 | admission refuses every record without patterns | `empty_aligned_view_has_no_bind_and_v2_empty_group_stays_refused` | `b19b08f70ed73264b23e099c13b87eb8c717693396cf4c35246e813318079410` |
| ORV-RS-12 | ORV-MUT-12 | every nonzero persist byte reads as persistent | `malformed_field_bounds_and_peek_tails_are_refused` | `35b04bad57bd5b8747fcad764810223be92af66592c7858455202b60c998a77d` |

**Receive accelerator controls (task 8010).** Each of the 25 RAC-RS controls is
one fault. [`scripts/generate-receive-accelerator-mutant.py`](../../../scripts/generate-receive-accelerator-mutant.py)
applies it to an export of the tree, never committed; the export is rebuilt in a
debug build and the named test is run, which must fail. The seventeen harness
controls change the test-only reference oracle,
`rspace++/tests/support/receive_occurrence_reference.rs`, or, for RAC-RS-25, its
adoption module `rspace++/tests/support/receive_accelerator_adoption.rs`. RAC-RS-01 to RAC-RS-15
are the executable analogues of the refutations RAC-MUT-01 to RAC-MUT-15, and
each refutation is owned by the killing test of its analogue. RAC-RS-16 leaves a
removal unmaintained, and RAC-RS-25 makes adoption ignore the bound manifest.
The harness controls show that each test observes the property it owns; they are
classified `concrete_witness` and are not production evidence. The eight
production controls change `rspace++/src/rspace/hot_store.rs` or
`rspace++/src/rspace/rspace.rs`, at anchors that the checker requires to occur
exactly once, and are classified `executable_refinement`.
All 25 controls were killed on tree `1199984c` in a debug build with seed
`41318010`. The unmutated tree passed 31 of 31 specification tests and the
library test in the release gate, and each of the 22 distinct named tests alone
in the debug baseline. The run log is
`target/perf/logs/8010/red-11997-20261101T035318Z.log` (SHA-256
`005818573afb7041700e09cb344821ea77872c7be73b34255c4adc8af821c2fb`), and each
control's own log is identified below. Logs under `target/` are not durable. pgmcp task 11997 holds
sanitized copies: the run log is artifact 1309, the control, baseline and
skeleton logs are artifacts 1312 to 1359, and artifact 1363 maps each log to
its raw and sanitized SHA-256 and its artifact. The completion commit of task
8010 re-ran all 25 controls on its own tree with the same verdicts; pgmcp task
11998 records that run.

| Mutant | Class | Formal counterpart | Change | Killing test | Log SHA-256 |
|---|---|---|---|---|---|
| RAC-RS-01 | harness | RAC-MUT-01 | a receive identifier resolves by its hash bucket alone | `colliding_buckets_resolve_by_full_bytes` | `f43ea07c5a216e303c379af962e7780192fb3313b0697604696119ddb8b4cb06` |
| RAC-RS-02 | harness | RAC-MUT-02 | a receive identifier is the record's ordinal in its group | `incremental_nonfinal_removal_equals_rebuild` | `15282d3978e5c8dd8658dc0f4c7e3010ad117d87ded38e54b58668499d3206bb` |
| RAC-RS-03 | harness | RAC-MUT-03 | an absent accelerator answers the empty set | `reference_reads_equal_derivation_in_every_accelerator_state` | `bf00779b0d3ffa51e6e451934c6c5f9a68165598fa193bf81da823ec65ef3f5c` |
| RAC-RS-04 | harness | RAC-MUT-04 | a stale accelerator answers from its old entries | `stale_accelerators_fall_back_after_every_mutation` | `87cd48e982a84c52d82f14ae72dbbf18e6516fa525245dd990612f3609c6edde` |
| RAC-RS-05 | harness | RAC-MUT-05 | an accelerator of a foreign generation or manifest is adopted | `foreign_generation_or_manifest_accelerators_are_never_adopted` | `883538a16dfc82c78a79dc6a5dbcfccfe5c5ddef5ce634bafaa4531c25c69c8c` |
| RAC-RS-06 | harness | RAC-MUT-06 | an incomplete accelerator answers | `unloaded_history_groups_make_the_accelerator_incomplete` | `d658ed6eff1cfb900e30a9881039a8817082c382366a55d5c58a2285afd28a1a` |
| RAC-RS-07 | harness | RAC-MUT-07 | a repeated channel keeps only its first position | `repeated_channels_yield_one_occurrence_with_every_position` | `1239da0c142ac69b7454be56a08db18a93c39745998bba7365f3d2be70611500` |
| RAC-RS-08 | harness | RAC-MUT-08 | an installed record and a byte-identical resting record share one target | `install_and_generation_uninstall_equal_rebuild` | `32ba5d5a38334781f1677599151ed4fe4ff21c67550b6f186ab2301338104334` |
| RAC-RS-09 | harness | RAC-MUT-09 | the derivation depends on construction order | `construction_order_does_not_change_the_relation_or_ids` | `5c8a7915d46f6b50404ac8bef0f274bebb59a6b84322ee7fcf82667f0e1b3f06` |
| RAC-RS-10 | harness | RAC-MUT-10 | the automaton artifact keeps the previous revision | `catalog_occurrence_and_automaton_publish_at_one_revision` | `d2d5f3cc1f9b50ab8a9907e800729666523633402e61932e813bd860e228aabe` |
| RAC-RS-11 | harness | RAC-MUT-11 | a failed update changes the roots and leaves the accelerator marked current | `failed_update_leaves_store_and_accelerator_unchanged` | `a0017a67b8570cf7f0ee2d6ca395abe39c1b9987f5d5ac21b3f39060ff67aa0a` |
| RAC-RS-12 | harness | RAC-MUT-12 | the semantic identity includes the accelerator | `enabled_and_disabled_acceleration_select_identically` | `8cd9febe02a2049809073564a810fc30c92965a8c276bc92f8bad6fb790d41fc` |
| RAC-RS-13 | harness | RAC-MUT-13 | an alias insertion replaces the first source | `first_source_representative_survives_alias_insertion` | `1b2189f81a3fdec5d7e1e955fa8ce1ac37d40f5e7c2ec47f2aeba2b2581e54ab` |
| RAC-RS-14 | harness | RAC-MUT-14 | a candidate commits without the epoch check | `stale_accelerator_candidates_never_commit` | `8e9193336223023e696d0ddc3e8ec7b4eafe7cb1dcad68ffef800fe02ba96199` |
| RAC-RS-15 | harness | RAC-MUT-15 | the target erases the peeks | `metadata_distinct_records_keep_distinct_ids` | `4b3a4f3c1a8e3d2077f02d9dab4426c8804de376e1d88d285db8a2ce81042d03` |
| RAC-RS-16 | harness | RAC-TLA-04 | a removal leaves its occurrences in a current accelerator | `reference_answers_never_include_absent_receives` | `1de1668b94f728723af90e1c5eb19b5412a326dd84fe5d091ba175a8b4ae0681` |
| RAC-RS-17 | production | RAC-RC-15 | `put_continuation` keeps the group's epoch after an insertion (`hot_store.rs:1034-1035`) | `incremental_insertion_equals_rebuild` | `3760c1d7f376266954c892692636253fb6b29ec54e4a2b8e4ab1989572c959d8` |
| RAC-RS-18 | production | RAC-RC-18 | `install_continuation` keeps the group's epoch (`hot_store.rs:1047-1048`) | `install_and_generation_uninstall_equal_rebuild` | `6e0956879db1b272724ee8986d96440af4e4b05404ae167f15bb2357974f64ae` |
| RAC-RS-19 | production | RAC-RC-16 | `remove_continuation` keeps the group's epoch (`hot_store.rs:1084-1086`) | `incremental_nonfinal_removal_equals_rebuild` | `a254bd6c48b217c51a398c2757d314bce3db0733156791c76b2827abce3ed757` |
| RAC-RS-20 | production | RAC-RC-21 | an accepted ordinary removal keeps the group's epoch (`hot_store.rs:1593`) | `equal_versions_imply_equal_group_relation` | `1fcf586a324ae2348fafd4259b07165e86ad650b0463b48cf7cf57b14393e9a1` |
| RAC-RS-21 | production | RAC-RC-20 | `clear` keeps the generation (`hot_store.rs:1972-1973`) | `clear_and_set_state_invalidate_every_held_version_and_rebuild_equal` | `85815c5f0c9aaaa1d8ffe9eb17f0b4698743bf800a91c85a09fbb4011a314025` |
| RAC-RS-22 | production | RAC-RC-13 | `set_state` copies the old base epoch into its replacement (after `hot_store.rs:2027`) | `ids_from_a_prior_revision_or_generation_never_resolve` | `50790601a39369db6513c009915349f7a77f62e72d1e7d10b9e0c4601401105c` |
| RAC-RS-23 | production | RAC-RC-25 | the stale-ticket check accepts a changed version (`hot_store.rs:1518`) | `stale_accelerator_candidates_never_commit` | `816ef243b283a8474ab7120b9ba7fa228d75f890124c9401ba3dab9e812dac7a` |
| RAC-RS-24 | production | RAC-RC-24 | the retained-set automaton cache ignores the version (`rspace.rs:1166-1167`) | `same_count_group_replacement_never_reuses_a_stale_automaton` | `5733fb7a1c6d15c87afe4cfdfb7c952b62ba4bd53826db3e95f98fc572b8cfd0` |
| RAC-RS-25 | harness | adoption clause of RAC-RC-20 and RAC-TLA-08 | adoption ignores the bound semantic manifest | the persisted-manifest library test, `rspace::persistence::tests::receive_accelerator_manifest_binding` | `4808eead86469d379cf1c95621f108f3d15edf5ab238deb8fd68906c5e8efb36` |

**Production receive accelerator controls (task 8011).** Each of the 25
controls RAC-RS-26 to RAC-RS-50 is one fault in production source, in
`rspace++/src/rspace/receive_accelerator.rs`, `rspace++/src/rspace/hot_store.rs`
or `rspace++/src/rspace/record_index.rs`, at an anchor that the checker requires
to occur exactly once. The same generator applies it to an export of the tree,
never committed; the export is rebuilt in a debug build, and the killing test
runs alone on one test thread with seed `41318011` and must fail. The checker
requires each killing test to be a production witness of every formal id that
its control names, and every control is classified `executable_refinement`.
Sixteen controls are the analogues of the harness controls RAC-RS-01 to
RAC-RS-16, RAC-RS-47 is the analogue of RAC-RS-25, RAC-RS-32, RAC-RS-45 and
RAC-RS-50 are the analogues of the refutations RAC-CSMUT-02, RAC-CSMUT-03 and
RAC-CSMUT-04, and RAC-RS-33, RAC-RS-44, RAC-RS-46, RAC-RS-48 and RAC-RS-49 have
no earlier analogue. The controls were run in five batches, one for each pgmcp
task that added them, each on that task's own tree, which the export equalled
before and after the run. Each distinct killing test first passed alone on the
unmutated export, and all 25 controls were killed. Each batch's run log is
listed below, and each control's own log,
`target/perf/logs/8011/red-<task>/<control>.log`, is identified in the table
after it. Logs under `target/` are not durable; each pgmcp task holds sanitized
copies of its batch's logs as artifacts.

| Task | Controls | Tree (commit) | Run log | SHA-256 |
|---|---|---|---|---|
| 12387 | RAC-RS-26 to RAC-RS-30 | `5447174466e5548d0f82d87b19f9dbcdd88c3725` (`1f08c7f0e`) | `target/perf/logs/8011/red-12387-20261101T143811Z.log` | `c459e88486a2a7106964f6469fa0b43d8ab7055a79c7416aca01ee5e4ee45301` |
| 12388 | RAC-RS-31 to RAC-RS-35 | `b5cef1603cbfcd04788d8d34b1dcfbe179c69328` (`e49600937`) | `target/perf/logs/8011/red-12388-20261101T145127Z.log` | `b5776a06295216afe9c18a1c909d0f6b59051107e58430af50760a4a8115ba3f` |
| 12389 | RAC-RS-36 to RAC-RS-40 | `767caf50f1882c20be44b39b0d2380034ec01e57` (`b753b1063`) | `target/perf/logs/8011/red-12389-20261101T145911Z.log` | `9a1315db465fe67a37642bd57ea297d3bddb735bae10c947de2da8db1fb040ea` |
| 12390 | RAC-RS-41 to RAC-RS-45 | `b2913d420f1b5381e0f0f9d63ae54a7df4e359b2` (`2db834b7e`) | `target/perf/logs/8011/red-12390-20261101T151607Z.log` | `f2e5b39ee60a30811bfbdf783b5a41adf9ef51dc8da42d00afdff955da2bf94d` |
| 12391 | RAC-RS-46 to RAC-RS-50 | `fadb7802094ba69270c4e41b3792e7eb4f7d08f0` (`30fff03a6`) | `target/perf/logs/8011/red-12391-20261101T153334Z.log` | `2c58c22b4906ee160e1c63e3e2eaadd0d8e5264e34b70538f0bbd0f914eb0a8e` |

| Mutant | Class | Formal counterpart | Change | Killing test | Log SHA-256 |
|---|---|---|---|---|---|
| RAC-RS-26 | production | RAC-RC-10, RAC-RC-12; analogue of RAC-RS-01 | `resolve` accepts a target whose identity has the same length (`receive_accelerator.rs:315`) | `production_receive_ids_resolve_only_to_byte_identical_records` | `7feaaaa62fd4a9c5be8589597b4b1dba0103f3f90c27335a732278d26beffb52` on tree `5447174466e5` |
| RAC-RS-27 | production | RAC-RC-16; analogue of RAC-RS-02 | the removal of a group's first ordered record is treated as final (`hot_store.rs:1265`) | `production_nonfinal_removals_maintain_the_published_accelerator` | `c0ec28bc2fa4b5816d60497e86dde0e9fc0becfbad0c884635ee1edefcb3496d` on tree `5447174466e5` |
| RAC-RS-28 | production | RAC-RC-22, RAC-TLA-05; analogue of RAC-RS-03 | a read without a handle on an Absent store answers the exact empty set, materialized from an empty rebuilt store (`hot_store.rs:1329-1330`) | `production_reads_equal_derivation_in_every_accelerator_state` | `7cbdb1b1a380a2dab6f8aafa04a117d9845e1a2fa8bf0031d4e880eec41e7519` on tree `5447174466e5` |
| RAC-RS-29 | production | RAC-TLA-06; analogue of RAC-RS-04 | the store-token test is ignored, so a stale handle answers from its old index (`receive_accelerator.rs:131`) | `production_held_handles_fall_back_after_every_mutation` | `fadb5eb88e3ea4a511b6854284e8a1a1518469411f20ef3cd5d1da51fc98645f` on tree `5447174466e5` |
| RAC-RS-30 | production | RAC-RC-20; analogue of RAC-RS-05 | the generation test is ignored (`receive_accelerator.rs:129`) | `production_clear_and_set_state_retire_held_handles_and_rebuilds_equal_the_derivation` | `bb2996360ce75ad68eaef82b4e8fd08a88626c41f7571414289a8b4962f1f4f4` on tree `5447174466e5` |
| RAC-RS-31 | production | RAC-TLA-07; analogue of RAC-RS-06 | J2 skips the join groups outside the handle's scope (`receive_accelerator.rs:371-374`) | `production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated` | `c9245a3ad1ab07bfabff1d8081523a9aaf501d2d784056d3fafa335cd4a8e549` on tree `b5cef1603cbf` |
| RAC-RS-32 | production | RAC-TLA-07; analogue of RAC-CSMUT-02 | J3 skips an installed-only group outside the handle's scope, so the channel answers without it (`receive_accelerator.rs:149`) | `production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated` | `f5eb22aafc3e4c4da98d7e9a2b89c9784d858ccf5ceb3aed2dcd0f705e4ee069` on tree `b5cef1603cbf` |
| RAC-RS-33 | production | RAC-TLA-07; no earlier analogue | J1 treats an unloaded join view as one without join groups (`receive_accelerator.rs:377-378`) | `production_scope_gaps_fall_back_as_incomplete_until_the_channel_is_hydrated` | `3e71ba425388c155f45f43be9598b6e69b39af01bb1e4b6122ffc4bf86e18bc9` on tree `b5cef1603cbf` |
| RAC-RS-34 | production | RAC-RC-04; analogue of RAC-RS-07 | a repeated channel keeps only its first position (`receive_accelerator.rs:429`) | `production_repeated_channels_yield_one_occurrence_per_target` | `954fc75f6c16cbe48a9a155e8308144781cae15a23549930ea5d3ae098220ea9` on tree `b5cef1603cbf` |
| RAC-RS-35 | production | RAC-RC-11; analogue of RAC-RS-08 | an installed target is dropped beside a resting record with the same identity and source bytes (`receive_accelerator.rs:175-182`) | `production_metadata_distinct_records_keep_distinct_ids` | `df17f52eb33474260cf395a7ab38449071346b93c2010b84c45450ee1c376a31` on tree `b5cef1603cbf` |
| RAC-RS-36 | production | RAC-RC-09; analogue of RAC-RS-09 | resting targets are answered in reverse historical order (`receive_accelerator.rs:179`) | `production_construction_order_does_not_change_answers_or_ids` | `7d3e5974d5a2af642e6751f3de0eb1b0f0d81e30fb3745267a4307712cca2513` on tree `767caf50f188` |
| RAC-RS-37 | production | RAC-RC-24, RAC-TLA-02; analogue of RAC-RS-10 | a handle is stamped with the generation's base epoch as its store token (`hot_store.rs:687`) | `production_index_and_catalog_publish_at_one_revision` | `b3ffeb326dc358dda3842b28b7228475e0f3fe55fd819298ecb7369aff38e23e` on tree `767caf50f188` |
| RAC-RS-38 | production | RAC-RC-23, RAC-TLA-10; analogue of RAC-RS-11 | a failed maintenance publishes its replacement (`record_index.rs:1637`) | `faulted_maintenance_publishes_nothing` (library, `rspace::receive_accelerator::occurrence_index_tests`) | `64ff2fa23cdcb6cabfa2e1407aa039ac08d06fab2258ba3455c18b60a6730a91` on tree `767caf50f188` |
| RAC-RS-39 | production | RAC-RC-26, RAC-TLA-11; analogue of RAC-RS-12 | a rebuild clears the pending journals (`hot_store.rs:1295`) | `production_accelerator_never_changes_semantic_identity` | `405e6c1aed7d1644f786bbfae3b24cd7b4e1141bc37fe942a158161d08fcbf47` on tree `767caf50f188` |
| RAC-RS-40 | production | RAC-RC-10; analogue of RAC-RS-13 | target equality ignores the source bytes (`receive_accelerator.rs:261-262`) | `forged_alias_source_targets_never_resolve_at_current_tokens` (library, `rspace::hot_store::receive_accelerator_bindings_tests`) | `0d65d8c6970e8f103cdd07ac94735cbfe3bde5260a282b1ecc0ced5232e3d9a4` on tree `767caf50f188` |
| RAC-RS-41 | production | RAC-RC-13; analogue of RAC-RS-14 | `resolve` skips the revision test (`receive_accelerator.rs:311`) | `production_ids_and_handles_from_a_prior_revision_or_generation_never_answer` | `e13673a7041bcf573333a6c19651f40c6d3144d5c72eb967efe717ae1f854b6a` on tree `b2913d420f1b` |
| RAC-RS-42 | production | RAC-RC-11; analogue of RAC-RS-15 | a resting target's identity is its continuation body, which erases the persist flag and the peeks (`receive_accelerator.rs:238`) | `production_metadata_distinct_records_keep_distinct_ids` | `ac2a611a9c6b2a6e0433ea2c282183c5da93a42b1b739a8b4fe0a08fe38966d1` on tree `b2913d420f1b` |
| RAC-RS-43 | production | RAC-RC-17; analogue of RAC-RS-16 | a final removal keeps the group's keys (`record_index.rs:1631`) | `production_final_removal_clears_the_maintained_group` | `3529d0e74a4b0d5af5be5626273a68a9def5c78dbf522c6eca7339cb44dd23c8` on tree `b2913d420f1b` |
| RAC-RS-44 | production | RAC-RC-19; no earlier analogue | a history load skips maintenance (`record_index.rs:1261-1262`) | `production_history_loads_extend_the_maintained_scope` | `0bdb4fdc6a25a224b422fc6e84fdd84845cc8cb5f4ab1defed72e7d37652cdee` on tree `b2913d420f1b` |
| RAC-RS-45 | production | RAC-RC-06; analogue of RAC-CSMUT-03 | a key is built from the decoded raw channel image instead of its escaped frame (`receive_accelerator.rs:411`) | `occurrence_keys_are_exact_under_prefix_aliased_images` (library, `rspace::receive_accelerator::occurrence_index_tests`) | `84d738a0545a41bc035e9cc773883ea33e0431b981984913bbf002f6c44cc4f8` on tree `b2913d420f1b` |
| RAC-RS-46 | production | RAC-RC-23, RAC-TLA-10; no earlier analogue (D4) | publication ignores the load count (`hot_store.rs:738`) | `publication_refuses_a_stale_base_or_a_history_load` (library, `rspace::hot_store::receive_accelerator_bindings_tests`) | `97c45e171e242299eacdf49af078747636711203073cd9688d94f831bb80ae5d` on tree `fadb7802094b` |
| RAC-RS-47 | production | RAC-TLA-08; analogue of RAC-RS-25 | adoption ignores the bound manifest (`receive_accelerator.rs:388`) | `production_foreign_generation_or_manifest_accelerators_are_never_adopted` | `95dff1a5663c82bf434354feb0fd32d4e4947b10c9bdc06a90e5868447130ff8` on tree `fadb7802094b` |
| RAC-RS-48 | production | RAC-RC-20, RAC-TLA-08; no earlier analogue | persisted adoption compares only the manifest digests (`receive_accelerator.rs:404`) | `production_adoption_requires_the_stored_bound_semantic_manifest` (library, `rspace::persistence::tests::receive_accelerator_manifest_binding`) | `7fa27b5416abe9ae1d2f02eee2d8819fea5714e5a3a9ad81d38614091496d67e` on tree `fadb7802094b` |
| RAC-RS-49 | production | RAC-RC-21; no earlier analogue | a continuation change bumps its group epoch but keeps the previous store token (`hot_store.rs:947-948`) | `every_continuation_change_issues_the_bumped_group_epoch_as_a_fresh_store_token` (library, `rspace::hot_store::receive_revision_tokens_tests`) | `c1eda485609cc8c4fb485988e7822eac347d749a00803a3c33d7a543d9019142` on tree `fadb7802094b` |
| RAC-RS-50 | production | RAC-TLA-07; analogue of RAC-CSMUT-04 (D3) | J2 inspects the live store's scope (`hot_store.rs:697`) | `production_held_handles_never_answer_for_groups_loaded_after_they_were_taken` | `84519f165a56ed9f770ac373fc9310720fcb462d481c57469370d45ee2b9cf59` on tree `fadb7802094b` |

**Codec observation repair.** The prior continuation differential compared a
machine decoder with an independently derived decoder reading the same emitted
record. It could miss a shared encoder permutation. The strengthened helper also
compares every decoded field with the original record and compares complete bytes
with the independent frozen tuple
`(patterns, continuation, persist, peeks, source)`. The actual generated stable
encoders for each `BindPattern` and `TaggedContinuation` are also compared with
their frozen bincode field encodings. Exact decoded pattern and continuation
bytes check all metadata, including guard/random state, in addition to semantic
equality. The signed-field property
uses 256 cases with seed `41318008` in the recorded repeat; deterministic extrema,
negative-only peeks, source aliases and field-erasure cases supplement generation.

**Proof boundary.** These capstones prove structural field/position laws and exact
escaped-channel identity; the task-8009 capstones add record admission, field
slicing, channel profiles and position width; and the task-8010 capstones add the
derived occurrence relation, exact receive identifiers, revision binding, the
fail-closed read rule and one-revision publication for a functional model of the
roots, whose lifecycle under snapshot readers and private builds the TLA+ model
checks up to its bound. They do not formally verify every generated protobuf or
bincode implementation, the complete RhoPathV2 outer parser, semantic RCI2 channel
equality or selected-COMM equivalence, and they do not extract or verify the Rust
view, any Rust accelerator, PathMap traversal, the split record index, the
hot-store `RwLock` and its `Arc::ptr_eq` epochs, the canonical encoders,
BLAKE2b-256, or allocation and stack use; the task-8010 model idealizes
`ObjectRefV1` identity as equality of the roots. The codec, byte-view and
accelerator tests provide concrete executable correspondence, and the source
mutants show that the checks observe each modelled property; the task-8010
harness controls exercise the reference oracle, not production. Neither is proof
extraction. Task 8011 adds the channel-scope capstones, which refine the
task-8010 classification to the channel being read and, like the others, are
proved for a functional model. It binds every task-8010 capstone and every
invariant except RAC-TLA-01 to production witnesses, and its production
controls fault the Rust accelerator itself. That is refinement by testing, not
by extraction or proof: the Rust accelerator, its PathMap traversal and the
hot-store lock remain unverified, and the bounded TLA+ model is unchanged for
the reasons given under
[why the TLA+ model is unchanged](../rspace/pathmap-bag-rspace-integration.md#why-the-tla-model-is-unchanged).
Activation, selection and qualification stay with tasks 8012, 8013 and 6021,
so the family remains PARTIAL.

**Consumers and pins.** The byte view's consumers are tasks 8011 (the
PathMap-derived catalogs), 8012 (selection) and 7931 (the durable path). The view
changes no persisted byte. The runner's `--ordered-receive-view` flag pins that
with `serializer_byte_goldens` (5 tests), `serializer_par_byte_goldens` (7),
`event_hash_bytes_differential` (11), `event_hash_encoder_differential` (5),
`catalog_hash_preimages_are_exact_raw_identity_bytes` (1) and `rho_path_spec`
(24). The derived-index specification's consumers are tasks 8011 (the production
accelerator, which must implement `AcceleratorUnderTest` and pass the 31
specification tests and the persisted-manifest test), 8012 (selection through the
fail-closed read rule) and 8013 (qualification). Task 8010 changes no production
source and no persisted byte. Besides its own 31 specification and five Rholang
tests, the runner's `--receive-accelerator` flag pins the behavior that the
relation relies on with `hot_store_spec` (57 tests), `ordered_receive_view_spec`
(13), the `rspace::record_index` library tests (20), the
`accepted_match_formal_bindings` library tests (17), the continuation and join
lifecycle equivalence (1), `guarded_matching_tests` (18), `replay_comm_order` (3),
`ordered_receive_view_rholang` (4) and `matching_lifecycle_acceptance` (2).
Task 8011 implements that accelerator: `ProductionAccelerator`, the production
adapter of the harness, implements `AcceleratorUnderTest` for published and
detached handles. The flag now also compiles the two channel-scope files with
closed counts 8 and 5 and checks them with `coqchk`, and it runs the production
specification (31 tests), the occurrence-index (14), binding (18) and
revision-token (5) library tests, both tests of the persisted-manifest module
and the production Rholang tests (5); the checker pins every count. Task 8011
adds no semantic root, persisted format, history key, state root, RhoPath
version or wire message, and the registry's `accelerator_isolation` entry makes
the checker refuse accelerator names in persistence, history, codec,
candidate-index, hot-store-action and checkpoint sources. The production
accelerator's consumers are tasks 8012 (selection through its channel-scoped
reads), 8013 (qualification) and 7931 (the durable path). Task 7931 must keep
routing every change of the continuation tries through the five mutators of
`RSpaceContinuationStore`, `set_group`, `put_prepared`, `remove_path`, `install`
and `clear`, which maintain the published index or leave it Absent, so that a
published index stays equal to a rebuild; only unit tests call the persisted
adoption form until task 7931 adopts persisted accelerator manifests.

## Current production gaps

The committed formal family is ahead of `RSpaceDataIndex` in four material respects:

1. no production RBA1 codec, streaming writer/reader, compatibility adapter, or recovery/readmission path exists;
2. no single Rust transaction API currently exposes the complete matcher-to-authoritative-commit composition proved in Rocq;
3. exactly-one publication is optional instrumentation rather than a continuously asserted production invariant;
4. most exact-once disposer, barrier, abort, and fail-stop ownership obligations lack executable event-ledger controls.

The private exact `u128` aggregate is exercised from the crate-private child property module so the saturated `u64` API cannot conceal the boundary case.

## Absolute datum address and flat-delta refinement (task 6588)

**2026-09-08, formal and executable refinement.** The preparation model
constructs exact Stored/Fresh addresses and parses absolute keys. Its original
`fully_parsed_partition_is_exact_flat_partition` theorem restates the recorded
computation; it does not independently prove that grouped channel counts equal
flat absolute-key counts. The new grouped and trace theorems below discharge
that distinction independently; no runtime defect or algorithm replacement was
needed for this slice.

The following new invariants extend, rather than replace, the existing 24 closed
preparation capstones. Each theorem is in `TwoLevelRSpaceMatchPreparation.v`.

| ID | Formal statement | Executable owner and status |
|---|---|---|
| TLRS-ADDR-01 | `parsed_relative_offset_is_exact`: the marker length is in bounds and skipping that prefix yields exactly the channel-relative key | `rho_path_spec::datum_offsets_refine_exact_prefix_and_raw_identity_suffix`; 128 generated cases and shifted-offset control pass |
| TLRS-ADDR-02 | `parsed_identity_offset_is_exact`: raw identity length is bounded by absolute length and the end-relative slice is exactly the raw identity, without a second unescape | The same passing property, with empty/nonempty identity handling and an independent shifted identity-offset control |
| TLRS-DELTA-01 | `flat_record_removal_is_exact_opposite_cancellation`: removal cancels existing additions before extending removals | `flat_delta_refinement_tests::flat_delta_operation_traces_refine_independent_logical_counts`; 128 generated traces pass, with separate addition/removal counts checked after every record |
| TLRS-DELTA-02 | `flat_record_addition_preserves_other`: addition preserves both counts at every other absolute key | The same passing trace property checks all 12 fixture endpoints, including endpoints untouched by the current record |
| TLRS-DELTA-03 | `flat_record_removal_preserves_other`: removal preserves both counts at every other absolute key | The same passing trace property checks both normalized supports and exact distinct-support cardinalities |
| TLRS-DELTA-04 | `flat_record_addition_preserves_normalization` and `flat_record_removal_preserves_normalization`: successful recording preserves unique, positive, bounded, disjoint supports | The trace property checks the positive and negative parts of an independent signed-count oracle separately; `flat_delta_record_overflow_preserves_both_supports` checks refusal without mutation in both polarities |

The amended theory compiles with **29 closed reports** and passes independent
kernel checking after rebuilding its exact imports. Proof runs use a 2-GiB hard
memory cap, zero swap and a two-core quota. The compile log has SHA-256
`40fa88cdd153b800cde0c1d021c121a576e24c207b8203edad96e6afc3cd16b0`; the fresh-import
and kernel-check log has SHA-256
`801f8db3bb64797c1fb70cb2253c37c36da0fc285758085f255a6cba5d03c7b4`.
The existing four DataIndex delta tests also pass, but are not substitutes for
the independent trace property.

The final `rho_path_spec` release target passes all **22 tests**, including the
128-case offset property with seed `41316588`, under a 6-GiB hard memory cap and
zero swap. Its log has SHA-256
`13e5dad11ec12323ce72eccb38e9d99ef02b1b50331be9f582c5fab60ff687e1`.
These are current byte-boundary checks, not a new latency/RSS benchmark or
complete accepted-match transaction verification.

The subsequent normalized-support extension compiles with **31 closed reports**;
its compile log has SHA-256
`6eda5bea911eb616d45737b83ead130c9bab2ff031fccda6fee67f1f0dbd79e6`.
This is the intermediate compile result, preceding the grouped proof extension.

The independent oracle in `rspace++/tests/data_index/flat_delta_refinement.rs`
passes **6 tests**, including 128 generated traces with seed `41316588`.
It indexes expected counts by logical channel/datum pairs rather than by the
production partition function. A compensating tail cancels negative balances so
every generated case tests successful store application as well as its original
trace's success or refusal. Tests compare addition and removal counts separately,
all endpoint counts, distinct supports, exact aggregate cardinality, loaded
channel membership, and frozen pre-update candidate catalogs. Two endpoints can
each reach `u64::MAX` without expanding multiplicities into individual elements.
Explicit controls distinguish wrong channels and offsets, polarity swaps,
skipped/doubled cancellation, omitted entries, overflow, a valid entry followed
by malformed framing, and a Fresh-shifted navigation ordinal used as identity.
The run log has SHA-256
`042deb95bc81cc27cfef57f719cdbd3a71ac7aa0ba980d73c8b20929052f6d18`.

The accompanying existing-witness run passes **76 test executions**: 26 DataIndex,
19 two-level channel authority, 6 candidate reservation, 3 Bag multiplicity/delta,
and all 22 RhoPath tests. Its log has SHA-256
`62fea3de9edeee2b21f4b106328db688788157f640b99ce2801bed9d1841b3bb`.
Both Rust runs use the 6-GiB hard memory cap and zero swap. These are correctness
results, not performance measurements or full-workspace qualification.

The completed preparation theory has **38 closed reports** and passes kernel
checking. The finite-trace client has **4 closed reports**; the named formal
mutation client has **12 closed reports**. Their obligations are:

| ID | Formal statement | Executable owner |
|---|---|---|
| TLRS-DELTA-05 | `grouped_partition_preserves_both_flat_sides`: each successfully parsed query has exactly its original addition and removal counts | `flat_delta_operation_traces_refine_independent_logical_counts`, including actual store application |
| TLRS-DELTA-06 | `normalized_parsed_partition_exists` and `partition_parsed_marker_domain`: normalized fully parsed input succeeds with unique, normalized groups and exactly the touched markers | The trace property’s guaranteed successful tail, exact endpoint/aggregate counts, and loaded-channel membership |
| TLRS-DELTA-07 | `grouped_partition_has_exact_source_endpoints`: every nonzero grouped endpoint witnesses a same-polarity original absolute entry with identical multiplicity | Exact support cardinalities, all fixture endpoints, and omitted/wrong-channel/polarity controls |
| TLRS-DELTA-08 | `normalized_partition_refusal_is_exactly_parse_refusal`: normalized input cannot gain a grouping-only refusal | `valid_prefix_then_malformed_delta_preserves_published_authority`, underflow cases in generated traces, and existing failure-atomicity witnesses; arithmetic application refusal remains distinct from parsing/grouping refusal |
| TLRS-DELTA-09 | `finite_flat_record_trace_refines_logical_sum` and `normalized_supports_are_separate_signed_projections`: every successful finite trace preserves normalized supports and refines an independent signed operation sum | Checks after every record against the independent logical-pair oracle |
| TLRS-DELTA-10 | `finite_flat_trace_partitions_into_exact_logical_supports`: complete traces compose with grouping to produce the positive and negative parts separately | The full generated trace followed by real `DataStore::apply_delta`, including frozen CoW snapshots and full-width multiplicity witnesses |

The formal mutation client includes a successful exact-partition witness before
checking wrong channel grouping, shifted boundaries, swapped polarities, omitted
groups, non-disjoint supports whose net count is zero, skipped/doubled cancellation,
same-key arrival cancellation, record overflow, late malformed paths, malformed
Fresh construction, invalid frame escapes, and non-byte constructor input.

The focused command is
`bash scripts/check-stack-safe-pda-proofs.sh --match-preparation`.
It rebuilds exact imports, requires the stated closed-report counts, rejects proof
placeholders, kernel-checks all three clients, and requires exactly 82 nonignored
runtime witness executions. The final composed gate exited **0**, including all
38 preparation, 4 trace, 12 control, and 21 datum-framing closed reports,
an explicit successful kernel check, and all 82 tests with no failures or ignored
cases. Its log has SHA-256
`ce86a1a4940f74be67eeb4504027604ad3d6f67895520393f7236acb6dd57dc0`.
The preparation source checked by this run has SHA-256
`acb2d056173c68706a8d7b0129e52d299151532f3a10d834b5eb9c65baf85226`.
Earlier client attempts failed on module import, working-directory, implication
application, and literal-scope details; none were treated as passing evidence.
The successful gate is local execution evidence, not a daemon-issued verification
receipt or evidence that the entire persistence campaign is complete.
Equality of net signed counts alone is insufficient because erroneous equal
entries on both sides could cancel observationally.

Production `apply_delta` parses and groups privately in one pass, whereas the
model parses every path before grouping. The required refinement is equal
successful results and unchanged published state on refusal, not identical
private allocation or phase order. Fresh-induced catalog ordinal shifts are
navigation metadata, distinct from both byte offsets above; original Stored
addresses must remain unchanged by reservations or exclusions. No runtime
algorithm, parser, dependency or byte format is modified in this foundation.

## Minimum gate before the in-memory two-level checkpoint

The in-memory production checkpoint requires all of the following additions:

1. TLRS-PROP-01 generated prefix-relation and loaded-empty exact-channel property — **implemented**;
2. TLRS-PROP-02 aggregate-metadata oracle, including exact `u128` and saturated `u64` boundary evidence — **implemented for current operation generators**;
3. TLRS-PROP-03 combined canonical candidate-order/identity property — **implemented**, with combined lazy-decode instrumentation still open;
4. TLRS-PROP-04 publication-count and strong-failure-atomicity property — **root-identity and failure behavior implemented; exact transient publication-event counting remains open**;
5. TLRS-PROP-05 exact selected-authority freshness model, positive same-channel interference control, exhausted-selection refusal control, and immutable issued-pool evidence — **implemented**;
6. TLRS-PROP-06 end-to-end accepted/refused authoritative match property;
7. TLRS-PROP-07 lifecycle ledger and bounded-stack fault-injection properties.

If the checkpoint includes persistence authority, it remains blocked until TLRS-PROP-08, TLRS-PROP-09, and TLRS-PROP-10 have production implementations, positive properties, and causal red controls. Traceability registries cannot satisfy this gate.

## Living-document protocol

Any change to the two-level authority layout, exact channel framing, aggregate metadata, candidate ordering, catalog freshness, match publication, guarded ownership, RBA1 bytes, streaming resource model, migration, generation semantics, or restart admission must update this catalog in the same change. Stable `TLRS-PROP-*` identifiers are never silently repurposed. New formal invariants must be assigned to exactly one executable owner; new executable tests must name the formal IDs they refine. A status moves to VERIFIED only when production behavior, a positive executable property, its causal red control, and relevant bounded-resource evidence are all enforced by the repository gate.
