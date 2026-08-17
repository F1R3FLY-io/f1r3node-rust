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
    ReceiveBind
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

    /// Borrowed fast path for the high-volume fold matcher. Concrete patterns
    /// remain comparison-only; a binding pattern is cloned once into the
    /// owned PDA, where the target copy is also the value a successful free
    /// variable capture retains.
    pub fn spatial_match_par_ref(&mut self, target: &Par, pattern: &Par) -> Option<()> {
        if !pattern.connective_used {
            guard(match_pars(target, pattern))
        } else {
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

pda_impl!(Bundle, match_bundle);
pda_impl!(Send, match_send);
pda_impl!(Receive, match_receive);
pda_impl!(New, match_new);
pda_impl!(Expr, match_expr);
pda_impl!(Match, match_match);
pda_impl!(GUnforgeable, match_unforgeable);
pda_impl!(ReceiveBind, match_receive_bind);
pda_impl!(MatchCase, match_case);
