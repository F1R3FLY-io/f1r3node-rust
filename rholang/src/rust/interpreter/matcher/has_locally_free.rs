use models::create_bit_vector;
use models::rust::utils::union;

use super::exports::*;

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
