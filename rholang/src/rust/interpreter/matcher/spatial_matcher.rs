// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - trait SpatialMatcher

use std::sync::{Arc, Mutex};

use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::SourceMeter;
use shared::rust::clone_backing::{
    self, arc_allocation_bytes, BackingError, BackingMeter, CloneBacking,
};
use shared::rust::collection_backing::{tree_growth, tree_insert_moves, tree_search_bound};

use super::exports::*;
use super::fold_match::FoldMatch;
// Changed by D-D2 (D-M8, DR-104): the matcher tests its values by reference.
// use super::has_locally_free::HasLocallyFree;
use super::has_locally_free::HasLocallyFreeRef;
use super::list_match::{aggregate_updates, ListMatch, Pattern};
use super::match_pars::match_pars;
use super::par_count::ParCount;
use super::sub_pars::sub_pars;
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
    meter: Option<&'a (dyn SourceMeter + std::marker::Send + std::marker::Sync)>,
    error: Option<Arc<Mutex<Option<RSpaceError>>>>,
}

impl<'a> MatcherWork<'a> {
    fn new() -> Self {
        Self {
            meter: None,
            error: None,
        }
    }

    fn with_meter(
        meter: &'a (dyn SourceMeter + std::marker::Send + std::marker::Sync),
    ) -> Result<Self, RSpaceError> {
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

    // D-E2 (DR-109): the matcher sites use the block walks below. The
    // per-level helpers remain only as the reference charges of the tests,
    // so they compile only for tests.
    #[cfg(test)]
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

    #[cfg(test)]
    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, false)
    }

    #[cfg(test)]
    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.reserve_backing(value, true)
    }

    #[cfg(test)]
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

    /// D-E2 (DR-109): runs one block walk of the shared walker (DR-92) with
    /// the matcher's meter. Without a meter it walks nothing, as the
    /// per-level helpers did, and a recorded error still stops the match. A
    /// rejected reservation records the meter's error.
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

    /// D-E2 (DR-109): one linear traversal of `value`.
    pub fn inspect_blocks<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.block_walk(|meter| clone_backing::inspect_blocks(value, meter))
    }

    /// D-E2 (DR-109): one clone of `value` and the release of the clone.
    pub fn reserve_blocks_copy_and_cleanup<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.block_walk(|meter| clone_backing::reserve_blocks_copy_and_cleanup(value, meter))
    }

    /// D-E2 (DR-109): the copy of a slice into a new vector, and the release
    /// of the copy.
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
        let Some((operations, growth)) = tree_growth::<i32, Par>(entries, 1) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        let Some(operations) = operations.checked_add(1) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(operations, moves, growth)
    }

    /// D-D2 (DR-104): `retain_no_frees` on `entries` expressions. The
    /// predicate reads at most three words of each expression. Each
    /// expression is then kept in place, moved, which reads and writes it, or
    /// dropped in place, which reads at most three words
    /// (`MatcherReadsByReference.retain_work_within_charge`).
    pub fn reserve_retain_no_frees(&self, entries: usize) -> Option<()> {
        if self.meter.is_none() {
            return self.reserve(0, 0, 0);
        }
        let per_entry = std::mem::size_of::<Expr>()
            .checked_mul(2)
            .and_then(|bytes| bytes.checked_add(3 * std::mem::size_of::<usize>()));
        let (Some(operations), Some(scanned)) = (
            entries.checked_mul(2),
            per_entry.and_then(|bytes| bytes.checked_mul(entries)),
        ) else {
            return self.reject(RSpaceError::HostWorkRejected);
        };
        self.reserve(operations, scanned, 0)
    }

    /// D-D3 (DR-105): the free-variable test reads the lengths of the eleven
    /// other lists of a pattern, the pointer to its expressions and one `Expr`.
    pub fn reserve_free_variable_test(&self) -> Option<()> {
        self.reserve(
            12,
            12 * std::mem::size_of::<usize>() + std::mem::size_of::<Expr>(),
            0,
        )
    }
}

/// D-D3 (DR-105): the level of a pattern that is exactly one free variable. The
/// pattern uses connectives, its only expression is `EVar(FreeVar(level))`,
/// and its other lists, its connectives and its cost terms are empty. The
/// pattern's `locally_free` is not read.
pub(super) fn free_variable_level(pattern: &Par) -> Option<i32> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used,
        cost_signed_terms,
        cost_stacks,
    } = pattern;
    if !*connective_used
        || !sends.is_empty()
        || !receives.is_empty()
        || !news.is_empty()
        || !matches.is_empty()
        || !unforgeables.is_empty()
        || !bundles.is_empty()
        || !connectives.is_empty()
        || !conditionals.is_empty()
        || !cost_signed_terms.is_empty()
        || !cost_stacks.is_empty()
    {
        return None;
    }
    match exprs.as_slice() {
        [Expr {
            expr_instance:
                Some(EVarBody(EVar {
                    v:
                        Some(Var {
                            var_instance: Some(FreeVar(level)),
                        }),
                })),
        }] => Some(*level),
        _ => None,
    }
}

#[cfg(test)]
thread_local! {
    /// D-D3 (DR-105): while the flag is set, matches on this thread take the
    /// general path for a free-variable pattern, for the equality tests.
    pub(super) static LEGACY_FREE_VARIABLE_PATH: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn free_variable_fast_path() -> bool {
    #[cfg(test)]
    {
        !LEGACY_FREE_VARIABLE_PATH.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        true
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

/// D-D1b (DR-103): merges the remainder of a set pattern into its binding in
/// place. Every reservation comes before the one assignment, so a rejected
/// merge leaves the binding unchanged (`FreeMapBindings.rejected_merge_leaves_map`).
pub(super) fn merge_set_remainder(p: &mut Par, r: Vec<Par>, work: &MatcherWork<'_>) -> Option<()> {
    let mut unique = Vec::new();
    for element in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&element)?;
        // work.reserve_inspect(&unique)?;
        work.inspect_blocks(&element)?;
        work.inspect_blocks(&unique)?;
        // Added by D-E2 (DR-109): `contains` reads the element once for each
        // item, in lockstep with the item, so it reads no more of the element
        // than of the items. A second inspection of `unique` pays that side
        // (`MatcherReadsByReference.two_container_inspections_cover_membership_scan`).
        work.inspect_blocks(&unique)?;
        if !unique.contains(&element) {
            work.reserve_vec(&mut unique, 1)?;
            unique.push(element);
        }
    }
    let mut exprs = Vec::new();
    work.reserve_vec(&mut exprs, 1)?;
    exprs.push(Expr {
        expr_instance: Some(ESetBody(ESet {
            ps: unique,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    });
    p.exprs = exprs;
    Some(())
}

/// D-D1b (DR-103): merges the remainder of a map pattern into its binding in
/// place. Every reservation comes before the one assignment, so a rejected
/// merge leaves the binding unchanged (`FreeMapBindings.rejected_merge_leaves_map`).
pub(super) fn merge_map_remainder(
    p: &mut Par,
    r: Vec<(Par, Par)>,
    work: &MatcherWork<'_>,
) -> Option<()> {
    let mut unique = Vec::new();
    for (key, value) in r {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // work.reserve_inspect(&key)?;
        // work.reserve_inspect(&unique)?;
        work.inspect_blocks(&key)?;
        work.inspect_blocks(&unique)?;
        // Added by D-E2 (DR-109): `position` reads the key once for each
        // entry, in lockstep with the entry's key, so it reads no more of the
        // key than of the entries. A second inspection of `unique` pays that
        // side (`MatcherReadsByReference.two_container_inspections_cover_membership_scan`).
        work.inspect_blocks(&unique)?;
        if let Some(index) = unique
            .iter()
            .position(|(existing, _): &(Par, Par)| existing == &key)
        {
            unique[index].1 = value;
        } else {
            work.reserve_vec(&mut unique, 1)?;
            unique.push((key, value));
        }
    }
    let mut kvs = Vec::new();
    work.reserve_vec(&mut kvs, unique.len())?;
    for (key, value) in unique {
        kvs.push(KeyValuePair {
            key: Some(key),
            value: Some(value),
        });
    }
    let mut exprs = Vec::new();
    work.reserve_vec(&mut exprs, 1)?;
    exprs.push(Expr {
        expr_instance: Some(EMapBody(EMap {
            kvs,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    });
    p.exprs = exprs;
    Some(())
}

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

    pub fn with_meter(
        meter: &'a (dyn SourceMeter + std::marker::Send + std::marker::Sync),
    ) -> Result<Self, RSpaceError> {
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

    // D-E2 (DR-109): the per-level delegations remain only as the reference
    // charges of the tests.
    #[cfg(test)]
    pub fn reserve_clone<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_clone(value)
    }

    #[cfg(test)]
    pub fn reserve_inspect<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_inspect(value)
    }

    #[cfg(test)]
    pub fn reserve_slice<T: CloneBacking>(&self, values: &[T]) -> Option<()> {
        self.work.reserve_slice(values)
    }

    /// D-E2 (DR-109): one linear traversal of `value`.
    pub fn inspect_blocks<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.inspect_blocks(value)
    }

    /// D-E2 (DR-109): one clone of `value` and the release of the clone.
    pub fn reserve_blocks_copy_and_cleanup<T: CloneBacking>(&self, value: &T) -> Option<()> {
        self.work.reserve_blocks_copy_and_cleanup(value)
    }

    /// D-E2 (DR-109): the copy of a slice into a new vector, and the release
    /// of the copy.
    pub fn reserve_blocks_slice_copy_and_cleanup<T: CloneBacking>(
        &self,
        values: &[T],
    ) -> Option<()> {
        self.work.reserve_blocks_slice_copy_and_cleanup(values)
    }

    /// D-E2 (DR-109): `single_expr` clones the only expression of a target
    /// that has no sends, receives, news, matches or bundles, and drops the
    /// clone. This reserves that copy and its release, under the same
    /// condition. The inspection of the target pays the reads of the
    /// condition.
    pub(super) fn reserve_single_expr_copy(&self, target: &Par) -> Option<()> {
        if target.sends.is_empty()
            && target.receives.is_empty()
            && target.news.is_empty()
            && target.matches.is_empty()
            && target.bundles.is_empty()
        {
            if let [expr] = target.exprs.as_slice() {
                self.reserve_blocks_copy_and_cleanup(expr)?;
            }
        }
        Some(())
    }

    pub(super) fn reserve_free_map_search(&self, entries: usize) -> Option<()> {
        self.work.reserve_free_map_search(entries)
    }

    pub(super) fn reserve_free_map_insert(&self, entries: usize) -> Option<()> {
        self.work.reserve_free_map_insert(entries)
    }

    pub(super) fn reserve_retain_no_frees(&self, entries: usize) -> Option<()> {
        self.work.reserve_retain_no_frees(entries)
    }

    /// D-D2 (DR-104): the read of one `connective_used` flag before any
    /// inspection of the value that holds it.
    pub(super) fn reserve_flag_read(&self) -> Option<()> { self.reserve(1, 8, 0) }

    /// D-D2 (DR-104): compares a target with a ground pattern by reference.
    /// `match_pars` reads both sides in lockstep, so it reads no more of the
    /// target than of the pattern, and two inspections of the pattern cover
    /// both sides (`MatcherReadsByReference.two_pattern_inspections_cover_lockstep_reads`).
    pub(super) fn match_ground_par(&self, target: &Par, pattern: &Par) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(pattern)?;
        // self.reserve_inspect(pattern)?;
        self.inspect_blocks(pattern)?;
        self.inspect_blocks(pattern)?;
        guard(match_pars(target, pattern))
    }

    pub(super) fn reserve_free_variable_test(&self) -> Option<()> {
        self.work.reserve_free_variable_test()
    }

    pub(super) fn free_variable_fast_path(&self) -> bool { free_variable_fast_path() }

    /// D-D3 (DR-105): binds an owned target to a free-variable pattern. It
    /// makes the ten `list_match_single_` calls of the Par/Par matcher, in the
    /// same order and with the same empty pattern lists, so the result and the
    /// free map equal those of the general path, also on failure
    /// (`FreeVariableFastPath.fast_path_equals_general`).
    pub(super) fn bind_free_variable(&mut self, target: Par, level: i32) -> Option<()> {
        let remainder = Some(level);
        self.list_match_single_(
            target.sends,
            Vec::new(),
            &|p, s, _| {
                p.sends = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.receives,
            Vec::new(),
            &|p, s, _| {
                p.receives = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.news,
            Vec::new(),
            &|p, s, _| {
                p.news = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.exprs,
            Vec::new(),
            &|p, s, _| {
                p.exprs = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.matches,
            Vec::new(),
            &|p, s, _| {
                p.matches = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.bundles,
            Vec::new(),
            &|p, s, _| {
                p.bundles = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.unforgeables,
            Vec::new(),
            &|p, s, _| {
                p.unforgeables = s;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.conditionals,
            Vec::new(),
            &|p, values, _| {
                p.conditionals = values;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.cost_signed_terms,
            Vec::new(),
            &|p, values, _| {
                p.cost_signed_terms = values;
                Some(())
            },
            remainder,
            false,
        )?;
        self.list_match_single_(
            target.cost_stacks,
            Vec::new(),
            &|p, values, _| {
                p.cost_stacks = values;
                Some(())
            },
            remainder,
            false,
        )
    }

    /// D-D3 (DR-105): binds a borrowed target to a free-variable pattern. Each
    /// field is checked as `list_match_single_` checks it, then copied, with its
    /// copy and cleanup reserved, and merged by `handle_remainder`, in the order
    /// of the Par/Par matcher.
    pub(super) fn bind_free_variable_by_reference(
        &mut self,
        target: &Par,
        level: i32,
    ) -> Option<()> {
        self.bind_field_by_reference(&target.sends, level, &|p, s, _| {
            p.sends = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.receives, level, &|p, s, _| {
            p.receives = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.news, level, &|p, s, _| {
            p.news = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.exprs, level, &|p, s, _| {
            p.exprs = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.matches, level, &|p, s, _| {
            p.matches = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.bundles, level, &|p, s, _| {
            p.bundles = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.unforgeables, level, &|p, s, _| {
            p.unforgeables = s;
            Some(())
        })?;
        self.bind_field_by_reference(&target.conditionals, level, &|p, values, _| {
            p.conditionals = values;
            Some(())
        })?;
        self.bind_field_by_reference(&target.cost_signed_terms, level, &|p, values, _| {
            p.cost_signed_terms = values;
            Some(())
        })?;
        self.bind_field_by_reference(&target.cost_stacks, level, &|p, values, _| {
            p.cost_stacks = values;
            Some(())
        })
    }

    fn bind_field_by_reference<T: Clone + CloneBacking>(
        &mut self,
        field: &[T],
        level: i32,
        merger: &dyn Fn(&mut Par, Vec<T>, &MatcherWork<'_>) -> Option<()>,
    ) -> Option<()>
    where
        Self: ListMatch<T> + HasLocallyFreeRef<T>,
    {
        for element in field {
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(element)?;
            self.inspect_blocks(element)?;
            if !self.locally_free_is_empty(element, 0) {
                return None;
            }
        }
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_slice(field)?;
        self.reserve_blocks_slice_copy_and_cleanup(field)?;
        let copied = field.to_vec();
        self.handle_remainder(copied, level, merger)
    }

    pub(super) fn work(&self) -> Option<MatcherWork<'a>> {
        self.reserve(1, 0, 0)?;
        Some(self.work.clone())
    }

    pub fn spatial_match_result(&mut self, target: Par, pattern: Par) -> Option<&FreeMap> {
        let do_match = self.spatial_match(target, pattern);

        match do_match {
            Some(_) => Some(&self.free_map),
            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - forTuple
impl<'a> SpatialMatcher<(Par, Par), (Par, Par)> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: (Par, Par), pattern: (Par, Par)) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        self.spatial_match(target.0, pattern.0)
            .and_then(|_| self.spatial_match(target.1, pattern.1))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - connectiveMatcher
impl<'a> SpatialMatcher<Par, Connective> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Connective) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        // Added by D-E2 (DR-109): the scalar connectives test the target with
        // `single_expr`, which copies its only expression and drops the copy.
        // The per-level inspection of the target paid those reads, and no
        // charge paid the copy's backing.
        if matches!(
            pattern.connective_instance,
            Some(ConnBool(_) | ConnInt(_) | ConnString(_) | ConnUri(_) | ConnByteArray(_))
        ) {
            self.reserve_single_expr_copy(&target)?;
        }
        match pattern.connective_instance {
            Some(ConnAndBody(connective_body)) => {
                connective_body.ps.into_iter().try_fold((), |_, p| {
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // self.reserve_clone(&target)?;
                    self.reserve_blocks_copy_and_cleanup(&target)?;
                    let match_result = self.spatial_match(target.clone(), p);
                    match_result.map(|_| ())
                })
            }

            Some(ConnOrBody(connective_body)) => connective_body.ps.into_iter().find_map(|p| {
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&self.free_map)?;
                self.reserve_blocks_copy_and_cleanup(&self.free_map)?;
                let matches = self.free_map.clone();
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_clone(&target)?;
                self.reserve_blocks_copy_and_cleanup(&target)?;
                self.spatial_match(target.clone(), p)?;
                self.free_map = matches;
                Some(())
            }),

            Some(ConnNotBody(p)) => {
                // Check if there is a ConnOrBody inside the ConnNotBody
                let has_or_body = match &p {
                    Par { connectives, .. } => connectives
                        .iter()
                        .any(|c| matches!(c.connective_instance, Some(ConnOrBody(_)))),
                };

                if has_or_body {
                    // If there is a ConnOrBody inside, we need to handle it specially
                    let match_option = self.spatial_match(target, p);
                    match match_option {
                        Some(_) => None,  // If inner pattern matches, the negation fails
                        None => Some(()), // If inner pattern doesn't match, the negation succeeds
                    }
                } else {
                    // Regular negation handling
                    let match_option = self.spatial_match(target, p);
                    match match_option {
                        Some(_) => None,
                        None => Some(()),
                    }
                }
            }

            Some(VarRefBody(_)) => None,

            Some(ConnBool(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GBool(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnInt(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GInt(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnString(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GString(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnUri(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GUri(_)),
                }) => Some(()),
                _ => None,
            },

            Some(ConnByteArray(_)) => match single_expr(&target) {
                Some(Expr {
                    expr_instance: Some(GByteArray(_)),
                }) => Some(()),
                _ => None,
            },

            None => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - parSpatialMatcher
impl<'a> SpatialMatcher<Par, Par> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Par, pattern: Par) -> Option<()> {
        // Changed by D-D2 (D-M8, DR-104): a ground pattern charges the lockstep
        // bound of match_ground_par, not an inspection of the target. A pattern
        // with connectives still inspects both values.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        // if !pattern.connective_used {
        //     // guard(pattern == target)
        //     guard(match_pars(&target, &pattern))
        // } else {
        self.reserve_flag_read()?;
        if !pattern.connective_used {
            self.match_ground_par(&target, &pattern)
        } else {
            // Changed by D-D3 (D-M9, DR-105): the inspection of the pattern comes
            // first and prepays the free-variable test, so a free-variable pattern
            // binds the target's fields without an inspection of the target.
            // self.reserve_inspect(&target)?;
            // self.reserve_inspect(&pattern)?;
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(&pattern)?;
            self.inspect_blocks(&pattern)?;
            if self.free_variable_fast_path() {
                if let Some(level) = free_variable_level(&pattern) {
                    return self.bind_free_variable(target, level);
                }
            }
            // Changed by D-O1 (DR-109): block accounting charges inline bytes
            // once per enclosing block.
            // self.reserve_inspect(&target)?;
            self.inspect_blocks(&target)?;
            let var_level: Option<i32> = pattern.exprs.iter().find_map(|expr| match expr {
                Expr {
                    expr_instance:
                        Some(EVarBody(EVar {
                            v:
                                Some(Var {
                                    var_instance: Some(FreeVar(level)),
                                }),
                        })),
                } => Some(*level),
                _ => None,
            });

            let wildcard: bool = pattern
                .exprs
                .iter()
                .find_map(|expr| match expr {
                    Expr {
                        expr_instance:
                            Some(EVarBody(EVar {
                                v:
                                    Some(Var {
                                        var_instance: Some(Wildcard(_)),
                                    }),
                            })),
                    } => Some(()),
                    _ => None,
                })
                .is_some();

            let pc = ParCount::without_frees(&pattern);
            let min_rem = pc.clone();
            let max_rem = if wildcard || !var_level.is_none() {
                pc._max()
            } else {
                pc.clone()
            };

            let mut individual_bounds = Vec::new();
            for con in &pattern.connectives {
                // Changed by D-O1 (DR-109): block accounting charges inline bytes
                // once per enclosing block.
                // self.reserve_inspect(con)?;
                self.inspect_blocks(con)?;
                self.reserve_vec(&mut individual_bounds, 1)?;
                individual_bounds.push(pc.min_max_con(con));
            }

            let mut remainder_bounds = Vec::new();
            self.reserve_vec(&mut remainder_bounds, 1)?;
            remainder_bounds.push((min_rem, max_rem));
            for bounds in individual_bounds.iter().rev() {
                let last = remainder_bounds.last().unwrap();
                let next = (bounds.0.add(&last.0), bounds.1.add(&last.1));
                self.reserve_vec(&mut remainder_bounds, 1)?;
                remainder_bounds.push(next);
            }
            remainder_bounds.pop();
            remainder_bounds.reverse();

            fn match_connective_with_bounds(
                s: &mut SpatialMatcherContext<'_>,
                target: Par,
                labeled_connective: (&Connective, &(ParCount, ParCount), &(ParCount, ParCount)),
            ) -> Option<Par> {
                let (con, bounds, remainders) = labeled_connective;

                let subsets = sub_pars(
                    &target,
                    &bounds.0,
                    &bounds.1,
                    &remainders.0,
                    &remainders.1,
                    s.work()?,
                )?;
                for sp in subsets {
                    let sp = sp?;
                    // Changed by D-O1 (DR-109): block accounting charges inline bytes
                    // once per enclosing block.
                    // s.reserve_clone(con)?;
                    s.reserve_blocks_copy_and_cleanup(con)?;
                    if s.spatial_match(sp.0, con.clone()).is_some() {
                        return Some(sp.1);
                    }
                }
                None
            }

            let remainder = pattern
                .connectives
                .iter()
                .zip(individual_bounds.iter())
                .zip(remainder_bounds.iter())
                .try_fold(target, |acc, ((connective, bounds), remainders)| {
                    match_connective_with_bounds(self, acc, (connective, bounds, remainders))
                })?;

            self.list_match_single_(
                remainder.sends,
                pattern.sends,
                &|p, s, _| {
                    p.sends = s;
                    Some(())
                },
                var_level,
                wildcard,
            )
            .and_then(|_| {
                self.list_match_single_(
                    remainder.receives,
                    pattern.receives,
                    &|p, s, _| {
                        p.receives = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.news,
                    pattern.news,
                    &|p, s, _| {
                        p.news = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                // Changed by D-D2 (D-M8, DR-104): the owned pattern expressions are
                // filtered in place, without a copy of the slice.
                // self.reserve_slice(&pattern.exprs)?;
                let mut exprs = pattern.exprs;
                self.reserve_retain_no_frees(exprs.len())?;
                retain_no_frees(&mut exprs);
                self.list_match_single_(
                    remainder.exprs,
                    // no_frees_exprs(&pattern.exprs),
                    exprs,
                    &|p, s, _| {
                        p.exprs = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.matches,
                    pattern.matches,
                    &|p, s, _| {
                        p.matches = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.bundles,
                    pattern.bundles,
                    &|p, s, _| {
                        p.bundles = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.unforgeables,
                    pattern.unforgeables,
                    &|p, s, _| {
                        p.unforgeables = s;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.conditionals,
                    pattern.conditionals,
                    &|p, values, _| {
                        p.conditionals = values;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_signed_terms,
                    pattern.cost_signed_terms,
                    &|p, values, _| {
                        p.cost_signed_terms = values;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
            .and_then(|_| {
                self.list_match_single_(
                    remainder.cost_stacks,
                    pattern.cost_stacks,
                    &|p, values, _| {
                        p.cost_stacks = values;
                        Some(())
                    },
                    var_level,
                    wildcard,
                )
            })
        }
    }
}

impl<'a> SpatialMatcher<If, If> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: If, pattern: If) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target == pattern)
    }
}

impl<'a> SpatialMatcher<CostSignedTerm, CostSignedTerm> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: CostSignedTerm, pattern: CostSignedTerm) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target == pattern)
    }
}

impl<'a> SpatialMatcher<CostStack, CostStack> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: CostStack, pattern: CostStack) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target == pattern)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - bundleSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl<'a> SpatialMatcher<Bundle, Bundle> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Bundle, pattern: Bundle) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(pattern == target)
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - sendSpatialMatcherInstance
impl<'a> SpatialMatcher<Send, Send> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Send, pattern: Send) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        let result = guard(target.persistent == pattern.persistent)
            .and_then(|_| self.spatial_match(target.chan.unwrap(), pattern.chan.unwrap()))
            .and_then(|_| self.fold_match(&target.data, &pattern.data, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveSpatialMatcherInstance
impl<'a> SpatialMatcher<Receive, Receive> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Receive, pattern: Receive) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target.persistent == pattern.persistent)
            .and_then(|_| self.list_match_single(target.binds, pattern.binds))
            .and_then(|_| self.spatial_match(target.body.unwrap(), pattern.body.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - newSpatialMatcherInstance
impl<'a> SpatialMatcher<New, New> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: New, pattern: New) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target.bind_count == pattern.bind_count)
            .and_then(|_| self.spatial_match(target.p.unwrap(), pattern.p.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - exprSpatialMatcherInstance
impl<'a> SpatialMatcher<Expr, Expr> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Expr, pattern: Expr) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        match (target.expr_instance, pattern.expr_instance) {
            (
                Some(EListBody(EList {
                    ps: tlist,
                    locally_free: _,
                    connective_used: _,
                    remainder: _,
                })),
                Some(EListBody(EList {
                    ps: plist,
                    locally_free: _,
                    connective_used: _,
                    remainder: rem,
                })),
            ) => {
                let free_level = match &rem {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let matched_rem = self.fold_match(&tlist, &plist, rem)?;

                match free_level {
                    Some(level) => {
                        // Changed by D-D1a (D-M2, DR-103): whole-tree backing on every
                        // insert, the pattern DR-77 replaced; the insert now charges its
                        // search, its moves and the growth of the tree.
                        // let Some(entries) = self.free_map.len().checked_add(1) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some((operations, bytes)) =
                        //     shared::rust::collection_backing::tree_backing::<i32, Par>(entries)
                        // else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some(backing) = bytes.checked_add(std::mem::size_of::<Expr>()) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // let Some(operations) = operations.checked_add(2) else {
                        //     return self.reject(RSpaceError::HostWorkRejected);
                        // };
                        // self.reserve(operations, bytes, backing)?;
                        self.reserve(2, 0, std::mem::size_of::<Expr>())?;
                        self.reserve_free_map_insert(self.free_map.len())?;
                        self.free_map.insert(
                            level,
                            new_elist_par(matched_rem, Vec::new(), false, None, Vec::new(), false),
                        );
                        Some(())
                    }

                    _ => Some(()),
                }
            }

            (
                Some(ETupleBody(ETuple {
                    ps: tlist,
                    locally_free: _,
                    connective_used: _,
                })),
                Some(ETupleBody(ETuple {
                    ps: plist,
                    locally_free: _,
                    connective_used: _,
                })),
            ) => self.fold_match(&tlist, &plist, None).map(|_| ()),

            (Some(ESetBody(t_set)), Some(ESetBody(p_set))) => {
                let is_wildcard = match p_set.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match p_set.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let work = self.work()?;
                let meter = |operations, scanned, backing| {
                    work.reserve(operations, scanned, backing)
                        .ok_or(BackingError::Rejected)
                };
                let tlist = match ParSetTypeMapper::eset_to_par_set_metered(t_set, &meter) {
                    Ok(set) => set.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };
                let plist = match ParSetTypeMapper::eset_to_par_set_metered(p_set, &meter) {
                    Ok(set) => set.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };

                // Changed by D-D1b (D-M2, DR-103): the merger takes the binding by
                // reference and merges it in place (merge_set_remainder).
                // let merger = |mut p: Par, r: Vec<Par>, work: &MatcherWork<'_>| {
                //     let mut unique = Vec::new();
                //     for element in r {
                //         work.reserve_inspect(&element)?;
                //         work.reserve_inspect(&unique)?;
                //         if !unique.contains(&element) {
                //             work.reserve_vec(&mut unique, 1)?;
                //             unique.push(element);
                //         }
                //     }
                //     let mut exprs = Vec::new();
                //     work.reserve_vec(&mut exprs, 1)?;
                //     exprs.push(Expr {
                //         expr_instance: Some(ESetBody(ESet {
                //             ps: unique,
                //             locally_free: Vec::new(),
                //             connective_used: false,
                //             remainder: None,
                //         })),
                //     });
                //     p.exprs = exprs;
                //     Some(p)
                // };

                self.list_match_single_(
                    tlist.sorted_pars,
                    plist.sorted_pars,
                    &merge_set_remainder,
                    remainder_var_opt,
                    is_wildcard,
                )
            }

            (Some(EMapBody(t_emap)), Some(EMapBody(p_emap))) => {
                let is_wildcard = match p_emap.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(Wildcard(_)),
                    }) => true,
                    _ => false,
                };

                let remainder_var_opt = match p_emap.remainder.as_ref() {
                    Some(Var {
                        var_instance: Some(FreeVar(level)),
                    }) => Some(*level),
                    _ => None,
                };
                let work = self.work()?;
                let meter = |operations, scanned, backing| {
                    work.reserve(operations, scanned, backing)
                        .ok_or(BackingError::Rejected)
                };
                let tlist = match ParMapTypeMapper::emap_to_par_map_metered(t_emap, &meter) {
                    Ok(map) => map.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };
                let plist = match ParMapTypeMapper::emap_to_par_map_metered(p_emap, &meter) {
                    Ok(map) => map.ps,
                    Err(_) => return self.reject(RSpaceError::HostWorkRejected),
                };

                // Changed by D-D1b (D-M2, DR-103): the merger takes the binding by
                // reference and merges it in place (merge_map_remainder).
                // let merger = |mut p: Par, r: Vec<(Par, Par)>, work: &MatcherWork<'_>| {
                //     let mut unique = Vec::new();
                //     for (key, value) in r {
                //         work.reserve_inspect(&key)?;
                //         work.reserve_inspect(&unique)?;
                //         if let Some(index) = unique
                //             .iter()
                //             .position(|(existing, _): &(Par, Par)| existing == &key)
                //         {
                //             unique[index].1 = value;
                //         } else {
                //             work.reserve_vec(&mut unique, 1)?;
                //             unique.push((key, value));
                //         }
                //     }
                //     let mut kvs = Vec::new();
                //     work.reserve_vec(&mut kvs, unique.len())?;
                //     for (key, value) in unique {
                //         kvs.push(KeyValuePair {
                //             key: Some(key),
                //             value: Some(value),
                //         });
                //     }
                //     let mut exprs = Vec::new();
                //     work.reserve_vec(&mut exprs, 1)?;
                //     exprs.push(Expr {
                //         expr_instance: Some(EMapBody(EMap {
                //             kvs,
                //             locally_free: Vec::new(),
                //             connective_used: false,
                //             remainder: None,
                //         })),
                //     });
                //     p.exprs = exprs;
                //     Some(p)
                // };

                self.list_match_single_(
                    tlist.sorted_list,
                    plist.sorted_list,
                    &merge_map_remainder,
                    remainder_var_opt,
                    is_wildcard,
                )
            }

            (Some(EVarBody(EVar { v: vp })), Some(EVarBody(EVar { v: vt }))) => guard(vp == vt),

            (Some(ENotBody(ENot { p: t })), Some(ENotBody(ENot { p }))) => {
                self.spatial_match(t.unwrap(), p.unwrap())
            }

            (Some(ENegBody(ENeg { p: t })), Some(ENegBody(ENeg { p }))) => {
                self.spatial_match(t.unwrap(), p.unwrap())
            }

            (Some(EMultBody(EMult { p1: t1, p2: t2 })), Some(EMultBody(EMult { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EDivBody(EDiv { p1: t1, p2: t2 })), Some(EDivBody(EDiv { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EModBody(EMod { p1: t1, p2: t2 })), Some(EModBody(EMod { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EPercentPercentBody(EPercentPercent { p1: t1, p2: t2 })),
                Some(EPercentPercentBody(EPercentPercent { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (Some(EPlusBody(EPlus { p1: t1, p2: t2 })), Some(EPlusBody(EPlus { p1, p2 }))) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EPlusPlusBody(EPlusPlus { p1: t1, p2: t2 })),
                Some(EPlusPlusBody(EPlusPlus { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            (
                Some(EMinusMinusBody(EMinusMinus { p1: t1, p2: t2 })),
                Some(EMinusMinusBody(EMinusMinus { p1, p2 })),
            ) => self
                .spatial_match(t1.unwrap(), p1.unwrap())
                .and_then(|_| self.spatial_match(t2.unwrap(), p2.unwrap())),

            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchSpatialMatcherInstance
impl<'a> SpatialMatcher<Match, Match> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: Match, pattern: Match) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        let result = self
            .spatial_match(target.target.unwrap(), pattern.target.unwrap())
            .and_then(|_| self.fold_match(&target.cases, &pattern.cases, None));

        result.map(|_| ())
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - unfSpatialMatcherInstance
// Apparently this code is never reached according to Scala code comment
impl<'a> SpatialMatcher<GUnforgeable, GUnforgeable> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: GUnforgeable, pattern: GUnforgeable) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        match (target.unf_instance, pattern.unf_instance) {
            (Some(GPrivateBody(t)), Some(GPrivateBody(p))) => guard(t == p),
            (Some(GDeployerIdBody(t)), Some(GDeployerIdBody(p))) => guard(t == p),
            _ => None,
        }
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - receiveBindSpatialMatcherInstance
impl<'a> SpatialMatcher<ReceiveBind, ReceiveBind> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: ReceiveBind, pattern: ReceiveBind) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target.patterns == pattern.patterns)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// See rholang/src/main/scala/coop/rchain/rholang/interpreter/matcher/SpatialMatcher.scala - matchCaseSpatialMatcherInstance
impl<'a> SpatialMatcher<MatchCase, MatchCase> for SpatialMatcherContext<'a> {
    fn spatial_match(&mut self, target: MatchCase, pattern: MatchCase) -> Option<()> {
        // Changed by D-O1 (DR-109): block accounting charges inline bytes
        // once per enclosing block.
        // self.reserve_inspect(&target)?;
        // self.reserve_inspect(&pattern)?;
        self.inspect_blocks(&target)?;
        self.inspect_blocks(&pattern)?;
        guard(target.pattern == pattern.pattern)
            .and_then(|_| self.spatial_match(target.source.unwrap(), pattern.source.unwrap()))
    }
}

// This implementation for type 'KeyValuePair' is NOT on the Scala side
// Somewhere, somehow, on Scala side they are are just calling this logic
// Could be related to ParMap. See RhoTypes.proto and how they set custom types for fields
// impl SpatialMatcher<KeyValuePair, KeyValuePair> for SpatialMatcherContext {
//     fn spatial_match(&mut self, target: KeyValuePair, pattern: KeyValuePair) -> Option<()> {
//         self.spatial_match(target.key.unwrap(), pattern.key.unwrap())
//             .and_then(|_| self.spatial_match(target.value.unwrap(), pattern.value.unwrap()))
//     }
// }

#[cfg(test)]
mod metered_tests {
    use super::*;

    #[test]
    fn list_remainder_rejects_before_free_map_publication() {
        let target = new_elist_expr(Vec::new(), Vec::new(), false, None);
        let pattern = new_elist_expr(Vec::new(), Vec::new(), false, Some(new_freevar_var(0)));
        let used = Mutex::new(0usize);
        let unlimited = |operations: usize, _: usize, _: usize| {
            *used.lock().unwrap() += operations;
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
        assert!(context
            .spatial_match(target.clone(), pattern.clone())
            .is_some());
        assert!(context.free_map.contains_key(&0));
        let limit = *used.lock().unwrap() - 1;
        let spent = Mutex::new(0usize);
        let meter = |operations: usize, _: usize, _: usize| {
            let mut current = spent.lock().unwrap();
            if *current + operations > limit {
                return Err(RSpaceError::HostWorkRejected);
            }
            *current += operations;
            Ok(())
        };
        let mut rejected = SpatialMatcherContext::with_meter(&meter).unwrap();
        assert!(rejected.spatial_match(target, pattern).is_none());
        assert!(rejected.free_map.is_empty());
        assert!(matches!(
            rejected.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

    #[test]
    fn set_and_map_remainders_keep_canonical_payloads() {
        let element = new_gint_par(7, Vec::new(), false);
        let set_target = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element.clone(), element.clone()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let set_pattern = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: Some(new_freevar_var(0)),
            })),
        };
        let mut set_context = SpatialMatcherContext::new();
        assert!(set_context.spatial_match(set_target, set_pattern).is_some());
        let expected_set = vector_par(Vec::new(), false).with_exprs(vec![new_eset_expr(
            vec![element],
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(set_context.free_map.get(&0), Some(&expected_set));

        let key = new_gint_par(1, Vec::new(), false);
        let old_value = new_gint_par(2, Vec::new(), false);
        let new_value = new_gint_par(3, Vec::new(), false);
        let map_target = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![
                    new_key_value_pair(key.clone(), old_value),
                    new_key_value_pair(key.clone(), new_value.clone()),
                ],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let map_pattern = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: Some(new_freevar_var(0)),
            })),
        };
        let mut map_context = SpatialMatcherContext::new();
        assert!(map_context.spatial_match(map_target, map_pattern).is_some());
        let expected_map = vector_par(Vec::new(), false).with_exprs(vec![new_emap_expr(
            vec![new_key_value_pair(key, new_value)],
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(map_context.free_map.get(&0), Some(&expected_map));

        let first = Par {
            exprs: vec![new_gint_expr(1), new_gint_expr(2)],
            ..Par::default()
        };
        let second = Par {
            exprs: vec![new_gint_expr(2), new_gint_expr(1)],
            ..Par::default()
        };
        assert_ne!(first, second);
        let duplicate_set = ESet {
            ps: vec![first.clone(), second.clone()],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let canonical_set = ParSetTypeMapper::eset_to_par_set(duplicate_set.clone())
            .ps
            .sorted_pars;
        let mut set_context = SpatialMatcherContext::new();
        assert!(set_context
            .spatial_match(
                Expr {
                    expr_instance: Some(ESetBody(duplicate_set)),
                },
                Expr {
                    expr_instance: Some(ESetBody(ESet {
                        ps: Vec::new(),
                        locally_free: Vec::new(),
                        connective_used: false,
                        remainder: Some(new_freevar_var(0)),
                    })),
                },
            )
            .is_some());
        let expected_set = vector_par(Vec::new(), false).with_exprs(vec![new_eset_expr(
            canonical_set,
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(set_context.free_map.get(&0), Some(&expected_set));

        let duplicate_map = EMap {
            kvs: vec![
                new_key_value_pair(first, new_gint_par(4, Vec::new(), false)),
                new_key_value_pair(second, new_gint_par(4, Vec::new(), false)),
            ],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        };
        let canonical_map = ParMapTypeMapper::emap_to_par_map(duplicate_map.clone())
            .ps
            .sorted_list;
        let mut map_context = SpatialMatcherContext::new();
        assert!(map_context
            .spatial_match(
                Expr {
                    expr_instance: Some(EMapBody(duplicate_map)),
                },
                Expr {
                    expr_instance: Some(EMapBody(EMap {
                        kvs: Vec::new(),
                        locally_free: Vec::new(),
                        connective_used: false,
                        remainder: Some(new_freevar_var(0)),
                    })),
                },
            )
            .is_some());
        let expected_map = vector_par(Vec::new(), false).with_exprs(vec![new_emap_expr(
            canonical_map
                .into_iter()
                .map(|(key, value)| new_key_value_pair(key, value))
                .collect(),
            Vec::new(),
            false,
            None,
        )]);
        assert_eq!(map_context.free_map.get(&0), Some(&expected_map));
    }

    #[test]
    fn malformed_map_pair_rejects_without_publication() {
        let target = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![KeyValuePair {
                    key: None,
                    value: Some(Par::default()),
                }],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: Vec::new(),
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let mut context = SpatialMatcherContext::with_meter(&meter).unwrap();
        assert!(context.spatial_match(target, pattern).is_none());
        assert!(context.free_map.is_empty());
        assert!(matches!(
            context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

    #[test]
    fn set_and_map_matching_preserve_unique_element_semantics() {
        let element = new_gstring_par("element".to_owned(), Vec::new(), false);
        let target_set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element.clone(), element.clone()],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern_set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let key = new_gint_par(1, Vec::new(), false);
        let old_value = new_gint_par(2, Vec::new(), false);
        let new_value = new_gint_par(3, Vec::new(), false);
        let target_map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![
                    new_key_value_pair(key.clone(), old_value),
                    new_key_value_pair(key.clone(), new_value.clone()),
                ],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let pattern_map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs: vec![new_key_value_pair(key, new_value)],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };

        for (target, pattern) in [(target_set, pattern_set), (target_map, pattern_map)] {
            let mut ordinary = SpatialMatcherContext::new();
            assert!(ordinary
                .spatial_match(target.clone(), pattern.clone())
                .is_some());
            let meter = |_: usize, _: usize, _: usize| Ok(());
            let mut metered = SpatialMatcherContext::with_meter(&meter).unwrap();
            assert!(metered.spatial_match(target, pattern).is_some());
            assert_eq!(metered.free_map, ordinary.free_map);
            assert!(metered.take_error().is_none());
        }
    }

    #[test]
    fn set_and_map_conversion_reject_before_initial_allocation() {
        let element = new_gstring_par("x".repeat(4096), Vec::new(), false);
        let set = Expr {
            expr_instance: Some(ESetBody(ESet {
                ps: vec![element],
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let set_meter = |_: usize, _: usize, backing: usize| {
            if backing >= 4096 {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut set_context = SpatialMatcherContext::with_meter(&set_meter).unwrap();
        assert!(set_context.spatial_match(set.clone(), set).is_none());
        assert!(matches!(
            set_context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));

        let kvs: Vec<_> = (0..16)
            .map(|value| {
                new_key_value_pair(
                    new_gint_par(value, Vec::new(), false),
                    new_gint_par(value, Vec::new(), false),
                )
            })
            .collect();
        let threshold = kvs.len() * std::mem::size_of::<(Par, Par)>();
        let map = Expr {
            expr_instance: Some(EMapBody(EMap {
                kvs,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        };
        let map_meter = |_: usize, _: usize, backing: usize| {
            if backing >= threshold {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let mut map_context = SpatialMatcherContext::with_meter(&map_meter).unwrap();
        assert!(map_context.spatial_match(map.clone(), map).is_none());
        assert!(matches!(
            map_context.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

    #[test]
    fn connective_clones_require_complete_host_credit() {
        let target_bytes = "target".len() * 4096;
        let binding_bytes = "binding".len() * 4096;
        let target = new_gstring_par("target".repeat(4096), Vec::new(), false);
        let binding = new_gstring_par("binding".repeat(4096), Vec::new(), false);
        for disjunction in [false, true] {
            let body = ConnectiveBody {
                ps: vec![Par::default()],
            };
            let pattern = Connective {
                connective_instance: Some(if disjunction {
                    ConnOrBody(body)
                } else {
                    ConnAndBody(body)
                }),
            };
            let used = Mutex::new([0usize; 3]);
            let unlimited = |operations: usize, scanned: usize, backing: usize| {
                let mut totals = used.lock().unwrap();
                for (total, amount) in totals.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok(())
            };
            let mut context = SpatialMatcherContext::with_meter(&unlimited).unwrap();
            context.free_map.insert(0, binding.clone());
            let actual = context.spatial_match(target.clone(), pattern.clone());
            assert!(context.take_error().is_none());
            let mut legacy = SpatialMatcherContext::new();
            legacy.free_map.insert(0, binding.clone());
            assert_eq!(
                actual,
                legacy.spatial_match(target.clone(), pattern.clone())
            );
            let required = *used.lock().unwrap();
            assert!(required[2] >= target_bytes);
            if disjunction {
                assert!(required[2] >= binding_bytes);
            }
            for dimension in 0..3 {
                let mut limit = required;
                limit[dimension] -= 1;
                let spent = Mutex::new([0usize; 3]);
                let meter = |operations: usize, scanned: usize, backing: usize| {
                    let mut totals = spent.lock().unwrap();
                    let amounts = [operations, scanned, backing];
                    if totals
                        .iter()
                        .zip(amounts)
                        .zip(limit)
                        .any(|((used, add), max)| *used + add > max)
                    {
                        return Err(RSpaceError::HostWorkRejected);
                    }
                    for (used, add) in totals.iter_mut().zip(amounts) {
                        *used += add;
                    }
                    Ok(())
                };
                let mut context = SpatialMatcherContext::with_meter(&meter).unwrap();
                context.free_map.insert(0, binding.clone());
                context.spatial_match(target.clone(), pattern.clone());
                assert!(matches!(
                    context.take_error(),
                    Some(RSpaceError::HostWorkRejected)
                ));
            }
        }
    }

    /// The three totals that a metered closure receives during `action`.
    fn charge_during(action: impl FnOnce(&mut SpatialMatcherContext<'_>)) -> [usize; 3] {
        let totals = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut sum = totals.lock().expect("totals lock");
            for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let before = *totals.lock().expect("totals lock");
        action(&mut context);
        let after = *totals.lock().expect("totals lock");
        [
            after[0] - before[0],
            after[1] - before[1],
            after[2] - before[2],
        ]
    }

    fn free_or_ground_expr() -> impl proptest::strategy::Strategy<Value = Expr> {
        use proptest::strategy::Strategy;

        (0usize..6, 0i32..4).prop_map(|(variant, index)| {
            let v = match variant {
                0 => {
                    return Expr {
                        expr_instance: Some(GInt(index as i64)),
                    }
                }
                1 => None,
                2 => Some(Var { var_instance: None }),
                3 => Some(Var {
                    var_instance: Some(BoundVar(index)),
                }),
                4 => Some(Var {
                    var_instance: Some(FreeVar(index)),
                }),
                _ => Some(Var {
                    var_instance: Some(Wildcard(WildcardMsg {})),
                }),
            };
            Expr {
                expr_instance: Some(EVarBody(EVar { v })),
            }
        })
    }

    fn ground_par(size: usize) -> Par {
        Par {
            exprs: vec![new_gint_expr(7); size],
            ..Default::default()
        }
    }

    /// D-D2 (DR-104): the in-place filter charges `2n` operations and
    /// `n * (2 * size_of::<Expr>() + 3 words)` scanned bytes, and no backing.
    #[test]
    fn retain_no_frees_charge_is_exact() {
        let per_entry = 2 * std::mem::size_of::<Expr>() + 3 * std::mem::size_of::<usize>();
        for entries in [0usize, 1, 7, 64] {
            assert_eq!(
                charge_during(|context| context
                    .reserve_retain_no_frees(entries)
                    .expect("filter charge")),
                [2 * entries, entries * per_entry, 0],
                "{entries} entries"
            );
        }
    }

    /// D-D2 (DR-104): the in-place filter allocates nothing.
    #[test]
    fn retain_no_frees_allocates_nothing() {
        let mut exprs: Vec<Expr> = (0..32)
            .map(|index| {
                if index % 3 == 0 {
                    new_gint_expr(index)
                } else {
                    Expr {
                        expr_instance: Some(EVarBody(EVar {
                            v: Some(Var {
                                var_instance: Some(FreeVar(index as i32)),
                            }),
                        })),
                    }
                }
            })
            .collect();
        let ((), allocated) = crate::rust::interpreter::accounting::measured_allocations(|| {
            retain_no_frees(&mut exprs)
        });
        assert_eq!(allocated, 0);
        assert_eq!(exprs.len(), 11);
    }

    /// D-D2 (DR-104): comparing a ground pattern charges the same for a small
    /// and for a large target.
    #[test]
    fn ground_comparison_charge_is_independent_of_target_size() {
        let pattern = ground_par(1);
        let charges = [1usize, 4096].map(|size| {
            charge_during(|context| {
                context.spatial_match(ground_par(size), pattern.clone());
            })
        });
        assert_eq!(charges[0], charges[1]);
    }

    /// Negative control: the charge before D-D2 inspected the whole target, so
    /// it grew with the target.
    #[test]
    fn legacy_ground_charge_grew_with_target_size() {
        let pattern = ground_par(1);
        let legacy = |target: Par| {
            charge_during(|context| {
                context.reserve_inspect(&target).expect("target inspection");
                context
                    .reserve_inspect(&pattern)
                    .expect("pattern inspection");
            })
        };
        let small = legacy(ground_par(1));
        let large = legacy(ground_par(4096));
        assert!(large[0] > small[0]);
        assert!(large[1] > small[1]);
    }

    /// D-D2 (DR-104): the ground-pair path accepts its exact charge and
    /// rejects one unit less in any dimension.
    #[test]
    fn ground_pair_by_reference_accepts_exact_credit() {
        let targets = vec![ground_par(3), ground_par(2)];
        let patterns = vec![ground_par(3), ground_par(2)];
        let required = charge_during(|context| {
            assert!(context.fold_match(&targets, &patterns, None).is_some());
        });
        assert!(required.iter().take(2).all(|value| *value > 0));
        for dimension in 0..2 {
            let mut limit = required;
            limit[dimension] -= 1;
            let spent = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut totals = spent.lock().expect("totals lock");
                let amounts = [operations, scanned, backing];
                if totals
                    .iter()
                    .zip(amounts)
                    .zip(limit)
                    .any(|((used, add), max)| *used + add > max)
                {
                    return Err(RSpaceError::HostWorkRejected);
                }
                for (used, add) in totals.iter_mut().zip(amounts) {
                    *used += add;
                }
                Ok(())
            };
            let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
            assert!(context.fold_match(&targets, &patterns, None).is_none());
            assert!(matches!(
                context.take_error(),
                Some(RSpaceError::HostWorkRejected)
            ));
        }
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// D-D2 (DR-104): the in-place filter keeps the expressions that
        /// `no_frees_exprs` keeps, in the same order.
        #[test]
        fn retained_no_frees_equals_cloned_filter(
            exprs in proptest::collection::vec(free_or_ground_expr(), 0..12),
        ) {
            let expected = no_frees_exprs(&exprs);
            let mut retained = exprs;
            retain_no_frees(&mut retained);
            proptest::prop_assert_eq!(retained, expected);
        }

        /// D-D2 (DR-104): a fold that compares ground pairs by reference gives
        /// the result and the free map of the walk that copies every pair into
        /// `spatial_match`.
        #[test]
        fn fold_match_by_reference_equals_cloned_pair_walk(
            targets in proptest::collection::vec((0i64..3, 1usize..3), 0..5),
            patterns in proptest::collection::vec((proptest::prelude::any::<bool>(), 0i64..3, 1usize..3), 0..5),
        ) {
            let targets: Vec<Par> = targets
                .into_iter()
                .map(|(value, size)| Par {
                    exprs: vec![new_gint_expr(value); size],
                    ..Default::default()
                })
                .collect();
            let patterns: Vec<Par> = patterns
                .into_iter()
                .enumerate()
                .map(|(level, (free, value, size))| {
                    if free {
                        let mut par = new_freevar_par(level as i32, Vec::new());
                        par.connective_used = true;
                        par
                    } else {
                        Par {
                            exprs: vec![new_gint_expr(value); size],
                            ..Default::default()
                        }
                    }
                })
                .collect();
            let mut by_reference = SpatialMatcherContext::new();
            let folded = by_reference.fold_match(&targets, &patterns, None);
            let mut copied = SpatialMatcherContext::new();
            let walked = (|| {
                for (target, pattern) in targets.iter().zip(&patterns) {
                    copied.spatial_match(target.clone(), pattern.clone())?;
                }
                (targets.len() == patterns.len()).then(Vec::new)
            })();
            proptest::prop_assert_eq!(folded, walked);
            proptest::prop_assert_eq!(by_reference.free_map, copied.free_map);
        }
    }

    fn with_legacy_free_variable_path<R>(legacy: bool, action: impl FnOnce() -> R) -> R {
        LEGACY_FREE_VARIABLE_PATH.with(|flag| flag.set(legacy));
        let result = action();
        LEGACY_FREE_VARIABLE_PATH.with(|flag| flag.set(false));
        result
    }

    fn free_variable(level: i32, locally_free: Vec<u8>) -> Par {
        let mut par = new_freevar_par(level, Vec::new());
        par.connective_used = true;
        par.locally_free = locally_free;
        par
    }

    fn locally(open: bool) -> Vec<u8> {
        if open {
            vec![1]
        } else {
            Vec::new()
        }
    }

    fn body(open: bool) -> Option<Par> {
        Some(Par {
            locally_free: locally(open),
            ..Default::default()
        })
    }

    fn open_or_closed_expr(open: bool) -> Expr {
        if open {
            new_boundvar_expr(0)
        } else {
            new_gint_expr(3)
        }
    }

    /// A target whose ten fields hold closed or open elements, with a random
    /// shell. An open element has a locally free variable.
    fn free_variable_target() -> impl proptest::strategy::Strategy<Value = Par> {
        use proptest::strategy::Strategy;

        let field = || proptest::collection::vec(proptest::prelude::any::<bool>(), 0..3);
        (
            (field(), field(), field(), field(), field()),
            (field(), field(), field(), field(), field()),
            proptest::prelude::any::<bool>(),
            proptest::collection::vec(0u8..2, 0..3),
        )
            .prop_map(
                |(
                    (sends, receives, news, exprs, matches),
                    (bundles, unforgeables, conditionals, signed, stacks),
                    connective_used,
                    locally_free,
                )| Par {
                    sends: sends
                        .into_iter()
                        .map(|open| Send {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    receives: receives
                        .into_iter()
                        .map(|open| Receive {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    news: news
                        .into_iter()
                        .map(|open| New {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    exprs: exprs.into_iter().map(open_or_closed_expr).collect(),
                    matches: matches
                        .into_iter()
                        .map(|open| Match {
                            locally_free: locally(open),
                            ..Default::default()
                        })
                        .collect(),
                    bundles: bundles
                        .into_iter()
                        .map(|open| Bundle {
                            body: body(open),
                            ..Default::default()
                        })
                        .collect(),
                    unforgeables: unforgeables
                        .into_iter()
                        .map(|_| GUnforgeable::default())
                        .collect(),
                    conditionals: conditionals
                        .into_iter()
                        .map(|open| If {
                            condition: body(open),
                            ..Default::default()
                        })
                        .collect(),
                    cost_signed_terms: signed
                        .into_iter()
                        .map(|open| CostSignedTerm {
                            body: body(open),
                            signature: None,
                        })
                        .collect(),
                    cost_stacks: stacks
                        .into_iter()
                        .map(|open| CostStack {
                            cells: if open {
                                vec![CostSignature {
                                    value: Some(models::rhoapi::cost_signature::Value::BoundLevel(
                                        0,
                                    )),
                                }]
                            } else {
                                Vec::new()
                            },
                        })
                        .collect(),
                    locally_free,
                    connective_used,
                    ..Default::default()
                },
            )
    }

    /// A stored binding with a shell that the merge never writes.
    fn shell_binding() -> Par {
        Par {
            exprs: vec![new_gint_expr(9)],
            locally_free: vec![1, 0],
            connective_used: true,
            connectives: vec![Connective::default()],
            ..Default::default()
        }
    }

    fn run_owned(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
        legacy: bool,
    ) -> (Option<()>, FreeMap) {
        with_legacy_free_variable_path(legacy, || {
            let mut context = SpatialMatcherContext::new();
            context.free_map = free_map.clone();
            let result = context.spatial_match(target.clone(), pattern.clone());
            (result, context.free_map)
        })
    }

    fn run_borrowed(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
        legacy: bool,
    ) -> (Option<Vec<Par>>, FreeMap) {
        with_legacy_free_variable_path(legacy, || {
            let mut context = SpatialMatcherContext::new();
            context.free_map = free_map.clone();
            let result = context.fold_match(
                std::slice::from_ref(target),
                std::slice::from_ref(pattern),
                None,
            );
            (result, context.free_map)
        })
    }

    /// The result, the free map and whether a reservation was rejected, under
    /// a meter that accepts everything.
    fn run_borrowed_metered(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
    ) -> (Option<Vec<Par>>, FreeMap, bool) {
        let meter = |_: usize, _: usize, _: usize| Ok(());
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        context.free_map = free_map.clone();
        let result = context.fold_match(
            std::slice::from_ref(target),
            std::slice::from_ref(pattern),
            None,
        );
        let rejected = context.take_error().is_some();
        (result, context.free_map, rejected)
    }

    fn pair_charge(target: &Par, pattern: &Par, legacy: bool) -> [usize; 3] {
        with_legacy_free_variable_path(legacy, || {
            charge_during(|context| {
                context.fold_match(
                    std::slice::from_ref(target),
                    std::slice::from_ref(pattern),
                    None,
                );
            })
        })
    }

    /// D-D3 (DR-105): the predicate accepts exactly one free variable.
    #[test]
    fn free_variable_level_accepts_only_one_free_variable() {
        assert_eq!(free_variable_level(&free_variable(2, vec![1])), Some(2));
        let mut not_connective = free_variable(2, Vec::new());
        not_connective.connective_used = false;
        assert_eq!(free_variable_level(&not_connective), None);
        let mut wildcard = new_wildcard_par(Vec::new(), true);
        wildcard.connective_used = true;
        assert_eq!(free_variable_level(&wildcard), None);
        let mut two = free_variable(0, Vec::new());
        two.exprs.push(new_freevar_expr(1));
        assert_eq!(free_variable_level(&two), None);
        let mut with_wildcard = free_variable(0, Vec::new());
        with_wildcard.exprs.push(new_wildcard_expr());
        assert_eq!(free_variable_level(&with_wildcard), None);
        let mut bound = free_variable(0, Vec::new());
        bound.exprs = vec![new_boundvar_expr(0)];
        assert_eq!(free_variable_level(&bound), None);
        for extend in [
            |p: &mut Par| p.sends.push(Send::default()),
            |p: &mut Par| p.receives.push(Receive::default()),
            |p: &mut Par| p.news.push(New::default()),
            |p: &mut Par| p.matches.push(Match::default()),
            |p: &mut Par| p.unforgeables.push(GUnforgeable::default()),
            |p: &mut Par| p.bundles.push(Bundle::default()),
            |p: &mut Par| p.connectives.push(Connective::default()),
            |p: &mut Par| p.conditionals.push(If::default()),
            |p: &mut Par| p.cost_signed_terms.push(CostSignedTerm::default()),
            |p: &mut Par| p.cost_stacks.push(CostStack::default()),
        ] {
            let mut pattern = free_variable(0, Vec::new());
            extend(&mut pattern);
            assert_eq!(free_variable_level(&pattern), None);
        }
    }

    /// D-D3 (DR-105): a field with a locally free element stops the binding,
    /// and the fields before it stay merged, as on the general path. The
    /// Par/Par order puts bundles before unforgeables.
    #[test]
    fn free_variable_failure_leaves_the_general_partial_binding() {
        let level = 1;
        let pattern = free_variable(level, Vec::new());
        let target = Par {
            sends: vec![Send::default()],
            exprs: vec![new_gint_expr(4)],
            bundles: vec![Bundle {
                body: body(true),
                ..Default::default()
            }],
            unforgeables: vec![GUnforgeable::default()],
            ..Default::default()
        };
        let mut expected = vector_par(Vec::new(), false);
        expected.sends = target.sends.clone();
        expected.exprs = target.exprs.clone();
        let expected_map: FreeMap = [(level, expected)].into_iter().collect();
        for legacy in [true, false] {
            let (result, free_map) = run_owned(&target, &pattern, &FreeMap::new(), legacy);
            assert!(result.is_none());
            assert_eq!(free_map, expected_map, "owned, legacy {legacy}");
            let (result, free_map) = run_borrowed(&target, &pattern, &FreeMap::new(), legacy);
            assert!(result.is_none());
            assert_eq!(free_map, expected_map, "borrowed, legacy {legacy}");
        }
        let first_open = Par {
            sends: vec![Send {
                locally_free: vec![1],
                ..Default::default()
            }],
            exprs: vec![new_gint_expr(4)],
            ..Default::default()
        };
        let existing: FreeMap = [(level, shell_binding())].into_iter().collect();
        for legacy in [true, false] {
            assert_eq!(
                run_owned(&first_open, &pattern, &existing, legacy),
                (None, existing.clone())
            );
            assert_eq!(
                run_borrowed(&first_open, &pattern, &existing, legacy),
                (None, existing.clone())
            );
        }
    }

    /// D-D3 (DR-105): the free-variable pair accepts its exact charge and
    /// rejects one unit less in any charged dimension.
    #[test]
    fn free_variable_pair_accepts_exact_credit() {
        let pattern = free_variable(0, Vec::new());
        let target = Par {
            sends: vec![Send::default()],
            exprs: vec![new_gint_expr(4), new_gint_expr(5)],
            ..Default::default()
        };
        let required = pair_charge(&target, &pattern, false);
        assert!(required.iter().all(|value| *value > 0));
        for dimension in 0..3 {
            let mut limit = required;
            limit[dimension] -= 1;
            let spent = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut totals = spent.lock().expect("totals lock");
                let amounts = [operations, scanned, backing];
                if totals
                    .iter()
                    .zip(amounts)
                    .zip(limit)
                    .any(|((used, add), max)| *used + add > max)
                {
                    return Err(RSpaceError::HostWorkRejected);
                }
                for (used, add) in totals.iter_mut().zip(amounts) {
                    *used += add;
                }
                Ok(())
            };
            let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
            assert!(context
                .fold_match(
                    std::slice::from_ref(&target),
                    std::slice::from_ref(&pattern),
                    None
                )
                .is_none());
            assert!(matches!(
                context.take_error(),
                Some(RSpaceError::HostWorkRejected)
            ));
        }
    }

    /// Negative control: the general path charged a copy of the pattern and of
    /// the whole target and an inspection of the target, so its charge exceeds
    /// the fast path's, and the gap grows with the target.
    #[test]
    fn legacy_free_variable_charge_grew_with_target_size() {
        let pattern = free_variable(0, Vec::new());
        let gap = |size: usize| {
            let target = ground_par(size);
            let legacy = pair_charge(&target, &pattern, true);
            let fast = pair_charge(&target, &pattern, false);
            assert!(
                legacy[0] > fast[0] && legacy[1] > fast[1],
                "{size}: {legacy:?} {fast:?}"
            );
            legacy[1] - fast[1]
        };
        assert!(gap(4096) > gap(1));
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// D-D3 (DR-105): on the owned and the borrowed entry, the fast path for
        /// a free-variable pattern gives the result and the free map of the
        /// general path, also when a field holds a locally free element, with
        /// and without a stored binding at the level. Under a meter that
        /// accepts everything it gives the same outcome and rejects nothing.
        #[test]
        fn free_variable_fast_path_equals_general_path(
            level in 0i32..4,
            target in free_variable_target(),
            pattern_locally_free in proptest::collection::vec(0u8..2, 0..3),
            existing in proptest::prelude::any::<bool>(),
            others in proptest::collection::vec(0i64..3, 0..3),
        ) {
            let pattern = free_variable(level, pattern_locally_free);
            let mut free_map = FreeMap::new();
            for (offset, value) in others.into_iter().enumerate() {
                free_map.insert(level + 1 + offset as i32, Par {
                    exprs: vec![new_gint_expr(value)],
                    ..Default::default()
                });
            }
            if existing {
                free_map.insert(level, shell_binding());
            }
            let general = run_owned(&target, &pattern, &free_map, true);
            proptest::prop_assert_eq!(run_owned(&target, &pattern, &free_map, false), general.clone());
            let general_fold = run_borrowed(&target, &pattern, &free_map, true);
            proptest::prop_assert_eq!(run_borrowed(&target, &pattern, &free_map, false), general_fold.clone());
            proptest::prop_assert_eq!(&general_fold.1, &general.1);
            proptest::prop_assert_eq!(general_fold.0.is_some(), general.0.is_some());
            let (result, metered_map, rejected) = run_borrowed_metered(&target, &pattern, &free_map);
            proptest::prop_assert_eq!(result, general_fold.0);
            proptest::prop_assert_eq!(metered_map, general_fold.1);
            proptest::prop_assert!(!rejected);
        }

        /// D-D3 (DR-105): the copies of the borrowed fields fit the backing
        /// that the fast path reserves (counting allocator). The test measures
        /// the binding itself: `fold_match` also allocates the labels of its
        /// metrics counters, which are outside the host-work model.
        #[test]
        fn free_variable_copies_fit_reserved_backing(target in free_variable_target()) {
            let totals = Mutex::new([0usize; 3]);
            let meter = |operations: usize, scanned: usize, backing: usize| {
                let mut sum = totals.lock().expect("totals lock");
                for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                    *total += amount;
                }
                Ok(())
            };
            let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
            let start = totals.lock().expect("totals lock")[2];
            let (_, allocated) = crate::rust::interpreter::accounting::measured_allocations(|| {
                context.bind_free_variable_by_reference(&target, 0)
            });
            let reserved = totals.lock().expect("totals lock")[2] - start;
            proptest::prop_assert!(
                allocated <= reserved,
                "allocated {} > reserved {}",
                allocated,
                reserved
            );
        }
    }

    // D-E2 (DR-109): the matcher sites in block mode.

    /// The result of `action` on a metered context, and the reservations that
    /// it makes, in call order, after the context's own setup charge.
    fn matcher_calls<R>(
        action: impl FnOnce(&mut SpatialMatcherContext<'_>) -> R,
    ) -> (R, Vec<[usize; 3]>) {
        let log = Mutex::new(Vec::with_capacity(1_024));
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.lock()
                .expect("log lock")
                .push([operations, scanned, backing]);
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let start = log.lock().expect("log lock").len();
        let result = action(&mut context);
        assert!(context.take_error().is_none());
        let calls = log.lock().expect("log lock")[start..].to_vec();
        (result, calls)
    }

    /// The reservations of one walk of the shared walker, in call order.
    fn walk_calls(
        walk: impl FnOnce(&dyn BackingMeter) -> Result<(), BackingError>,
    ) -> Vec<[usize; 3]> {
        let log = std::cell::RefCell::new(Vec::new());
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.borrow_mut().push([operations, scanned, backing]);
            Ok::<(), BackingError>(())
        };
        walk(&meter).expect("an unlimited meter");
        log.into_inner()
    }

    fn scanned(calls: &[[usize; 3]]) -> usize { calls.iter().map(|call| call[1]).sum() }

    fn large_string_par(length: usize) -> Par {
        Par {
            exprs: vec![new_gstring_expr("s".repeat(length))],
            ..Par::default()
        }
    }

    /// The charge of `MatcherWork::reserve_vec` for one more element of a
    /// vector of `len` elements whose capacity is `capacity`.
    fn vec_growth<T>(len: usize, capacity: &mut usize) -> [usize; 3] {
        let needed = len + 1;
        if needed <= *capacity {
            return [1, 0, 0];
        }
        let next = needed.max(*capacity * 2).max(4);
        *capacity = next;
        [
            1,
            len * std::mem::size_of::<T>(),
            next * std::mem::size_of::<T>(),
        ]
    }

    /// D-E2 (DR-109): each block wrapper of the matcher makes exactly the
    /// reservations of the shared block walk that it names. Without a meter
    /// it walks nothing, and after a rejection it reserves nothing more.
    #[test]
    fn matcher_block_wrappers_charge_the_shared_block_walks() {
        let par = Par {
            exprs: vec![new_gint_expr(3), new_gstring_expr("value".repeat(20))],
            sends: vec![Send {
                data: vec![ground_par(2)],
                ..Default::default()
            }],
            ..Par::default()
        };
        let pars = vec![par.clone(), ground_par(4), Par::default()];
        let free_map: FreeMap = (0..3).map(|key| (key, ground_par(2))).collect();
        let (_, inspected) = matcher_calls(|context| context.inspect_blocks(&par));
        assert_eq!(
            inspected,
            walk_calls(|meter| clone_backing::inspect_blocks(&par, meter))
        );
        let (_, copied) =
            matcher_calls(|context| context.reserve_blocks_copy_and_cleanup(&free_map));
        assert_eq!(
            copied,
            walk_calls(|meter| clone_backing::reserve_blocks_copy_and_cleanup(&free_map, meter))
        );
        let (_, sliced) =
            matcher_calls(|context| context.reserve_blocks_slice_copy_and_cleanup(&pars));
        assert_eq!(
            sliced,
            walk_calls(|meter| clone_backing::reserve_blocks_slice_copy_and_cleanup(&pars, meter))
        );
        let unmetered = SpatialMatcherContext::new();
        assert_eq!(unmetered.inspect_blocks(&par), Some(()));
        assert_eq!(
            unmetered.reserve_blocks_copy_and_cleanup(&free_map),
            Some(())
        );
        assert_eq!(
            unmetered.reserve_blocks_slice_copy_and_cleanup(&pars),
            Some(())
        );
        let made = Mutex::new(0usize);
        // The first call is the context's setup charge; the second is refused.
        let meter = |_: usize, _: usize, _: usize| {
            let mut calls = made.lock().expect("calls lock");
            *calls += 1;
            if *calls == 2 {
                Err(RSpaceError::HostWorkRejected)
            } else {
                Ok(())
            }
        };
        let rejecting = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        assert_eq!(rejecting.inspect_blocks(&par), None);
        let calls = *made.lock().expect("calls lock");
        assert_eq!(rejecting.reserve_blocks_copy_and_cleanup(&free_map), None);
        assert_eq!(rejecting.reserve_blocks_slice_copy_and_cleanup(&pars), None);
        assert_eq!(*made.lock().expect("calls lock"), calls);
        assert!(matches!(
            rejecting.take_error(),
            Some(RSpaceError::HostWorkRejected)
        ));
    }

    /// D-E2 (DR-109): each block wrapper returns `None` at every cut, records
    /// the meter's error, and reserves nothing after the cut.
    #[test]
    fn matcher_block_wrappers_stop_at_every_cut() {
        let par = large_string_par(64);
        let pars = vec![par.clone(), ground_par(3)];
        let wrappers: [(&str, &dyn Fn(&SpatialMatcherContext<'_>) -> Option<()>); 3] = [
            ("inspection", &|context| context.inspect_blocks(&par)),
            ("copy", &|context| {
                context.reserve_blocks_copy_and_cleanup(&par)
            }),
            ("slice copy", &|context| {
                context.reserve_blocks_slice_copy_and_cleanup(&pars)
            }),
        ];
        for (name, wrapper) in wrappers {
            let (_, baseline) = matcher_calls(|context| wrapper(context));
            assert!(!baseline.is_empty(), "{name}");
            for cut in 0..baseline.len() {
                let made = Mutex::new(0usize);
                // The first call is the context's setup charge.
                let meter = |_: usize, _: usize, _: usize| {
                    let mut calls = made.lock().expect("calls lock");
                    *calls += 1;
                    if *calls == cut + 2 {
                        Err(RSpaceError::HostWorkRejected)
                    } else {
                        Ok(())
                    }
                };
                let context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
                assert_eq!(wrapper(&context), None, "{name}, cut {cut}");
                assert_eq!(
                    *made.lock().expect("calls lock"),
                    cut + 2,
                    "{name}, cut {cut}"
                );
                assert!(
                    matches!(context.take_error(), Some(RSpaceError::HostWorkRejected)),
                    "{name}, cut {cut}"
                );
            }
        }
    }

    /// D-E2 (DR-109): a ground comparison charges two block inspections of the
    /// pattern and nothing that depends on the target
    /// (`MatcherReadsByReference.two_pattern_inspections_cover_lockstep_reads`).
    #[test]
    fn ground_comparison_charges_two_block_pattern_inspections() {
        let pattern = ground_par(3);
        let pattern_walk = walk_calls(|meter| clone_backing::inspect_blocks(&pattern, meter));
        let expected = [pattern_walk.clone(), pattern_walk].concat();
        for size in [1usize, 3, 4_096] {
            let target = ground_par(size);
            let (result, calls) =
                matcher_calls(|context| context.match_ground_par(&target, &pattern));
            assert_eq!(result.is_some(), size == 3, "{size}");
            assert_eq!(calls, expected, "{size}");
        }
    }

    /// D-E2 (DR-109): merging a set remainder inspects each element once and
    /// the vector of unique elements twice before each `contains` scan
    /// (`MatcherReadsByReference.two_container_inspections_cover_membership_scan`).
    #[test]
    fn merge_set_remainder_charges_two_container_traversals() {
        let elements = vec![
            ground_par(2),
            large_string_par(256),
            ground_par(2),
            ground_par(5),
        ];
        let (result, calls) = matcher_calls(|context| {
            let work = context.work()?;
            let mut binding = Par::default();
            merge_set_remainder(&mut binding, elements.clone(), &work)
        });
        assert!(result.is_some());
        // `work()` charges one operation.
        let mut expected = vec![[1, 0, 0]];
        let mut unique: Vec<Par> = Vec::new();
        let mut capacity = 0;
        for element in &elements {
            expected.extend(walk_calls(|meter| {
                clone_backing::inspect_blocks(element, meter)
            }));
            for _ in 0..2 {
                expected.extend(walk_calls(|meter| {
                    clone_backing::inspect_blocks(&unique, meter)
                }));
            }
            if !unique.contains(element) {
                expected.push(vec_growth::<Par>(unique.len(), &mut capacity));
                unique.push(element.clone());
            }
        }
        assert_eq!(&calls[..expected.len()], expected.as_slice());
    }

    /// D-E2 (DR-109): merging a map remainder inspects each key once and the
    /// vector of unique entries twice before each `position` scan.
    #[test]
    fn merge_map_remainder_charges_two_container_traversals() {
        let entries = vec![
            (ground_par(1), ground_par(2)),
            (large_string_par(256), ground_par(3)),
            (ground_par(1), ground_par(4)),
        ];
        let (result, calls) = matcher_calls(|context| {
            let work = context.work()?;
            let mut binding = Par::default();
            merge_map_remainder(&mut binding, entries.clone(), &work)
        });
        assert!(result.is_some());
        let mut expected = vec![[1, 0, 0]];
        let mut unique: Vec<(Par, Par)> = Vec::new();
        let mut capacity = 0;
        for (key, value) in &entries {
            expected.extend(walk_calls(|meter| {
                clone_backing::inspect_blocks(key, meter)
            }));
            for _ in 0..2 {
                expected.extend(walk_calls(|meter| {
                    clone_backing::inspect_blocks(&unique, meter)
                }));
            }
            if let Some(index) = unique.iter().position(|(existing, _)| existing == key) {
                unique[index].1 = value.clone();
            } else {
                expected.push(vec_growth::<(Par, Par)>(unique.len(), &mut capacity));
                unique.push((key.clone(), value.clone()));
            }
        }
        assert_eq!(&calls[..expected.len()], expected.as_slice());
    }

    /// D-E2 (DR-109): a scalar connective tests the target with `single_expr`,
    /// which copies the target's only expression. The test reserves exactly
    /// that copy and its release, and the backing covers the copy of a 4 KiB
    /// string. A target with two expressions, and a connective that does not
    /// call `single_expr`, reserve no copy.
    #[test]
    fn scalar_connective_test_reserves_the_single_expression_copy() {
        let target = large_string_par(4_096);
        let string = Connective {
            connective_instance: Some(ConnString(true)),
        };
        let inspections = |target: &Par, connective: &Connective| {
            [
                walk_calls(|meter| clone_backing::inspect_blocks(target, meter)),
                walk_calls(|meter| clone_backing::inspect_blocks(connective, meter)),
            ]
            .concat()
        };
        let log = Mutex::new(Vec::with_capacity(1_024));
        let meter = |operations: usize, scanned: usize, backing: usize| {
            log.lock()
                .expect("log lock")
                .push([operations, scanned, backing]);
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let start = log.lock().expect("log lock").len();
        let (target_argument, pattern_argument) = (target.clone(), string.clone());
        let (result, allocated) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                context.spatial_match(target_argument, pattern_argument)
            });
        assert_eq!(result, Some(()));
        let calls = log.lock().expect("log lock")[start..].to_vec();
        let copy = walk_calls(|meter| {
            clone_backing::reserve_blocks_copy_and_cleanup(&target.exprs[0], meter)
        });
        assert_eq!(calls, [inspections(&target, &string), copy].concat());
        let reserved: usize = calls.iter().map(|call| call[2]).sum();
        assert!(
            allocated <= reserved,
            "allocated {allocated}, reserved {reserved}"
        );
        let two = Par {
            exprs: vec![new_gstring_expr("a".to_owned()); 2],
            ..Par::default()
        };
        let (result, calls) =
            matcher_calls(|context| context.spatial_match(two.clone(), string.clone()));
        assert_eq!(result, None);
        assert_eq!(calls, inspections(&two, &string));
        let reference = Connective {
            connective_instance: Some(VarRefBody(VarRef::default())),
        };
        let (result, calls) =
            matcher_calls(|context| context.spatial_match(target.clone(), reference.clone()));
        assert_eq!(result, None);
        assert_eq!(calls, inspections(&target, &reference));
    }

    /// D-E2 (DR-109): the copies of the fields that a free-variable pattern
    /// binds fit the reserved backing also when the fields hold large
    /// payloads. A release walk alone would reserve less than one payload.
    #[test]
    fn free_variable_copies_of_large_fields_fit_reserved_backing() {
        let target = Par {
            exprs: vec![new_gstring_expr("e".repeat(4_096)); 2],
            sends: vec![Send {
                data: vec![large_string_par(4_096)],
                ..Default::default()
            }],
            ..Par::default()
        };
        let release: usize = walk_calls(|meter| clone_backing::inspect_blocks(&target, meter))
            .iter()
            .map(|call| call[2])
            .sum();
        assert!(release < 4_096, "release backing {release}");
        let totals = Mutex::new([0usize; 3]);
        let meter = |operations: usize, scanned: usize, backing: usize| {
            let mut sum = totals.lock().expect("totals lock");
            for (total, amount) in sum.iter_mut().zip([operations, scanned, backing]) {
                *total += amount;
            }
            Ok(())
        };
        let mut context = SpatialMatcherContext::with_meter(&meter).expect("metered context");
        let start = totals.lock().expect("totals lock")[2];
        let (result, allocated) =
            crate::rust::interpreter::accounting::measured_allocations(|| {
                context.bind_free_variable_by_reference(&target, 0)
            });
        assert!(result.is_some());
        let reserved = totals.lock().expect("totals lock")[2] - start;
        assert!(
            allocated <= reserved,
            "allocated {allocated}, reserved {reserved}"
        );
    }

    /// D-E2 (DR-109): for the `Par` values that the matcher walks, a block walk
    /// never charges more VerificationBytes than the per-level walk, for an
    /// inspection, a copy and cleanup, and a slice copy and cleanup.
    #[test]
    fn block_charge_le_per_level_for_matcher_values() {
        let values = vec![
            Par::default(),
            ground_par(1),
            ground_par(64),
            large_string_par(1),
            large_string_par(4_096),
            Par {
                sends: vec![Send {
                    data: vec![ground_par(2), large_string_par(32)],
                    ..Default::default()
                }],
                exprs: vec![new_freevar_expr(0), new_gint_expr(5)],
                ..Par::default()
            },
            new_elist_par(
                vec![ground_par(1), large_string_par(8)],
                Vec::new(),
                false,
                Some(new_freevar_var(0)),
                Vec::new(),
                false,
            ),
        ];
        let per_level = |action: &dyn Fn(&SpatialMatcherContext<'_>) -> Option<()>| {
            scanned(&matcher_calls(|context| action(context)).1)
        };
        for value in &values {
            assert!(
                per_level(&|context| context.inspect_blocks(value))
                    <= per_level(&|context| context.reserve_inspect(value)),
                "inspection of {value:?}"
            );
            assert!(
                per_level(&|context| context.reserve_blocks_copy_and_cleanup(value))
                    <= per_level(&|context| context.reserve_clone(value)),
                "copy of {value:?}"
            );
        }
        assert!(
            per_level(&|context| context.reserve_blocks_slice_copy_and_cleanup(&values))
                <= per_level(&|context| context.reserve_slice(&values))
        );
    }

    /// Negative control for `block_charge_le_per_level_for_matcher_values`: a
    /// remainder variable is a chain of entries smaller than 19 bytes
    /// (`Option<Var>`, `Var`, `Option<VarInstance>`, `VarInstance`). Each entry
    /// costs two entry constants in block mode, against two pushes of three
    /// reads of its bytes in per-level mode, so the block copy and cleanup
    /// charges more. The root is read five times in block mode.
    #[test]
    fn block_copy_exceeds_per_level_for_remainder_variables() {
        use models::rhoapi::var::VarInstance;
        let entry = shared::rust::clone_backing::BLOCK_ENTRY_SCANNED;
        let field = shared::rust::clone_backing::BLOCK_FIELD_SCANNED;
        let root = std::mem::size_of::<Option<Var>>();
        let chain = std::mem::size_of::<Option<Var>>()
            + std::mem::size_of::<Var>()
            + std::mem::size_of::<Option<VarInstance>>()
            + std::mem::size_of::<VarInstance>();
        let free = Some(new_freevar_var(0));
        let wildcard = Some(new_wildcard_var());
        let charge = |copy: bool, value: &Option<Var>| {
            scanned(&walk_calls(|meter| {
                if copy {
                    clone_backing::reserve_blocks_copy_and_cleanup(value, meter)
                } else {
                    clone_backing::reserve_copy_and_cleanup(value, meter)
                }
            }))
        };
        // A free variable ends in an `i32` field. A wildcard ends in an empty
        // message, which is one more entry.
        assert_eq!(charge(true, &free), 5 * root + 2 * (4 * entry + field));
        assert_eq!(
            charge(false, &free),
            2 * (3 * chain + 3 * std::mem::size_of::<i32>())
        );
        assert_eq!(charge(true, &wildcard), 5 * root + 2 * (5 * entry));
        assert_eq!(charge(false, &wildcard), 2 * (3 * chain));
        assert!(charge(true, &free) > charge(false, &free));
        assert!(charge(true, &wildcard) > charge(false, &wildcard));
    }

    /// Negative control for `block_charge_le_per_level_for_matcher_values`: the
    /// node of a free map with one binding has eleven slots. The block copy
    /// and cleanup reads the whole node five times, against four times in
    /// per-level mode, and one binding saves less than that extra read.
    #[test]
    fn block_copy_exceeds_per_level_for_sparse_free_maps() {
        let free_map: FreeMap = [(0, Par::default())].into_iter().collect();
        let node = shared::rust::collection_backing::tree_backing::<i32, Par>(1)
            .expect("one node")
            .1;
        let block = scanned(&walk_calls(|meter| {
            clone_backing::reserve_blocks_copy_and_cleanup(&free_map, meter)
        }));
        let per_level = scanned(&walk_calls(|meter| {
            clone_backing::reserve_copy_and_cleanup(&free_map, meter)
        }));
        assert!(block > per_level, "{block} against {per_level}");
        assert!(
            block - per_level <= node,
            "{} of a {node}-byte node",
            block - per_level
        );
    }

    /// D-E2 (DR-109): for the values that the matcher copies, the backing of
    /// a block copy and cleanup covers the allocations of its walks, of the
    /// clone and of its release, and the backing of a block inspection covers
    /// the allocations of its walk.
    #[test]
    fn block_walks_cover_matcher_copy_and_worklist_allocations() {
        fn covered<T: CloneBacking + Clone>(value: &T, name: &str) {
            let reserved = std::cell::Cell::new(0usize);
            let meter = |_: usize, _: usize, backing: usize| {
                reserved.set(reserved.get() + backing);
                Ok::<(), BackingError>(())
            };
            let ((), allocated) =
                crate::rust::interpreter::accounting::measured_allocations(|| {
                    clone_backing::reserve_blocks_copy_and_cleanup(value, &meter)
                        .expect("an unlimited meter");
                    drop(value.clone());
                });
            assert!(
                allocated <= reserved.get(),
                "{name}: {allocated} > {}",
                reserved.get()
            );
            reserved.set(0);
            let ((), allocated) =
                crate::rust::interpreter::accounting::measured_allocations(|| {
                    clone_backing::inspect_blocks(value, &meter).expect("an unlimited meter");
                });
            assert!(
                allocated <= reserved.get(),
                "{name}: {allocated} > {}",
                reserved.get()
            );
        }
        let nested = Par {
            sends: vec![Send {
                chan: Some(large_string_par(4_096)),
                data: vec![ground_par(3), large_string_par(512)],
                ..Default::default()
            }],
            exprs: vec![new_freevar_expr(0), new_gstring_expr("x".repeat(2_048))],
            ..Par::default()
        };
        covered(&Par::default(), "empty par");
        covered(&ground_par(64), "ground par");
        covered(&large_string_par(4_096), "string par");
        covered(&nested, "nested par");
        covered(&vec![nested.clone(), ground_par(2)], "par vector");
        covered(&nested.exprs[1], "expression");
        let free_map: FreeMap = (0..12).map(|key| (key, nested.clone())).collect();
        covered(&free_map, "free map");
        covered(&Some(new_freevar_var(0)), "remainder variable");
        covered(
            &Connective {
                connective_instance: Some(ConnAndBody(ConnectiveBody {
                    ps: vec![nested.clone(), ground_par(1)],
                })),
            },
            "connective",
        );
    }
}
