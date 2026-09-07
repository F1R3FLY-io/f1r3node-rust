use models::create_bit_vector;
use models::rust::utils::union;

use super::exports::*;
use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

// See models/src/main/scala/coop/rchain/models/HasLocallyFree.scala
pub trait HasLocallyFree<T> {
    /** Return true if a connective (including free variables and wildcards) is
     *  used anywhere in {@code source}.
     *  @param source the object in question
     *  Specifically looks for constructions that make a pattern non-concrete.
     *  A non-concrete pattern cannot be viewed as if it were a term.
     */
    fn connective_used(&self, source: T) -> bool;

    /** Returns a bitset representing which variables are locally free if the term
     *  is located at depth {@code depth}
     *  @param source the object in question
     *  @param depth pattern nesting depth
     *  This relies on cached values based on the actual depth of a term and will
     *  only return the correct answer if asked about the actual depth of a term.
     *  The reason we pass depth is that building the caches calls this API and for
     *  the few instances where we don't rely on the cache, we need to know the
     *  depth.
     *
     *  Depth is related to pattern nesting. A top level term is depth 0. A pattern
     *  in a top-level term is depth 1. A pattern in a depth 1 term is depth 2,
     *  etc.
     */
    fn locally_free(&self, source: T, depth: i32) -> Vec<u8>;
}

// forTuple
impl HasLocallyFree<(Par, Par)> for SpatialMatcherContext {
    fn connective_used(&self, source: (Par, Par)) -> bool {
        self.connective_used(source.0) || self.connective_used(source.1)
    }

    fn locally_free(&self, source: (Par, Par), depth: i32) -> Vec<u8> {
        union(
            self.locally_free(source.0, depth),
            self.locally_free(source.1, depth),
        )
    }
}

// See models/src/main/scala/coop/rchain/models/rholang/implicits.scala - line 357 and beyond
impl HasLocallyFree<Par> for SpatialMatcherContext {
    fn connective_used(&self, p: Par) -> bool { p.connective_used }

    fn locally_free(&self, p: Par, _depth: i32) -> Vec<u8> { p.locally_free }
}

impl HasLocallyFree<Bundle> for SpatialMatcherContext {
    fn connective_used(&self, _source: Bundle) -> bool { false }

    fn locally_free(&self, source: Bundle, _depth: i32) -> Vec<u8> {
        source.body.unwrap().locally_free
    }
}

impl HasLocallyFree<Send> for SpatialMatcherContext {
    fn connective_used(&self, s: Send) -> bool { s.connective_used }

    fn locally_free(&self, s: Send, _depth: i32) -> Vec<u8> { s.locally_free }
}

impl HasLocallyFree<GUnforgeable> for SpatialMatcherContext {
    fn connective_used(&self, _unf: GUnforgeable) -> bool { false }

    fn locally_free(&self, _s: GUnforgeable, _depth: i32) -> Vec<u8> { Default::default() }
}

impl HasLocallyFree<Expr> for SpatialMatcherContext {
    fn connective_used(&self, e: Expr) -> bool {
        match e.expr_instance {
            Some(GBool(_)) => false,
            Some(GInt(_)) => false,
            Some(GDouble(_)) => false,
            Some(GBigInt(_)) => false,
            Some(GBigRat(_)) => false,
            Some(GFixedPoint(_)) => false,
            Some(GString(_)) => false,
            Some(GUri(_)) => false,
            Some(GByteArray(_)) => false,

            Some(EListBody(e)) => e.connective_used,
            Some(ETupleBody(e)) => e.connective_used,
            Some(ESetBody(e)) => e.connective_used,
            Some(EMapBody(e)) => e.connective_used,
            Some(EPathmapBody(e)) => e.connective_used,
            Some(EZipperBody(e)) => e.connective_used,

            Some(EVarBody(EVar { v })) => self.connective_used(v.unwrap()),
            Some(ENotBody(ENot { p })) => p.unwrap().connective_used,
            Some(ENegBody(ENeg { p })) => p.unwrap().connective_used,

            Some(EMultBody(EMult { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EModBody(EMod { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }

            Some(EMethodBody(e)) => e.connective_used,
            Some(EMatchesBody(EMatches { target, .. })) => target.unwrap().connective_used,

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }

            None => false,
        }
    }

    fn locally_free(&self, e: Expr, depth: i32) -> Vec<u8> {
        match e.expr_instance {
            Some(GBool(_)) => Default::default(),
            Some(GInt(_)) => Default::default(),
            Some(GDouble(_)) => Default::default(),
            Some(GBigInt(_)) => Default::default(),
            Some(GBigRat(_)) => Default::default(),
            Some(GFixedPoint(_)) => Default::default(),
            Some(GString(_)) => Default::default(),
            Some(GUri(_)) => Default::default(),
            Some(GByteArray(_)) => Default::default(),

            Some(EListBody(e)) => e.locally_free,
            Some(ETupleBody(e)) => e.locally_free,
            Some(ESetBody(e)) => e.locally_free,
            Some(EMapBody(e)) => e.locally_free,
            Some(EPathmapBody(e)) => e.locally_free,
            Some(EZipperBody(e)) => e.locally_free,

            Some(EVarBody(EVar { v })) => self.locally_free(v.unwrap(), depth),
            Some(ENotBody(ENot { p })) => p.unwrap().locally_free,
            Some(ENegBody(ENeg { p })) => p.unwrap().locally_free,

            Some(EMultBody(EMult { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EModBody(EMod { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }

            Some(EMethodBody(e)) => e.locally_free,
            Some(EMatchesBody(EMatches { target, .. })) => target.unwrap().locally_free,

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }

            None => Default::default(),
        }
    }
}

impl HasLocallyFree<New> for SpatialMatcherContext {
    fn connective_used(&self, n: New) -> bool { n.p.unwrap().connective_used }

    fn locally_free(&self, n: New, _depth: i32) -> Vec<u8> { n.locally_free }
}

impl HasLocallyFree<VarInstance> for SpatialMatcherContext {
    fn connective_used(&self, v: VarInstance) -> bool {
        match v {
            BoundVar(_) => false,
            FreeVar(_) => true,
            Wildcard(_) => true,
        }
    }

    fn locally_free(&self, v: VarInstance, depth: i32) -> Vec<u8> {
        match v {
            BoundVar(index) => {
                if depth == 0 {
                    create_bit_vector(&[index as usize])
                } else {
                    Default::default()
                }
            }
            FreeVar(_) => Default::default(),
            Wildcard(_) => Default::default(),
        }
    }
}

impl HasLocallyFree<Var> for SpatialMatcherContext {
    fn connective_used(&self, v: Var) -> bool { self.connective_used(v.var_instance.unwrap()) }

    fn locally_free(&self, v: Var, depth: i32) -> Vec<u8> {
        self.locally_free(v.var_instance.unwrap(), depth)
    }
}

impl HasLocallyFree<Receive> for SpatialMatcherContext {
    fn connective_used(&self, r: Receive) -> bool { r.connective_used }

    fn locally_free(&self, r: Receive, _depth: i32) -> Vec<u8> { r.locally_free }
}

impl HasLocallyFree<ReceiveBind> for SpatialMatcherContext {
    fn connective_used(&self, rb: ReceiveBind) -> bool { self.connective_used(rb.source.unwrap()) }

    fn locally_free(&self, rb: ReceiveBind, depth: i32) -> Vec<u8> {
        union(
            self.locally_free(rb.source.unwrap(), depth),
            rb.patterns.iter().fold(Default::default(), |acc, pat| {
                union(acc, self.locally_free(pat.to_owned(), depth + 1))
            }),
        )
    }
}

impl HasLocallyFree<Match> for SpatialMatcherContext {
    fn connective_used(&self, m: Match) -> bool { m.connective_used }

    fn locally_free(&self, m: Match, _depth: i32) -> Vec<u8> { m.locally_free }
}

impl HasLocallyFree<MatchCase> for SpatialMatcherContext {
    fn connective_used(&self, mc: MatchCase) -> bool { mc.source.unwrap().connective_used }

    fn locally_free(&self, mc: MatchCase, depth: i32) -> Vec<u8> {
        union(
            mc.source.unwrap().locally_free,
            self.locally_free(mc.pattern.unwrap(), depth + 1),
        )
    }
}

impl HasLocallyFree<Connective> for SpatialMatcherContext {
    fn connective_used(&self, conn: Connective) -> bool {
        match conn.connective_instance {
            Some(ConnAndBody(_)) => true,
            Some(ConnOrBody(_)) => true,
            Some(ConnNotBody(_)) => true,
            Some(VarRefBody(_)) => false,
            Some(ConnBool(_)) => true,
            Some(ConnInt(_)) => true,
            Some(ConnString(_)) => true,
            Some(ConnUri(_)) => true,
            Some(ConnByteArray(_)) => true,
            None => false,
        }
    }

    fn locally_free(&self, conn: Connective, depth: i32) -> Vec<u8> {
        match conn.connective_instance {
            Some(VarRefBody(VarRef {
                index: idx,
                depth: var_depth,
            })) => {
                if depth == var_depth {
                    create_bit_vector(&[idx as usize])
                } else {
                    Default::default()
                }
            }
            _ => Default::default(),
        }
    }
}

impl HasLocallyFree<VarInstance> for VarInstance {
    fn connective_used(&self, v: VarInstance) -> bool {
        match v {
            BoundVar(_) => false,
            FreeVar(_) => true,
            Wildcard(_) => true,
        }
    }

    fn locally_free(&self, v: VarInstance, depth: i32) -> Vec<u8> {
        match v {
            BoundVar(index) => {
                if depth == 0 {
                    create_bit_vector(&[index as usize])
                } else {
                    Default::default()
                }
            }
            FreeVar(_) => Default::default(),
            Wildcard(_) => Default::default(),
        }
    }
}

impl HasLocallyFree<Var> for Var {
    fn connective_used(&self, v: Var) -> bool {
        v.clone()
            .var_instance
            .unwrap()
            .connective_used(v.var_instance.unwrap())
    }

    fn locally_free(&self, v: Var, depth: i32) -> Vec<u8> {
        v.clone()
            .var_instance
            .unwrap()
            .locally_free(v.var_instance.unwrap(), depth)
    }
}

impl HasLocallyFree<Expr> for Expr {
    fn connective_used(&self, e: Expr) -> bool {
        match e.expr_instance {
            Some(GBool(_)) => false,
            Some(GInt(_)) => false,
            Some(GDouble(_)) => false,
            Some(GBigInt(_)) => false,
            Some(GBigRat(_)) => false,
            Some(GFixedPoint(_)) => false,
            Some(GString(_)) => false,
            Some(GUri(_)) => false,
            Some(GByteArray(_)) => false,

            Some(EListBody(e)) => e.connective_used,
            Some(ETupleBody(e)) => e.connective_used,
            Some(ESetBody(e)) => e.connective_used,
            Some(EMapBody(e)) => e.connective_used,
            Some(EPathmapBody(e)) => e.connective_used,
            Some(EZipperBody(e)) => e.connective_used,

            Some(EVarBody(EVar { v })) => v.clone().unwrap().connective_used(v.unwrap()),
            Some(ENotBody(ENot { p })) => p.unwrap().connective_used,
            Some(ENegBody(ENeg { p })) => p.unwrap().connective_used,

            Some(EMultBody(EMult { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EModBody(EMod { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }

            Some(EMethodBody(e)) => e.connective_used,
            Some(EMatchesBody(EMatches { target, .. })) => target.unwrap().connective_used,

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                p1.unwrap().connective_used | p2.unwrap().connective_used
            }

            None => false,
        }
    }

    fn locally_free(&self, e: Expr, depth: i32) -> Vec<u8> {
        match e.expr_instance {
            Some(GBool(_)) => Default::default(),
            Some(GInt(_)) => Default::default(),
            Some(GDouble(_)) => Default::default(),
            Some(GBigInt(_)) => Default::default(),
            Some(GBigRat(_)) => Default::default(),
            Some(GFixedPoint(_)) => Default::default(),
            Some(GString(_)) => Default::default(),
            Some(GUri(_)) => Default::default(),
            Some(GByteArray(_)) => Default::default(),

            Some(EListBody(e)) => e.locally_free,
            Some(ETupleBody(e)) => e.locally_free,
            Some(ESetBody(e)) => e.locally_free,
            Some(EMapBody(e)) => e.locally_free,
            Some(EPathmapBody(e)) => e.locally_free,
            Some(EZipperBody(e)) => e.locally_free,

            Some(EVarBody(EVar { v })) => v.clone().unwrap().locally_free(v.unwrap(), depth),
            Some(ENotBody(ENot { p })) => p.unwrap().locally_free,
            Some(ENegBody(ENeg { p })) => p.unwrap().locally_free,

            Some(EMultBody(EMult { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EModBody(EMod { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }

            Some(EMethodBody(e)) => e.locally_free,
            Some(EMatchesBody(EMatches { target, .. })) => target.unwrap().locally_free,

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                union(p1.unwrap().locally_free, p2.unwrap().locally_free)
            }

            None => Default::default(),
        }
    }
}

impl HasLocallyFree<Connective> for Connective {
    fn connective_used(&self, conn: Connective) -> bool {
        match conn.connective_instance {
            Some(ConnAndBody(_)) => true,
            Some(ConnOrBody(_)) => true,
            Some(ConnNotBody(_)) => true,
            Some(VarRefBody(_)) => false,
            Some(ConnBool(_)) => true,
            Some(ConnInt(_)) => true,
            Some(ConnString(_)) => true,
            Some(ConnUri(_)) => true,
            Some(ConnByteArray(_)) => true,
            None => false,
        }
    }

    fn locally_free(&self, conn: Connective, depth: i32) -> Vec<u8> {
        match conn.connective_instance {
            Some(VarRefBody(VarRef {
                index: idx,
                depth: var_depth,
            })) => {
                if depth == var_depth {
                    create_bit_vector(&[idx as usize])
                } else {
                    Default::default()
                }
            }
            _ => Default::default(),
        }
    }
}

impl HasLocallyFree<Par> for Par {
    fn connective_used(&self, p: Par) -> bool { p.connective_used }

    fn locally_free(&self, p: Par, _depth: i32) -> Vec<u8> { p.locally_free }
}

impl HasLocallyFree<New> for New {
    fn connective_used(&self, n: New) -> bool { n.p.unwrap().connective_used }

    fn locally_free(&self, n: New, _depth: i32) -> Vec<u8> { n.locally_free }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::EMethod;
    use models::rust::utils::{new_freevar_par, new_gint_par};

    use super::*;

    fn ctx() -> SpatialMatcherContext { SpatialMatcherContext::new() }

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
        let context = ctx();
        for op in binary_ops() {
            let dirty = expr_of(op(free_par(), plain_par()));
            assert!(
                HasLocallyFree::<Expr>::connective_used(&context, dirty.clone()),
                "context impl"
            );
            assert!(
                dirty.clone().connective_used(dirty.clone()),
                "inherent Expr impl"
            );

            let clean = expr_of(op(plain_par(), plain_par()));
            assert!(!HasLocallyFree::<Expr>::connective_used(
                &context,
                clean.clone()
            ));
            assert!(!clean.clone().connective_used(clean));

            let unioned = expr_of(op(marked_par(0), marked_par(1)));
            let expected = union(create_bit_vector(&[0]), create_bit_vector(&[1]));
            assert_eq!(
                HasLocallyFree::<Expr>::locally_free(&context, unioned.clone(), 0),
                expected
            );
            assert_eq!(unioned.clone().locally_free(unioned, 0), expected);
        }
    }

    #[test]
    fn ground_exprs_are_concrete_with_no_free_variables() {
        let context = ctx();
        let grounds = vec![
            GBool(true),
            GInt(1),
            GString("s".to_string()),
            GUri("u".to_string()),
            GByteArray(vec![1]),
            GDouble(1.5f64.to_bits()),
        ];
        for ground in grounds {
            let e = expr_of(ground);
            assert!(!HasLocallyFree::<Expr>::connective_used(
                &context,
                e.clone()
            ));
            assert!(HasLocallyFree::<Expr>::locally_free(&context, e.clone(), 0).is_empty());
            assert!(!e.clone().connective_used(e.clone()));
            assert!(e.clone().locally_free(e, 0).is_empty());
        }
    }

    #[test]
    fn unary_method_and_matches_exprs_delegate_to_their_bodies() {
        let context = ctx();

        let not_expr = expr_of(ENotBody(ENot {
            p: Some(free_par()),
        }));
        assert!(HasLocallyFree::<Expr>::connective_used(
            &context,
            not_expr.clone()
        ));
        let neg_expr = expr_of(ENegBody(ENeg {
            p: Some(marked_par(2)),
        }));
        assert!(!HasLocallyFree::<Expr>::connective_used(
            &context,
            neg_expr.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, neg_expr.clone(), 0),
            create_bit_vector(&[2])
        );
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, not_expr, 0),
            Vec::<u8>::new()
        );
        assert_eq!(
            neg_expr.clone().locally_free(neg_expr, 0),
            create_bit_vector(&[2])
        );

        let matches_expr = expr_of(EMatchesBody(EMatches {
            target: Some(marked_par(1)),
            pattern: Some(free_par()),
        }));
        assert!(!HasLocallyFree::<Expr>::connective_used(
            &context,
            matches_expr.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, matches_expr.clone(), 0),
            create_bit_vector(&[1])
        );
        assert!(!matches_expr.clone().connective_used(matches_expr));

        let method_expr = expr_of(EMethodBody(EMethod {
            method_name: "nth".to_string(),
            target: Some(plain_par()),
            arguments: vec![],
            locally_free: create_bit_vector(&[3]),
            connective_used: true,
        }));
        assert!(HasLocallyFree::<Expr>::connective_used(
            &context,
            method_expr.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, method_expr.clone(), 0),
            create_bit_vector(&[3])
        );
        assert!(method_expr.clone().connective_used(method_expr));
    }

    #[test]
    fn collection_exprs_read_their_cached_fields() {
        let context = ctx();
        let list = expr_of(EListBody(EList {
            ps: vec![],
            locally_free: create_bit_vector(&[1]),
            connective_used: true,
            remainder: None,
        }));
        assert!(HasLocallyFree::<Expr>::connective_used(
            &context,
            list.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, list.clone(), 0),
            create_bit_vector(&[1])
        );
        assert!(list.clone().connective_used(list.clone()));
        assert_eq!(list.clone().locally_free(list, 0), create_bit_vector(&[1]));

        let tuple = expr_of(ETupleBody(ETuple {
            ps: vec![],
            locally_free: create_bit_vector(&[2]),
            connective_used: false,
        }));
        assert!(!HasLocallyFree::<Expr>::connective_used(
            &context,
            tuple.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, tuple.clone(), 0),
            create_bit_vector(&[2])
        );
        assert_eq!(
            tuple.clone().locally_free(tuple, 0),
            create_bit_vector(&[2])
        );
    }

    #[test]
    fn var_instances_classify_and_report_bound_indices() {
        let context = ctx();

        assert!(!HasLocallyFree::<VarInstance>::connective_used(
            &context,
            BoundVar(1)
        ));
        assert!(HasLocallyFree::<VarInstance>::connective_used(
            &context,
            FreeVar(1)
        ));
        assert!(HasLocallyFree::<VarInstance>::connective_used(
            &context,
            Wildcard(Default::default())
        ));

        assert_eq!(
            HasLocallyFree::<VarInstance>::locally_free(&context, BoundVar(2), 0),
            create_bit_vector(&[2])
        );
        assert!(HasLocallyFree::<VarInstance>::locally_free(&context, BoundVar(2), 1).is_empty());
        assert!(HasLocallyFree::<VarInstance>::locally_free(&context, FreeVar(2), 0).is_empty());
        assert!(HasLocallyFree::<VarInstance>::locally_free(
            &context,
            Wildcard(Default::default()),
            0
        )
        .is_empty());

        let bound_var = Var {
            var_instance: Some(BoundVar(3)),
        };
        assert!(!HasLocallyFree::<Var>::connective_used(
            &context,
            bound_var.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Var>::locally_free(&context, bound_var.clone(), 0),
            create_bit_vector(&[3])
        );
        assert!(!bound_var.clone().connective_used(bound_var.clone()));
        assert_eq!(
            bound_var.clone().locally_free(bound_var, 0),
            create_bit_vector(&[3])
        );

        assert!(BoundVar(1).locally_free(BoundVar(1), 1).is_empty());
        assert!(FreeVar(1).connective_used(FreeVar(1)));

        let evar = expr_of(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(BoundVar(1)),
            }),
        }));
        assert!(!HasLocallyFree::<Expr>::connective_used(
            &context,
            evar.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Expr>::locally_free(&context, evar.clone(), 0),
            create_bit_vector(&[1])
        );
        assert_eq!(evar.clone().locally_free(evar, 0), create_bit_vector(&[1]));
    }

    #[test]
    fn structural_terms_read_their_cached_fields() {
        let context = ctx();

        let par = Par {
            connective_used: true,
            locally_free: create_bit_vector(&[1]),
            ..Default::default()
        };
        assert!(HasLocallyFree::<Par>::connective_used(
            &context,
            par.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Par>::locally_free(&context, par.clone(), 0),
            create_bit_vector(&[1])
        );
        assert!(par.clone().connective_used(par.clone()));
        assert_eq!(
            par.clone().locally_free(par.clone(), 0),
            create_bit_vector(&[1])
        );

        assert!(HasLocallyFree::<(Par, Par)>::connective_used(
            &context,
            (par.clone(), plain_par())
        ));
        assert_eq!(
            HasLocallyFree::<(Par, Par)>::locally_free(&context, (par, marked_par(2)), 0),
            union(create_bit_vector(&[1]), create_bit_vector(&[2]))
        );

        let bundle = Bundle {
            body: Some(marked_par(2)),
            write_flag: false,
            read_flag: false,
        };
        assert!(!HasLocallyFree::<Bundle>::connective_used(
            &context,
            bundle.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Bundle>::locally_free(&context, bundle, 0),
            create_bit_vector(&[2])
        );

        let send = Send {
            chan: None,
            data: vec![],
            persistent: false,
            locally_free: create_bit_vector(&[3]),
            connective_used: true,
        };
        assert!(HasLocallyFree::<Send>::connective_used(
            &context,
            send.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Send>::locally_free(&context, send, 0),
            create_bit_vector(&[3])
        );

        let unforgeable = GUnforgeable { unf_instance: None };
        assert!(!HasLocallyFree::<GUnforgeable>::connective_used(
            &context,
            unforgeable.clone()
        ));
        assert!(HasLocallyFree::<GUnforgeable>::locally_free(&context, unforgeable, 0).is_empty());

        let new_term = New {
            bind_count: 1,
            p: Some(free_par()),
            locally_free: create_bit_vector(&[4]),
            ..Default::default()
        };
        assert!(HasLocallyFree::<New>::connective_used(
            &context,
            new_term.clone()
        ));
        assert_eq!(
            HasLocallyFree::<New>::locally_free(&context, new_term.clone(), 0),
            create_bit_vector(&[4])
        );
        assert!(new_term.clone().connective_used(new_term.clone()));
        assert_eq!(
            new_term.clone().locally_free(new_term, 0),
            create_bit_vector(&[4])
        );

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
        assert!(HasLocallyFree::<Receive>::connective_used(
            &context,
            receive.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Receive>::locally_free(&context, receive, 0),
            create_bit_vector(&[5])
        );

        let match_term = Match {
            target: Some(plain_par()),
            cases: vec![],
            locally_free: create_bit_vector(&[6]),
            connective_used: true,
        };
        assert!(HasLocallyFree::<Match>::connective_used(
            &context,
            match_term.clone()
        ));
        assert_eq!(
            HasLocallyFree::<Match>::locally_free(&context, match_term, 0),
            create_bit_vector(&[6])
        );
    }

    #[test]
    fn receive_binds_and_match_cases_union_sources_and_patterns() {
        let context = ctx();

        let bind = ReceiveBind {
            patterns: vec![marked_par(1)],
            source: Some(marked_par(2)),
            remainder: None,
            free_count: 0,
        };
        assert!(!HasLocallyFree::<ReceiveBind>::connective_used(
            &context,
            bind.clone()
        ));
        assert_eq!(
            HasLocallyFree::<ReceiveBind>::locally_free(&context, bind, 0),
            union(create_bit_vector(&[1]), create_bit_vector(&[2]))
        );

        let dirty_bind = ReceiveBind {
            patterns: vec![],
            source: Some(free_par()),
            remainder: None,
            free_count: 1,
        };
        assert!(HasLocallyFree::<ReceiveBind>::connective_used(
            &context, dirty_bind
        ));

        let case = MatchCase {
            pattern: Some(marked_par(1)),
            source: Some(marked_par(2)),
            free_count: 0,
            guard: None,
        };
        assert!(!HasLocallyFree::<MatchCase>::connective_used(
            &context,
            case.clone()
        ));
        assert_eq!(
            HasLocallyFree::<MatchCase>::locally_free(&context, case, 0),
            union(create_bit_vector(&[2]), create_bit_vector(&[1]))
        );
    }

    #[test]
    fn connectives_classify_and_var_refs_report_depth_matched_indices() {
        let context = ctx();
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
            assert!(HasLocallyFree::<Connective>::connective_used(
                &context,
                used.clone()
            ));
            assert!(used.clone().connective_used(used.clone()));
            assert!(HasLocallyFree::<Connective>::locally_free(&context, used, 1).is_empty());
        }

        let var_ref = conn(VarRefBody(VarRef { index: 2, depth: 1 }));
        assert!(!HasLocallyFree::<Connective>::connective_used(
            &context,
            var_ref.clone()
        ));
        assert!(!var_ref.clone().connective_used(var_ref.clone()));
        assert_eq!(
            HasLocallyFree::<Connective>::locally_free(&context, var_ref.clone(), 1),
            create_bit_vector(&[2])
        );
        assert!(
            HasLocallyFree::<Connective>::locally_free(&context, var_ref.clone(), 0).is_empty()
        );
        assert_eq!(
            var_ref.clone().locally_free(var_ref.clone(), 1),
            create_bit_vector(&[2])
        );
        assert!(var_ref.clone().locally_free(var_ref, 0).is_empty());
    }
}
