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
