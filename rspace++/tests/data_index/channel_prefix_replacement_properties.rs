use std::collections::BTreeSet;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use proptest::collection::vec;
use proptest::prelude::*;
use serde::Serialize;

use super::{RSpaceDataIndex, RSpaceDataStore};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::Datum;
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::trace::event::Produce;

const FORMAL_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../formal/rocq/stack_safe_pda/theories/ChannelPrefixReplacement.v"
));
const MUTANT_SOURCE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../formal/rocq/stack_safe_pda/theories/ChannelPrefixReplacementMutants.v"
));
const THIS_SOURCE: &str = include_str!("channel_prefix_replacement_properties.rs");
const DATA_INDEX_SOURCE: &str = include_str!("../../src/rspace/data_index.rs");
const BAG_SOURCE: &str = include_str!("../../src/rspace/bag.rs");
const BAG_SPEC_SOURCE: &str = include_str!("../bag_spec.rs");
const DATA_INDEX_SPEC_SOURCE: &str = include_str!("../data_index_spec.rs");
const GUARDED_SOURCE: &str = include_str!("../../src/rspace/guarded_pathmap.rs");

#[derive(Clone, Copy)]
struct InvariantBinding {
    id: &'static str,
    constructor: &'static str,
    theorem: &'static str,
    property: &'static str,
}

const INVARIANT_BINDINGS: &[InvariantBinding] = &[
    InvariantBinding {
        id: "CR-01",
        constructor: "StrongFailureAtomicityInvariant",
        theorem: "every_preparation_failure_preserves_the_entire_public_state",
        property: "preparation_faults_are_strongly_atomic",
    },
    InvariantBinding {
        id: "CR-02",
        constructor: "ExactTargetPrefixInvariant",
        theorem: "successful_publication_exactly_replaces_target_prefix",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-03",
        constructor: "NonTargetKeyPreservationInvariant",
        theorem: "successful_publication_preserves_every_non_target_key",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-04",
        constructor: "IndependentChannelPreservationInvariant",
        theorem: "successful_publication_preserves_independent_prefix",
        property: "channel_replacement_preserves_other_channels_and_loaded_empty_authority",
    },
    InvariantBinding {
        id: "CR-05",
        constructor: "PrefixSuffixCompositionInvariant",
        theorem: "strip_prefix_composed_path",
        property: "relative_prefix_publication_matches_an_independent_bag_rebuild",
    },
    InvariantBinding {
        id: "CR-06",
        constructor: "ReusableBufferRestorationInvariant",
        theorem: "reusable_buffer_is_at_marker_after_whole_replay",
        property: "prefix_rooted_writer_cannot_escape_or_address_a_sibling_as_relative",
    },
    InvariantBinding {
        id: "CR-07",
        constructor: "LoadedSetInvariant",
        theorem: "target_is_loaded_even_when_replacement_is_empty",
        property: "data_store_distinguishes_unloaded_from_loaded_empty_channels",
    },
    InvariantBinding {
        id: "CR-08",
        constructor: "LoadedCountInvariant",
        theorem: "publication_loaded_count_matches_loaded_set",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-09",
        constructor: "EmptyLoadedChannelInvariant",
        theorem: "target_is_loaded_even_when_replacement_is_empty",
        property: "channel_replacement_preserves_other_channels_and_loaded_empty_authority",
    },
    InvariantBinding {
        id: "CR-10",
        constructor: "TargetCatalogInvalidationInvariant",
        theorem: "target_catalog_is_invalidated_exactly",
        property:
            "channel_replacement_invalidates_only_the_target_catalog_and_keeps_old_pools_valid",
    },
    InvariantBinding {
        id: "CR-11",
        constructor: "NonTargetCatalogIdentityInvariant",
        theorem: "non_target_catalog_identity_is_preserved",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-12",
        constructor: "ExternalCatalogSnapshotInvariant",
        theorem: "externally_held_catalog_snapshots_remain_valid_immutable_values",
        property:
            "channel_replacement_invalidates_only_the_target_catalog_and_keeps_old_pools_valid",
    },
    InvariantBinding {
        id: "CR-13",
        constructor: "DistinctCountFormulaInvariant",
        theorem: "publication_preserves_distinct_count_formula",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-14",
        constructor: "CardinalityFormulaInvariant",
        theorem: "publication_preserves_cardinality_formula",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-15",
        constructor: "MultiplicityPreservationInvariant",
        theorem: "published_target_multiplicity_is_exact",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-16",
        constructor: "LegacySuccessEquivalenceInvariant",
        theorem: "prepare_then_publish_equals_legacy_success_semantics",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-17",
        constructor: "PreparedBagWellFormedInvariant",
        theorem: "publication_preserves_prepared_input_aggregation",
        property: "multiplicity_rejects_zero_and_detects_overflow",
    },
    InvariantBinding {
        id: "CR-18",
        constructor: "CheckedU64MultiplicityInvariant",
        theorem: "publication_preserves_checked_u64_multiplicities",
        property: "multiplicity_rejects_zero_and_detects_overflow",
    },
    InvariantBinding {
        id: "CR-19",
        constructor: "ResultWellFormedInvariant",
        theorem: "publication_preserves_authoritative_well_formedness",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-20",
        constructor: "PrefixRootedCursorInvariant",
        theorem: "prefix_rooted_cursor_publication_equals_prefix_replacement",
        property: "prefix_publication_uses_one_restricted_cursor_without_projection",
    },
    InvariantBinding {
        id: "CR-21",
        constructor: "RelativeCursorNoEscapeInvariant",
        theorem: "prefix_rooted_cursor_cannot_change_non_target_lookup",
        property: "prefix_rooted_writer_cannot_escape_or_address_a_sibling_as_relative",
    },
    InvariantBinding {
        id: "CR-22",
        constructor: "SingleCursorReplayInvariant",
        theorem: "prefix_rooted_cursor_replay_is_exact_suffix_lifting",
        property: "prefix_publication_uses_one_restricted_cursor_without_projection",
    },
    InvariantBinding {
        id: "CR-23",
        constructor: "LoadedCapacityInvariant",
        theorem: "publication_respects_preflighted_loaded_capacity",
        property: "loaded_count_overflow_preflight_precedes_iterator_consumption",
    },
    InvariantBinding {
        id: "CR-24",
        constructor: "LoadedSetUniquenessInvariant",
        theorem: "publication_preserves_loaded_set_uniqueness",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-25",
        constructor: "PreparationFailurePrecedenceInvariant",
        theorem: "every_preparation_failure_has_precedence_over_publication",
        property: "preparation_faults_are_strongly_atomic",
    },
    InvariantBinding {
        id: "CR-26",
        constructor: "CatalogInvariantPreservationInvariant",
        theorem: "publication_preserves_catalog_invariant",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
    InvariantBinding {
        id: "CR-27",
        constructor: "PreparedAggregationRefinementInvariant",
        theorem: "publication_preserves_prepared_input_aggregation",
        property: "set_channel_matches_frozen_legacy_success_oracle",
    },
];

const MUTANT_BINDINGS: &[(&str, &str)] = &[
    ("ClearBeforeEncodingMutant", "kill_clear_before_encoding_failure_atomicity"),
    ("PublishAfterEncodingFailureMutant", "kill_publication_after_encoding_failure_precedence"),
    ("WholeAuthorityClearMutant", "kill_whole_bag_clear_non_target_loss"),
    ("RetainOldTargetMutant", "kill_old_target_retention"),
    ("SkipPreparedSuffixMutant", "kill_skipped_prepared_suffix"),
    ("DuplicatePreparedSuffixMutant", "kill_duplicated_prepared_suffix"),
    ("UnframedRelativeKeyMutant", "kill_unframed_relative_key_publication"),
    ("FlattenMultiplicityMutant", "kill_multiplicity_flattening"),
    ("DuplicatePreparedKeyMutant", "kill_duplicate_prepared_key_acceptance"),
    ("U64OverflowMutant", "kill_unchecked_u64_multiplicity"),
    ("CursorTargetDriftMutant", "kill_cursor_target_root_drift"),
    ("AbsoluteCursorWriteMutant", "kill_out_of_prefix_absolute_cursor_write"),
    ("WrongBufferRestoreMutant", "kill_reusable_buffer_wrong_restore"),
    ("DoubleLoadedCountMutant", "kill_double_loaded_count_increment"),
    ("EmptyNotLoadedMutant", "kill_empty_channel_not_loaded"),
    ("CapacityPreflightOmissionMutant", "kill_missing_loaded_capacity_preflight"),
    ("StaleTargetCatalogMutant", "kill_stale_target_catalog"),
    ("ClearAllCatalogsMutant", "kill_non_target_catalog_clear"),
    ("ExternalSnapshotMutationMutant", "kill_external_catalog_snapshot_mutation"),
    ("NoncanonicalReplayOrderMutant", "kill_noncanonical_replay_order"),
];

fn datum(label: u8, persist: bool) -> Datum<String> {
    Datum {
        a: Arc::new(format!("value-{label}")),
        persist,
        source: Produce::new(
            Blake2b256Hash::new(&[label]),
            Blake2b256Hash::new(&[label, u8::from(persist)]),
            false,
        ),
    }
}

fn frozen_legacy_set_channel(
    mut store: RSpaceDataStore<String, String>,
    channel: String,
    datums: &[Datum<String>],
) -> RSpaceDataStore<String, String> {
    let marker = RhoPathV1::datum_prefix(&channel);
    let was_loaded = store.loaded_channels.contains(&marker);
    store.index.clear_channel(&channel);
    for value in datums {
        store.index.insert(&channel, value).unwrap();
    }
    if !was_loaded {
        assert!(store.loaded_channels.insert(&marker, ()).is_none());
        store.loaded_channel_count += 1;
    }
    store.candidate_catalogs.get_mut().unwrap().remove(&marker);
    store
}

proptest! {
    #[test]
    fn set_channel_matches_frozen_legacy_success_oracle(
        target_preloaded in any::<bool>(),
        old_entries in vec((0u8..8, any::<bool>()), 0..32),
        replacement_entries in vec((0u8..8, any::<bool>()), 0..32),
        sibling_entries in vec((8u8..16, any::<bool>()), 0..24),
    ) {
        let target = "shared\0target".to_owned();
        let sibling = "shared\0sibling".to_owned();
        let old = old_entries.into_iter().map(|(label, persist)| datum(label, persist)).collect::<Vec<_>>();
        let replacement = replacement_entries
            .into_iter()
            .map(|(label, persist)| datum(label, persist))
            .collect::<Vec<_>>();
        let sibling_values = sibling_entries
            .into_iter()
            .map(|(label, persist)| datum(label, persist))
            .collect::<Vec<_>>();

        let mut before = RSpaceDataStore::new();
        before.set_channel(sibling.clone(), sibling_values).unwrap();
        if target_preloaded {
            before.set_channel(target.clone(), old).unwrap();
        }
        let target_marker = RhoPathV1::datum_prefix(&target);
        let sibling_marker = RhoPathV1::datum_prefix(&sibling);
        let _ = before.channel_pool(&sibling).unwrap().unwrap();
        if target_preloaded {
            let _ = before.channel_pool(&target).unwrap().unwrap();
        }
        let sibling_catalog = Arc::clone(
            before.candidate_catalogs.read().unwrap().get(&sibling_marker).unwrap()
        );

        let mut actual = before.clone();
        let expected = frozen_legacy_set_channel(before, target.clone(), &replacement);
        actual.set_channel(target, replacement).unwrap();

        prop_assert_eq!(&actual, &expected);
        prop_assert_eq!(actual.index.as_bag(), expected.index.as_bag());
        prop_assert_eq!(actual.distinct_len(), expected.distinct_len());
        prop_assert_eq!(actual.cardinality(), expected.cardinality());
        prop_assert_eq!(actual.loaded_channel_count(), expected.loaded_channel_count());
        let catalogs = actual.candidate_catalogs.read().unwrap();
        prop_assert!(!catalogs.contains_key(&target_marker));
        prop_assert!(Arc::ptr_eq(catalogs.get(&sibling_marker).unwrap(), &sibling_catalog));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum PreparationFault {
    Structure,
    Serialization,
}

impl Serialize for PreparationFault {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where S: serde::Serializer {
        match self {
            Self::Structure => serializer.serialize_u8(1),
            Self::Serialization => Err(serde::ser::Error::custom("synthetic datum encoding fault")),
        }
    }
}

impl StableHashSerialize for PreparationFault {}

impl RhoPathEncode for PreparationFault {
    fn rho_path_structure(&self) -> Vec<u8> {
        match self {
            Self::Structure => panic!("synthetic structure encoding fault"),
            Self::Serialization => b"valid-structure".to_vec(),
        }
    }
}

fn fault_datum(fault: PreparationFault) -> Datum<PreparationFault> {
    Datum {
        a: Arc::new(fault),
        persist: false,
        source: Produce::new(
            Blake2b256Hash::new(b"fault"),
            Blake2b256Hash::new(b"fault-sequence"),
            false,
        ),
    }
}

#[test]
fn preparation_faults_are_strongly_atomic() {
    for fault in [PreparationFault::Structure, PreparationFault::Serialization] {
        let mut store = RSpaceDataStore::new();
        let before = store.clone();
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _ = store.set_channel("fault-target".to_owned(), [fault_datum(fault)]);
        }));
        assert!(outcome.is_err());
        assert_eq!(store, before);
        assert_eq!(store.index.as_bag(), before.index.as_bag());
    }
}

#[test]
fn catalog_lock_poison_precedes_authoritative_publication() {
    let target = "target".to_owned();
    let sibling = "sibling".to_owned();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(target.clone(), [datum(1, false)])
        .unwrap();
    store
        .set_channel(sibling.clone(), [datum(2, true)])
        .unwrap();
    let _ = store.channel_pool(&target).unwrap().unwrap();
    let _ = store.channel_pool(&sibling).unwrap().unwrap();
    let target_marker = RhoPathV1::datum_prefix(&target);
    let sibling_marker = RhoPathV1::datum_prefix(&sibling);
    let before_index = store.index.clone();
    let before_loaded = store.loaded_channels.clone();
    let before_count = store.loaded_channel_count;
    let before_target_catalog = Arc::clone(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&target_marker)
            .unwrap(),
    );
    let before_sibling_catalog = Arc::clone(
        store
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&sibling_marker)
            .unwrap(),
    );

    let poisoned = catch_unwind(AssertUnwindSafe(|| {
        let _guard = store.candidate_catalogs.write().unwrap();
        panic!("synthetic catalog lock poison");
    }));
    assert!(poisoned.is_err());
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.set_channel(target.clone(), [datum(3, false)]);
    }));
    assert!(outcome.is_err());
    store.candidate_catalogs.clear_poison();

    assert_eq!(store.index, before_index);
    assert_eq!(store.loaded_channels.iter().count(), before_loaded.iter().count());
    assert_eq!(store.loaded_channel_count, before_count);
    let catalogs = store.candidate_catalogs.read().unwrap();
    assert!(Arc::ptr_eq(catalogs.get(&target_marker).unwrap(), &before_target_catalog));
    assert!(Arc::ptr_eq(catalogs.get(&sibling_marker).unwrap(), &before_sibling_catalog));
}

#[test]
fn loaded_count_overflow_preflight_precedes_iterator_consumption() {
    let mut store = RSpaceDataStore::<String, String>::new();
    store.loaded_channel_count = usize::MAX;
    let before_index = store.index.clone();
    let consumed = std::cell::Cell::new(false);
    let datums = std::iter::from_fn(|| {
        consumed.set(true);
        Some(datum(1, false))
    });

    let outcome = catch_unwind(AssertUnwindSafe(|| {
        let _ = store.set_channel("new-target".to_owned(), datums);
    }));
    assert!(outcome.is_err());
    assert!(!consumed.get());
    assert_eq!(store.index, before_index);
    assert_eq!(store.loaded_channel_count, usize::MAX);
    assert_eq!(store.loaded_channels.iter().count(), 0);
}

#[test]
fn prefix_publication_uses_one_restricted_cursor_without_projection() {
    let start = BAG_SOURCE
        .find("    pub(crate) fn replace_prefix_with_relative(")
        .expect("prefix replacement source boundary");
    let tail = &BAG_SOURCE[start..];
    let end = tail
        .find("\n    pub fn set_count(")
        .expect("next Bag method boundary");
    let body = &tail[..end];
    assert_eq!(body.matches("with_write_cursor_at").count(), 1);
    for forbidden in ["Vec<", "entries_with_prefix", "PathMap::new", "self.clone()", "make_map"] {
        assert!(!body.contains(forbidden), "forbidden prefix-publication capability: {forbidden}");
    }
}

fn constructors_between(source: &str, start: &str, end: &str) -> BTreeSet<String> {
    let body = source
        .split_once(start)
        .expect("formal catalog start")
        .1
        .split_once(end)
        .expect("formal catalog end")
        .0;
    body.lines()
        .filter_map(|line| line.trim().strip_prefix('|'))
        .map(|line| line.trim().trim_end_matches('.').to_owned())
        .collect()
}

#[test]
fn formal_invariant_and_mutant_bindings_are_unique_exhaustive_and_live() {
    let executable_sources = [
        THIS_SOURCE,
        DATA_INDEX_SOURCE,
        BAG_SOURCE,
        BAG_SPEC_SOURCE,
        DATA_INDEX_SPEC_SOURCE,
        GUARDED_SOURCE,
    ]
    .concat();
    let ids = INVARIANT_BINDINGS
        .iter()
        .map(|binding| binding.id)
        .collect::<BTreeSet<_>>();
    let constructors = INVARIANT_BINDINGS
        .iter()
        .map(|binding| binding.constructor)
        .collect::<BTreeSet<_>>();
    assert_eq!(INVARIANT_BINDINGS.len(), 27);
    assert_eq!(ids.len(), INVARIANT_BINDINGS.len(), "duplicate CR property ID");
    assert_eq!(constructors.len(), INVARIANT_BINDINGS.len(), "duplicate formal invariant binding");
    assert_eq!(
        constructors,
        constructors_between(
            FORMAL_SOURCE,
            "Inductive channel_replacement_invariant_id : Type :=",
            "Definition channel_replacement_invariant_catalog"
        )
        .iter()
        .map(String::as_str)
        .collect(),
        "the executable registry must bind every formal invariant exactly once"
    );
    for binding in INVARIANT_BINDINGS {
        assert!(
            FORMAL_SOURCE.contains(binding.theorem),
            "missing formal theorem: {}",
            binding.theorem
        );
        assert!(
            executable_sources.contains(binding.property),
            "missing executable property family: {}",
            binding.property
        );
    }

    let mutant_ids = MUTANT_BINDINGS
        .iter()
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    assert_eq!(MUTANT_BINDINGS.len(), 20);
    assert_eq!(mutant_ids.len(), MUTANT_BINDINGS.len(), "duplicate causal mutant binding");
    assert_eq!(
        mutant_ids,
        constructors_between(
            MUTANT_SOURCE,
            "Inductive channel_replacement_mutant_id : Type :=",
            "Definition channel_replacement_mutant_catalog"
        )
        .iter()
        .map(String::as_str)
        .collect(),
        "every causal mutant must remain registered exactly once"
    );
    for (_, theorem) in MUTANT_BINDINGS {
        assert!(MUTANT_SOURCE.contains(theorem), "missing mutant witness: {theorem}");
    }
}

#[test]
fn relative_datum_paths_recompose_to_the_public_absolute_key() {
    for label in 0..32 {
        let channel = format!("channel\0{}", label % 5);
        let value = datum(label, label % 2 == 0);
        let mut recomposed = RhoPathV1::datum_prefix(&channel);
        recomposed.extend_from_slice(&RSpaceDataIndex::relative_datum_path(&value));
        assert_eq!(recomposed, RSpaceDataIndex::datum_path(&channel, &value));
    }
}
