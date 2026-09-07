use models::rhoapi::If;
use models::rust::par_bag::ParComponentItem;

use super::exports::*;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum CountBound {
    Finite(u128),
    Unbounded,
}

impl CountBound {
    pub fn add(self, other: Self) -> Self {
        match (self, other) {
            (Self::Finite(left), Self::Finite(right)) => left
                .checked_add(right)
                .map(Self::Finite)
                .unwrap_or(Self::Unbounded),
            _ => Self::Unbounded,
        }
    }

    pub fn scale(self, factor: u64) -> Self {
        if factor == 0 {
            return Self::Finite(0);
        }
        match self {
            Self::Finite(value) => value
                .checked_mul(u128::from(factor))
                .map(Self::Finite)
                .unwrap_or(Self::Unbounded),
            Self::Unbounded => Self::Unbounded,
        }
    }
}

impl From<u64> for CountBound {
    fn from(value: u64) -> Self { Self::Finite(u128::from(value)) }
}

impl From<u128> for CountBound {
    fn from(value: u128) -> Self { Self::Finite(value) }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParCount.scala
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParCount {
    pub sends: CountBound,
    pub receives: CountBound,
    pub news: CountBound,
    pub exprs: CountBound,
    pub matches: CountBound,
    pub unforgeables: CountBound,
    pub bundles: CountBound,
    pub conditionals: CountBound,
}

impl ParCount {
    pub fn new(par: &Par) -> Self {
        ParCount {
            sends: CountBound::Finite(par.family_cardinality_exact::<Send>()),
            receives: CountBound::Finite(par.family_cardinality_exact::<Receive>()),
            news: CountBound::Finite(par.family_cardinality_exact::<New>()),
            exprs: CountBound::Finite(par.family_cardinality_exact::<Expr>()),
            matches: CountBound::Finite(par.family_cardinality_exact::<Match>()),
            unforgeables: CountBound::Finite(par.family_cardinality_exact::<GUnforgeable>()),
            bundles: CountBound::Finite(par.family_cardinality_exact::<Bundle>()),
            conditionals: CountBound::Finite(par.family_cardinality_exact::<If>()),
        }
    }

    pub const fn zero() -> Self {
        ParCount {
            sends: CountBound::Finite(0),
            receives: CountBound::Finite(0),
            news: CountBound::Finite(0),
            exprs: CountBound::Finite(0),
            matches: CountBound::Finite(0),
            unforgeables: CountBound::Finite(0),
            bundles: CountBound::Finite(0),
            conditionals: CountBound::Finite(0),
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
            conditionals: self.conditionals.max(other.conditionals),
        }
    }

    /// Top element of the matcher count interval domain.
    ///
    /// This is a logical bound, not a resource budget. The historical port
    /// used `1000`, which silently rejected legal wildcard/remainder matches
    /// above that cardinality. Each search intersects this value with the
    /// finite target family before producing positional candidates.
    pub const fn unbounded() -> ParCount {
        Self {
            sends: CountBound::Unbounded,
            receives: CountBound::Unbounded,
            news: CountBound::Unbounded,
            matches: CountBound::Unbounded,
            exprs: CountBound::Unbounded,
            unforgeables: CountBound::Unbounded,
            bundles: CountBound::Unbounded,
            conditionals: CountBound::Unbounded,
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
            conditionals: self.conditionals.min(other.conditionals),
        }
    }

    pub fn add(&self, other: &Self) -> Self {
        Self {
            sends: self.sends.add(other.sends),
            receives: self.receives.add(other.receives),
            news: self.news.add(other.news),
            exprs: self.exprs.add(other.exprs),
            matches: self.matches.add(other.matches),
            unforgeables: self.unforgeables.add(other.unforgeables),
            bundles: self.bundles.add(other.bundles),
            conditionals: self.conditionals.add(other.conditionals),
        }
    }

    fn scale(&self, amount: u64) -> Self {
        Self {
            sends: self.sends.scale(amount),
            receives: self.receives.scale(amount),
            news: self.news.scale(amount),
            exprs: self.exprs.scale(amount),
            matches: self.matches.scale(amount),
            unforgeables: self.unforgeables.scale(amount),
            bundles: self.bundles.scale(amount),
            conditionals: self.conditionals.scale(amount),
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
        let mut wildcard = false;
        let mut concrete_exprs = 0u128;
        par.try_visit_components::<Expr, std::convert::Infallible>(|expr, multiplicity| {
            if matches!(
                expr.as_ref().expr_instance.as_ref(),
                Some(EVarBody(EVar {
                    v: Some(Var {
                        var_instance: Some(FreeVar(_) | Wildcard(_)),
                    }),
                }))
            ) {
                wildcard = true;
            } else {
                concrete_exprs = concrete_exprs
                    .checked_add(u128::from(multiplicity.get()))
                    .expect("nonfree expression mass fits the exact family total");
            }
            Ok(())
        })
        .expect("Par invariant: expression endpoints are canonical");
        let pc = ParCount {
            sends: CountBound::Finite(par.family_cardinality_exact::<Send>()),
            receives: CountBound::Finite(par.family_cardinality_exact::<Receive>()),
            news: CountBound::Finite(par.family_cardinality_exact::<New>()),
            exprs: CountBound::Finite(concrete_exprs),
            matches: CountBound::Finite(par.family_cardinality_exact::<Match>()),
            unforgeables: CountBound::Finite(par.family_cardinality_exact::<GUnforgeable>()),
            bundles: CountBound::Finite(par.family_cardinality_exact::<Bundle>()),
            conditionals: CountBound::Finite(par.family_cardinality_exact::<If>()),
        };
        let base = (pc.clone(), if wildcard { Self::unbounded() } else { pc });
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
                        Some(ConnNotBody(_)) => values.push((Self::zero(), Self::unbounded())),
                        None | Some(VarRefBody(_)) => {
                            values.push((Self::zero(), Self::zero()));
                        }
                        Some(
                            ConnBool(_) | ConnInt(_) | ConnString(_) | ConnUri(_)
                            | ConnByteArray(_),
                        ) => {
                            let mut one = Self::zero();
                            one.exprs = CountBound::Finite(1);
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
                        Some(ConnNotBody(_)) => values.push((Self::zero(), Self::unbounded())),
                        None | Some(VarRefBody(_)) => {
                            values.push((Self::zero(), Self::zero()));
                        }
                        Some(
                            ConnBool(_) | ConnInt(_) | ConnString(_) | ConnUri(_)
                            | ConnByteArray(_),
                        ) => {
                            let mut one = Self::zero();
                            one.exprs = CountBound::Finite(1);
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
                    let mut result = (Self::zero(), Self::unbounded());
                    for (child_min, child_max) in values.drain(start..) {
                        result.0 = result.0.max(&child_min);
                        result.1 = result.1.min(&child_max);
                    }
                    values.push(result);
                }
                CountWork::ReduceOr(child_count) => {
                    let start = values.len() - child_count;
                    let mut result = (Self::unbounded(), Self::zero());
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
