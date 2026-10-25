use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{BindPattern, Expr, ListParWithRandom, Par, TaggedContinuation, Var};
use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{
    new_boundvar_par, new_elist_expr, new_elist_par, new_eset_par, new_freevar_par,
    new_freevar_var, new_gint_par, new_gstring_par, new_wildcard_par,
};
use proptest::prelude::*;
use rholang::rust::interpreter::matcher::r#match::Matcher;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::internal::Row;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::replay_rspace::ReplayRSpace;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{ContResult, ISpace, RSpaceResult};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{Event, COMM};
use rspace_plus_plus::rspace::trace::Log;

type RhoSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type RhoReplay = ReplayRSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;
type Selection = (
    ContResult<Par, BindPattern, TaggedContinuation>,
    Vec<RSpaceResult<Par, ListParWithRandom>>,
);
type State = HashMap<Vec<Par>, Row<BindPattern, ListParWithRandom, TaggedContinuation>>;

#[derive(Clone, Default)]
struct FullScanMatcher;

impl Match<BindPattern, ListParWithRandom, TaggedContinuation> for FullScanMatcher {
    fn get(&self, pattern: &BindPattern, datum: &ListParWithRandom) -> Option<ListParWithRandom> {
        Matcher.get(pattern, datum)
    }

    fn check_commit(
        &self,
        continuation: &TaggedContinuation,
        matched: &[&ListParWithRandom],
    ) -> bool {
        Matcher.check_commit(continuation, matched)
    }
}

#[derive(Clone, Copy)]
enum Direction {
    ConsumeLast,
    ProduceLast,
}

struct Case {
    name: &'static str,
    channels: Vec<Par>,
    patterns: Vec<BindPattern>,
    background: Vec<(Par, ListParWithRandom, bool)>,
    arrival: (Par, ListParWithRandom),
    continuation: TaggedContinuation,
    peeks: BTreeSet<i32>,
    expected_bindings: Vec<Vec<Par>>,
    remaining: Vec<(Par, Vec<(ListParWithRandom, bool)>)>,
}

struct Observation {
    selection: Selection,
    state: State,
    root: Blake2b256Hash,
    log: Log,
}

fn integer(value: i64) -> Par { new_gint_par(value, Vec::new(), false) }

fn channel(name: &str) -> Par { new_gstring_par(name.to_owned(), Vec::new(), false) }

fn datum(pars: Vec<Par>) -> ListParWithRandom {
    ListParWithRandom {
        pars,
        ..ListParWithRandom::default()
    }
}

fn bind(patterns: Vec<Par>, remainder: Option<Var>, free_count: i32) -> BindPattern {
    BindPattern {
        patterns,
        remainder,
        free_count,
    }
}

fn guarded_continuation() -> TaggedContinuation {
    TaggedContinuation {
        guard: Some(Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELteBody(models::rhoapi::ELte {
                p1: Some(new_boundvar_par(
                    0,
                    models::create_bit_vector(&vec![0]),
                    false,
                )),
                p2: Some(integer(45)),
            })),
        }])),
        ..TaggedContinuation::default()
    }
}

fn guard_case(accepted: i64, rejected: i64, accepted_seed: u8, rejected_seed: u8) -> Case {
    let guard_channel = channel("guard");
    let mut guard_decoy = datum(vec![integer(rejected)]);
    guard_decoy.random_state = vec![rejected_seed];
    let mut guard_arrival = datum(vec![integer(accepted)]);
    guard_arrival.random_state = vec![accepted_seed];
    Case {
        name: "guard",
        channels: vec![guard_channel.clone()],
        patterns: vec![bind(vec![new_freevar_par(0, Vec::new())], None, 1)],
        background: vec![(guard_channel.clone(), guard_decoy.clone(), false)],
        arrival: (guard_channel.clone(), guard_arrival),
        continuation: guarded_continuation(),
        peeks: BTreeSet::new(),
        expected_bindings: vec![vec![integer(accepted)]],
        remaining: vec![(guard_channel, vec![(guard_decoy, false)])],
    }
}

fn cases() -> Vec<Case> {
    let ordered_channel = channel("ordered");
    let ordered_decoy = datum(vec![integer(8)]);
    let ordered_arrival = datum(vec![new_elist_par(
        vec![integer(11), integer(7)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )]);
    let ac_channel = channel("ac");
    let ac_decoy = datum(vec![integer(8)]);
    let ac_arrival = datum(vec![new_eset_par(
        vec![integer(7), integer(11)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )]);
    let remainder_channel = channel("remainder");
    let remainder_arrival = datum(vec![integer(11), integer(7), integer(12), integer(13)]);
    let remainder_binding = vector_par(Vec::new(), false).with_exprs(vec![new_elist_expr(
        vec![integer(12), integer(13)],
        Vec::new(),
        false,
        None,
    )]);
    let peek_left = channel("peek-left");
    let peek_right = channel("peek-right");
    let peek_witness = datum(vec![integer(30)]);
    let peek_arrival = datum(vec![integer(32)]);
    let persist_left = channel("persist-left");
    let persist_right = channel("persist-right");
    let persist_witness = datum(vec![integer(30)]);
    let persist_arrival = datum(vec![integer(31)]);
    let repeated_channel = channel("repeated");
    let repeated_witness = datum(vec![integer(7)]);
    let repeated_arrival = datum(vec![integer(11)]);
    vec![
        Case {
            name: "ordered",
            channels: vec![ordered_channel.clone()],
            patterns: vec![bind(
                vec![new_elist_par(
                    vec![new_freevar_par(0, Vec::new()), integer(7)],
                    Vec::new(),
                    true,
                    None,
                    Vec::new(),
                    true,
                )],
                None,
                1,
            )],
            background: vec![(ordered_channel.clone(), ordered_decoy.clone(), false)],
            arrival: (ordered_channel.clone(), ordered_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            expected_bindings: vec![vec![integer(11)]],
            remaining: vec![(ordered_channel, vec![(ordered_decoy, false)])],
        },
        Case {
            name: "ac",
            channels: vec![ac_channel.clone()],
            patterns: vec![bind(
                vec![new_eset_par(
                    vec![integer(7), new_freevar_par(0, Vec::new())],
                    Vec::new(),
                    true,
                    None,
                    Vec::new(),
                    true,
                )],
                None,
                1,
            )],
            background: vec![(ac_channel.clone(), ac_decoy.clone(), false)],
            arrival: (ac_channel.clone(), ac_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            expected_bindings: vec![vec![integer(11)]],
            remaining: vec![(ac_channel, vec![(ac_decoy, false)])],
        },
        Case {
            name: "remainder",
            channels: vec![remainder_channel.clone()],
            patterns: vec![bind(
                vec![new_wildcard_par(Vec::new(), true), integer(7)],
                Some(new_freevar_var(0)),
                1,
            )],
            background: Vec::new(),
            arrival: (remainder_channel.clone(), remainder_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            expected_bindings: vec![vec![remainder_binding]],
            remaining: vec![(remainder_channel, Vec::new())],
        },
        guard_case(42, 55, 0, 0),
        Case {
            name: "peek",
            channels: vec![peek_left.clone(), peek_right.clone()],
            patterns: vec![
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
                bind(vec![integer(32)], None, 0),
            ],
            background: vec![(peek_left.clone(), peek_witness.clone(), false)],
            arrival: (peek_right.clone(), peek_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::from([0]),
            expected_bindings: vec![vec![integer(30)], Vec::new()],
            remaining: vec![(peek_left, Vec::new()), (peek_right, Vec::new())],
        },
        Case {
            name: "persistent",
            channels: vec![persist_left.clone(), persist_right.clone()],
            patterns: vec![
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
                bind(vec![integer(31)], None, 0),
            ],
            background: vec![(persist_left.clone(), persist_witness.clone(), true)],
            arrival: (persist_right.clone(), persist_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            expected_bindings: vec![vec![integer(30)], Vec::new()],
            remaining: vec![
                (persist_left, vec![(persist_witness, true)]),
                (persist_right, Vec::new()),
            ],
        },
        Case {
            name: "repeated-channel",
            channels: vec![repeated_channel.clone(), repeated_channel.clone()],
            patterns: vec![
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
                bind(vec![integer(11)], None, 0),
            ],
            background: vec![(repeated_channel.clone(), repeated_witness, false)],
            arrival: (repeated_channel.clone(), repeated_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            expected_bindings: vec![vec![integer(7)], Vec::new()],
            remaining: vec![(repeated_channel, Vec::new())],
        },
    ]
}

async fn pair<M>(matcher: M) -> (RhoSpace, RhoReplay)
where M: Match<BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    let mut manager = InMemoryStoreManager::new();
    let stores = manager
        .r_space_stores()
        .await
        .expect("in-memory RSpace stores");
    RSpace::create_with_replay(stores, Arc::new(Box::new(matcher))).expect("play/replay pair")
}

async fn execute<S>(space: &S, case: &Case, direction: Direction) -> Selection
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    for (channel, datum, persistent) in &case.background {
        assert!(
            space
                .produce(channel.clone(), datum.clone(), *persistent)
                .await
                .unwrap_or_else(|error| panic!("{}: background produce: {error}", case.name))
                .is_none(),
            "{}: background cannot fire",
            case.name
        );
    }
    match direction {
        Direction::ConsumeLast => {
            assert!(
                space
                    .produce(case.arrival.0.clone(), case.arrival.1.clone(), false)
                    .await
                    .unwrap_or_else(|error| panic!("{}: arrival produce: {error}", case.name))
                    .is_none(),
                "{}: datum must rest before the receive arrives",
                case.name
            );
            space
                .consume(
                    case.channels.clone(),
                    case.patterns.clone(),
                    case.continuation.clone(),
                    false,
                    case.peeks.clone(),
                )
                .await
                .unwrap_or_else(|error| panic!("{}: consume: {error}", case.name))
                .unwrap_or_else(|| panic!("{}: consume direction must fire", case.name))
        }
        Direction::ProduceLast => {
            assert!(
                space
                    .consume(
                        case.channels.clone(),
                        case.patterns.clone(),
                        case.continuation.clone(),
                        false,
                        case.peeks.clone(),
                    )
                    .await
                    .unwrap_or_else(|error| panic!("{}: waiting consume: {error}", case.name))
                    .is_none(),
                "{}: receive must wait for arrival",
                case.name
            );
            let (continuation, data, _) = space
                .produce(case.arrival.0.clone(), case.arrival.1.clone(), false)
                .await
                .unwrap_or_else(|error| panic!("{}: completing produce: {error}", case.name))
                .unwrap_or_else(|| panic!("{}: produce direction must fire", case.name));
            (continuation, data)
        }
    }
}

async fn observe<M>(matcher: M, case: &Case, direction: Direction) -> Observation
where M: Match<BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    let (play, replay) = pair(matcher).await;
    let initial = play.create_checkpoint().await.expect("initial checkpoint");
    let selection = execute(&play, case, direction).await;
    assert_eq!(
        selection.1.len(),
        case.channels.len(),
        "{}: bind count",
        case.name
    );
    assert_eq!(
        selection
            .1
            .iter()
            .map(|result| result.matched_datum.pars.clone())
            .collect::<Vec<_>>(),
        case.expected_bindings,
        "{}: exact FreeMap projection in bind order",
        case.name
    );
    for (channel, expected) in &case.remaining {
        let actual = play
            .get_data(channel)
            .await
            .into_iter()
            .map(|datum| ((*datum.a).clone(), datum.persist))
            .collect::<Vec<_>>();
        assert_eq!(actual, *expected, "{}: exact remaining data", case.name);
    }
    let state = play.to_map().await;
    let point = play.create_checkpoint().await.expect("play checkpoint");
    let comms = point
        .log
        .iter()
        .filter(|event| matches!(event, Event::Comm(_)))
        .count();
    assert_eq!(comms, 1, "{}: exactly one COMM", case.name);
    assert!(matches!(point.log.first(), Some(Event::Comm(_))));

    replay
        .rig_and_reset(initial.root, point.log.clone())
        .await
        .expect("rig replay");
    let replay_selection = execute(&replay, case, direction).await;
    assert_eq!(
        replay_selection, selection,
        "{}: exact replay binding",
        case.name
    );
    assert_eq!(
        replay.to_map().await,
        state,
        "{}: replay post-state",
        case.name
    );
    replay
        .check_replay_data()
        .await
        .expect("replay COMM exhausted");
    let replay_point = replay.create_checkpoint().await.expect("replay checkpoint");
    assert_eq!(replay_point.root, point.root, "{}: replay root", case.name);

    Observation {
        selection,
        state,
        root: point.root,
        log: point.log,
    }
}

fn io_multiset(log: &Log) -> HashMap<Event, usize> {
    let mut counts = HashMap::new();
    for event in log {
        if matches!(event, Event::IoEvent(_)) {
            *counts.entry(event.clone()).or_insert(0) += 1;
        }
    }
    counts
}

fn comm_trace(log: &Log) -> Vec<COMM> {
    log.iter()
        .filter_map(|event| match event {
            Event::Comm(comm) => Some(comm.clone()),
            Event::IoEvent(_) => None,
        })
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn both_matching_directions_preserve_bindings_comm_state_root_and_replay() {
    let cases = cases();
    assert_eq!(cases.len(), 7);
    for case in &cases {
        let consume = observe(Matcher, case, Direction::ConsumeLast).await;
        let produce = observe(Matcher, case, Direction::ProduceLast).await;
        let control_consume = observe(FullScanMatcher, case, Direction::ConsumeLast).await;
        let control_produce = observe(FullScanMatcher, case, Direction::ProduceLast).await;

        assert_eq!(
            consume.selection, produce.selection,
            "{}: direction bindings",
            case.name
        );
        assert_eq!(
            consume.state, produce.state,
            "{}: direction post-state",
            case.name
        );
        assert_eq!(consume.root, produce.root, "{}: direction root", case.name);
        assert_eq!(
            comm_trace(&consume.log),
            comm_trace(&produce.log),
            "{}: COMM trace",
            case.name
        );
        assert_eq!(
            io_multiset(&consume.log),
            io_multiset(&produce.log),
            "{}: IO events",
            case.name
        );
        assert_eq!(
            consume.selection, control_consume.selection,
            "{}: consume oracle",
            case.name
        );
        assert_eq!(
            produce.selection, control_produce.selection,
            "{}: produce oracle",
            case.name
        );
        assert_eq!(
            consume.log, control_consume.log,
            "{}: consume event oracle",
            case.name
        );
        assert_eq!(
            produce.log, control_produce.log,
            "{}: produce event oracle",
            case.name
        );
        assert_eq!(
            consume.root, control_consume.root,
            "{}: consume root oracle",
            case.name
        );
        assert_eq!(
            produce.root, control_produce.root,
            "{}: produce root oracle",
            case.name
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    #[test]
    fn replay_subsequence_preserves_first_admissible_binding_in_both_directions(
        accepted in -20i64..46,
        rejected in 46i64..120,
        accepted_seed in any::<u8>(),
        rejected_seed in any::<u8>(),
    ) {
        let case = guard_case(accepted, rejected, accepted_seed, rejected_seed);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("bounded property runtime");
        runtime.block_on(async {
            let consume = observe(Matcher, &case, Direction::ConsumeLast).await;
            let produce = observe(Matcher, &case, Direction::ProduceLast).await;
            prop_assert_eq!(&consume.selection, &produce.selection);
            prop_assert_eq!(&consume.state, &produce.state);
            prop_assert_eq!(consume.root, produce.root);
            prop_assert_eq!(comm_trace(&consume.log), comm_trace(&produce.log));
            prop_assert_eq!(io_multiset(&consume.log), io_multiset(&produce.log));
            Ok::<(), TestCaseError>(())
        })?;
    }
}
