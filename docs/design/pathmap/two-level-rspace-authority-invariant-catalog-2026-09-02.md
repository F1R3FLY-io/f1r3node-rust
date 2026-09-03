# Two-level RSpace authority invariant catalog

Date: 2026-09-02

Status: in-memory authority partially verified; direct RBA1 persistence remains an implementation gap

Scope: the selected two-level RSpace representation, its operations and lifecycle, authoritative matching, and direct RBA1 checkpoint refinement

## Purpose

This living engineering report is the non-duplicative traceability authority between the committed Rocq models and executable Rust properties for the two-level RSpace design. The outer `PathMap` stores one exact endpoint per loaded channel. Each real endpoint owns an independently guarded, channel-relative `Bag`, represented by `PathMap<Multiplicity>`. A loaded channel with an empty Bag remains observably different from an unloaded channel.

The formal catalogs contain **171 distinct obligations**. They are normalized below into ten executable property families. Each formal invariant has exactly one primary executable owner even when several formal modules derive related consequences. This prevents duplicated green tests from obscuring an untested seam.

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

Open implementation and executable obligation: bind the actual hot-store accepted-match path to a generated property with an independent shallow recursive oracle. Assert selected identity, multiplicity-aware delta, untouched authority/catalog snapshots, loaded-empty outcome, strict PDA progress, and exactly one or zero publication. Reuse TLRS-PROP-05's exact-live-selection positive and exhausted-selection causal controls; do not reintroduce whole-catalog generation equality.

### TLRS-PROP-07 — Copy-on-write and guarded lifecycle

Status: **PARTIAL**

Formal owner set: all 40 GuardedOwnership invariants, including outer and inner clone/drop isolation, registered fresh anchors, structural preflight/disjointness, exact real-slot handoff, barrier separation, two-phase teardown, canonical-empty fast drop, generated PDA progress, constant control stack, zipper identity, lease/publication rules, and fail-stop atomicity.

Production surface: `GuardedPathMap<ChannelSlot, ChannelSlotDisposer, ChannelSlotBarrier>`, independently guarded inner Bags, copy-on-write clones, and generated iterative teardown.

Current executable evidence: sibling-anchor preservation, candidate authority sharing, incremental catalog anchor retention, clone splitting, cloned-store isolation, issued-pool isolation, and deep outer/inner clone-and-drop on a small stack.

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

## Current production gaps

The committed formal family is ahead of `RSpaceDataIndex` in four material respects:

1. no production RBA1 codec, streaming writer/reader, compatibility adapter, or recovery/readmission path exists;
2. no single Rust transaction API currently exposes the complete matcher-to-authoritative-commit composition proved in Rocq;
3. exactly-one publication is optional instrumentation rather than a continuously asserted production invariant;
4. most exact-once disposer, barrier, abort, and fail-stop ownership obligations lack executable event-ledger controls.

The private exact `u128` aggregate is exercised from the crate-private child property module so the saturated `u64` API cannot conceal the boundary case.

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
