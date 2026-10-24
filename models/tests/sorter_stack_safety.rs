use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostSignedTerm, CostStack, New, Par, Receive, ReceiveBind};
use models::rust::rholang::dismantle::dismantle_term;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;

fn nested(depth: usize, kind: usize) -> Par {
    let mut par = Par::default();
    for _ in 0..depth {
        par = match kind {
            0 => Par {
                news: vec![New {
                    p: Some(par),
                    ..Default::default()
                }],
                ..Default::default()
            },
            1 => Par {
                cost_signed_terms: vec![CostSignedTerm {
                    signature: Some(CostSignature {
                        value: Some(Value::Quote(par)),
                    }),
                    held_price: Some(u64::MAX),
                    ..Default::default()
                }],
                ..Default::default()
            },
            2 => Par {
                cost_stacks: vec![CostStack {
                    cells: vec![CostSignature {
                        value: Some(Value::Name(par)),
                    }],
                }],
                ..Default::default()
            },
            _ => Par {
                receives: vec![Receive {
                    binds: vec![ReceiveBind {
                        source: Some(Par::default()),
                        cost_signature: Some(CostSignature {
                            value: Some(Value::Quote(par)),
                        }),
                        ..Default::default()
                    }],
                    body: Some(Par::default()),
                    ..Default::default()
                }],
                ..Default::default()
            },
        };
    }
    par
}

#[test]
fn deep_terms_and_cost_signatures_use_a_bounded_sort_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            for kind in 0..4 {
                let input = nested(4096, kind);
                let sorted = ParSortMatcher::sort_match(&input);
                let mut cursor = &sorted.term;
                for _ in 0..4096 {
                    let signature = match kind {
                        0 => {
                            cursor = cursor.news[0].p.as_ref().unwrap();
                            continue;
                        }
                        1 => {
                            assert_eq!(cursor.cost_signed_terms[0].held_price, Some(u64::MAX));
                            cursor.cost_signed_terms[0].signature.as_ref().unwrap()
                        }
                        2 => &cursor.cost_stacks[0].cells[0],
                        _ => cursor.receives[0].binds[0].cost_signature.as_ref().unwrap(),
                    };
                    cursor = match signature.value.as_ref().unwrap() {
                        Value::Quote(par) | Value::Name(par) => par,
                        _ => panic!("expected a nested signature"),
                    };
                }
                assert_eq!(cursor, &Par::default());
                dismantle_term(input);
                dismantle_term(sorted.term);
                drop(sorted.score);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn deep_equal_siblings_use_a_bounded_comparison_stack() {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(|| {
            let input = Par {
                news: vec![
                    New {
                        p: Some(nested(4096, 0)),
                        ..Default::default()
                    },
                    New {
                        p: Some(nested(4096, 0)),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            };
            let sorted = ParSortMatcher::sort_match(&input);
            assert_eq!(sorted.term.news.len(), 2);
            dismantle_term(input);
            dismantle_term(sorted.term);
            drop(sorted.score);
        })
        .unwrap()
        .join()
        .unwrap();
}
