use models::rhoapi::If;
use models::rust::par_bag::{
    canonical_connective_parts, canonical_count_expr_is_hole, CanonicalConnectiveParts,
    CanonicalOwnedConnectiveChildren, ParBagComponent, ParComponentItem,
};
use models::rust::rholang::protobuf_cursor::{
    CanonicalByteRange, CanonicalParRegionIndex, CanonicalRegionDelimitedCursor,
    CanonicalRegionDelimitedRun, ProtobufRegion,
};
use models::rust::rholang::protobuf_decoder;

use super::exports::*;

#[cfg(test)]
#[path = "../../../../tests/support/par_count_byte_native.rs"]
mod byte_native_tests;

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
    pub fn min_max_par_ref(&self, par: &Par) -> (ParCount, ParCount) { self.min_max_resident(par) }

    /// Borrowed entry used by the production PDA while it still owns the AST.
    /// The legacy owned method remains for source compatibility.
    pub fn min_max_con_ref(&self, con: &Connective) -> (ParCount, ParCount) {
        self.min_max_drive::<ResidentByteSource>(CountNode::Connective(con))
    }

    fn min_max_resident(&self, par: &Par) -> (ParCount, ParCount) {
        let mut minimum = Self::new(par);
        minimum.exprs = CountBound::Finite(0);
        let mut hole = false;
        par.visit_encoded::<Expr>(|entry| {
            let multiplicity = entry.multiplicity();
            let is_hole = canonical_count_expr_is_hole(entry.payload()).unwrap_or_else(|| {
                #[cfg(test)]
                byte_native_tests::decoded_component();
                let expr = entry
                    .decode()
                    .expect("Par invariant: expression endpoints are canonical");
                expression_is_hole(&expr)
            });
            if is_hole {
                hole = true;
            } else {
                minimum.exprs = minimum.exprs.add(CountBound::from(multiplicity.get()));
            }
        });
        let mut result = (
            minimum.clone(),
            if hole { Self::unbounded() } else { minimum },
        );
        par.visit_encoded::<Connective>(|entry| {
            #[cfg(test)]
            byte_native_tests::copied_endpoint(entry.payload().len(), entry.multiplicity().get());
            let region = ProtobufRegion::copy_from_slice(entry.payload());
            let child = self.min_max_drive::<DecodedSource>(CountNode::EncodedConnective(region));
            result.0 = result.0.add(&child.0.scale(entry.multiplicity().get()));
            result.1 = result.1.add(&child.1.scale(entry.multiplicity().get()));
        });
        result
    }

    fn encoded_par_frame(
        &self,
        index: CanonicalParRegionIndex,
    ) -> ((ParCount, ParCount), Option<CanonicalRegionDelimitedCursor>) {
        let count = |run: Option<CanonicalRegionDelimitedRun>| {
            let mut total = 0u128;
            if let Some(run) = run {
                let mut cursor = run.cursor();
                while cursor
                    .next_region()
                    .expect("canonical Par family run")
                    .is_some()
                {
                    total += 1;
                }
            }
            CountBound::Finite(total)
        };
        let mut minimum = ParCount {
            sends: count(index.sends),
            receives: count(index.receives),
            news: count(index.news),
            exprs: CountBound::Finite(0),
            matches: count(index.matches),
            unforgeables: count(index.unforgeables),
            bundles: count(index.bundles),
            conditionals: count(index.conditionals),
        };
        let mut hole = false;
        if let Some(run) = index.exprs {
            let mut cursor = run.cursor();
            while let Some(expr) = cursor.next_region().expect("canonical Expr run") {
                let is_hole = canonical_count_expr_is_hole(expr.as_bytes()).unwrap_or_else(|| {
                    let decoded = decode_count_component(
                        expr.as_bytes(),
                        Expr::decode_protobuf_deferred_epathmap_validation,
                    );
                    expression_is_hole(&decoded)
                });
                if is_hole {
                    hole = true;
                } else {
                    minimum.exprs = minimum.exprs.add(CountBound::Finite(1));
                }
            }
        }
        let result = (
            minimum.clone(),
            if hole { Self::unbounded() } else { minimum },
        );
        (result, index.connectives.map(|run| run.cursor()))
    }

    fn par_frame(&self, par: &Par) -> (usize, (ParCount, ParCount)) {
        let mut wildcard = false;
        let mut concrete_exprs = 0u128;
        par.try_visit_components::<Expr, std::convert::Infallible>(|expr, multiplicity| {
            #[cfg(test)]
            byte_native_tests::decoded_component();
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
                #[cfg(test)]
                byte_native_tests::decoded_component();
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
                #[cfg(test)]
                byte_native_tests::decoded_component();
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
    fn min_max_drive<S: ResidentCountSource>(&self, root: CountNode<'_>) -> (ParCount, ParCount) {
        let mut work = vec![CountWork::Visit(root)];
        let mut values = Vec::new();

        while let Some(task) = work.pop() {
            #[cfg(test)]
            byte_native_tests::live_work(work.len() + 1);
            match task {
                CountWork::Visit(CountNode::Par(par)) => {
                    S::borrowed(self, par, &mut work, &mut values)
                }
                CountWork::Visit(CountNode::EncodedPar(region)) => {
                    #[cfg(test)]
                    byte_native_tests::visited_region(false, &region);
                    match CanonicalParRegionIndex::parse(region.clone()) {
                        Ok(index) => {
                            let (result, children) = self.encoded_par_frame(index);
                            if let Some(children) = children {
                                work.push(CountWork::ContinueFold {
                                    operator: CountOperator::Parallel,
                                    children: CountChildren::EncodedConnectives(children),
                                    result,
                                });
                            } else {
                                values.push(result);
                            }
                        }
                        Err(_) => work.push(CountWork::Visit(CountNode::OwnedPar(
                            decode_count_component(region.as_bytes(), |bytes| {
                                protobuf_decoder::decode_par(bytes)
                            }),
                        ))),
                    }
                }
                CountWork::Visit(CountNode::EncodedConnective(region)) => {
                    #[cfg(test)]
                    byte_native_tests::visited_region(true, &region);
                    match canonical_connective_parts(region.as_bytes()) {
                        Some(
                            parts @ (CanonicalConnectiveParts::And(_)
                            | CanonicalConnectiveParts::Or(_)),
                        ) => {
                            let (operator, children) = match parts {
                                CanonicalConnectiveParts::And(children) => {
                                    (CountOperator::And, children)
                                }
                                CanonicalConnectiveParts::Or(children) => {
                                    (CountOperator::Or, children)
                                }
                                _ => unreachable!("the matched connective is AND or OR"),
                            };
                            let body = region
                                .subregion(CanonicalByteRange {
                                    start: region.len() - children.message().len(),
                                    end: region.len(),
                                })
                                .expect("canonical connective body ends at its owner boundary");
                            let children = children
                                .bind_owned(body)
                                .expect("canonical connective cursor retains its exact owner");
                            work.push(CountWork::ContinueFold {
                                operator,
                                children: CountChildren::EncodedPars(children),
                                result: operator.seed(),
                            });
                        }
                        Some(CanonicalConnectiveParts::Not(_)) => {
                            values.push((Self::zero(), Self::unbounded()));
                        }
                        Some(
                            CanonicalConnectiveParts::VarRef | CanonicalConnectiveParts::Absent,
                        ) => {
                            values.push((Self::zero(), Self::zero()));
                        }
                        Some(CanonicalConnectiveParts::Predicate { .. }) => {
                            let mut one = Self::zero();
                            one.exprs = CountBound::Finite(1);
                            values.push((one.clone(), one));
                        }
                        None => work.push(CountWork::Visit(CountNode::OwnedConnective(
                            decode_count_component(
                                region.as_bytes(),
                                Connective::decode_protobuf_deferred_epathmap_validation,
                            ),
                        ))),
                    }
                }
                CountWork::Visit(CountNode::Connective(connective)) => {
                    match connective.connective_instance.as_ref() {
                        Some(ConnAndBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ContinueFold {
                                operator: CountOperator::And,
                                children: CountChildren::BorrowedPars(ps.iter()),
                                result: CountOperator::And.seed(),
                            });
                        }
                        Some(ConnOrBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ContinueFold {
                                operator: CountOperator::Or,
                                children: CountChildren::BorrowedPars(ps.iter()),
                                result: CountOperator::Or.seed(),
                            });
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
                            work.push(CountWork::ContinueFold {
                                operator: CountOperator::And,
                                children: CountChildren::OwnedPars(ps.into_iter()),
                                result: CountOperator::And.seed(),
                            });
                        }
                        Some(ConnOrBody(ConnectiveBody { ps })) => {
                            work.push(CountWork::ContinueFold {
                                operator: CountOperator::Or,
                                children: CountChildren::OwnedPars(ps.into_iter()),
                                result: CountOperator::Or.seed(),
                            });
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
                    S::owned(self, par, &mut work, &mut values);
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
                CountWork::ContinueFold {
                    operator,
                    mut children,
                    result,
                } => {
                    if let Some(child) = children.next() {
                        work.push(CountWork::AcceptChild {
                            operator,
                            children,
                            result,
                        });
                        work.push(CountWork::Visit(child));
                    } else {
                        values.push(result);
                    }
                }
                CountWork::AcceptChild {
                    operator,
                    children,
                    result,
                } => {
                    let child = values
                        .pop()
                        .expect("one interval per suspended count child");
                    work.push(CountWork::ContinueFold {
                        operator,
                        children,
                        result: operator.combine(result, child),
                    });
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
    EncodedPar(ProtobufRegion),
    EncodedConnective(ProtobufRegion),
}

fn expression_is_hole(expr: &Expr) -> bool {
    matches!(
        expr.expr_instance.as_ref(),
        Some(EVarBody(EVar {
            v: Some(Var {
                var_instance: Some(FreeVar(_) | Wildcard(_))
            }),
        }))
    )
}

fn decode_count_component<T, E: std::fmt::Debug>(
    bytes: &[u8],
    decode: impl FnOnce(&[u8]) -> Result<T, E>,
) -> T {
    #[cfg(test)]
    byte_native_tests::decoded_component();
    decode(bytes).expect("Par count cursor delegates unproved framing to its generated decoder")
}

trait ResidentCountSource {
    fn borrowed<'a>(
        counter: &ParCount,
        par: &'a Par,
        work: &mut Vec<CountWork<'a>>,
        values: &mut Vec<(ParCount, ParCount)>,
    );
    fn owned<'a>(
        counter: &ParCount,
        par: Par,
        work: &mut Vec<CountWork<'a>>,
        values: &mut Vec<(ParCount, ParCount)>,
    );
}

struct ResidentByteSource;
struct DecodedSource;

impl ResidentCountSource for ResidentByteSource {
    fn borrowed<'a>(
        counter: &ParCount,
        par: &'a Par,
        _work: &mut Vec<CountWork<'a>>,
        values: &mut Vec<(ParCount, ParCount)>,
    ) {
        values.push(counter.min_max_resident(par));
    }

    fn owned<'a>(
        counter: &ParCount,
        par: Par,
        _work: &mut Vec<CountWork<'a>>,
        values: &mut Vec<(ParCount, ParCount)>,
    ) {
        values.push(counter.min_max_resident(&par));
    }
}

impl ResidentCountSource for DecodedSource {
    fn borrowed<'a>(
        counter: &ParCount,
        par: &'a Par,
        work: &mut Vec<CountWork<'a>>,
        _values: &mut Vec<(ParCount, ParCount)>,
    ) {
        counter.schedule_borrowed_par(par, work);
    }

    fn owned<'a>(
        counter: &ParCount,
        par: Par,
        work: &mut Vec<CountWork<'a>>,
        _values: &mut Vec<(ParCount, ParCount)>,
    ) {
        counter.schedule_owned_par(par, work);
    }
}

#[derive(Clone, Copy)]
enum CountOperator {
    Parallel,
    And,
    Or,
}

impl CountOperator {
    fn seed(self) -> (ParCount, ParCount) {
        match self {
            Self::Parallel => (ParCount::zero(), ParCount::zero()),
            Self::And => (ParCount::zero(), ParCount::unbounded()),
            Self::Or => (ParCount::unbounded(), ParCount::zero()),
        }
    }

    fn combine(
        self,
        left: (ParCount, ParCount),
        right: (ParCount, ParCount),
    ) -> (ParCount, ParCount) {
        match self {
            Self::Parallel => (left.0.add(&right.0), left.1.add(&right.1)),
            Self::And => (left.0.max(&right.0), left.1.min(&right.1)),
            Self::Or => (left.0.min(&right.0), left.1.max(&right.1)),
        }
    }
}

enum CountChildren<'a> {
    BorrowedPars(std::slice::Iter<'a, Par>),
    OwnedPars(std::vec::IntoIter<Par>),
    EncodedPars(CanonicalOwnedConnectiveChildren<ProtobufRegion>),
    EncodedConnectives(CanonicalRegionDelimitedCursor),
}

impl<'a> CountChildren<'a> {
    fn next(&mut self) -> Option<CountNode<'a>> {
        match self {
            Self::BorrowedPars(children) => children.next().map(CountNode::Par),
            Self::OwnedPars(children) => children.next().map(CountNode::OwnedPar),
            Self::EncodedPars(children) => {
                let range = children.next()?;
                let region = children
                    .body()
                    .subregion(CanonicalByteRange {
                        start: range.start,
                        end: range.end,
                    })
                    .expect("validated child range remains in its retained connective owner");
                Some(CountNode::EncodedPar(region))
            }
            Self::EncodedConnectives(children) => children
                .next_region()
                .expect("canonical connective family run")
                .map(CountNode::EncodedConnective),
        }
    }
}

enum CountWork<'a> {
    Visit(CountNode<'a>),
    ReducePar {
        child_count: usize,
        base: (ParCount, ParCount),
    },
    Scale(u64),
    ContinueFold {
        operator: CountOperator,
        children: CountChildren<'a>,
        result: (ParCount, ParCount),
    },
    AcceptChild {
        operator: CountOperator,
        children: CountChildren<'a>,
        result: (ParCount, ParCount),
    },
}
