use models::rust::utils::no_frees;

use super::exports::*;

/// The count of a kind a remainder or wildcard can take, as Scala's
/// `Int.MaxValue`. Counts saturate here, so every count also fits an `isize`.
pub const UNBOUNDED: usize = isize::MAX as usize;

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
            sends: UNBOUNDED,
            receives: UNBOUNDED,
            news: UNBOUNDED,
            matches: UNBOUNDED,
            exprs: UNBOUNDED,
            unforgeables: UNBOUNDED,
            bundles: UNBOUNDED,
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
        let add = |a: usize, b: usize| a.saturating_add(b).min(UNBOUNDED);
        Self {
            sends: add(self.sends, other.sends),
            receives: add(self.receives, other.receives),
            news: add(self.news, other.news),
            exprs: add(self.exprs, other.exprs),
            matches: add(self.matches, other.matches),
            unforgeables: add(self.unforgeables, other.unforgeables),
            bundles: add(self.bundles, other.bundles),
        }
    }

    pub fn min_max_par(&self, par: Par) -> (ParCount, ParCount) {
        let pc = ParCount::new(&no_frees(&par));
        let wildcard: bool = par.exprs.iter().any(|expr| match &expr.expr_instance {
            Some(EVarBody(EVar { v })) => match v.as_ref().unwrap().var_instance {
                Some(Wildcard(_)) => true,
                Some(FreeVar(_)) => true,
                _ => false,
            },

            _ => false,
        });

        let min_init = pc.clone();
        let max_init = if wildcard { self._max() } else { pc };

        par.connectives
            .iter()
            .fold((min_init, max_init), |(min, max), con| {
                let (cmin, cmax) = self.min_max_con(con.clone());
                (min.add(&cmin), max.add(&cmax))
            })
    }

    pub fn min_max_con(&self, con: Connective) -> (ParCount, ParCount) {
        match con.connective_instance {
            Some(ConnAndBody(ConnectiveBody { ps })) => {
                let p_min_max: Vec<(ParCount, ParCount)> =
                    ps.iter().map(|p| self.min_max_par(p.clone())).collect();

                let min = p_min_max
                    .iter()
                    .fold(self._new(), |acc, (min, _)| acc.max(min));
                let max = p_min_max
                    .iter()
                    .fold(self._max(), |acc, (_, max)| acc.min(max));
                (min, max)
            }

            Some(ConnOrBody(ConnectiveBody { ps })) => {
                let p_min_max: Vec<(ParCount, ParCount)> =
                    ps.iter().map(|p| self.min_max_par(p.clone())).collect();

                let min = p_min_max
                    .iter()
                    .fold(self._max(), |acc, (min, _)| acc.min(min));
                let max = p_min_max
                    .iter()
                    .fold(self._new(), |acc, (_, max)| acc.max(max));
                (min, max)
            }

            Some(ConnNotBody(_)) => (self._new(), self._max()),

            // Is this the same as 'ConnectiveInstance.Empty' in Scala?
            None => (self._new(), self._new()),

            Some(VarRefBody(_)) => (self._new(), self._new()),

            Some(ConnBool(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }

            Some(ConnInt(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }

            Some(ConnString(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }

            Some(ConnUri(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }

            Some(ConnByteArray(_))
            | Some(ConnInt32(_))
            | Some(ConnUint64(_))
            | Some(ConnUint32(_))
            | Some(ConnUint16(_))
            | Some(ConnUint8(_))
            | Some(ConnFloat64(_))
            | Some(ConnFloat32(_))
            | Some(ConnBigInt(_))
            | Some(ConnBigRat(_))
            | Some(ConnFixedPointBody(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }

            // CoC Phase 2c: `Type` matches a single type-level value.
            Some(ConnType(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }
        }
    }
}
