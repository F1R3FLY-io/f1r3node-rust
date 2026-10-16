use models::create_bit_vector;
use models::rust::utils::union;

use super::exports::*;
use crate::rust::interpreter::matcher::spatial_matcher::SpatialMatcherContext;

/// Moves the cached bitset out while leaving `Par` valid for its iterative
/// destructor. This is allocation-neutral (`Vec::new()` replaces the field).
#[inline]
fn take_par_locally_free(mut par: Par) -> Vec<u8> { std::mem::take(&mut par.locally_free) }

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

/// `target matches pattern` — the node's `locally_free`, over **both** slots.
///
/// # The invariant, and where it is specified
///
/// [`HasLocallyFree::locally_free`]'s contract, three doc-comments above, is
/// *"a bitset representing which variables are locally free if the term is
/// located at depth `depth`"*, where *"a top level term is depth 0, a pattern in
/// a top-level term is depth 1"*. It is a question about **scope**: which de
/// Bruijn indices of the enclosing *process* scope does this subtree name?
///
/// Every contribution lives in that one index space, at every depth, which is
/// why the two slots can be `union`ed raw with no shift. A `BoundVar(i)`
/// contributes only at `depth == 0` (`HasLocallyFree<VarInstance>`), and a
/// `VarRef { index, depth: d }` contributes only when the traversal depth
/// equals `d` (`HasLocallyFree<Connective>`) — i.e. exactly when the chain
/// distance `d` carries it back to absolute depth 0. So a bit means "index `i`
/// of the process scope", whoever set it and however deep they were.
///
/// # Why the `pattern` slot contributes — the `VarRef` case
///
/// `p_matches_normalizer::combine_p_matches` normalizes the pattern under
/// `bound_map_chain.push()` in a fresh `FreeMap`, so it is a nested pattern one
/// binding level deeper — the same status `ReceiveBind::patterns` and
/// `MatchCase::pattern` have. Two lookups decide what a name inside it means:
///
/// * a plain `x` goes through `BoundMapChain::get`, which reads the **current**
///   scope only, so it is a fresh binding occurrence and can never point
///   outside. It contributes nothing, and needs no reporting.
/// * `=x` goes through `BoundMapChain::find`, which walks the **whole chain**,
///   and is emitted as `VarRef { depth }` carrying the chain distance. It names
///   an outer binder from inside the pattern, and the normalizer already sets
///   its bit in `pattern.locally_free`.
///
/// The second case is the one that must be reported, and it is the case the two
/// sibling pattern positions do report: `p_match_normalizer` unions
/// `pattern_result.par.locally_free` into `Match.locally_free`, and
/// `p_input_normalizer` folds `done_patterns`' bitsets into
/// `Receive.locally_free`. Reading the target alone made `EMatches` the sole
/// dissenter among the language's three pattern positions, and the bit it
/// dropped never reappeared: for
/// `for (@x <- @"c") { for (@y <- @"d") { @"o"!(10 matches =x) } }` the pattern
/// carried `[0, 1]` while every ancestor read `[]`, where the `match`-cased twin
/// propagated `[0, 1]` up and let the *outer* `for` clear it.
///
/// ⚠ It also broke the agreement with substitution. `locally_free` sets a
/// `VarRef`'s bit when `depth == var_depth`; `maybe_substitute_var_ref_view`
/// replaces it when `term.depth == ctx.depth`. One predicate. Since `8853f839`
/// substitution visits `EMatches::pattern` at `depth + 1` — the depth at which
/// that `VarRef` *does* contribute — so the substituter was acting on an index
/// this reader refused to name.
///
/// # ⚠ Why [`HasLocallyFree::connective_used`] reads the TARGET ALONE
///
/// Its contract is a different question: *"constructions that make a pattern
/// non-concrete. A non-concrete pattern cannot be viewed as if it were a
/// term."* A `matches` pattern is **allowed** to be non-concrete — that is the
/// entire purpose of the construct — while `target matches pattern` is itself a
/// perfectly concrete term that evaluates to a `GBool`. So `1 matches ~1` has
/// `connective_used == false`, pinned by
/// `p_matches_normalizer`'s `p_matches_should_normalize_one_matches_tilda_with_
/// connective_used_false`. That asymmetry is correct and is deliberately left
/// standing; only the `locally_free` half moved.
pub fn ematches_locally_free(target: &Par, pattern: &Par) -> Vec<u8> {
    union(target.locally_free.clone(), pattern.locally_free.clone())
}

// forTuple
impl<'a> HasLocallyFree<(Par, Par)> for SpatialMatcherContext<'a> {
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
impl<'a> HasLocallyFree<Par> for SpatialMatcherContext<'a> {
    fn connective_used(&self, p: Par) -> bool { p.connective_used }

    fn locally_free(&self, p: Par, _depth: i32) -> Vec<u8> { take_par_locally_free(p) }
}

fn signature_locally_free(signature: &CostSignature) -> Vec<u8> {
    use models::rhoapi::cost_signature::Value;

    let mut pending = vec![signature];
    let mut free = Vec::new();
    while let Some(current) = pending.pop() {
        match &current.value {
            Some(Value::BoundLevel(level)) if *level >= 0 => {
                free = union(free, create_bit_vector(&[*level as usize]));
            }
            Some(Value::Quote(par)) | Some(Value::Name(par)) => {
                free = union(free, par.locally_free.clone());
            }
            Some(Value::Compound(compound)) => pending.extend(compound.elements.iter()),
            _ => {}
        }
    }
    free
}

fn signature_connective_used(signature: &CostSignature) -> bool {
    use models::rhoapi::cost_signature::Value;

    let mut pending = vec![signature];
    while let Some(current) = pending.pop() {
        match &current.value {
            Some(Value::Quote(par)) | Some(Value::Name(par)) if par.connective_used => return true,
            Some(Value::Compound(compound)) => pending.extend(compound.elements.iter()),
            _ => {}
        }
    }
    false
}

impl<'a> HasLocallyFree<If> for SpatialMatcherContext<'a> {
    fn connective_used(&self, source: If) -> bool {
        source.condition.as_ref().is_some_and(|p| p.connective_used)
            || source.if_true.as_ref().is_some_and(|p| p.connective_used)
            || source.if_false.as_ref().is_some_and(|p| p.connective_used)
    }

    fn locally_free(&self, source: If, _depth: i32) -> Vec<u8> {
        [source.condition, source.if_true, source.if_false]
            .into_iter()
            .flatten()
            .map(|mut p| std::mem::take(&mut p.locally_free))
            .fold(Vec::new(), union)
    }
}

impl<'a> HasLocallyFree<CostSignedTerm> for SpatialMatcherContext<'a> {
    fn connective_used(&self, source: CostSignedTerm) -> bool {
        source.body.as_ref().is_some_and(|p| p.connective_used)
            || source
                .signature
                .as_ref()
                .is_some_and(signature_connective_used)
    }

    fn locally_free(&self, source: CostSignedTerm, _depth: i32) -> Vec<u8> {
        union(
            source
                .body
                .map(|mut p| std::mem::take(&mut p.locally_free))
                .unwrap_or_default(),
            source
                .signature
                .as_ref()
                .map(signature_locally_free)
                .unwrap_or_default(),
        )
    }
}

impl<'a> HasLocallyFree<CostStack> for SpatialMatcherContext<'a> {
    fn connective_used(&self, source: CostStack) -> bool {
        let mut pending = vec![&source];
        while let Some(stack) = pending.pop() {
            if stack.cells.iter().any(signature_connective_used) {
                return true;
            }
            pending.extend(stack.parallel_tail.iter());
        }
        false
    }

    fn locally_free(&self, source: CostStack, _depth: i32) -> Vec<u8> {
        let mut pending = vec![&source];
        let mut free = Vec::new();
        while let Some(stack) = pending.pop() {
            for signature in &stack.cells {
                free = union(free, signature_locally_free(signature));
            }
            pending.extend(stack.parallel_tail.iter());
        }
        free
    }
}

impl<'a> HasLocallyFree<Bundle> for SpatialMatcherContext<'a> {
    fn connective_used(&self, _source: Bundle) -> bool { false }

    fn locally_free(&self, source: Bundle, _depth: i32) -> Vec<u8> {
        take_par_locally_free(source.body.expect("Bundle.body"))
    }
}

impl<'a> HasLocallyFree<Send> for SpatialMatcherContext<'a> {
    fn connective_used(&self, s: Send) -> bool { s.connective_used }

    fn locally_free(&self, s: Send, _depth: i32) -> Vec<u8> { s.locally_free }
}

impl<'a> HasLocallyFree<GUnforgeable> for SpatialMatcherContext<'a> {
    fn connective_used(&self, _unf: GUnforgeable) -> bool { false }

    fn locally_free(&self, _s: GUnforgeable, _depth: i32) -> Vec<u8> { Default::default() }
}

impl<'a> HasLocallyFree<Expr> for SpatialMatcherContext<'a> {
    fn connective_used(&self, e: Expr) -> bool {
        match e.expr_instance {
            Some(GBool(_)) => false,
            Some(GInt(_)) => false,
            Some(GUint64(_)) => false,
            Some(GInt32(_)) => false,
            Some(GUint32(_)) => false,
            Some(GUint16(_)) => false,
            Some(GUint8(_)) => false,
            Some(GDouble(_)) => false,
            Some(GFloat32(_)) => false,
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

            Some(EVarBody(EVar { v })) => self.connective_used(v.expect("EVar.v")),
            Some(ENotBody(ENot { p })) => p.expect("ENot.p").connective_used,
            Some(ENegBody(ENeg { p })) => p.expect("ENeg.p").connective_used,

            Some(EMultBody(EMult { p1, p2 })) => {
                p1.expect("EMult.p1").connective_used | p2.expect("EMult.p2").connective_used
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                p1.expect("EDiv.p1").connective_used | p2.expect("EDiv.p2").connective_used
            }
            Some(EModBody(EMod { p1, p2 })) => {
                p1.expect("EMod.p1").connective_used | p2.expect("EMod.p2").connective_used
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                p1.expect("EPlus.p1").connective_used | p2.expect("EPlus.p2").connective_used
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                p1.expect("EMinus.p1").connective_used | p2.expect("EMinus.p2").connective_used
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                p1.expect("ELt.p1").connective_used | p2.expect("ELt.p2").connective_used
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                p1.expect("ELte.p1").connective_used | p2.expect("ELte.p2").connective_used
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                p1.expect("EGt.p1").connective_used | p2.expect("EGt.p2").connective_used
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                p1.expect("EGte.p1").connective_used | p2.expect("EGte.p2").connective_used
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                p1.expect("EEq.p1").connective_used | p2.expect("EEq.p2").connective_used
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                p1.expect("ENeq.p1").connective_used | p2.expect("ENeq.p2").connective_used
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                p1.expect("EAnd.p1").connective_used | p2.expect("EAnd.p2").connective_used
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                p1.expect("EOr.p1").connective_used | p2.expect("EOr.p2").connective_used
            }

            Some(EMethodBody(e)) => e.connective_used,
            Some(EMatchesBody(EMatches { target, .. })) => {
                target.expect("EMatches.target").connective_used
            }

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                p1.expect("EPercentPercent.p1").connective_used
                    | p2.expect("EPercentPercent.p2").connective_used
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                p1.expect("EPlusPlus.p1").connective_used
                    | p2.expect("EPlusPlus.p2").connective_used
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                p1.expect("EMinusMinus.p1").connective_used
                    | p2.expect("EMinusMinus.p2").connective_used
            }

            None => false,
        }
    }

    fn locally_free(&self, e: Expr, depth: i32) -> Vec<u8> {
        match e.expr_instance {
            Some(GBool(_)) => Default::default(),
            Some(GInt(_)) => Default::default(),
            Some(GUint64(_)) => Default::default(),
            Some(GInt32(_)) => Default::default(),
            Some(GUint32(_)) => Default::default(),
            Some(GUint16(_)) => Default::default(),
            Some(GUint8(_)) => Default::default(),
            Some(GDouble(_)) => Default::default(),
            Some(GFloat32(_)) => Default::default(),
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

            Some(EVarBody(EVar { v })) => self.locally_free(v.expect("EVar.v"), depth),
            Some(ENotBody(ENot { p })) => take_par_locally_free(p.expect("ENot.p")),
            Some(ENegBody(ENeg { p })) => take_par_locally_free(p.expect("ENeg.p")),

            Some(EMultBody(EMult { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMult.p1")),
                take_par_locally_free(p2.expect("EMult.p2")),
            ),
            Some(EDivBody(EDiv { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EDiv.p1")),
                take_par_locally_free(p2.expect("EDiv.p2")),
            ),
            Some(EModBody(EMod { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMod.p1")),
                take_par_locally_free(p2.expect("EMod.p2")),
            ),
            Some(EPlusBody(EPlus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPlus.p1")),
                take_par_locally_free(p2.expect("EPlus.p2")),
            ),
            Some(EMinusBody(EMinus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMinus.p1")),
                take_par_locally_free(p2.expect("EMinus.p2")),
            ),
            Some(ELtBody(ELt { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ELt.p1")),
                take_par_locally_free(p2.expect("ELt.p2")),
            ),
            Some(ELteBody(ELte { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ELte.p1")),
                take_par_locally_free(p2.expect("ELte.p2")),
            ),
            Some(EGtBody(EGt { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EGt.p1")),
                take_par_locally_free(p2.expect("EGt.p2")),
            ),
            Some(EGteBody(EGte { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EGte.p1")),
                take_par_locally_free(p2.expect("EGte.p2")),
            ),
            Some(EEqBody(EEq { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EEq.p1")),
                take_par_locally_free(p2.expect("EEq.p2")),
            ),
            Some(ENeqBody(ENeq { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ENeq.p1")),
                take_par_locally_free(p2.expect("ENeq.p2")),
            ),
            Some(EAndBody(EAnd { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EAnd.p1")),
                take_par_locally_free(p2.expect("EAnd.p2")),
            ),
            Some(EOrBody(EOr { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EOr.p1")),
                take_par_locally_free(p2.expect("EOr.p2")),
            ),

            Some(EMethodBody(e)) => e.locally_free,
            // ★ BOTH SLOTS — see `ematches_locally_free` for why, and for why
            // `connective_used` reads the target ALONE and must keep doing so.
            // Written out rather than delegated: these two impls take the term
            // BY VALUE and can MOVE both bitsets, where the helper (a
            // by-reference reader) must clone.
            Some(EMatchesBody(EMatches { target, pattern })) => union(
                take_par_locally_free(target.expect("EMatches.target")),
                take_par_locally_free(pattern.expect("EMatches.pattern")),
            ),

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPercentPercent.p1")),
                take_par_locally_free(p2.expect("EPercentPercent.p2")),
            ),
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPlusPlus.p1")),
                take_par_locally_free(p2.expect("EPlusPlus.p2")),
            ),
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMinusMinus.p1")),
                take_par_locally_free(p2.expect("EMinusMinus.p2")),
            ),

            None => Default::default(),
        }
    }
}

impl<'a> HasLocallyFree<New> for SpatialMatcherContext<'a> {
    fn connective_used(&self, n: New) -> bool { n.p.expect("New.p").connective_used }

    fn locally_free(&self, n: New, _depth: i32) -> Vec<u8> { n.locally_free }
}

impl<'a> HasLocallyFree<VarInstance> for SpatialMatcherContext<'a> {
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

impl<'a> HasLocallyFree<Var> for SpatialMatcherContext<'a> {
    fn connective_used(&self, v: Var) -> bool {
        self.connective_used(v.var_instance.expect("Var.var_instance"))
    }

    fn locally_free(&self, v: Var, depth: i32) -> Vec<u8> {
        self.locally_free(v.var_instance.expect("Var.var_instance"), depth)
    }
}

impl<'a> HasLocallyFree<Receive> for SpatialMatcherContext<'a> {
    fn connective_used(&self, r: Receive) -> bool { r.connective_used }

    fn locally_free(&self, r: Receive, _depth: i32) -> Vec<u8> { r.locally_free }
}

impl<'a> HasLocallyFree<ReceiveBind> for SpatialMatcherContext<'a> {
    fn connective_used(&self, rb: ReceiveBind) -> bool {
        self.connective_used(rb.source.expect("ReceiveBind.source"))
    }

    fn locally_free(&self, rb: ReceiveBind, depth: i32) -> Vec<u8> {
        union(
            self.locally_free(rb.source.expect("ReceiveBind.source"), depth),
            rb.patterns.iter().fold(Default::default(), |acc, pat| {
                union(acc, self.locally_free(pat.to_owned(), depth + 1))
            }),
        )
    }
}

impl<'a> HasLocallyFree<Match> for SpatialMatcherContext<'a> {
    fn connective_used(&self, m: Match) -> bool { m.connective_used }

    fn locally_free(&self, m: Match, _depth: i32) -> Vec<u8> { m.locally_free }
}

impl<'a> HasLocallyFree<MatchCase> for SpatialMatcherContext<'a> {
    fn connective_used(&self, mc: MatchCase) -> bool {
        mc.source.expect("MatchCase.source").connective_used
    }

    fn locally_free(&self, mc: MatchCase, depth: i32) -> Vec<u8> {
        union(
            take_par_locally_free(mc.source.expect("MatchCase.source")),
            self.locally_free(mc.pattern.expect("MatchCase.pattern"), depth + 1),
        )
    }
}

impl<'a> HasLocallyFree<Connective> for SpatialMatcherContext<'a> {
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
            .expect("Var.var_instance")
            .connective_used(v.var_instance.expect("Var.var_instance"))
    }

    fn locally_free(&self, v: Var, depth: i32) -> Vec<u8> {
        v.clone()
            .var_instance
            .expect("Var.var_instance")
            .locally_free(v.var_instance.expect("Var.var_instance"), depth)
    }
}

impl HasLocallyFree<Expr> for Expr {
    fn connective_used(&self, e: Expr) -> bool {
        match e.expr_instance {
            Some(GBool(_)) => false,
            Some(GInt(_)) => false,
            Some(GUint64(_)) => false,
            Some(GInt32(_)) => false,
            Some(GUint32(_)) => false,
            Some(GUint16(_)) => false,
            Some(GUint8(_)) => false,
            Some(GDouble(_)) => false,
            Some(GFloat32(_)) => false,
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

            Some(EVarBody(EVar { v })) => v
                .clone()
                .expect("EVar.v")
                .connective_used(v.expect("EVar.v")),
            Some(ENotBody(ENot { p })) => p.expect("ENot.p").connective_used,
            Some(ENegBody(ENeg { p })) => p.expect("ENeg.p").connective_used,

            Some(EMultBody(EMult { p1, p2 })) => {
                p1.expect("EMult.p1").connective_used | p2.expect("EMult.p2").connective_used
            }
            Some(EDivBody(EDiv { p1, p2 })) => {
                p1.expect("EDiv.p1").connective_used | p2.expect("EDiv.p2").connective_used
            }
            Some(EModBody(EMod { p1, p2 })) => {
                p1.expect("EMod.p1").connective_used | p2.expect("EMod.p2").connective_used
            }
            Some(EPlusBody(EPlus { p1, p2 })) => {
                p1.expect("EPlus.p1").connective_used | p2.expect("EPlus.p2").connective_used
            }
            Some(EMinusBody(EMinus { p1, p2 })) => {
                p1.expect("EMinus.p1").connective_used | p2.expect("EMinus.p2").connective_used
            }
            Some(ELtBody(ELt { p1, p2 })) => {
                p1.expect("ELt.p1").connective_used | p2.expect("ELt.p2").connective_used
            }
            Some(ELteBody(ELte { p1, p2 })) => {
                p1.expect("ELte.p1").connective_used | p2.expect("ELte.p2").connective_used
            }
            Some(EGtBody(EGt { p1, p2 })) => {
                p1.expect("EGt.p1").connective_used | p2.expect("EGt.p2").connective_used
            }
            Some(EGteBody(EGte { p1, p2 })) => {
                p1.expect("EGte.p1").connective_used | p2.expect("EGte.p2").connective_used
            }
            Some(EEqBody(EEq { p1, p2 })) => {
                p1.expect("EEq.p1").connective_used | p2.expect("EEq.p2").connective_used
            }
            Some(ENeqBody(ENeq { p1, p2 })) => {
                p1.expect("ENeq.p1").connective_used | p2.expect("ENeq.p2").connective_used
            }
            Some(EAndBody(EAnd { p1, p2 })) => {
                p1.expect("EAnd.p1").connective_used | p2.expect("EAnd.p2").connective_used
            }
            Some(EOrBody(EOr { p1, p2 })) => {
                p1.expect("EOr.p1").connective_used | p2.expect("EOr.p2").connective_used
            }

            Some(EMethodBody(e)) => e.connective_used,
            Some(EMatchesBody(EMatches { target, .. })) => {
                target.expect("EMatches.target").connective_used
            }

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => {
                p1.expect("EPercentPercent.p1").connective_used
                    | p2.expect("EPercentPercent.p2").connective_used
            }
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => {
                p1.expect("EPlusPlus.p1").connective_used
                    | p2.expect("EPlusPlus.p2").connective_used
            }
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => {
                p1.expect("EMinusMinus.p1").connective_used
                    | p2.expect("EMinusMinus.p2").connective_used
            }

            None => false,
        }
    }

    fn locally_free(&self, e: Expr, depth: i32) -> Vec<u8> {
        match e.expr_instance {
            Some(GBool(_)) => Default::default(),
            Some(GInt(_)) => Default::default(),
            Some(GUint64(_)) => Default::default(),
            Some(GInt32(_)) => Default::default(),
            Some(GUint32(_)) => Default::default(),
            Some(GUint16(_)) => Default::default(),
            Some(GUint8(_)) => Default::default(),
            Some(GDouble(_)) => Default::default(),
            Some(GFloat32(_)) => Default::default(),
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

            Some(EVarBody(EVar { v })) => v
                .clone()
                .expect("EVar.v")
                .locally_free(v.expect("EVar.v"), depth),
            Some(ENotBody(ENot { p })) => take_par_locally_free(p.expect("ENot.p")),
            Some(ENegBody(ENeg { p })) => take_par_locally_free(p.expect("ENeg.p")),

            Some(EMultBody(EMult { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMult.p1")),
                take_par_locally_free(p2.expect("EMult.p2")),
            ),
            Some(EDivBody(EDiv { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EDiv.p1")),
                take_par_locally_free(p2.expect("EDiv.p2")),
            ),
            Some(EModBody(EMod { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMod.p1")),
                take_par_locally_free(p2.expect("EMod.p2")),
            ),
            Some(EPlusBody(EPlus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPlus.p1")),
                take_par_locally_free(p2.expect("EPlus.p2")),
            ),
            Some(EMinusBody(EMinus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMinus.p1")),
                take_par_locally_free(p2.expect("EMinus.p2")),
            ),
            Some(ELtBody(ELt { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ELt.p1")),
                take_par_locally_free(p2.expect("ELt.p2")),
            ),
            Some(ELteBody(ELte { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ELte.p1")),
                take_par_locally_free(p2.expect("ELte.p2")),
            ),
            Some(EGtBody(EGt { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EGt.p1")),
                take_par_locally_free(p2.expect("EGt.p2")),
            ),
            Some(EGteBody(EGte { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EGte.p1")),
                take_par_locally_free(p2.expect("EGte.p2")),
            ),
            Some(EEqBody(EEq { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EEq.p1")),
                take_par_locally_free(p2.expect("EEq.p2")),
            ),
            Some(ENeqBody(ENeq { p1, p2 })) => union(
                take_par_locally_free(p1.expect("ENeq.p1")),
                take_par_locally_free(p2.expect("ENeq.p2")),
            ),
            Some(EAndBody(EAnd { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EAnd.p1")),
                take_par_locally_free(p2.expect("EAnd.p2")),
            ),
            Some(EOrBody(EOr { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EOr.p1")),
                take_par_locally_free(p2.expect("EOr.p2")),
            ),

            Some(EMethodBody(e)) => e.locally_free,
            // ★ BOTH SLOTS — see `ematches_locally_free` for why, and for why
            // `connective_used` reads the target ALONE and must keep doing so.
            // Written out rather than delegated: these two impls take the term
            // BY VALUE and can MOVE both bitsets, where the helper (a
            // by-reference reader) must clone.
            Some(EMatchesBody(EMatches { target, pattern })) => union(
                take_par_locally_free(target.expect("EMatches.target")),
                take_par_locally_free(pattern.expect("EMatches.pattern")),
            ),

            Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPercentPercent.p1")),
                take_par_locally_free(p2.expect("EPercentPercent.p2")),
            ),
            Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EPlusPlus.p1")),
                take_par_locally_free(p2.expect("EPlusPlus.p2")),
            ),
            Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => union(
                take_par_locally_free(p1.expect("EMinusMinus.p1")),
                take_par_locally_free(p2.expect("EMinusMinus.p2")),
            ),

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

    fn locally_free(&self, p: Par, _depth: i32) -> Vec<u8> { take_par_locally_free(p) }
}

impl HasLocallyFree<New> for New {
    fn connective_used(&self, n: New) -> bool { n.p.expect("New.p").connective_used }

    fn locally_free(&self, n: New, _depth: i32) -> Vec<u8> { n.locally_free }
}

// ===========================================================================
// BY-REFERENCE READERS — the Leg-1 shape for the `HasLocallyFree` family.
//
// Every method of `HasLocallyFree<T> for T` takes its subject BY VALUE, but
// none of them recurses: each one reads a CACHED `locally_free` / `connective_
// used` field off the node (or off its immediate children). The by-value
// signature therefore forces callers to write `x.locally_free(x.clone(), d)` —
// a full Theta(depth) recursive deep clone of the entire subtree to read one
// cached bitset.
//
// That clone is not merely wasteful: `<Par as Clone>::clone` is itself a
// Theta(depth) NATIVE-STACK traversal (measured: 15.50 KiB/level debug,
// 2.78 KiB/level release — see `docs/design/audits/theta-depth-traversals-2026-07-26.md`),
// so a reader used inside another traversal ADDS a second depth-linear stack
// consumer on top of it.
//
// These free functions are the by-reference equivalents. They are O(1) per node
// and allocate only the returned bitset. Results are byte-identical to the
// by-value trait methods by construction: each arm reads exactly the same field.
//
// `expr_locally_free_ref` was first introduced (and validated against the
// eval-SCC differential harness) as a private helper in `reduce.rs`; it is
// hoisted here so there is ONE implementation shared by every caller.
// ===========================================================================

/// By-reference equivalent of `<Expr as HasLocallyFree<Expr>>::locally_free`.
/// `EVar` clones only the SHALLOW `Var`, matching the original.
///
/// ⚠ `depth` is threaded into the `EVar` arm exactly as the by-value impl does.
/// The original private copy in `reduce.rs` hardcoded `0` — sound there because
/// its only caller (`update_locally_free_par`) is a depth-0 reader, but NOT sound
/// for `prepend_expr`, which is called at pattern depth > 0 from `sub_exp`.
/// Parameterising it is what makes one shared implementation exact everywhere.
pub fn expr_locally_free_ref(expr: &Expr, depth: i32) -> Vec<u8> {
    match &expr.expr_instance {
        Some(GBool(_)) => Vec::new(),
        Some(GInt(_)) => Vec::new(),
        Some(GUint64(_)) => Vec::new(),
        Some(GInt32(_)) => Vec::new(),
        Some(GUint32(_)) => Vec::new(),
        Some(GUint16(_)) => Vec::new(),
        Some(GUint8(_)) => Vec::new(),
        Some(GDouble(_)) => Vec::new(),
        Some(GFloat32(_)) => Vec::new(),
        Some(GBigInt(_)) => Vec::new(),
        Some(GBigRat(_)) => Vec::new(),
        Some(GFixedPoint(_)) => Vec::new(),
        Some(GString(_)) => Vec::new(),
        Some(GUri(_)) => Vec::new(),
        Some(GByteArray(_)) => Vec::new(),

        Some(EListBody(e)) => e.locally_free.clone(),
        Some(ETupleBody(e)) => e.locally_free.clone(),
        Some(ESetBody(e)) => e.locally_free.clone(),
        Some(EMapBody(e)) => e.locally_free.clone(),
        Some(EPathmapBody(e)) => e.locally_free.clone(),
        Some(EZipperBody(e)) => e.locally_free.clone(),

        Some(EVarBody(EVar { v })) => {
            let v = v.clone().expect("expr_locally_free_ref: EVar with no Var");
            v.clone().locally_free(v, depth)
        }
        Some(ENotBody(ENot { p })) => p.as_ref().expect("ENot.p").locally_free.clone(),
        Some(ENegBody(ENeg { p })) => p.as_ref().expect("ENeg.p").locally_free.clone(),

        Some(EMultBody(EMult { p1, p2 })) => lf2(p1, p2),
        Some(EDivBody(EDiv { p1, p2 })) => lf2(p1, p2),
        Some(EModBody(EMod { p1, p2 })) => lf2(p1, p2),
        Some(EPlusBody(EPlus { p1, p2 })) => lf2(p1, p2),
        Some(EMinusBody(EMinus { p1, p2 })) => lf2(p1, p2),
        Some(ELtBody(ELt { p1, p2 })) => lf2(p1, p2),
        Some(ELteBody(ELte { p1, p2 })) => lf2(p1, p2),
        Some(EGtBody(EGt { p1, p2 })) => lf2(p1, p2),
        Some(EGteBody(EGte { p1, p2 })) => lf2(p1, p2),
        Some(EEqBody(EEq { p1, p2 })) => lf2(p1, p2),
        Some(ENeqBody(ENeq { p1, p2 })) => lf2(p1, p2),
        Some(EAndBody(EAnd { p1, p2 })) => lf2(p1, p2),
        Some(EOrBody(EOr { p1, p2 })) => lf2(p1, p2),

        Some(EMethodBody(e)) => e.locally_free.clone(),
        // ★ BOTH SLOTS. Not `lf2`, so a missing slot names ITSELF rather than
        // "binary operand", and not inlined, so the normalizer's production
        // reader and the guard assert the same function.
        Some(EMatchesBody(EMatches { target, pattern })) => ematches_locally_free(
            target.as_ref().expect("EMatches.target"),
            pattern.as_ref().expect("EMatches.pattern"),
        ),

        Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => lf2(p1, p2),
        Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => lf2(p1, p2),
        Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => lf2(p1, p2),

        None => Vec::new(),
    }
}

/// `union` of two operand Pars' cached `locally_free` bitsets, by reference.
#[inline]
fn lf2(p1: &Option<Par>, p2: &Option<Par>) -> Vec<u8> {
    union(
        p1.as_ref().expect("binary operand p1").locally_free.clone(),
        p2.as_ref().expect("binary operand p2").locally_free.clone(),
    )
}

/// By-reference equivalent of `<Expr as HasLocallyFree<Expr>>::connective_used`.
/// Every arm reads the same cached field the by-value form reads.
pub fn expr_connective_used_ref(expr: &Expr) -> bool {
    match &expr.expr_instance {
        Some(GBool(_)) | Some(GInt(_)) | Some(GUint64(_)) | Some(GInt32(_)) | Some(GUint32(_))
        | Some(GUint16(_)) | Some(GUint8(_)) | Some(GDouble(_)) | Some(GFloat32(_))
        | Some(GBigInt(_)) | Some(GBigRat(_)) | Some(GFixedPoint(_)) | Some(GString(_))
        | Some(GUri(_)) | Some(GByteArray(_)) => false,

        Some(EListBody(e)) => e.connective_used,
        Some(ETupleBody(e)) => e.connective_used,
        Some(ESetBody(e)) => e.connective_used,
        Some(EMapBody(e)) => e.connective_used,
        Some(EPathmapBody(e)) => e.connective_used,
        Some(EZipperBody(e)) => e.connective_used,

        Some(EVarBody(EVar { v })) => {
            let v = v
                .clone()
                .expect("expr_connective_used_ref: EVar with no Var");
            v.clone().connective_used(v)
        }
        Some(ENotBody(ENot { p })) => p.as_ref().expect("ENot.p").connective_used,
        Some(ENegBody(ENeg { p })) => p.as_ref().expect("ENeg.p").connective_used,

        Some(EMultBody(EMult { p1, p2 })) => cu2(p1, p2),
        Some(EDivBody(EDiv { p1, p2 })) => cu2(p1, p2),
        Some(EModBody(EMod { p1, p2 })) => cu2(p1, p2),
        Some(EPlusBody(EPlus { p1, p2 })) => cu2(p1, p2),
        Some(EMinusBody(EMinus { p1, p2 })) => cu2(p1, p2),
        Some(ELtBody(ELt { p1, p2 })) => cu2(p1, p2),
        Some(ELteBody(ELte { p1, p2 })) => cu2(p1, p2),
        Some(EGtBody(EGt { p1, p2 })) => cu2(p1, p2),
        Some(EGteBody(EGte { p1, p2 })) => cu2(p1, p2),
        Some(EEqBody(EEq { p1, p2 })) => cu2(p1, p2),
        Some(ENeqBody(ENeq { p1, p2 })) => cu2(p1, p2),
        Some(EAndBody(EAnd { p1, p2 })) => cu2(p1, p2),
        Some(EOrBody(EOr { p1, p2 })) => cu2(p1, p2),

        Some(EMethodBody(e)) => e.connective_used,
        Some(EMatchesBody(EMatches { target, .. })) => {
            target.as_ref().expect("EMatches.target").connective_used
        }

        Some(EPercentPercentBody(EPercentPercent { p1, p2 })) => cu2(p1, p2),
        Some(EPlusPlusBody(EPlusPlus { p1, p2 })) => cu2(p1, p2),
        Some(EMinusMinusBody(EMinusMinus { p1, p2 })) => cu2(p1, p2),

        None => false,
    }
}

#[inline]
fn cu2(p1: &Option<Par>, p2: &Option<Par>) -> bool {
    p1.as_ref().expect("binary operand p1").connective_used
        | p2.as_ref().expect("binary operand p2").connective_used
}

/// By-reference equivalent of `<Connective as HasLocallyFree<Connective>>::locally_free`.
pub fn connective_locally_free_ref(conn: &Connective, depth: i32) -> Vec<u8> {
    match &conn.connective_instance {
        Some(VarRefBody(VarRef {
            index: idx,
            depth: var_depth,
        })) => {
            if depth == *var_depth {
                create_bit_vector(&[*idx as usize])
            } else {
                Default::default()
            }
        }
        _ => Default::default(),
    }
}

/// By-reference equivalent of `<Connective as HasLocallyFree<Connective>>::connective_used`.
pub fn connective_connective_used_ref(conn: &Connective) -> bool {
    match &conn.connective_instance {
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

#[cfg(test)]
mod tests {
    use models::rhoapi::connective::ConnectiveInstance;
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::EMethod;
    use models::rust::utils::{new_freevar_par, new_gint_par};

    use super::*;

    fn ctx() -> SpatialMatcherContext<'static> { SpatialMatcherContext::new() }

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

        let par = models::par_from_default! {
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
            cost_signature: None,
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
            cost_signature: None,
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
