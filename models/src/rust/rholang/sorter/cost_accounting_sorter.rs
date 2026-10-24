use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};

pub fn sort_signature(signature: &CostSignature) -> ScoredTerm<CostSignature> {
    super::sort_drive::sort_signature(signature)
}

pub(crate) fn combine_signature(
    signature: &CostSignature,
    par: Option<ScoredTerm<Par>>,
    mut elements: Vec<ScoredTerm<CostSignature>>,
) -> ScoredTerm<CostSignature> {
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
        Some(Value::Quote(_)) => {
            let sorted = par.expect("signature Par child");
            ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Quote(sorted.term)),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_QUOTE, vec![
                    sorted.score,
                ]),
            }
        }
        Some(Value::Compound(_)) => {
            elements.retain(|element| !matches!(element.term.value, Some(Value::Unit(_))));
            if elements.is_empty() {
                return ScoredTerm {
                    term: CostSignature {
                        value: Some(Value::Unit(true)),
                    },
                    score: Tree::<ScoreAtom>::create_node_from_i32(
                        Score::COST_SIG_UNIT,
                        Vec::new(),
                    ),
                };
            }
            if elements.len() == 1 {
                return elements.pop().expect("one cost-signature element");
            }
            ScoredTerm::sort_vec(&mut elements);
            let (terms, scores) = super::sort_combine::split_scored_terms(elements);
            ScoredTerm {
                term: CostSignature {
                    value: Some(Value::Compound(CostSignatureCompound { elements: terms })),
                },
                score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_COMPOUND, scores),
            }
        }
        Some(Value::Name(_)) => {
            let sorted = par.expect("signature Par child");
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
// order, as the recursive version gave them. That order matters, because
// `ScoredTerm::sort_vec` keeps ties in input order. The recursive version
// below stays as the test oracle.
#[cfg(test)]
fn collect_compound(signatures: &[CostSignature], output: &mut Vec<ScoredTerm<CostSignature>>) {
    let mut work: Vec<&CostSignature> = signatures.iter().rev().collect();
    while let Some(signature) = work.pop() {
        match &signature.value {
            Some(Value::Compound(compound)) => work.extend(compound.elements.iter().rev()),
            _ => output.push(sort_signature(signature)),
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
            _ => output.push(sort_signature(signature)),
        }
    }
}

pub fn sort_signed_term(term: &CostSignedTerm) -> ScoredTerm<CostSignedTerm> {
    super::sort_drive::sort_signed_term(term)
}

pub(crate) fn combine_signed_term(
    term: &CostSignedTerm,
    body: Option<ScoredTerm<Par>>,
    signature: Option<ScoredTerm<CostSignature>>,
) -> ScoredTerm<CostSignedTerm> {
    let body = body.unwrap_or_else(|| ScoredTerm {
        term: Par::default(),
        score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
    });
    let signature = signature.unwrap_or_else(|| ScoredTerm {
        term: CostSignature::default(),
        score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
    });
    let mut scores = Vec::with_capacity(3);
    scores.push(signature.score);
    scores.push(body.score);
    if let Some(price) = term.held_price {
        scores.push(Tree::<ScoreAtom>::create_leaf_from_bytes(
            price.to_be_bytes().to_vec(),
        ));
    }
    ScoredTerm {
        term: CostSignedTerm {
            body: term.body.as_ref().map(|_| body.term),
            signature: term.signature.as_ref().map(|_| signature.term),
            held_price: term.held_price,
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIGNED_TERM, scores),
    }
}

pub fn sort_stack(stack: &CostStack) -> ScoredTerm<CostStack> {
    super::sort_drive::sort_stack(stack)
}

pub(crate) fn combine_stack(cells: Vec<ScoredTerm<CostSignature>>) -> ScoredTerm<CostStack> {
    let (cells, scores) = super::sort_combine::split_scored_terms(cells);
    ScoredTerm {
        term: CostStack { cells },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_STACK, scores),
    }
}

// Added by DR-131 P1: the stack-safety tests of the cost-signature sort.
#[cfg(test)]
mod stack_safety_tests;
