//! Calculus of Constructions typechecker.
//!
//! Phase 4.1a (list types): synthesis (`synth`) for ground literals,
//! `SimpleType` connectives, Pi well-formedness, dependent application,
//! fully-annotated lambdas, β-definitional equality in `check`, the
//! integer binary operators, `ENeg`, comparison operators, the boolean
//! connectives on values, the logical type connectives, and `EList`
//! (whose type is `List<T>` where `T` is the LUB of element types).
//! Matcher integration for `ConnList` patterns, and the remaining
//! collection kinds (Set / Map / Tuple), arrive in later Phase 4
//! substages.
//!
//! **Types are processes.** A type is a `Par` whose semantic shape is a
//! single [`Connective`] (base types) or a single [`EPi`] expression
//! (dependent function type). `synth` returns such a `Par`. The universe
//! is represented by `ConnType(true)`.

use models::rhoapi::connective::ConnectiveInstance;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{Connective, Par};

use crate::rust::interpreter::accounting::costs::Cost;
use crate::rust::interpreter::accounting::CostManager;
use crate::rust::interpreter::env::Env;
use crate::rust::interpreter::errors::InterpreterError;
use crate::rust::interpreter::substitute::{Substitute, SubstituteTrait};

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
        // Connective position: base type, Type universe, or a logical
        // type connective (ConnAnd/ConnOr/ConnNot of types).
        (None, Some(conn)) => synth_connective(ctx, conn),
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

        // Pi formation. A Pi `(x_0: A_0, …, x_{n-1}: A_{n-1}) -> B` has
        // type `Type` iff every `A_i` has type `Type` in `_ctx` (the
        // outer scope — the normalizer does not thread earlier binders
        // into later param types) and `B` has type `Type` in the ctx
        // extended with all `A_i`. We extend in reverse so that
        // `BoundVar(i)` in `B` resolves to `A_i` (same convention as the
        // lambda-check branch).
        ExprInstance::EPiBody(pi) => {
            if pi.bind_count as usize != pi.param_types.len() {
                return Err(TypeError::NotWellFormed(format!(
                    "EPi bind_count {} does not match param_types len {}",
                    pi.bind_count,
                    pi.param_types.len()
                )));
            }
            let type_univ = type_par(ConnectiveInstance::ConnType(true));
            for pt in pi.param_types.iter() {
                check(_ctx, pt, &type_univ)?;
            }
            let mut inner = _ctx.clone();
            for pt in pi.param_types.iter().rev() {
                inner = inner.extend(pt.clone());
            }
            let codomain = pi
                .codomain
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EPi with no codomain".to_string()))?;
            check(&inner, codomain, &type_univ)?;
            Ok(type_univ)
        }

        // Lambda synthesis. With a full set of non-empty param
        // annotations we can build a Pi for the lambda: well-form each
        // annotation (must have type `Type`), extend ctx in reverse
        // (same convention as check-against-Pi), synth the body's type
        // as the codomain, and return `Pi(annotations, codomain)`.
        //
        // A lambda with no annotations or any empty-slot annotation is
        // still unsynthesizable — use `check` with an expected Pi.
        ExprInstance::ELambdaBody(lambda) => {
            if lambda.param_types.is_empty()
                || lambda.param_types.len() != lambda.bind_count as usize
                || lambda.param_types.iter().any(is_empty_par)
            {
                return Err(TypeError::Unsupported(
                    "cannot synthesize an unannotated (or partially annotated) \
                     lambda; use `check` with a Pi annotation"
                        .to_string(),
                ));
            }
            let type_univ = type_par(ConnectiveInstance::ConnType(true));
            for pt in lambda.param_types.iter() {
                check(_ctx, pt, &type_univ)?;
            }
            let mut inner = _ctx.clone();
            for pt in lambda.param_types.iter().rev() {
                inner = inner.extend(pt.clone());
            }
            let body = lambda
                .body
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("ELambda with no body".to_string()))?;
            let codomain = synth(&inner, body)?;
            Ok(build_pi(lambda.param_types.clone(), codomain))
        }

        // Function application: synth fun → Pi; check each arg against
        // the corresponding param type; substitute args into the
        // codomain and return the result.
        //
        // Convention: BoundVar(i) in the codomain refers to param i (same
        // layout the Pi-formation and check-against-Pi rules use). We
        // build an Env so that `env.get(i) = arg_i` by pushing args in
        // reverse (last-pushed = highest-indexed by de Bruijn lookup,
        // but env.get(i) looks up level-i-1 so the first-pushed binding
        // ends up at the highest index). The Substitute helper handles
        // shifts for nested binders inside the codomain.
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

            let codomain = pi
                .codomain
                .clone()
                .ok_or_else(|| TypeError::NotWellFormed("EPi with no codomain".to_string()))?;

            substitute_args_into_codomain(&app.args, codomain)
        }

        // Integer arithmetic. `EPlus/EMinus/EMult/EDiv/EMod` require
        // both operands to have type `Int` and return `Int`. Overloads
        // for other numeric types or string concat (EPlusPlus) aren't
        // handled yet.
        ExprInstance::EPlusBody(e) => {
            let p1 = e
                .p1
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EPlus with no p1".to_string()))?;
            let p2 = e
                .p2
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EPlus with no p2".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p1, &int_ty)?;
            check(_ctx, p2, &int_ty)?;
            Ok(int_ty)
        }
        ExprInstance::EMinusBody(e) => {
            let p1 = e
                .p1
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMinus with no p1".to_string()))?;
            let p2 = e
                .p2
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMinus with no p2".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p1, &int_ty)?;
            check(_ctx, p2, &int_ty)?;
            Ok(int_ty)
        }
        ExprInstance::EMultBody(e) => {
            let p1 = e
                .p1
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMult with no p1".to_string()))?;
            let p2 = e
                .p2
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMult with no p2".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p1, &int_ty)?;
            check(_ctx, p2, &int_ty)?;
            Ok(int_ty)
        }
        ExprInstance::EDivBody(e) => {
            let p1 = e
                .p1
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EDiv with no p1".to_string()))?;
            let p2 = e
                .p2
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EDiv with no p2".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p1, &int_ty)?;
            check(_ctx, p2, &int_ty)?;
            Ok(int_ty)
        }
        ExprInstance::EModBody(e) => {
            let p1 = e
                .p1
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMod with no p1".to_string()))?;
            let p2 = e
                .p2
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("EMod with no p2".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p1, &int_ty)?;
            check(_ctx, p2, &int_ty)?;
            Ok(int_ty)
        }

        // Integer ordering comparisons. `Int × Int → Bool`.
        ExprInstance::ELtBody(e) => synth_int_cmp(_ctx, e.p1.as_ref(), e.p2.as_ref(), "ELt"),
        ExprInstance::ELteBody(e) => synth_int_cmp(_ctx, e.p1.as_ref(), e.p2.as_ref(), "ELte"),
        ExprInstance::EGtBody(e) => synth_int_cmp(_ctx, e.p1.as_ref(), e.p2.as_ref(), "EGt"),
        ExprInstance::EGteBody(e) => synth_int_cmp(_ctx, e.p1.as_ref(), e.p2.as_ref(), "EGte"),

        // Equality / inequality. Both operands must have the same type
        // (up to β-def-eq); result is Bool.
        ExprInstance::EEqBody(e) => synth_poly_eq(_ctx, e.p1.as_ref(), e.p2.as_ref(), "EEq"),
        ExprInstance::ENeqBody(e) => synth_poly_eq(_ctx, e.p1.as_ref(), e.p2.as_ref(), "ENeq"),

        // Boolean logical operators. `Bool × Bool → Bool` / `Bool → Bool`.
        ExprInstance::EAndBody(e) => synth_bool_binop(_ctx, e.p1.as_ref(), e.p2.as_ref(), "EAnd"),
        ExprInstance::EOrBody(e) => synth_bool_binop(_ctx, e.p1.as_ref(), e.p2.as_ref(), "EOr"),
        ExprInstance::ENotBody(e) => {
            let p = e
                .p
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("ENot with no p".to_string()))?;
            let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
            check(_ctx, p, &bool_ty)?;
            Ok(bool_ty)
        }

        // Integer negation. `Int → Int`.
        ExprInstance::ENegBody(e) => {
            let p = e
                .p
                .as_ref()
                .ok_or_else(|| TypeError::NotWellFormed("ENeg with no p".to_string()))?;
            let int_ty = type_par(ConnectiveInstance::ConnInt(true));
            check(_ctx, p, &int_ty)?;
            Ok(int_ty)
        }

        // List literal `[e_0, …, e_{n-1}]`. Synth each element and
        // combine their types into a least-upper-bound via `lub_types`
        // (currently: all equal → that type; empty list → `Par::default()`
        // sentinel meaning "unknown element"; otherwise ConnOr of
        // per-element types). The result is `ConnList(element_type)`.
        ExprInstance::EListBody(list) => {
            let element_types: Vec<Par> = list
                .ps
                .iter()
                .map(|p| synth(_ctx, p))
                .collect::<Result<_, _>>()?;
            let element = lub_types(element_types);
            Ok(type_par(ConnectiveInstance::ConnList(element)))
        }

        // Set literal `Set(e_0, …)` or `{e_0, …}` — same element-type
        // LUB logic as EList, wrapped in ConnSet.
        ExprInstance::ESetBody(set) => {
            let element_types: Vec<Par> = set
                .ps
                .iter()
                .map(|p| synth(_ctx, p))
                .collect::<Result<_, _>>()?;
            let element = lub_types(element_types);
            Ok(type_par(ConnectiveInstance::ConnSet(element)))
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
    // context. If the lambda carries per-param annotations (populated
    // when the source was `(x: T) => body`), cross-verify each
    // non-empty annotation against the Pi's corresponding param type;
    // an empty-Par slot means that param was left un-annotated and the
    // Pi's type is accepted directly.
    if let (Some(ExprInstance::ELambdaBody(lambda)), Some(ExprInstance::EPiBody(pi))) =
        (single_expr(par), single_expr(expected))
    {
        if lambda.bind_count != pi.bind_count {
            return Err(TypeError::ArityMismatch {
                expected: pi.bind_count,
                actual: lambda.bind_count,
            });
        }

        if !lambda.param_types.is_empty() {
            if lambda.param_types.len() != pi.param_types.len() {
                return Err(TypeError::NotWellFormed(format!(
                    "ELambda has {} param_types but matches a Pi with {}",
                    lambda.param_types.len(),
                    pi.param_types.len(),
                )));
            }
            for (ann, pi_pt) in lambda.param_types.iter().zip(pi.param_types.iter()) {
                if is_empty_par(ann) {
                    continue;
                }
                if !def_eq(ann, pi_pt) {
                    return Err(TypeError::Mismatch {
                        expected: Box::new(pi_pt.clone()),
                        actual: Box::new(ann.clone()),
                    });
                }
            }
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

    // Fall through: synth and compare up to β-definitional equality.
    let actual = synth(ctx, par)?;
    if def_eq(&actual, expected) {
        Ok(())
    } else {
        Err(TypeError::Mismatch {
            expected: Box::new(expected.clone()),
            actual: Box::new(actual),
        })
    }
}

/// Definitional (β) equality on types.
///
/// Two types are definitionally equal iff β-reducing both to WHNF
/// produces equal top-level shapes; equal recursively on the sub-Pars
/// of a matching Pi.
pub fn def_eq(a: &Par, b: &Par) -> bool {
    let na = beta_whnf(a.clone());
    let nb = beta_whnf(b.clone());
    def_eq_whnf(&na, &nb)
}

fn def_eq_whnf(a: &Par, b: &Par) -> bool {
    if a == b {
        return true;
    }
    // Pi — recurse on param_types + codomain so a Pi whose codomain
    // was constructed by substitution (and thus carries recomputed
    // flags) can still match a syntactically-equal Pi.
    if let (Some(ExprInstance::EPiBody(pa)), Some(ExprInstance::EPiBody(pb))) =
        (single_expr(a), single_expr(b))
    {
        if pa.bind_count != pb.bind_count {
            return false;
        }
        if pa.param_types.len() != pb.param_types.len() {
            return false;
        }
        for (x, y) in pa.param_types.iter().zip(pb.param_types.iter()) {
            if !def_eq(x, y) {
                return false;
            }
        }
        return match (&pa.codomain, &pb.codomain) {
            (Some(x), Some(y)) => def_eq(x, y),
            (None, None) => true,
            _ => false,
        };
    }
    false
}

/// β-reduce `par` to weak head normal form by repeatedly collapsing
/// `EApp(ELambda, args)` at the head. Other shapes are returned
/// unchanged.
pub fn beta_whnf(mut par: Par) -> Par {
    loop {
        match beta_step(&par) {
            Some(next) => par = next,
            None => return par,
        }
    }
}

fn beta_step(par: &Par) -> Option<Par> {
    let app = match single_expr(par)? {
        ExprInstance::EAppBody(app) => app,
        _ => return None,
    };
    let fun = app.fun.as_ref()?;
    let lambda = match single_expr(fun)? {
        ExprInstance::ELambdaBody(lambda) => lambda,
        _ => return None,
    };
    if app.args.len() as i32 != lambda.bind_count {
        return None;
    }
    let body = lambda.body.as_ref()?.clone();
    let subst = Substitute {
        cost: CostManager::new(Cost::unsafe_max()),
    };
    let env = Env::new().extended(app.args.iter().rev().cloned());
    subst.substitute_no_sort(body, 0, &env).ok()
}

fn single_expr(par: &Par) -> Option<&ExprInstance> {
    if par.exprs.len() != 1 {
        return None;
    }
    par.exprs.first()?.expr_instance.as_ref()
}

/// Shared helper for ELt/ELte/EGt/EGte — both operands must be Int;
/// result is Bool.
fn synth_int_cmp(
    ctx: &Context,
    p1: Option<&Par>,
    p2: Option<&Par>,
    label: &'static str,
) -> Result<Par, TypeError> {
    let p1 = p1.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p1")))?;
    let p2 = p2.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p2")))?;
    let int_ty = type_par(ConnectiveInstance::ConnInt(true));
    check(ctx, p1, &int_ty)?;
    check(ctx, p2, &int_ty)?;
    Ok(type_par(ConnectiveInstance::ConnBool(true)))
}

/// Shared helper for EEq/ENeq — operands must share a type (up to
/// β-def-eq); result is Bool.
fn synth_poly_eq(
    ctx: &Context,
    p1: Option<&Par>,
    p2: Option<&Par>,
    label: &'static str,
) -> Result<Par, TypeError> {
    let p1 = p1.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p1")))?;
    let p2 = p2.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p2")))?;
    let t1 = synth(ctx, p1)?;
    let t2 = synth(ctx, p2)?;
    if !def_eq(&t1, &t2) {
        return Err(TypeError::Mismatch {
            expected: Box::new(t1),
            actual: Box::new(t2),
        });
    }
    Ok(type_par(ConnectiveInstance::ConnBool(true)))
}

/// Shared helper for EAnd/EOr — both operands must be Bool; result
/// is Bool.
fn synth_bool_binop(
    ctx: &Context,
    p1: Option<&Par>,
    p2: Option<&Par>,
    label: &'static str,
) -> Result<Par, TypeError> {
    let p1 = p1.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p1")))?;
    let p2 = p2.ok_or_else(|| TypeError::NotWellFormed(format!("{label} with no p2")))?;
    let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
    check(ctx, p1, &bool_ty)?;
    check(ctx, p2, &bool_ty)?;
    Ok(bool_ty)
}

/// Combine a list of element types into a single type via least
/// upper bound.
///
/// - Empty list: `Par::default()` sentinel ("element type unknown").
/// - All elements β-def-equal: that common type.
/// - Otherwise: a `ConnOrBody` of the deduplicated element types.
fn lub_types(mut types: Vec<Par>) -> Par {
    if types.is_empty() {
        return Par::default();
    }
    if types.len() == 1 {
        return types.pop().unwrap();
    }
    // Deduplicate under β-def-eq.
    let mut unique: Vec<Par> = Vec::new();
    for t in types {
        if !unique.iter().any(|u| def_eq(u, &t)) {
            unique.push(t);
        }
    }
    if unique.len() == 1 {
        return unique.pop().unwrap();
    }
    use models::rhoapi::ConnectiveBody;
    let conn_used = unique.iter().any(|p| p.connective_used);
    let mut par = Par::default().with_connectives(vec![Connective {
        connective_instance: Some(ConnectiveInstance::ConnOrBody(ConnectiveBody { ps: unique })),
    }]);
    par.connective_used = true;
    let _ = conn_used;
    par
}

/// Wrap the given param types and codomain into a Pi Par. Used by
/// lambda synthesis to produce the inferred type.
fn build_pi(param_types: Vec<Par>, codomain: Par) -> Par {
    use models::rhoapi::{EPi, Expr};
    let bind_count = param_types.len() as i32;
    let conn_used =
        codomain.connective_used || param_types.iter().any(|p| p.connective_used);
    let mut par = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPiBody(EPi {
            bind_count,
            param_types,
            codomain: Some(codomain),
            locally_free: Vec::new(),
            connective_used: conn_used,
        })),
    }]);
    par.connective_used = conn_used;
    par
}

/// A lambda's `param_types[i]` is `Par::default()` when the source
/// left slot `i` un-annotated. Treated as "no annotation; accept the
/// Pi's type here".
fn is_empty_par(p: &Par) -> bool {
    p.sends.is_empty()
        && p.receives.is_empty()
        && p.news.is_empty()
        && p.exprs.is_empty()
        && p.matches.is_empty()
        && p.bundles.is_empty()
        && p.conditionals.is_empty()
        && p.unforgeables.is_empty()
        && p.connectives.is_empty()
}

/// Substitute a list of application arguments into a Pi codomain.
///
/// The codomain was synthesized under a ctx extended with the Pi's
/// `param_types` (in `.rev()` order — see the Pi-formation and
/// check-against-Pi paths), which makes `BoundVar(i)` in the codomain
/// refer to `param_types[i]`. For a well-typed `f(arg_0, …, arg_{n-1})`,
/// replacing `BoundVar(i)` with `arg_i` yields the result type.
///
/// We delegate to the runtime `Substitute` with an unbounded cost
/// budget — Substitute already handles de Bruijn shifts for nested
/// binders inside the codomain. `Env::extended(args.iter().rev())`
/// pushes `arg_{n-1}` first and `arg_0` last; `env.get(i)` resolves
/// `BoundVar(i)` to `bindings[len - i - 1]`, so `BoundVar(0)` recovers
/// the last-pushed arg (`arg_0`), and so on.
fn substitute_args_into_codomain(args: &[Par], codomain: Par) -> Result<Par, TypeError> {
    if args.is_empty() {
        return Ok(codomain);
    }
    let subst = Substitute {
        cost: CostManager::new(Cost::unsafe_max()),
    };
    let env = Env::new().extended(args.iter().rev().cloned());
    subst
        .substitute_no_sort(codomain, 0, &env)
        .map_err(|e| TypeError::NotWellFormed(format!("codomain substitution failed: {e:?}")))
}

fn synth_connective(ctx: &Context, conn: &Connective) -> Result<Par, TypeError> {
    let inst = conn
        .connective_instance
        .as_ref()
        .ok_or_else(|| TypeError::NotWellFormed("Connective with empty instance".to_string()))?;

    let type_univ = type_par(ConnectiveInstance::ConnType(true));
    // Base types (Int, Bool, String, Uri, ByteArray) and the universe
    // itself are all inhabitants of Type. Logical connectives build
    // compound types from inner types: each inner slot must have type
    // `Type`, and the result has type `Type`.
    match inst {
        ConnectiveInstance::ConnBool(_)
        | ConnectiveInstance::ConnInt(_)
        | ConnectiveInstance::ConnString(_)
        | ConnectiveInstance::ConnUri(_)
        | ConnectiveInstance::ConnByteArray(_)
        | ConnectiveInstance::ConnType(_)
        | ConnectiveInstance::ConnInt32(_)
        | ConnectiveInstance::ConnUint64(_)
        | ConnectiveInstance::ConnUint32(_)
        | ConnectiveInstance::ConnUint16(_)
        | ConnectiveInstance::ConnUint8(_)
        | ConnectiveInstance::ConnFloat64(_)
        | ConnectiveInstance::ConnFloat32(_)
        | ConnectiveInstance::ConnBigInt(_)
        | ConnectiveInstance::ConnBigRat(_)
        | ConnectiveInstance::ConnFixedPointBody(_) => Ok(type_univ),

        ConnectiveInstance::ConnAndBody(body) | ConnectiveInstance::ConnOrBody(body) => {
            for p in &body.ps {
                check(ctx, p, &type_univ)?;
            }
            Ok(type_univ)
        }
        ConnectiveInstance::ConnNotBody(p) => {
            check(ctx, p, &type_univ)?;
            Ok(type_univ)
        }

        // `List<T>` is a type when `T` is a type (or empty-Par sentinel
        // for an unknown element type, which is accepted vacuously).
        ConnectiveInstance::ConnList(element) => {
            if !is_empty_par(element) {
                check(ctx, element, &type_univ)?;
            }
            Ok(type_univ)
        }

        // `Set<T>` — same well-formedness rule as List<T>.
        ConnectiveInstance::ConnSet(element) => {
            if !is_empty_par(element) {
                check(ctx, element, &type_univ)?;
            }
            Ok(type_univ)
        }

        other => Err(TypeError::Unsupported(format!(
            "no typing rule for connective kind yet: {:?}",
            discriminant_name_conn(other)
        ))),
    }
}

/// Build a Par carrying a single [`Connective`] with the given instance.
/// Used throughout the typechecker to express base-type and universe
/// values. Sets `connective_used: true` so the resulting Par matches
/// what the normalizer produces for base-type patterns (via
/// `prepend_connective`) and what `substitute` recomputes after a walk.
fn type_par(instance: ConnectiveInstance) -> Par {
    let mut par = Par::default().with_connectives(vec![Connective {
        connective_instance: Some(instance),
    }]);
    par.connective_used = true;
    par
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
        ConnectiveInstance::ConnInt32(_) => "ConnInt32",
        ConnectiveInstance::ConnUint64(_) => "ConnUint64",
        ConnectiveInstance::ConnUint32(_) => "ConnUint32",
        ConnectiveInstance::ConnUint16(_) => "ConnUint16",
        ConnectiveInstance::ConnUint8(_) => "ConnUint8",
        ConnectiveInstance::ConnFloat64(_) => "ConnFloat64",
        ConnectiveInstance::ConnFloat32(_) => "ConnFloat32",
        ConnectiveInstance::ConnBigInt(_) => "ConnBigInt",
        ConnectiveInstance::ConnBigRat(_) => "ConnBigRat",
        ConnectiveInstance::ConnFixedPointBody(_) => "ConnFixedPoint",
        ConnectiveInstance::ConnList(_) => "ConnList",
        ConnectiveInstance::ConnSet(_) => "ConnSet",
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
    fn synth_unannotated_lambda_errors() {
        // Lambda with no annotations (legacy shape) can't be synthed —
        // caller must use `check` with a Pi.
        use models::rhoapi::ELambda;
        let ctx = Context::new();
        let lam = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
                bind_count: 1,
                body: Some(Par::default()),
                locally_free: Vec::new(),
                connective_used: false,
                param_types: Vec::new(),
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

    // Helper: build a Pi `(_: A) -> B` for tests. Propagates
    // `connective_used` from the subterms so the result matches
    // normalizer output.
    fn pi_par(param_ty: Par, codomain: Par) -> Par {
        use models::rhoapi::{EPi, Expr};
        let conn_used = param_ty.connective_used || codomain.connective_used;
        let mut par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPiBody(EPi {
                bind_count: 1,
                param_types: vec![param_ty],
                codomain: Some(codomain),
                locally_free: Vec::new(),
                connective_used: conn_used,
            })),
        }]);
        par.connective_used = conn_used;
        par
    }

    // Helper: build an untyped lambda `(_) => body` for tests.
    fn lambda_par(bind_count: i32, body: Par) -> Par {
        lambda_par_typed(bind_count, body, Vec::new())
    }

    // Helper: build a lambda whose params carry type annotations
    // (populated `param_types` of length `bind_count`).
    fn lambda_par_typed(bind_count: i32, body: Par, param_types: Vec<Par>) -> Par {
        use models::rhoapi::{ELambda, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELambdaBody(ELambda {
                bind_count,
                body: Some(body),
                locally_free: Vec::new(),
                connective_used: false,
                param_types,
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

    #[test]
    fn synth_pi_int_to_int_is_type() {
        // `(x: Int) -> Int` is well-formed and has type Type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let pi = pi_par(int_ty.clone(), int_ty);
        let ty = synth(&ctx, &pi).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnType(_))
        ));
    }

    #[test]
    fn synth_pi_rejects_non_type_param() {
        // `(x: 42) -> Int` is ill-formed — 42 is a value, not a type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let bad_pi = pi_par(gint_par(42), int_ty);
        let err = synth(&ctx, &bad_pi).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_pi_rejects_non_type_codomain() {
        // `(x: Int) -> 42` is ill-formed — 42 is a value, not a type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let bad_pi = pi_par(int_ty, gint_par(42));
        let err = synth(&ctx, &bad_pi).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_pi_dependent_codomain() {
        // `(A: Type) -> A` is well-formed: in the extended ctx,
        // BoundVar(0) = A has type Type, so the codomain synths to Type.
        let ctx = Context::new();
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        let pi = pi_par(type_univ, bvar_par(0));
        let ty = synth(&ctx, &pi).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnType(_))
        ));
    }

    #[test]
    fn synth_pi_bind_count_mismatch_is_malformed() {
        use models::rhoapi::{EPi, Expr};
        // bind_count claims 2 but only one param_type — malformed proto.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let malformed = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPiBody(EPi {
                bind_count: 2,
                param_types: vec![int_ty.clone()],
                codomain: Some(int_ty),
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }]);
        let err = synth(&ctx, &malformed).unwrap_err();
        assert!(matches!(err, TypeError::NotWellFormed(_)));
    }

    #[test]
    fn synth_pi_nested_pi_param_type_is_type() {
        // `(f: Int -> Int) -> Int` is well-formed: the param_type is
        // itself a Pi, which must synth to Type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let int_to_int = pi_par(int_ty.clone(), int_ty.clone());
        let higher_order = pi_par(int_to_int, int_ty);
        let ty = synth(&ctx, &higher_order).unwrap();
        assert!(matches!(
            connective_of(&ty),
            Some(ConnectiveInstance::ConnType(_))
        ));
    }

    // Build an EApp Par `fun(args...)`.
    fn app_par(fun: Par, args: Vec<Par>) -> Par {
        use models::rhoapi::{EApp, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAppBody(EApp {
                fun: Some(fun),
                args,
                locally_free: Vec::new(),
                connective_used: false,
            })),
        }])
    }

    // Build a Par that carries an int-typed function `f : Int -> Int`
    // via the context. Returns (ctx with f bound, bvar reference to f).
    fn ctx_with_int_endo() -> (Context, Par) {
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let endo = pi_par(int_ty.clone(), int_ty);
        (Context::new().extend(endo), bvar_par(0))
    }

    #[test]
    fn synth_app_dependent_identity_returns_arg_type() {
        // `id : (A: Type) -> A -> A` applied to `Int` should give
        // `Int -> Int` (the codomain `A -> A` with `BoundVar(0) := Int`).
        // We model `id`'s type in the ctx: Pi((A: Type)) -> Pi((_: A)) -> A.
        // Then `id(Int)` has type Pi((_: Int)) -> Int (after sub).
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        // Inner Pi is `(_: A) -> A` where A is BoundVar(0) in the outer scope.
        let inner_pi = pi_par(bvar_par(0), bvar_par(1));
        let outer_pi = pi_par(type_univ, inner_pi);

        let ctx = Context::new().extend(outer_pi);
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let app = app_par(bvar_par(0), vec![int_ty.clone()]);

        let ty = synth(&ctx, &app).unwrap();
        // Expected shape: `(_: Int) -> Int`. Substitute does not refresh
        // the outer `connective_used` book-keeping, so we compare the
        // EPi structure rather than full Par equality.
        let pi = match single_expr(&ty) {
            Some(ExprInstance::EPiBody(pi)) => pi,
            other => panic!("expected EPi, got {other:?}"),
        };
        assert_eq!(pi.bind_count, 1);
        assert_eq!(pi.param_types.len(), 1);
        assert_eq!(
            pi.param_types[0].connectives.first().and_then(|c| c.connective_instance.as_ref()),
            Some(&ConnectiveInstance::ConnInt(true))
        );
        assert_eq!(
            pi.codomain
                .as_ref()
                .and_then(|p| p.connectives.first())
                .and_then(|c| c.connective_instance.as_ref()),
            Some(&ConnectiveInstance::ConnInt(true))
        );
    }

    #[test]
    fn synth_app_non_dependent_returns_codomain_unchanged() {
        // `f : Int -> Int` applied to `42` → `Int`. Non-dependent codomain
        // has no BoundVar refs, so substitution is a no-op.
        let (ctx, f_ref) = ctx_with_int_endo();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let app = app_par(f_ref, vec![gint_par(42)]);

        let ty = synth(&ctx, &app).unwrap();
        assert_eq!(ty, int_ty);
    }

    #[test]
    fn check_annotated_lambda_matches_pi() {
        // `(x: Int) => x` against `(_: Int) -> Int` — annotation matches.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let expected = pi_par(int_ty.clone(), int_ty.clone());
        let lam = lambda_par_typed(1, bvar_par(0), vec![int_ty]);
        check(&ctx, &lam, &expected).unwrap();
    }

    #[test]
    fn check_annotated_lambda_mismatch_errors() {
        // `(x: String) => x` against `(_: Int) -> Int` — annotation type
        // conflicts with the Pi's param type.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let string_ty = type_par(ConnectiveInstance::ConnString(true));
        let expected = pi_par(int_ty, bvar_par(0));
        let lam = lambda_par_typed(1, bvar_par(0), vec![string_ty]);
        let err = check(&ctx, &lam, &expected).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn check_partially_annotated_lambda_accepts_empty_slot() {
        // `(x, y: Int) => y` against `(_: Bool, _: Int) -> Int`. The
        // un-annotated first slot (empty Par) is accepted without
        // cross-check; the annotated second slot must match. Body
        // references BoundVar(1), which under reverse-push context has
        // type Int (= param_types[1]).
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let expected = pi_par_multi(vec![bool_ty, int_ty.clone()], int_ty.clone());
        let lam = lambda_par_typed(2, bvar_par(1), vec![Par::default(), int_ty]);
        check(&ctx, &lam, &expected).unwrap();
    }

    #[test]
    fn synth_fully_annotated_lambda_produces_pi() {
        // `(x: Int) => x` synthesizes to `(_: Int) -> Int`.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let lam = lambda_par_typed(1, bvar_par(0), vec![int_ty.clone()]);

        let ty = synth(&ctx, &lam).unwrap();
        let pi = match single_expr(&ty) {
            Some(ExprInstance::EPiBody(pi)) => pi,
            other => panic!("expected EPi, got {other:?}"),
        };
        assert_eq!(pi.bind_count, 1);
        assert_eq!(
            pi.param_types[0].connectives.first().and_then(|c| c.connective_instance.as_ref()),
            Some(&ConnectiveInstance::ConnInt(true))
        );
        assert_eq!(
            pi.codomain
                .as_ref()
                .and_then(|p| p.connectives.first())
                .and_then(|c| c.connective_instance.as_ref()),
            Some(&ConnectiveInstance::ConnInt(true))
        );
    }

    #[test]
    fn synth_partially_annotated_lambda_errors() {
        // Lambda whose first slot is annotated but second is empty
        // cannot be synthed.
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let lam = lambda_par_typed(2, bvar_par(0), vec![int_ty, Par::default()]);
        let err = synth(&ctx, &lam).unwrap_err();
        assert!(matches!(err, TypeError::Unsupported(_)));
    }

    #[test]
    fn synth_annotated_lambda_rejects_non_type_annotation() {
        // `(x: 42) => x` — annotation is a value, not a type. Pi
        // formation's check(_, pt, Type) catches this.
        let ctx = Context::new();
        let lam = lambda_par_typed(1, bvar_par(0), vec![gint_par(42)]);
        let err = synth(&ctx, &lam).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn check_annotated_lambda_mismatch_in_non_first_slot_errors() {
        // Annotation mismatch in the second slot of a multi-param lambda.
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let string_ty = type_par(ConnectiveInstance::ConnString(true));
        let expected =
            pi_par_multi(vec![bool_ty.clone(), int_ty.clone()], int_ty);
        // Lambda annotates slot 1 as String, which conflicts with the
        // Pi's Int in that slot.
        let lam = lambda_par_typed(2, bvar_par(1), vec![bool_ty, string_ty]);
        let err = check(&ctx, &lam, &expected).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    // Helper for multi-param Pi tests.
    fn pi_par_multi(param_types: Vec<Par>, codomain: Par) -> Par {
        use models::rhoapi::{EPi, Expr};
        let conn_used =
            codomain.connective_used || param_types.iter().any(|p| p.connective_used);
        let bind_count = param_types.len() as i32;
        let mut par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPiBody(EPi {
                bind_count,
                param_types,
                codomain: Some(codomain),
                locally_free: Vec::new(),
                connective_used: conn_used,
            })),
        }]);
        par.connective_used = conn_used;
        par
    }

    #[test]
    fn def_eq_reduces_app_of_identity_to_arg() {
        // `((A: Type) => A)(Int)` β-reduces to `Int`.
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        let id_lam = lambda_par_typed(1, bvar_par(0), vec![type_univ]);
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let redex = app_par(id_lam, vec![int_ty.clone()]);
        assert!(def_eq(&redex, &int_ty));
    }

    #[test]
    fn def_eq_distinct_bases_not_equal() {
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        assert!(!def_eq(&int_ty, &bool_ty));
    }

    #[test]
    fn def_eq_recurses_into_pi_structure() {
        // Pi `(_: Int) -> Int` equals itself; and equals a Pi where
        // param_type was built by reducing `((A: Type) => A)(Int)`.
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let id_lam = lambda_par_typed(1, bvar_par(0), vec![type_univ]);
        let reducible_param = app_par(id_lam, vec![int_ty.clone()]);
        let pi_direct = pi_par(int_ty.clone(), int_ty.clone());
        let pi_reducible = pi_par(reducible_param, int_ty);
        assert!(def_eq(&pi_direct, &pi_reducible));
    }

    // Helper: build an EPlus Par.
    fn plus_par(p1: Par, p2: Par) -> Par {
        use models::rhoapi::{EPlus, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPlusBody(EPlus {
                p1: Some(p1),
                p2: Some(p2),
            })),
        }])
    }

    #[test]
    fn synth_eplus_int_int_is_int() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let e = plus_par(gint_par(1), gint_par(2));
        let ty = synth(&ctx, &e).unwrap();
        assert!(def_eq(&ty, &int_ty));
    }

    #[test]
    fn synth_eplus_rejects_non_int_operand() {
        let ctx = Context::new();
        let e = plus_par(gint_par(1), gstring_par("hello"));
        let err = synth(&ctx, &e).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_eminus_emult_ediv_emod_are_int() {
        use models::rhoapi::{EDiv, EMinus, EMod, EMult, Expr};
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));

        for expr in [
            ExprInstance::EMinusBody(EMinus {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            }),
            ExprInstance::EMultBody(EMult {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            }),
            ExprInstance::EDivBody(EDiv {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            }),
            ExprInstance::EModBody(EMod {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            }),
        ] {
            let par = Par::default().with_exprs(vec![Expr {
                expr_instance: Some(expr),
            }]);
            let ty = synth(&ctx, &par).unwrap();
            assert!(def_eq(&ty, &int_ty));
        }
    }

    #[test]
    fn synth_elt_int_int_is_bool() {
        use models::rhoapi::{ELt, Expr};
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ELtBody(ELt {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            })),
        }]);
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &bool_ty));
    }

    #[test]
    fn synth_eeq_requires_matching_operand_types() {
        use models::rhoapi::{EEq, Expr};
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        // Int == Int → Bool.
        let ok_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EEqBody(EEq {
                p1: Some(gint_par(1)),
                p2: Some(gint_par(2)),
            })),
        }]);
        let ty = synth(&ctx, &ok_par).unwrap();
        assert!(def_eq(&ty, &bool_ty));

        // Int == String → Mismatch.
        let bad_par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EEqBody(EEq {
                p1: Some(gint_par(1)),
                p2: Some(gstring_par("x")),
            })),
        }]);
        let err = synth(&ctx, &bad_par).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_eand_bool_bool_is_bool() {
        use models::rhoapi::{EAnd, Expr};
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAndBody(EAnd {
                p1: Some(gbool_par(true)),
                p2: Some(gbool_par(false)),
            })),
        }]);
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &bool_ty));
    }

    #[test]
    fn synth_enot_bool_is_bool() {
        use models::rhoapi::{ENot, Expr};
        let ctx = Context::new();
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ENotBody(ENot {
                p: Some(gbool_par(true)),
            })),
        }]);
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &bool_ty));
    }

    #[test]
    fn synth_eneg_int_is_int() {
        use models::rhoapi::{ENeg, Expr};
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ENegBody(ENeg {
                p: Some(gint_par(5)),
            })),
        }]);
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &int_ty));
    }

    #[test]
    fn synth_eand_rejects_int_operand() {
        use models::rhoapi::{EAnd, Expr};
        let ctx = Context::new();
        let par = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EAndBody(EAnd {
                p1: Some(gbool_par(true)),
                p2: Some(gint_par(42)),
            })),
        }]);
        let err = synth(&ctx, &par).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_conn_and_of_types_is_type() {
        use models::rhoapi::ConnectiveBody;
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let bool_ty = type_par(ConnectiveInstance::ConnBool(true));
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![int_ty, bool_ty],
            })),
        }]);
        par.connective_used = true;
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &type_par(ConnectiveInstance::ConnType(true))));
    }

    #[test]
    fn synth_conn_and_rejects_non_type_inner() {
        use models::rhoapi::ConnectiveBody;
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnAndBody(ConnectiveBody {
                ps: vec![int_ty, gint_par(7)], // second slot is a value
            })),
        }]);
        par.connective_used = true;
        let err = synth(&ctx, &par).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    #[test]
    fn synth_conn_not_of_type_is_type() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let mut par = Par::default().with_connectives(vec![Connective {
            connective_instance: Some(ConnectiveInstance::ConnNotBody(int_ty)),
        }]);
        par.connective_used = true;
        let ty = synth(&ctx, &par).unwrap();
        assert!(def_eq(&ty, &type_par(ConnectiveInstance::ConnType(true))));
    }

    fn elist_par(elements: Vec<Par>) -> Par {
        use models::rhoapi::{EList, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    fn expect_conn_list(par: &Par) -> &Par {
        match par.connectives.first().and_then(|c| c.connective_instance.as_ref()) {
            Some(ConnectiveInstance::ConnList(elem)) => elem,
            other => panic!("expected ConnList, got {other:?}"),
        }
    }

    #[test]
    fn synth_list_of_ints_is_list_of_int() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let par = elist_par(vec![gint_par(1), gint_par(2), gint_par(3)]);
        let ty = synth(&ctx, &par).unwrap();
        let elem = expect_conn_list(&ty);
        assert!(def_eq(elem, &int_ty));
    }

    #[test]
    fn synth_empty_list_has_unknown_element() {
        let ctx = Context::new();
        let par = elist_par(Vec::new());
        let ty = synth(&ctx, &par).unwrap();
        let elem = expect_conn_list(&ty);
        assert!(is_empty_par(elem), "empty list's element type is Par::default()");
    }

    #[test]
    fn synth_heterogeneous_list_has_or_element() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let string_ty = type_par(ConnectiveInstance::ConnString(true));
        let par = elist_par(vec![gint_par(1), gstring_par("x"), gint_par(2)]);
        let ty = synth(&ctx, &par).unwrap();
        let elem = expect_conn_list(&ty);
        // Should be ConnOr of Int and String.
        match elem.connectives.first().and_then(|c| c.connective_instance.as_ref()) {
            Some(ConnectiveInstance::ConnOrBody(body)) => {
                assert_eq!(body.ps.len(), 2, "deduped to two entries");
                assert!(body.ps.iter().any(|p| def_eq(p, &int_ty)));
                assert!(body.ps.iter().any(|p| def_eq(p, &string_ty)));
            }
            other => panic!("expected ConnOr for heterogeneous list, got {other:?}"),
        }
    }

    #[test]
    fn synth_conn_list_type_is_type_when_element_is_type() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let list_ty = type_par(ConnectiveInstance::ConnList(int_ty));
        let ty = synth(&ctx, &list_ty).unwrap();
        assert!(def_eq(&ty, &type_par(ConnectiveInstance::ConnType(true))));
    }

    #[test]
    fn synth_conn_list_rejects_non_type_element() {
        let ctx = Context::new();
        // ConnList(gint_par(42)): element is a value, not a type.
        let bad = type_par(ConnectiveInstance::ConnList(gint_par(42)));
        let err = synth(&ctx, &bad).unwrap_err();
        assert!(matches!(err, TypeError::Mismatch { .. }));
    }

    fn eset_par(elements: Vec<Par>) -> Par {
        use models::rhoapi::{ESet, Expr};
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: elements,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    #[test]
    fn synth_set_of_ints_is_set_of_int() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let par = eset_par(vec![gint_par(1), gint_par(2)]);
        let ty = synth(&ctx, &par).unwrap();
        match ty.connectives.first().and_then(|c| c.connective_instance.as_ref()) {
            Some(ConnectiveInstance::ConnSet(elem)) => assert!(def_eq(elem, &int_ty)),
            other => panic!("expected ConnSet, got {other:?}"),
        }
    }

    #[test]
    fn synth_conn_set_type_is_type_when_element_is_type() {
        let ctx = Context::new();
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let set_ty = type_par(ConnectiveInstance::ConnSet(int_ty));
        let ty = synth(&ctx, &set_ty).unwrap();
        assert!(def_eq(&ty, &type_par(ConnectiveInstance::ConnType(true))));
    }

    #[test]
    fn check_accepts_beta_equivalent_annotation() {
        // Annotation on the lambda is a reducible expression that
        // β-reduces to Int; the expected Pi has Int directly.
        let ctx = Context::new();
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let id_lam = lambda_par_typed(1, bvar_par(0), vec![type_univ]);
        let reducible_int = app_par(id_lam, vec![int_ty.clone()]);
        // Expected: `(_: Int) -> Int`. Lambda body = x : BoundVar(0) : Int
        // (via check-against-Pi's reverse-push ctx).
        let expected = pi_par(int_ty.clone(), int_ty);
        // Lambda whose annotation is the reducible int.
        let lam = lambda_par_typed(1, bvar_par(0), vec![reducible_int]);
        check(&ctx, &lam, &expected).unwrap();
    }

    #[test]
    fn synth_app_preserves_lambda_in_codomain() {
        // `const : (A: Type) -> (B: Type) -> A -> B -> A`.
        // Model `const`'s type as nested Pis; `const(Int)` should sub
        // BoundVar(0) := Int in the remaining `(B: Type) -> A -> B -> A`.
        // After one app, the type's innermost reference to A is
        // BoundVar(2) (crossed B, then x: A, then y: B). We only verify
        // that substitution produces a well-formed Pi-of-Pi here.
        let type_univ = type_par(ConnectiveInstance::ConnType(true));
        // const_ty = Pi((A: Type)) -> Pi((B: Type)) -> A.
        let inner = pi_par(type_univ.clone(), bvar_par(1));
        let const_ty = pi_par(type_univ, inner);

        let ctx = Context::new().extend(const_ty);
        let int_ty = type_par(ConnectiveInstance::ConnInt(true));
        let app = app_par(bvar_par(0), vec![int_ty.clone()]);

        let ty = synth(&ctx, &app).unwrap();
        // After substituting A := Int in `Pi((B: Type)) -> A`, we expect
        // `Pi((B: Type)) -> Int` (since A is BoundVar(1), crossing B's
        // binder shifts it, but substitution swaps it for Int).
        let expected = pi_par(type_par(ConnectiveInstance::ConnType(true)), int_ty);
        assert_eq!(ty, expected);
    }
}
