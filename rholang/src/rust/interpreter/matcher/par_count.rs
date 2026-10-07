use super::exports::*;

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/ParCount.scala
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParCount {
    pub sends: usize,
    pub receives: usize,
    pub news: usize,
    pub exprs: usize,
    pub matches: usize,
    pub unforgeables: usize,
    pub bundles: usize,
    pub conditionals: usize,
    pub cost_signed_terms: usize,
    pub cost_stacks: usize,
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
            conditionals: par.conditionals.len(),
            cost_signed_terms: par.cost_signed_terms.len(),
            cost_stacks: par.cost_stacks.len(),
        }
    }

    pub fn without_frees(par: &Par) -> Self {
        let mut count = Self::new(par);
        count.exprs = par
            .exprs
            .iter()
            .filter(|expr| {
                !matches!(
                    &expr.expr_instance,
                    Some(EVarBody(EVar {
                        v: Some(Var {
                            var_instance: Some(FreeVar(_) | Wildcard(_)),
                        }),
                    }))
                )
            })
            .count();
        count
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
            conditionals: 0,
            cost_signed_terms: 0,
            cost_stacks: 0,
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
            cost_signed_terms: self.cost_signed_terms.max(other.cost_signed_terms),
            cost_stacks: self.cost_stacks.max(other.cost_stacks),
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
            conditionals: 1000,
            cost_signed_terms: 1000,
            cost_stacks: 1000,
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
            cost_signed_terms: self.cost_signed_terms.min(other.cost_signed_terms),
            cost_stacks: self.cost_stacks.min(other.cost_stacks),
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
            conditionals: self.conditionals.saturating_add(other.conditionals),
            cost_signed_terms: self
                .cost_signed_terms
                .saturating_add(other.cost_signed_terms),
            cost_stacks: self.cost_stacks.saturating_add(other.cost_stacks),
        }
    }

    pub fn min_max_par(&self, par: &Par) -> (ParCount, ParCount) {
        let pc = ParCount::without_frees(par);
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
                let (cmin, cmax) = self.min_max_con(con);
                (min.add(&cmin), max.add(&cmax))
            })
    }

    pub fn min_max_con(&self, con: &Connective) -> (ParCount, ParCount) {
        match &con.connective_instance {
            Some(ConnAndBody(ConnectiveBody { ps })) => {
                ps.iter().fold((self._new(), self._max()), |(min, max), p| {
                    let (p_min, p_max) = self.min_max_par(p);
                    (min.max(&p_min), max.min(&p_max))
                })
            }

            Some(ConnOrBody(ConnectiveBody { ps })) => {
                ps.iter().fold((self._max(), self._new()), |(min, max), p| {
                    let (p_min, p_max) = self.min_max_par(p);
                    (min.min(&p_min), max.max(&p_max))
                })
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

            Some(ConnByteArray(_)) => {
                let mut p_count_1 = self._new();
                p_count_1.exprs = 1;

                (p_count_1.clone(), p_count_1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use models::rust::utils::{
        new_boundvar_expr, new_freevar_expr, new_gint_expr, new_wildcard_expr, no_frees,
    };

    use super::*;

    #[test]
    fn borrowed_count_matches_filtered_pattern() {
        let par = Par {
            exprs: vec![
                new_wildcard_expr(),
                new_freevar_expr(3),
                new_boundvar_expr(2),
                new_gint_expr(7),
            ],
            ..Par::default()
        };

        assert_eq!(
            ParCount::without_frees(&par),
            ParCount::new(&no_frees(&par))
        );
        assert_eq!(ParCount::without_frees(&par).exprs, 2);
    }

    #[test]
    fn nested_connective_bounds_keep_fixed_and_flexible_alternatives() {
        let fixed = Par {
            exprs: vec![new_gint_expr(1), new_gint_expr(2)],
            ..Par::default()
        };
        let flexible = Par {
            exprs: vec![new_wildcard_expr()],
            ..Par::default()
        };
        let count = ParCount::new(&Par::default());
        let and = Connective {
            connective_instance: Some(ConnAndBody(ConnectiveBody {
                ps: vec![fixed.clone(), flexible.clone()],
            })),
        };
        let or = Connective {
            connective_instance: Some(ConnOrBody(ConnectiveBody {
                ps: vec![fixed, flexible],
            })),
        };

        let (and_min, and_max) = count.min_max_con(&and);
        assert_eq!((and_min.exprs, and_max.exprs), (2, 2));
        let (or_min, or_max) = count.min_max_con(&or);
        assert_eq!((or_min.exprs, or_max.exprs), (0, 1000));

        let nested = Par {
            exprs: vec![new_gint_expr(3)],
            connectives: vec![and],
            ..Par::default()
        };
        let (nested_min, nested_max) = count.min_max_par(&nested);
        assert_eq!((nested_min.exprs, nested_max.exprs), (3, 3));
    }
}
