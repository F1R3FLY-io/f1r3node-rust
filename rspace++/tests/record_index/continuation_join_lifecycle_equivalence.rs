//! End-to-end continuation/join lifecycle equivalence.
//!
//! The production side uses the PathMap-backed hot store.  The independent
//! side uses a small HashMap/Vec oracle and typed legacy checkpoint actions.
//! One fixture crosses first-wins identity, candidate order, duplicate joins,
//! canonical checkpoint bytes/root, reset, authenticated export/import,
//! play/replay, and the final post-state.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::serializers::serializers::CandidateOrderingBytes;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportPage, StateImportPublicationMode, StateImportPublicationWitness, StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use rspace_plus_plus::rspace::trace::event::{Consume, Event};
use shared::rust::{Byte, ByteVector};

#[path = "../support/legacy_continuation_join_oracle.rs"]
mod legacy_oracle;

use legacy_oracle::{LegacyContinuationJoinOracle, TestState, same_continuation_identity};

type TestRepository = dyn HistoryRepository<String, String, String, String> + Send + Sync + 'static;
type TestSpace = RSpace<String, String, String, String>;
type TestReplay = ReplayRSpace<String, String, String, String>;
type StateItem = (Blake2b256Hash, ByteVector);
type StatePath = Vec<(Blake2b256Hash, Option<Byte>)>;

#[derive(Clone)]
struct ExactMatch;

impl Match<String, String, String> for ExactMatch {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        ((pattern == "keep-a" && datum.starts_with("keep-a")) || pattern == datum)
            .then(|| datum.clone())
    }
}

async fn repository(manager: &mut InMemoryStoreManager, name: &str) -> Arc<Box<TestRepository>> {
    Arc::new(
        HistoryRepositoryInstances::<String, String, String, String>::repository(
            manager.state_authority(name).await.expect("test authority"),
        )
        .expect("history repository"),
    )
}

fn space_from_repository(repository: Arc<Box<TestRepository>>) -> TestSpace {
    let reader = repository
        .get_history_reader(&repository.root())
        .expect("history reader");
    RSpace::apply(
        repository,
        HotStoreInstances::create_from_hs_and_hr(TestState::default(), reader.base()),
        Arc::new(Box::new(ExactMatch)),
    )
}

fn replay_from_repository(repository: Arc<Box<TestRepository>>) -> TestReplay {
    let reader = repository
        .get_history_reader(&repository.root())
        .expect("history reader");
    ReplayRSpace::apply(
        repository,
        Arc::new(HotStoreInstances::create_from_hs_and_hr(TestState::default(), reader.base())),
        Arc::new(Box::new(ExactMatch)),
    )
}

fn waiting(
    channels: &[String],
    name: String,
    source_marker: u8,
) -> WaitingContinuation<String, String> {
    let patterns = vec!["keep-a".to_string(), "keep-b".to_string()];
    let mut source = Consume::create(&channels.to_vec(), &patterns, &name, false);
    source.hash = Blake2b256Hash::from_bytes(vec![source_marker; 32]);
    WaitingContinuation {
        patterns: Arc::new(patterns),
        continuation: Arc::new(name),
        persist: false,
        peeks: BTreeSet::new(),
        source,
    }
}

/// Find two complete records whose byte order and digest-first candidate order
/// disagree, so the fixture cannot pass by inheriting storage order.
fn opposed_continuations(
    channels: &[String],
) -> (WaitingContinuation<String, String>, WaitingContinuation<String, String>) {
    let candidates = (0_u8..32)
        .map(|marker| waiting(channels, format!("continuation-{marker:02}"), marker + 1))
        .collect::<Vec<_>>();
    for left in &candidates {
        for right in &candidates {
            if left.continuation == right.continuation {
                continue;
            }
            let left_bytes = left.candidate_ordering_bytes();
            let right_bytes = right.candidate_ordering_bytes();
            let byte_order = left_bytes.cmp(&right_bytes);
            let candidate_order =
                rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash(left)
                    .cmp(&rspace_plus_plus::rspace::candidate_order::deterministic_candidate_hash(
                        right,
                    ))
                    .then_with(|| byte_order);
            if byte_order != candidate_order {
                return (left.clone(), right.clone());
            }
        }
    }
    panic!("32 frozen continuation records must contain an order-opposed pair");
}

fn fixture_state(
    channels: &[String],
    first: WaitingContinuation<String, String>,
    alias: WaitingContinuation<String, String>,
    second: WaitingContinuation<String, String>,
) -> HotStoreState<String, String, String, String> {
    let a = channels[0].clone();
    let b = channels[1].clone();
    let other = vec![a.clone(), "other".to_string()];
    HotStoreState {
        continuations: HashMap::from([(channels.to_vec(), vec![first, alias, second])]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(b.clone(), vec![Datum::create(&b, "keep-b".to_string(), true)])]),
        joins: HashMap::from([
            (a, vec![channels.to_vec(), other, channels.to_vec()]),
            (b, vec![channels.to_vec(), channels.to_vec()]),
        ]),
        installed_joins: HashMap::new(),
    }
}

fn assert_state_fields(actual: &TestState, expected: &TestState) {
    let mut actual_continuations = actual.continuations.clone();
    let mut expected_continuations = expected.continuations.clone();
    actual_continuations.retain(|_, values| !values.is_empty());
    expected_continuations.retain(|_, values| !values.is_empty());
    assert_eq!(actual_continuations, expected_continuations);
    assert_eq!(actual.installed_continuations, expected.installed_continuations);
    let mut actual_data = actual.data.clone();
    let mut expected_data = expected.data.clone();
    actual_data.retain(|_, values| !values.is_empty());
    expected_data.retain(|_, values| !values.is_empty());
    assert_eq!(actual_data, expected_data);
    let mut actual_joins = actual.joins.clone();
    let mut expected_joins = expected.joins.clone();
    actual_joins.retain(|_, values| !values.is_empty());
    expected_joins.retain(|_, values| !values.is_empty());
    assert_eq!(actual_joins, expected_joins);
    assert_eq!(actual.installed_joins, expected.installed_joins);
}

async fn load_observed_namespaces(space: &TestSpace, channels: &[String]) -> TestState {
    let _ = space.get_waiting_continuations(channels.to_vec()).await;
    for channel in channels {
        let _ = space.get_data(channel).await;
        let _ = space.get_joins(channel.clone()).await;
    }
    space.get_store().snapshot()
}

fn export_all(
    exporter: Arc<dyn RSpaceExporter>,
    root: &Blake2b256Hash,
    page_size: i32,
) -> (Vec<StateItem>, Vec<StateItem>) {
    let start: StatePath = vec![(root.clone(), None)];
    let mut skip = 0_i32;
    let mut history = Vec::new();
    let mut data = Vec::new();
    loop {
        let exported = RSpaceExporterItems::get_history_and_data(
            Arc::clone(&exporter),
            start.clone(),
            skip,
            page_size,
        )
        .expect("authenticated export page");
        assert_eq!(exported.0.last_path, exported.1.last_path);
        let terminal = exported.0.last_path == start;
        history.extend(exported.0.items);
        data.extend(exported.1.items);
        if terminal {
            break;
        }
        skip = skip
            .checked_add(page_size)
            .expect("bounded test pagination");
    }
    assert!(!history.is_empty(), "the fixture must export authenticated history objects");
    (history, data)
}

fn unique_projection(items: &[StateItem]) -> BTreeMap<Blake2b256Hash, ByteVector> {
    let mut unique = BTreeMap::new();
    for (digest, bytes) in items {
        if let Some(previous) = unique.insert(digest.clone(), bytes.clone()) {
            assert_eq!(previous, *bytes, "one digest cannot name conflicting bytes");
        }
    }
    unique
}

fn import_root(
    exporter: Arc<dyn RSpaceExporter>,
    importer: Arc<dyn RSpaceImporter>,
    root: &Blake2b256Hash,
) {
    let spec =
        StateImportSpec::new([root.clone()], StateImportPublicationMode::Activate(root.clone()))
            .expect("one activation root");
    let mut session = importer.begin_import(spec).expect("begin affine import");
    let mut start: StatePath = vec![(root.clone(), None)];
    let mut seen = BTreeSet::new();
    loop {
        assert!(seen.insert(start.clone()), "import cursor must progress");
        let exported =
            RSpaceExporterItems::get_history_and_data(Arc::clone(&exporter), start.clone(), 0, 2)
                .expect("export affine page");
        assert_eq!(exported.0.last_path, exported.1.last_path);
        let receipt = session
            .validate_and_stage_page(StateImportPage {
                start_path: start.clone(),
                advertised_next_path: exported.0.last_path,
                skip: 0,
                take: 2,
                history_items: exported.0.items,
                data_items: exported.1.items,
            })
            .expect("validate affine page");
        if receipt.terminal {
            break;
        }
        assert_ne!(receipt.next_path, start);
        start = receipt.next_path;
    }
    let publication = session
        .seal()
        .expect("complete closure")
        .publish(&StateImportPublicationWitness::new())
        .expect("publish imported root");
    assert_eq!(publication.verified_roots, vec![root.clone()]);
    assert_eq!(publication.activation.expect("activation").root, *root);
}

fn continuation_names(values: &[WaitingContinuation<String, String>]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.continuation.as_ref().clone())
        .collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LifecycleObservation {
    selected: Vec<String>,
    selected_sources: Vec<Blake2b256Hash>,
    comms: usize,
    continuation_count: usize,
    join_a: Vec<Vec<String>>,
    join_b: Vec<Vec<String>>,
    export_history_objects: usize,
}

#[derive(Clone, Copy, Debug)]
enum LifecycleMutant {
    LastWinsContinuationIdentity,
    CollapseDuplicateJoinMultiplicity,
    ReverseOperationalCandidateOrder,
    OmitComm,
    DropJoinDuringReset,
    DropCheckpointJoinOccurrence,
    DropExportedClosureObject,
    ReplaySelectNextContinuation,
    RetainRemovedContinuationInFinalState,
}

fn mutate(
    baseline: &LifecycleObservation,
    mutant: LifecycleMutant,
    aliased_identity: &str,
    alias_source: &Blake2b256Hash,
) -> LifecycleObservation {
    let mut changed = baseline.clone();
    match mutant {
        LifecycleMutant::LastWinsContinuationIdentity => {
            let position = changed
                .selected
                .iter()
                .position(|name| name == aliased_identity)
                .expect("the aliased identity must be selected");
            changed.selected_sources[position] = alias_source.clone();
        }
        LifecycleMutant::CollapseDuplicateJoinMultiplicity |
        LifecycleMutant::DropCheckpointJoinOccurrence => {
            changed.join_a.pop();
        }
        LifecycleMutant::ReverseOperationalCandidateOrder |
        LifecycleMutant::ReplaySelectNextContinuation => changed.selected.reverse(),
        LifecycleMutant::OmitComm => changed.comms -= 1,
        LifecycleMutant::DropJoinDuringReset => changed.join_b.clear(),
        LifecycleMutant::DropExportedClosureObject => changed.export_history_objects -= 1,
        LifecycleMutant::RetainRemovedContinuationInFinalState => changed.continuation_count += 1,
    }
    changed
}

#[tokio::test(flavor = "current_thread")]
async fn legacy_and_pathmap_continuation_join_lifecycles_are_identical() {
    let channels = vec!["a".to_string(), "b".to_string()];
    let (first, second) = opposed_continuations(&channels);
    let mut alias = first.clone();
    alias.source.hash = Blake2b256Hash::from_bytes(vec![0xee; 32]);
    assert!(same_continuation_identity(&first, &alias));
    assert_ne!(first.source, alias.source, "alias must exercise first-source retention");

    let raw_state = fixture_state(&channels, first.clone(), alias.clone(), second.clone());
    let mut oracle = LegacyContinuationJoinOracle::from_snapshot(raw_state.clone());
    assert_eq!(oracle.continuations[&channels].len(), 2);
    assert_eq!(oracle.continuations[&channels][0].source, first.source);

    let mut manager = InMemoryStoreManager::new();
    let production_repository = repository(&mut manager, "lifecycle-production").await;
    let legacy_repository = repository(&mut manager, "lifecycle-legacy").await;
    let import_repository = repository(&mut manager, "lifecycle-import").await;
    let final_import_repository = repository(&mut manager, "lifecycle-final-import").await;
    let play = space_from_repository(Arc::clone(&production_repository));
    let replay = replay_from_repository(Arc::clone(&production_repository));
    play.get_store().set_state(raw_state);
    assert_state_fields(&play.get_store().snapshot(), &oracle.snapshot());

    let initial = play.create_checkpoint().await.expect("PathMap checkpoint");
    let legacy_initial = legacy_repository
        .checkpoint(oracle.typed_checkpoint_actions())
        .expect("legacy typed checkpoint");
    assert_eq!(initial.root, legacy_initial.root(), "initial root equivalence");

    let production_export = export_all(production_repository.exporter(), &initial.root, 2);
    let legacy_export = export_all(legacy_initial.exporter(), &initial.root, 2);
    assert_eq!(
        unique_projection(&production_export.0),
        unique_projection(&legacy_export.0),
        "history object closure"
    );
    assert_eq!(
        unique_projection(&production_export.1),
        unique_projection(&legacy_export.1),
        "cold-leaf object closure"
    );

    import_root(production_repository.exporter(), import_repository.importer(), &initial.root);
    let imported = space_from_repository(Arc::clone(&import_repository));
    imported
        .reset(&initial.root)
        .await
        .expect("reset imported root");
    assert_state_fields(
        &load_observed_namespaces(&imported, &channels).await,
        &oracle.canonical_history_snapshot(),
    );

    play.reset(&initial.root)
        .await
        .expect("reset production root");
    assert_state_fields(
        &load_observed_namespaces(&play, &channels).await,
        &oracle.canonical_history_snapshot(),
    );
    oracle = LegacyContinuationJoinOracle::from_snapshot(oracle.canonical_history_snapshot());

    // Non-vacuity probe: inject only the missing `a` datum into a private hot
    // view of the same checkpoint. Both continuations must be enabled and the
    // PathMap enumerator must equal the independent digest/bytes/index order.
    let probe = space_from_repository(Arc::clone(&production_repository));
    probe.reset(&initial.root).await.expect("probe reset");
    let _ = load_observed_namespaces(&probe, &channels).await;
    probe
        .get_store()
        .put_datum(&channels[0], Datum::create(&channels[0], "keep-a".to_string(), true));
    let enabled = probe.enabled_rendezvous();
    assert_eq!(enabled.len(), 2, "both first-wins continuations are enabled");
    let expected_order = oracle.ordered_continuations(&channels);
    let enabled_names = enabled
        .iter()
        .map(|candidate| candidate.continuation().continuation.as_ref().clone())
        .collect::<Vec<_>>();
    assert_eq!(
        enabled_names,
        expected_order
            .iter()
            .map(|(continuation, _)| continuation.continuation.as_ref().clone())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        enabled
            .iter()
            .map(|candidate| candidate.continuation_index())
            .collect::<Vec<_>>(),
        expected_order
            .iter()
            .map(|(_, index)| *index)
            .collect::<Vec<_>>()
    );
    assert_ne!(
        enabled_names,
        continuation_names(&oracle.canonical_history_snapshot().continuations[&channels]),
        "candidate order must oppose cold-store record order"
    );

    let mut selected = Vec::new();
    let mut selected_sources = Vec::new();
    for arrival in ["keep-a-1", "keep-a-2"] {
        let expected = oracle.ordered_continuations(&channels)[0].clone();
        let fired = play
            .produce(channels[0].clone(), arrival.to_string(), true)
            .await
            .expect("play produce")
            .expect("fresh datum completes the join");
        assert_eq!(&fired.0.continuation, expected.0.continuation.as_ref());
        assert_eq!(
            fired
                .1
                .iter()
                .map(|result| result.matched_datum.clone())
                .collect::<Vec<_>>(),
            vec![arrival.to_string(), "keep-b".to_string()]
        );
        selected.push(fired.0.continuation.clone());
        let removed = oracle.apply_selected(&channels, expected.1);
        assert_eq!(removed.continuation.as_ref(), &fired.0.continuation);
        selected_sources.push(removed.source.hash);
    }
    assert_ne!(selected[0], selected[1]);
    assert!(oracle.continuations[&channels].is_empty());
    assert_eq!(oracle.joins[&channels[0]].len(), 2);
    assert_eq!(oracle.joins[&channels[1]].len(), 1);
    assert_state_fields(&play.get_store().snapshot(), &oracle.snapshot());

    let final_point = play.create_checkpoint().await.expect("final checkpoint");
    let comms = final_point
        .log
        .iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count();
    assert_eq!(comms, 2, "both selections must publish a COMM");
    let legacy_final = repository(&mut manager, "lifecycle-legacy-final")
        .await
        .checkpoint(oracle.typed_checkpoint_actions())
        .expect("legacy final checkpoint");
    assert_eq!(final_point.root, legacy_final.root(), "final root equivalence");

    replay
        .rig_and_reset(initial.root.clone(), final_point.log.clone())
        .await
        .expect("rig replay from initial root");
    let mut replay_selected = Vec::new();
    for arrival in ["keep-a-1", "keep-a-2"] {
        let fired = replay
            .produce(channels[0].clone(), arrival.to_string(), true)
            .await
            .expect("replay produce")
            .expect("recorded COMM must replay");
        replay_selected.push(fired.0.continuation);
    }
    assert_eq!(replay_selected, selected);
    replay
        .check_replay_data()
        .await
        .expect("all replay bindings consumed");
    let replay_final = replay.create_checkpoint().await.expect("replay checkpoint");
    assert_eq!(replay_final.root, final_point.root, "replay final root");

    let final_export = export_all(production_repository.exporter(), &final_point.root, 2);
    import_root(
        production_repository.exporter(),
        final_import_repository.importer(),
        &final_point.root,
    );
    let final_imported = space_from_repository(Arc::clone(&final_import_repository));
    final_imported
        .reset(&final_point.root)
        .await
        .expect("reset final imported root");
    assert_state_fields(
        &load_observed_namespaces(&final_imported, &channels).await,
        &oracle.canonical_history_snapshot(),
    );

    let baseline = LifecycleObservation {
        selected,
        selected_sources,
        comms,
        continuation_count: oracle.continuations[&channels].len(),
        join_a: oracle.joins[&channels[0]].clone(),
        join_b: oracle.joins[&channels[1]].clone(),
        export_history_objects: unique_projection(&final_export.0).len(),
    };
    assert!(baseline.export_history_objects > 0);
    for mutant in [
        LifecycleMutant::LastWinsContinuationIdentity,
        LifecycleMutant::CollapseDuplicateJoinMultiplicity,
        LifecycleMutant::ReverseOperationalCandidateOrder,
        LifecycleMutant::OmitComm,
        LifecycleMutant::DropJoinDuringReset,
        LifecycleMutant::DropCheckpointJoinOccurrence,
        LifecycleMutant::DropExportedClosureObject,
        LifecycleMutant::ReplaySelectNextContinuation,
        LifecycleMutant::RetainRemovedContinuationInFinalState,
    ] {
        assert_ne!(
            mutate(&baseline, mutant, alias.continuation.as_ref(), &alias.source.hash,),
            baseline,
            "fixture failed to kill {mutant:?}"
        );
    }
}
