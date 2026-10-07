use std::collections::HashMap;

use models::rhoapi::cost_signature::Value as CostSignatureValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{CostSignature, CostSignatureCompound, Expr, Par};
use models::rust::rholang::sorter::cost_accounting_sorter::sort_signature;
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use models::rust::utils::union;
use prost::Message;
use rholang_parser::ast::{Name, Signature, Var};
use rholang_parser::{RholangParser, SourceSpan};

use crate::rust::interpreter::compiler::bound_map_chain::BoundMapChain;
use crate::rust::interpreter::compiler::free_map::FreeMap;
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, ProcVisitInputs, VarSort};
use crate::rust::interpreter::errors::InterpreterError;

pub fn signature_to_wire<'ast>(
    sig: &Signature<'ast>,
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(CostSignature, FreeMap<VarSort>), InterpreterError> {
    match sig {
        Signature::Ground(name) => match name {
            Name::NameVar(Var::Id(id)) => match bound_map_chain.get(id.name) {
                Some(ctx) if ctx.typ == VarSort::NameSort => Ok((
                    CostSignature {
                        value: Some(CostSignatureValue::BoundLevel(ctx.index as i32)),
                    },
                    free_map,
                )),
                Some(ctx) => Err(InterpreterError::UnexpectedNameContext {
                    var_name: id.name.to_string(),
                    proc_var_source_span: ctx.source_span,
                    name_source_span: SourceSpan {
                        start: id.pos,
                        end: id.pos,
                    },
                }),
                None => Ok((
                    CostSignature {
                        value: Some(CostSignatureValue::Ground(canon_ground(id.name))),
                    },
                    free_map,
                )),
            },
            Name::NameVar(Var::Wildcard) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a wildcard `_` is not a valid ground signature".to_string(),
            )),
            Name::Quote(_) => Err(InterpreterError::NormalizerError(
                "cost-accounting: a quoted-principal ground signature `@P` is not supported"
                    .to_string(),
            )),
        },
        Signature::Hash(proc) => {
            let normalized = normalize_ann_proc(
                proc,
                ProcVisitInputs {
                    par: Par::default(),
                    bound_map_chain: bound_map_chain.clone(),
                    free_map,
                },
                env,
                parser,
            )?;
            Ok((
                CostSignature {
                    value: Some(CostSignatureValue::Quote(
                        ParSortMatcher::sort_match(&normalized.par).term,
                    )),
                },
                normalized.free_map,
            ))
        }
        Signature::Compound(left, right) => {
            let (left, free_map) = signature_to_wire(left, bound_map_chain, free_map, env, parser)?;
            let (right, free_map) =
                signature_to_wire(right, bound_map_chain, free_map, env, parser)?;
            let signature = CostSignature {
                value: Some(CostSignatureValue::Compound(CostSignatureCompound {
                    elements: vec![left, right],
                })),
            };
            Ok((sort_signature(&signature).term, free_map))
        }
        Signature::Transfer(_, _) => Err(InterpreterError::NormalizerError(
            "cost-accounting: a lollipop `-o` cannot appear in a fundable signature".to_string(),
        )),
    }
}

pub fn wire_signature_locally_free(signature: &CostSignature) -> Vec<u8> {
    match &signature.value {
        Some(CostSignatureValue::BoundLevel(level)) if *level >= 0 => {
            models::create_bit_vector(&[*level as usize])
        }
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
            par.locally_free.clone()
        }
        Some(CostSignatureValue::Compound(compound)) => compound
            .elements
            .iter()
            .map(wire_signature_locally_free)
            .fold(Vec::new(), union),
        _ => Vec::new(),
    }
}

pub fn wire_signature_connective_used(signature: &CostSignature) -> bool {
    match &signature.value {
        Some(CostSignatureValue::Quote(par)) | Some(CostSignatureValue::Name(par)) => {
            par.connective_used
        }
        Some(CostSignatureValue::Compound(compound)) => {
            compound.elements.iter().any(wire_signature_connective_used)
        }
        _ => false,
    }
}

pub fn canon_ground(name: &str) -> Vec<u8> {
    let par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(name.to_string())),
    }]);
    ParSortMatcher::sort_match(&par).term.encode_to_vec()
}
