use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, RwLock};

use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_reader::HistoryReaderBase;
use crate::rspace::internal::AcceptedDatumChanges;
use crate::rspace::trace::event::Consume;

type TestStore = InMemHotStore<String, String, String, String>;
type TestState = InMemHotStoreState<String, String, String, String>;
type TestSnapshot = HotStoreState<String, String, String, String>;
type TestContinuation = WaitingContinuation<String, String>;

const GROUP_COUNT: usize = 4;
const CHANNEL_COUNT: usize = 3;

struct GroupHistory {
    continuations: HashMap<Vec<String>, Vec<TestContinuation>>,
    joins: HashMap<String, Vec<Vec<String>>>,
}

impl GroupHistory {
    fn resting(&self, channels: &[String]) -> Vec<TestContinuation> {
        self.continuations
            .get(channels)
            .cloned()
            .unwrap_or_default()
    }
}

impl HistoryReaderBase<String, String, String, String> for GroupHistory {
    fn get_data_canonical(&self, _key: &String) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, key: &[String]) -> Vec<TestContinuation> {
        self.resting(key)
    }

    fn get_joins_canonical(&self, key: &String) -> Vec<Vec<String>> {
        self.joins.get(key).cloned().unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
struct GroupObservation {
    resting: Vec<TestContinuation>,
    installed: Option<TestContinuation>,
    version: Option<ContinuationVersion>,
}

#[derive(Clone, Debug)]
enum TokenOperation {
    Read {
        group: usize,
    },
    Put {
        group: usize,
        name: u8,
        source: u8,
        persist: bool,
    },
    Install {
        group: usize,
        name: u8,
        persist: bool,
    },
    Remove {
        group: usize,
        index: i32,
    },
    Accept {
        group: usize,
        target: usize,
        index: usize,
    },
    PutDatum {
        channel: usize,
        value: u8,
    },
    RemoveDatum {
        channel: usize,
        slot: usize,
    },
    PutJoin {
        channel: usize,
        group: usize,
    },
    InstallJoin {
        channel: usize,
        group: usize,
    },
    RemoveJoin {
        channel: usize,
        group: usize,
    },
    ReadJoins {
        channel: usize,
    },
    Clear,
    SetState,
}

enum TokenEffect {
    Neutral,
    Revision(usize),
    Generation,
}

fn group(names: &[&str]) -> Vec<String> { names.iter().map(|name| (*name).to_owned()).collect() }

fn continuation(name: u8, source: u8, persist: bool) -> TestContinuation {
    WaitingContinuation {
        patterns: Arc::new(vec![format!("pattern-{name}")]),
        continuation: Arc::new(format!("body-{name}")),
        persist,
        peeks: BTreeSet::new(),
        source: Consume {
            channel_hashes: Vec::new(),
            hash: Blake2b256Hash::from_bytes(vec![source; 32]),
            persistent: persist,
        },
    }
}

fn datum(channel: &str, value: u8) -> Datum<String> {
    Datum::create(&channel.to_owned(), format!("datum-{value}"), false)
}

fn universe() -> [Vec<String>; GROUP_COUNT] {
    [
        group(&["resting"]),
        group(&["installed"]),
        group(&["history"]),
        group(&["resting", "history"]),
    ]
}

fn channel_names() -> [String; CHANNEL_COUNT] {
    ["resting".to_owned(), "installed".to_owned(), "history".to_owned()]
}

fn populated_snapshot() -> TestSnapshot {
    let resting = group(&["resting"]);
    let installed = group(&["installed"]);
    HotStoreState {
        continuations: HashMap::from([
            (resting.clone(), vec![continuation(0, 0, false), continuation(1, 0, true)]),
            (installed.clone(), vec![continuation(21, 0, false)]),
        ]),
        installed_continuations: HashMap::from([(installed, continuation(20, 0, true))]),
        data: HashMap::from([("resting".to_owned(), vec![datum("resting", 0)])]),
        joins: HashMap::from([("resting".to_owned(), vec![resting])]),
        installed_joins: HashMap::new(),
    }
}

fn fixture_history() -> GroupHistory {
    GroupHistory {
        continuations: HashMap::from([
            (group(&["history"]), vec![continuation(30, 0, false), continuation(31, 0, true)]),
            (group(&["resting", "history"]), vec![continuation(40, 0, false)]),
        ]),
        joins: HashMap::from([
            ("history".to_owned(), vec![group(&["history"]), group(&["resting", "history"])]),
            ("resting".to_owned(), vec![group(&["resting", "history"])]),
        ]),
    }
}

fn store_from(snapshot: TestSnapshot, history: GroupHistory) -> TestStore {
    TestStore {
        state: RwLock::new(
            TestState::from_snapshot(snapshot)
                .expect("the fixture snapshot fits the multiplicity model"),
        ),
        history_cache: RwLock::new(HistoryStoreCache::default()),
        history_reader_base: Box::new(history),
    }
}

fn revision(store: &TestStore) -> ReceiveRevision {
    store
        .receive_revision()
        .expect("the in-memory hot store issues receive revisions")
}

fn is_loaded(store: &TestStore, channels: &[String]) -> bool {
    store
        .state
        .read()
        .expect("hot store state read lock")
        .continuations
        .is_loaded(channels)
}

fn store_token_is_group_epoch(state: &TestState, channels: &[String]) -> bool {
    match state
        .continuation_epochs
        .get(RhoPathV1::continuation_prefix(channels))
    {
        Some(epoch) => {
            Arc::ptr_eq(&state.continuation_store_epoch, epoch) &&
                !Arc::ptr_eq(epoch, &state.continuation_base_epoch)
        }
        None => false,
    }
}

fn store_token_is_base_epoch(state: &TestState) -> bool {
    Arc::ptr_eq(&state.continuation_store_epoch, &state.continuation_base_epoch)
}

fn assert_fresh_revision(
    store: &TestStore,
    issued: &mut Vec<ReceiveRevision>,
    channels: &[String],
) {
    let current = revision(store);
    let previous = issued.last().expect("the initial revision was recorded");
    assert!(current.shares_generation(previous), "a continuation change keeps the generation");
    assert!(
        issued.iter().all(|earlier| earlier != &current),
        "a continuation change issues a store token that was never issued before"
    );
    assert!(
        store_token_is_group_epoch(
            &store.state.read().expect("hot store state read lock"),
            channels
        ),
        "the store token is the allocation of the bumped group's fresh epoch"
    );
    issued.push(current);
}

fn assert_revision_kept(store: &TestStore, expected: &ReceiveRevision, operation: &str) {
    assert_eq!(&revision(store), expected, "{operation} must keep the receive revision");
}

fn assert_generation_token(store: &TestStore) {
    let state = store.state.read().expect("hot store state read lock");
    assert!(
        store_token_is_base_epoch(&state),
        "the store token is the fresh base epoch allocation"
    );
    assert!(state.continuation_epochs.is_empty(), "a generation bump retires every group epoch");
}

fn accepted(
    channels: &[String],
    version: Option<ContinuationVersion>,
    index: usize,
    selected: &TestContinuation,
) -> AcceptedMatch<String> {
    AcceptedMatch {
        channels: channels.to_vec(),
        continuation: Some(StoredContinuationCommit {
            address: ContinuationAddress::new(
                version,
                i32::try_from(index).expect("a fixture continuation index fits i32"),
            ),
            source: selected.source.clone(),
            persist: selected.persist,
            peeks: selected.peeks.clone(),
        }),
        datum_delta: AcceptedDatumChanges::from_removals(BagDelta::new())
            .expect("an empty removal delta is admissible"),
        required_persistent_paths: Vec::new(),
        join_channels: Vec::new(),
    }
}

fn observe(
    store: &TestStore,
    history: &GroupHistory,
    groups: &[Vec<String>; GROUP_COUNT],
) -> Vec<GroupObservation> {
    let state = store.state.read().expect("hot store state read lock");
    let mut observations = Vec::with_capacity(GROUP_COUNT);
    for channels in groups {
        let resting = match state.continuations.resting_records(channels) {
            Some(records) => records
                .iter()
                .map(|record| record.value().clone())
                .collect(),
            None => history.resting(channels),
        };
        observations.push(GroupObservation {
            resting,
            installed: state.continuations.installed(channels),
            version: state.continuation_version(channels),
        });
    }
    observations
}

fn apply_token_operation(
    store: &TestStore,
    groups: &[Vec<String>; GROUP_COUNT],
    channels: &[String; CHANNEL_COUNT],
    operation: &TokenOperation,
) -> TokenEffect {
    match *operation {
        TokenOperation::Read { group } => {
            store.get_continuations(&groups[group]);
            TokenEffect::Neutral
        }
        TokenOperation::Put {
            group,
            name,
            source,
            persist,
        } => match store
            .put_continuation(&groups[group], continuation(name, source, persist))
            .expect("the in-memory hot store answers every put")
        {
            true => TokenEffect::Revision(group),
            false => TokenEffect::Neutral,
        },
        TokenOperation::Install {
            group,
            name,
            persist,
        } => {
            store
                .install_continuation(&groups[group], continuation(name, 0, persist))
                .expect("the in-memory hot store installs every continuation");
            TokenEffect::Revision(group)
        }
        TokenOperation::Remove { group, index } => {
            match store.remove_continuation(&groups[group], index) {
                Some(()) => TokenEffect::Revision(group),
                None => TokenEffect::Neutral,
            }
        }
        TokenOperation::Accept {
            group,
            target,
            index,
        } => {
            let (version, visible) = store.get_continuations_versioned(&groups[target]);
            let Some(selected) = visible.get(index) else {
                return TokenEffect::Neutral;
            };
            let outcome = store
                .apply_accepted_match(accepted(&groups[group], version, index, selected))
                .expect("a continuation-only accepted match never fails");
            match (outcome, selected.persist) {
                (AcceptedMatchOutcome::Committed { .. }, false) => TokenEffect::Revision(group),
                (AcceptedMatchOutcome::Committed { .. }, true) |
                (AcceptedMatchOutcome::Stale, _) => TokenEffect::Neutral,
            }
        }
        TokenOperation::PutDatum { channel, value } => {
            store.put_datum(&channels[channel], datum(&channels[channel], value));
            TokenEffect::Neutral
        }
        TokenOperation::RemoveDatum { channel, slot } => {
            if let Some(index) = slot.checked_rem(store.get_data(&channels[channel]).len()) {
                store
                    .remove_datum(
                        &channels[channel],
                        i32::try_from(index).expect("a generated datum index fits i32"),
                    )
                    .expect("an in-range datum removal succeeds");
            }
            TokenEffect::Neutral
        }
        TokenOperation::PutJoin { channel, group } => {
            store
                .put_join(&channels[channel], &groups[group])
                .expect("the in-memory hot store records every join");
            TokenEffect::Neutral
        }
        TokenOperation::InstallJoin { channel, group } => {
            store
                .install_join(&channels[channel], &groups[group])
                .expect("the in-memory hot store installs every join");
            TokenEffect::Neutral
        }
        TokenOperation::RemoveJoin { channel, group } => {
            store
                .remove_join(&channels[channel], &groups[group])
                .expect("the in-memory hot store answers every join removal");
            TokenEffect::Neutral
        }
        TokenOperation::ReadJoins { channel } => {
            store.get_joins(&channels[channel]);
            TokenEffect::Neutral
        }
        TokenOperation::Clear => {
            store.clear();
            TokenEffect::Generation
        }
        TokenOperation::SetState => {
            store.set_state(store.snapshot());
            TokenEffect::Generation
        }
    }
}

fn token_operation() -> impl Strategy<Value = TokenOperation> {
    let group = 0..GROUP_COUNT;
    let channel = 0..CHANNEL_COUNT;
    prop_oneof![
        3 => group.clone().prop_map(|group| TokenOperation::Read { group }),
        6 => (
            group.clone(),
            prop::sample::select(vec![0_u8, 1, 2, 21, 30, 31, 40]),
            0_u8..2,
            any::<bool>(),
        )
            .prop_map(|(group, name, source, persist)| TokenOperation::Put {
                group,
                name,
                source,
                persist,
            }),
        2 => (group.clone(), prop::sample::select(vec![20_u8, 22]), any::<bool>())
            .prop_map(|(group, name, persist)| TokenOperation::Install {
                group,
                name,
                persist,
            }),
        4 => (group.clone(), -1_i32..4)
            .prop_map(|(group, index)| TokenOperation::Remove { group, index }),
        4 => (group.clone(), group.clone(), 0_usize..4).prop_map(|(group, target, index)| {
            TokenOperation::Accept {
                group,
                target,
                index,
            }
        }),
        2 => (channel.clone(), 0_u8..3)
            .prop_map(|(channel, value)| TokenOperation::PutDatum { channel, value }),
        2 => (channel.clone(), 0_usize..4)
            .prop_map(|(channel, slot)| TokenOperation::RemoveDatum { channel, slot }),
        1 => (channel.clone(), group.clone())
            .prop_map(|(channel, group)| TokenOperation::PutJoin { channel, group }),
        1 => (channel.clone(), group.clone())
            .prop_map(|(channel, group)| TokenOperation::InstallJoin { channel, group }),
        1 => (channel.clone(), group)
            .prop_map(|(channel, group)| TokenOperation::RemoveJoin { channel, group }),
        1 => channel.prop_map(|channel| TokenOperation::ReadJoins { channel }),
        1 => Just(TokenOperation::Clear),
        1 => Just(TokenOperation::SetState),
    ]
}

#[test]
fn fresh_states_issue_their_base_epoch_as_the_store_token() {
    let default_state = TestState::default();
    assert!(store_token_is_base_epoch(&default_state));
    assert!(default_state.continuation_epochs.is_empty());
    let restored = TestState::from_snapshot(populated_snapshot())
        .expect("the fixture snapshot fits the multiplicity model");
    assert!(store_token_is_base_epoch(&restored));
    assert!(restored.continuation_epochs.is_empty());

    let (default_token, restored_token) =
        (default_state.receive_revision(), restored.receive_revision());
    assert_eq!(default_token, default_state.receive_revision());
    assert_ne!(default_token, restored_token);
    assert!(!default_token.shares_generation(&restored_token));

    let store = store_from(populated_snapshot(), fixture_history());
    let state_token = store
        .state
        .read()
        .expect("hot store state read lock")
        .receive_revision();
    assert_eq!(revision(&store), state_token);
}

#[test]
fn every_continuation_change_issues_the_bumped_group_epoch_as_a_fresh_store_token() {
    let store = store_from(populated_snapshot(), fixture_history());
    let resting = group(&["resting"]);
    let installed = group(&["installed"]);
    let history_group = group(&["history"]);
    let fresh = group(&["fresh"]);
    let mut issued = Vec::with_capacity(8);
    issued.push(revision(&store));

    assert_eq!(store.put_continuation(&resting, continuation(2, 0, false)), Some(true));
    assert_fresh_revision(&store, &mut issued, &resting);

    assert!(!is_loaded(&store, &history_group));
    assert_eq!(store.put_continuation(&history_group, continuation(32, 0, false)), Some(true));
    assert_fresh_revision(&store, &mut issued, &history_group);

    assert_eq!(store.install_continuation(&installed, continuation(22, 0, true)), Some(()));
    assert_fresh_revision(&store, &mut issued, &installed);

    assert_eq!(store.install_continuation(&fresh, continuation(23, 0, true)), Some(()));
    assert_fresh_revision(&store, &mut issued, &fresh);

    assert_eq!(store.remove_continuation(&installed, 1), Some(()));
    assert_fresh_revision(&store, &mut issued, &installed);

    assert_eq!(store.remove_continuation(&resting, 0), Some(()));
    assert_fresh_revision(&store, &mut issued, &resting);

    let (version, visible) = store.get_continuations_versioned(&history_group);
    let index = visible
        .iter()
        .position(|candidate| !candidate.persist)
        .expect("the history group holds an ordinary continuation");
    assert!(matches!(
        store
            .apply_accepted_match(accepted(&history_group, version, index, &visible[index]))
            .expect("a continuation-only accepted match never fails"),
        AcceptedMatchOutcome::Committed { .. }
    ));
    assert_fresh_revision(&store, &mut issued, &history_group);
}

#[test]
fn history_loads_duplicates_refusals_datum_and_join_operations_keep_the_receive_revision() {
    let store = store_from(populated_snapshot(), fixture_history());
    let resting = group(&["resting"]);
    let installed = group(&["installed"]);
    let history_group = group(&["history"]);
    let joined = group(&["resting", "history"]);
    let absent = group(&["absent"]);
    let kept = revision(&store);

    assert!(!is_loaded(&store, &history_group));
    assert_eq!(store.get_continuations(&history_group), vec![
        continuation(30, 0, false),
        continuation(31, 0, true)
    ]);
    assert!(is_loaded(&store, &history_group));
    assert_revision_kept(&store, &kept, "a history load");

    let (_, visible) = store.get_continuations_versioned(&absent);
    assert!(visible.is_empty());
    assert!(is_loaded(&store, &absent));
    assert_revision_kept(&store, &kept, "an empty history load");

    assert_eq!(store.put_continuation(&resting, continuation(0, 0, false)), Some(false));
    assert_revision_kept(&store, &kept, "a duplicate put");

    assert_eq!(store.put_continuation(&resting, continuation(0, 9, false)), Some(false));
    assert_eq!(store.get_continuations(&resting)[0].source, continuation(0, 0, false).source);
    assert_revision_kept(&store, &kept, "an alias put");

    assert!(!is_loaded(&store, &joined));
    assert_eq!(store.put_continuation(&joined, continuation(40, 0, false)), Some(false));
    assert!(is_loaded(&store, &joined));
    assert_revision_kept(&store, &kept, "a put that duplicates a history record");

    assert_eq!(store.remove_continuation(&installed, 0), None);
    assert_revision_kept(&store, &kept, "a refused removal of the installed continuation");
    assert_eq!(store.remove_continuation(&resting, 7), None);
    assert_eq!(store.remove_continuation(&resting, -1), None);
    assert_revision_kept(&store, &kept, "out-of-range continuation removals");

    let datum_channel = "resting".to_owned();
    store.put_datum(&datum_channel, datum("resting", 1));
    store
        .remove_datum(&datum_channel, 0)
        .expect("an in-range datum removal succeeds");
    assert!(store.remove_datum(&datum_channel, 9).is_err());
    assert_eq!(store.get_data(&datum_channel).len(), 1);
    assert_revision_kept(&store, &kept, "datum operations");

    let join_channel = "history".to_owned();
    assert_eq!(store.get_joins(&join_channel).len(), 2);
    store
        .put_join(&datum_channel, &joined)
        .expect("the in-memory hot store records every join");
    store
        .install_join(&join_channel, &history_group)
        .expect("the in-memory hot store installs every join");
    store
        .remove_join(&datum_channel, &resting)
        .expect("the in-memory hot store answers every join removal");
    store
        .remove_join(&datum_channel, &absent)
        .expect("the in-memory hot store answers every join removal");
    assert_revision_kept(&store, &kept, "join operations");

    let (version, visible) = store.get_continuations_versioned(&resting);
    assert!(matches!(
        store
            .apply_accepted_match(accepted(&history_group, version, 0, &visible[0]))
            .expect("a continuation-only accepted match never fails"),
        AcceptedMatchOutcome::Stale
    ));
    assert_revision_kept(&store, &kept, "a stale accepted match");

    let (version, visible) = store.get_continuations_versioned(&resting);
    let index = visible
        .iter()
        .position(|candidate| candidate.persist)
        .expect("the resting group holds a persistent continuation");
    assert!(matches!(
        store
            .apply_accepted_match(accepted(&resting, version, index, &visible[index]))
            .expect("a continuation-only accepted match never fails"),
        AcceptedMatchOutcome::Committed { .. }
    ));
    assert_eq!(store.get_continuations(&resting), visible);
    assert_revision_kept(&store, &kept, "a committed match that keeps a persistent continuation");

    let (version, visible) = store.get_continuations_versioned(&installed);
    assert!(matches!(
        store
            .apply_accepted_match(accepted(&installed, version, 0, &visible[0]))
            .expect("a continuation-only accepted match never fails"),
        AcceptedMatchOutcome::Committed { .. }
    ));
    assert_eq!(store.get_continuations(&installed), visible);
    assert_revision_kept(&store, &kept, "a committed match on the installed continuation");
}

#[test]
fn clear_and_set_state_issue_a_fresh_generation_whose_base_epoch_is_the_store_token() {
    let store = store_from(populated_snapshot(), fixture_history());
    let resting = group(&["resting"]);
    assert_eq!(store.put_continuation(&resting, continuation(2, 0, false)), Some(true));
    let bumped = revision(&store);
    assert!(!store_token_is_base_epoch(&store.state.read().expect("hot store state read lock")));

    store.clear();
    let cleared = revision(&store);
    assert_ne!(cleared, bumped);
    assert!(!cleared.shares_generation(&bumped));
    assert_generation_token(&store);

    store.set_state(populated_snapshot());
    let restored = revision(&store);
    assert!(!restored.shares_generation(&cleared));
    assert!(!restored.shares_generation(&bumped));
    assert_generation_token(&store);

    store.set_state(store.snapshot());
    let reloaded = revision(&store);
    assert!(!reloaded.shares_generation(&restored));
    assert_generation_token(&store);

    assert_eq!(store.put_continuation(&resting, continuation(3, 0, false)), Some(true));
    let next = revision(&store);
    assert!(next.shares_generation(&reloaded));
    assert_ne!(next, reloaded);
}

proptest! {
    #[test]
    fn store_token_equality_implies_equal_receive_state_across_generated_traces(
        operations in prop::collection::vec(token_operation(), 1..48),
    ) {
        let groups = universe();
        let channels = channel_names();
        let history = fixture_history();
        let store = store_from(populated_snapshot(), fixture_history());
        let mut trace = Vec::with_capacity(operations.len().saturating_add(1));
        trace.push((revision(&store), observe(&store, &history, &groups)));
        for operation in &operations {
            let effect = apply_token_operation(&store, &groups, &channels, operation);
            let token = revision(&store);
            let observation = observe(&store, &history, &groups);
            let (previous, _) = trace.last().expect("the trace starts at the initial state");
            match effect {
                TokenEffect::Neutral => {
                    prop_assert_eq!(&token, previous);
                }
                TokenEffect::Revision(group) => {
                    prop_assert!(token.shares_generation(previous));
                    prop_assert!(trace.iter().all(|(issued, _)| issued != &token));
                    prop_assert!(store_token_is_group_epoch(
                        &store.state.read().expect("hot store state read lock"),
                        &groups[group],
                    ));
                }
                TokenEffect::Generation => {
                    prop_assert!(trace.iter().all(|(issued, _)| !issued.shares_generation(&token)));
                    let state = store.state.read().expect("hot store state read lock");
                    prop_assert!(store_token_is_base_epoch(&state));
                    prop_assert!(state.continuation_epochs.is_empty());
                }
            }
            for (_, issued_observation) in trace.iter().filter(|(issued, _)| issued == &token) {
                prop_assert_eq!(issued_observation, &observation);
            }
            trace.push((token, observation));
        }
    }
}
