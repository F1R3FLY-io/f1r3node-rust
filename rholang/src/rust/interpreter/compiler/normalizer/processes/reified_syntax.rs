//! Thin adapters from the tree-sitter AST to the frontend-neutral builders of the Reified
//! RSpaces syntax (plan decisions 18 and 21).
//!
//! Each adapter normalizes the parts of a construct in the scope that the syntax contract gives
//! them, and builds the marker tuple only through `models::rust::reified::builders`. The
//! builders re-check every guarantee of the parser: each key is a unique param key, no
//! positional param follows a named one, a query takes named params only and is never empty,
//! and a URI binder takes no space annotation. The adapters apply `validate_param_value` to each
//! literal named param value. `check_query_source` re-checks that a query has a simple source.

use std::collections::HashMap;

use models::rhoapi::{New, Par};
use models::rust::reified::builders::{new_space, push_process_expr, query_spec, send_params};
use models::rust::reified::params::SendParam;
use models::rust::reified::validation::{validate_param_value, ReifiedSyntaxError};
use rholang_parser::ast::{
    AnnProc, Bind, Hyperparam, HyperparamList, Name, NameDecl, Proc, QuerySpec, SendType, Source,
};
use rholang_parser::parser::errors::QUERY_NON_SIMPLE_SOURCE;
use rholang_parser::RholangParser;

use crate::rust::interpreter::compiler::exports::{
    BoundMapChain, FreeMap, NameVisitInputs, ProcVisitInputs,
};
use crate::rust::interpreter::compiler::normalize::{normalize_ann_proc, VarSort};
use crate::rust::interpreter::compiler::normalizer::name_normalize_matcher::normalize_name;
use crate::rust::interpreter::errors::InterpreterError;

/// The normalizer error for Reified syntax that a builder or a validator rejects.
pub fn reified_syntax_error(error: ReifiedSyntaxError) -> InterpreterError {
    InterpreterError::NormalizerError(format!("reified syntax: {error}"))
}

/// Normalizes `procs` in the current scope, in source order.
pub fn normalize_procs<'ast>(
    procs: &[AnnProc<'ast>],
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(Vec<Par>, FreeMap<VarSort>), InterpreterError> {
    let mut pars = Vec::with_capacity(procs.len());
    let mut free_map = free_map;
    for proc in procs {
        let result = normalize_ann_proc(
            proc,
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: bound_map_chain.clone(),
                free_map,
            },
            env,
            parser,
        )?;
        pars.push(result.par);
        free_map = result.free_map;
    }
    Ok((pars, free_map))
}

/// True when `proc` is a literal, whose normalized `Par` is also its evaluated value.
fn is_literal(proc: &Proc) -> bool {
    matches!(
        proc,
        Proc::Nil
            | Proc::Unit
            | Proc::BoolLiteral(_)
            | Proc::LongLiteral(_)
            | Proc::SignedIntLiteral { .. }
            | Proc::UnsignedIntLiteral { .. }
            | Proc::BigIntLiteral(_)
            | Proc::BigRatLiteral(_)
            | Proc::FloatLiteral { .. }
            | Proc::FixedPointLiteral { .. }
            | Proc::StringLiteral(_)
            | Proc::UriLiteral(_)
    )
}

/// Normalizes the values of one hyperparameter section in the current scope, in source order.
/// A named param with a literal value must be in the range of its key.
pub fn normalize_hyperparams<'ast>(
    params: &[Hyperparam<'ast>],
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(Vec<SendParam>, FreeMap<VarSort>), InterpreterError> {
    let mut normalized = Vec::with_capacity(params.len());
    let mut free_map = free_map;
    for param in params {
        let result = normalize_ann_proc(
            param.value(),
            ProcVisitInputs {
                par: Par::default(),
                bound_map_chain: bound_map_chain.clone(),
                free_map,
            },
            env,
            parser,
        )?;
        free_map = result.free_map;
        normalized.push(match param {
            Hyperparam::Positional(_) => SendParam::Positional(result.par),
            Hyperparam::Named { key, value } => {
                if is_literal(value.proc) {
                    validate_param_value(key.name, &result.par).map_err(reified_syntax_error)?;
                }
                SendParam::Named(key.name.to_owned(), result.par)
            }
        });
    }
    Ok((normalized, free_map))
}

/// The trailing PARAMS data element of a send `x!(d…; p…, k=v…)`, or `None` when the send has
/// no `;` section or an empty one. The values follow the data in the current scope.
pub fn normalize_send_params<'ast>(
    params: Option<&HyperparamList<'ast>>,
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(Option<Par>, FreeMap<VarSort>), InterpreterError> {
    match params {
        None => Ok((None, free_map)),
        Some(params) => {
            let (params, free_map) =
                normalize_hyperparams(params, bound_map_chain, free_map, env, parser)?;
            let trailing = send_params(params).map_err(reified_syntax_error)?;
            Ok((trailing, free_map))
        }
    }
}

/// The source of a receive bind with the channel `channel`: the channel itself, or the QUERY
/// tuple `(QUERY, ch, [q…], {k: v…})` when the bind has a query. The payload and then the params
/// follow the channel in the scope outside the receive.
pub fn normalize_bind_source<'ast>(
    channel: Par,
    query: Option<&QuerySpec<'ast>>,
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(Par, FreeMap<VarSort>), InterpreterError> {
    let Some(query) = query else {
        return Ok((channel, free_map));
    };
    let (payload, free_map) =
        normalize_procs(&query.payload, bound_map_chain, free_map, env, parser)?;
    let (params, free_map) = match &query.params {
        Some(params) => normalize_hyperparams(params, bound_map_chain, free_map, env, parser)?,
        None => (Vec::new(), free_map),
    };
    let source = query_spec(channel, payload, params).map_err(reified_syntax_error)?;
    Ok((source, free_map))
}

/// Fails when a receive query is on a `!?`, `?!` or `!m` source, whose desugaring receives on a
/// fresh reply channel.
pub fn check_query_source(bind: &Bind) -> Result<(), InterpreterError> {
    match bind {
        Bind::Linear {
            rhs: Source::ReceiveSend { .. } | Source::SendReceive { .. },
            query_spec: Some(_),
            ..
        } => Err(InterpreterError::NormalizerError(format!(
            "reified syntax: {QUERY_NON_SIMPLE_SOURCE}"
        ))),
        Bind::Linear { .. } | Bind::Repeated { .. } | Bind::Peek { .. } | Bind::Signed { .. } => {
            Ok(())
        }
    }
}

/// Allocates the send of a `!?` desugaring: `channel!(inputs)`, or `channel!(inputs; params)`
/// when the source text has a `;` section.
pub fn alloc_desugared_send<'ast>(
    parser: &'ast RholangParser<'ast>,
    channel: Name<'ast>,
    inputs: &[AnnProc<'ast>],
    params: Option<&HyperparamList<'ast>>,
) -> &'ast Proc<'ast> {
    match params {
        Some(params) => parser.ast_builder().alloc_send_with_params(
            SendType::Single,
            channel,
            inputs,
            params.clone(),
        ),
        None => parser
            .ast_builder()
            .alloc_send(SendType::Single, channel, inputs),
    }
}

/// The space annotations of the binders of a `new`, one entry per binder in binding order, or
/// `None` when no binder has one. The binding order of `normalize_p_new` lists the plain binders
/// in source order and then the URI binders sorted by URI. Each annotation is a name in the
/// scope outside the `new`.
pub fn normalize_space_annotations<'ast>(
    decls: &[NameDecl<'ast>],
    bound_map_chain: &BoundMapChain<VarSort>,
    free_map: FreeMap<VarSort>,
    env: &HashMap<String, Par>,
    parser: &'ast RholangParser<'ast>,
) -> Result<(Option<Vec<Option<Par>>>, FreeMap<VarSort>), InterpreterError> {
    if decls.iter().all(|decl| decl.space.is_none()) {
        return Ok((None, free_map));
    }
    let mut binders: Vec<&NameDecl<'ast>> = decls.iter().collect();
    binders.sort_by(|left, right| left.uri.as_deref().cmp(&right.uri.as_deref()));
    let mut annotations = Vec::with_capacity(binders.len());
    let mut free_map = free_map;
    for binder in binders {
        annotations.push(match &binder.space {
            Some(space) => {
                let result = normalize_name(
                    space,
                    NameVisitInputs {
                        bound_map_chain: bound_map_chain.clone(),
                        free_map,
                    },
                    env,
                    parser,
                )?;
                free_map = result.free_map;
                Some(result.par)
            }
            None => None,
        });
    }
    Ok((Some(annotations), free_map))
}

/// Adds the NEWSPACE tuple of `new` and its binder annotations in front of the expressions of
/// `par`.
pub fn push_space_annotated_new(
    par: Par,
    new: New,
    annotations: Vec<Option<Par>>,
) -> Result<Par, InterpreterError> {
    let tuple = new_space(new, annotations).map_err(reified_syntax_error)?;
    Ok(push_process_expr(par, tuple))
}
