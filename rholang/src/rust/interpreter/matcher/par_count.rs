use super::exports::*;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParCount.scala
#[derive(Clone, Debug)]
pub struct ParCount {
    pub sends: usize,
    pub receives: usize,
    pub news: usize,
    pub exprs: usize,
    pub matches: usize,
    pub unforgeables: usize,
    pub bundles: usize,
}

impl ParCount {
    pub fn new(par: &Par) -> Self {
        ParCount {
            sends: par.sends.len(),
            receives: par.receives.len(),
            news: par.news.len(),
            exprs: par.exprs.len(),
            matches: par.matches.len(),
            unforgeables: par.unforgeables.len(),
            bundles: par.bundles.len(),
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

    pub fn min_max_par(&self, par: Par) -> (ParCount, ParCount) {
        self.min_max_drive(CountNode::Par(&par))
    }

    pub fn min_max_con(&self, con: Connective) -> (ParCount, ParCount) {
        self.min_max_drive(CountNode::Connective(&con))
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
                CountWork::Visit(CountNode::Par(par)) => {
                    let free_exprs = par
                        .exprs
                        .iter()
                        .filter(|expr| {
                            matches!(
                                expr.expr_instance,
                                Some(EVarBody(EVar {
                                    v: Some(Var {
                                        var_instance: Some(FreeVar(_) | Wildcard(_)),
                                    }),
                                }))
                            )
                        })
                        .count();
                    let wildcard = free_exprs != 0;
                    let pc = ParCount {
                        sends: par.sends.len(),
                        receives: par.receives.len(),
                        news: par.news.len(),
                        exprs: par.exprs.len() - free_exprs,
                        matches: par.matches.len(),
                        unforgeables: par.unforgeables.len(),
                        bundles: par.bundles.len(),
                    };
                    let base = (pc.clone(), if wildcard { self._max() } else { pc });
                    work.push(CountWork::ReducePar {
                        child_count: par.connectives.len(),
                        base,
                    });
                    for connective in par.connectives.iter().rev() {
                        work.push(CountWork::Visit(CountNode::Connective(connective)));
                    }
                }
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

#[derive(Clone, Copy)]
enum CountNode<'a> {
    Par(&'a Par),
    Connective(&'a Connective),
}

enum CountWork<'a> {
    Visit(CountNode<'a>),
    ReducePar {
        child_count: usize,
        base: (ParCount, ParCount),
    },
    ReduceAnd(usize),
    ReduceOr(usize),
}
