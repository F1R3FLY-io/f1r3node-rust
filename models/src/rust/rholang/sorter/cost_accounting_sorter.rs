use shared::rust::clone_backing::BackingError;

use super::metered::SorterMeter;
use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};

pub fn sort_signature(signature: &CostSignature) -> ScoredTerm<CostSignature> {
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
            elements.retain(|element| !matches!(element.term.value, Some(Value::Unit(_))));
            if elements.is_empty() {
                return sort_signature(&CostSignature {
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

fn collect_compound(signatures: &[CostSignature], output: &mut Vec<ScoredTerm<CostSignature>>) {
    for signature in signatures {
        match &signature.value {
            Some(Value::Compound(compound)) => collect_compound(&compound.elements, output),
            _ => output.push(sort_signature(signature)),
        }
    }
}

pub fn sort_signed_term(term: &CostSignedTerm) -> ScoredTerm<CostSignedTerm> {
    let body = term
        .body
        .as_ref()
        .map(ParSortMatcher::sort_match)
        .unwrap_or_else(|| ScoredTerm {
            term: Par::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        });
    let signature = term
        .signature
        .as_ref()
        .map(sort_signature)
        .unwrap_or_else(|| ScoredTerm {
            term: CostSignature::default(),
            score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
        });
    ScoredTerm {
        term: CostSignedTerm {
            body: term.body.as_ref().map(|_| body.term),
            signature: term.signature.as_ref().map(|_| signature.term),
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIGNED_TERM, vec![
            signature.score,
            body.score,
        ]),
    }
}

pub fn sort_stack(stack: &CostStack) -> ScoredTerm<CostStack> {
    let cells: Vec<_> = stack.cells.iter().map(sort_signature).collect();
    ScoredTerm {
        term: CostStack {
            cells: cells.iter().map(|cell| cell.term.clone()).collect(),
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(
            Score::COST_STACK,
            cells.into_iter().map(|cell| cell.score).collect(),
        ),
    }
}

fn scored_node(
    kind: i32,
    child: Tree<ScoreAtom>,
    meter: &SorterMeter<'_>,
) -> Result<Tree<ScoreAtom>, BackingError> {
    // Changed by D-E4 (DR-111): Rule S, one read of each slot.
    // let mut children = meter.vec(1)?;
    let mut children = meter.score_vec(1)?;
    children.push(child);
    Tree::<ScoreAtom>::create_node_from_i32_metered(kind, children, meter)
}

pub fn sort_signature_metered(
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
                // Changed by D-O1 (DR-111): block accounting.
                // value: Some(Value::Ground(meter.clone(bytes)?)),
                value: Some(Value::Ground(meter.clone_blocks(bytes)?)),
            },
            score: scored_node(
                Score::COST_SIG_GROUND,
                // Changed by D-O1 (DR-111): block accounting.
                // Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone(bytes)?),
                Tree::<ScoreAtom>::create_leaf_from_bytes(meter.clone_blocks(bytes)?),
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
            elements.retain(|element| !matches!(element.term.value, Some(Value::Unit(_))));
            if elements.is_empty() {
                return sort_signature_metered(
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
            // Changed by D-E4 (DR-111): Rule S, one read of each slot.
            // let mut scores = meter.vec(elements.len())?;
            let mut scores = meter.score_vec(elements.len())?;
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
            _ => meter.push(output, sort_signature_metered(signature, meter)?)?,
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
    // Changed by D-E4 (DR-111): Rule S, one read of each slot.
    // let mut scores = meter.vec(2)?;
    let mut scores = meter.score_vec(2)?;
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
    meter.reserve(1, std::mem::size_of::<CostStack>(), 0)?;
    let mut cells = meter.vec(stack.cells.len())?;
    for cell in &stack.cells {
        cells.push(sort_signature_metered(cell, meter)?);
    }
    let mut terms = meter.vec(cells.len())?;
    // Changed by D-E4 (DR-111): Rule S, one read of each slot.
    // let mut scores = meter.vec(cells.len())?;
    let mut scores = meter.score_vec(cells.len())?;
    for cell in cells {
        terms.push(cell.term);
        scores.push(cell.score);
    }
    Ok(ScoredTerm {
        term: CostStack { cells: terms },
        score: Tree::<ScoreAtom>::create_node_from_i32_metered(Score::COST_STACK, scores, meter)?,
    })
}

#[cfg(test)]
mod metered_tests {
    use super::*;

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
}
