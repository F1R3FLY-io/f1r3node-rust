//! Calculus of Constructions typechecker.
//!
//! Phase 2c-ii (skeleton): synthesis (`synth`) for ground literals and
//! `SimpleType` connectives. Subsequent phases add Pi formation, lambda
//! checking, application checking, and definitional equality.
//!
//! **Types are processes.** A type is a `Par` whose semantic shape is a
//! single [`Connective`] (base types) or a single [`EPi`] expression
//! (dependent function type). `synth` returns such a `Par`. The universe
//! is represented by `ConnType(true)`.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Connective, Par};

use crate::rust::interpreter::errors::InterpreterError;

/// Typing context — a stack of `Par` types indexed by De Bruijn level.
/// Entry at position 0 is the innermost binder. Mirrors the parser's
/// `bound_map_chain` layout for ProcSort entries.
#[derive(Clone, Debug, Default)]
pub struct Context {
    bindings: Vec<Par>,
}

impl Context {
    pub fn new() -> Self {
        Context {
            bindings: Vec::new(),
        }
    }

    /// Push a new binder's type. The pushed entry becomes `BoundVar(0)`.
    pub fn extend(&self, ty: Par) -> Context {
        let mut new_bindings = self.bindings.clone();
        new_bindings.push(ty);
        Context {
            bindings: new_bindings,
        }
    }

    /// Look up the type of `BoundVar(index)`.
    pub fn lookup(&self, index: i32) -> Option<&Par> {
        if index < 0 {
            return None;
        }
        let level = self.bindings.len();
        let pos = level.checked_sub(1 + index as usize)?;
        self.bindings.get(pos)
    }

    pub fn depth(&self) -> usize { self.bindings.len() }
}

/// Error raised when typechecking fails. Par-carrying variants box the
/// Par to keep `Result<_, TypeError>` compact (Par is ~248 bytes).
#[derive(Debug, Clone, PartialEq)]
pub enum TypeError {
    /// The term has a shape the typechecker doesn't yet understand.
    Unsupported(String),
    /// The term's representation is malformed (e.g. `exprs: []` for a
    /// single-expression Par).
    NotWellFormed(String),
    /// `check` found a type different from what it expected (syntactic
    /// comparison — Phase 2c-iv adds definitional equality).
    Mismatch {
        expected: Box<Par>,
        actual: Box<Par>,
    },
    /// Application `fun(args)` has a different number of args than
    /// `fun`'s Pi type binds.
    ArityMismatch { expected: i32, actual: i32 },
    /// `synth` on a `BoundVar(k)` could not find an entry in the context.
    UnboundVar(i32),
    /// `synth` on an application whose `fun` does not have a Pi type.
    AppOfNonFunction(Box<Par>),
}

impl From<TypeError> for InterpreterError {
    fn from(err: TypeError) -> Self { InterpreterError::ReduceError(format!("{:?}", err)) }
}

/// Synthesize the type of `par` in `ctx`. Returns a `Par` whose shape
/// is a well-formed type (currently: a single-connective Par for base
/// types, or `Type` itself).
pub fn synth(ctx: &Context, par: &Par) -> Result<Par, TypeError> {
    // Phase 2c-ii only looks at single-expression or single-connective
    // Pars; process-level content (sends, receives, news) is unsupported.
    if !par.sends.is_empty()
        || !par.receives.is_empty()
        || !par.news.is_empty()
        || !par.matches.is_empty()
        || !par.bundles.is_empty()
        || !par.conditionals.is_empty()
        || !par.unforgeables.is_empty()
    {
        return Err(TypeError::Unsupported(
            "process-level construct cannot yet be typechecked".to_string(),
        ));
    }

    match (par.exprs.first(), par.connectives.first()) {
        // Expression position: synth from the Expr.
        (Some(expr), None) => synth_expr(ctx, expr.expr_instance.as_ref()),
        // Connective position: must be a base type (Int, Bool, …) or Type.
        (None, Some(conn)) => synth_connective(conn),
        (None, None) => Err(TypeError::NotWellFormed(
            "empty Par has no type".to_string(),
        )),
        (Some(_), Some(_)) => Err(TypeError::Unsupported(
            "Par with both expr and connective slots is not yet typechecked".to_string(),
        )),
    }
}

fn synth_expr(_ctx: &Context, expr: Option<&ExprInstance>) -> Result<Par, TypeError> {
    let inst =
        expr.ok_or_else(|| TypeError::NotWellFormed("Expr with empty expr_instance".to_string()))?;

    // Ground literals: produce their base type (as a single-connective Par).
    match inst {
        ExprInstance::GBool(_) => Ok(type_par(ConnectiveInstance::ConnBool(true))),
        ExprInstance::GInt(_)
        | ExprInstance::GUint64(_)
        | ExprInstance::GInt32(_)
        | ExprInstance::GUint32(_)
        | ExprInstance::GUint16(_)
        | ExprInstance::GUint8(_)
        | ExprInstance::GBigInt(_)
        | ExprInstance::GBigRat(_)
        | ExprInstance::GFixedPoint(_)
        | ExprInstance::GDouble(_)
        | ExprInstance::GFloat32(_) => Ok(type_par(ConnectiveInstance::ConnInt(true))),
        ExprInstance::GString(_) => Ok(type_par(ConnectiveInstance::ConnString(true))),
        ExprInstance::GUri(_) => Ok(type_par(ConnectiveInstance::ConnUri(true))),
        ExprInstance::GByteArray(_) => Ok(type_par(ConnectiveInstance::ConnByteArray(true))),

        // Pi types are inhabitants of Type. Phase 2c-iii will check
        // that the param types and codomain are themselves types; for
        // now accept any EPi.
        ExprInstance::EPiBody(_) => Ok(type_par(ConnectiveInstance::ConnType(true))),

        // Bare lambda has no inherent type without an expected Pi — use
        // `check` with an annotation instead.
        ExprInstance::ELambdaBody(_) => Err(TypeError::Unsupported(
            "cannot synthesize a lambda's type without an expected Pi; use \
             `check` with a Pi annotation"
                .to_string(),
        )),

        // Function application: synth fun → Pi; check each arg against
        // the corresponding param type; return the codomain. Phase 2c-iii
        // omits dependent substitution of the arg into the codomain —
        // that's Phase 2c-iv.
        ExprInstance::EAppBody(app) => {
            let fun_par = app
                .fun
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EApp with no fun".to_string()))?;
            let fun_ty = synth(_ctx, fun_par)?;

            let pi = match single_expr(&fun_ty) {
                Some(ExprInstance::EPiBody(pi)) => pi.clone(),
                _ => return Err(TypeError::AppOfNonFunction(Box::new(fun_ty))),
            };

            if app.args.len() as i32 != pi.bind_count {
                return Err(TypeError::ArityMismatch {
                    expected: pi.bind_count,
                    actual: app.args.len() as i32,
                });
            }

            for (arg, pt) in app.args.iter().zip(pi.param_types.iter()) {
                check(_ctx, arg, pt)?;
            }

            // Return codomain as the application's type. Non-dependent
            // for now; dependent codomains need param substitution.
            pi.codomain
                .clone()
                .ok_or_else(|| TypeError::NotWellFormed("EPi with no codomain".to_string()))
        }

        // A bound variable's type is its entry in the context.
        ExprInstance::EVarBody(evar) => {
            let v = evar
                .v
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EVar with no Var".to_string()))?;
            match v.var_instance.as_ref() {
                Some(VarInstance::BoundVar(k)) => {
                    _ctx.lookup(*k).cloned().ok_or(TypeError::UnboundVar(*k))
                }
                _ => Err(TypeError::Unsupported(
                    "only BoundVar can be typechecked; FreeVar/Wildcard are \
                     pattern-level and don't have inherent types"
                        .to_string(),
                )),
            }
        }

        other => Err(TypeError::Unsupported(format!(
            "no typing rule for expression kind yet: {:?}",
            discriminant_name(other)
        ))),
    }
}

/// Check that `par` has type `expected` in `ctx`. Bidirectional rule
/// for lambdas: a lambda is checkable against a Pi of matching arity;
/// the body is checked against the Pi's codomain under a context
/// extended with the Pi's param types. All other shapes fall through
/// to `synth` + syntactic type equality.
pub fn check(ctx: &Context, par: &Par, expected: &Par) -> Result<(), TypeError> {
    // Lambda-against-Pi: use the Pi's param types as the body's
    // context. The lambda itself has no annotations in the current
    // proto; Phase 2c-ii collected them in the parser but dropped
    // them at normalization time.
    if let (Some(ExprInstance::ELambdaBody(lambda)), Some(ExprInstance::EPiBody(pi))) =
        (single_expr(par), single_expr(expected))
    {
        if lambda.bind_count != pi.bind_count {
            return Err(TypeError::ArityMismatch {
                expected: pi.bind_count,
                actual: lambda.bind_count,
            });
        }

        // Extend context so BoundVar(0) in body = first param of the Pi.
        // Push in reverse so lookup(0) returns the last-pushed = first param.
        let mut inner = ctx.clone();
        for pt in pi.param_types.iter().rev() {
            inner = inner.extend(pt.clone());
        }

        let body = lambda
            .body
            .as_ref()
            .ok_or_else(|| TypeError::NotWellFormed("ELambda with no body".to_string()))?;
        let codomain = pi
            .codomain
            .as_ref()
            .ok_or_else(|| TypeError::NotWellFormed("EPi with no codomain".to_string()))?;

        return check(&inner, body, codomain);
    }

    // Fall through: synth and compare syntactically.
    let actual = synth(ctx, par)?;
    if actual == *expected {
        Ok(())
    } else {
        Err(TypeError::Mismatch {
            expected: Box::new(expected.clone()),
            actual: Box::new(actual),
        })
    }
}

fn single_expr(par: &Par) -> Option<&ExprInstance> {
    if par.exprs.len() != 1 {
        return None;
    }
    par.exprs.first()?.expr_instance.as_ref()
}

fn synth_connective(conn: &Connective) -> Result<Par, TypeError> {
    let inst = conn
        .connective_instance
        .as_ref()
        .ok_or_else(|| TypeError::NotWellFormed("Connective with empty instance".to_string()))?;

    // Base types (Int, Bool, String, Uri, ByteArray) and the universe
    // itself are all inhabitants of Type. Logical connectives
    // (ConnAnd/ConnOr/ConnNot/VarRef) aren't typed yet.
    match inst {
        ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_)
        | ConnectiveInstance::ConnType(_) => Ok(type_par(ConnectiveInstance::ConnType(true))),
        other => Err(TypeError::Unsupported(format!(
            "no typing rule for connective kind yet: {:?}",
            discriminant_name_conn(other)
        ))),
    }
}

/// Build a Par carrying a single [`Connective`] with the given instance.
/// Used throughout the typechecker to express base-type and universe
/// values.
fn type_par(instance: ConnectiveInstance) -> Par {
    Par::default().with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }])
}

fn discriminant_name(inst: &ExprInstance) -> &'static str {
    match inst {
        ExprInstance::GBool(_) => "GBool",
        ExprInstance::GInt(_) => "GInt",
        ExprInstance::GString(_) => "GString",
        ExprInstance::GUri(_) => "GUri",
        ExprInstance::GByteArray(_) => "GByteArray",
        ExprInstance::GDouble(_) => "GDouble",
        ExprInstance::GFloat32(_) => "GFloat32",
        ExprInstance::GBigInt(_) => "GBigInt",
        ExprInstance::GBigRat(_) => "GBigRat",
        ExprInstance::GFixedPoint(_) => "GFixedPoint",
        ExprInstance::GUint64(_) => "GUint64",
        ExprInstance::GInt32(_) => "GInt32",
        ExprInstance::GUint32(_) => "GUint32",
        ExprInstance::GUint16(_) => "GUint16",
        ExprInstance::GUint8(_) => "GUint8",
        ExprInstance::ELambdaBody(_) => "ELambda",
        ExprInstance::EAppBody(_) => "EApp",
        ExprInstance::EPiBody(_) => "EPi",
        ExprInstance::ENotBody(_) => "ENot",
        ExprInstance::ENegBody(_) => "ENeg",
        ExprInstance::EMultBody(_) => "EMult",
        ExprInstance::EDivBody(_) => "EDiv",
        ExprInstance::EModBody(_) => "EMod",
        ExprInstance::EPlusBody(_) => "EPlus",
        ExprInstance::EMinusBody(_) => "EMinus",
        ExprInstance::ELtBody(_) => "ELt",
        ExprInstance::ELteBody(_) => "ELte",
        ExprInstance::EGtBody(_) => "EGt",
        ExprInstance::EGteBody(_) => "EGte",
        ExprInstance::EEqBody(_) => "EEq",
        ExprInstance::ENeqBody(_) => "ENeq",
        ExprInstance::EAndBody(_) => "EAnd",
        ExprInstance::EOrBody(_) => "EOr",
        ExprInstance::EVarBody(_) => "EVar",
        ExprInstance::EListBody(_) => "EList",
        ExprInstance::ETupleBody(_) => "ETuple",
        ExprInstance::ESetBody(_) => "ESet",
        ExprInstance::EMapBody(_) => "EMap",
        ExprInstance::EPathmapBody(_) => "EPathmap",
        ExprInstance::EZipperBody(_) => "EZipper",
        ExprInstance::EMethodBody(_) => "EMethod",
        ExprInstance::EMatchesBody(_) => "EMatches",
        ExprInstance::EPercentPercentBody(_) => "EPercentPercent",
        ExprInstance::EPlusPlusBody(_) => "EPlusPlus",
        ExprInstance::EMinusMinusBody(_) => "EMinusMinus",
    }
}

fn discriminant_name_conn(inst: &ConnectiveInstance) -> &'static str {
    match inst {
        ConnectiveInstance::ConnAndBody(_) => "ConnAnd",
        ConnectiveInstance::ConnOrBody(_) => "ConnOr",
        ConnectiveInstance::ConnNotBody(_) => "ConnNot",
        ConnectiveInstance::VarRefBody(_) => "VarRef",
        ConnectiveInstance::ConnBool(_) => "ConnBool",
        ConnectiveInstance::ConnInt(_) => "ConnInt",
        ConnectiveInstance::ConnString(_) => "ConnString",
        ConnectiveInstance::ConnUri(_) => "ConnUri",
        ConnectiveInstance::ConnByteArray(_) => "ConnByteArray",
        ConnectiveInstance::ConnType(_) => "ConnType",
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::Expr;

    use super::*;

    fn gint_par(v: i64) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(v)),
        }])
    }

    fn gstring_par(s: &str) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GString(s.to_string())),
        }])
    }

    fn gbool_par(b: bool) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GBool(b)),
        }])
    }

    fn connective_of(par: &Par) -> Option<&ConnectiveInstance> {
        par.connectives.first()?.connective_instance.as_ref()
    }

    #[test]
    fn synth_gint_is_int() {
        let ctx = Context::new();
        let ty = synth(&ctx, &gint_par(42)).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnInt(_))
        ));
    }

    #[test]
    fn synth_gstring_is_string() {
        let ctx = Context::new();
        let ty = synth(&ctx, &gstring_par("hello")).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnString(_))
        ));
    }

    #[test]
    fn synth_gbool_is_bool() {
        let ctx = Context::new();
        let ty = synth(&ctx, &gbool_par(true)).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnBool(_))
        ));
    }

    #[test]
    fn synth_int_connective_is_type() {
        // The `Int` simple type (a connective) has type Type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let meta_ty = synth(&ctx, &int_ty).unwrap();
        assert!(matches!(
            connective_of(&meta_ty),
            Some(ConnectiveInstance::ConnType(_))
        ));
    }

    #[test]
    fn synth_type_connective_is_type() {
        // Girard-style: Type : Type. Consistent for Phase 2c-ii; a
        // predicativity fix (Type_0 : Type_1 : ...) can come later.
        let ctx = Context::new();
        let type_val = type_par(ConnectiveInstance::ConnType(true));
        let meta_ty = synth(&ctx, &type_val).unwrap();
        assert!(matches!(
            connective_of(&meta_ty),
            Some(ConnectiveInstance::ConnType(_))
        ));
    }

    #[test]
    fn synth_unsupported_lambda_errors() {
        use models::rhoapi::ELambda;
        let ctx = Context::new();
        let lam = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
                bind_count: 1,
                body: Some(Par::default()),
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let err = synth(&ctx, &lam).unwrap_err();
        assert!(matches!(err, TypeError::Unsupported(_)));
    }

    #[test]
    fn synth_empty_par_errors() {
        let ctx = Context::new();
        let err = synth(&ctx, &Par::default()).unwrap_err();
        assert!(matches!(err, TypeError::NotWellFormed(_)));
    }

    #[test]
    fn context_lookup_returns_binding() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let ctx = ctx.extend(int_ty.clone());
        // BoundVar(0) → innermost = the Int type we just pushed.
        let looked = ctx.lookup(0).expect("binder visible");
        assert_eq!(looked, &int_ty);
        assert!(ctx.lookup(1).is_none());
        assert_eq!(ctx.depth(), 1);
    }

    // Helper: build a Pi `(_: A) -> B` for tests.
    fn pi_par(param_ty: Par, codomain: Par) -> Par {
        use models::rhoapi::{EPi, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPiBody(EPi {
                bind_count: 1,
                param_types: vec![param_ty],
                codomain: Some(codomain),
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }

    // Helper: build a lambda `(_) => body` for tests.
    fn lambda_par(bind_count: i32, body: Par) -> Par {
        use models::rhoapi::{ELambda, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
                bind_count,
                body: Some(body),
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }

    // Helper: BoundVar(k) as a Par.
    fn bvar_par(k: i32) -> Par {
        use models::rhoapi::var::VarInstance;
        use models::rhoapi::{EVar, Expr, Var};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::BoundVar(k)),
                }),
            })),
        }])
    }

    #[test]
    fn check_identity_lambda_against_int_to_int() {
        // ((x) => x) : Int -> Int
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let expected = pi_par(int_ty.clone(), int_ty);
        let id_lambda = lambda_par(1, bvar_par(0));
        check(&ctx, &id_lambda, &expected).unwrap();
    }

    #[test]
    fn check_lambda_arity_mismatch() {
        // 2-arg lambda against a 1-arg Pi → ArityMismatch
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let expected = pi_par(int_ty.clone(), int_ty);
        let two_arg = lambda_par(2, bvar_par(0));
        let err = check(&ctx, &two_arg, &expected).unwrap_err();
        assert!(matches!(err, TypeError::ArityMismatch {
            expected: 1,
            actual: 2
        }));
    }

    #[test]
    fn check_lambda_body_type_mismatch() {
        // ((x) => "hello") : Int -> Int is a mismatch — body is String,
        // codomain is Int.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let expected = pi_par(int_ty.clone(), int_ty);
        let bad_lambda = lambda_par(1, gstring_par("hello"));
        let err = check(&ctx, &bad_lambda, &expected).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_app_of_identity_is_codomain() {
        // Applying identity :: Int -> Int to 42 has type Int.
        use models::rhoapi::{EApp, Expr};
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let identity_pi = pi_par(int_ty.clone(), int_ty.clone());

        // We fabricate an app whose fun is a lambda wrapped so synth can
        // run: synth on a bare lambda errors, so we check via context —
        // pretend `f` is in ctx with type `Int -> Int`, then `f(42)`
        // should synth to `Int`.
        let ctx = ctx.extend(identity_pi);
        let f_app = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAppBody(EApp {
                fun: Some(bvar_par(0)),
                args: vec![gint_par(42)],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let ty = synth(&ctx, &f_app).unwrap();
        assert_eq!(ty, int_ty);
    }

    #[test]
    fn synth_app_arity_mismatch() {
        use models::rhoapi::{EApp, Expr};
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let identity_pi = pi_par(int_ty.clone(), int_ty);
        let ctx = ctx.extend(identity_pi);

        // Pass two args to a 1-arg function.
        let bad_app = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAppBody(EApp {
                fun: Some(bvar_par(0)),
                args: vec![gint_par(1), gint_par(2)],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let err = synth(&ctx, &bad_app).unwrap_err();
        assert!(matches!(err, TypeError::ArityMismatch {
            expected: 1,
            actual: 2
        }));
    }

    #[test]
    fn synth_app_of_non_function_errors() {
        use models::rhoapi::{EApp, Expr};
        let ctx = Context::new();
        // ctx has no bindings; calling BoundVar(0) is unbound ...
        let bad_app = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAppBody(EApp {
                fun: Some(gint_par(42)), // applying an Int as a function
                args: vec![gint_par(1)],
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let err = synth(&ctx, &bad_app).unwrap_err();
        assert!(matches!(err, TypeError::AppOfNonFunction(_)));
    }

    #[test]
    fn synth_bound_var_returns_context_entry() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let ctx = ctx.extend(int_ty.clone());
        let ty = synth(&ctx, &bvar_par(0)).unwrap();
        assert_eq!(ty, int_ty);
    }

    #[test]
    fn synth_unbound_var_errors() {
        let ctx = Context::new();
        let err = synth(&ctx, &bvar_par(0)).unwrap_err();
        assert!(matches!(err, TypeError::UnboundVar(0)));
    }
}
