use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sort_drive::{sort_cost_signature, sort_cost_signed_term, sort_cost_stack};
use super::sortable::Sortable;
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};

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

pub fn sort_signed_term(term: &CostSignedTerm) -> ScoredTerm<CostSignedTerm> {
    sort_cost_signed_term(term)
}

pub fn sort_stack(stack: &CostStack) -> ScoredTerm<CostStack> { sort_cost_stack(stack) }

fn scored_node(
    kind: i32,
    child: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    let mut children = meter.vec(1)?;
    children.push(child);
    Tree::<ScoreAtom>::create_node_from_i32_metered(kind, children, meter)
}

pub fn sort_signature_metered(
    signature: &CostSignature,
    meter: &SorterMeter<'_>,
) -> Result<ScoredTerm<CostSignature>, BackingError> {
    sort_signature_metered_source(signature, meter)
}

// Retained as the source charge-order oracle while the metered explicit
// continuation driver is integrated. Its recursive calls stay within this
// implementation so shallow differential tests remain independent of the
// production entry point after that entry point moves to the driver.
fn sort_signature_metered_source(
    signature: &CostSignature,
    meter: &SorterMeter<'_>,
) -> Result<ScoredTerm<CostSignature>, BackingError> {
    let _depth = meter.enter()?;
    meter.reserve(1, std::mem::size_of::<CostSignature>(), 0)?;
    match &signature.value {
        None => Ok(ScoredTerm {
            term: CostSignature::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        }),
        Some(Value::Ground(bytes)) => Ok(ScoredTerm {
            term: CostSignature {
                value: Some(Value::Ground(meter.clone(bytes)?)),
            },
            score: scored_node(
                Score::COST_SIG_GROUND,
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(bytes)?),
                meter,
            )?,
        }),
        Some(Value::Unit(_)) => Ok(ScoredTerm {
            term: CostSignature {
                value: Some(Value::Unit(true)),
            },
            score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                Score::COST_SIG_UNIT,
                Vec::new(),
                meter,
            )?,
        }),
        Some(Value::BoundLevel(level)) => Ok(ScoredTerm {
            term: CostSignature {
                value: Some(Value::BoundLevel(*level)),
            },
            score: scored_node(
                Score::COST_SIG_BOUND,
                Tree::<ScoreAtom>::create_leaf_from_i64(*level as i64),
                meter,
            )?,
        }),
        Some(Value::Quote(par)) | Some(Value::Name(par)) => {
            let kind = if matches!(&signature.value, Some(Value::Quote(_))) {
                Score::COST_SIG_QUOTE
            } else {
                Score::COST_SIG_NAME
            };
            let sorted = ParSortMatcher::sort_match_metered(par, meter)?;
            Ok(ScoredTerm {
                term: CostSignature {
                    value: Some(if kind == Score::COST_SIG_QUOTE {
                        Value::Quote(sorted.term)
                    } else {
                        Value::Name(sorted.term)
                    }),
                },
                score: scored_node(kind, sorted.score, meter)?,
            })
        }
        Some(Value::Compound(compound)) => {
            let mut elements = meter.vec(compound.elements.len())?;
            collect_compound_metered(&compound.elements, &mut elements, meter)?;
            meter.reserve(
                elements.len(),
                elements
                    .len()
                    .checked_mul(std::mem::size_of::<ScoredTerm<CostSignature>>())
                    .ok_or(BackingError::Overflow)?,
                0,
            )?;
            elements.retain(|element| !matches!(element.term.value.as_ref(), Some(Value::Unit(_))));
            if elements.is_empty() {
                return sort_signature_metered_source(
                    &CostSignature {
                        value: Some(Value::Unit(true)),
                    },
                    meter,
                );
            }
            if elements.len() == 1 {
                return elements.pop().ok_or(BackingError::Rejected);
            }
            ScoredTerm::sort_vec_metered(&mut elements, meter)?;
            let mut scores = meter.vec(elements.len())?;
            let mut terms = meter.vec(elements.len())?;
            for element in elements {
                scores.push(element.score);
                terms.push(element.term);
            }
            Ok(ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Compound(CostSignatureCompound { elements: terms })),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                    Score::COST_SIG_COMPOUND,
                    scores,
                    meter,
                )?,
            })
        }
    }
}

fn collect_compound_metered(
    signatures: &[CostSignature],
    output: &mut Vec<ScoredTerm<CostSignature>>,
    meter: &SorterMeter<'_>,
) -> Result<(), BackingError> {
    let _depth = meter.enter()?;
    for signature in signatures {
        meter.reserve(1, std::mem::size_of::<CostSignature>(), 0)?;
        match &signature.value {
            Some(Value::Compound(compound)) => {
                collect_compound_metered(&compound.elements, output, meter)?;
            }
            _ => meter.push(output, sort_signature_metered_source(signature, meter)?)?,
        }
    }
    Ok(())
}

pub fn sort_signed_term_metered(
    term: &CostSignedTerm,
    meter: &SorterMeter<'_>,
) -> Result<ScoredTerm<CostSignedTerm>, BackingError> {
    meter.reserve(1, std::mem::size_of::<CostSignedTerm>(), 0)?;
    let body = term
        .body
        .as_ref()
        .map(|par| ParSortMatcher::sort_match_metered(par, meter))
        .transpose()?;
    let signature = term
        .signature
        .as_ref()
        .map(|signature| sort_signature_metered(signature, meter))
        .transpose()?;
    let (body_term, body_score) = match body {
        Some(value) => (Some(value.term), value.score),
        None => (
            None,
            Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        ),
    };
    let (signature_term, signature_score) = match signature {
        Some(value) => (Some(value.term), value.score),
        None => (
            None,
            Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        ),
    };
    let mut scores = meter.vec(2)?;
    scores.push(signature_score);
    scores.push(body_score);
    Ok(ScoredTerm {
        term: CostSignedTerm {
            body: body_term,
            signature: signature_term,
        },
        score: Tree::<ScoreAtom>::create_node_from_i32_metered(
            Score::COST_SIGNED_TERM,
            scores,
            meter,
        )?,
    })
}

pub fn sort_stack_metered(
    stack: &CostStack,
    meter: &SorterMeter<'_>,
) -> Result<ScoredTerm<CostStack>, BackingError> {
    // The protobuf tree may be arbitrarily deep. Schedule child visits on the
    // heap and finish each stack after all of its parallel-tail branches.
    enum Work<'a> {
        Visit(&'a CostStack),
        Finish(&'a CostStack),
    }
    let mut work = meter.vec(1)?;
    let mut values = meter.vec(1)?;
    work.push(Work::Visit(stack));
    while let Some(job) = work.pop() {
        match job {
            Work::Visit(node) => {
                meter.reserve(1, std::mem::size_of::<CostStack>(), 0)?;
                meter.push(&mut work, Work::Finish(node))?;
                for branch in node.parallel_tail.iter().rev() {
                    meter.push(&mut work, Work::Visit(branch))?;
                }
            }
            Work::Finish(node) => {
                let mut branches = meter.vec(node.parallel_tail.len())?;
                for _ in 0..node.parallel_tail.len() {
                    branches.push(values.pop().ok_or(BackingError::Rejected)?);
                }
                branches.reverse();
                if !branches.is_empty() {
                    ScoredTerm::sort_vec_metered(&mut branches, meter)?;
                }
                let mut cells = meter.vec(node.cells.len())?;
                for cell in &node.cells {
                    cells.push(sort_signature_metered(cell, meter)?);
                }
                let mut terms = meter.vec(cells.len())?;
                let mut scores = meter.vec(cells.len() + usize::from(!branches.is_empty()))?;
                for cell in cells {
                    terms.push(cell.term);
                    scores.push(cell.score);
                }
                let mut parallel_tail = meter.vec(branches.len())?;
                if !branches.is_empty() {
                    let mut branch_scores = meter.vec(branches.len())?;
                    for branch in branches {
                        parallel_tail.push(branch.term);
                        branch_scores.push(branch.score);
                    }
                    scores.push(Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::COST_STACK_PARALLEL,
                        branch_scores,
                        meter,
                    )?);
                }
                meter.push(&mut values, ScoredTerm {
                    term: CostStack {
                        cells: terms,
                        parallel_tail,
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32_metered(
                        Score::COST_STACK,
                        scores,
                        meter,
                    )?,
                })?;
            }
        }
    }
    values.pop().ok_or(BackingError::Rejected)
}

#[cfg(test)]
mod metered_tests {
    use std::cell::RefCell;

    use super::*;
    use crate::rhoapi::{Receive, ReceiveBind};

    fn charged_signature_sort(
        signature: &CostSignature,
        sort: impl for<'m> Fn(
            &CostSignature,
            &SorterMeter<'m>,
        ) -> Result<ScoredTerm<CostSignature>, BackingError>,
    ) -> (
        Result<ScoredTerm<CostSignature>, BackingError>,
        Vec<[usize; 3]>,
    ) {
        let charges = RefCell::new(Vec::new());
        let reserve = |operations: usize, scanned: usize, backing: usize| {
            charges.borrow_mut().push([operations, scanned, backing]);
            Ok(())
        };
        let meter = SorterMeter::new(&reserve);
        let result = sort(signature, &meter);
        (result, charges.into_inner())
    }

    #[test]
    fn metered_signature_result_and_charge_order_match_source() {
        let cases = [
            CostSignature::default(),
            CostSignature {
                value: Some(Value::Ground(vec![0, 9, 1])),
            },
            CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![
                        CostSignature {
                            value: Some(Value::Unit(false)),
                        },
                        CostSignature {
                            value: Some(Value::BoundLevel(7)),
                        },
                        CostSignature {
                            value: Some(Value::Name(Par::default())),
                        },
                    ],
                })),
            },
            CostSignature {
                value: Some(Value::Quote(Par::default())),
            },
        ];
        for signature in &cases {
            let expected = charged_signature_sort(signature, sort_signature_metered_source);
            let actual = charged_signature_sort(signature, sort_signature_metered);
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn metered_signature_depth_rejection_matches_source_charge_prefix() {
        let mut signature = CostSignature {
            value: Some(Value::Unit(true)),
        };
        for _ in 0..260 {
            signature = CostSignature {
                value: Some(Value::Compound(CostSignatureCompound {
                    elements: vec![signature],
                })),
            };
        }
        let expected = charged_signature_sort(&signature, sort_signature_metered_source);
        let actual = charged_signature_sort(&signature, sort_signature_metered);
        assert!(matches!(&expected.0, Err(BackingError::Rejected)));
        assert_eq!(actual, expected);
    }

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
    fn metered_cost_sorting_preserves_nested_signature_and_stack_order() {
        let signature = CostSignature {
            value: Some(Value::Compound(CostSignatureCompound {
                elements: vec![
                    CostSignature {
                        value: Some(Value::BoundLevel(7)),
                    },
                    CostSignature {
                        value: Some(Value::Compound(CostSignatureCompound {
                            elements: vec![
                                CostSignature {
                                    value: Some(Value::Ground(vec![3, 2, 1])),
                                },
                                CostSignature {
                                    value: Some(Value::Unit(true)),
                                },
                            ],
                        })),
                    },
                ],
            })),
        };
        let signed = CostSignedTerm {
            body: Some(Par::default()),
            signature: Some(signature.clone()),
        };
        let stack = CostStack {
            cells: vec![signature.clone(), signature.clone()],
            parallel_tail: Vec::new(),
        };
        let reserve = |_: usize, _: usize, _: usize| Ok(());
        let meter = SorterMeter::new(&reserve);
        assert_eq!(
            sort_signature_metered(&signature, &meter).unwrap(),
            sort_signature(&signature)
        );
        assert_eq!(
            sort_signed_term_metered(&signed, &meter).unwrap(),
            sort_signed_term(&signed)
        );
        assert_eq!(
            sort_stack_metered(&stack, &meter).unwrap(),
            sort_stack(&stack)
        );
    }

    #[test]
    fn metered_ground_signature_rejects_before_large_byte_copy() {
        let signature = CostSignature {
            value: Some(Value::Ground(vec![7; 4096])),
        };
        let reserve = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(BackingError::Rejected)
            } else {
                Ok(())
            }
        };
        let meter = SorterMeter::new(&reserve);
        assert!(matches!(
            sort_signature_metered(&signature, &meter),
            Err(BackingError::Rejected)
        ));
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
