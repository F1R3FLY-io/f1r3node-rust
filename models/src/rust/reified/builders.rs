// Changed by decision 112: the builders emit the protobuf fields. The text before:
// ```
// //! Builders of the Reified marker tuples for the frontends.
// //!
// //! Each builder checks the syntax that a forged tuple could otherwise express, then emits the
// //! canonical encoding. The `locally_free` and `connective_used` fields follow the normalizer:
// //! a tuple or a list takes the union and the disjunction of its elements, a map comes from
// //! `ParMap::create_from_vec`, `Par{news: [New]}` follows `prepend_new`, and
// //! `push_process_expr` follows `prepend_expr`. The frontend applies
// //! `validation::validate_param_value` to each literal param value.
// ```
//! Builders of the Reified protobuf fields for the frontends.
//!
//! Each builder checks the syntax that a forged field could otherwise express, then emits the
//! canonical field. The `locally_free` and `connective_used` fields follow the normalizer: the
//! `push_` builders take the union and the disjunction of the values that they add, as
//! `prepend_expr` does. The frontend applies `validation::validate_param_value` to each literal
//! param value.

// Changed by decision 112: the builders emit the fields, so the tuple builders have no use.
// The text before:
// ```
// use super::encoding::{
//     free_tuple, new_space_tuple, params_tuple, query_tuple, tuple_par, use_tuple, SendParams,
// };
// ```
use super::encoding::{efree_message, ReifiedEncodeError};
use super::params::{collect_send_params, SendParam};
use super::query_spec::collect_query_params;
use super::validation::ReifiedSyntaxError;
use crate::rhoapi::expr::ExprInstance;
// Changed by decision 112: the builders return the generated messages. The text before:
// use crate::rhoapi::{ETuple, Expr, New, Par};
use crate::rhoapi::{EFree, Expr, New, Par, QuerySpec, SendParams, SpaceAnnotation, UseBlock};
use crate::rust::utils::union;

// Changed by decision 112: the use block is a field of `Par`. The text before:
// ```
// /// `use s { P }`: the USE tuple for `Par.exprs`.
// pub fn use_block(space: Par, body: Par) -> ETuple { use_tuple(space, body) }
// ```
/// `use s { P }`: the use block of a `Par`.
pub fn use_block(space: Par, body: Par) -> UseBlock {
    UseBlock {
        space: Some(space),
        body: Some(body),
    }
}

// Changed by decision 112: the annotations are a field of `New`. The text before:
// /// `new x_1[: s_1], … in P` with at least one annotation: the NEWSPACE tuple for `Par.exprs`.
/// `new x_1[: s_1], … in P` with at least one annotation: the `New` with its `space_types`.
///
/// `new` is the normalized `New`. `binder_annotations` holds one entry for each binder in
/// binding order: the plain binders in source order, then the URI binders. The annotations
/// are normalized in the scope outside the `new`. A URI binder takes no annotation.
// Changed by decision 112: the annotations are a field of `New`. The text before:
// ```
// pub fn new_space(
//     new: New,
//     binder_annotations: Vec<Option<Par>>,
// ) -> Result<ETuple, ReifiedSyntaxError> {
// ```
pub fn new_space(
    new: New,
    binder_annotations: Vec<Option<Par>>,
) -> Result<New, ReifiedSyntaxError> {
    let binders = binder_annotations.len();
    let plain = match (
        usize::try_from(new.bind_count),
        binders.checked_sub(new.uri.len()),
    ) {
        (Ok(bind_count), Some(plain)) if bind_count == binders => plain,
        (Ok(_) | Err(_), Some(_) | None) => {
            return Err(ReifiedSyntaxError::AnnotationCount {
                annotations: binders,
                bind_count: new.bind_count,
                uris: new.uri.len(),
            })
        }
    };
    let mut annotations = binder_annotations;
    match annotations.get(plain..) {
        Some(uri_annotations) if uri_annotations.iter().any(Option::is_some) => {
            return Err(ReifiedSyntaxError::SpaceTypeOnUriBinding)
        }
        Some(_) | None => annotations.truncate(plain),
    }
    // Changed by decision 112: the no-annotation check moves here from the tuple builder, and
    // the annotations become the `space_types` of the `New`. The text before:
    // Ok(new_space_tuple(&annotations, new)?)
    match annotations.iter().any(Option::is_some) {
        true => {}
        false => return Err(ReifiedEncodeError::NoAnnotation.into()),
    }
    let mut new = new;
    new.space_types = annotations
        .into_iter()
        .map(|space| SpaceAnnotation { space })
        .collect();
    for annotation in &new.space_types {
        if let Some(space) = &annotation.space {
            new.locally_free = union(new.locally_free, space.locally_free.clone());
        }
    }
    Ok(new)
}

// Changed by decision 112: the params are the `params` field of the send. The text before:
// ```
// /// `x!(d…; p…, k=v…)`: the trailing data element of the send, or `None` when the send has
// /// no params. `x!(a;)` thus builds the same data as `x!(a)`.
// pub fn send_params(params: Vec<SendParam>) -> Result<Option<Par>, ReifiedSyntaxError> {
//     let SendParams { positional, named } = collect_send_params(params)?;
//     if positional.is_empty() && named.is_empty() {
//         return Ok(None);
//     }
//     Ok(Some(tuple_par(params_tuple(positional, &named)?)))
// }
// ```
/// `x!(d…; p…, k=v…)`: the `params` field of the send, or `None` when the send has no params.
/// `x!(a;)` thus builds the same term as `x!(a)`.
pub fn send_params(params: Vec<SendParam>) -> Result<Option<SendParams>, ReifiedSyntaxError> {
    let SendParams { positional, named } = collect_send_params(params)?;
    match (positional.is_empty(), named.is_empty()) {
        (true, true) => Ok(None),
        _ => Ok(Some(SendParams { positional, named })),
    }
}

// Changed by decision 112: the query is the `query_spec` field of the bind. The text before:
// ```
// /// `for (y <- ch ~ (q…; k=v…)) P`: the bind source. A bare `ch ~ e` has the payload `[e]`.
// pub fn query_spec(
//     channel: Par,
//     payload: Vec<Par>,
//     params: Vec<SendParam>,
// ) -> Result<Par, ReifiedSyntaxError> {
//     let named = collect_query_params(params)?;
//     Ok(tuple_par(query_tuple(channel, payload, &named)?))
// }
// ```
/// `for (y <- ch ~ (q…; k=v…)) P`: the `query_spec` field of the bind. A bare `ch ~ e` has
/// the payload `[e]`.
pub fn query_spec(
    payload: Vec<Par>,
    params: Vec<SendParam>,
) -> Result<QuerySpec, ReifiedSyntaxError> {
    let named = collect_query_params(params)?;
    match (payload.is_empty(), named.is_empty()) {
        (true, true) => Err(ReifiedEncodeError::EmptyQuery.into()),
        _ => Ok(QuerySpec { payload, named }),
    }
}

// Changed by decision 112: the theory is an `EFree` message. The text before:
// ```
// /// `free Th(a…)`: the FREE tuple.
// pub fn efree(name: &str, args: Vec<Par>) -> Result<ETuple, ReifiedSyntaxError> {
//     Ok(free_tuple(name, args)?)
// }
// ```
/// `free Th(a…)`: the `EFree` message.
pub fn efree(name: &str, args: Vec<Par>) -> Result<EFree, ReifiedSyntaxError> {
    Ok(efree_message(name, args)?)
}

// Retired by decision 112: no process tuple exists to prepend. The text before:
// ```
// /// Adds a tuple in front of the expressions of a process `Par`, as `prepend_expr` does.
// pub fn push_process_expr(par: Par, tuple: ETuple) -> Par {
//     let locally_free = union(par.locally_free, tuple.locally_free.clone());
//     let connective_used = par.connective_used || tuple.connective_used;
//     let mut exprs = Vec::with_capacity(par.exprs.len() + 1);
//     exprs.push(Expr {
//         expr_instance: Some(ExprInstance::ETupleBody(tuple)),
//     });
//     exprs.extend(par.exprs);
//     Par {
//         exprs,
//         locally_free,
//         connective_used,
//         ..par
//     }
// }
// ```

/// Adds a use block in front of the blocks of a process `Par`, with the flags of its space and
/// its body, as `prepend_expr` adds the flags of an expression.
pub fn push_use_block(par: Par, block: UseBlock) -> Par {
    let mut locally_free = par.locally_free;
    let mut connective_used = par.connective_used;
    for part in [block.space.as_ref(), block.body.as_ref()]
        .into_iter()
        .flatten()
    {
        locally_free = union(locally_free, part.locally_free.clone());
        connective_used = connective_used || part.connective_used;
    }
    let mut use_blocks = Vec::with_capacity(par.use_blocks.len() + 1);
    use_blocks.push(block);
    use_blocks.extend(par.use_blocks);
    Par {
        use_blocks,
        locally_free,
        connective_used,
        ..par
    }
}

/// Adds `free Th(a…)` in front of the expressions of a process `Par`, with the flags of its
/// arguments, as `prepend_expr` adds the flags of an expression.
pub fn push_free(par: Par, free: EFree) -> Par {
    let mut locally_free = par.locally_free;
    let mut connective_used = par.connective_used;
    for arg in &free.args {
        locally_free = union(locally_free, arg.locally_free.clone());
        connective_used = connective_used || arg.connective_used;
    }
    let mut exprs = Vec::with_capacity(par.exprs.len() + 1);
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::EFreeBody(free)),
    });
    exprs.extend(par.exprs);
    Par {
        exprs,
        locally_free,
        connective_used,
        ..par
    }
}
