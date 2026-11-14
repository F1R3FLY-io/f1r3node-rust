# RSpace channel-prefix replacement invariant catalog — 2026-08-31

## Status and scope

This living engineering report records the channel-replacement proof and measurements that
preceded selection of the two-level RSpace authority. Its semantic invariants remain active
regression obligations for `RSpaceDataStore::set_channel`, but its former physical mechanism — a
prefix-rooted replacement inside one global `PathMap<Multiplicity>` — was superseded on
2026-09-02 by an exact outer `PathMap<ChannelSlot>` whose real endpoints own independent
channel-relative `Bag` values. The selected implementation is traced by the two-level authority
formal family and its current invariant catalog; the measurements below remain historical evidence
for why whole-store cloning was rejected. Here:

- **Bag** means the prefix-compressed multiset wrapper whose endpoint values are checked
  `Multiplicity(u64)` values;
- **marker** means the canonical `RhoPathV1` byte key identifying one exact outer channel endpoint;
- **relative suffix** means the canonical datum path after that marker;
- **candidate catalog** means a derived immutable `Arc`-owned ordering/decoding index for one
  channel; and
- **publication** means replacement of one exact outer endpoint after its private relative Bag and
  aggregate metadata have been fully preflighted.

The required refinement order was followed:

1. model relative preparation and prefix-local publication;
2. prove the successful and failing transition contracts;
3. bind causal mutants to their violated obligations;
4. extract every formal invariant into the executable registry;
5. implement the prefix-confined transaction; and
6. run the bound properties, semantic differentials, stack gate, and causal profiles.

The proof uses finite lists as mathematical finite-support witnesses. It does not authorize a
runtime list, `Vec<Par>`, flattened entry projection, or element sort. Production retains native
PathMap prefix compression, CoW roots, and zipper mutation.

## Transaction refinement

The algorithm below is the dated single-trie implementation proved and measured by this packet.
It is retained as a semantic reference, not as a description of current production topology. The
current two-level implementation prepares a private relative Bag and replaces exactly one outer
endpoint; multi-channel deltas prepare every touched Bag and preflight the final aggregate metadata
before writing the touched outer endpoints in place; `Arc::make_mut` copies the outer header once,
and only when another version shares it (2026-11-14,
[stack-safety report §5.60.8](../stack-safety/stack-safety-report-2026-07-29.md#5608-in-place-outer-delta-batches)).

```text
⟨Prepare one channel-relative replacement⟩ ≡
    marker       ← canonical channel datum prefix
    was_loaded   ← marker membership in loaded-channel trie
    next_count   ← checked successor only when marker is new
    prepared     ← fold every datum into a private relative PathMap Bag
    catalogs     ← acquire mutable catalog authority

⟨Publish the already-prepared replacement⟩ ≡
    cursor       ← one guarded write zipper rooted exactly at marker
    if was_loaded then drain only cursor's target subtrie
    replay each borrowed canonical relative endpoint through cursor
    update distinct and exact cardinality once
    if marker is new then publish marker and next_count once
    remove exactly catalogs[marker]
```

The guarded cursor accepts only relative navigation. It exposes no raw map, absolute sibling
address, detached subtree, graft operation, complete-root replacement, or retained zipper clone.

## Formal packet

| Artifact | Role | SHA-256 |
|---|---|---|
| `formal/rocq/stack_safe_pda/theories/ChannelPrefixReplacement.v` | 27-invariant transition/refinement proof | `95fd8b17161e1fb6de6c8be213824864e385922b57f622d971f540fb9b36c240` |
| `formal/rocq/stack_safe_pda/theories/ChannelPrefixReplacementMutants.v` | 20 bound causal negative controls | `5a1a17c04b242b7ba3a3c5554f8af5ea2b0f45f2c6ba2f64d59a5f6e200d8e7b` |
| `formal/rocq/stack_safe_pda/_CoqProject` | checked proof manifest | `043dfb244072077c31a0ab51fd36f73e3df7508d85a39260ff1b3fdafca6d03a` |

Main and mutant `coqc` runs exited zero under `MemoryMax=768M`, `MemorySwapMax=0`, and
`CPUQuota=150%`; their systemd invocation identifiers were
`893b50869c3c40708ee7c22eca4a67ca` and `34ca5d1423a747de8dc51aeb876d4193`.
Independent `coqchk -silent` exited zero under the same envelope with invocation
`5f31be29031e4fb2ac5211c73c05f754`. Every printed capstone is closed under the global context;
neither proof source contains an axiom, parameter, admission, or aborted proof.

The primary theorem is `prepare_then_publish_channel_replacement_capstone`. It composes exact target
replacement, non-target preservation, loaded/catalog invariants, multiplicity bounds, metric
formulas, one prefix-rooted cursor, and legacy-success equivalence.

## Exhaustive invariant-to-property binding

The executable `INVARIANT_BINDINGS` registry contains exactly 27 unique IDs and exactly the 27
constructors extracted from Rocq's `channel_replacement_invariant_catalog`. Its live test rejects a
missing or duplicate formal constructor, unresolved theorem symbol, or missing executable property
name.

| ID | Formal invariant | Principal theorem | Executable property family | Required observation |
|---|---|---|---|---|
| CR-01 | `StrongFailureAtomicityInvariant` | `every_preparation_failure_preserves_the_entire_public_state` | `preparation_faults_are_strongly_atomic` | structure and datum-serialization faults publish no state |
| CR-02 | `ExactTargetPrefixInvariant` | `successful_publication_exactly_replaces_target_prefix` | `set_channel_matches_independent_channel_bag_reference` | target prefix equals only the aggregated replacement |
| CR-03 | `NonTargetKeyPreservationInvariant` | `successful_publication_preserves_every_non_target_key` | independent channel-Bag reference | every non-target endpoint and multiplicity remains exact |
| CR-04 | `IndependentChannelPreservationInvariant` | `successful_publication_preserves_independent_prefix` | `channel_replacement_preserves_other_channels_and_loaded_empty_authority` | adjacent framed channels cannot overlap or disappear |
| CR-05 | `PrefixSuffixCompositionInvariant` | `strip_prefix_composed_path` | `relative_prefix_publication_matches_an_independent_bag_rebuild` | absolute key is exactly marker followed by relative suffix |
| CR-06 | `ReusableBufferRestorationInvariant` | `reusable_buffer_is_at_marker_after_whole_replay` | `prefix_rooted_writer_cannot_escape_or_address_a_sibling_as_relative` | PathMap's rooted cursor restores relative addressing at its immutable marker |
| CR-07 | `LoadedSetInvariant` | `target_is_loaded_even_when_replacement_is_empty` | `data_store_distinguishes_unloaded_from_loaded_empty_channels` | an empty replacement still publishes loaded authority |
| CR-08 | `LoadedCountInvariant` | `publication_loaded_count_matches_loaded_set` | independent channel-Bag reference | count equals the exact loaded-marker population |
| CR-09 | `EmptyLoadedChannelInvariant` | `target_is_loaded_even_when_replacement_is_empty` | loaded-empty replacement property | loaded-empty remains distinguishable from unloaded |
| CR-10 | `TargetCatalogInvalidationInvariant` | `target_catalog_is_invalidated_exactly` | `channel_replacement_invalidates_only_the_target_catalog_and_keeps_old_pools_valid` | target derived catalog is absent after publication |
| CR-11 | `NonTargetCatalogIdentityInvariant` | `non_target_catalog_identity_is_preserved` | independent channel-Bag reference | sibling catalog retains exact `Arc` identity |
| CR-12 | `ExternalCatalogSnapshotInvariant` | `externally_held_catalog_snapshots_remain_valid_immutable_values` | old-pool validity property | an already lent catalog remains immutable and usable |
| CR-13 | `DistinctCountFormulaInvariant` | `publication_preserves_distinct_count_formula` | independent channel-Bag reference | old target distinct count is subtracted and prepared distinct count added once |
| CR-14 | `CardinalityFormulaInvariant` | `publication_preserves_cardinality_formula` | independent channel-Bag reference | exact `u128` cardinality follows the replacement equation |
| CR-15 | `MultiplicityPreservationInvariant` | `published_target_multiplicity_is_exact` | independent channel-Bag reference | duplicate identities remain one endpoint with exact multiplicity |
| CR-16 | `LegacySuccessEquivalenceInvariant` | `prepare_then_publish_equals_legacy_success_semantics` | independent channel-Bag reference | complete successful result equals the independent channel-relative Bag semantics |
| CR-17 | `PreparedBagWellFormedInvariant` | `publication_preserves_prepared_input_aggregation` | `multiplicity_rejects_zero_and_detects_overflow` | prepared keys are unique and multiplicities positive |
| CR-18 | `CheckedU64MultiplicityInvariant` | `publication_preserves_checked_u64_multiplicities` | multiplicity boundary property | zero and endpoint overflow are rejected atomically |
| CR-19 | `ResultWellFormedInvariant` | `publication_preserves_authoritative_well_formedness` | independent channel-Bag reference | resulting Bag agrees with independently built valid state |
| CR-20 | `PrefixRootedCursorInvariant` | `prefix_rooted_cursor_publication_equals_prefix_replacement` | `prefix_publication_uses_one_restricted_cursor_without_projection` | production uses exactly one restricted prefix writer |
| CR-21 | `RelativeCursorNoEscapeInvariant` | `prefix_rooted_cursor_cannot_change_non_target_lookup` | prefix-rooted sibling-escape property | no relative path can address a sibling |
| CR-22 | `SingleCursorReplayInvariant` | `prefix_rooted_cursor_replay_is_exact_suffix_lifting` | single-cursor source-capability property | no `Vec`, entry projection, raw PathMap, clone, or `make_map` appears in publication |
| CR-23 | `LoadedCapacityInvariant` | `publication_respects_preflighted_loaded_capacity` | `loaded_count_overflow_preflight_precedes_iterator_consumption` | checked count failure occurs before iterator consumption or mutation |
| CR-24 | `LoadedSetUniquenessInvariant` | `publication_preserves_loaded_set_uniqueness` | independent channel-Bag reference | replacing an already loaded channel never duplicates or increments its marker |
| CR-25 | `PreparationFailurePrecedenceInvariant` | `every_preparation_failure_has_precedence_over_publication` | `preparation_faults_are_strongly_atomic`; `catalog_lock_poison_precedes_authoritative_publication` | encoder, iterator, count, and catalog-authority faults precede publication |
| CR-26 | `CatalogInvariantPreservationInvariant` | `publication_preserves_catalog_invariant` | independent channel-Bag reference | every retained catalog still denotes its unchanged channel state |
| CR-27 | `PreparedAggregationRefinementInvariant` | `publication_preserves_prepared_input_aggregation` | independent channel-Bag reference | repeated input identities aggregate without occurrence expansion or flattening |

The independent reference is test-only. It builds a private channel-relative Bag from semantic
inputs and publishes it through the exact outer endpoint; it does not reconstruct the retired flat
physical topology. Proptest varies whether the target was loaded, duplicate target identities,
empty replacements, persistent flags, sibling contents, and shared framed channel prefixes. Every
generated case compares the complete two-level authority, metrics, marker/count state, target
invalidation, and sibling catalog `Arc` identity.

## Causal mutant registry

Rocq's `channel_replacement_mutant_catalog` contains exactly 20 unique constructors. The executable
manifest verifies exact constructor equality and resolves every named counterexample theorem.

| Mutant family | Defect exposed |
|---|---|
| `ClearBeforeEncodingMutant` | target cleared before preparation succeeds |
| `PublishAfterEncodingFailureMutant` | partial result published after an encoder failure |
| `WholeAuthorityClearMutant` | unrelated resident state cleared |
| `RetainOldTargetMutant` | stale target endpoint retained |
| `SkipPreparedSuffixMutant` | replacement endpoint omitted |
| `DuplicatePreparedSuffixMutant` | replacement endpoint duplicated |
| `UnframedRelativeKeyMutant` | relative key published without channel marker |
| `FlattenMultiplicityMutant` | multiplicity replaced by one |
| `DuplicatePreparedKeyMutant` | prepared Bag violates unique-key representation |
| `U64OverflowMutant` | endpoint multiplicity exceeds `u64` refinement |
| `CursorTargetDriftMutant` | cursor rebases to a different authoritative root |
| `AbsoluteCursorWriteMutant` | writer escapes the target prefix |
| `WrongBufferRestoreMutant` | replay restores to suffix rather than marker |
| `DoubleLoadedCountMutant` | already-loaded replacement increments count |
| `EmptyNotLoadedMutant` | empty replacement loses loaded authority |
| `CapacityPreflightOmissionMutant` | new marker publishes past checked capacity |
| `StaleTargetCatalogMutant` | target catalog survives replacement |
| `ClearAllCatalogsMutant` | unrelated catalogs are invalidated |
| `ExternalSnapshotMutationMutant` | previously lent immutable catalog changes |
| `NoncanonicalReplayOrderMutant` | private replay departs from prepared canonical order |

`channel_replacement_mutant_catalog_is_unique_complete_and_exact` proves that all 20 entries are
present, unique, and killed by their bound counterexamples. Mutants are proof-side negative
controls; none is compiled into production.

## Executable and resource evidence

The eight focused formal-to-property tests pass under an 8 GiB resident hard cap, disabled swap, and
an eight-core quota. They include the randomized independent channel-Bag reference, two encoder fault sites,
catalog-lock poison for replacement and clear, checked-count preflight, key recomposition, cursor capability inspection, and
the exact 27/20 manifest binding. The surrounding gates pass:

- `rspace_plus_plus --lib`: 499/499, including this seven-test family and the `u64::MAX`
  prefix-metric boundary;
- `bag_spec`: 30/30;
- `data_index_spec`: 25/25;
- debug `guarded_matching_tests`: 18/18, including 20,000 binds in 94.36 s; and
- release `bag_spec` / `data_index_spec` / `guarded_matching_tests`: 30/30, 25/25, and 18/18; and
- exact release 20,000-bind gate: 7.52 s, 151,408 KiB peak RSS, zero swap.

The causal timing and profiler evidence is retained in
[`../stack-safety/measurements/rspace-channel-prefix-publication-2026-08-31.md`](../stack-safety/measurements/rspace-channel-prefix-publication-2026-08-31.md).

## Completion and maintenance contract

This checkpoint is acceptable only while all of the following remain true:

- both Rocq files compile, independent `coqchk` passes, every printed assumption set is closed, and
  the 20-mutant theorem remains exact;
- the mechanical test continues to bind exactly 27 formal invariants and 20 mutants;
- every bound executable property name resolves to a live source target;
- successful randomized results agree with the independent channel-Bag reference;
- all preparation and prepublication authority failures leave the complete state unchanged;
- publication uses exact outer endpoints and channel-relative guarded Bags without a flattened projection;
- the exact 20,000-bind and geometric scaling gates remain green;
- the separate final-root teardown task remains visible until its allocation mechanism is repaired;
  and
- the PathMap journal, stack-safety report, consensus register, and pgmcp descendants of epic 4131
  are updated whenever any premise, result, limitation, or task disposition changes.
