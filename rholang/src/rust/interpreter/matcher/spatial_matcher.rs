//! Public spatial-matcher surface.
//!
//! Every entry delegates to the single heterogeneous PDA in
//! `spatial_matcher_pda`. Keeping the trait surface preserves the interpreter's
//! API while preventing calls through a different message type from silently
//! reintroducing mutual native-stack recursion.

use std::sync::{Arc, Mutex};

use models::rust::rholang::implicits::vector_par;
use models::rust::utils::{guard, new_free_map, FreeMap, IsolatableState};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use shared::rust::clone_backing::{
    self, arc_allocation_bytes, BackingError, BackingMeter, CloneBacking,
};
use shared::rust::collection_backing::{
    tree_insert_moves, tree_insert_transient_backing, tree_search_bound,
};

use super::exports::*;
use super::has_locally_free::HasLocallyFree;
use super::list_match::{aggregate_updates_metered, ListMatch, Pattern};
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
    ReceiveBind,
    If,
    CostSignedTerm,
    CostStack
);

pub trait SpatialMatcher<T, P> {
    fn spatial_match(&mut self, target: T, pattern: P) -> Option<()>;
}

#[derive(Clone)]
pub(super) struct MatcherWork<'a> {
    meter: Option<&'a dyn SourceMeter>,
    error: Option<Arc<Mutex<Option<RSpaceError>>>>,
}

impl<'a> MatcherWork<'a> {
    fn new() -> Self {
        Self {
            meter: None,
            error: None,
        }
    }

    fn with_meter(meter: &'a dyn SourceMeter) -> Result<Self, RSpaceError> {
        let bytes = arc_allocation_bytes::<Mutex<Option<RSpaceError>>>()
            .ok_or(RSpaceError::HostWorkRejected)?;
        meter.reserve(1, 0, bytes)?;
        Ok(Self {
            meter: Some(meter),
            error: Some(Arc::new(Mutex::new(None))),
        })
    }

    pub fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Option<()> {
        if let Some(error) = &self.error {
            if error.lock().expect("matcher meter lock").is_some() {
                return None;
            }
        }
        if let Some(meter) = self.meter {
            if let Err(error) = meter.reserve(operations, scanned, backing) {
                *self
                    .error
                    .as_ref()
                    .expect("meter error slot")
                    .lock()
                    .expect("matcher meter lock") = Some(error);
                return None;
            }
        }
        Some(())
    }

    pub fn take_error(&self) -> Option<RSpaceError> {
        self.error
            .as_ref()?
            .lock()
            .expect("matcher meter lock")
            .take()
    }

    pub fn reject<T>(&self, error: RSpaceError) -> Option<T> {
        if let Some(slot) = &self.error {
            let mut failed = slot.lock().expect("matcher meter lock");
            if failed.is_none() {
                *failed = Some(error);
            }
        }
        None
    }

    pub fn reserve_vec<T>(&self, values: &mut Vec<T>, additional: usize) -> Option<()> {
        let needed = match values.len().checked_add(additional) {
            Some(needed) => needed,
            None => return self.reject(RSpaceError::HostWorkRejected),
        };
        if needed <= values.capacity() {
            return self.reserve(1, 0, 0);
        }
        let Some(next_capacity) = values
            .capacity()
            .checked_mul(2)
            .map(|doubled| needed.max(doubled).max(4))
        else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(bytes) = next_capacity.checked_mul(std::mem::size_of::<T>()) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(scanned) = values.len().checked_mul(std::mem::size_of::<T>()) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(1, scanned, bytes)?;
        if values
            .try_reserve_exact(next_capacity - values.len())
            .is_err()
        {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    fn reserve_backing<T: CloneBacking>(&self, value: &T, inspect: bool) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let meter = |operations, scanned, backing| {
            self.reserve(operations, scanned, backing)
                .ok_or(BackingError::Rejected)
        };
        let result = if inspect {
            clone_backing::inspect(value, &meter)
        } else {
            clone_backing::reserve_copy_and_cleanup(value, &meter)
        };
        if result.is_err() {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, false)
    }

    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, true)
    }

    pub fn reserve_slice<T: CloneBacking>(&self, values: &[T]) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let meter = |operations, scanned, backing| {
            self.reserve(operations, scanned, backing)
                .ok_or(BackingError::Rejected)
        };
        if clone_backing::reserve_slice_copy_and_cleanup(values, &meter).is_err() {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    /// Use the shared block walker for one complete traversal. This keeps
    /// matcher accounting on the same finite SourceMeter and records a
    /// refusal before any caller copies or mutates a value.
    fn block_walk(
        &self,
        walk: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>,
    ) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let meter = |operations, scanned, backing| {
            self.reserve(operations, scanned, backing)
                .ok_or(BackingError::Rejected)
        };
        if walk(&meter).is_err() {
            return self.reject(RSpaceError::HostWorkRejected);
        }
        Some(())
    }

    pub fn inspect_blocks<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.block_walk(|meter| clone_backing::inspect_blocks(value, meter))
    }

    pub fn reserve_blocks_copy_and_cleanup<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.block_walk(|meter| clone_backing::reserve_blocks_copy_and_cleanup(value, meter))
    }

    pub fn reserve_blocks_slice_copy_and_cleanup<T: CloneBacking>(
        &self,
        values: &[T],
    ) -> Option<()> {
        self.block_walk(|meter| clone_backing::reserve_blocks_slice_copy_and_cleanup(values, meter))
    }

    /// D-D1a (DR-103): one search of a free map with `entries` entries reads
    /// one root-to-leaf path of `i32` keys.
    pub fn reserve_free_map_search(&self, entries: usize) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let comparisons = tree_search_bound(entries);
        let Some(scanned) = comparisons.checked_mul(2 * std::mem::size_of::<i32>()) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(comparisons, scanned, 0)
    }

    /// D-D1a (DR-103): one insert into a free map with `entries` entries: its
    /// search, the slots, edges and parent links that it moves on each level,
    /// and the growth of the tree's nodes.
    pub fn reserve_free_map_insert(&self, entries: usize) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        self.reserve_free_map_search(entries)?;
        let Some(moves) = entries
            .checked_add(1)
            .and_then(tree_insert_moves::<i32, Par>)
        else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        // Final retained growth may be zero during a split. Prepay the
        // generic per-path transient node work and allocation bound instead.
        let Some((operations, growth)) = tree_insert_transient_backing::<i32, Par>(entries) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(operations) = operations.checked_add(1) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(operations, moves, growth)
    }
}

// D-D1a (DR-103): the per-level move bound of a free-map insert, as the Rocq
// model `FreeMapBindings.level_charge_value` and DR-103 state it.
#[cfg(target_pointer_width = "64")]
const _: () = assert!(
    12 * (std::mem::size_of::<i32>() + std::mem::size_of::<Par>())
        + 12 * 8
        + 12 * (8 + 2)
        + 4 * 2
        + 8
        == 3_832
);

#[derive(Clone)]
pub struct SpatialMatcherContext<'a> {
    pub free_map: FreeMap,
    work: MatcherWork<'a>,
}

impl<'a> SpatialMatcherContext<'a> {
    pub fn new() -> Self {
        Self {
            free_map: new_free_map(),
            work: MatcherWork::new(),
        }
    }

    pub fn with_meter(meter: &'a dyn SourceMeter) -> Result<Self, RSpaceError> {
        Ok(Self {
            free_map: new_free_map(),
            work: MatcherWork::with_meter(meter)?,
        })
    }

    pub fn reserve(&self, operations: usize, scanned: usize, backing: usize) -> Option<()> {
        self.work.reserve(operations, scanned, backing)
    }

    pub fn take_error(&self) -> Option<RSpaceError> { self.work.take_error() }

    pub fn reject<T>(&self, error: RSpaceError) -> Option<T> { self.work.reject(error) }

    pub fn reserve_vec<T>(&self, values: &mut Vec<T>, additional: usize) -> Option<()> {
        self.work.reserve_vec(values, additional)
    }

    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_clone(value)
    }

    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_inspect(value)
    }

    pub fn reserve_slice<T: CloneBacking>(&self, values: &[T]) -> Option<()> {
        self.work.reserve_slice(values)
    }

    pub fn inspect_blocks<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.inspect_blocks(value)
    }

    pub fn reserve_blocks_copy_and_cleanup<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_blocks_copy_and_cleanup(value)
    }

    pub fn reserve_blocks_slice_copy_and_cleanup<T: CloneBacking>(
        &self,
        values: &[T],
    ) -> Option<()> {
        self.work.reserve_blocks_slice_copy_and_cleanup(values)
    }

    pub(super) fn reserve_free_map_search(&self, entries: usize) -> Option<()> {
        self.work.reserve_free_map_search(entries)
    }

    pub(super) fn reserve_free_map_insert(&self, entries: usize) -> Option<()> {
        self.work.reserve_free_map_insert(entries)
    }

    pub(super) fn work(&self) -> Option<MatcherWork<'a>> {
        self.reserve(1, 0, 0)?;
        Some(self.work.clone())
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
            if let Some(level) = spatial_matcher_pda::free_variable_level_borrowed(pattern) {
                return spatial_matcher_pda::bind_free_variable_borrowed(self, target, level);
            }
            self.reserve_blocks_copy_and_cleanup(target)?;
            self.reserve_blocks_copy_and_cleanup(pattern)?;
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

impl<'a> Default for SpatialMatcherContext<'a> {
    fn default() -> Self { Self::new() }
}

impl<'a> IsolatableState for SpatialMatcherContext<'a> {
    fn free_map_mut(&mut self) -> &mut FreeMap { &mut self.free_map }
}

impl<'a> SpatialMatcher<Par, Par> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Par) -> Option<()> {
        spatial_matcher_pda::match_par(self, target, pattern)
    }
}

impl<'a> SpatialMatcher<(Par, Par), (Par, Par)> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: (Par, Par), pattern: (Par, Par)) -> Option<()> {
        spatial_matcher_pda::match_par_pair(self, target, pattern)
    }
}

impl<'a> SpatialMatcher<Par, Connective> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        spatial_matcher_pda::match_connective(self, target, pattern)
    }
}

macro_rules! pda_impl {
    ($target:ty, $entry:ident) => {
        impl<'a> SpatialMatcher<$target, $target> for SpatialMatcherContext<'a> {
            fn spatial_match(&mut self, target: $target, pattern: $target) -> Option<()> {
                spatial_matcher_pda::$entry(self, target, pattern)
            }
        }
    };
}

impl<'a> SpatialMatcher<If, If> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: If, pattern: If) -> Option<()> {
        spatial_matcher_pda::match_if(self, target, pattern)
    }
}

impl<'a> SpatialMatcher<CostSignedTerm, CostSignedTerm> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: CostSignedTerm, pattern: CostSignedTerm) -> Option<()> {
        spatial_matcher_pda::match_cost_signed_term(self, target, pattern)
    }
}

impl<'a> SpatialMatcher<CostStack, CostStack> for SpatialMatcherContext<'a> {
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
