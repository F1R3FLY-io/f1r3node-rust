use models::create_bit_vector;
use models::rust::utils::union;

use super::exports::*;
use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

/// Free-variable facts about a Rholang term.
pub trait HasLocallyFree {
    /// Whether a connective, free variable or wildcard occurs anywhere in the
    /// term, which makes it a pattern rather than a concrete term.
    fn connective_used(&self) -> bool;

    /// The variables locally free in the term when it sits at pattern nesting
    /// `depth` (a top-level term is depth 0, a pattern inside it depth 1, and
    /// so on). Terms that cache the set return the cache, which is correct
    /// only when asked at the term's actual depth.
    fn locally_free(&self, depth: i32) -> Vec<u8>;
}

fn required<'a, T>(field: &'a Option<T>, name: &str) -> &'a T {
    field
        .as_ref()
        .unwrap_or_else(|| panic!("normalized term is missing its {name}"))
}

impl HasLocallyFree for Par {
    fn connective_used(&self) -> bool { self.connective_used }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { self.locally_free.clone() }
}

impl HasLocallyFree for (Par, Par) {
    fn connective_used(&self) -> bool { self.0.connective_used || self.1.connective_used }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        union(self.0.locally_free(depth), self.1.locally_free(depth))
    }
}

impl HasLocallyFree for Bundle {
    fn connective_used(&self) -> bool { false }

    fn locally_free(&self, _depth: i32) -> Vec<u8> {
        required(&self.body, "bundle body").locally_free.clone()
    }
}

impl HasLocallyFree for Send {
    fn connective_used(&self) -> bool { self.connective_used }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { self.locally_free.clone() }
}

impl HasLocallyFree for GUnforgeable {
    fn connective_used(&self) -> bool { false }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { Vec::new() }
}

impl HasLocallyFree for Expr {
    fn connective_used(&self) -> bool {
        match &self.expr_instance {
            None
            | Some(
                GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
                | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
                | GUri(_) | GByteArray(_),
            ) => false,

            Some(EListBody(e)) => e.connective_used,
            Some(ETupleBody(e)) => e.connective_used,
            Some(ESetBody(e)) => e.connective_used,
            Some(EMapBody(e)) => e.connective_used,
            Some(EPathmapBody(e)) => e.connective_used,
            Some(EZipperBody(e)) => e.connective_used,
            Some(EMethodBody(e)) => e.connective_used,

            Some(EVarBody(EVar { v })) => required(v, "variable").connective_used(),
            Some(ENotBody(ENot { p }) | ENegBody(ENeg { p })) => {
                required(p, "operand").connective_used
            }
            Some(EMatchesBody(EMatches { target, .. })) => {
                required(target, "target").connective_used
            }

            Some(
                EMultBody(EMult { p1, p2 })
                | EDivBody(EDiv { p1, p2 })
                | EModBody(EMod { p1, p2 })
                | EPlusBody(EPlus { p1, p2 })
                | EMinusBody(EMinus { p1, p2 })
                | ELtBody(ELt { p1, p2 })
                | ELteBody(ELte { p1, p2 })
                | EGtBody(EGt { p1, p2 })
                | EGteBody(EGte { p1, p2 })
                | EEqBody(EEq { p1, p2 })
                | ENeqBody(ENeq { p1, p2 })
                | EAndBody(EAnd { p1, p2 })
                | EOrBody(EOr { p1, p2 })
                | EPercentPercentBody(EPercentPercent { p1, p2 })
                | EPlusPlusBody(EPlusPlus { p1, p2 })
                | EMinusMinusBody(EMinusMinus { p1, p2 }),
            ) => {
                required(p1, "left operand").connective_used
                    | required(p2, "right operand").connective_used
            }
        }
    }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        match &self.expr_instance {
            None
            | Some(
                GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
                | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
                | GUri(_) | GByteArray(_),
            ) => Vec::new(),

            Some(EListBody(e)) => e.locally_free.clone(),
            Some(ETupleBody(e)) => e.locally_free.clone(),
            Some(ESetBody(e)) => e.locally_free.clone(),
            Some(EMapBody(e)) => e.locally_free.clone(),
            Some(EPathmapBody(e)) => e.locally_free.clone(),
            Some(EZipperBody(e)) => e.locally_free.clone(),
            Some(EMethodBody(e)) => e.locally_free.clone(),

            Some(EVarBody(EVar { v })) => required(v, "variable").locally_free(depth),
            Some(ENotBody(ENot { p }) | ENegBody(ENeg { p })) => {
                required(p, "operand").locally_free.clone()
            }
            Some(EMatchesBody(EMatches { target, .. })) => {
                required(target, "target").locally_free.clone()
            }

            Some(
                EMultBody(EMult { p1, p2 })
                | EDivBody(EDiv { p1, p2 })
                | EModBody(EMod { p1, p2 })
                | EPlusBody(EPlus { p1, p2 })
                | EMinusBody(EMinus { p1, p2 })
                | ELtBody(ELt { p1, p2 })
                | ELteBody(ELte { p1, p2 })
                | EGtBody(EGt { p1, p2 })
                | EGteBody(EGte { p1, p2 })
                | EEqBody(EEq { p1, p2 })
                | ENeqBody(ENeq { p1, p2 })
                | EAndBody(EAnd { p1, p2 })
                | EOrBody(EOr { p1, p2 })
                | EPercentPercentBody(EPercentPercent { p1, p2 })
                | EPlusPlusBody(EPlusPlus { p1, p2 })
                | EMinusMinusBody(EMinusMinus { p1, p2 }),
            ) => union(
                required(p1, "left operand").locally_free.clone(),
                required(p2, "right operand").locally_free.clone(),
            ),
        }
    }
}

impl HasLocallyFree for New {
    fn connective_used(&self) -> bool { required(&self.p, "body").connective_used }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { self.locally_free.clone() }
}

impl HasLocallyFree for VarInstance {
    fn connective_used(&self) -> bool {
        match self {
            BoundVar(_) => false,
            FreeVar(_) | Wildcard(_) => true,
        }
    }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        match self {
            BoundVar(index) if depth == 0 => create_bit_vector(&[*index as usize]),
            BoundVar(_) | FreeVar(_) | Wildcard(_) => Vec::new(),
        }
    }
}

impl HasLocallyFree for Var {
    fn connective_used(&self) -> bool {
        required(&self.var_instance, "variable instance").connective_used()
    }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        required(&self.var_instance, "variable instance").locally_free(depth)
    }
}

impl HasLocallyFree for Receive {
    fn connective_used(&self) -> bool { self.connective_used }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { self.locally_free.clone() }
}

impl HasLocallyFree for ReceiveBind {
    fn connective_used(&self) -> bool { required(&self.source, "source").connective_used }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        union(
            required(&self.source, "source").locally_free(depth),
            self.patterns.iter().fold(Vec::new(), |acc, pattern| {
                union(acc, pattern.locally_free(depth + 1))
            }),
        )
    }
}

impl HasLocallyFree for Match {
    fn connective_used(&self) -> bool { self.connective_used }

    fn locally_free(&self, _depth: i32) -> Vec<u8> { self.locally_free.clone() }
}

impl HasLocallyFree for MatchCase {
    fn connective_used(&self) -> bool { required(&self.source, "source").connective_used }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        union(
            required(&self.source, "source").locally_free.clone(),
            required(&self.pattern, "pattern").locally_free(depth + 1),
        )
    }
}

impl HasLocallyFree for Connective {
    fn connective_used(&self) -> bool {
        match &self.connective_instance {
            None | Some(VarRefBody(_)) => false,
            Some(
                ConnAndBody(_) | ConnOrBody(_) | ConnNotBody(_) | ConnBool(_) | ConnInt(_)
                | ConnString(_) | ConnUri(_) | ConnByteArray(_),
            ) => true,
        }
    }

    fn locally_free(&self, depth: i32) -> Vec<u8> {
        match &self.connective_instance {
            Some(VarRefBody(VarRef {
                index,
                depth: var_depth,
            })) if depth == *var_depth => create_bit_vector(&[*index as usize]),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::EMethod;
    use models::rust::utils::{new_freevar_par, new_gint_par};

    use super::*;

    fn free_par() -> Par {
        let mut par = new_freevar_par(0, Vec::new());
        par.connective_used = true;
        par
    }

    fn marked_par(bit: usize) -> Par {
        let mut par = new_gint_par(0, Vec::new(), false);
        par.locally_free = create_bit_vector(&[bit]);
        par
    }

    fn plain_par() -> Par { new_gint_par(0, Vec::new(), false) }

    type BinOp = fn(Par, Par) -> ExprInstance;

    fn binary_ops() -> Vec<BinOp> {
        vec![
            |p1, p2| {
                EMultBody(EMult {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EDivBody(EDiv {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EModBody(EMod {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EPlusBody(EPlus {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EMinusBody(EMinus {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                ELtBody(ELt {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                ELteBody(ELte {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EGtBody(EGt {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EGteBody(EGte {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EEqBody(EEq {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                ENeqBody(ENeq {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EAndBody(EAnd {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EOrBody(EOr {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EPercentPercentBody(EPercentPercent {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EPlusPlusBody(EPlusPlus {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
            |p1, p2| {
                EMinusMinusBody(EMinusMinus {
                    p1: Some(p1),
                    p2: Some(p2),
                })
            },
        ]
    }

    fn expr_of(instance: ExprInstance) -> Expr {
        Expr {
            expr_instance: Some(instance),
        }
    }

    #[test]
    fn binary_operator_exprs_or_their_operands() {
        for op in binary_ops() {
            assert!(expr_of(op(free_par(), plain_par())).connective_used());
            assert!(expr_of(op(plain_par(), free_par())).connective_used());
            assert!(!expr_of(op(plain_par(), plain_par())).connective_used());
            assert_eq!(
                expr_of(op(marked_par(0), marked_par(1))).locally_free(0),
                union(create_bit_vector(&[0]), create_bit_vector(&[1]))
            );
        }
    }

    #[test]
    fn ground_exprs_are_concrete_with_no_free_variables() {
        for ground in [
            GBool(true),
            GInt(1),
            GString("s".to_string()),
            GUri("u".to_string()),
            GByteArray(vec![1]),
            GDouble(1.5f64.to_bits()),
        ] {
            let e = expr_of(ground);
            assert!(!e.connective_used());
            assert!(e.locally_free(0).is_empty());
        }
        let empty = Expr {
            expr_instance: None,
        };
        assert!(!empty.connective_used());
        assert!(empty.locally_free(0).is_empty());
    }

    #[test]
    fn unary_method_and_matches_exprs_delegate_to_their_bodies() {
        let not_expr = expr_of(ENotBody(ENot {
            p: Some(free_par()),
        }));
        assert!(not_expr.connective_used());
        assert!(not_expr.locally_free(0).is_empty());

        let neg_expr = expr_of(ENegBody(ENeg {
            p: Some(marked_par(2)),
        }));
        assert!(!neg_expr.connective_used());
        assert_eq!(neg_expr.locally_free(0), create_bit_vector(&[2]));

        let matches_expr = expr_of(EMatchesBody(EMatches {
            target: Some(marked_par(1)),
            pattern: Some(free_par()),
        }));
        assert!(!matches_expr.connective_used());
        assert_eq!(matches_expr.locally_free(0), create_bit_vector(&[1]));

        let method_expr = expr_of(EMethodBody(EMethod {
            method_name: "nth".to_string(),
            target: Some(plain_par()),
            arguments: vec![],
            locally_free: create_bit_vector(&[3]),
            connective_used: true,
        }));
        assert!(method_expr.connective_used());
        assert_eq!(method_expr.locally_free(0), create_bit_vector(&[3]));
    }

    #[test]
    fn collection_exprs_read_their_cached_fields() {
        let list = expr_of(EListBody(EList {
            ps: vec![],
            locally_free: create_bit_vector(&[1]),
            connective_used: true,
            remainder: None,
        }));
        assert!(list.connective_used());
        assert_eq!(list.locally_free(0), create_bit_vector(&[1]));

        let tuple = expr_of(ETupleBody(ETuple {
            ps: vec![],
            locally_free: create_bit_vector(&[2]),
            connective_used: false,
        }));
        assert!(!tuple.connective_used());
        assert_eq!(tuple.locally_free(0), create_bit_vector(&[2]));
    }

    #[test]
    fn var_instances_classify_and_report_bound_indices() {
        assert!(!BoundVar(1).connective_used());
        assert!(FreeVar(1).connective_used());
        assert!(Wildcard(Default::default()).connective_used());

        assert_eq!(BoundVar(2).locally_free(0), create_bit_vector(&[2]));
        assert!(BoundVar(2).locally_free(1).is_empty());
        assert!(FreeVar(2).locally_free(0).is_empty());
        assert!(Wildcard(Default::default()).locally_free(0).is_empty());

        let bound_var = Var {
            var_instance: Some(BoundVar(3)),
        };
        assert!(!bound_var.connective_used());
        assert_eq!(bound_var.locally_free(0), create_bit_vector(&[3]));

        let evar = expr_of(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(BoundVar(1)),
            }),
        }));
        assert!(!evar.connective_used());
        assert_eq!(evar.locally_free(0), create_bit_vector(&[1]));
    }

    #[test]
    fn structural_terms_read_their_cached_fields() {
        let par = Par {
            connective_used: true,
            locally_free: create_bit_vector(&[1]),
            ..Default::default()
        };
        assert!(par.connective_used());
        assert_eq!(par.locally_free(0), create_bit_vector(&[1]));

        assert!((par.clone(), plain_par()).connective_used());
        assert!(!(plain_par(), plain_par()).connective_used());
        assert_eq!(
            (par, marked_par(2)).locally_free(0),
            union(create_bit_vector(&[1]), create_bit_vector(&[2]))
        );

        let bundle = Bundle {
            body: Some(marked_par(2)),
            write_flag: false,
            read_flag: false,
        };
        assert!(!bundle.connective_used());
        assert_eq!(bundle.locally_free(0), create_bit_vector(&[2]));

        let send = Send {
            chan: None,
            data: vec![],
            persistent: false,
            locally_free: create_bit_vector(&[3]),
            connective_used: true,
        };
        assert!(send.connective_used());
        assert_eq!(send.locally_free(0), create_bit_vector(&[3]));

        let unforgeable = GUnforgeable { unf_instance: None };
        assert!(!unforgeable.connective_used());
        assert!(unforgeable.locally_free(0).is_empty());

        let new_term = New {
            bind_count: 1,
            p: Some(free_par()),
            locally_free: create_bit_vector(&[4]),
            ..Default::default()
        };
        assert!(new_term.connective_used());
        assert_eq!(new_term.locally_free(0), create_bit_vector(&[4]));

        let receive = Receive {
            binds: vec![],
            body: None,
            persistent: false,
            peek: false,
            bind_count: 0,
            locally_free: create_bit_vector(&[5]),
            connective_used: true,
            condition: None,
        };
        assert!(receive.connective_used());
        assert_eq!(receive.locally_free(0), create_bit_vector(&[5]));

        let match_term = Match {
            target: Some(plain_par()),
            cases: vec![],
            locally_free: create_bit_vector(&[6]),
            connective_used: true,
        };
        assert!(match_term.connective_used());
        assert_eq!(match_term.locally_free(0), create_bit_vector(&[6]));
    }

    #[test]
    fn receive_binds_and_match_cases_union_sources_and_patterns() {
        let bind = ReceiveBind {
            cost_signature: None,
            patterns: vec![marked_par(1)],
            source: Some(marked_par(2)),
            remainder: None,
            free_count: 0,
        };
        assert!(!bind.connective_used());
        assert_eq!(
            bind.locally_free(0),
            union(create_bit_vector(&[1]), create_bit_vector(&[2]))
        );

        let dirty_bind = ReceiveBind {
            cost_signature: None,
            patterns: vec![],
            source: Some(free_par()),
            remainder: None,
            free_count: 1,
        };
        assert!(dirty_bind.connective_used());

        let case = MatchCase {
            pattern: Some(marked_par(1)),
            source: Some(marked_par(2)),
            free_count: 0,
            guard: None,
        };
        assert!(!case.connective_used());
        assert_eq!(
            case.locally_free(0),
            union(create_bit_vector(&[2]), create_bit_vector(&[1]))
        );
    }

    #[test]
    fn connectives_classify_and_var_refs_report_depth_matched_indices() {
        let conn = |instance: ConnectiveInstance| Connective {
            connective_instance: Some(instance),
        };

        for used in [
            conn(ConnAndBody(ConnectiveBody { ps: vec![] })),
            conn(ConnOrBody(ConnectiveBody { ps: vec![] })),
            conn(ConnNotBody(Par::default())),
            conn(ConnBool(true)),
            conn(ConnInt(true)),
            conn(ConnString(true)),
            conn(ConnUri(true)),
            conn(ConnByteArray(true)),
        ] {
            assert!(used.connective_used());
            assert!(used.locally_free(1).is_empty());
        }

        let var_ref = conn(VarRefBody(VarRef { index: 2, depth: 1 }));
        assert!(!var_ref.connective_used());
        assert_eq!(var_ref.locally_free(1), create_bit_vector(&[2]));
        assert!(var_ref.locally_free(0).is_empty());

        let unset = Connective {
            connective_instance: None,
        };
        assert!(!unset.connective_used());
        assert!(unset.locally_free(0).is_empty());
    }
}

fn signature_locally_free(signature: &CostSignature) -> Vec<u8> {
    use models::rhoapi::cost_signature::Value;

    match &signature.value {
        Some(Value::BoundLevel(level)) if *level >= 0 => create_bit_vector(&[*level as usize]),
        Some(Value::Quote(par)) | Some(Value::Name(par)) => par.locally_free.clone(),
        Some(Value::Compound(compound)) => compound
            .elements
            .iter()
            .map(signature_locally_free)
            .fold(Vec::new(), union),
        _ => Vec::new(),
    }
}

fn signature_connective_used(signature: &CostSignature) -> bool {
    use models::rhoapi::cost_signature::Value;

    match &signature.value {
        Some(Value::Quote(par)) | Some(Value::Name(par)) => par.connective_used,
        Some(Value::Compound(compound)) => compound.elements.iter().any(signature_connective_used),
        _ => false,
    }
}

impl HasLocallyFree for If {
    fn connective_used(&self) -> bool {
        self.condition.as_ref().is_some_and(|p| p.connective_used)
            || self.if_true.as_ref().is_some_and(|p| p.connective_used)
            || self.if_false.as_ref().is_some_and(|p| p.connective_used)
    }

    fn locally_free(&self, _depth: i32) -> Vec<u8> {
        [&self.condition, &self.if_true, &self.if_false]
            .into_iter()
            .flatten()
            .map(|p| p.locally_free.clone())
            .fold(Vec::new(), union)
    }
}

impl HasLocallyFree for CostSignedTerm {
    fn connective_used(&self) -> bool {
        self.body.as_ref().is_some_and(|p| p.connective_used)
            || self
                .signature
                .as_ref()
                .is_some_and(signature_connective_used)
    }

    fn locally_free(&self, _depth: i32) -> Vec<u8> {
        union(
            self.body
                .as_ref()
                .map(|p| p.locally_free.clone())
                .unwrap_or_default(),
            self.signature
                .as_ref()
                .map(signature_locally_free)
                .unwrap_or_default(),
        )
    }
}

impl HasLocallyFree for CostStack {
    fn connective_used(&self) -> bool { self.cells.iter().any(signature_connective_used) }

    fn locally_free(&self, _depth: i32) -> Vec<u8> {
        self.cells
            .iter()
            .map(signature_locally_free)
            .fold(Vec::new(), union)
    }
}

/// D-D2 (D-M8, DR-104): the predicates of [`HasLocallyFree`] on a borrowed
/// value, so the matcher does not copy a value only to test it.
/// `connective_used_ref(&x)` equals `connective_used(x.clone())`, and
/// `locally_free_is_empty(&x, depth)` equals
/// `locally_free(x.clone(), depth).is_empty()`, because a union of bitsets is
/// empty exactly when every part is empty
/// (`MatcherReadsByReference.fold_union_empty_iff_all_empty`). Both panic where
/// the owned versions panic: on a missing field that the owned version
/// unwraps, and on a negative bound variable at depth 0. They allocate nothing.
pub trait HasLocallyFreeRef<T> {
    fn connective_used_ref(&self, source: &T) -> bool;

    fn locally_free_is_empty(&self, source: &T, depth: i32) -> bool;
}

fn required_par(par: &Option<Par>) -> &Par {
    par.as_ref()
        .expect("the owned predicate unwraps this field")
}

fn binary_connective_used(p1: &Option<Par>, p2: &Option<Par>) -> bool {
    let left = required_par(p1).connective_used;
    let right = required_par(p2).connective_used;
    left | right
}

fn binary_locally_free_is_empty(p1: &Option<Par>, p2: &Option<Par>) -> bool {
    let left = required_par(p1).locally_free.is_empty();
    let right = required_par(p2).locally_free.is_empty();
    left && right
}

fn var_connective_used(v: &Option<Var>) -> bool {
    match v
        .as_ref()
        .expect("the owned predicate unwraps the variable")
        .var_instance
        .as_ref()
        .expect("the owned predicate unwraps the variable instance")
    {
        BoundVar(_) => false,
        FreeVar(_) | Wildcard(_) => true,
    }
}

fn var_locally_free_is_empty(v: &Option<Var>, depth: i32) -> bool {
    match v
        .as_ref()
        .expect("the owned predicate unwraps the variable")
        .var_instance
        .as_ref()
        .expect("the owned predicate unwraps the variable instance")
    {
        BoundVar(index) if depth == 0 => {
            assert!(
                *index >= 0,
                "a negative bound variable has no locally free bitset"
            );
            false
        }
        BoundVar(_) | FreeVar(_) | Wildcard(_) => true,
    }
}

fn signature_locally_free_is_empty(signature: &CostSignature) -> bool {
    use models::rhoapi::cost_signature::Value;

    match &signature.value {
        Some(Value::BoundLevel(level)) if *level >= 0 => false,
        Some(Value::Quote(par)) | Some(Value::Name(par)) => par.locally_free.is_empty(),
        Some(Value::Compound(compound)) => compound
            .elements
            .iter()
            .all(signature_locally_free_is_empty),
        _ => true,
    }
}

impl HasLocallyFreeRef<(Par, Par)> for SpatialMatcherContext {
    fn connective_used_ref(&self, source: &(Par, Par)) -> bool {
        source.0.connective_used || source.1.connective_used
    }

    fn locally_free_is_empty(&self, source: &(Par, Par), _depth: i32) -> bool {
        source.0.locally_free.is_empty() && source.1.locally_free.is_empty()
    }
}

impl HasLocallyFreeRef<Par> for SpatialMatcherContext {
    fn connective_used_ref(&self, p: &Par) -> bool { p.connective_used }

    fn locally_free_is_empty(&self, p: &Par, _depth: i32) -> bool { p.locally_free.is_empty() }
}

impl HasLocallyFreeRef<If> for SpatialMatcherContext {
    fn connective_used_ref(&self, source: &If) -> bool {
        source.condition.as_ref().is_some_and(|p| p.connective_used)
            || source.if_true.as_ref().is_some_and(|p| p.connective_used)
            || source.if_false.as_ref().is_some_and(|p| p.connective_used)
    }

    fn locally_free_is_empty(&self, source: &If, _depth: i32) -> bool {
        [&source.condition, &source.if_true, &source.if_false]
            .into_iter()
            .flatten()
            .all(|p| p.locally_free.is_empty())
    }
}

impl HasLocallyFreeRef<CostSignedTerm> for SpatialMatcherContext {
    fn connective_used_ref(&self, source: &CostSignedTerm) -> bool {
        source.body.as_ref().is_some_and(|p| p.connective_used)
            || source
                .signature
                .as_ref()
                .is_some_and(signature_connective_used)
    }

    fn locally_free_is_empty(&self, source: &CostSignedTerm, _depth: i32) -> bool {
        source
            .body
            .as_ref()
            .is_none_or(|p| p.locally_free.is_empty())
            && source
                .signature
                .as_ref()
                .is_none_or(signature_locally_free_is_empty)
    }
}

impl HasLocallyFreeRef<CostStack> for SpatialMatcherContext {
    fn connective_used_ref(&self, source: &CostStack) -> bool {
        source.cells.iter().any(signature_connective_used)
    }

    fn locally_free_is_empty(&self, source: &CostStack, _depth: i32) -> bool {
        source.cells.iter().all(signature_locally_free_is_empty)
    }
}

impl HasLocallyFreeRef<Bundle> for SpatialMatcherContext {
    fn connective_used_ref(&self, _source: &Bundle) -> bool { false }

    fn locally_free_is_empty(&self, source: &Bundle, _depth: i32) -> bool {
        required_par(&source.body).locally_free.is_empty()
    }
}

impl HasLocallyFreeRef<Send> for SpatialMatcherContext {
    fn connective_used_ref(&self, s: &Send) -> bool { s.connective_used }

    fn locally_free_is_empty(&self, s: &Send, _depth: i32) -> bool { s.locally_free.is_empty() }
}

impl HasLocallyFreeRef<GUnforgeable> for SpatialMatcherContext {
    fn connective_used_ref(&self, _unf: &GUnforgeable) -> bool { false }

    fn locally_free_is_empty(&self, _unf: &GUnforgeable, _depth: i32) -> bool { true }
}

impl HasLocallyFreeRef<Expr> for SpatialMatcherContext {
    fn connective_used_ref(&self, e: &Expr) -> bool {
        match &e.expr_instance {
            Some(
                GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
                | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
                | GUri(_) | GByteArray(_),
            ) => false,

            Some(EListBody(e)) => e.connective_used,
            Some(ETupleBody(e)) => e.connective_used,
            Some(ESetBody(e)) => e.connective_used,
            Some(EMapBody(e)) => e.connective_used,
            Some(EPathmapBody(e)) => e.connective_used,
            Some(EZipperBody(e)) => e.connective_used,

            Some(EVarBody(EVar { v })) => var_connective_used(v),
            Some(ENotBody(ENot { p }) | ENegBody(ENeg { p })) => required_par(p).connective_used,

            Some(
                EMultBody(EMult { p1, p2 })
                | EDivBody(EDiv { p1, p2 })
                | EModBody(EMod { p1, p2 })
                | EPlusBody(EPlus { p1, p2 })
                | EMinusBody(EMinus { p1, p2 })
                | ELtBody(ELt { p1, p2 })
                | ELteBody(ELte { p1, p2 })
                | EGtBody(EGt { p1, p2 })
                | EGteBody(EGte { p1, p2 })
                | EEqBody(EEq { p1, p2 })
                | ENeqBody(ENeq { p1, p2 })
                | EAndBody(EAnd { p1, p2 })
                | EOrBody(EOr { p1, p2 })
                | EPercentPercentBody(EPercentPercent { p1, p2 })
                | EPlusPlusBody(EPlusPlus { p1, p2 })
                | EMinusMinusBody(EMinusMinus { p1, p2 }),
            ) => binary_connective_used(p1, p2),

            Some(EMethodBody(e)) => e.connective_used,
            Some(EMatchesBody(EMatches { target, .. })) => required_par(target).connective_used,

            None => false,
        }
    }

    fn locally_free_is_empty(&self, e: &Expr, depth: i32) -> bool {
        match &e.expr_instance {
            Some(
                GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
                | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
                | GUri(_) | GByteArray(_),
            ) => true,

            Some(EListBody(e)) => e.locally_free.is_empty(),
            Some(ETupleBody(e)) => e.locally_free.is_empty(),
            Some(ESetBody(e)) => e.locally_free.is_empty(),
            Some(EMapBody(e)) => e.locally_free.is_empty(),
            Some(EPathmapBody(e)) => e.locally_free.is_empty(),
            Some(EZipperBody(e)) => e.locally_free.is_empty(),

            Some(EVarBody(EVar { v })) => var_locally_free_is_empty(v, depth),
            Some(ENotBody(ENot { p }) | ENegBody(ENeg { p })) => {
                required_par(p).locally_free.is_empty()
            }

            Some(
                EMultBody(EMult { p1, p2 })
                | EDivBody(EDiv { p1, p2 })
                | EModBody(EMod { p1, p2 })
                | EPlusBody(EPlus { p1, p2 })
                | EMinusBody(EMinus { p1, p2 })
                | ELtBody(ELt { p1, p2 })
                | ELteBody(ELte { p1, p2 })
                | EGtBody(EGt { p1, p2 })
                | EGteBody(EGte { p1, p2 })
                | EEqBody(EEq { p1, p2 })
                | ENeqBody(ENeq { p1, p2 })
                | EAndBody(EAnd { p1, p2 })
                | EOrBody(EOr { p1, p2 })
                | EPercentPercentBody(EPercentPercent { p1, p2 })
                | EPlusPlusBody(EPlusPlus { p1, p2 })
                | EMinusMinusBody(EMinusMinus { p1, p2 }),
            ) => binary_locally_free_is_empty(p1, p2),

            Some(EMethodBody(e)) => e.locally_free.is_empty(),
            Some(EMatchesBody(EMatches { target, .. })) => {
                required_par(target).locally_free.is_empty()
            }

            None => true,
        }
    }
}

impl HasLocallyFreeRef<New> for SpatialMatcherContext {
    fn connective_used_ref(&self, n: &New) -> bool { required_par(&n.p).connective_used }

    fn locally_free_is_empty(&self, n: &New, _depth: i32) -> bool { n.locally_free.is_empty() }
}

impl HasLocallyFreeRef<Receive> for SpatialMatcherContext {
    fn connective_used_ref(&self, r: &Receive) -> bool { r.connective_used }

    fn locally_free_is_empty(&self, r: &Receive, _depth: i32) -> bool { r.locally_free.is_empty() }
}

impl HasLocallyFreeRef<ReceiveBind> for SpatialMatcherContext {
    fn connective_used_ref(&self, rb: &ReceiveBind) -> bool {
        required_par(&rb.source).connective_used
    }

    fn locally_free_is_empty(&self, rb: &ReceiveBind, _depth: i32) -> bool {
        let source = required_par(&rb.source).locally_free.is_empty();
        source && rb.patterns.iter().all(|p| p.locally_free.is_empty())
    }
}

impl HasLocallyFreeRef<Match> for SpatialMatcherContext {
    fn connective_used_ref(&self, m: &Match) -> bool { m.connective_used }

    fn locally_free_is_empty(&self, m: &Match, _depth: i32) -> bool { m.locally_free.is_empty() }
}

impl HasLocallyFreeRef<MatchCase> for SpatialMatcherContext {
    fn connective_used_ref(&self, mc: &MatchCase) -> bool {
        required_par(&mc.source).connective_used
    }

    fn locally_free_is_empty(&self, mc: &MatchCase, _depth: i32) -> bool {
        let source = required_par(&mc.source).locally_free.is_empty();
        let pattern = required_par(&mc.pattern).locally_free.is_empty();
        source && pattern
    }
}

#[cfg(test)]
mod by_reference_tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    use models::rhoapi::cost_signature::Value;
    use models::rhoapi::{CostSignatureCompound, EMethod, EZipper};
    use proptest::prelude::*;
    use proptest::test_runner::TestCaseError;

    use super::*;

    fn cached() -> impl Strategy<Value = (bool, Vec<u8>)> {
        (any::<bool>(), proptest::collection::vec(0u8..2, 0..3))
    }

    fn par() -> impl Strategy<Value = Par> {
        cached().prop_map(|(connective_used, locally_free)| Par {
            connective_used,
            locally_free,
            ..Default::default()
        })
    }

    fn optional_par() -> impl Strategy<Value = Option<Par>> {
        proptest::option::weighted(0.85, par())
    }

    fn var() -> impl Strategy<Value = Option<Var>> {
        let instance = prop_oneof![
            1 => Just(None),
            4 => (-3i32..64).prop_map(|index| Some(BoundVar(index))),
            2 => (0i32..4).prop_map(|level| Some(FreeVar(level))),
            2 => Just(Some(Wildcard(WildcardMsg {}))),
        ];
        proptest::option::weighted(0.85, instance.prop_map(|var_instance| Var { var_instance }))
    }

    fn signature() -> impl Strategy<Value = CostSignature> {
        let leaf = prop_oneof![
            Just(None),
            (-3i32..64).prop_map(|level| Some(Value::BoundLevel(level))),
            par().prop_map(|p| Some(Value::Quote(p))),
            par().prop_map(|p| Some(Value::Name(p))),
            proptest::collection::vec(any::<u8>(), 0..3)
                .prop_map(|bytes| Some(Value::Ground(bytes))),
            any::<bool>().prop_map(|unit| Some(Value::Unit(unit))),
        ]
        .prop_map(|value| CostSignature { value });
        leaf.prop_recursive(2, 8, 3, |inner| {
            proptest::collection::vec(inner, 0..3).prop_map(|elements| CostSignature {
                value: Some(Value::Compound(CostSignatureCompound { elements })),
            })
        })
    }

    /// Every variant of `Expr`, including missing operands and variables.
    fn expr() -> impl Strategy<Value = Expr> {
        (0usize..43, optional_par(), optional_par(), var(), cached()).prop_map(
            |(variant, p1, p2, v, (connective_used, locally_free))| {
                let expr_instance = match variant {
                    0 => None,
                    1 => Some(GBool(connective_used)),
                    2 => Some(GInt(7)),
                    3 => Some(GUint64(7)),
                    4 => Some(GInt32(7)),
                    5 => Some(GUint32(7)),
                    6 => Some(GUint16(7)),
                    7 => Some(GUint8(7)),
                    8 => Some(GDouble(7)),
                    9 => Some(GFloat32(7)),
                    10 => Some(GBigInt(vec![7])),
                    11 => Some(GBigRat(GBigRational::default())),
                    12 => Some(GFixedPoint(Default::default())),
                    13 => Some(GString("s".to_owned())),
                    14 => Some(GUri("u".to_owned())),
                    15 => Some(GByteArray(vec![7])),
                    16 => Some(EListBody(EList {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    17 => Some(ETupleBody(ETuple {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    18 => Some(ESetBody(ESet {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    19 => Some(EMapBody(EMap {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    20 => Some(EPathmapBody(EPathMap {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    21 => Some(EZipperBody(EZipper {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    22 => Some(EVarBody(EVar { v })),
                    23 => Some(ENotBody(ENot { p: p1 })),
                    24 => Some(ENegBody(ENeg { p: p1 })),
                    25 => Some(EMultBody(EMult { p1, p2 })),
                    26 => Some(EDivBody(EDiv { p1, p2 })),
                    27 => Some(EModBody(EMod { p1, p2 })),
                    28 => Some(EPlusBody(EPlus { p1, p2 })),
                    29 => Some(EMinusBody(EMinus { p1, p2 })),
                    30 => Some(ELtBody(ELt { p1, p2 })),
                    31 => Some(ELteBody(ELte { p1, p2 })),
                    32 => Some(EGtBody(EGt { p1, p2 })),
                    33 => Some(EGteBody(EGte { p1, p2 })),
                    34 => Some(EEqBody(EEq { p1, p2 })),
                    35 => Some(ENeqBody(ENeq { p1, p2 })),
                    36 => Some(EAndBody(EAnd { p1, p2 })),
                    37 => Some(EOrBody(EOr { p1, p2 })),
                    38 => Some(EMethodBody(EMethod {
                        connective_used,
                        locally_free,
                        ..Default::default()
                    })),
                    39 => Some(EMatchesBody(EMatches {
                        target: p1,
                        pattern: p2,
                    })),
                    40 => Some(EPercentPercentBody(EPercentPercent { p1, p2 })),
                    41 => Some(EPlusPlusBody(EPlusPlus { p1, p2 })),
                    _ => Some(EMinusMinusBody(EMinusMinus { p1, p2 })),
                };
                Expr { expr_instance }
            },
        )
    }

    /// Both closures panic, or both return the same value.
    fn same_outcome(
        owned: impl FnOnce() -> bool,
        by_reference: impl FnOnce() -> bool,
    ) -> Result<(), TestCaseError> {
        let owned = catch_unwind(AssertUnwindSafe(owned));
        let by_reference = catch_unwind(AssertUnwindSafe(by_reference));
        match (owned, by_reference) {
            (Ok(owned), Ok(by_reference)) => prop_assert_eq!(owned, by_reference),
            (Err(_), Err(_)) => {}
            (owned, by_reference) => prop_assert!(
                false,
                "owned panicked: {}, by reference panicked: {}",
                owned.is_err(),
                by_reference.is_err()
            ),
        }
        Ok(())
    }

    fn check<T>(context: &SpatialMatcherContext, value: T) -> Result<(), TestCaseError>
    where
        T: Clone + HasLocallyFree,
        SpatialMatcherContext: HasLocallyFreeRef<T>,
    {
        same_outcome(
            || value.connective_used(),
            || context.connective_used_ref(&value),
        )?;
        for depth in [0, 1] {
            same_outcome(
                || value.locally_free(depth).is_empty(),
                || context.locally_free_is_empty(&value, depth),
            )?;
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// D-D2 (DR-104): for the thirteen element types of `list_match!` and for
        /// `MatchCase`, at depths 0 and 1, the predicates by reference return the
        /// owned predicates' values and panic exactly when they panic.
        #[test]
        fn predicates_by_reference_equal_owned_predicates(
            pair in (par(), par()),
            single in par(),
            branches in (optional_par(), optional_par(), optional_par()),
            signed in (optional_par(), proptest::option::of(signature())),
            cells in proptest::collection::vec(signature(), 0..3),
            body in optional_par(),
            send in cached(),
            expression in expr(),
            new in (optional_par(), cached()),
            receive in cached(),
            bind in (optional_par(), proptest::collection::vec(par(), 0..3)),
            matched in cached(),
            case in (optional_par(), optional_par()),
        ) {
            let context = SpatialMatcherContext::new();
            check(&context, pair)?;
            check(&context, single)?;
            check(&context, If {
                condition: branches.0,
                if_true: branches.1,
                if_false: branches.2,
                ..Default::default()
            })?;
            check(&context, CostSignedTerm {
                body: signed.0,
                signature: signed.1,
                // Added by G3 (DR-135, S3b): a surface signed term.
                held_price: None,
            })?;
            check(&context, CostStack { cells })?;
            check(&context, Bundle {
                body,
                ..Default::default()
            })?;
            check(&context, Send {
                connective_used: send.0,
                locally_free: send.1,
                ..Default::default()
            })?;
            check(&context, GUnforgeable::default())?;
            check(&context, expression)?;
            check(&context, New {
                p: new.0,
                locally_free: new.1.1,
                ..Default::default()
            })?;
            check(&context, Receive {
                connective_used: receive.0,
                locally_free: receive.1,
                ..Default::default()
            })?;
            check(&context, ReceiveBind {
                source: bind.0,
                patterns: bind.1,
                ..Default::default()
            })?;
            check(&context, Match {
                connective_used: matched.0,
                locally_free: matched.1,
                ..Default::default()
            })?;
            check(&context, MatchCase {
                source: case.0,
                pattern: case.1,
                ..Default::default()
            })?;
        }
    }

    /// The negative-variable control: the owned predicate panics on a negative
    /// bound variable at depth 0, and so does the predicate by reference.
    #[test]
    fn negative_bound_variable_panics_in_both_predicates() {
        let context = SpatialMatcherContext::new();
        let expression = Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(BoundVar(-1)),
                }),
            })),
        };
        let owned = catch_unwind(AssertUnwindSafe(|| expression.locally_free(0).is_empty()));
        let by_reference = catch_unwind(AssertUnwindSafe(|| {
            context.locally_free_is_empty(&expression, 0)
        }));
        assert!(owned.is_err());
        assert!(by_reference.is_err());
        assert!(context.locally_free_is_empty(&expression, 1));
    }
}
