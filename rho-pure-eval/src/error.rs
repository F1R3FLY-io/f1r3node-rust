use models::rhoapi::Par;

/// Errors raised during pure evaluation. These never arise from side
/// effects (the evaluator has none); they signal a structural or type
/// problem with the input.
#[derive(Debug, Clone, PartialEq)]
pub enum EvalError {
    /// An EVar referenced an index that's not in the env. Mirrors the
    /// "unbound variable" failure that rholang's reducer raises in the
    /// same case.
    UnboundVariable { index: i32 },

    /// A binary or unary operator received operands of types it doesn't
    /// support (e.g. `1 + true`).
    OperatorTypeMismatch {
        op: &'static str,
        left: String,
        right: Option<String>,
    },

    /// Division or modulo by zero.
    DivisionByZero,

    /// A method call or `EMatchExpr` was reached. These are not yet
    /// supported in this crate; callers that need them must fall back
    /// to the full reducer in `rholang`. See the plan at
    /// docs/plans/where-clauses-and-match-guards-2026-04-29.md §3.9.
    UnsupportedExpression { kind: &'static str },

    /// An Expr's `expr_instance` field was None. Indicates a malformed
    /// Par from upstream.
    MissingExprInstance,

    /// A sub-Par was expected to reduce to a single ground value (e.g.
    /// for the operands of `==`) but reduced to something more complex.
    /// Boxed so that the rest of the EvalError variants stay small.
    NotASingleValue { actual: Box<Par> },
}

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalError::UnboundVariable { index } => {
                write!(f, "unbound variable at de Bruijn index {index}")
            }
            EvalError::OperatorTypeMismatch { op, left, right } => match right {
                Some(r) => write!(f, "operator `{op}` not defined on `{left}`, `{r}`"),
                None => write!(f, "operator `{op}` not defined on `{left}`"),
            },
            EvalError::DivisionByZero => write!(f, "division by zero"),
            EvalError::UnsupportedExpression { kind } => {
                write!(
                    f,
                    "expression kind `{kind}` is not supported by rho-pure-eval"
                )
            }
            EvalError::MissingExprInstance => write!(f, "Expr.expr_instance was None"),
            EvalError::NotASingleValue { .. } => {
                write!(f, "expected a single ground value")
            }
        }
    }
}

impl std::error::Error for EvalError {}
