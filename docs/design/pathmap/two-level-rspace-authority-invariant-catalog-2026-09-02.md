# Two-level RSpace authority invariant catalog

Date: 2026-09-02

Status: in-memory authority partially verified; direct RBA1 persistence remains an implementation gap

Scope: the selected two-level RSpace representation, its operations and lifecycle, authoritative matching, and direct RBA1 checkpoint refinement

## Purpose

This living engineering report is the non-duplicative traceability authority between the committed Rocq models and executable Rust properties for the two-level RSpace design. The outer `PathMap` stores one exact endpoint per loaded channel. Each real endpoint owns an independently guarded, channel-relative `Bag`, represented by `PathMap<Multiplicity>`. A loaded channel with an empty Bag remains observably different from an unloaded channel.

The original ten-module formal catalogs contain **171 distinct obligations**. They are normalized below into ten executable property families. The ordered-receive extension in TLRS-PROP-11 separately maps 22 additional capstones from task 8008 and 15 from task 8009. Each formal invariant has exactly one primary executable owner even when several formal modules derive related consequences. This prevents duplicated green tests from obscuring an untested seam.

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

Status: **PARTIAL — structural proof, existing-code codec witnesses and the
borrowed byte view (task 8009) verified; the derived index, selection and
qualification remain tasks 8010–8013**

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
Each row lists every capstone and source mutant it owns.

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
escaped-channel identity, and the task-8009 capstones add record admission, field
slicing, channel profiles and position width. They do not formally verify every
generated protobuf or bincode implementation, the complete RhoPathV2 outer parser,
semantic RCI2 channel equality, index revision publication or selected-COMM
equivalence, and they do not extract or verify the Rust view, PathMap traversal,
the hot-store `RwLock`, or allocation and stack use. The codec and byte-view tests
provide concrete executable correspondence, and the source mutants show that the
production checks observe each modelled property; neither is proof extraction.
The stale-index, activation, selection and qualification obligations stay with
tasks 8010–8013 and 6021. The family therefore remains PARTIAL.

**Consumers and pins.** The byte view's consumers are tasks 8011 (the
PathMap-derived catalogs), 8012 (selection) and 7931 (the durable path). The view
changes no persisted byte. The runner's `--ordered-receive-view` flag pins that
with `serializer_byte_goldens` (5 tests), `serializer_par_byte_goldens` (7),
`event_hash_bytes_differential` (11), `event_hash_encoder_differential` (5),
`catalog_hash_preimages_are_exact_raw_identity_bytes` (1) and `rho_path_spec`
(24).

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
