use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{ETuple, Expr, Par};

/// Helper functions to extract values from Par expressions
/// Equivalent to Scala's getGString, getGInt, getGBool, getETupleBody methods
pub trait ParExt {
    /// Extract a string value from a Par expression
    fn get_g_string(&self) -> Option<String>;

    /// Extract an integer value from a Par expression
    fn get_g_int(&self) -> Option<i64>;

    /// Extract a boolean value from a Par expression
    fn get_g_bool(&self) -> Option<bool>;

    /// Extract an ETuple body from a Par expression
    fn get_e_tuple_body(&self) -> Option<ETuple>;
}

impl ParExt for Par {
    fn get_g_string(&self) -> Option<String> {
        match first_expr(self)?.expr_instance? {
            ExprInstance::GString(value) => Some(value),
            _ => None,
        }
    }

    fn get_g_int(&self) -> Option<i64> {
        match first_expr(self)?.expr_instance? {
            ExprInstance::GInt(value) => Some(value),
            _ => None,
        }
    }

    fn get_g_bool(&self) -> Option<bool> {
        match first_expr(self)?.expr_instance? {
            ExprInstance::GBool(value) => Some(value),
            _ => None,
        }
    }

    fn get_e_tuple_body(&self) -> Option<ETuple> {
        match first_expr(self)?.expr_instance? {
            ExprInstance::ETupleBody(value) => Some(value),
            _ => None,
        }
    }
}

fn first_expr(par: &Par) -> Option<Expr> {
    par.first::<Expr>()
        .expect("PathMap-backed Par invariant: stored Expr keys are canonical")
        .map(|(expr, _multiplicity)| expr)
}
