//! Shared IEEE-754 NaN detection for Rholang equality.
//!
//! Equality in both the pure evaluator and the full reducer must treat a NaN
//! anywhere inside the collection values they compare as unequal.  This module
//! is the single implementation of that policy.  It deliberately follows the
//! historical semantic boundary: the top-level `Expr` family and nested
//! `EList`, `ETuple`, `ESet`, and `EMap` children.  Other expression and process
//! families remain opaque.

use models::rhoapi::expr::ExprInstance::{EListBody, EMapBody, ESetBody, ETupleBody, GDouble};
use models::rhoapi::{Expr, Par};
use models::rust::rholang::match_schema_tables::EXPR_RHO_MATCH_PROGRAM;
use models::rust::rholang::protobuf_cursor::{
    CanonicalByteRange, CanonicalParRegionIndex, ProtobufRegion,
};
use models::rust::rholang::protobuf_decoder::decode_expr;
use models::rust::rholang::protobuf_region::cut_at_par;

enum Work {
    Par(ProtobufRegion),
    Expr(ProtobufRegion),
}

/// Return whether `par` contains an IEEE-754 NaN in the collection domain used
/// by Rholang equality.
///
/// The traversal is iterative and visits the root `Par`'s distinct
/// PathMap-backed `Expr` endpoints once. Multiplicity is irrelevant to this
/// existential predicate and therefore remains compressed instead of being
/// expanded into occurrences. Nested protobuf messages remain constant-size
/// regions into shared backing storage; no successively shorter recursive
/// suffix is decoded or copied.
#[must_use]
pub fn par_contains_nan_double(par: &Par) -> bool {
    // PathMap lends each root payload only for the duration of its visitor.
    // Coalesce those distinct bodies once, then retain ranges into one Bytes
    // allocation. This keeps the work item constant-size without one allocation
    // per root endpoint. Logical multiplicity stays in the PathMap counter and
    // is deliberately not expanded for this existential query.
    let mut root_bytes = Vec::new();
    let mut root_ranges = Vec::with_capacity(par.family_distinct_len::<Expr>());
    par.visit_encoded::<Expr>(|entry| {
        let start = root_bytes.len();
        root_bytes.extend_from_slice(entry.payload());
        root_ranges.push(CanonicalByteRange {
            start,
            end: root_bytes.len(),
        });
    });

    let backing = ProtobufRegion::from(root_bytes);
    let mut work = Vec::with_capacity(root_ranges.len());
    for range in root_ranges {
        work.push(Work::Expr(
            backing
                .subregion(range)
                .expect("root Expr ranges were measured from the same backing"),
        ));
    }

    while let Some(next) = work.pop() {
        match next {
            Work::Par(region) => {
                let index = CanonicalParRegionIndex::parse(region)
                    .expect("Par invariant: nested protobuf Par is canonical");
                if let Some(run) = index.exprs {
                    let mut cursor = run.cursor();
                    while let Some(expr) = cursor
                        .next_region()
                        .expect("Par invariant: nested Expr run is canonical")
                    {
                        work.push(Work::Expr(expr));
                    }
                }
            }
            Work::Expr(region) => {
                // The generated schema program is the sole authority for which
                // nested messages are Pars. `cut_at_par` stops at those edges,
                // so its shell is bounded even when the original expression is
                // thousands of collection levels deep.
                let plan = cut_at_par(region, &EXPR_RHO_MATCH_PROGRAM)
                    .expect("Par invariant: Expr protobuf is canonical");
                let shell = decode_expr(plan.shallow_bytes())
                    .expect("Par invariant: shallow Expr protobuf is canonical");
                match shell.expr_instance {
                    Some(GDouble(bits)) if f64::from_bits(bits).is_nan() => return true,
                    Some(EListBody(_) | ETupleBody(_) | ESetBody(_) | EMapBody(_)) => {
                        // Cuts are emitted in protobuf/source order and the
                        // worklist is LIFO. Besides avoiding an observable order
                        // promise for this pure query, that preserves the old
                        // predicate's right-to-left early-return opportunity.
                        for cut in plan.cuts() {
                            work.push(Work::Par(cut.region().clone()));
                        }
                    }
                    // Historical equality treats every other Expr constructor,
                    // including operators with Par operands and EPathMap values,
                    // as opaque. Discard their generated cuts deliberately.
                    _ => {}
                }
            }
        }
    }
    false
}
