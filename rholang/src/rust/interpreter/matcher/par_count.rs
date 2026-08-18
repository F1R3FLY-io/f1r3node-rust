use models::rust::par_bag::ParComponentItem;

use super::exports::*;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParCount.scala
#[derive(Clone, Debug)]
pub struct ParCount {
    pub sends: u64,
    pub receives: u64,
    pub news: u64,
    pub exprs: u64,
    pub matches: u64,
    pub unforgeables: u64,
    pub bundles: u64,
}

impl ParCount {
    pub fn new(par: &Par) -> Self {
        ParCount {
            sends: par.family_cardinality::<Send>(),
            receives: par.family_cardinality::<Receive>(),
            news: par.family_cardinality::<New>(),
            exprs: par.family_cardinality::<Expr>(),
            matches: par.family_cardinality::<Match>(),
            unforgeables: par.family_cardinality::<GUnforgeable>(),
            bundles: par.family_cardinality::<Bundle>(),
        }
    }

    pub fn _new(&self) -> Self {
        ParCount {
            sends: 0,
            receives: 0,
            news: 0,
            exprs: 0,
            matches: 0,
            unforgeables: 0,
            bundles: 0,
        }
    }

    pub fn max(&self, other: &Self) -> Self {
        Self {
            sends: self.sends.max(other.sends),
            receives: self.receives.max(other.receives),
            news: self.news.max(other.news),
            exprs: self.exprs.max(other.exprs),
            matches: self.matches.max(other.matches),
            unforgeables: self.unforgeables.max(other.unforgeables),
            bundles: self.bundles.max(other.bundles),
        }
    }

    pub fn _max(&self) -> ParCount {
        Self {
            sends: 1000,
            receives: 1000,
            news: 1000,
            matches: 1000,
            exprs: 1000,
            unforgeables: 1000,
            bundles: 1000,
        }
    }

    pub fn min(&self, other: &Self) -> Self {
        Self {
            sends: self.sends.min(other.sends),
            receives: self.receives.min(other.receives),
            news: self.news.min(other.news),
            exprs: self.exprs.min(other.exprs),
            matches: self.matches.min(other.matches),
            unforgeables: self.unforgeables.min(other.unforgeables),
            bundles: self.bundles.min(other.bundles),
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self {
            sends: self.sends.saturating_add(other.sends),
            receives: self.receives.saturating_add(other.receives),
            news: self.news.saturating_add(other.news),
            exprs: self.exprs.saturating_add(other.exprs),
            matches: self.matches.saturating_add(other.matches),
            unforgeables: self.unforgeables.saturating_add(other.unforgeables),
            bundles: self.bundles.saturating_add(other.bundles),
        }
    }

    fn scale(&self, amount: u64) -> Self {
        Self {
            sends: self.sends.saturating_mul(amount),
            receives: self.receives.saturating_mul(amount),
            news: self.news.saturating_mul(amount),
            exprs: self.exprs.saturating_mul(amount),
            matches: self.matches.saturating_mul(amount),
            unforgeables: self.unforgeables.saturating_mul(amount),
            bundles: self.bundles.saturating_mul(amount),
        }
    }

    pub fn min_max_par(&self, par: Par) -> (ParCount, ParCount) { self.min_max_par_ref(&par) }

    pub fn min_max_con(&self, con: Connective) -> (ParCount, ParCount) {
        self.min_max_con_ref(&con)
    }

    /// Borrowed entry used by the production PDA while it still owns the AST.
    /// Avoids cloning an entire recursive connective solely to compute bounds.
    pub fn min_max_par_ref(&self, par: &Par) -> (ParCount, ParCount) {
        self.min_max_drive(CountNode::Par(par))
    }

    /// Borrowed entry used by the production PDA while it still owns the AST.
    /// The legacy owned method remains for source compatibility.
    pub fn min_max_con_ref(&self, con: &Connective) -> (ParCount, ParCount) {
        self.min_max_drive(CountNode::Connective(con))
    }

    fn par_frame(&self, par: &Par) -> (usize, (ParCount, ParCount)) {
        let mut free_exprs = 0u64;
        par.try_visit_components::<Expr, std::convert::Infallible>(|expr, multiplicity| {
            if matches!(
                expr.as_ref().expr_instance.as_ref(),
                Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(FreeVar(_) | Wildcard(_)),
                    }),
                }))
            ) {
                free_exprs = free_exprs.saturating_add(multiplicity.get());
            }
            Ok(())
        })
        .expect("Par invariant: expression endpoints are canonical");
        let wildcard = free_exprs != 0;
        let exprs = par.family_cardinality::<Expr>();
        let pc = ParCount {
            sends: par.family_cardinality::<Send>(),
            receives: par.family_cardinality::<Receive>(),
            news: par.family_cardinality::<New>(),
            exprs: exprs.saturating_sub(free_exprs),
            matches: par.family_cardinality::<Match>(),
            unforgeables: par.family_cardinality::<GUnforgeable>(),
            bundles: par.family_cardinality::<Bundle>(),
        };
        let base = (pc.clone(), if wildcard { self._max() } else { pc });
        (par.family_distinct_len::<Connective>(), base)
    }

    fn schedule_borrowed_par<'a>(&self, par: &'a Par, work: &mut Vec<CountWork<'a>>) {
        let (child_count, base) = self.par_frame(par);
        work.push(CountWork::ReducePar { child_count, base });
        par.try_visit_components::<Connective, std::convert::Infallible>(
            |connective, multiplicity| {
                work.push(CountWork::Scale(multiplicity.get()));
                work.push(CountWork::Visit(match connective {
                    ParComponentItem::Borrowed(value) => CountNode::Connective(value),
                    ParComponentItem::Owned(value) => CountNode::OwnedConnective(value),
                }));
                Ok(())
            },
        )
        .expect("Par invariant: connective endpoints are canonical");
    }

    fn schedule_owned_par<'a>(&self, par: Par, work: &mut Vec<CountWork<'a>>) {
        let (child_count, base) = self.par_frame(&par);
        work.push(CountWork::ReducePar { child_count, base });
        par.try_into_components::<Connective, std::convert::Infallible>(
            |connective, multiplicity| {
                work.push(CountWork::Scale(multiplicity.get()));
                work.push(CountWork::Visit(CountNode::OwnedConnective(connective)));
                Ok(())
            },
        )
        .expect("Par invariant: connective endpoints are canonical");
    }

    /// Defunctionalized evaluator for the mutually recursive
    /// `min_max_par`/`min_max_con` equations.
    ///
    /// The old implementation consumed one native frame for every nested
    /// connective. `CountWork` is the generated-call shape made explicit:
    /// children are evaluated left-to-right, their values are reduced in one
    /// heap-resident frame, and the public entry points retain their original
    /// owned signatures.
    fn min_max_drive(&self, root: CountNode<'_>) -> (ParCount, ParCount) {
        let mut work = vec![CountWork::Visit(root)];
        let mut values = Vec::new();

        while let Some(task) = work.pop() {
            match task {
                CountWork::Visit(CountNode::Par(par)) => self.schedule_borrowed_par(par, &mut work),
                CountWork::Visit(CountNode::Connective(connective)) => {
                    match connective.connective_instance.as_ref() {
                        Some(ConnAndBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ReduceAnd(ps.len()));
                            for par in ps.iter().rev() {
                                work.push(CountWork::Visit(CountNode::Par(par)));
                            }
                        }
                        Some(ConnOrBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ReduceOr(ps.len()));
                            for par in ps.iter().rev() {
                                work.push(CountWork::Visit(CountNode::Par(par)));
                            }
                        }
                        Some(ConnNotBody(_)) => values.push((self._new(), self._max())),
                        None | Some(VarRefBody(_)) => {
                            values.push((self._new(), self._new()));
                        }
                        Some(
                            ConnBool(_) | ConnInt(_) | ConnString(_) | ConnUri(_)
                            | ConnByteArray(_),
                        ) => {
                            let mut one = self._new();
                            one.exprs = 1;
                            values.push((one.clone(), one));
                        }
                    }
                }
                CountWork::Visit(CountNode::OwnedConnective(connective)) => {
                    match connective.connective_instance {
                        Some(ConnAndBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ReduceAnd(ps.len()));
                            for par in ps.into_iter().rev() {
                                work.push(CountWork::Visit(CountNode::OwnedPar(par)));
                            }
                        }
                        Some(ConnOrBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ReduceOr(ps.len()));
                            for par in ps.into_iter().rev() {
                                work.push(CountWork::Visit(CountNode::OwnedPar(par)));
                            }
                        }
                        Some(ConnNotBody(_)) => values.push((self._new(), self._max())),
                        None | Some(VarRefBody(_)) => {
                            values.push((self._new(), self._new()));
                        }
                        Some(
                            ConnBool(_) | ConnInt(_) | ConnString(_) | ConnUri(_)
                            | ConnByteArray(_),
                        ) => {
                            let mut one = self._new();
                            one.exprs = 1;
                            values.push((one.clone(), one));
                        }
                    }
                }
                CountWork::Visit(CountNode::OwnedPar(par)) => {
                    self.schedule_owned_par(par, &mut work);
                }
                CountWork::Scale(amount) => {
                    let (minimum, maximum) = values
                        .pop()
                        .expect("multiplicity scales one connective result");
                    values.push((minimum.scale(amount), maximum.scale(amount)));
                }
                CountWork::ReducePar { child_count, base } => {
                    let start = values.len() - child_count;
                    let mut result = base;
                    for (child_min, child_max) in values.drain(start..) {
                        result.0 = result.0.add(&child_min);
                        result.1 = result.1.add(&child_max);
                    }
                    values.push(result);
                }
                CountWork::ReduceAnd(child_count) => {
                    let start = values.len() - child_count;
                    let mut result = (self._new(), self._max());
                    for (child_min, child_max) in values.drain(start..) {
                        result.0 = result.0.max(&child_min);
                        result.1 = result.1.min(&child_max);
                    }
                    values.push(result);
                }
                CountWork::ReduceOr(child_count) => {
                    let start = values.len() - child_count;
                    let mut result = (self._max(), self._new());
                    for (child_min, child_max) in values.drain(start..) {
                        result.0 = result.0.min(&child_min);
                        result.1 = result.1.max(&child_max);
                    }
                    values.push(result);
                }
            }
        }

        let result = values
            .pop()
            .expect("the ParCount PDA emits one result for its root");
        debug_assert!(
            values.is_empty(),
            "the ParCount PDA leaves no sibling values"
        );
        result
    }
}

enum CountNode<'a> {
    Par(&'a Par),
    Connective(&'a Connective),
    OwnedPar(Par),
    OwnedConnective(Connective),
}

enum CountWork<'a> {
    Visit(CountNode<'a>),
    ReducePar {
        child_count: usize,
        base: (ParCount, ParCount),
    },
    ReduceAnd(usize),
    ReduceOr(usize),
    Scale(u64),
}
