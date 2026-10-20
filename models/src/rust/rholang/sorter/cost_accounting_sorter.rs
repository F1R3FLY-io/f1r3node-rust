// Changed by the merge of feature/cost-accounted-rho (DR-131): the removed
// metered sorter used most of these names in production code. Now only the
// test oracles below (`sort_signature_recursive`, `collect_compound` and
// `collect_compound_recursive`) use them, and no code uses `Par`.
// use super::par_sort_matcher::ParSortMatcher;
// use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
// use super::sort_drive::{sort_cost_signature, sort_cost_signed_term, sort_cost_stack};
// use super::sortable::Sortable;
// use crate::rhoapi::cost_signature::Value;
// use crate::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};
#[cfg(test)]
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::ScoredTerm;
#[cfg(test)]
use super::score_tree::{Score, ScoreAtom, Tree};
use super::sort_drive::{sort_cost_signature, sort_cost_signed_term, sort_cost_stack};
#[cfg(test)]
use super::sortable::Sortable;
#[cfg(test)]
use crate::rhoapi::cost_signature::Value;
#[cfg(test)]
use crate::rhoapi::CostSignatureCompound;
use crate::rhoapi::{CostSignature, CostSignedTerm, CostStack};

pub fn sort_signature(signature: &CostSignature) -> ScoredTerm<CostSignature> {
    sort_cost_signature(signature)
}

#[cfg(test)]
fn sort_signature_recursive(signature: &CostSignature) -> ScoredTerm<CostSignature> {
    match &signature.value {
        None => ScoredTerm {
            term: CostSignature::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        },
        Some(Value::Ground(bytes)) => ScoredTerm {
            term: CostSignature {
                value: Some(Value::Ground(bytes.clone())),
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_GROUND, vec![
                Tree::<ScoreAtom>::create_leaf_from_bytes(bytes.clone()),
            ]),
        },
        Some(Value::Unit(_)) => ScoredTerm {
            term: CostSignature {
                value: Some(Value::Unit(true)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_UNIT, Vec::new()),
        },
        Some(Value::BoundLevel(level)) => ScoredTerm {
            term: CostSignature {
                value: Some(Value::BoundLevel(*level)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_BOUND, vec![
                Tree::<ScoreAtom>::create_leaf_from_i64(*level as i64),
            ]),
        },
        Some(Value::Quote(par)) => {
            let sorted = ParSortMatcher::sort_match(par);
            ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Quote(sorted.term)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_QUOTE, vec![
                    sorted.score,
                ]),
            }
        }
        Some(Value::Compound(compound)) => {
            let mut elements = Vec::new();
            collect_compound(&compound.elements, &mut elements);
            elements.retain(|element| !matches!(element.term.value.as_ref(), Some(Value::Unit(_))));
            if elements.is_empty() {
                return sort_signature_recursive(&CostSignature {
                    value: Some(Value::Unit(true)),
                });
            }
            if elements.len() == 1 {
                return elements.pop().expect("one cost-signature element");
            }
            ScoredTerm::sort_vec(&mut elements);
            let scores = elements
                .iter()
                .map(|element| element.score.clone())
                .collect();
            let terms = elements.into_iter().map(|element| element.term).collect();
            ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Compound(CostSignatureCompound { elements: terms })),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_COMPOUND, scores),
            }
        }
        Some(Value::Name(par)) => {
            let sorted = ParSortMatcher::sort_match(par);
            ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Name(sorted.term)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_NAME, vec![
                    sorted.score,
                ]),
            }
        }
    }
}

// Changed by DR-131 P1: the flatten runs on an explicit work stack, so the
// native stack no longer grows with the compound depth. The stack holds the
// pending signatures in reverse source order, so the leaves come out in source
// order, as the recursive version gave them. The recursive version below stays
// as the test oracle.
// Changed by the merge of feature/cost-accounted-rho (finding R3-19): this
// flatten is a test oracle. Production `sort_signature` flattens in
// `descend_cost_signature` (`sort_drive.rs`). `ScoredTerm::sort_vec` is a total
// order (`score_tree.rs`), so the sorted result does not depend on the order
// of the leaves. The removed sentence said: "That order matters, because
// `ScoredTerm::sort_vec` keeps ties in input order."
#[cfg(test)]
fn collect_compound(signatures: &[CostSignature], output: &mut Vec<ScoredTerm<CostSignature>>) {
    let mut work: Vec<&CostSignature> = signatures.iter().rev().collect();
    while let Some(signature) = work.pop() {
        match &signature.value {
            Some(Value::Compound(compound)) => work.extend(compound.elements.iter().rev()),
            _ => output.push(sort_signature_recursive(signature)),
        }
    }
}

// Replaced by DR-131 P1 with the work-stack flatten above, because it recursed
// once for each compound level. It stays as the oracle of the stack-safety
// tests.
#[cfg(test)]
fn collect_compound_recursive(
    signatures: &[CostSignature],
    output: &mut Vec<ScoredTerm<CostSignature>>,
) {
    for signature in signatures {
        match &signature.value {
            Some(Value::Compound(compound)) => {
                collect_compound_recursive(&compound.elements, output)
            }
            _ => output.push(sort_signature_recursive(signature)),
        }
    }
}

pub fn sort_signed_term(term: &CostSignedTerm) -> ScoredTerm<CostSignedTerm> {
    sort_cost_signed_term(term)
}

pub fn sort_stack(stack: &CostStack) -> ScoredTerm<CostStack> { sort_cost_stack(stack) }

// Added by DR-131 P1: the stack-safety tests of the cost-signature sort.
#[cfg(test)]
mod stack_safety_tests;

// Added by the merge of feature/cost-accounted-rho (plan step 1.4): four tests
// of F1R3Lang's `metered_tests` module at HEAD, copied verbatim. DR-131 removed
// the metered tests of that module. These four call only `sort_signature` and
// its test oracle `sort_signature_recursive`. Theirs' `stack_safety_tests`
// covers chains of compound signatures only. These tests add mixed nesting
// through quoted Pars, cost stacks, names and receive binds.
#[cfg(test)]
mod sort_tests {
    use super::*;
    use crate::rhoapi::{Par, Receive, ReceiveBind};

    #[test]
    fn worklist_matches_source_signature_fold_on_shallow_mixed_cases() {
        let cases = [
            CostSignature::default(),
            CostSignature {
                value: Some(Value::Ground(vec![0, 9, 1])),
            },
            CostSignature {
                value: Some(Value::Unit(false)),
            },
            CostSignature {
                value: Some(Value::BoundLevel(3)),
            },
            CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![
                        CostSignature {
                            value: Some(Value::Unit(true)),
                        },
                        CostSignature {
                            value: Some(Value::BoundLevel(2)),
                        },
                        CostSignature {
                            value: Some(Value::Compound(CostSignatureCompound {
                                elements: vec![
                                    CostSignature {
                                        value: Some(Value::Ground(vec![3])),
                                    },
                                    CostSignature {
                                        value: Some(Value::BoundLevel(1)),
                                    },
                                ],
                            })),
                        },
                    ],
                })),
            },
            CostSignature {
                value: Some(Value::Quote(Par::default())),
            },
            CostSignature {
                value: Some(Value::Name(Par::default())),
            },
        ];
        for case in &cases {
            assert_eq!(sort_signature(case), sort_signature_recursive(case));
        }
    }

    #[test]
    fn deeply_nested_compound_sort_is_stack_safe() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut signature = CostSignature {
                    value: Some(Value::Unit(true)),
                };
                for _ in 0..4096 {
                    signature = CostSignature {
                        value: Some(Value::Compound(CostSignatureCompound {
                            elements: vec![signature],
                        })),
                    };
                }
                let sorted = sort_signature(&signature);
                assert!(matches!(
                    sorted.term.value.as_ref(),
                    Some(Value::Unit(true))
                ));
            })
            .expect("spawn")
            .join()
            .expect("small-stack compound sort thread");
    }

    #[test]
    fn alternating_quote_par_cost_stack_sort_is_stack_safe() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut signature = CostSignature {
                    value: Some(Value::BoundLevel(0)),
                };
                for _ in 0..4096 {
                    let mut par = Par::default();
                    par.cost_stacks.push(CostStack {
                        cells: vec![signature],
                        parallel_tail: Vec::new(),
                    });
                    signature = CostSignature {
                        value: Some(Value::Quote(par)),
                    };
                }
                let sorted = sort_signature(&signature);
                assert!(matches!(sorted.term.value.as_ref(), Some(Value::Quote(_))));
            })
            .expect("spawn")
            .join()
            .expect("small-stack alternating cost sort thread");
    }

    #[test]
    fn alternating_name_par_receive_bind_signature_sort_is_stack_safe() {
        std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut signature = CostSignature {
                    value: Some(Value::BoundLevel(0)),
                };
                for _ in 0..512 {
                    let mut par = Par::default();
                    par.receives.push(Receive {
                        binds: vec![ReceiveBind {
                            source: Some(Par::default()),
                            cost_signature: Some(signature),
                            ..ReceiveBind::default()
                        }],
                        body: Some(Par::default()),
                        ..Receive::default()
                    });
                    signature = CostSignature {
                        value: Some(Value::Name(par)),
                    };
                }
                let sorted = sort_signature(&signature);
                assert!(matches!(sorted.term.value.as_ref(), Some(Value::Name(_))));
            })
            .expect("spawn")
            .join()
            .expect("small-stack bind-cost alternating sort thread");
    }
}
