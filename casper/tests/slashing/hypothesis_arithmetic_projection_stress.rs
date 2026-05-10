// UC-85 — Arithmetic projection stress frontier.
//
// Maps to: docs/theory/slashing/slashing-specification.md §12 UC-85.
// Theorems: T-8 (slash transfers stake) at the projection edge,
// T-12 arithmetic.
// Reference: formal/tlaplus/slashing/TwoLevelSlashing.tla invariants
// `Inv_UnsignedArithmeticBoundary` / `Inv_SignedArithmeticBoundary`
// / `Inv_ArithmeticSafeEnvelope` / `ArithmeticProjectionStressClass`.
//
// Property: exact arithmetic and fixed-width projections agree
// EXCEPT at documented projection boundaries — overflow, underflow,
// off-by-one at `max + 1`. Any disagreement outside the documented
// boundary set is `UnexpectedDivergence` (CI failure).

use super::divergence_class::{
    classify, frontier_classification_ok, DivergenceClass, DivergenceReason,
};

#[derive(Debug, Clone, Copy)]
struct ArithmeticTrace {
    /// Value computed in exact (BigInt) arithmetic.
    exact: i128,
    /// Value computed in fixed-width (i64 / saturating / wrapping).
    fixed_width: i64,
}

fn classify_arith(t: ArithmeticTrace) -> DivergenceClass {
    if t.exact == t.fixed_width as i128 {
        DivergenceClass::Bisimilar
    } else if t.exact > i64::MAX as i128 || t.exact < i64::MIN as i128 {
        // Overflow / underflow is the documented projection
        // boundary; classify accordingly.
        classify(DivergenceReason::ProjectionBoundary)
    } else {
        classify(DivergenceReason::Unexpected)
    }
}

#[test]
fn uc_85_in_range_arithmetic_is_bisimilar() {
    let t = ArithmeticTrace {
        exact: 100,
        fixed_width: 100,
    };
    assert_eq!(classify_arith(t), DivergenceClass::Bisimilar);
}

#[test]
fn uc_85_overflow_is_projection_boundary() {
    // exact = i64::MAX + 1 cannot fit in i64; the i64-projection
    // either saturates (i64::MAX) or wraps (i64::MIN). Both are
    // ProjectionBoundary divergences, not Unexpected.
    let t = ArithmeticTrace {
        exact: (i64::MAX as i128) + 1,
        fixed_width: i64::MAX,
    };
    let class = classify_arith(t);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));

    let t = ArithmeticTrace {
        exact: (i64::MAX as i128) + 1,
        fixed_width: i64::MIN,
    };
    let class = classify_arith(t);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
    assert!(frontier_classification_ok(class));
}

#[test]
fn uc_85_underflow_is_projection_boundary() {
    let t = ArithmeticTrace {
        exact: (i64::MIN as i128) - 1,
        fixed_width: i64::MIN,
    };
    let class = classify_arith(t);
    assert_eq!(class, DivergenceClass::CandidateBoundaryDivergence);
}
