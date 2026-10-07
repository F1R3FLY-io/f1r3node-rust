//! Nonrecursive metered assembly for the substitution PDA.
//!
//! The reservation formulas and their order are the source native path's
//! `cleanup_meter`, `collect_metered`, `set_bits_until_metered`, and
//! `concat_metered`. Keeping these builders separate from traversal lets the
//! explicit worklist preserve the source charge trace without using its
//! recursive `substitute_par_metered` implementation.

use std::mem::size_of;

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    Bundle, Connective, CostSignedTerm, CostStack, EVar, Expr, GUnforgeable, If, Match, New, Par,
    Receive, Send,
};
use models::rust::canonical_path::encode_trie_path;
use models::rust::rholang::sorter::metered::{SortDepthGuard, SorterMeter};
use shared::rust::clone_backing::{
    self as clone_backing, BackingError, BackingMeter, CloneBacking,
};

use super::errors::InterpreterError;

fn rejected(_: BackingError) -> InterpreterError { InterpreterError::HostWorkRejected }

pub(crate) struct NativeOwnedMeter<'a>(pub &'a dyn BackingMeter);

impl BackingMeter for NativeOwnedMeter<'_> {
    fn reserve(
        &self,
        operations: usize,
        scanned: usize,
        backing: usize,
    ) -> Result<(), BackingError> {
        self.0.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            backing,
        )
    }
}

/// The supplied meter is the signature helper's first cleanup layer. Source
/// `native_par` applies a second layer to every allocation/cleanup charge.
fn reserve_cleanup(
    meter: &dyn BackingMeter,
    operations: usize,
    scanned: usize,
    backing: usize,
) -> Result<(), InterpreterError> {
    meter
        .reserve(
            operations
                .checked_mul(2)
                .ok_or(InterpreterError::HostWorkRejected)?,
            scanned,
            backing,
        )
        .map_err(rejected)
}

pub(crate) fn reserve_par_frame(
    meter: &SorterMeter<'_>,
) -> Result<SortDepthGuard, InterpreterError> {
    let guard = meter.enter().map_err(rejected)?;
    meter.reserve(1, size_of::<Par>(), 0).map_err(rejected)?;
    Ok(guard)
}

pub(crate) fn reserve_expr_entry(
    expr: &Expr,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    let owned = |operations: usize, scanned: usize, backing: usize| {
        meter.reserve(
            operations.checked_mul(2).ok_or(BackingError::Overflow)?,
            scanned,
            backing,
        )
    };
    clone_backing::reserve(expr, &owned).map_err(rejected)
}

pub(crate) fn reserve_many(
    count: usize,
    element_size: usize,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    let scanned = count
        .checked_mul(element_size)
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(meter, count, scanned, 0)?;
    reserve_cleanup(meter, count, 0, scanned)
}

pub(crate) fn reserve_entries(
    count: usize,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    let scanned = count
        .checked_mul(size_of::<(Par, Par)>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(
        meter,
        count
            .checked_mul(2)
            .ok_or(InterpreterError::HostWorkRejected)?,
        scanned,
        0,
    )?;
    reserve_cleanup(meter, count, 0, scanned)
}

/// EPathMap is a target-only substitution arm: source native metering leaves
/// its entries opaque, whereas target scoping substitutes them. These charges
/// bound the additional host work and never create a consensus billable event.
pub(crate) fn reserve_pathmap_decode(
    key_bytes: usize,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    // A canonical-path byte can introduce at most one decoded syntax node.
    // Include a Par shell and vector header per byte as a conservative bound
    // on decoder worklist/backing, plus the owned cursor's key copy.
    let node_bytes = size_of::<Par>()
        .checked_add(size_of::<Vec<Par>>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    let backing = key_bytes
        .checked_mul(node_bytes)
        .ok_or(InterpreterError::HostWorkRejected)?;
    meter
        .reserve(
            key_bytes
                .checked_add(1)
                .ok_or(InterpreterError::HostWorkRejected)?,
            key_bytes,
            backing,
        )
        .map_err(rejected)
}

pub(crate) fn reserve_pathmap_insert(
    key: &Par,
    has_value: bool,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    // Pay the structural walk before encoding; the canonical encoder itself is
    // iterative, and the resulting path length then prices the trie update.
    clone_backing::inspect(key, meter).map_err(rejected)?;
    clone_backing::reserve_copy_and_cleanup(key, meter).map_err(rejected)?;
    let path = encode_trie_path(key);
    let key_bytes = path.len();
    // This adapter encodes once to measure the key, and EntryTrie encodes once
    // more while inserting it. Price both byte passes explicitly.
    let scanned = key_bytes
        .checked_mul(3)
        .ok_or(InterpreterError::HostWorkRejected)?;
    let backing = key_bytes
        .checked_mul(size_of::<usize>() * 4)
        .and_then(|bytes| bytes.checked_add(size_of::<Par>()))
        .and_then(|bytes| {
            if has_value {
                bytes.checked_add(size_of::<Par>())
            } else {
                Some(bytes)
            }
        })
        .ok_or(InterpreterError::HostWorkRejected)?;
    meter
        .reserve(
            key_bytes
                .checked_mul(2)
                .and_then(|bytes| bytes.checked_add(1 + usize::from(has_value)))
                .ok_or(InterpreterError::HostWorkRejected)?,
            scanned,
            backing,
        )
        .map_err(rejected)
}

pub(crate) fn reserve_missing_required<T>(
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    let count = std::any::type_name::<T>()
        .len()
        .checked_add(2)
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(meter, 1, count, count)
}

pub(crate) fn reserve_collection<T>(
    count: usize,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    reserve_collection_layout(count, size_of::<T>(), meter)
}

pub(crate) fn reserve_collection_layout(
    count: usize,
    element_size: usize,
    meter: &dyn BackingMeter,
) -> Result<(), InterpreterError> {
    let bytes = count
        .checked_mul(element_size)
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(
        meter,
        count
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?,
        bytes,
        bytes,
    )
}

pub(crate) fn set_bits_until_metered(
    bits: Vec<u8>,
    until: i32,
    meter: &dyn BackingMeter,
) -> Result<Vec<u8>, InterpreterError> {
    let count = if until <= 0 {
        0
    } else {
        bits.len().min(until as usize)
    };
    reserve_cleanup(
        meter,
        count
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?,
        count,
        count,
    )?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    result.extend(bits.into_iter().take(count));
    Ok(result)
}

fn joined_vec<T>(
    left: Vec<T>,
    right: Vec<T>,
    meter: &dyn BackingMeter,
) -> Result<Vec<T>, InterpreterError> {
    let count = left
        .len()
        .checked_add(right.len())
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_collection::<T>(count, meter)?;
    let mut joined = Vec::new();
    joined
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    joined.extend(right);
    joined.extend(left);
    Ok(joined)
}

fn union_bits_metered(
    left: &[u8],
    right: &[u8],
    meter: &dyn BackingMeter,
) -> Result<Vec<u8>, InterpreterError> {
    let count = left.len().max(right.len());
    reserve_cleanup(
        meter,
        count
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?,
        count
            .checked_mul(2)
            .ok_or(InterpreterError::HostWorkRejected)?,
        count,
    )?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    for index in 0..count {
        result.push(left.get(index).copied().unwrap_or(0) | right.get(index).copied().unwrap_or(0));
    }
    Ok(result)
}

fn copy_metered<T: Clone + CloneBacking>(
    value: &T,
    meter: &dyn BackingMeter,
) -> Result<T, InterpreterError> {
    clone_backing::reserve_copy_and_cleanup(value, meter).map_err(rejected)?;
    Ok(value.clone())
}

fn bit_vector_metered(index: i32, meter: &dyn BackingMeter) -> Result<Vec<u8>, InterpreterError> {
    let count = usize::try_from(index)
        .ok()
        .and_then(|index| index.checked_add(1))
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(
        meter,
        count
            .checked_add(1)
            .ok_or(InterpreterError::HostWorkRejected)?,
        count,
        count,
    )?;
    let mut bits = Vec::new();
    bits.try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    bits.resize(count, 0);
    bits[count - 1] = 1;
    Ok(bits)
}

fn expr_metadata(
    expr: &Expr,
    depth: i32,
    meter: &dyn BackingMeter,
) -> Result<(Vec<u8>, bool), InterpreterError> {
    let unary = |value: &Option<Par>| -> Result<(Vec<u8>, bool), InterpreterError> {
        let par = value.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
        Ok((copy_metered(&par.locally_free, meter)?, par.connective_used))
    };
    let binary =
        |left: &Option<Par>, right: &Option<Par>| -> Result<(Vec<u8>, bool), InterpreterError> {
            let left = left.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
            let right = right.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
            Ok((
                union_bits_metered(&left.locally_free, &right.locally_free, meter)?,
                left.connective_used | right.connective_used,
            ))
        };
    macro_rules! cached { ($($variant:ident),+ $(,)?) => {$(
        if let Some(ExprInstance::$variant(value)) = &expr.expr_instance {
            return Ok((copy_metered(&value.locally_free, meter)?, value.connective_used));
        }
    )+}; }
    macro_rules! binary_arms { ($($variant:ident),+ $(,)?) => {$(
        if let Some(ExprInstance::$variant(value)) = &expr.expr_instance {
            return binary(&value.p1, &value.p2);
        }
    )+}; }
    cached!(
        EListBody,
        ETupleBody,
        ESetBody,
        EMapBody,
        EPathmapBody,
        EZipperBody
    );
    binary_arms!(
        EMultBody,
        EDivBody,
        EModBody,
        EPlusBody,
        EMinusBody,
        ELtBody,
        ELteBody,
        EGtBody,
        EGteBody,
        EEqBody,
        ENeqBody,
        EAndBody,
        EOrBody,
        EPercentPercentBody,
        EPlusPlusBody,
        EMinusMinusBody,
    );
    match &expr.expr_instance {
        Some(ExprInstance::EVarBody(EVar { v })) => {
            let var = v.as_ref().ok_or(InterpreterError::HostWorkRejected)?;
            match var
                .var_instance
                .as_ref()
                .ok_or(InterpreterError::HostWorkRejected)?
            {
                VarInstance::BoundVar(index) if depth == 0 => {
                    Ok((bit_vector_metered(*index, meter)?, false))
                }
                VarInstance::BoundVar(_) => Ok((Vec::new(), false)),
                VarInstance::FreeVar(_) | VarInstance::Wildcard(_) => Ok((Vec::new(), true)),
            }
        }
        Some(ExprInstance::ENotBody(value)) => unary(&value.p),
        Some(ExprInstance::ENegBody(value)) => unary(&value.p),
        Some(ExprInstance::EMethodBody(value)) => Ok((
            copy_metered(&value.locally_free, meter)?,
            value.connective_used,
        )),
        Some(ExprInstance::EMatchesBody(value)) => unary(&value.target),
        _ => Ok((Vec::new(), false)),
    }
}

fn prepended_vec<T>(
    value: T,
    tail: Vec<T>,
    meter: &dyn BackingMeter,
) -> Result<Vec<T>, InterpreterError> {
    let count = tail
        .len()
        .checked_add(1)
        .ok_or(InterpreterError::HostWorkRejected)?;
    let bytes = count
        .checked_mul(size_of::<T>())
        .ok_or(InterpreterError::HostWorkRejected)?;
    reserve_cleanup(meter, count, bytes, bytes)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| InterpreterError::HostWorkRejected)?;
    result.push(value);
    result.extend(tail);
    Ok(result)
}

pub(crate) fn prepend_expr_metered(
    mut par: Par,
    expr: Expr,
    depth: i32,
    meter: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let (expr_free, expr_connective) = expr_metadata(&expr, depth, meter)?;
    let locally_free = union_bits_metered(&par.locally_free, &expr_free, meter)?;
    let connective_used = par.connective_used || expr_connective;
    par.exprs = prepended_vec(expr, std::mem::take(&mut par.exprs), meter)?;
    par.locally_free = locally_free;
    par.connective_used = connective_used;
    Ok(par)
}

pub(crate) fn prepend_connective_metered(
    mut par: Par,
    connective: Connective,
    depth: i32,
    meter: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let locally_free = match &connective.connective_instance {
        Some(ConnectiveInstance::VarRefBody(value)) if value.depth == depth => {
            bit_vector_metered(value.index, meter)?
        }
        _ => Vec::new(),
    };
    let connective_used = par.connective_used
        || !matches!(
            &connective.connective_instance,
            Some(ConnectiveInstance::VarRefBody(_)) | None,
        );
    par.connectives = prepended_vec(connective, std::mem::take(&mut par.connectives), meter)?;
    par.locally_free = locally_free;
    par.connective_used = connective_used;
    Ok(par)
}

pub(crate) fn concat_metered(
    left: Par,
    right: Par,
    meter: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let locally_free = union_bits_metered(&right.locally_free, &left.locally_free, meter)?;
    let connective_used = right.connective_used || left.connective_used;
    let left = left.into_parts();
    let right = right.into_parts();
    Ok(Par {
        sends: joined_vec(left.sends, right.sends, meter)?,
        receives: joined_vec(left.receives, right.receives, meter)?,
        news: joined_vec(left.news, right.news, meter)?,
        exprs: joined_vec(left.exprs, right.exprs, meter)?,
        matches: joined_vec(left.matches, right.matches, meter)?,
        unforgeables: joined_vec(left.unforgeables, right.unforgeables, meter)?,
        bundles: joined_vec(left.bundles, right.bundles, meter)?,
        connectives: joined_vec(left.connectives, right.connectives, meter)?,
        conditionals: joined_vec(left.conditionals, right.conditionals, meter)?,
        cost_signed_terms: joined_vec(left.cost_signed_terms, right.cost_signed_terms, meter)?,
        cost_stacks: joined_vec(left.cost_stacks, right.cost_stacks, meter)?,
        locally_free,
        connective_used,
    })
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_par_metered(
    exprs_par: Par,
    connectives_par: Par,
    sends: Vec<Send>,
    receives: Vec<Receive>,
    news: Vec<New>,
    matches: Vec<Match>,
    bundles: Vec<Bundle>,
    conditionals: Vec<If>,
    cost_signed_terms: Vec<CostSignedTerm>,
    cost_stacks: Vec<CostStack>,
    unforgeables: Vec<GUnforgeable>,
    locally_free: Vec<u8>,
    connective_used: bool,
    shift: i32,
    meter: &dyn BackingMeter,
) -> Result<Par, InterpreterError> {
    let base = Par {
        sends,
        receives,
        news,
        exprs: Vec::new(),
        matches,
        unforgeables,
        bundles,
        connectives: Vec::new(),
        conditionals,
        cost_signed_terms,
        cost_stacks,
        locally_free: set_bits_until_metered(locally_free, shift, meter)?,
        connective_used,
    };
    concat_metered(
        exprs_par,
        concat_metered(connectives_par, base, meter)?,
        meter,
    )
}
