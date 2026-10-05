//! Builders of the Reified marker tuples for the frontends.
//!
//! Each builder checks the syntax that a forged tuple could otherwise express, then emits the
//! canonical encoding. The `locally_free` and `connective_used` fields follow the normalizer:
//! a tuple or a list takes the union and the disjunction of its elements, a map comes from
//! `ParMap::create_from_vec`, `Par{news: [New]}` follows `prepend_new`, and
//! `push_process_expr` follows `prepend_expr`. The frontend applies
//! `validation::validate_param_value` to each literal param value.

use super::encoding::{
    free_tuple, new_space_tuple, params_tuple, query_tuple, tuple_par, use_tuple, SendParams,
};
use super::params::{collect_send_params, SendParam};
use super::query_spec::collect_query_params;
use super::validation::ReifiedSyntaxError;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{ETuple, Expr, New, Par};
use crate::rust::utils::union;

/// `use s { P }`: the USE tuple for `Par.exprs`.
pub fn use_block(space: Par, body: Par) -> ETuple { use_tuple(space, body) }

/// `new x_1[: s_1], … in P` with at least one annotation: the NEWSPACE tuple for `Par.exprs`.
///
/// `new` is the normalized `New`. `binder_annotations` holds one entry for each binder in
/// binding order: the plain binders in source order, then the URI binders. The annotations
/// are normalized in the scope outside the `new`. A URI binder takes no annotation.
pub fn new_space(
    new: New,
    binder_annotations: Vec<Option<Par>>,
) -> Result<ETuple, ReifiedSyntaxError> {
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
    Ok(new_space_tuple(&annotations, new)?)
}

/// `x!(d…; p…, k=v…)`: the trailing data element of the send, or `None` when the send has
/// no params. `x!(a;)` thus builds the same data as `x!(a)`.
pub fn send_params(params: Vec<SendParam>) -> Result<Option<Par>, ReifiedSyntaxError> {
    let SendParams { positional, named } = collect_send_params(params)?;
    if positional.is_empty() && named.is_empty() {
        return Ok(None);
    }
    Ok(Some(tuple_par(params_tuple(positional, &named)?)))
}

/// `for (y <- ch ~ (q…; k=v…)) P`: the bind source. A bare `ch ~ e` has the payload `[e]`.
pub fn query_spec(
    channel: Par,
    payload: Vec<Par>,
    params: Vec<SendParam>,
) -> Result<Par, ReifiedSyntaxError> {
    let named = collect_query_params(params)?;
    Ok(tuple_par(query_tuple(channel, payload, &named)?))
}

/// `free Th(a…)`: the FREE tuple.
pub fn efree(name: &str, args: Vec<Par>) -> Result<ETuple, ReifiedSyntaxError> {
    Ok(free_tuple(name, args)?)
}

/// Adds a tuple in front of the expressions of a process `Par`, as `prepend_expr` does.
pub fn push_process_expr(par: Par, tuple: ETuple) -> Par {
    let locally_free = union(par.locally_free, tuple.locally_free.clone());
    let connective_used = par.connective_used || tuple.connective_used;
    let mut exprs = Vec::with_capacity(par.exprs.len() + 1);
    exprs.push(Expr {
        expr_instance: Some(ExprInstance::ETupleBody(tuple)),
    });
    exprs.extend(par.exprs);
    Par {
        exprs,
        locally_free,
        connective_used,
        ..par
    }
}
