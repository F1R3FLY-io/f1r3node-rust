use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use proptest::collection::vec;
use proptest::prelude::*;
use serde::Serialize;

use super::{DataIndexError, RSpaceDataIndex, RSpaceDataStore};
use crate::rspace::bag::{Bag, BagDelta, Multiplicity};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::Datum;
use crate::rspace::rho_path::{RhoPathEncode, RhoPathV1};
use crate::rspace::trace::event::Produce;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ChannelInvariant {
    StrongFailureAtomicity,
    ExactTargetPrefix,
    NonTargetKeyPreservation,
    IndependentChannelPreservation,
    PrefixSuffixComposition,
    ReusableBufferRestoration,
    LoadedSet,
    LoadedCount,
    EmptyLoadedChannel,
    TargetCatalogInvalidation,
    NonTargetCatalogIdentity,
    ExternalCatalogSnapshot,
    DistinctCountFormula,
    CardinalityFormula,
    MultiplicityPreservation,
    ReferenceSuccessEquivalence,
    PreparedBagWellFormed,
    CheckedU64Multiplicity,
    ResultWellFormed,
    PrefixRootedCursor,
    RelativeCursorNoEscape,
    SingleCursorReplay,
    LoadedCapacity,
    LoadedSetUniqueness,
    PreparationFailurePrecedence,
    CatalogInvariantPreservation,
    PreparedAggregationRefinement,
}

#[derive(Clone, Copy, Debug)]
struct InvariantBinding {
    id: &'static str,
    invariant: ChannelInvariant,
}

const INVARIANT_BINDINGS: &[InvariantBinding] = &[
    InvariantBinding {
        id: "CR-01",
        invariant: ChannelInvariant::StrongFailureAtomicity,
    },
    InvariantBinding {
        id: "CR-02",
        invariant: ChannelInvariant::ExactTargetPrefix,
    },
    InvariantBinding {
        id: "CR-03",
        invariant: ChannelInvariant::NonTargetKeyPreservation,
    },
    InvariantBinding {
        id: "CR-04",
        invariant: ChannelInvariant::IndependentChannelPreservation,
    },
    InvariantBinding {
        id: "CR-05",
        invariant: ChannelInvariant::PrefixSuffixComposition,
    },
    InvariantBinding {
        id: "CR-06",
        invariant: ChannelInvariant::ReusableBufferRestoration,
    },
    InvariantBinding {
        id: "CR-07",
        invariant: ChannelInvariant::LoadedSet,
    },
    InvariantBinding {
        id: "CR-08",
        invariant: ChannelInvariant::LoadedCount,
    },
    InvariantBinding {
        id: "CR-09",
        invariant: ChannelInvariant::EmptyLoadedChannel,
    },
    InvariantBinding {
        id: "CR-10",
        invariant: ChannelInvariant::TargetCatalogInvalidation,
    },
    InvariantBinding {
        id: "CR-11",
        invariant: ChannelInvariant::NonTargetCatalogIdentity,
    },
    InvariantBinding {
        id: "CR-12",
        invariant: ChannelInvariant::ExternalCatalogSnapshot,
    },
    InvariantBinding {
        id: "CR-13",
        invariant: ChannelInvariant::DistinctCountFormula,
    },
    InvariantBinding {
        id: "CR-14",
        invariant: ChannelInvariant::CardinalityFormula,
    },
    InvariantBinding {
        id: "CR-15",
        invariant: ChannelInvariant::MultiplicityPreservation,
    },
    InvariantBinding {
        id: "CR-16",
        invariant: ChannelInvariant::ReferenceSuccessEquivalence,
    },
    InvariantBinding {
        id: "CR-17",
        invariant: ChannelInvariant::PreparedBagWellFormed,
    },
    InvariantBinding {
        id: "CR-18",
        invariant: ChannelInvariant::CheckedU64Multiplicity,
    },
    InvariantBinding {
        id: "CR-19",
        invariant: ChannelInvariant::ResultWellFormed,
    },
    InvariantBinding {
        id: "CR-20",
        invariant: ChannelInvariant::PrefixRootedCursor,
    },
    InvariantBinding {
        id: "CR-21",
        invariant: ChannelInvariant::RelativeCursorNoEscape,
    },
    InvariantBinding {
        id: "CR-22",
        invariant: ChannelInvariant::SingleCursorReplay,
    },
    InvariantBinding {
        id: "CR-23",
        invariant: ChannelInvariant::LoadedCapacity,
    },
    InvariantBinding {
        id: "CR-24",
        invariant: ChannelInvariant::LoadedSetUniqueness,
    },
    InvariantBinding {
        id: "CR-25",
        invariant: ChannelInvariant::PreparationFailurePrecedence,
    },
    InvariantBinding {
        id: "CR-26",
        invariant: ChannelInvariant::CatalogInvariantPreservation,
    },
    InvariantBinding {
        id: "CR-27",
        invariant: ChannelInvariant::PreparedAggregationRefinement,
    },
];

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ChannelMutant {
    ClearBeforeEncoding,
    PublishAfterEncodingFailure,
    WholeAuthorityClear,
    RetainOldTarget,
    SkipPreparedSuffix,
    DuplicatePreparedSuffix,
    UnframedRelativeKey,
    FlattenMultiplicity,
    DuplicatePreparedKey,
    U64Overflow,
    CursorTargetDrift,
    AbsoluteCursorWrite,
    WrongBufferRestore,
    DoubleLoadedCount,
    EmptyNotLoaded,
    CapacityPreflightOmission,
    StaleTargetCatalog,
    ClearAllCatalogs,
    ExternalSnapshotMutation,
    NoncanonicalReplayOrder,
}

const MUTANT_CATALOG: &[ChannelMutant] = &[
    ChannelMutant::ClearBeforeEncoding,
    ChannelMutant::PublishAfterEncodingFailure,
    ChannelMutant::WholeAuthorityClear,
    ChannelMutant::RetainOldTarget,
    ChannelMutant::SkipPreparedSuffix,
    ChannelMutant::DuplicatePreparedSuffix,
    ChannelMutant::UnframedRelativeKey,
    ChannelMutant::FlattenMultiplicity,
    ChannelMutant::DuplicatePreparedKey,
    ChannelMutant::U64Overflow,
    ChannelMutant::CursorTargetDrift,
    ChannelMutant::AbsoluteCursorWrite,
    ChannelMutant::WrongBufferRestore,
    ChannelMutant::DoubleLoadedCount,
    ChannelMutant::EmptyNotLoaded,
    ChannelMutant::CapacityPreflightOmission,
    ChannelMutant::StaleTargetCatalog,
    ChannelMutant::ClearAllCatalogs,
    ChannelMutant::ExternalSnapshotMutation,
    ChannelMutant::NoncanonicalReplayOrder,
];

#[derive(Clone, Debug, Eq, PartialEq)]
struct ModelState {
    authority: BTreeMap<Vec<u8>, u64>,
    loaded: BTreeSet<Vec<u8>>,
    loaded_count: usize,
    catalogs: BTreeMap<Vec<u8>, u64>,
    external_snapshot: u64,
    replay_trace: Vec<Vec<u8>>,
}

fn compose_path(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    prefix.iter().chain(suffix).copied().collect()
}

fn model_fixture() -> (ModelState, Vec<u8>, Vec<u8>, Vec<(Vec<u8>, u64)>) {
    let target = vec![1, 2];
    let sibling = vec![9];
    let state = ModelState {
        authority: BTreeMap::from([(vec![1, 2, 3], 2), (vec![9, 4], 7)]),
        loaded: BTreeSet::from([target.clone(), sibling.clone()]),
        loaded_count: 2,
        catalogs: BTreeMap::from([(target.clone(), 21), (sibling.clone(), 34)]),
        external_snapshot: 41,
        replay_trace: Vec::new(),
    };
    let prepared = vec![(vec![5], 11), (vec![6], 13)];
    (state, target, sibling, prepared)
}

fn model_publish(mut state: ModelState, target: &[u8], prepared: &[(Vec<u8>, u64)]) -> ModelState {
    state.authority.retain(|path, _| !path.starts_with(target));
    state.replay_trace.clear();
    for (suffix, multiplicity) in prepared {
        let path = compose_path(target, suffix);
        state.authority.insert(path.clone(), *multiplicity);
        state.replay_trace.push(path);
    }
    state.loaded.insert(target.to_vec());
    state.loaded_count = state.loaded.len();
    state.catalogs.remove(target);
    state
}

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

fn reference_set_channel(
    mut store: RSpaceDataStore<String, String>,
    channel: String,
    datums: &[Datum<String>],
) -> RSpaceDataStore<String, String> {
    let marker = RhoPathV1::datum_prefix(&channel);
    let mut prepared = Bag::new();
    for value in datums {
        prepared
            .insert(&RSpaceDataIndex::relative_datum_path(value))
            .unwrap();
    }
    store
        .index
        .replace_channel_at_marker(&marker, prepared)
        .unwrap();
    store.candidate_catalogs.get_mut().unwrap().remove(&marker);
    store
}

fn physical_channel_count(index: &RSpaceDataIndex) -> usize {
    let mut count = 0;
    index
        .try_visit_channels(|_, _| {
            count += 1;
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    count
}

fn assert_store_coherent(
    invariant: ChannelInvariant,
    phase: &str,
    store: &RSpaceDataStore<String, String>,
) {
    let mut physical_distinct = 0usize;
    let mut physical_cardinality = 0u128;
    store
        .index
        .try_visit_channels(|_, bag| {
            assert_eq!(
                bag.iter().count(),
                bag.distinct_len(),
                "{invariant:?}: inner trie and distinct metadata diverged {phase}",
            );
            physical_distinct += bag.distinct_len();
            physical_cardinality += bag.exact_cardinality();
            Ok::<(), std::convert::Infallible>(())
        })
        .unwrap();
    assert_eq!(physical_channel_count(&store.index), store.loaded_channel_count());
    assert_eq!(physical_distinct, store.distinct_len());
    assert_eq!(physical_cardinality, store.index.exact_cardinality);
}

proptest! {
    #[test]
    fn set_channel_matches_independent_channel_bag_reference(
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
        let expected = reference_set_channel(before, target.clone(), &replacement);
        actual.set_channel(target, replacement).unwrap();

        prop_assert_eq!(&actual, &expected);
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
    assert_channel_failure_child("structure", "synthetic structure encoding fault");
    assert_channel_failure_child("serialization", "synthetic datum encoding fault");
}

#[test]
fn catalog_lock_poison_precedes_authoritative_publication() {
    assert_channel_failure_child("poison-publication", "candidate catalog write lock");
}

#[test]
fn catalog_lock_poison_precedes_clear_publication() {
    assert_channel_failure_child("poison-clear", "candidate catalog write lock");
}

#[test]
fn catalog_lock_poison_precedes_delta_publication() {
    assert_channel_failure_child("poison-delta", "candidate catalog write lock");
}

fn assert_channel_failure_child(case: &str, expected_diagnostic: &str) {
    let scratch = shared::rust::test_scratch::acquire("channel-replacement-failure-");
    let marker = scratch.path().join("preserved");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "rspace::data_index::channel_prefix_replacement_properties::channel_failure_child",
            "--nocapture",
        ])
        .env("CHANNEL_REPLACEMENT_FAILURE_CASE", case)
        .env("CHANNEL_REPLACEMENT_FAILURE_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run the channel replacement failure child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(failure.kind, shared::rust::test_process::ProbeFailureKind::Panicked);
            assert!(failure.stderr.contains(expected_diagnostic), "{}", failure.stderr);
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("the channel replacement failure child did not fail: {case}")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), case.as_bytes());
}

#[test]
#[ignore = "isolated channel replacement failure child"]
fn channel_failure_child() {
    let Some(case) = std::env::var_os("CHANNEL_REPLACEMENT_FAILURE_CASE") else {
        return;
    };
    let marker = std::env::var_os("CHANNEL_REPLACEMENT_FAILURE_MARKER")
        .expect("channel replacement failure marker");
    let case = case
        .to_str()
        .expect("channel replacement failure case is UTF-8");
    match case {
        "structure" => run_preparation_failure_child(PreparationFault::Structure, case, marker),
        "serialization" => {
            run_preparation_failure_child(PreparationFault::Serialization, case, marker)
        }
        "poison-publication" => run_poison_publication_child(case, marker),
        "poison-clear" => run_poison_clear_child(case, marker),
        "poison-delta" => run_poison_delta_child(case, marker),
        _ => panic!("unknown channel replacement failure case: {case}"),
    }
}

fn run_preparation_failure_child(fault: PreparationFault, case: &str, marker: std::ffi::OsString) {
    struct VerifyOnDrop {
        store: RSpaceDataStore<String, PreparationFault>,
        before: RSpaceDataStore<String, PreparationFault>,
        case: String,
        marker: std::path::PathBuf,
    }
    impl Drop for VerifyOnDrop {
        fn drop(&mut self) {
            assert_eq!(self.store, self.before);
            assert_eq!(self.store.index, self.before.index);
            std::fs::write(&self.marker, self.case.as_bytes()).unwrap();
        }
    }
    let store = RSpaceDataStore::new();
    let mut verify = VerifyOnDrop {
        before: store.clone(),
        store,
        case: case.to_owned(),
        marker: marker.into(),
    };
    let _ = verify
        .store
        .set_channel("fault-target".to_owned(), [fault_datum(fault)]);
}

struct VerifyPoisonedStoreOnDrop {
    store: RSpaceDataStore<String, String>,
    before: RSpaceDataStore<String, String>,
    channels: Vec<String>,
    case: String,
    marker: std::path::PathBuf,
}

impl Drop for VerifyPoisonedStoreOnDrop {
    fn drop(&mut self) {
        assert!(self.store.candidate_catalogs.is_poisoned());
        self.store.candidate_catalogs.clear_poison();
        assert_eq!(self.store, self.before);
        assert_eq!(self.store.index, self.before.index);
        assert_eq!(self.store.loaded_channel_count(), self.before.loaded_channel_count());
        assert_eq!(
            physical_channel_count(&self.store.index),
            physical_channel_count(&self.before.index)
        );
        let catalogs = self.store.candidate_catalogs.read().unwrap();
        let before_catalogs = self.before.candidate_catalogs.read().unwrap();
        assert_eq!(catalogs.len(), before_catalogs.len());
        for channel in &self.channels {
            assert_eq!(
                self.store.channel_data(channel).unwrap(),
                self.before.channel_data(channel).unwrap()
            );
            let marker = RhoPathV1::datum_prefix(channel);
            assert!(Arc::ptr_eq(
                catalogs.get(&marker).unwrap(),
                before_catalogs.get(&marker).unwrap()
            ));
        }
        std::fs::write(&self.marker, self.case.as_bytes()).unwrap();
    }
}

fn poison_catalog_lock(store: &RSpaceDataStore<String, String>) {
    std::thread::scope(|scope| {
        let catalogs = &store.candidate_catalogs;
        let _ = scope
            .spawn(move || {
                let _guard = catalogs.write().unwrap();
                panic!("synthetic catalog lock poison");
            })
            .join();
    });
    assert!(store.candidate_catalogs.is_poisoned());
}

fn run_poison_publication_child(case: &str, marker: std::ffi::OsString) {
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
    let mut verify = VerifyPoisonedStoreOnDrop {
        before: store.clone(),
        store,
        channels: vec![target.clone(), sibling],
        case: case.to_owned(),
        marker: marker.into(),
    };
    poison_catalog_lock(&verify.store);
    let _ = verify.store.set_channel(target, [datum(3, false)]);
}

fn run_poison_clear_child(case: &str, marker: std::ffi::OsString) {
    let channel = "loaded".to_owned();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(channel.clone(), [datum(1, false)])
        .unwrap();
    let _ = store.channel_pool(&channel).unwrap().unwrap();
    let mut verify = VerifyPoisonedStoreOnDrop {
        before: store.clone(),
        store,
        channels: vec![channel],
        case: case.to_owned(),
        marker: marker.into(),
    };
    poison_catalog_lock(&verify.store);
    verify.store.clear();
}

fn run_poison_delta_child(case: &str, marker: std::ffi::OsString) {
    assert!(
        matches!(
            shared::rust::test_process::prepare_expected_probe_core_dump(),
            shared::rust::test_process::ExpectedProbeCorePolicy::SuppressionInstalled |
                shared::rust::test_process::ExpectedProbeCorePolicy::ExplicitlyAllowed
        ),
        "the poison-delta child cannot suppress its core dump"
    );
    let target = "target".to_owned();
    let sibling = "sibling".to_owned();
    let mut store = RSpaceDataStore::new();
    store
        .set_channel(target.clone(), [datum(1, false)])
        .expect("the target channel loads");
    store
        .set_channel(sibling.clone(), [datum(2, true)])
        .expect("the sibling channel loads");
    let _ = store
        .channel_pool(&target)
        .expect("the target catalog builds")
        .expect("the target channel is loaded");
    let _ = store
        .channel_pool(&sibling)
        .expect("the sibling catalog builds")
        .expect("the sibling channel is loaded");
    let mut delta = BagDelta::new();
    delta
        .record_addition(&RSpaceDataIndex::datum_path(&target, &datum(3, false)), Multiplicity::ONE)
        .expect("the delta records one addition");
    delta
        .record_removal(&RSpaceDataIndex::datum_path(&sibling, &datum(2, true)), Multiplicity::ONE)
        .expect("the delta records one removal");
    let mut verify = VerifyPoisonedStoreOnDrop {
        before: store.clone(),
        store,
        channels: vec![target, sibling],
        case: case.to_owned(),
        marker: marker.into(),
    };
    poison_catalog_lock(&verify.store);
    let _ = verify.store.apply_delta(&delta);
}

#[test]
fn loaded_count_overflow_preflight_precedes_iterator_consumption() {
    let mut store = RSpaceDataStore::<String, String>::new();
    store.index.loaded_channel_count = usize::MAX;
    let before_index = store.index.clone();
    let consumed = std::cell::Cell::new(false);
    let datums = std::iter::once_with(|| {
        consumed.set(true);
        datum(1, false)
    });

    let outcome = store.set_channel("new-target".to_owned(), datums);
    assert_eq!(outcome, Err(DataIndexError::LoadedChannelCountOverflow));
    assert!(!consumed.get());
    assert_eq!(store.index, before_index);
    assert_eq!(store.loaded_channel_count(), usize::MAX);
    assert_eq!(physical_channel_count(&store.index), 0);
}

fn assert_channel_invariant(invariant: ChannelInvariant) {
    match invariant {
        ChannelInvariant::StrongFailureAtomicity => {
            preparation_faults_are_strongly_atomic();
            return;
        }
        ChannelInvariant::PreparationFailurePrecedence => {
            catalog_lock_poison_precedes_authoritative_publication();
            return;
        }
        ChannelInvariant::LoadedCapacity => {
            loaded_count_overflow_preflight_precedes_iterator_consumption();
            return;
        }
        ChannelInvariant::CheckedU64Multiplicity => {
            let maximum = Multiplicity::new(u64::MAX).unwrap();
            assert!(maximum.checked_add(Multiplicity::ONE).is_err());
            assert!(Multiplicity::new(0).is_err());
            return;
        }
        ChannelInvariant::PrefixSuffixComposition => {
            for marker in [vec![], vec![0], vec![1, 2], vec![255, 0, 255]] {
                for suffix in [vec![], vec![3], vec![4, 5], vec![0, 255, 0]] {
                    let absolute = compose_path(&marker, &suffix);
                    assert_eq!(absolute.strip_prefix(marker.as_slice()), Some(suffix.as_slice()));
                }
            }
            return;
        }
        ChannelInvariant::EmptyLoadedChannel => {
            let channel = "loaded-empty".to_owned();
            let mut store = RSpaceDataStore::<String, String>::new();
            store.set_channel(channel.clone(), []).unwrap();
            assert!(store.is_loaded(&channel));
            assert_eq!(store.loaded_channel_count(), 1);
            assert_eq!(store.channel_data(&channel).unwrap(), Some(Vec::new()));
            return;
        }
        _ => {}
    }

    let target = "shared\0target".to_owned();
    let sibling = "shared\0sibling".to_owned();
    let old_repeated = datum(1, false);
    let old_second = datum(2, true);
    let replacement_repeated = datum(3, false);
    let replacement_second = datum(4, true);
    let sibling_value = datum(9, false);
    let replacement = vec![
        replacement_repeated.clone(),
        replacement_repeated.clone(),
        replacement_second.clone(),
    ];

    let mut before = RSpaceDataStore::new();
    before
        .set_channel(target.clone(), [
            old_repeated.clone(),
            old_repeated.clone(),
            old_second.clone(),
        ])
        .unwrap();
    before
        .set_channel(sibling.clone(), [sibling_value.clone()])
        .unwrap();
    assert_store_coherent(invariant, "after initial channel publication", &before);
    let target_marker = RhoPathV1::datum_prefix(&target);
    let sibling_marker = RhoPathV1::datum_prefix(&sibling);
    let old_target_pool = before.channel_pool(&target).unwrap().unwrap();
    assert_store_coherent(invariant, "after target candidate-pool snapshot", &before);
    let _ = before.channel_pool(&sibling).unwrap().unwrap();
    assert_store_coherent(invariant, "after sibling candidate-pool snapshot", &before);
    let sibling_catalog = Arc::clone(
        before
            .candidate_catalogs
            .read()
            .unwrap()
            .get(&sibling_marker)
            .unwrap(),
    );
    let before_distinct = before.distinct_len();
    let before_cardinality = before.cardinality();
    let old_target_distinct = before
        .index
        .channel_entries::<_, String>(&target)
        .unwrap()
        .len();
    let old_target_cardinality = before.channel_data(&target).unwrap().unwrap().len() as u64;
    let expected = reference_set_channel(before.clone(), target.clone(), &replacement);
    assert_store_coherent(invariant, "after reference construction", &before);
    assert_store_coherent(invariant, "inside the reference image", &expected);
    let mut actual = before;
    actual.set_channel(target.clone(), replacement).unwrap();
    assert_store_coherent(invariant, "after production replacement", &actual);

    let repeated_path = RSpaceDataIndex::datum_path(&target, &replacement_repeated);
    let second_path = RSpaceDataIndex::datum_path(&target, &replacement_second);
    let old_path = RSpaceDataIndex::datum_path(&target, &old_repeated);
    let sibling_path = RSpaceDataIndex::datum_path(&sibling, &sibling_value);
    let repeated_relative = RSpaceDataIndex::relative_datum_path(&replacement_repeated);
    let second_relative = RSpaceDataIndex::relative_datum_path(&replacement_second);

    match invariant {
        ChannelInvariant::ExactTargetPrefix => {
            assert_eq!(actual.index.count_path(&old_path), 0);
            assert_eq!(actual.index.count_path(&repeated_path), 2);
            assert_eq!(actual.index.count_path(&second_path), 1);
        }
        ChannelInvariant::NonTargetKeyPreservation | ChannelInvariant::RelativeCursorNoEscape => {
            assert_eq!(actual.index.count_path(&sibling_path), 1);
        }
        ChannelInvariant::IndependentChannelPreservation => {
            assert_eq!(actual.channel_data(&sibling).unwrap(), Some(vec![sibling_value.clone()]));
        }
        ChannelInvariant::ReusableBufferRestoration => {
            assert_eq!(repeated_path, compose_path(&target_marker, &repeated_relative));
            assert_eq!(second_path, compose_path(&target_marker, &second_relative));
            let leaked = compose_path(&repeated_path, &second_relative);
            assert_eq!(actual.index.count_path(&leaked), 0);
        }
        ChannelInvariant::LoadedSet => assert!(actual.is_loaded(&target)),
        ChannelInvariant::LoadedCount | ChannelInvariant::LoadedSetUniqueness => {
            assert_eq!(actual.loaded_channel_count(), physical_channel_count(&actual.index));
        }
        ChannelInvariant::TargetCatalogInvalidation => {
            assert!(
                !actual
                    .candidate_catalogs
                    .read()
                    .unwrap()
                    .contains_key(&target_marker)
            );
        }
        ChannelInvariant::NonTargetCatalogIdentity => {
            assert!(Arc::ptr_eq(
                actual
                    .candidate_catalogs
                    .read()
                    .unwrap()
                    .get(&sibling_marker)
                    .unwrap(),
                &sibling_catalog,
            ));
        }
        ChannelInvariant::ExternalCatalogSnapshot => {
            assert_eq!(old_target_pool.len(), 2);
            assert_eq!(old_target_pool.stored_availability().exact_cardinality(), 3);
        }
        ChannelInvariant::DistinctCountFormula => {
            assert_eq!(actual.distinct_len(), before_distinct - old_target_distinct + 2);
        }
        ChannelInvariant::CardinalityFormula => {
            assert_eq!(actual.cardinality(), before_cardinality - old_target_cardinality + 3);
        }
        ChannelInvariant::MultiplicityPreservation |
        ChannelInvariant::PreparedAggregationRefinement => {
            assert_eq!(actual.index.count_path(&repeated_path), 2);
            assert_eq!(actual.index.count_path(&second_path), 1);
        }
        ChannelInvariant::ReferenceSuccessEquivalence |
        ChannelInvariant::PrefixRootedCursor |
        ChannelInvariant::SingleCursorReplay => {
            assert_eq!(actual, expected);
        }
        ChannelInvariant::PreparedBagWellFormed | ChannelInvariant::ResultWellFormed => {
            actual
                .index
                .try_visit_channels(|_, bag| {
                    let expected_entries = bag
                        .iter()
                        .map(|(path, multiplicity)| (path, multiplicity.get()))
                        .collect::<Vec<_>>();
                    assert_eq!(
                        expected_entries.len(),
                        bag.distinct_len(),
                        "{invariant:?}: inner trie and distinct metadata diverged before encoding",
                    );
                    let bytes = bag.compact_bytes();
                    let decoded = Bag::from_compact_bytes(&bytes).unwrap();
                    assert_eq!(
                        bag.iter()
                            .map(|(path, multiplicity)| (path, multiplicity.get()))
                            .collect::<Vec<_>>(),
                        expected_entries,
                        "{invariant:?}: decoding an independent byte image changed the authority",
                    );
                    assert_eq!(
                        decoded
                            .iter()
                            .map(|(path, multiplicity)| (path, multiplicity.get()))
                            .collect::<Vec<_>>(),
                        expected_entries,
                        "{invariant:?}: compact decode changed canonical endpoints",
                    );
                    assert!(
                        decoded
                            .iter()
                            .all(|(_, multiplicity)| multiplicity.get() > 0)
                    );
                    Ok::<(), std::convert::Infallible>(())
                })
                .unwrap();
        }
        ChannelInvariant::CatalogInvariantPreservation => {
            let catalogs = actual.candidate_catalogs.read().unwrap();
            assert!(!catalogs.contains_key(&target_marker));
            assert!(Arc::ptr_eq(catalogs.get(&sibling_marker).unwrap(), &sibling_catalog));
        }
        ChannelInvariant::StrongFailureAtomicity |
        ChannelInvariant::PrefixSuffixComposition |
        ChannelInvariant::EmptyLoadedChannel |
        ChannelInvariant::CheckedU64Multiplicity |
        ChannelInvariant::LoadedCapacity |
        ChannelInvariant::PreparationFailurePrecedence => unreachable!(),
    }
}

fn assert_channel_mutant_is_causally_killed(mutant: ChannelMutant) {
    let (before, target, sibling, prepared) = model_fixture();
    let expected = model_publish(before.clone(), &target, &prepared);
    match mutant {
        ChannelMutant::ClearBeforeEncoding => {
            let mut changed = before.clone();
            changed
                .authority
                .retain(|path, _| !path.starts_with(&target));
            assert_ne!(changed, before, "failure must preserve the complete pre-state");
        }
        ChannelMutant::PublishAfterEncodingFailure => {
            assert_ne!(expected, before, "a failed preparation cannot publish a prepared image");
        }
        ChannelMutant::WholeAuthorityClear => {
            let mut changed = expected.clone();
            changed
                .authority
                .retain(|path, _| path.starts_with(&target));
            assert_ne!(changed, expected);
        }
        ChannelMutant::RetainOldTarget => {
            let mut changed = expected.clone();
            changed.authority.insert(vec![1, 2, 3], 2);
            assert_ne!(changed, expected);
        }
        ChannelMutant::SkipPreparedSuffix => {
            assert_ne!(model_publish(before, &target, &[]), expected);
        }
        ChannelMutant::DuplicatePreparedSuffix => {
            let duplicated = vec![prepared[0].clone(), prepared[0].clone(), prepared[1].clone()];
            assert_ne!(model_publish(before, &target, &duplicated), expected);
        }
        ChannelMutant::UnframedRelativeKey => {
            let mut changed = before;
            changed
                .authority
                .retain(|path, _| !path.starts_with(&target));
            changed.replay_trace.clear();
            for (suffix, multiplicity) in &prepared {
                changed.authority.insert(suffix.clone(), *multiplicity);
                changed.replay_trace.push(suffix.clone());
            }
            changed.catalogs.remove(&target);
            assert_ne!(changed, expected);
        }
        ChannelMutant::FlattenMultiplicity => {
            let flattened = prepared
                .iter()
                .map(|(suffix, _)| (suffix.clone(), 1))
                .collect::<Vec<_>>();
            assert_ne!(model_publish(before, &target, &flattened), expected);
        }
        ChannelMutant::DuplicatePreparedKey => {
            let duplicate_keys = [prepared[0].0.clone(), prepared[0].0.clone()];
            assert_ne!(
                duplicate_keys.iter().collect::<BTreeSet<_>>().len(),
                duplicate_keys.len(),
                "sorted-unique preparation must reject duplicate suffix keys",
            );
        }
        ChannelMutant::U64Overflow => {
            assert!(u64::MAX.checked_add(1).is_none(), "overflow must fault before publication");
        }
        ChannelMutant::CursorTargetDrift => {
            assert_ne!(compose_path(&prepared[0].0, &[]), compose_path(&target, &prepared[0].0));
        }
        ChannelMutant::AbsoluteCursorWrite => {
            let mut changed = expected.clone();
            changed.authority.insert(vec![9, 8], 1);
            assert_ne!(changed, expected);
        }
        ChannelMutant::WrongBufferRestore => {
            let correct_restore = target.clone();
            let wrong_restore = prepared[0].0.clone();
            assert_ne!(wrong_restore, correct_restore);
        }
        ChannelMutant::DoubleLoadedCount => {
            let mut changed = expected.clone();
            changed.loaded_count += 1;
            assert_ne!(changed.loaded_count, changed.loaded.len());
        }
        ChannelMutant::EmptyNotLoaded => {
            let mut unloaded = before;
            unloaded.loaded.remove(&target);
            unloaded.loaded_count = unloaded.loaded.len();
            unloaded.catalogs.remove(&target);
            let published = model_publish(unloaded.clone(), &target, &[]);
            assert_ne!(unloaded.loaded, published.loaded);
        }
        ChannelMutant::CapacityPreflightOmission => {
            assert!(usize::MAX.checked_add(1).is_none());
        }
        ChannelMutant::StaleTargetCatalog => {
            let mut changed = expected.clone();
            changed.catalogs.insert(target, 21);
            assert_ne!(changed, expected);
        }
        ChannelMutant::ClearAllCatalogs => {
            let mut changed = expected.clone();
            changed.catalogs.clear();
            assert_ne!(changed.catalogs.get(&sibling), expected.catalogs.get(&sibling));
        }
        ChannelMutant::ExternalSnapshotMutation => {
            let mut changed = expected.clone();
            changed.external_snapshot = 42;
            assert_ne!(changed.external_snapshot, expected.external_snapshot);
        }
        ChannelMutant::NoncanonicalReplayOrder => {
            let mut changed = expected.clone();
            changed.replay_trace.reverse();
            assert_ne!(changed.replay_trace, expected.replay_trace);
        }
    }
}

#[test]
fn formal_invariants_and_causal_mutants_have_executable_witnesses() {
    let ids = INVARIANT_BINDINGS
        .iter()
        .map(|binding| binding.id)
        .collect::<BTreeSet<_>>();
    let invariants = INVARIANT_BINDINGS
        .iter()
        .map(|binding| binding.invariant)
        .collect::<BTreeSet<_>>();
    assert_eq!(INVARIANT_BINDINGS.len(), 27);
    assert_eq!(ids.len(), INVARIANT_BINDINGS.len(), "duplicate CR property ID");
    assert_eq!(invariants.len(), INVARIANT_BINDINGS.len(), "duplicate invariant witness");
    for binding in INVARIANT_BINDINGS {
        assert_channel_invariant(binding.invariant);
    }

    let mutants = MUTANT_CATALOG.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(MUTANT_CATALOG.len(), 20);
    assert_eq!(mutants.len(), MUTANT_CATALOG.len(), "duplicate causal mutant control");
    for mutant in MUTANT_CATALOG {
        assert_channel_mutant_is_causally_killed(*mutant);
    }
}

#[test]
fn reference_equivalence_teardown_preserves_the_next_fresh_bag_authority() {
    assert_channel_invariant(ChannelInvariant::ReferenceSuccessEquivalence);
    assert_channel_invariant(ChannelInvariant::PreparedBagWellFormed);
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
