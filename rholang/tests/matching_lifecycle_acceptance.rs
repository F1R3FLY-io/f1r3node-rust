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
use rspace_plus_plus::rspace::internal::{Datum, Row, WaitingContinuation};
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
    outcomes: Vec<Outcome>,
}

struct Outcome {
    bindings: Vec<Vec<Par>>,
    remaining: Vec<(Par, Vec<(ListParWithRandom, bool)>)>,
}

#[derive(Debug, PartialEq)]
struct CandidateOrder {
    state: State,
    data: Vec<Vec<Datum<ListParWithRandom>>>,
    continuations: Vec<WaitingContinuation<BindPattern, TaggedContinuation>>,
    joins: Vec<Vec<Vec<Par>>>,
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

fn one_variable_twice() -> Vec<Par> {
    vec![
        new_freevar_par(0, Vec::new()),
        new_freevar_par(0, Vec::new()),
    ]
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
        outcomes: vec![Outcome {
            bindings: vec![vec![integer(accepted)]],
            remaining: vec![(guard_channel, vec![(guard_decoy, false)])],
        }],
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
    let nonlinear_equal_channel = channel("nonlinear-equal");
    let nonlinear_equal_decoy = datum(vec![integer(7)]);
    let nonlinear_conflicting_channel = channel("nonlinear-conflicting");
    let nonlinear_conflicting_decoy = datum(vec![integer(7)]);
    let nonlinear_nested_channel = channel("nonlinear-nested");
    let nonlinear_nested_decoy = datum(vec![integer(8)]);
    let nonlinear_nested_arrival = datum(vec![new_elist_par(
        vec![integer(13), integer(14)],
        Vec::new(),
        false,
        None,
        Vec::new(),
        false,
    )]);
    let choice_left = channel("choice-left");
    let choice_right = channel("choice-right");
    let choice_first = datum(vec![integer(21)]);
    let choice_second = datum(vec![integer(22)]);
    let repeated_choice_channel = channel("repeated-choice");
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
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(11)]],
                remaining: vec![(ordered_channel, vec![(ordered_decoy, false)])],
            }],
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
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(11)]],
                remaining: vec![(ac_channel, vec![(ac_decoy, false)])],
            }],
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
            outcomes: vec![Outcome {
                bindings: vec![vec![remainder_binding]],
                remaining: vec![(remainder_channel, Vec::new())],
            }],
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
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(30)], Vec::new()],
                remaining: vec![(peek_left, Vec::new()), (peek_right, Vec::new())],
            }],
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
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(30)], Vec::new()],
                remaining: vec![
                    (persist_left, vec![(persist_witness, true)]),
                    (persist_right, Vec::new()),
                ],
            }],
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
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(7)], Vec::new()],
                remaining: vec![(repeated_channel, Vec::new())],
            }],
        },
        Case {
            name: "nonlinear-equal",
            channels: vec![nonlinear_equal_channel.clone()],
            patterns: vec![bind(one_variable_twice(), None, 1)],
            background: vec![(
                nonlinear_equal_channel.clone(),
                nonlinear_equal_decoy.clone(),
                false,
            )],
            arrival: (
                nonlinear_equal_channel.clone(),
                datum(vec![integer(11), integer(11)]),
            ),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(11)]],
                remaining: vec![(nonlinear_equal_channel, vec![(
                    nonlinear_equal_decoy,
                    false,
                )])],
            }],
        },
        Case {
            name: "nonlinear-conflicting",
            channels: vec![nonlinear_conflicting_channel.clone()],
            patterns: vec![bind(one_variable_twice(), None, 1)],
            background: vec![(
                nonlinear_conflicting_channel.clone(),
                nonlinear_conflicting_decoy.clone(),
                false,
            )],
            arrival: (
                nonlinear_conflicting_channel.clone(),
                datum(vec![integer(11), integer(12)]),
            ),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(12)]],
                remaining: vec![(nonlinear_conflicting_channel, vec![(
                    nonlinear_conflicting_decoy,
                    false,
                )])],
            }],
        },
        Case {
            name: "nonlinear-nested",
            channels: vec![nonlinear_nested_channel.clone()],
            patterns: vec![bind(
                vec![new_elist_par(
                    one_variable_twice(),
                    Vec::new(),
                    true,
                    None,
                    Vec::new(),
                    true,
                )],
                None,
                1,
            )],
            background: vec![(
                nonlinear_nested_channel.clone(),
                nonlinear_nested_decoy.clone(),
                false,
            )],
            arrival: (nonlinear_nested_channel.clone(), nonlinear_nested_arrival),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            outcomes: vec![Outcome {
                bindings: vec![vec![integer(14)]],
                remaining: vec![(nonlinear_nested_channel, vec![(
                    nonlinear_nested_decoy,
                    false,
                )])],
            }],
        },
        Case {
            name: "join-choice",
            channels: vec![choice_left.clone(), choice_right.clone()],
            patterns: vec![
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
            ],
            background: vec![
                (choice_left.clone(), choice_first.clone(), false),
                (choice_left.clone(), choice_second.clone(), false),
            ],
            arrival: (choice_right.clone(), datum(vec![integer(23)])),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            outcomes: vec![
                Outcome {
                    bindings: vec![vec![integer(21)], vec![integer(23)]],
                    remaining: vec![
                        (choice_left.clone(), vec![(choice_second.clone(), false)]),
                        (choice_right.clone(), Vec::new()),
                    ],
                },
                Outcome {
                    bindings: vec![vec![integer(22)], vec![integer(23)]],
                    remaining: vec![
                        (choice_left, vec![(choice_first, false)]),
                        (choice_right, Vec::new()),
                    ],
                },
            ],
        },
        Case {
            name: "repeated-channel-choice",
            channels: vec![
                repeated_choice_channel.clone(),
                repeated_choice_channel.clone(),
            ],
            patterns: vec![
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
                bind(vec![new_freevar_par(0, Vec::new())], None, 1),
            ],
            background: vec![(
                repeated_choice_channel.clone(),
                datum(vec![integer(31)]),
                false,
            )],
            arrival: (repeated_choice_channel.clone(), datum(vec![integer(32)])),
            continuation: TaggedContinuation::default(),
            peeks: BTreeSet::new(),
            outcomes: vec![
                Outcome {
                    bindings: vec![vec![integer(31)], vec![integer(32)]],
                    remaining: vec![(repeated_choice_channel.clone(), Vec::new())],
                },
                Outcome {
                    bindings: vec![vec![integer(32)], vec![integer(31)]],
                    remaining: vec![(repeated_choice_channel, Vec::new())],
                },
            ],
        },
        dynamic_root_case(
            "new-ground",
            "new x in { 1 } | _",
            "new a in { 2 } | new b in { 3 }",
            "new a in { 1 } | new b in { 2 }",
            &[],
        ),
        dynamic_root_case(
            "new-open",
            "new x(`rho:id:beta`) in { y } | _",
            "new a, b in { 3 } | new c, d in { 4 }",
            "new a(`rho:id:alpha`) in { 1 } | new b(`rho:id:beta`) in { 2 }",
            &["1"],
        ),
        dynamic_root_case(
            "new-binder",
            "new x in { x!(y) } | _",
            r#"new a in { @"out"!(3) } | new b in { @"out"!(4) }"#,
            r#"new a in { a!(1) } | new b in { @"out"!(2) }"#,
            &["1"],
        ),
        dynamic_root_case(
            "new-remainder",
            "new x in { y } | rest",
            "new a, b in { 3 } | new c, d in { 4 }",
            "new a in { 1 } | new b, c in { 2 }",
            &["1", "new b, c in { 2 }"],
        ),
        dynamic_root_case(
            "receive-ground",
            r#"for (@a <- @"r") { 5 } | _"#,
            r#"for (@a <- @"r") { 7 } | for (@a <- @"s") { 5 }"#,
            r#"for (@a <- @"r") { 5 } | for (@a <- @"r") { 6 }"#,
            &[],
        ),
        dynamic_root_case(
            "receive-open",
            r#"for (@a <- @"r") { y } | _"#,
            r#"for (@b, @c <- @"r") { 3 } | for (@b <= @"r") { 4 }"#,
            r#"for (@b <<- @"r") { 1 } | for (@b <- @"s") { 2 }"#,
            &["1"],
        ),
        dynamic_root_case(
            "receive-binder",
            r#"for (@a <- @"r") { @"out"!(a, y) } | _"#,
            r#"for (@b <- @"r") { @"out"!(2, 3) } | for (@b <- @"q") { @"out"!(b, 4) }"#,
            r#"for (@b <- @"r") { @"out"!(b, 1) } | for (@b <- @"r") { @"out"!(1, b) }"#,
            &["1"],
        ),
        dynamic_root_case(
            "receive-remainder",
            r#"for (@a <- @"r") { 5 | 6 } | rest"#,
            r#"for (@a <- @"r") { 5 } | for (@a <- @"r") { 6 }"#,
            r#"for (@a <- @"r") { 5 | 6 } | for (@a <- @"r") { 5 } | @"side"!(9)"#,
            &[r#"for (@a <- @"r") { 5 } | @"side"!(9)"#],
        ),
        dynamic_root_case(
            "match-ground",
            "match 5 { 5 => 6 } | _",
            "match 4 { 5 => 6 } | match 5 { 5 => 8 }",
            "match 5 { 5 => 6 } | match 5 { 5 => 7 }",
            &[],
        ),
        dynamic_root_case(
            "match-open",
            "match x { 5 => 6 } | _",
            "match 5 { 4 => 6 } | match 6 { 5 => 7 }",
            "match 5 { 5 where true => 6 } | match 7 { 4 => 6 }",
            &["5"],
        ),
        dynamic_root_case(
            "match-binder",
            "match 5 { a => [a, y] } | _",
            "match 5 { [b] => [b, 3] } | match 6 { b => [b, 4] }",
            "match 5 { b => [b, 1] } | match 5 { b => [2, b] }",
            &["1"],
        ),
        dynamic_root_case(
            "match-remainder",
            "match x { 5 => { 6 | 7 } } | rest",
            "match 5 { 4 => { 6 | 7 } } | match 6 { 5 => { 6 | 8 } }",
            "match 5 { 5 => { 6 | 7 } } | match 8 { 4 => { 6 | 7 } }",
            &["5", "match 8 { 4 => { 6 | 7 } }"],
        ),
        dynamic_root_case(
            "receive-bind-ground",
            r#"for (@a <- @"p" & @b <- @"q") { y } | _"#,
            r#"for (@c <- @"p" & @d, @e <- @"q") { 3 } | for (@c <- @"p") { 4 }"#,
            r#"for (@c <- @"p" & @d <- @"q") { 1 } | for (@c <- @"p" & @d <- @"r") { 2 }"#,
            &["1"],
        ),
        dynamic_root_case(
            "receive-bind-open",
            r#"for (@a <- x & @b <- @"q") { 5 } | _"#,
            r#"for (@c <- @"p" & @d <- @"r") { 5 } | for (@c, @e <- @"p" & @d <- @"q") { 5 }"#,
            r#"for (@c <- @"p" & @d <- @"q") { 5 } | for (@c <- @"s" & @d <- @"t") { 5 }"#,
            &[r#""p""#],
        ),
        dynamic_root_case(
            "receive-bind-binder",
            r#"for (@[a, b] <- x & @c <- @"q") { 5 } | _"#,
            r#"for (@[d] <- @"p" & @f <- @"q") { 5 } | for (@[d, e] <- @"p" & @f <- @"r") { 5 }"#,
            r#"for (@[d, e] <- @"p" & @f <- @"q") { 5 } | for (@d <- @"p" & @f <- @"q") { 5 }"#,
            &[r#""p""#],
        ),
        dynamic_root_case(
            "receive-bind-remainder",
            r#"for (@a ...@r <- x & @b <- @"q") { y } | _"#,
            r#"for (@c <- @"p" & @d <- @"r") { 5 } | for (@c, @e <- @"p" & @d <- @"q") { 5 }"#,
            r#"for (@c <- @"p" & @d <- @"q") { 5 } | for (@c, @e <- @"p" & @d <- @"q") { 6 }"#,
            &[r#""p""#, "5"],
        ),
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

async fn execute<S>(space: &S, case: &Case, direction: Direction) -> (Selection, CandidateOrder)
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
            let before = read_order_twice(space, case, "before the receive arrives").await;
            let selection = space
                .consume(
                    case.channels.clone(),
                    case.patterns.clone(),
                    case.continuation.clone(),
                    false,
                    case.peeks.clone(),
                )
                .await
                .unwrap_or_else(|error| panic!("{}: consume: {error}", case.name))
                .unwrap_or_else(|| panic!("{}: consume direction must fire", case.name));
            (selection, before)
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
            let before = read_order_twice(space, case, "before the datum arrives").await;
            let (continuation, data, _) = space
                .produce(case.arrival.0.clone(), case.arrival.1.clone(), false)
                .await
                .unwrap_or_else(|error| panic!("{}: completing produce: {error}", case.name))
                .unwrap_or_else(|| panic!("{}: produce direction must fire", case.name));
            ((continuation, data), before)
        }
    }
}

async fn observe<M>(matcher: M, case: &Case, direction: Direction) -> Observation
where M: Match<BindPattern, ListParWithRandom, TaggedContinuation> + 'static {
    let (play, replay) = pair(matcher).await;
    let initial = play.create_checkpoint().await.expect("initial checkpoint");
    let (selection, before) = execute(&play, case, direction).await;
    assert_eq!(
        selection.1.len(),
        case.channels.len(),
        "{}: bind count",
        case.name
    );
    let bindings = selection
        .1
        .iter()
        .map(|result| result.matched_datum.pars.clone())
        .collect::<Vec<_>>();
    let outcome = case
        .outcomes
        .iter()
        .find(|outcome| outcome.bindings == bindings)
        .unwrap_or_else(|| {
            panic!(
                "{}: exact FreeMap projection in bind order: {bindings:?} is none of the enabled COMMs",
                case.name
            )
        });
    for (channel, expected) in &outcome.remaining {
        let actual = play
            .get_data(channel)
            .await
            .into_iter()
            .map(|datum| ((*datum.a).clone(), datum.persist))
            .collect::<Vec<_>>();
        assert_eq!(actual, *expected, "{}: exact remaining data", case.name);
    }
    let after = read_order_twice(&play, case, "after the COMM").await;
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
    let (replay_selection, replay_before) = execute(&replay, case, direction).await;
    assert_eq!(
        replay_selection, selection,
        "{}: exact replay binding",
        case.name
    );
    assert_eq!(
        replay_before, before,
        "{}: K4: replay reads the play state's candidate order before the COMM",
        case.name
    );
    assert_eq!(
        read_order_twice(&replay, case, "after the replayed COMM").await,
        after,
        "{}: K4: replay reads the play state's candidate order after the COMM",
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

async fn candidate_order<S>(space: &S, case: &Case) -> CandidateOrder
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let mut data = Vec::with_capacity(case.channels.len());
    let mut joins = Vec::with_capacity(case.channels.len());
    for channel in &case.channels {
        data.push(space.get_data(channel).await);
        joins.push(space.get_joins(channel.clone()).await);
    }
    CandidateOrder {
        state: space.to_map().await,
        data,
        continuations: space.get_waiting_continuations(case.channels.clone()).await,
        joins,
    }
}

async fn read_order_twice<S>(space: &S, case: &Case, moment: &str) -> CandidateOrder
where S: ISpace<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
    let first = candidate_order(space, case).await;
    let second = candidate_order(space, case).await;
    assert_eq!(
        first, second,
        "{}: K4: {moment}: the same state read twice gives the same candidate order",
        case.name
    );
    first
}

async fn enabled_comms(case: &Case) -> Vec<Vec<Vec<Par>>> {
    let (space, _) = pair(Matcher).await;
    let arrival = (case.arrival.0.clone(), case.arrival.1.clone(), false);
    for (channel, datum, persistent) in case.background.iter().chain([&arrival]) {
        assert!(
            space
                .produce(channel.clone(), datum.clone(), *persistent)
                .await
                .unwrap_or_else(|error| panic!("{}: staged produce: {error}", case.name))
                .is_none(),
            "{}: staged data rests while no receive is stored",
            case.name
        );
    }
    let store = space.get_store();
    let _ = store.put_continuation(
        &case.channels,
        WaitingContinuation::create(
            &case.channels,
            &case.patterns,
            &case.continuation,
            false,
            case.peeks.clone(),
        ),
    );
    for channel in &case.channels {
        store.put_join(channel, &case.channels);
    }
    let mut enabled = Vec::new();
    for rendezvous in space.enabled_rendezvous() {
        let bindings = rendezvous
            .data_candidates()
            .iter()
            .map(|candidate| candidate.datum.a.pars.clone())
            .collect::<Vec<_>>();
        if !enabled.contains(&bindings) {
            enabled.push(bindings);
        }
    }
    enabled
}

#[tokio::test(flavor = "current_thread")]
async fn both_matching_directions_preserve_bindings_comm_state_root_and_replay() {
    let cases = cases();
    assert_eq!(cases.len(), 28);
    for case in &cases {
        let consume = observe(Matcher, case, Direction::ConsumeLast).await;
        let produce = observe(Matcher, case, Direction::ProduceLast).await;
        let control_consume = observe(FullScanMatcher, case, Direction::ConsumeLast).await;
        let control_produce = observe(FullScanMatcher, case, Direction::ProduceLast).await;

        if let [_] = case.outcomes.as_slice() {
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
        }
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
            consume.state, control_consume.state,
            "{}: consume post-state oracle",
            case.name
        );
        assert_eq!(
            produce.state, control_produce.state,
            "{}: produce post-state oracle",
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

#[tokio::test(flavor = "current_thread")]
async fn exactly_one_comm_is_enabled_in_each_case_whose_directions_must_agree() {
    let mut single = Vec::new();
    for case in &cases() {
        let enabled = enabled_comms(case).await;
        let declared = case
            .outcomes
            .iter()
            .map(|outcome| outcome.bindings.clone())
            .collect::<Vec<_>>();
        assert!(
            declared.len() == enabled.len()
                && declared.iter().all(|bindings| enabled.contains(bindings))
                && enabled.iter().all(|bindings| declared.contains(bindings)),
            "{}: the declared outcomes {declared:?} are the enabled COMMs {enabled:?}",
            case.name
        );
        if let [_] = enabled.as_slice() {
            single.push(case.name);
        }
    }
    assert_eq!(
        single,
        [
            "ordered",
            "ac",
            "remainder",
            "guard",
            "peek",
            "persistent",
            "repeated-channel",
            "nonlinear-equal",
            "nonlinear-conflicting",
            "nonlinear-nested",
            "new-ground",
            "new-open",
            "new-binder",
            "new-remainder",
            "receive-ground",
            "receive-open",
            "receive-binder",
            "receive-remainder",
            "match-ground",
            "match-open",
            "match-binder",
            "match-remainder",
            "receive-bind-ground",
            "receive-bind-open",
            "receive-bind-binder",
            "receive-bind-remainder",
        ],
        "the cases whose two directions must agree are exactly the cases with one enabled COMM"
    );
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
            prop_assert_eq!(enabled_comms(&case).await, vec![vec![vec![integer(accepted)]]]);
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

struct AlphaPair {
    name: &'static str,
    left: &'static str,
    right: &'static str,
    swapped: &'static str,
    arrival: Vec<Par>,
}

fn alpha_pairs() -> [AlphaPair; 3] {
    [
        AlphaPair {
            name: "receive binder",
            left: r#"for (@x, @y <- @"alpha") { @"out"!(x, y) }"#,
            right: r#"for (@first, @second <- @"alpha") { @"out"!(first, second) }"#,
            swapped: r#"for (@x, @y <- @"alpha") { @"out"!(y, x) }"#,
            arrival: vec![integer(1), integer(2)],
        },
        AlphaPair {
            name: "nested new",
            left: r#"for (@x, @y <- @"beta") { new ack in { ack!(x) | @"out"!(y) } }"#,
            right: r#"for (@x, @y <- @"beta") { new reply in { reply!(x) | @"out"!(y) } }"#,
            swapped: r#"for (@x, @y <- @"beta") { new ack in { ack!(y) | @"out"!(x) } }"#,
            arrival: vec![integer(3), integer(4)],
        },
        AlphaPair {
            name: "match case binder",
            left: r#"for (@x <- @"gamma") { match x { [head, tail] => @"out"!(head, tail) } }"#,
            right: r#"for (@x <- @"gamma") { match x { [first, rest] => @"out"!(first, rest) } }"#,
            swapped: r#"for (@x <- @"gamma") { match x { [head, tail] => @"out"!(tail, head) } }"#,
            arrival: vec![new_elist_par(
                vec![integer(5), integer(6)],
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )],
        },
    ]
}

fn normalized(label: &str, source: &str) -> Par {
    match rholang::rust::interpreter::compiler::compiler::Compiler::source_to_adt(source) {
        Ok(process) => process,
        Err(error) => panic!("{label}: the Rholang source must compile: {error:?}"),
    }
}

fn receive_case(name: &'static str, process: &Par, arrival: &[Par]) -> Case {
    let receive = match <[models::rhoapi::Receive; 1]>::try_from(
        process.decode_occurrences::<models::rhoapi::Receive>(),
    ) {
        Ok([receive]) => receive,
        Err(receives) => panic!("{name}: expected one receive, found {}", receives.len()),
    };
    assert!(
        !receive.persistent && !receive.peek,
        "{name}: the receive is linear and consuming"
    );
    let receive_bind = match <[models::rhoapi::ReceiveBind; 1]>::try_from(receive.binds) {
        Ok([receive_bind]) => receive_bind,
        Err(binds) => panic!("{name}: expected one bind, found {}", binds.len()),
    };
    let source = receive_bind
        .source
        .expect("a compiled receive bind carries its source");
    Case {
        name,
        channels: vec![source.clone()],
        patterns: vec![bind(
            receive_bind.patterns,
            receive_bind.remainder,
            receive_bind.free_count,
        )],
        background: Vec::new(),
        arrival: (source.clone(), datum(arrival.to_vec())),
        continuation: TaggedContinuation {
            tagged_cont: Some(models::rhoapi::tagged_continuation::TaggedCont::ParBody(
                models::rhoapi::ParWithRandom {
                    body: receive.body,
                    random_state: name.as_bytes().to_vec(),
                },
            )),
            guard: receive
                .condition
                .filter(|condition| *condition != Par::default()),
            ..TaggedContinuation::default()
        },
        peeks: BTreeSet::new(),
        outcomes: vec![Outcome {
            bindings: vec![arrival.to_vec()],
            remaining: vec![(source, Vec::new())],
        }],
    }
}

fn dynamic_root_case(
    name: &'static str,
    pattern: &str,
    decoy: &str,
    arrival: &str,
    bindings: &[&str],
) -> Case {
    let receive = normalized(
        name,
        &format!("for (@{{ {pattern} }} <- @\"{name}\") {{ Nil }}"),
    );
    let case = receive_case(name, &receive, &[normalized(name, arrival)]);
    let channel = case.arrival.0.clone();
    let decoy = datum(vec![normalized(name, decoy)]);
    Case {
        background: vec![(channel.clone(), decoy.clone(), false)],
        outcomes: vec![Outcome {
            bindings: vec![bindings
                .iter()
                .map(|binding| normalized(name, binding))
                .collect()],
            remaining: vec![(channel, vec![(decoy, false)])],
        }],
        ..case
    }
}

#[tokio::test(flavor = "current_thread")]
async fn alpha_equivalent_receives_match_alike_in_both_directions() {
    for pair in alpha_pairs() {
        assert_ne!(
            pair.left, pair.right,
            "{}: the two sources differ in the names of bound variables",
            pair.name
        );
        let left = normalized(pair.name, pair.left);
        let right = normalized(pair.name, pair.right);
        let swapped = normalized(pair.name, pair.swapped);
        let left_bytes = prost::Message::encode_to_vec(&left);
        assert_eq!(
            left_bytes,
            prost::Message::encode_to_vec(&right),
            "{}: receives that differ only in bound names normalize to identical protobuf bytes",
            pair.name
        );
        assert_ne!(
            left_bytes,
            prost::Message::encode_to_vec(&swapped),
            "{}: swapping the uses of two bound variables changes the normalized protobuf bytes",
            pair.name
        );

        let left_case = receive_case(pair.name, &left, &pair.arrival);
        let right_case = receive_case(pair.name, &right, &pair.arrival);
        for (order, direction) in [
            ("consume last", Direction::ConsumeLast),
            ("produce last", Direction::ProduceLast),
        ] {
            let left_seen = observe(Matcher, &left_case, direction).await;
            let right_seen = observe(Matcher, &right_case, direction).await;
            assert_eq!(
                left_seen.selection, right_seen.selection,
                "{}, {order}: selection",
                pair.name
            );
            assert_eq!(
                left_seen.state, right_seen.state,
                "{}, {order}: post-state",
                pair.name
            );
            assert_eq!(
                left_seen.root, right_seen.root,
                "{}, {order}: root",
                pair.name
            );
            assert_eq!(
                comm_trace(&left_seen.log),
                comm_trace(&right_seen.log),
                "{}, {order}: COMM trace",
                pair.name
            );
        }
    }
}
