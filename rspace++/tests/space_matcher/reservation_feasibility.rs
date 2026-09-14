use std::collections::HashMap;

use super::walk_candidate_selections;
use crate::rspace::data_index::RSpaceDataIndex;
use crate::rspace::internal::{Datum, DatumAddress};
use crate::rspace::r#match::Match;

struct ReservationMatch;

impl Match<(), String, bool> for ReservationMatch {
    fn get(&self, _: &(), data: &String) -> Option<String> { Some(data.clone()) }
    fn check_commit(&self, accepts: &bool, _: &[&String]) -> bool { *accepts }
}

fn check_reservation_feasibility_case(
    case: (u8, u8, bool, Option<bool>, bool, bool, bool),
) -> Result<(), proptest::test_runner::TestCaseError> {
    use proptest::prelude::*;

    let (count, binds, persistent, fresh, keep_stored, keep_fresh, guard_accepts) = case;
    let channel = "reservation".to_owned();
    let stored = Datum::create(&channel, "7".to_owned(), persistent);
    let mut index = RSpaceDataIndex::new();
    let mut stored_path = None;
    for _ in 0..count {
        stored_path = Some(index.insert(&channel, &stored).expect("stored fixture").0);
    }
    let mut pool = index
        .candidate_pool::<_, String>(&channel)
        .expect("candidate pool");
    if let Some(fresh_persistent) = fresh {
        prop_assert!(
            pool.prepend_fresh(Datum::create(&channel, "8".to_owned(), fresh_persistent,))
        );
    }
    let filtered = pool.retain_before_selection(|datum| {
        if datum.a.as_str() == "8" {
            keep_fresh
        } else {
            keep_stored
        }
    });
    prop_assert!(filtered);
    let initial = HashMap::from([(channel.clone(), pool)]);
    let fresh_eligible = fresh.is_some() && keep_fresh;
    let mut expected = vec![Vec::<bool>::new()];
    for _ in 0..binds {
        let mut next = Vec::new();
        for prefix in expected {
            let fresh_used = prefix.iter().filter(|is_fresh| **is_fresh).count();
            let stored_used = prefix.len() - fresh_used;
            if fresh_eligible && (fresh == Some(true) || fresh_used == 0) {
                let mut branch = prefix.clone();
                branch.push(true);
                next.push(branch);
            }
            if keep_stored && count != 0 && (persistent || stored_used < usize::from(count)) {
                let mut branch = prefix;
                branch.push(false);
                next.push(branch);
            }
        }
        expected = next;
    }
    if !guard_accepts {
        expected.clear();
    }

    let patterns = vec![(); usize::from(binds)];
    let pairs = patterns
        .iter()
        .map(|pattern| (&channel, pattern))
        .collect::<Vec<_>>();
    let continuation = guard_accepts;
    let matcher: Box<dyn Match<(), String, bool>> = Box::new(ReservationMatch);
    let mut selected_pool = initial.clone();
    let mut selected_values = Vec::new();
    let outcome = walk_candidate_selections(
        &matcher,
        &pairs,
        &continuation,
        &mut selected_pool,
        0,
        &mut selected_values,
        |_| true,
        |_| {},
    );
    let selected = outcome.stopped.then_some(selected_values);
    let selected_addresses = selected.as_ref().map(|selection| {
        selection
            .iter()
            .map(|candidate| candidate.address.is_fresh())
            .collect::<Vec<_>>()
    });
    prop_assert_eq!(selected_addresses.as_ref(), expected.first());
    let mut expected_pool = initial.clone();
    if let Some(first) = expected.first() {
        for is_fresh in first {
            if if *is_fresh {
                fresh != Some(true)
            } else {
                !persistent
            } {
                let position = if *is_fresh {
                    0
                } else {
                    usize::from(fresh_eligible)
                };
                prop_assert!(expected_pool.get_mut(&channel).unwrap().reserve(position));
            }
        }
    }
    prop_assert_eq!(&selected_pool, &expected_pool);

    let mut enumerated_pool = initial.clone();
    let mut chosen = Vec::new();
    let mut enumerated = Vec::new();
    let outcome = walk_candidate_selections(
        &matcher,
        &pairs,
        &continuation,
        &mut enumerated_pool,
        0,
        &mut chosen,
        |selection| {
            enumerated.push(selection.to_vec());
            false
        },
        |_| {},
    );
    prop_assert!(!outcome.stopped);
    let enumerated_addresses = enumerated
        .iter()
        .map(|selection| {
            selection
                .iter()
                .map(|candidate| candidate.address.is_fresh())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    prop_assert_eq!(enumerated_addresses, expected);
    prop_assert!(chosen.is_empty());
    prop_assert_eq!(enumerated_pool, initial);
    for selection in selected.iter().chain(enumerated.iter()) {
        prop_assert_eq!(selection.len(), usize::from(binds));
        for candidate in selection {
            prop_assert_eq!(&candidate.channel, &channel);
            let is_fresh = candidate.address.is_fresh();
            prop_assert_eq!(candidate.datum.a.as_str(), if is_fresh { "8" } else { "7" });
            prop_assert_eq!(candidate.removed_datum.as_str(), candidate.datum.a.as_str());
            prop_assert_eq!(
                candidate.datum.persist,
                if is_fresh { fresh.unwrap() } else { persistent }
            );
            if let DatumAddress::Stored(path) = &candidate.address {
                prop_assert_eq!(path.as_ref(), stored_path.as_ref().unwrap().as_slice());
            }
        }
    }
    prop_assert_eq!(index.count(&channel, &stored), u64::from(count));
    Ok(())
}

#[test]
fn reservation_feasibility_matches_independent_multiset_oracle() {
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};

    for count in 0..=3 {
        for binds in 1..=4 {
            for persistent in [false, true] {
                for fresh in [None, Some(false), Some(true)] {
                    for keep_stored in [false, true] {
                        for keep_fresh in [false, true] {
                            for guard_accepts in [false, true] {
                                check_reservation_feasibility_case((
                                    count,
                                    binds,
                                    persistent,
                                    fresh,
                                    keep_stored,
                                    keep_fresh,
                                    guard_accepts,
                                ))
                                .expect("finite reservation matrix");
                            }
                        }
                    }
                }
            }
        }
    }
    let strategy = (
        0u8..9,
        1u8..7,
        any::<bool>(),
        proptest::option::of(any::<bool>()),
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
    );
    TestRunner::new(Config {
        cases: 128,
        ..Config::default()
    })
    .run(&strategy, |case| check_reservation_feasibility_case(case))
    .expect("generated reservation histories");
}
