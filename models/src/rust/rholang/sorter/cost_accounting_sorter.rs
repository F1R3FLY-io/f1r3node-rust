use super::par_sort_matcher::ParSortMatcher;
use super::score_tree::{Score, ScoreAtom, ScoredTerm, Tree};
use super::sortable::Sortable;
use crate::rhoapi::cost_signature::Value;
use crate::rhoapi::{CostSignature, CostSignatureCompound, CostSignedTerm, CostStack, Par};

fn absent<T: Default>() -> ScoredTerm<T> {
    ScoredTerm {
        term: T::default(),
        score: Tree::<ScoreAtom>::create_leaf_from_i64(Score::ABSENT as i64),
    }
}

fn unit() -> ScoredTerm<CostSignature> {
    ScoredTerm {
        term: CostSignature {
            value: Some(Value::Unit(true)),
        },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_UNIT, Vec::new()),
    }
}

fn leaves(signatures: &[CostSignature]) -> Vec<&CostSignature> {
    let mut work: Vec<&CostSignature> = signatures.iter().rev().collect();
    let mut output = Vec::new();
    while let Some(signature) = work.pop() {
        match &signature.value {
            Some(Value::Compound(compound)) => work.extend(compound.elements.iter().rev()),
            _ => output.push(signature),
        }
    }
    output
}

fn split<T>(scored: Vec<ScoredTerm<T>>) -> (Vec<T>, Vec<Tree<ScoreAtom>>) {
    scored.into_iter().map(|s| (s.term, s.score)).unzip()
}

pub fn sort_signature(signature: &CostSignature) -> ScoredTerm<CostSignature> {
    match &signature.value {
        None => absent(),
        Some(Value::Ground(bytes)) => ScoredTerm {
            term: CostSignature {
                value: Some(Value::Ground(bytes.clone())),
            },
            score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_SIG_GROUND, vec![
                Tree::<ScoreAtom>::create_leaf_from_bytes(bytes.clone()),
            ]),
        },
        Some(Value::Unit(_)) => unit(),
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
        Some(Value::Compound(compound)) => {
            let mut elements: Vec<ScoredTerm<CostSignature>> = leaves(&compound.elements)
                .into_iter()
                .map(sort_signature)
                .filter(|element| !matches!(element.term.value, Some(Value::Unit(_))))
                .collect();
            match elements.len() {
                0 => unit(),
                1 => elements.pop().unwrap_or_else(unit),
                _ => {
                    ScoredTerm::sort_vec(&mut elements);
                    let (terms, scores) = split(elements);
                    ScoredTerm {
                        term: CostSignature {
                            value: Some(Value::Compound(CostSignatureCompound {
                                elements: terms,
                            })),
                        },
                        score: Tree::<ScoreAtom>::create_node_from_i32(
                            Score::COST_SIG_COMPOUND,
                            scores,
                        ),
                    }
                }
            }
        }
    }
}

pub fn sort_signed_term(term: &CostSignedTerm) -> ScoredTerm<CostSignedTerm> {
    let body: ScoredTerm<Par> = term
        .body
        .as_ref()
        .map(ParSortMatcher::sort_match)
        .unwrap_or_else(absent);
    let signature = term
        .signature
        .as_ref()
        .map(sort_signature)
        .unwrap_or_else(absent);
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
    let (cells, scores) = split(stack.cells.iter().map(sort_signature).collect());
    ScoredTerm {
        term: CostStack { cells },
        score: Tree::<ScoreAtom>::create_node_from_i32(Score::COST_STACK, scores),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::Expr;

    fn ground(b: u8) -> CostSignature {
        CostSignature {
            value: Some(Value::Ground(vec![b])),
        }
    }

    fn compound(elements: Vec<CostSignature>) -> CostSignature {
        CostSignature {
            value: Some(Value::Compound(CostSignatureCompound { elements })),
        }
    }

    fn unit_sig() -> CostSignature {
        CostSignature {
            value: Some(Value::Unit(true)),
        }
    }

    #[test]
    fn compounds_are_commutative_and_associative() {
        let left = compound(vec![ground(1), compound(vec![ground(2), ground(3)])]);
        let right = compound(vec![compound(vec![ground(3), ground(1)]), ground(2)]);
        assert_eq!(sort_signature(&left).term, sort_signature(&right).term);
        assert_eq!(
            sort_signature(&left).term,
            compound(vec![ground(1), ground(2), ground(3)])
        );
    }

    #[test]
    fn unit_is_the_identity_of_a_compound() {
        assert_eq!(
            sort_signature(&compound(vec![unit_sig(), ground(7)])).term,
            ground(7)
        );
        assert_eq!(
            sort_signature(&compound(vec![unit_sig(), unit_sig()])).term,
            unit_sig()
        );
    }

    #[test]
    fn a_stack_keeps_its_cell_order() {
        let stack = CostStack {
            cells: vec![ground(2), ground(1)],
        };
        assert_eq!(sort_stack(&stack).term.cells, vec![ground(2), ground(1)]);
    }

    #[test]
    fn a_signed_term_sorts_its_body() {
        let e = |n| Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        };
        let term = CostSignedTerm {
            body: Some(Par::default().with_exprs(vec![e(2), e(1)])),
            signature: Some(ground(1)),
        };
        assert_eq!(
            sort_signed_term(&term).term.body,
            Some(Par::default().with_exprs(vec![e(1), e(2)]))
        );
    }

    #[test]
    fn a_compound_nested_two_thousand_deep_sorts() {
        let mut signature = ground(0);
        for n in 1..2_000u32 {
            signature = compound(vec![signature, ground((n % 251) as u8)]);
        }
        let sorted = sort_signature(&signature).term;
        match sorted.value {
            Some(Value::Compound(compound)) => assert_eq!(compound.elements.len(), 2_000),
            other => panic!("expected a compound, found {other:?}"),
        }
    }
}
