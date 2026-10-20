//! Public spatial-matcher surface.
//!
//! Every entry delegates to the single heterogeneous PDA in
//! `spatial_matcher_pda`. Keeping the trait surface preserves the interpreter's
//! API while preventing calls through a different message type from silently
//! reintroducing mutual native-stack recursion.

use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{guard, new_free_map, FreeMap, IsolatableState};

use super::exports::*;
use super::has_locally_free::HasLocallyFree;
use super::list_match::{aggregate_updates, ListMatch, Pattern};
use super::match_pars::match_pars;
use super::maximum_bipartite_match::MaximumBipartiteMatch;
use super::spatial_matcher_pda;
use crate::list_match;

list_match!(
    Par,
    (Par, Par),
    Send,
    Receive,
    New,
    Expr,
    Match,
    Bundle,
    GUnforgeable,
    ReceiveBind,
    If,
    CostSignedTerm,
    CostStack
);

pub trait SpatialMatcher<T, P> {
    fn spatial_match(&mut self, target: T, pattern: P) -> Option<()>;
}

#[derive(Clone)]
pub struct SpatialMatcherContext {
    pub free_map: FreeMap,
}

impl SpatialMatcherContext {
    pub fn new() -> Self {
        Self {
            free_map: new_free_map(),
        }
    }

    pub fn spatial_match_result(&mut self, target: Par, pattern: Par) -> Option<&FreeMap> {
        spatial_matcher_pda::match_par(self, target, pattern)?;
        Some(&self.free_map)
    }

    pub fn spatial_match_par_ref(&mut self, target: &Par, pattern: &Par) -> Option<()> {
        if !pattern.connective_used {
            guard(match_pars(target, pattern))
        } else {
            if let Some(level) = spatial_matcher_pda::free_variable_level_borrowed(pattern) {
                return spatial_matcher_pda::bind_free_variable_borrowed(self, target, level);
            }
            let clone_start = std::time::Instant::now();
            let target = target.clone();
            let pattern = pattern.clone();
            metrics::counter!(
                crate::rust::interpreter::metrics_constants::RHOLANG_MATCHER_FOLD_MATCH_TAIL_CLONE_NS_METRIC,
                "source" => crate::rust::interpreter::metrics_constants::RHOLANG_METRICS_SOURCE
            )
            .increment(clone_start.elapsed().as_nanos() as u64);
            spatial_matcher_pda::match_par(self, target, pattern)
        }
    }
}

impl Default for SpatialMatcherContext {
    fn default() -> Self { Self::new() }
}

impl IsolatableState for SpatialMatcherContext {
    fn free_map_mut(&mut self) -> &mut FreeMap { &mut self.free_map }
}

impl SpatialMatcher<Par, Par> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, pattern: Par) -> Option<()> {
        spatial_matcher_pda::match_par(self, target, pattern)
    }
}

impl SpatialMatcher<(Par, Par), (Par, Par)> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: (Par, Par), pattern: (Par, Par)) -> Option<()> {
        spatial_matcher_pda::match_par_pair(self, target, pattern)
    }
}

impl SpatialMatcher<Par, Connective> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        spatial_matcher_pda::match_connective(self, target, pattern)
    }
}

macro_rules! pda_impl {
    ($target:ty, $entry:ident) => {
        impl SpatialMatcher<$target, $target> for SpatialMatcherContext {
            fn spatial_match(&mut self, target: $target, pattern: $target) -> Option<()> {
                spatial_matcher_pda::$entry(self, target, pattern)
            }
        }
    };
}

impl SpatialMatcher<If, If> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: If, pattern: If) -> Option<()> {
        spatial_matcher_pda::match_if(self, target, pattern)
    }
}

impl SpatialMatcher<CostSignedTerm, CostSignedTerm> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: CostSignedTerm, pattern: CostSignedTerm) -> Option<()> {
        spatial_matcher_pda::match_cost_signed_term(self, target, pattern)
    }
}

impl SpatialMatcher<CostStack, CostStack> for SpatialMatcherContext {
    fn spatial_match(&mut self, target: CostStack, pattern: CostStack) -> Option<()> {
        spatial_matcher_pda::match_cost_stack(self, target, pattern)
    }
}

pda_impl!(Bundle, match_bundle);
pda_impl!(Send, match_send);
pda_impl!(Receive, match_receive);
pda_impl!(New, match_new);
pda_impl!(Expr, match_expr);
pda_impl!(Match, match_match);
pda_impl!(GUnforgeable, match_unforgeable);
pda_impl!(ReceiveBind, match_receive_bind);
pda_impl!(MatchCase, match_case);

pub(super) fn merge_set_remainder(p: &mut Par, r: Vec<Par>) {
    let mut unique = Vec::with_capacity(r.len());
    for element in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&element)?;
        // work.reserve_inspect(&unique)?;
        if !unique.contains(&element) {
            unique.push(element);
        }
    }
    p.exprs = vec![Expr {
        expr_instance: Some(ESetBody(ESet {
            ps: unique,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }];
}

pub(super) fn merge_map_remainder(p: &mut Par, r: Vec<(Par, Par)>) {
    let mut unique = Vec::with_capacity(r.len());
    for (key, value) in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&key)?;
        // work.reserve_inspect(&unique)?;
        if let Some(index) = unique
            .iter()
            .position(|(existing, _): &(Par, Par)| existing == &key)
        {
            unique[index].1 = value;
        } else {
            unique.push((key, value));
        }
    }
    let mut kvs = Vec::with_capacity(unique.len());
    for (key, value) in unique {
        kvs.push(KeyValuePair {
            key: Some(key),
            value: Some(value),
        });
    }
    p.exprs = vec![Expr {
        expr_instance: Some(EMapBody(EMap {
            kvs,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }];
}
