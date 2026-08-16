use std::collections::HashMap;

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{
    Bundle, Connective, CostSignedTerm, CostStack, Expr, GPrivate, GUnforgeable, If, Match, New,
    Par, Receive, Send,
};
use models::rust::utils::union;

use super::env::Env;
use super::errors::InterpreterError;
use super::matcher::has_locally_free::HasLocallyFree;
use super::rho_type::{RhoExpression, RhoUnforgeable};

pub mod address_tools;
pub mod base58;
pub mod vault_address;
#[cfg(feature = "chromadb")]
pub mod sbert_embeddings;

// Helper enum. This is 'GeneratedMessage' in Scala
#[derive(Clone, Debug)]
pub enum GeneratedMessage {
    Send(Send),
    Receive(Receive),
    New(New),
    Match(Match),
    Bundle(Bundle),
    Expr(Expr),
    If(If),
    CostSignedTerm(CostSignedTerm),
    CostStack(CostStack),
}

pub fn evaluation_terms(par: &Par) -> Vec<GeneratedMessage> {
    vec![
        par.sends
            .iter()
            .cloned()
            .map(GeneratedMessage::Send)
            .collect::<Vec<_>>(),
        par.receives
            .iter()
            .cloned()
            .map(GeneratedMessage::Receive)
            .collect(),
        par.news
            .iter()
            .cloned()
            .map(GeneratedMessage::New)
            .collect(),
        par.matches
            .iter()
            .cloned()
            .map(GeneratedMessage::Match)
            .collect(),
        par.conditionals
            .iter()
            .cloned()
            .map(GeneratedMessage::If)
            .collect(),
        par.bundles
            .iter()
            .cloned()
            .map(GeneratedMessage::Bundle)
            .collect(),
        par.exprs
            .iter()
            .filter(|expr| {
                matches!(
                    expr.expr_instance,
                    Some(ExprInstance::EVarBody(_)) | Some(ExprInstance::EMethodBody(_))
                )
            })
            .cloned()
            .map(GeneratedMessage::Expr)
            .collect(),
        par.cost_signed_terms
            .iter()
            .cloned()
            .map(GeneratedMessage::CostSignedTerm)
            .collect(),
        par.cost_stacks
            .iter()
            .cloned()
            .map(GeneratedMessage::CostStack)
            .collect(),
    ]
    .into_iter()
    .flatten()
    .collect()
}

pub fn evaluation_random(
    rand: &Blake2b512Random,
    index: usize,
    term_count: usize,
) -> Result<Blake2b512Random, InterpreterError> {
    if term_count > i16::MAX as usize || index >= term_count {
        return Err(InterpreterError::ReduceError(format!(
            "The number of terms in the Par is {}, which exceeds the limit of {}",
            term_count,
            i16::MAX
        )));
    }
    if term_count == 1 {
        Ok(rand.clone())
    } else if term_count > 256 {
        Ok(rand.split_short(index as u16 as i16))
    } else {
        Ok(rand.split_byte(index as u8 as i8))
    }
}

pub fn allocate_new_bindings(
    new: &New,
    env: &Env<Par>,
    rand: &mut Blake2b512Random,
    urn_map: &HashMap<String, Par>,
) -> Result<Env<Par>, InterpreterError> {
    let bind_count = usize::try_from(new.bind_count).map_err(|_| {
        InterpreterError::ReduceError("new binding count cannot be negative".to_string())
    })?;
    let simple_count = bind_count.checked_sub(new.uri.len()).ok_or_else(|| {
        InterpreterError::ReduceError(
            "new URI binding count exceeds the total binding count".to_string(),
        )
    })?;
    let mut result = env.clone();
    for _ in 0..simple_count {
        result = result.put(Par::default().with_unforgeables(vec![GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                id: rand.next().into_iter().map(|byte| byte as u8).collect(),
            })),
        }]));
    }
    for urn in &new.uri {
        let value = match urn_map.get(urn) {
            Some(value) => value.clone(),
            None => match new.injections.get(urn) {
                Some(value) => {
                    if let Some(unforgeable) = RhoUnforgeable::unapply(value) {
                        let instance = unforgeable.unf_instance.ok_or_else(|| {
                            InterpreterError::BugFoundError(
                                "unf_instance field is None".to_string(),
                            )
                        })?;
                        Par::default().with_unforgeables(vec![GUnforgeable {
                            unf_instance: Some(instance),
                        }])
                    } else if let Some(expression) = RhoExpression::unapply(value) {
                        let instance = expression.expr_instance.ok_or_else(|| {
                            InterpreterError::BugFoundError(
                                "expr_instance field is None".to_string(),
                            )
                        })?;
                        Par::default().with_exprs(vec![Expr {
                            expr_instance: Some(instance),
                        }])
                    } else {
                        return Err(InterpreterError::BugFoundError(
                            "invalid injection".to_string(),
                        ));
                    }
                }
                None => {
                    return Err(InterpreterError::BugFoundError(format!(
                    "No value set for {}. This is a bug in the normalizer or on the path from it.",
                    urn
                )))
                }
            },
        };
        result = result.put(value);
    }
    Ok(result)
}

// These two functions need to be under 'rholang' dir because of HasLocallyFree Trait.
// This trait should, I think, be moved to models

// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala - prepend
pub fn prepend_connective(mut p: Par, c: Connective, depth: i32) -> Par {
    let mut new_connectives = vec![c.clone()];
    new_connectives.append(&mut p.connectives);

    Par {
        connectives: new_connectives,
        locally_free: c.locally_free(c.clone(), depth),
        connective_used: p.connective_used || c.clone().connective_used(c),
        ..p.clone()
    }
}

pub fn prepend_expr(mut p: Par, e: Expr, depth: i32) -> Par {
    let mut new_exprs = vec![e.clone()];
    new_exprs.append(&mut p.exprs);

    Par {
        exprs: new_exprs,
        locally_free: union(p.locally_free.clone(), e.locally_free(e.clone(), depth)),
        connective_used: p.connective_used || e.clone().connective_used(e),
        ..p.clone()
    }
}

pub fn prepend_new(mut p: Par, n: New) -> Par {
    let mut new_news = vec![n.clone()];
    new_news.append(&mut p.news);

    Par {
        news: new_news,
        locally_free: union(p.locally_free.clone(), n.clone().locally_free),
        connective_used: p.connective_used || n.clone().connective_used(n),
        ..p.clone()
    }
}

pub fn prepend_bundle(mut p: Par, b: Bundle) -> Par {
    let mut new_bundles = vec![b.clone()];
    new_bundles.append(&mut p.bundles);

    Par {
        bundles: new_bundles,
        locally_free: union(p.locally_free.clone(), b.body.unwrap().locally_free),
        ..p.clone()
    }
}

// for locally_free parameter, in case when we have (bodyResult.par.locallyFree.from(boundCount).map(x => x - boundCount))
pub(crate) fn filter_and_adjust_bitset(bitset: Vec<u8>, bound_count: usize) -> Vec<u8> {
    bitset
        .into_iter()
        .enumerate()
        .filter_map(|(i, _)| {
            if i >= bound_count {
                Some(i as u8 - bound_count as u8)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluation_random_covers_the_complete_byte_index_domain() {
        let random = Blake2b512Random::create_from_bytes(&[7; 64]);
        for index in [0, 127, 128, 255] {
            assert_eq!(
                evaluation_random(&random, index, 256).unwrap(),
                random.split_byte(index as u8 as i8)
            );
        }
    }

    #[test]
    fn evaluation_random_switches_to_short_indices_above_256_terms() {
        let random = Blake2b512Random::create_from_bytes(&[11; 64]);
        for index in [0, 255, 256, i16::MAX as usize - 1] {
            assert_eq!(
                evaluation_random(&random, index, i16::MAX as usize).unwrap(),
                random.split_short(index as u16 as i16)
            );
        }
    }

    #[test]
    fn evaluation_random_rejects_out_of_range_indices_and_term_counts() {
        let random = Blake2b512Random::create_from_bytes(&[13; 64]);
        assert!(evaluation_random(&random, 1, 1).is_err());
        assert!(evaluation_random(&random, 0, i16::MAX as usize + 1).is_err());
    }
}
