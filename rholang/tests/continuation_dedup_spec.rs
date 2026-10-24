//! The hot store drops a waiting continuation that duplicates one already on
//! its channels. These tests pin which continuations count as duplicates for
//! the Rholang types: equal patterns, body, persistence and peeks, including
//! fields the models' `PartialEq` skips, such as `locally_free`.

use std::collections::BTreeSet;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, Expr, ListParWithRandom, Par, ParWithRandom, Send, TaggedContinuation,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::trace::event::Consume;

type Continuation = WaitingContinuation<BindPattern, TaggedContinuation>;

struct EmptyHistory;

impl HistoryReaderBase<Par, BindPattern, ListParWithRandom, TaggedContinuation> for EmptyHistory {
    fn get_data_proj(&self, _key: &Par) -> Vec<Datum<ListParWithRandom>> { Vec::new() }

    fn get_continuations_proj(&self, _key: &Vec<Par>) -> Vec<Continuation> { Vec::new() }

    fn get_joins_proj(&self, _key: &Par) -> Vec<Vec<Par>> { Vec::new() }
}

fn store() -> Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
    HotStoreInstances::create_from_hr(Box::new(EmptyHistory))
}

fn channels() -> Vec<Par> { vec![int_par(99, Vec::new())] }

fn int_par(value: i64, locally_free: Vec<u8>) -> Par {
    Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GInt(value)),
        }],
        locally_free,
        ..Par::default()
    }
}

/// Whether the store kept `second` after `first` was put on the same channels.
fn second_is_kept(first: &Continuation, second: &Continuation) -> bool {
    let store = store();
    assert_eq!(
        store.put_continuation(&channels(), first.clone()),
        Some(true)
    );
    let kept = store
        .put_continuation(&channels(), second.clone())
        .expect("put_continuation on the in-memory store");
    assert_eq!(
        store.get_continuations(&channels()).len(),
        if kept { 2 } else { 1 },
        "the returned flag must agree with what the store holds"
    );
    kept
}

/// The duplicate rule the store applied when it compared printed identities.
fn printed_identity(wc: &Continuation) -> String {
    format!(
        "{:?}|{:?}|{}|{:?}",
        wc.patterns, wc.continuation, wc.persist, wc.peeks
    )
}

fn small_bytes() -> impl Strategy<Value = Vec<u8>> {
    prop_oneof![Just(Vec::new()), Just(vec![1u8]), Just(vec![2u8])]
}

fn par_strategy() -> impl Strategy<Value = Par> {
    (
        prop::collection::vec(0i64..3, 0..2),
        small_bytes(),
        any::<bool>(),
        prop::option::of((0i64..2, small_bytes(), any::<bool>())),
    )
        .prop_map(|(ints, locally_free, connective_used, send)| Par {
            exprs: ints
                .into_iter()
                .map(|value| Expr {
                    expr_instance: Some(ExprInstance::GInt(value)),
                })
                .collect(),
            sends: send
                .into_iter()
                .map(|(chan, send_locally_free, persistent)| Send {
                    chan: Some(int_par(chan, Vec::new())),
                    data: Vec::new(),
                    persistent,
                    locally_free: send_locally_free,
                    connective_used: false,
                })
                .collect(),
            locally_free,
            connective_used,
            ..Par::default()
        })
}

fn continuation_strategy() -> impl Strategy<Value = Continuation> {
    (
        prop::collection::vec((prop::collection::vec(par_strategy(), 0..2), 0i32..2), 0..2),
        par_strategy(),
        small_bytes(),
        prop::option::of(par_strategy()),
        any::<bool>(),
        prop::collection::btree_set(0i32..2, 0..2),
    )
        .prop_map(|(patterns, body, random_state, guard, persist, peeks)| {
            WaitingContinuation {
                patterns: patterns
                    .into_iter()
                    .map(|(patterns, free_count)| BindPattern {
                        patterns,
                        remainder: None,
                        free_count,
                    })
                    .collect(),
                continuation: TaggedContinuation {
                    tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                        body: Some(body),
                        random_state,
                    })),
                    guard,
                },
                persist,
                peeks,
                source: Consume::default(),
            }
        })
}

#[derive(Debug, Clone, Copy)]
enum Mutation {
    None,
    BodyLocallyFree,
    PatternLocallyFree,
    SendLocallyFree,
    RandomState,
    Persist,
    Peeks,
    Guard,
    Source,
}

fn body_mut(wc: &mut Continuation) -> &mut Par {
    match wc.continuation.tagged_cont.as_mut() {
        Some(TaggedCont::ParBody(body)) => body.body.get_or_insert_with(Par::default),
        _ => unreachable!("the strategy builds Par bodies only"),
    }
}

fn flip(bytes: &mut Vec<u8>) {
    if bytes.is_empty() {
        bytes.push(7);
    } else {
        bytes.clear();
    }
}

fn mutate(mut wc: Continuation, mutation: Mutation) -> Continuation {
    match mutation {
        Mutation::None => {}
        Mutation::BodyLocallyFree => flip(&mut body_mut(&mut wc).locally_free),
        Mutation::PatternLocallyFree => match wc.patterns.first_mut() {
            Some(bind) => match bind.patterns.first_mut() {
                Some(pattern) => flip(&mut pattern.locally_free),
                None => bind.patterns.push(Par::default()),
            },
            None => wc.patterns.push(BindPattern::default()),
        },
        Mutation::SendLocallyFree => match body_mut(&mut wc).sends.first_mut() {
            Some(send) => flip(&mut send.locally_free),
            None => body_mut(&mut wc).sends.push(Send::default()),
        },
        Mutation::RandomState => match wc.continuation.tagged_cont.as_mut() {
            Some(TaggedCont::ParBody(body)) => flip(&mut body.random_state),
            _ => unreachable!("the strategy builds Par bodies only"),
        },
        Mutation::Persist => wc.persist = !wc.persist,
        Mutation::Peeks => {
            if !wc.peeks.remove(&0) {
                wc.peeks.insert(0);
            }
        }
        Mutation::Guard => {
            wc.continuation.guard = match wc.continuation.guard {
                Some(_) => None,
                None => Some(Par::default()),
            }
        }
        Mutation::Source => wc.source.persistent = !wc.source.persistent,
    }
    wc
}

fn mutation_strategy() -> impl Strategy<Value = Mutation> {
    prop_oneof![
        Just(Mutation::None),
        Just(Mutation::BodyLocallyFree),
        Just(Mutation::PatternLocallyFree),
        Just(Mutation::SendLocallyFree),
        Just(Mutation::RandomState),
        Just(Mutation::Persist),
        Just(Mutation::Peeks),
        Just(Mutation::Guard),
        Just(Mutation::Source),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    #[test]
    fn a_near_copy_is_dropped_exactly_when_it_prints_the_same(
        first in continuation_strategy(),
        mutation in mutation_strategy(),
    ) {
        let second = mutate(first.clone(), mutation);
        let duplicate = printed_identity(&first) == printed_identity(&second);
        prop_assert_eq!(second_is_kept(&first, &second), !duplicate);
    }

    #[test]
    fn an_unrelated_continuation_is_dropped_exactly_when_it_prints_the_same(
        first in continuation_strategy(),
        second in continuation_strategy(),
    ) {
        let duplicate = printed_identity(&first) == printed_identity(&second);
        prop_assert_eq!(second_is_kept(&first, &second), !duplicate);
    }
}

fn fixed_continuation() -> Continuation {
    WaitingContinuation {
        patterns: vec![BindPattern {
            patterns: vec![int_par(1, vec![1])],
            remainder: None,
            free_count: 1,
        }],
        continuation: TaggedContinuation {
            tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                body: Some(int_par(2, vec![1])),
                random_state: vec![3],
            })),
            guard: None,
        },
        persist: true,
        peeks: BTreeSet::new(),
        source: Consume::default(),
    }
}

#[test]
fn an_identical_continuation_is_dropped() {
    let wc = fixed_continuation();
    assert!(!second_is_kept(&wc, &wc));
}

#[test]
fn a_continuation_from_another_consume_is_still_a_duplicate() {
    let first = fixed_continuation();
    let second = mutate(first.clone(), Mutation::Source);
    assert!(!second_is_kept(&first, &second));
}

#[test]
fn a_continuation_differing_only_in_locally_free_is_kept() {
    let first = fixed_continuation();
    let second = mutate(first.clone(), Mutation::BodyLocallyFree);
    assert_eq!(first, second, "models' PartialEq skips locally_free");
    assert!(second_is_kept(&first, &second));
}

#[test]
fn a_continuation_with_a_different_body_is_kept() {
    let first = fixed_continuation();
    let mut second = first.clone();
    *body_mut(&mut second) = int_par(5, vec![1]);
    assert!(second_is_kept(&first, &second));
}
