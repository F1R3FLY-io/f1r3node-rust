//! Stack-safe spatial matching machine.
//!
//! Spatial matching is one mutually recursive semantic operation even though
//! the source-level entry points are split across `Par`, `Expr`, `Send`,
//! `Receive`, connectives, ordered folds, unordered collection matching and
//! EPathMap. This module defunctionalizes that whole call graph into one
//! `Job`/`Frame` pushdown automaton. A nested pattern therefore grows heap work
//! storage, never the native call stack.

#[cfg(any(
    test,
    feature = "spatial-matcher-candidate-mask-differential",
    feature = "spatial-matcher-connective-domain-differential",
    feature = "spatial-matcher-exact-scalar-differential"
))]
use std::cell::Cell;
use std::cmp::Ordering;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::If;
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_bag::{
    canonical_collection_remainder, canonical_connective_parts, canonical_epathmap_root_route_hint,
    canonical_epathmap_root_view, canonical_exact_free_var_expr_level,
    canonical_exact_free_var_par, canonical_exact_scalar_expr, canonical_exact_scalar_par,
    canonical_expr_oneof_tag, canonical_expr_parts, canonical_if_parts, canonical_match_parts,
    canonical_new_parts, canonical_par_connective_used, canonical_par_locally_free_is_empty,
    canonical_receive_parts, canonical_send_parts, canonical_single_connective_from_par,
    canonical_single_expr_from_par, canonical_single_if_from_par, canonical_single_match_from_par,
    canonical_single_new_from_par, canonical_single_receive_from_par,
    canonical_single_send_from_par, CanonicalConnectiveChildren, CanonicalConnectiveParts,
    CanonicalEPathMapRootRouteHint, CanonicalExprParts, CanonicalOwnedConnectiveChildren,
    OwnedCanonicalParComponent, ParBagComponent, StableConnectiveBodyOwner,
};
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rhoapi_ext::{EntryTrie, OwnedEPathMapEntry};
use models::rust::rholang::bincode_encoder;
use models::rust::rholang::implicits::vector_par;
use models::rust::rholang::match_path::{
    encode_match_path, encode_par_match_atom, shares_cow_bincode_identity,
};
use models::rust::rholang::match_schema::RhoMatchProgram;
use models::rust::rholang::match_schema_tables::{
    ParCursor, BUNDLE_RHO_MATCH_PROGRAM, CONNECTIVE_RHO_MATCH_PROGRAM, EXPR_RHO_MATCH_PROGRAM,
    GUNFORGEABLE_RHO_MATCH_PROGRAM, IF_RHO_MATCH_PROGRAM, MATCH_RHO_MATCH_PROGRAM,
    NEW_RHO_MATCH_PROGRAM, RECEIVE_RHO_MATCH_PROGRAM, SEND_RHO_MATCH_PROGRAM,
};
use models::rust::rholang::protobuf_cursor::{CanonicalParRegionIndex, ProtobufRegion};
use models::rust::rholang::protobuf_region::{
    cut_at_par, ProtobufCutPlan, ProtobufRegionStep, ProtobufRegionStepKind,
};
use models::rust::utils::*;
use pathmap::PathMap;
use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::rho_path::{RhoPathQuery, RhoPathV1, StructuralSequenceEncoder};

use super::exports::*;
#[cfg(test)]
use super::free_map_transaction::{
    reset_stats as reset_free_map_transaction_stats, stats as free_map_transaction_stats,
    FreeMapTransactionStats,
};
use super::free_map_transaction::{BindingDelta, FreeMapCheckpoint, FreeMapTransaction};
use super::has_locally_free::HasLocallyFree;
#[cfg(test)]
use super::lazy_relation::RelationalMatchStats;
use super::lazy_relation::{EdgeResult, LazyRelation, RelationCursor};
use super::list_match::validate_update_deltas;
use super::match_pars::{
    compare_bundles_without_locally_free, compare_connectives_without_locally_free,
    compare_exprs_without_locally_free, compare_ifs_without_locally_free,
    compare_matches_without_locally_free, compare_news_without_locally_free,
    compare_receives_without_locally_free, compare_sends_without_locally_free,
    compare_unforgeables_without_locally_free, match_pars,
};
use super::par_count::ParCount;
use super::r#match::{rho_path_query_atom, rho_path_query_cursor_atom};
use super::spatial_matcher::SpatialMatcherContext;
use super::sub_pars::sub_pars;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EncodedExprOuterRoute {
    RejectProvenDifferent,
    Delegate,
}

/// Decide only the outer-variant refinement. `None` is unproved evidence and
/// therefore always delegates to the encoded/decode fallback; it is never a
/// semantic negative.
#[inline]
pub(super) const fn encoded_expr_outer_route(
    target_outer_tag: Option<u32>,
    pattern_deep_tag: Option<u32>,
) -> EncodedExprOuterRoute {
    match (target_outer_tag, pattern_deep_tag) {
        (Some(target), Some(pattern)) if target != pattern => {
            EncodedExprOuterRoute::RejectProvenDifferent
        }
        _ => EncodedExprOuterRoute::Delegate,
    }
}

pub(super) fn match_par(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Par,
) -> Option<()> {
    drive(context, MatchPair::Par(target, pattern))
}

#[cfg(test)]
thread_local! {
    static LIST_RELATION_STATS: Cell<RelationalMatchStats> =
        Cell::new(RelationalMatchStats::default());
    static CONCRETE_REGION_STATS: Cell<ConcreteRegionMatchStats> =
        Cell::new(ConcreteRegionMatchStats::default());
    static FORCE_CANDIDATE_NODE_COUNT_ORDER: Cell<bool> = const { Cell::new(false) };
    static CANDIDATE_EXPANSION_MODE: Cell<CandidateExpansionMode> =
        const { Cell::new(CandidateExpansionMode::Adaptive) };
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CandidateExpansionMode {
    Adaptive,
    Sparse,
    Dense,
}

#[cfg(test)]
struct CandidateTestModeGuard<'a, T: Copy> {
    cell: &'a Cell<T>,
    previous: T,
}

#[cfg(test)]
impl<T: Copy> Drop for CandidateTestModeGuard<'_, T> {
    fn drop(&mut self) { self.cell.set(self.previous) }
}

#[cfg(test)]
fn with_candidate_node_count_order<T>(body: impl FnOnce() -> T) -> T {
    FORCE_CANDIDATE_NODE_COUNT_ORDER.with(|mode| {
        let previous = mode.replace(true);
        let _guard = CandidateTestModeGuard {
            cell: mode,
            previous,
        };
        body()
    })
}

#[cfg(test)]
fn with_candidate_expansion_mode<T>(
    selected: CandidateExpansionMode,
    body: impl FnOnce() -> T,
) -> T {
    CANDIDATE_EXPANSION_MODE.with(|mode| {
        let previous = mode.replace(selected);
        let _guard = CandidateTestModeGuard {
            cell: mode,
            previous,
        };
        body()
    })
}

/// Test-visible evidence that the concrete route performs bounded byte-region
/// work instead of rebuilding successively shorter recursive values.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ConcreteRegionMatchStats {
    pub authoritative_equal_fast_positives: usize,
    pub byte_region_attempts: usize,
    pub par_region_pairs: usize,
    pub component_region_pairs: usize,
    pub retained_par_cuts: usize,
    pub legacy_fallbacks: usize,
    pub max_pending_tasks: usize,
    pub epathmap_region_levels: usize,
    pub epathmap_endpoint_visits: usize,
    pub epathmap_region_preflight_bypasses: usize,
    pub epathmap_region_backing_copies: usize,
    pub epathmap_region_cut_attempts: usize,
    pub epathmap_direct_expr_decodes: usize,
    pub epathmap_full_decode_fallbacks: usize,
    pub epathmap_leaf_decodes: usize,
}

#[cfg(test)]
fn update_concrete_region_stats(update: impl FnOnce(&mut ConcreteRegionMatchStats)) {
    CONCRETE_REGION_STATS.with(|stats| {
        let mut current = stats.get();
        update(&mut current);
        stats.set(current);
    });
}

#[inline]
fn note_authoritative_equal_fast_positive() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.authoritative_equal_fast_positives += 1);
}

#[inline]
fn note_concrete_region_attempt() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.byte_region_attempts += 1);
}

#[inline]
fn note_concrete_region_fallback() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.legacy_fallbacks += 1);
}

#[inline]
fn note_concrete_par_region_pair() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.par_region_pairs += 1);
}

#[inline]
fn note_concrete_component_region_pair(cuts: usize) {
    #[cfg(test)]
    update_concrete_region_stats(|stats| {
        stats.component_region_pairs += 1;
        stats.retained_par_cuts += cuts;
    });
    #[cfg(not(test))]
    let _ = cuts;
}

#[inline]
fn note_concrete_pending_tasks(pending: usize) {
    #[cfg(test)]
    update_concrete_region_stats(|stats| {
        stats.max_pending_tasks = stats.max_pending_tasks.max(pending);
    });
    #[cfg(not(test))]
    let _ = pending;
}

#[inline]
fn note_epathmap_region_level() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_region_levels += 1);
}

#[inline]
fn note_epathmap_endpoint_visit() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_endpoint_visits += 1);
}

#[inline]
fn note_epathmap_region_preflight_bypass() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_region_preflight_bypasses += 1);
}

#[inline]
fn note_epathmap_region_backing_copy() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_region_backing_copies += 1);
}

#[inline]
fn note_epathmap_region_cut_attempt() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_region_cut_attempts += 1);
}

#[inline]
fn note_epathmap_direct_expr_decodes(count: usize) {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_direct_expr_decodes += count);
    #[cfg(not(test))]
    let _ = count;
}

#[inline]
fn note_epathmap_full_decode_fallback() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_full_decode_fallbacks += 1);
}

#[inline]
fn note_epathmap_leaf_decode() {
    #[cfg(test)]
    update_concrete_region_stats(|stats| stats.epathmap_leaf_decodes += 1);
}

#[cfg(feature = "spatial-matcher-connective-domain-differential")]
thread_local! {
    /// Benchmark-only causal control. The feature is absent from every
    /// default/production build, so candidate filtering cannot become a
    /// runtime consensus configuration.
    static FORCE_CONNECTIVE_DOMAIN_ALL: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
thread_local! {
    /// Benchmark-only causal control for the validated scalar byte-equality
    /// route. This branch is compiled out of every default/production build.
    static FORCE_EXACT_SCALAR_DECODE: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
thread_local! {
    /// Benchmark-only causal control for the former occurrence-expanded,
    /// postorder candidate-mask evaluator.  The branch is absent from every
    /// default/production build.
    static FORCE_OCCURRENCE_CANDIDATE_MASKS: Cell<bool> = const { Cell::new(false) };
}

#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
struct CandidateMaskModeGuard<'a> {
    cell: &'a Cell<bool>,
    previous: bool,
}

#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
impl Drop for CandidateMaskModeGuard<'_> {
    fn drop(&mut self) { self.cell.set(self.previous) }
}

#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
fn with_candidate_mask_mode<T>(optimized: bool, body: impl FnOnce() -> T) -> T {
    FORCE_OCCURRENCE_CANDIDATE_MASKS.with(|force_occurrence| {
        let previous = force_occurrence.replace(!optimized);
        let _guard = CandidateMaskModeGuard {
            cell: force_occurrence,
            previous,
        };
        body()
    })
}

#[cfg(feature = "spatial-matcher-connective-domain-differential")]
struct ConnectiveDomainModeGuard<'a> {
    cell: &'a Cell<bool>,
    previous: bool,
}

#[cfg(feature = "spatial-matcher-connective-domain-differential")]
impl Drop for ConnectiveDomainModeGuard<'_> {
    fn drop(&mut self) { self.cell.set(self.previous) }
}

#[cfg(feature = "spatial-matcher-connective-domain-differential")]
fn with_connective_domain_mode<T>(optimized: bool, body: impl FnOnce() -> T) -> T {
    FORCE_CONNECTIVE_DOMAIN_ALL.with(|force_all| {
        let previous = force_all.replace(!optimized);
        let _guard = ConnectiveDomainModeGuard {
            cell: force_all,
            previous,
        };
        body()
    })
}

#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
struct ExactScalarModeGuard<'a> {
    cell: &'a Cell<bool>,
    previous: bool,
}

#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
impl Drop for ExactScalarModeGuard<'_> {
    fn drop(&mut self) { self.cell.set(self.previous) }
}

#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
fn with_exact_scalar_mode<T>(optimized: bool, body: impl FnOnce() -> T) -> T {
    FORCE_EXACT_SCALAR_DECODE.with(|force_decode| {
        let previous = force_decode.replace(!optimized);
        let _guard = ExactScalarModeGuard {
            cell: force_decode,
            previous,
        };
        body()
    })
}

#[inline]
fn exact_scalar_route_enabled() -> bool {
    #[cfg(feature = "spatial-matcher-exact-scalar-differential")]
    {
        return !FORCE_EXACT_SCALAR_DECODE.with(Cell::get);
    }
    #[cfg(not(feature = "spatial-matcher-exact-scalar-differential"))]
    {
        true
    }
}

/// Benchmark-only production-list entry. Both arms execute the same matcher;
/// the control changes only pure-connective candidate compilation to `All`.
#[cfg(feature = "spatial-matcher-connective-domain-differential")]
pub fn benchmark_match_par_list(
    targets: Vec<Par>,
    patterns: Vec<Par>,
    optimized: bool,
) -> Option<()> {
    with_connective_domain_mode(optimized, || {
        let mut context = SpatialMatcherContext::new();
        let request = ListRequest::new(
            targets.into_iter().map(MatchValue::Par).collect(),
            patterns.into_iter().map(MatchValue::Par).collect(),
            Merger::Identity,
            None,
            false,
        );
        drive_job(&mut context, Job::ListInit(request))
    })
}

/// Benchmark-only remainder guardrail for the same production list PDA.
#[cfg(feature = "spatial-matcher-connective-domain-differential")]
pub fn benchmark_match_send_list_with_remainder(
    targets: Vec<Send>,
    patterns: Vec<Send>,
    remainder: i32,
    optimized: bool,
) -> Option<()> {
    with_connective_domain_mode(optimized, || {
        let mut context = SpatialMatcherContext::new();
        let request = ListRequest::new(
            targets.into_iter().map(MatchValue::Send).collect(),
            patterns.into_iter().map(MatchValue::Send).collect(),
            Merger::Identity,
            Some(remainder),
            false,
        );
        drive_job(&mut context, Job::ListInit(request))
    })
}

/// Benchmark-only direct matcher entry. Both arms execute the production PDA;
/// the control disables only the validated scalar byte-equality transition.
#[cfg(feature = "spatial-matcher-exact-scalar-differential")]
pub fn benchmark_match_par_exact_scalar(target: Par, pattern: Par, optimized: bool) -> Option<()> {
    with_exact_scalar_mode(optimized, || {
        let mut context = SpatialMatcherContext::new();
        drive(&mut context, MatchPair::Par(target, pattern))
    })
}

/// Prebuilt benchmark-only candidate-domain fixture.
///
/// Index and plan construction are deliberately outside timed evaluation: the
/// control freezes the pre-fix occurrence-expanded/postorder evaluator while
/// the treatment uses group masks, pressure scheduling and adaptive ordered
/// expansion.  Charging the new builder sidecars to both arms would neither be
/// a frozen end-to-end control nor isolate the retained-mask mechanism.
#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
pub struct CandidateMaskBenchmarkFixture {
    index: ListTargetIndex,
    plan: ListCandidatePlan,
    target_count: usize,
    optimized: bool,
}

#[cfg(feature = "spatial-matcher-candidate-mask-differential")]
impl CandidateMaskBenchmarkFixture {
    pub fn from_targets_and_pattern(
        targets: Vec<Par>,
        pattern: Par,
        optimized: bool,
    ) -> Option<Self> {
        with_candidate_mask_mode(optimized, || {
            let target_count = targets.len();
            let targets = targets.into_iter().map(MatchValue::Par).collect::<Vec<_>>();
            let index = ListTargetIndex::from_targets(&targets)?;
            let mut captures = PathMap::new();
            let plan = compile_par_candidate_plan(&pattern, &mut captures);
            Some(Self {
                index,
                plan,
                target_count,
                optimized,
            })
        })
    }

    pub fn positions(&self) -> Box<[u64]> {
        with_candidate_mask_mode(self.optimized, || {
            if self.plan.is_empty() {
                return Vec::new().into_boxed_slice();
            }
            self.index
                .positions_matching_plan(&self.plan)
                .unwrap_or_else(|| {
                    (0..self.target_count)
                        .map(|position| {
                            u64::try_from(position).expect("benchmark target position fits u64")
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice()
                })
        })
    }
}

#[cfg(test)]
fn update_list_relation_stats(update: impl FnOnce(&mut RelationalMatchStats)) {
    LIST_RELATION_STATS.with(|stats| {
        let mut current = stats.get();
        update(&mut current);
        stats.set(current);
    });
}

#[cfg(feature = "matcher-decline-counters")]
mod dynamic_root_decline_counters {
    use std::ffi::c_int;
    use std::fmt;
    use std::io::{self, Write};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Once;

    use models::rust::rholang::match_schema_tables::ParCursor;

    use super::{CandidatePlanNodeKind, ListCandidatePlan, MatchValue};
    use crate::rust::interpreter::matcher::projection_rules::declines;

    static ROWS_COMPILED: AtomicU64 = AtomicU64::new(0);
    static DECLINED_NEW: AtomicU64 = AtomicU64::new(0);
    static DECLINED_RECEIVE: AtomicU64 = AtomicU64::new(0);
    static DECLINED_MATCH: AtomicU64 = AtomicU64::new(0);
    static DECLINED_RECEIVE_BIND: AtomicU64 = AtomicU64::new(0);
    static EXIT_REPORT: Once = Once::new();

    extern "C" {
        fn atexit(callback: extern "C" fn()) -> c_int;
    }

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct DynamicRootDeclineCounts {
        pub rows_compiled: u64,
        pub rows_declined: u64,
        pub declined_new: u64,
        pub declined_receive: u64,
        pub declined_match: u64,
        pub declined_receive_bind: u64,
    }

    impl fmt::Display for DynamicRootDeclineCounts {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(
                formatter,
                "matcher-decline-counters rows_compiled={} rows_declined={} declined_new={} \
                 declined_receive={} declined_match={} declined_receive_bind={}",
                self.rows_compiled,
                self.rows_declined,
                self.declined_new,
                self.declined_receive,
                self.declined_match,
                self.declined_receive_bind
            )
        }
    }

    pub fn dynamic_root_decline_counts() -> DynamicRootDeclineCounts {
        let declined_new = DECLINED_NEW.load(Ordering::Relaxed);
        let declined_receive = DECLINED_RECEIVE.load(Ordering::Relaxed);
        let declined_match = DECLINED_MATCH.load(Ordering::Relaxed);
        let declined_receive_bind = DECLINED_RECEIVE_BIND.load(Ordering::Relaxed);
        DynamicRootDeclineCounts {
            rows_compiled: ROWS_COMPILED.load(Ordering::Relaxed),
            rows_declined: declined_new + declined_receive + declined_match + declined_receive_bind,
            declined_new,
            declined_receive,
            declined_match,
            declined_receive_bind,
        }
    }

    extern "C" fn report_at_exit() {
        let _ = writeln!(io::stderr(), "{}", dynamic_root_decline_counts());
    }

    fn register_exit_report() {
        let status = unsafe { atexit(report_at_exit) };
        assert_eq!(
            status, 0,
            "the matcher decline report registers for process exit"
        );
    }

    pub(super) fn count_rows(patterns: &[MatchValue], plans: &[ListCandidatePlan]) {
        EXIT_REPORT.call_once(register_exit_report);
        ROWS_COMPILED.fetch_add(
            u64::try_from(patterns.len()).expect("an AC row count fits in u64"),
            Ordering::Relaxed,
        );
        for (pattern, plan) in patterns.iter().zip(plans) {
            let (root, declined) = match pattern {
                MatchValue::New(value) => (ParCursor::from(value), &DECLINED_NEW),
                MatchValue::Receive(value) => (ParCursor::from(value), &DECLINED_RECEIVE),
                MatchValue::Match(value) => (ParCursor::from(value), &DECLINED_MATCH),
                MatchValue::ReceiveBind(value) => (ParCursor::from(value), &DECLINED_RECEIVE_BIND),
                _ => continue,
            };
            if let (CandidatePlanNodeKind::All, true) = (
                &plan.node(plan.root).kind,
                declines(root.program().type_name),
            ) {
                declined.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

#[cfg(feature = "matcher-decline-counters")]
pub use dynamic_root_decline_counters::{dynamic_root_decline_counts, DynamicRootDeclineCounts};

#[cfg(test)]
pub(super) fn match_par_list_with_stats(
    context: &mut SpatialMatcherContext,
    targets: Vec<Par>,
    patterns: Vec<Par>,
) -> (Option<()>, RelationalMatchStats) {
    LIST_RELATION_STATS.with(|stats| stats.set(RelationalMatchStats::default()));
    let request = ListRequest::new(
        targets.into_iter().map(MatchValue::Par).collect(),
        patterns.into_iter().map(MatchValue::Par).collect(),
        Merger::Identity,
        None,
        false,
    );
    let result = drive_job(context, Job::ListInit(request));
    let stats = LIST_RELATION_STATS.with(Cell::get);
    (result, stats)
}

#[cfg(test)]
pub(super) fn match_par_with_stats(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Par,
) -> (Option<()>, RelationalMatchStats) {
    LIST_RELATION_STATS.with(|stats| stats.set(RelationalMatchStats::default()));
    let result = drive(context, MatchPair::Par(target, pattern));
    let stats = LIST_RELATION_STATS.with(Cell::get);
    (result, stats)
}

#[cfg(test)]
pub(super) fn match_par_with_concrete_region_stats(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Par,
) -> (Option<()>, ConcreteRegionMatchStats) {
    CONCRETE_REGION_STATS.with(|stats| stats.set(ConcreteRegionMatchStats::default()));
    let result = drive(context, MatchPair::Par(target, pattern));
    let stats = CONCRETE_REGION_STATS.with(Cell::get);
    (result, stats)
}

#[cfg(test)]
pub(super) fn match_par_with_transaction_stats(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Par,
) -> (Option<()>, FreeMapTransactionStats) {
    reset_free_map_transaction_stats();
    let result = drive(context, MatchPair::Par(target, pattern));
    (result, free_map_transaction_stats())
}

#[cfg(test)]
pub(super) fn connective_candidate_positions_with_stats(
    targets: Vec<Par>,
    pattern: Par,
    encoded_compiler: bool,
) -> (Box<[u64]>, RelationalMatchStats) {
    LIST_RELATION_STATS.with(|stats| stats.set(RelationalMatchStats::default()));
    let targets = targets.into_iter().map(MatchValue::Par).collect::<Vec<_>>();
    let index = ListTargetIndex::from_targets(&targets)
        .expect("candidate-plan differential targets fit the Bag index");
    let mut captures = PathMap::new();
    let plan = if encoded_compiler {
        compile_par_candidate_plan(&pattern, &mut captures)
    } else {
        compile_candidate_plan(
            vec![CandidateCompileWork::EnterResident(pattern)],
            &mut captures,
        )
    };
    let positions = if plan.is_empty() {
        Vec::new().into_boxed_slice()
    } else {
        index.positions_matching_plan(&plan).unwrap_or_else(|| {
            (0..targets.len())
                .map(|position| u64::try_from(position).expect("target position fits u64"))
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
    };
    let stats = LIST_RELATION_STATS.with(Cell::get);
    (positions, stats)
}

#[cfg(test)]
pub(super) fn connective_candidate_positions_with_node_count_order_stats(
    targets: Vec<Par>,
    pattern: Par,
    encoded_compiler: bool,
) -> (Box<[u64]>, RelationalMatchStats) {
    with_candidate_node_count_order(|| {
        connective_candidate_positions_with_stats(targets, pattern, encoded_compiler)
    })
}

#[cfg(test)]
pub(super) fn connective_candidate_positions_with_expansion_mode_stats(
    targets: Vec<Par>,
    pattern: Par,
    encoded_compiler: bool,
    mode: CandidateExpansionMode,
) -> (Box<[u64]>, RelationalMatchStats) {
    with_candidate_expansion_mode(mode, || {
        connective_candidate_positions_with_stats(targets, pattern, encoded_compiler)
    })
}

#[cfg(test)]
pub(super) fn match_send_list_with_stats(
    context: &mut SpatialMatcherContext,
    targets: Vec<Send>,
    patterns: Vec<Send>,
) -> (Option<()>, RelationalMatchStats) {
    LIST_RELATION_STATS.with(|stats| stats.set(RelationalMatchStats::default()));
    let request = ListRequest::new(
        targets.into_iter().map(MatchValue::Send).collect(),
        patterns.into_iter().map(MatchValue::Send).collect(),
        Merger::Identity,
        None,
        false,
    );
    let result = drive_job(context, Job::ListInit(request));
    let stats = LIST_RELATION_STATS.with(Cell::get);
    (result, stats)
}

#[cfg(test)]
pub(super) fn match_send_list_with_remainder_stats(
    context: &mut SpatialMatcherContext,
    targets: Vec<Send>,
    patterns: Vec<Send>,
    remainder: i32,
) -> (Option<()>, RelationalMatchStats) {
    LIST_RELATION_STATS.with(|stats| stats.set(RelationalMatchStats::default()));
    let request = ListRequest::new(
        targets.into_iter().map(MatchValue::Send).collect(),
        patterns.into_iter().map(MatchValue::Send).collect(),
        Merger::ParSends,
        Some(remainder),
        false,
    );
    let result = drive_job(context, Job::ListInit(request));
    let stats = LIST_RELATION_STATS.with(Cell::get);
    (result, stats)
}

/// Test-only observation of the candidate query compiled for one direct
/// `MatchValue` AC row. `None` is the production fallback-to-All decision;
/// `Some(false)` is the only unsound result when the authoritative matcher
/// accepts the same `(target, pattern)` pair.
#[cfg(test)]
fn direct_match_value_ac_query_acceptance(target: MatchValue, pattern: MatchValue) -> Option<bool> {
    let context = SpatialMatcherContext::new();
    let mut captures = PathMap::new();
    pattern
        .rho_path_query(&context, &mut captures)
        .map(|query| query.matches_structure(&target.rho_path_structure()))
}

#[cfg(test)]
pub(super) fn new_ac_query_acceptance(target: New, pattern: New) -> Option<bool> {
    direct_match_value_ac_query_acceptance(MatchValue::New(target), MatchValue::New(pattern))
}

#[cfg(test)]
pub(super) fn match_ac_query_acceptance(target: Match, pattern: Match) -> Option<bool> {
    direct_match_value_ac_query_acceptance(MatchValue::Match(target), MatchValue::Match(pattern))
}

#[cfg(test)]
pub(super) fn receive_bind_ac_query_acceptance(
    target: ReceiveBind,
    pattern: ReceiveBind,
) -> Option<bool> {
    direct_match_value_ac_query_acceptance(
        MatchValue::ReceiveBind(target),
        MatchValue::ReceiveBind(pattern),
    )
}

#[cfg(test)]
fn direct_match_value_ac_query(pattern: MatchValue) -> Option<RhoPathQuery> {
    let context = SpatialMatcherContext::new();
    let mut captures = PathMap::new();
    pattern.rho_path_query(&context, &mut captures)
}

#[cfg(test)]
pub(super) fn new_ac_query(pattern: New) -> Option<RhoPathQuery> {
    direct_match_value_ac_query(MatchValue::New(pattern))
}

#[cfg(test)]
pub(super) fn receive_ac_query(pattern: Receive) -> Option<RhoPathQuery> {
    direct_match_value_ac_query(MatchValue::Receive(pattern))
}

#[cfg(test)]
pub(super) fn match_ac_query(pattern: Match) -> Option<RhoPathQuery> {
    direct_match_value_ac_query(MatchValue::Match(pattern))
}

#[cfg(test)]
pub(super) fn receive_bind_ac_query(pattern: ReceiveBind) -> Option<RhoPathQuery> {
    direct_match_value_ac_query(MatchValue::ReceiveBind(pattern))
}

#[cfg(all(test, feature = "matcher-decline-counters"))]
pub(super) fn match_dynamic_root_rows_against_themselves(
    news: Vec<New>,
    receives: Vec<Receive>,
    matches: Vec<Match>,
    binds: Vec<ReceiveBind>,
    sends: Vec<Send>,
) -> [Option<()>; 5] {
    [
        news.into_iter().map(MatchValue::New).collect::<Vec<_>>(),
        receives.into_iter().map(MatchValue::Receive).collect(),
        matches.into_iter().map(MatchValue::Match).collect(),
        binds.into_iter().map(MatchValue::ReceiveBind).collect(),
        sends.into_iter().map(MatchValue::Send).collect(),
    ]
    .map(|rows| {
        let request = ListRequest::new(rows.clone(), rows, Merger::Identity, None, false);
        drive_job(&mut SpatialMatcherContext::new(), Job::ListInit(request))
    })
}

#[cfg(test)]
pub(super) fn send_target_index_shape(targets: Vec<Send>) -> (u64, usize) {
    let targets = targets
        .into_iter()
        .map(MatchValue::Send)
        .collect::<Vec<_>>();
    let index = ListTargetIndex::from_targets(&targets)
        .expect("test targets fit the Bag target-index representation");
    (index.bag.cardinality(), index.bag.distinct_len())
}

#[cfg(test)]
pub(super) fn if_target_index_shape(targets: Vec<If>) -> (u64, usize) {
    let targets = targets.into_iter().map(MatchValue::If).collect::<Vec<_>>();
    let index = ListTargetIndex::from_targets(&targets)
        .expect("test targets fit the Bag target-index representation");
    (index.bag.cardinality(), index.bag.distinct_len())
}

pub(super) fn match_par_pair(
    context: &mut SpatialMatcherContext,
    target: (Par, Par),
    pattern: (Par, Par),
) -> Option<()> {
    drive(context, MatchPair::ParPair(target, pattern))
}

pub(super) fn match_connective(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Connective,
) -> Option<()> {
    drive(context, MatchPair::Connective(target, pattern))
}

macro_rules! pda_entry {
    ($name:ident, $variant:ident, $ty:ty) => {
        pub(super) fn $name(
            context: &mut SpatialMatcherContext,
            target: $ty,
            pattern: $ty,
        ) -> Option<()> {
            drive(context, MatchPair::$variant(target, pattern))
        }
    };
}

pda_entry!(match_bundle, Bundle, Bundle);
pda_entry!(match_send, Send, Send);
pda_entry!(match_receive, Receive, Receive);
pda_entry!(match_new, New, New);
pda_entry!(match_expr, Expr, Expr);
pda_entry!(match_match, Match, Match);
pda_entry!(match_unforgeable, Unforgeable, GUnforgeable);
pda_entry!(match_if, If, If);
pda_entry!(match_receive_bind, ReceiveBind, ReceiveBind);
pda_entry!(match_case, MatchCase, MatchCase);

/// One target owner shared by a synchronous encoded-connective PDA spine.
///
/// The matcher machine is already thread-local (`ListMachine` uses `Rc`), so
/// an `Arc<Par>` would add atomic traffic without enabling a cross-thread
/// boundary. `ParBag` and PathMap retain their own thread-safe CoW ownership.
type EncodedTarget = Rc<Par>;
type EncodedConnectiveChildren = CanonicalOwnedConnectiveChildren<EncodedRegion>;

enum Job {
    Match(Box<MatchPair>),
    EncodedPar(EncodedRegion, EncodedRegion),
    EncodedConnective {
        target: EncodedTarget,
        pattern: EncodedRegion,
    },
    EncodedIf(EncodedRegion, EncodedRegion),
    EncodedNew(EncodedRegion, EncodedRegion),
    EncodedSend(EncodedRegion, EncodedRegion),
    EncodedReceive(EncodedRegion, EncodedRegion),
    EncodedMatch(EncodedRegion, EncodedRegion),
    EncodedExpr(EncodedRegion, EncodedRegion),
    ProtobufPar(Box<(ProtobufRegion, ProtobufRegion)>),
    ProtobufExpr(Box<(ProtobufRegion, ProtobufRegion)>),
    ParConnectives(Box<ParConnectiveState>),
    ConnectiveCandidates(Box<ConnectiveCandidates>),
    ListInit(ListRequest),
    List(Box<ListMachine>),
    PathInit(Box<(EPathMap, EPathMap)>),
    Path(Box<PathMachine>),
    Return(bool),
}

enum Frame {
    Sequence {
        remaining: Vec<Job>,
    },
    RestoreOnFailure {
        checkpoint: FreeMapCheckpoint,
    },
    AndBranch {
        target: Box<Par>,
        patterns: std::vec::IntoIter<Par>,
    },
    OrBranch {
        target: Option<Box<Par>>,
        patterns: std::vec::IntoIter<Par>,
        checkpoint: FreeMapCheckpoint,
    },
    EncodedAndBranch {
        target: EncodedTarget,
        children: EncodedConnectiveChildren,
    },
    EncodedOrBranch {
        target: Option<EncodedTarget>,
        children: EncodedConnectiveChildren,
        checkpoint: FreeMapCheckpoint,
    },
    Not {
        checkpoint: FreeMapCheckpoint,
    },
    NotSpine {
        checkpoint: FreeMapCheckpoint,
        negated: bool,
    },
    EncodedIf {
        branches: Box<EncodedIfBranches>,
    },
    EncodedSibling {
        sibling: Box<(EncodedRegion, EncodedRegion)>,
    },
    ProtobufSibling {
        sibling: Box<(ProtobufRegion, ProtobufRegion)>,
    },
    BindOnSuccess {
        level: i32,
        value: Box<Par>,
    },
    ConnectiveCandidate {
        state: Box<ConnectiveCandidates>,
        checkpoint: Option<FreeMapCheckpoint>,
        remainder: Box<Par>,
    },
    ListEdge {
        machine: Box<ListMachine>,
        target_index: usize,
        candidate_was_seen: bool,
        checkpoint: FreeMapCheckpoint,
    },
    PathEdge {
        machine: Box<PathMachine>,
        target_key: Vec<u8>,
        checkpoint: FreeMapCheckpoint,
    },
}

struct EncodedIfBranches {
    if_true: Option<(EncodedRegion, EncodedRegion)>,
    if_false: (EncodedRegion, EncodedRegion),
}

/// Contiguous heap continuation stack. `Frame` never owns another `Frame`, so
/// normal unwinding and early failure both dismantle it iteratively.
struct FrameStack(Vec<Frame>);

impl FrameStack {
    fn new() -> Self { Self(Vec::new()) }

    fn push(&mut self, frame: Frame) { self.0.push(frame); }

    fn pop(&mut self) -> Option<Frame> { self.0.pop() }

    #[cfg(test)]
    fn len(&self) -> usize { self.0.len() }
}

enum MatchPair {
    Par(Par, Par),
    ParPair((Par, Par), (Par, Par)),
    Connective(Par, Connective),
    Bundle(Bundle, Bundle),
    Send(Send, Send),
    Receive(Receive, Receive),
    New(New, New),
    Expr(Expr, Expr),
    Match(Match, Match),
    Unforgeable(GUnforgeable, GUnforgeable),
    If(If, If),
    ReceiveBind(ReceiveBind, ReceiveBind),
    MatchCase(MatchCase, MatchCase),
}

/// An immutable range inside one canonical protobuf component body.
///
/// Descents clone only the `Arc` header and adjust offsets. A deeply nested
/// schema walk therefore retains one allocation per top-level ParBag endpoint,
/// not one decoded AST or copied suffix per level.
#[derive(Clone)]
struct EncodedRegion {
    // `Arc<Vec<u8>>` deliberately uses a thin Arc pointer. An `Arc<[u8]>` is a
    // fat pointer and made a pair of ranges widen `Job` beyond one cache line.
    // The Vec is never mutated after publication; descendants share it exactly
    // as immutable CoW storage.
    bytes: Arc<Vec<u8>>,
    start: usize,
    end: usize,
}

impl PartialEq for EncodedRegion {
    fn eq(&self, other: &Self) -> bool { self.as_slice() == other.as_slice() }
}

impl Eq for EncodedRegion {}

impl PartialOrd for EncodedRegion {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for EncodedRegion {
    fn cmp(&self, other: &Self) -> Ordering { self.as_slice().cmp(other.as_slice()) }
}

impl EncodedRegion {
    fn whole(bytes: Arc<Vec<u8>>) -> Self {
        let end = bytes.len();
        Self {
            bytes,
            start: 0,
            end,
        }
    }

    fn as_slice(&self) -> &[u8] { &self.bytes[self.start..self.end] }

    fn child(&self, child: &[u8]) -> Option<Self> {
        let root_start = self.as_slice().as_ptr() as usize;
        let child_start = child.as_ptr() as usize;
        let relative = child_start.checked_sub(root_start)?;
        let start = self.start.checked_add(relative)?;
        let end = start.checked_add(child.len())?;
        (start >= self.start && end <= self.end).then(|| Self {
            bytes: Arc::clone(&self.bytes),
            start,
            end,
        })
    }

    fn child_range(&self, child: std::ops::Range<usize>) -> Option<Self> {
        (child.start <= child.end).then_some(())?;
        let start = self.start.checked_add(child.start)?;
        let end = self.start.checked_add(child.end)?;
        (start >= self.start && end <= self.end).then(|| Self {
            bytes: Arc::clone(&self.bytes),
            start,
            end,
        })
    }
}

impl AsRef<[u8]> for EncodedRegion {
    fn as_ref(&self) -> &[u8] { self.as_slice() }
}

// SAFETY: its private Arc<Vec<u8>> and range are never mutated after publication.
unsafe impl StableConnectiveBodyOwner for EncodedRegion {}

fn drive(context: &mut SpatialMatcherContext, root: MatchPair) -> Option<()> {
    drive_job(context, match_job(root))
}

fn drive_job(context: &mut SpatialMatcherContext, mut job: Job) -> Option<()> {
    let mut transaction = FreeMapTransaction::new(context);
    let mut frames = FrameStack::new();

    loop {
        job = match job {
            Job::Match(pair) => evaluate_pair(&mut transaction, *pair, &mut frames),
            Job::EncodedPar(target, pattern) => evaluate_encoded_par(target, pattern),
            Job::EncodedConnective { target, pattern } => {
                evaluate_encoded_connective(&mut transaction, target, pattern, &mut frames)
            }
            Job::EncodedIf(target, pattern) => evaluate_encoded_if(target, pattern, &mut frames),
            Job::EncodedNew(target, pattern) => evaluate_encoded_new(target, pattern),
            Job::EncodedSend(target, pattern) => {
                evaluate_encoded_send(target, pattern, &mut frames)
            }
            Job::EncodedReceive(target, pattern) => {
                evaluate_encoded_receive(target, pattern, &mut frames)
            }
            Job::EncodedMatch(target, pattern) => {
                evaluate_encoded_match(target, pattern, &mut frames)
            }
            Job::EncodedExpr(target, pattern) => {
                evaluate_encoded_expr(target, pattern, &mut frames)
            }
            Job::ProtobufPar(pair) => {
                let (target, pattern) = *pair;
                evaluate_protobuf_par(&mut transaction, target, pattern)
            }
            Job::ProtobufExpr(pair) => {
                let (target, pattern) = *pair;
                evaluate_protobuf_expr(&mut transaction, target, pattern, &mut frames)
            }
            Job::ParConnectives(state) => step_par_connectives(*state, &mut frames),
            Job::ConnectiveCandidates(state) => {
                step_connective_candidates(&mut transaction, *state, &mut frames)
            }
            Job::ListInit(request) => init_list(&mut transaction, request, &mut frames),
            Job::List(machine) => step_list(&mut transaction, *machine, &mut frames),
            Job::PathInit(pair) => {
                let (target, pattern) = *pair;
                init_path(&mut transaction, target, pattern, &mut frames)
            }
            Job::Path(machine) => step_path(&mut transaction, *machine, &mut frames),
            Job::Return(result) => match frames.pop() {
                Some(frame) => resume(&mut transaction, frame, result, &mut frames),
                None => return transaction.finish(result),
            },
        };
    }
}

fn resume(
    context: &mut FreeMapTransaction<'_>,
    frame: Frame,
    result: bool,
    frames: &mut FrameStack,
) -> Job {
    match frame {
        Frame::Sequence { mut remaining } => {
            if !result {
                Job::Return(false)
            } else if let Some(next) = remaining.pop() {
                frames.push(Frame::Sequence { remaining });
                next
            } else {
                Job::Return(true)
            }
        }
        Frame::RestoreOnFailure { checkpoint } => {
            if !result {
                context.rollback(checkpoint);
            }
            Job::Return(result)
        }
        Frame::AndBranch { target, patterns } => {
            if result {
                start_and_branch(*target, patterns, frames)
            } else {
                Job::Return(false)
            }
        }
        Frame::OrBranch {
            target,
            patterns,
            checkpoint,
        } => {
            context.rollback(checkpoint);
            if result {
                Job::Return(true)
            } else if let Some(target) = target {
                start_or_branch(context, *target, patterns, frames)
            } else {
                debug_assert!(patterns.len() == 0);
                Job::Return(false)
            }
        }
        Frame::EncodedAndBranch { target, children } => {
            if result {
                start_encoded_and_branch(context, target, children, frames)
            } else {
                Job::Return(false)
            }
        }
        Frame::EncodedOrBranch {
            target,
            children,
            checkpoint,
        } => {
            context.rollback(checkpoint);
            if result {
                Job::Return(true)
            } else if let Some(target) = target {
                start_encoded_or_branch(context, target, children, frames)
            } else {
                debug_assert!(children.is_empty());
                Job::Return(false)
            }
        }
        Frame::Not { checkpoint } => {
            context.rollback(checkpoint);
            Job::Return(!result)
        }
        Frame::NotSpine {
            checkpoint,
            negated,
        } => {
            context.rollback(checkpoint);
            Job::Return(if negated { !result } else { result })
        }
        Frame::EncodedIf { mut branches } => {
            if !result {
                Job::Return(false)
            } else if let Some((target, pattern)) = branches.if_true.take() {
                frames.push(Frame::EncodedIf { branches });
                Job::EncodedPar(target, pattern)
            } else {
                let (target, pattern) = branches.if_false;
                Job::EncodedPar(target, pattern)
            }
        }
        Frame::EncodedSibling { sibling } => {
            if result {
                let (target, pattern) = *sibling;
                Job::EncodedPar(target, pattern)
            } else {
                Job::Return(false)
            }
        }
        Frame::ProtobufSibling { sibling } => {
            if result {
                Job::ProtobufPar(sibling)
            } else {
                Job::Return(false)
            }
        }
        Frame::BindOnSuccess { level, value } => {
            if result {
                context.write(level, *value);
            }
            Job::Return(result)
        }
        Frame::ConnectiveCandidate {
            state,
            checkpoint,
            remainder,
        } => {
            if let Some(checkpoint) = checkpoint {
                if !result {
                    context.rollback(checkpoint);
                }
            }
            if result {
                let mut parent = state.parent;
                parent.target = *remainder;
                Job::ParConnectives(Box::new(parent))
            } else {
                Job::ConnectiveCandidates(state)
            }
        }
        Frame::ListEdge {
            mut machine,
            target_index,
            candidate_was_seen,
            checkpoint,
        } => {
            if result {
                let produced = context.take_delta_and_rollback(checkpoint);
                #[cfg(test)]
                update_list_relation_stats(|stats| {
                    stats.successful_edge_evaluations += 1;
                });
                let pattern_index = match &machine
                    .search
                    .last()
                    .expect("a suspended list edge has a search frame")
                    .cursor
                {
                    ListSearchCursor::Term { pattern_index, .. } => *pattern_index,
                    ListSearchCursor::Remainder { .. } => {
                        unreachable!("a remainder edge never suspends in the spatial matcher")
                    }
                };
                let produced = if produced.is_empty() {
                    machine.relation.record_shared_success(
                        pattern_index,
                        target_index,
                        Rc::clone(&machine.empty_delta),
                    )
                } else {
                    machine
                        .relation
                        .record_success(pattern_index, target_index, produced)
                };
                if candidate_was_seen {
                    Job::List(machine)
                } else {
                    (*machine).accept_term_edge(target_index, EdgeResult::Shared(produced))
                }
            } else {
                context.rollback(checkpoint);
                Job::List(machine)
            }
        }
        Frame::PathEdge {
            machine,
            target_key,
            checkpoint,
        } => {
            if result {
                let produced = context.take_delta_and_rollback(checkpoint);
                (*machine).accept_edge(target_key, produced)
            } else {
                context.rollback(checkpoint);
                Job::Path(machine)
            }
        }
    }
}

#[inline]
fn match_job(pair: MatchPair) -> Job { Job::Match(Box::new(pair)) }

fn start_jobs(mut jobs: Vec<Job>, frames: &mut FrameStack) -> Job {
    jobs.reverse();
    match jobs.pop() {
        Some(first) => {
            frames.push(Frame::Sequence { remaining: jobs });
            first
        }
        None => Job::Return(true),
    }
}

fn pair_jobs(pairs: impl IntoIterator<Item = MatchPair>) -> Vec<Job> {
    pairs.into_iter().map(match_job).collect()
}

fn evaluate_pair(
    context: &mut FreeMapTransaction<'_>,
    pair: MatchPair,
    frames: &mut FrameStack,
) -> Job {
    match pair {
        MatchPair::Par(target, pattern) => evaluate_par(context, target, pattern, frames),
        MatchPair::ParPair((tk, tv), (pk, pv)) => start_jobs(
            pair_jobs([MatchPair::Par(tk, pk), MatchPair::Par(tv, pv)]),
            frames,
        ),
        MatchPair::Connective(target, pattern) => {
            evaluate_connective(context, target, pattern, frames)
        }
        MatchPair::Bundle(target, pattern) => Job::Return(target == pattern),
        MatchPair::Send(target, pattern) => evaluate_send(target, pattern, frames),
        MatchPair::Receive(target, pattern) => evaluate_receive(target, pattern, frames),
        MatchPair::New(target, pattern) => evaluate_new(target, pattern),
        MatchPair::Expr(target, pattern) => evaluate_expr(target, pattern, frames),
        MatchPair::Match(target, pattern) => evaluate_match(target, pattern, frames),
        MatchPair::Unforgeable(target, pattern) => evaluate_unforgeable(target, pattern),
        MatchPair::If(target, pattern) => evaluate_if(target, pattern, frames),
        MatchPair::ReceiveBind(target, pattern) => evaluate_receive_bind(target, pattern),
        MatchPair::MatchCase(target, pattern) => evaluate_match_case(target, pattern),
    }
}

fn singleton_component_region<T: ParBagComponent>(par: &Par) -> Option<EncodedRegion> {
    if par.family_cardinality::<T>() != 1 || !par.all_components_are::<T>() {
        return None;
    }
    let mut result = None;
    par.visit_encoded::<T>(|entry| {
        if entry.multiplicity().get() == 1 {
            result = Some(EncodedRegion::whole(Arc::new(entry.payload().to_vec())));
        }
    });
    result
}

struct SingletonEPathMapEndpoint {
    component: OwnedCanonicalParComponent<Expr>,
    route_hint: CanonicalEPathMapRootRouteHint,
}

impl SingletonEPathMapEndpoint {
    fn into_epathmap(self) -> EPathMap {
        let expr = self
            .component
            .decode()
            .expect("an authoritative singleton EPathMap Expr is canonical");
        match expr.expr_instance {
            Some(EPathmapBody(path)) => path,
            _ => unreachable!("the canonical root view proved an EPathMap Expr"),
        }
    }
}

/// Prove that one singleton Expr endpoint has an EPathMap envelope without
/// copying its potentially wide protobuf body.
fn singleton_epathmap_route_hint(par: &Par) -> Option<CanonicalEPathMapRootRouteHint> {
    if par.family_cardinality::<Expr>() != 1 || !par.all_components_are::<Expr>() {
        return None;
    }
    let mut result = None;
    par.visit_encoded::<Expr>(|entry| {
        if entry.multiplicity().get() == 1 {
            result = canonical_epathmap_root_view(entry.payload()).map(|view| view.route_hint());
        }
    });
    result
}

/// Own one proved singleton EPathMap endpoint after one ParBag zipper visit.
///
/// The callback borrows the ParBag zipper's reused path buffer, so it performs
/// exactly one copy of the authoritative Expr and no decode. Complete generated
/// validation occurs only after the callback returns and the zipper releases
/// its PathMap cursor and path buffer. This prevents a multi-megabyte decoded
/// EPathMap from overlapping the live source zipper while retaining one visit,
/// one copy, and the native PathMap representation. The route hint came from a
/// preceding non-consuming visit to the same immutable `Par`; keeping it as an
/// argument avoids reparsing the envelope during the ownership visit.
fn singleton_epathmap_endpoint(
    par: &Par,
    route_hint: CanonicalEPathMapRootRouteHint,
) -> Option<SingletonEPathMapEndpoint> {
    if par.family_cardinality::<Expr>() != 1 || !par.all_components_are::<Expr>() {
        return None;
    }
    let mut result = None;
    par.visit_encoded::<Expr>(|entry| {
        if entry.multiplicity().get() != 1 {
            return;
        }
        note_epathmap_endpoint_visit();
        result = Some(SingletonEPathMapEndpoint {
            component: entry.to_owned_canonical(),
            route_hint,
        });
        note_epathmap_region_backing_copy();
    });
    result
}

/// Whether the whole target is byte-for-byte equivalent to the sole candidate
/// that the legacy `sub_pars` projection would expose to a root Connective.
///
/// `sub_pars` partitions only the eight concrete component families: it drops
/// the Connective family and clears both root summaries.  The direct encoded
/// route may therefore skip that projection only when all three transformations
/// are already identities.  Each condition is an O(1) cached `Par`/`ParBag`
/// query; an unproved target falls through to the complete generic machine.
fn whole_target_is_connective_projection_identity(target: &Par) -> bool {
    target.family_cardinality::<Connective>() == 0
        && !target.connective_used()
        && target.locally_free().is_empty()
}

fn sole_free_var_level(pattern: &Par) -> Option<i32> {
    if pattern.family_cardinality::<Expr>() != 1 || !pattern.all_components_are::<Expr>() {
        return None;
    }
    let mut level = None;
    pattern.visit_encoded::<Expr>(|entry| {
        if entry.multiplicity().get() == 1 {
            level = canonical_exact_free_var_expr_level(entry.payload());
        }
    });
    level
}

fn concrete_target_components_are_locally_closed(target: &Par) -> bool {
    target.concrete_components_locally_free_are_empty() == Some(true)
}

fn sole_free_var_capture_is_admissible(
    context: &FreeMapTransaction<'_>,
    target: &Par,
    level: i32,
) -> bool {
    let prior_is_projection_identity = context
        .free_map()
        .get(&level)
        .map(whole_target_is_connective_projection_identity)
        .unwrap_or(true);
    prior_is_projection_identity
        && whole_target_is_connective_projection_identity(target)
        && concrete_target_components_are_locally_closed(target)
}

/// A nested `Par` is not compared under one universal equation. The concrete
/// matcher deliberately ignores component metadata only along `New.p` and the
/// recursive receive/bind edges. Every other component field uses ordinary
/// generated equality, whose nested `Par` leaves use `Par::eq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConcreteParEquation {
    MatchPars,
    ParEq,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConcreteComponentFamily {
    Send,
    Receive,
    New,
    Expr,
    Match,
    GUnforgeable,
    Bundle,
    Connective,
    If,
}

impl ConcreteComponentFamily {
    fn program(self) -> &'static RhoMatchProgram {
        match self {
            Self::Send => &SEND_RHO_MATCH_PROGRAM,
            Self::Receive => &RECEIVE_RHO_MATCH_PROGRAM,
            Self::New => &NEW_RHO_MATCH_PROGRAM,
            Self::Expr => &EXPR_RHO_MATCH_PROGRAM,
            Self::Match => &MATCH_RHO_MATCH_PROGRAM,
            Self::GUnforgeable => &GUNFORGEABLE_RHO_MATCH_PROGRAM,
            Self::Bundle => &BUNDLE_RHO_MATCH_PROGRAM,
            Self::Connective => &CONNECTIVE_RHO_MATCH_PROGRAM,
            Self::If => &IF_RHO_MATCH_PROGRAM,
        }
    }
}

#[derive(Clone)]
enum ConcreteRegionTask {
    Par {
        target: ProtobufRegion,
        pattern: ProtobufRegion,
        equation: ConcreteParEquation,
    },
    Component {
        target: ProtobufRegion,
        pattern: ProtobufRegion,
        family: ConcreteComponentFamily,
        equation: ConcreteParEquation,
    },
}

struct ConcreteCut {
    path: Vec<ProtobufRegionStep>,
    region: ProtobufRegion,
    equation: ConcreteParEquation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReceiveCutAddress {
    group: u8,
    bind: usize,
    field: u8,
    occurrence: usize,
}

impl ReceiveCutAddress {
    #[inline]
    const fn is_body(self) -> bool { self.group == 1 }
}

#[inline]
fn root_step_is(path: &[ProtobufRegionStep], program: &'static RhoMatchProgram, tag: u32) -> bool {
    path.first()
        .map(|step| step.owner_type_id == program.type_id && step.field_tag == tag)
        .unwrap_or(false)
}

/// Convert the retained cuts into exactly the recursive obligations of the
/// selected equality equation. `Receive.condition` is intentionally absent:
/// the historical concrete matcher does not inspect it. This omission is
/// scoped to `MatchPars`; generated equality continues to compare it.
fn concrete_cuts(
    plan: &ProtobufCutPlan,
    family: ConcreteComponentFamily,
    equation: ConcreteParEquation,
) -> Option<Vec<ConcreteCut>> {
    let mut cuts = Vec::with_capacity(plan.cuts().len());
    for (index, cut) in plan.cuts().iter().enumerate() {
        let path = plan.cut_path(index)?;
        if equation == ConcreteParEquation::MatchPars
            && family == ConcreteComponentFamily::Receive
            && root_step_is(&path, &RECEIVE_RHO_MATCH_PROGRAM, 8)
        {
            continue;
        }
        let nested_equation = match (equation, family) {
            (ConcreteParEquation::MatchPars, ConcreteComponentFamily::New)
                if root_step_is(&path, &NEW_RHO_MATCH_PROGRAM, 2) =>
            {
                ConcreteParEquation::MatchPars
            }
            (ConcreteParEquation::MatchPars, ConcreteComponentFamily::Receive) => {
                ConcreteParEquation::MatchPars
            }
            _ => ConcreteParEquation::ParEq,
        };
        cuts.push(ConcreteCut {
            path,
            region: cut.region().clone(),
            equation: nested_equation,
        });
    }
    Some(cuts)
}

fn receive_cut_address(cut: &ConcreteCut) -> Option<ReceiveCutAddress> {
    let root = cut.path.first()?;
    if root.owner_type_id != RECEIVE_RHO_MATCH_PROGRAM.type_id {
        return None;
    }
    match root.field_tag {
        1 => {
            let bind_field = cut.path.get(1)?;
            match bind_field.field_tag {
                1 => Some(ReceiveCutAddress {
                    group: 0,
                    bind: root.occurrence,
                    field: 0,
                    occurrence: bind_field.occurrence,
                }),
                2 => Some(ReceiveCutAddress {
                    group: 0,
                    bind: root.occurrence,
                    field: 1,
                    occurrence: 0,
                }),
                _ => None,
            }
        }
        2 => Some(ReceiveCutAddress {
            group: 1,
            bind: 0,
            field: 0,
            occurrence: 0,
        }),
        _ => None,
    }
}

/// Preserve the two historical `zip` equations exactly: surplus receive
/// binds, and surplus patterns inside a paired bind, are ignored. Canonical
/// cut paths expose both occurrence indexes, so the intersection can be
/// streamed without decoding or expanding the recursive `Par` bodies.
fn pair_receive_match_cuts(
    target: Vec<ConcreteCut>,
    pattern: Vec<ConcreteCut>,
) -> Option<Vec<ConcreteRegionTask>> {
    let (mut target_at, mut pattern_at) = (0usize, 0usize);
    let mut paired = Vec::new();
    while target_at < target.len() && pattern_at < pattern.len() {
        let target_address = receive_cut_address(&target[target_at])?;
        let pattern_address = receive_cut_address(&pattern[pattern_at])?;
        match target_address.cmp(&pattern_address) {
            Ordering::Equal => {
                if target[target_at].path != pattern[pattern_at].path {
                    return None;
                }
                paired.push(ConcreteRegionTask::Par {
                    target: target[target_at].region.clone(),
                    pattern: pattern[pattern_at].region.clone(),
                    equation: ConcreteParEquation::MatchPars,
                });
                target_at += 1;
                pattern_at += 1;
            }
            Ordering::Less if !target_address.is_body() => {
                target_at += 1;
            }
            Ordering::Greater if !pattern_address.is_body() => {
                pattern_at += 1;
            }
            _ => return None,
        }
    }
    for cut in target[target_at..].iter().chain(&pattern[pattern_at..]) {
        if receive_cut_address(cut)?.is_body() {
            return None;
        }
    }
    Some(paired)
}

fn compare_concrete_component_shells(
    family: ConcreteComponentFamily,
    equation: ConcreteParEquation,
    target: &[u8],
    pattern: &[u8],
) -> Option<bool> {
    use models::rust::rholang::protobuf_decoder;

    macro_rules! compare_shell {
        ($decode:ident, $legacy:ident) => {{
            let target = protobuf_decoder::$decode(target).ok()?;
            let pattern = protobuf_decoder::$decode(pattern).ok()?;
            Some(match equation {
                ConcreteParEquation::MatchPars => $legacy(&target, &pattern),
                ConcreteParEquation::ParEq => target == pattern,
            })
        }};
    }

    match family {
        ConcreteComponentFamily::Send => {
            compare_shell!(decode_send, compare_sends_without_locally_free)
        }
        ConcreteComponentFamily::Receive => {
            compare_shell!(decode_receive, compare_receives_without_locally_free)
        }
        ConcreteComponentFamily::New => {
            compare_shell!(decode_new, compare_news_without_locally_free)
        }
        ConcreteComponentFamily::Expr => {
            compare_shell!(decode_expr, compare_exprs_without_locally_free)
        }
        ConcreteComponentFamily::Match => {
            compare_shell!(decode_match, compare_matches_without_locally_free)
        }
        ConcreteComponentFamily::GUnforgeable => compare_shell!(
            decode_g_unforgeable,
            compare_unforgeables_without_locally_free
        ),
        ConcreteComponentFamily::Bundle => {
            compare_shell!(decode_bundle, compare_bundles_without_locally_free)
        }
        ConcreteComponentFamily::Connective => {
            compare_shell!(decode_connective, compare_connectives_without_locally_free)
        }
        ConcreteComponentFamily::If => {
            compare_shell!(decode_if, compare_ifs_without_locally_free)
        }
    }
}

fn compare_concrete_component_regions(
    target: ProtobufRegion,
    pattern: ProtobufRegion,
    family: ConcreteComponentFamily,
    equation: ConcreteParEquation,
    pending: &mut Vec<ConcreteRegionTask>,
) -> Option<bool> {
    if target == pattern {
        note_concrete_component_region_pair(0);
        return Some(true);
    }

    let target_plan = cut_at_par(target, family.program()).ok()?;
    let pattern_plan = cut_at_par(pattern, family.program()).ok()?;
    note_concrete_component_region_pair(target_plan.cuts().len() + pattern_plan.cuts().len());
    if !compare_concrete_component_shells(
        family,
        equation,
        target_plan.shallow_bytes().as_ref(),
        pattern_plan.shallow_bytes().as_ref(),
    )? {
        return Some(false);
    }

    let target_cuts = concrete_cuts(&target_plan, family, equation)?;
    let pattern_cuts = concrete_cuts(&pattern_plan, family, equation)?;
    if equation == ConcreteParEquation::MatchPars && family == ConcreteComponentFamily::Receive {
        let nested = pair_receive_match_cuts(target_cuts, pattern_cuts)?;
        pending.extend(nested.into_iter().rev());
        note_concrete_pending_tasks(pending.len());
        return Some(true);
    }
    if target_cuts.len() != pattern_cuts.len() {
        return None;
    }
    let mut nested = Vec::with_capacity(target_cuts.len());
    for (target, pattern) in target_cuts.into_iter().zip(pattern_cuts) {
        if target.path != pattern.path || target.equation != pattern.equation {
            return None;
        }
        nested.push(ConcreteRegionTask::Par {
            target: target.region,
            pattern: pattern.region,
            equation: target.equation,
        });
    }
    pending.extend(nested.into_iter().rev());
    note_concrete_pending_tasks(pending.len());
    Some(true)
}

fn enqueue_delimited_region_run(
    target: Option<&models::rust::rholang::protobuf_cursor::CanonicalRegionDelimitedRun>,
    pattern: Option<&models::rust::rholang::protobuf_cursor::CanonicalRegionDelimitedRun>,
    family: ConcreteComponentFamily,
    equation: ConcreteParEquation,
    tasks: &mut Vec<ConcreteRegionTask>,
) -> Option<bool> {
    let mut target = target.map(|run| run.cursor());
    let mut pattern = pattern.map(|run| run.cursor());
    loop {
        let target_region = match target.as_mut() {
            Some(cursor) => cursor.next_region().ok()?,
            None => None,
        };
        let pattern_region = match pattern.as_mut() {
            Some(cursor) => cursor.next_region().ok()?,
            None => None,
        };
        match (target_region, pattern_region) {
            (Some(target), Some(pattern)) => tasks.push(ConcreteRegionTask::Component {
                target,
                pattern,
                family,
                equation,
            }),
            (None, None) => return Some(true),
            _ => return Some(false),
        }
    }
}

fn compare_concrete_par_regions(
    target: ProtobufRegion,
    pattern: ProtobufRegion,
    equation: ConcreteParEquation,
    pending: &mut Vec<ConcreteRegionTask>,
) -> Option<bool> {
    note_concrete_par_region_pair();
    if target == pattern {
        return Some(true);
    }
    let target = CanonicalParRegionIndex::parse(target).ok()?;
    let pattern = CanonicalParRegionIndex::parse(pattern).ok()?;
    if target.connective_used != pattern.connective_used {
        return Some(false);
    }

    let mut components = Vec::new();
    macro_rules! enqueue_family {
        ($field:ident, $family:ident) => {
            if !enqueue_delimited_region_run(
                target.$field.as_ref(),
                pattern.$field.as_ref(),
                ConcreteComponentFamily::$family,
                equation,
                &mut components,
            )? {
                return Some(false);
            }
        };
    }
    // Preserve the legacy concrete matcher's observable family decision order.
    enqueue_family!(sends, Send);
    enqueue_family!(exprs, Expr);
    enqueue_family!(matches, Match);
    enqueue_family!(unforgeables, GUnforgeable);
    enqueue_family!(bundles, Bundle);
    enqueue_family!(connectives, Connective);
    enqueue_family!(conditionals, If);
    enqueue_family!(receives, Receive);
    enqueue_family!(news, New);
    pending.extend(components.into_iter().rev());
    note_concrete_pending_tasks(pending.len());
    Some(true)
}

fn resident_component_runs<T: ParBagComponent>(par: &Par) -> Vec<(ProtobufRegion, u64)> {
    let mut runs = Vec::new();
    par.visit_encoded::<T>(|entry| {
        runs.push((
            ProtobufRegion::copy_from_slice(entry.payload()),
            entry.multiplicity().get(),
        ));
    });
    runs
}

fn enqueue_resident_family<T: ParBagComponent>(
    target: &Par,
    pattern: &Par,
    family: ConcreteComponentFamily,
    tasks: &mut Vec<ConcreteRegionTask>,
) -> bool {
    let target = resident_component_runs::<T>(target);
    let pattern = resident_component_runs::<T>(pattern);
    let (mut target_index, mut pattern_index) = (0usize, 0usize);
    let (mut target_left, mut pattern_left) = (0u64, 0u64);
    while target_index < target.len() && pattern_index < pattern.len() {
        if target_left == 0 {
            target_left = target[target_index].1;
        }
        if pattern_left == 0 {
            pattern_left = pattern[pattern_index].1;
        }
        let paired = target_left.min(pattern_left);
        debug_assert_ne!(paired, 0);
        tasks.push(ConcreteRegionTask::Component {
            target: target[target_index].0.clone(),
            pattern: pattern[pattern_index].0.clone(),
            family,
            equation: ConcreteParEquation::MatchPars,
        });
        target_left -= paired;
        pattern_left -= paired;
        if target_left == 0 {
            target_index += 1;
        }
        if pattern_left == 0 {
            pattern_index += 1;
        }
    }
    target_index == target.len()
        && pattern_index == pattern.len()
        && target_left == 0
        && pattern_left == 0
}

/// Byte-region implementation of the concrete equations in `match_pars`.
/// `None` is a conservative request for the legacy PDA, never a negative.
fn match_concrete_par_regions(target: &Par, pattern: &Par) -> Option<bool> {
    note_concrete_region_attempt();
    if target.family_cardinality::<Send>() != pattern.family_cardinality::<Send>()
        || target.family_cardinality::<Receive>() != pattern.family_cardinality::<Receive>()
        || target.family_cardinality::<New>() != pattern.family_cardinality::<New>()
        || target.family_cardinality::<Expr>() != pattern.family_cardinality::<Expr>()
        || target.family_cardinality::<Match>() != pattern.family_cardinality::<Match>()
        || target.family_cardinality::<GUnforgeable>()
            != pattern.family_cardinality::<GUnforgeable>()
        || target.family_cardinality::<Bundle>() != pattern.family_cardinality::<Bundle>()
        || target.family_cardinality::<Connective>() != pattern.family_cardinality::<Connective>()
        || target.family_cardinality::<If>() != pattern.family_cardinality::<If>()
        || target.connective_used() != pattern.connective_used()
    {
        return Some(false);
    }

    let mut ordered = Vec::new();
    macro_rules! resident_family {
        ($ty:ty, $family:ident) => {
            if !enqueue_resident_family::<$ty>(
                target,
                pattern,
                ConcreteComponentFamily::$family,
                &mut ordered,
            ) {
                return Some(false);
            }
        };
    }
    resident_family!(Send, Send);
    resident_family!(Expr, Expr);
    resident_family!(Match, Match);
    resident_family!(GUnforgeable, GUnforgeable);
    resident_family!(Bundle, Bundle);
    resident_family!(Connective, Connective);
    resident_family!(If, If);
    resident_family!(Receive, Receive);
    resident_family!(New, New);

    let mut pending = Vec::with_capacity(ordered.len());
    pending.extend(ordered.into_iter().rev());
    note_concrete_pending_tasks(pending.len());
    while let Some(task) = pending.pop() {
        let matched = match task {
            ConcreteRegionTask::Par {
                target,
                pattern,
                equation,
            } => compare_concrete_par_regions(target, pattern, equation, &mut pending),
            ConcreteRegionTask::Component {
                target,
                pattern,
                family,
                equation,
            } => {
                compare_concrete_component_regions(target, pattern, family, equation, &mut pending)
            }
        }?;
        if !matched {
            return Some(false);
        }
    }
    Some(true)
}

struct ConnectivePlan {
    connective: Connective,
    bounds: (ParCount, ParCount),
    remainder_bounds: (ParCount, ParCount),
}

struct ParConnectiveState {
    target: Par,
    pattern: Par,
    plans: std::vec::IntoIter<ConnectivePlan>,
    remainder_level: Option<i32>,
    wildcard: bool,
}

struct ConnectiveCandidates {
    parent: ParConnectiveState,
    connective: Option<Connective>,
    candidates: std::iter::Peekable<Box<dyn Iterator<Item = (Par, Par)>>>,
    free_map_is_unreachable: bool,
}

fn evaluate_par(
    context: &mut FreeMapTransaction<'_>,
    target: Par,
    mut pattern: Par,
    frames: &mut FrameStack,
) -> Job {
    if !pattern.connective_used() {
        // `Par::eq` is authoritative over the resident PathMap representation
        // and is the cheapest positive proof for independently built concrete
        // terms. It is gated by the pattern's connective summary: taking it on
        // a spatial pattern would skip captures and connective semantics.
        if target == pattern {
            note_authoritative_equal_fast_positive();
            return Job::Return(true);
        }
        if let Some(matched) = match_concrete_par_regions(&target, &pattern) {
            return Job::Return(matched);
        }
        note_concrete_region_fallback();
        return Job::Return(match_pars(&target, &pattern));
    }

    if let Some(level) = sole_free_var_level(&pattern) {
        if sole_free_var_capture_is_admissible(context, &target, level) {
            context.write(level, target);
            return Job::Return(true);
        }
    }

    // An exact singleton `If` carrier has no AC choice to enumerate. Retain
    // the authoritative component bodies once and let the PDA descend through
    // shared protobuf ranges. This is a representation transition only: the
    // three children remain independent matching obligations and imply no
    // execution order between the Rholang processes they contain.
    if let Some(pattern_if) = singleton_component_region::<If>(&pattern) {
        return match singleton_component_region::<If>(&target) {
            Some(target_if) => {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::EncodedIf(target_if, pattern_if)
            }
            None => Job::Return(false),
        };
    }
    if let Some(pattern_new) = singleton_component_region::<New>(&pattern) {
        return match singleton_component_region::<New>(&target) {
            Some(target_new) => {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::EncodedNew(target_new, pattern_new)
            }
            None => Job::Return(false),
        };
    }
    if let Some(pattern_send) = singleton_component_region::<Send>(&pattern) {
        return match singleton_component_region::<Send>(&target) {
            Some(target_send) => {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::EncodedSend(target_send, pattern_send)
            }
            None => Job::Return(false),
        };
    }
    if let Some(pattern_receive) = singleton_component_region::<Receive>(&pattern) {
        return match singleton_component_region::<Receive>(&target) {
            Some(target_receive) => {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::EncodedReceive(target_receive, pattern_receive)
            }
            None => Job::Return(false),
        };
    }
    if let Some(pattern_match) = singleton_component_region::<Match>(&pattern) {
        return match singleton_component_region::<Match>(&target) {
            Some(target_match) => {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::EncodedMatch(target_match, pattern_match)
            }
            None => Job::Return(false),
        };
    }
    if let Some(pattern_hint) = singleton_epathmap_route_hint(&pattern) {
        let Some(target_hint) = singleton_epathmap_route_hint(&target) else {
            return Job::Return(false);
        };
        let Some(target_endpoint) = singleton_epathmap_endpoint(&target, target_hint) else {
            return Job::Return(false);
        };
        let Some(pattern_endpoint) = singleton_epathmap_endpoint(&pattern, pattern_hint) else {
            return Job::Return(false);
        };
        frames.push(Frame::RestoreOnFailure {
            checkpoint: context.checkpoint(),
        });
        if target_endpoint.route_hint.mode() == EPathMapMode::Map
            && pattern_endpoint.route_hint.mode() == EPathMapMode::Map
            && target_endpoint.route_hint.map_value_count() == 1
            && pattern_endpoint.route_hint.map_value_count() == 1
            && !pattern_endpoint.route_hint.has_remainder()
        {
            return Job::ProtobufExpr(Box::new((
                ProtobufRegion::from(target_endpoint.component.into_payload()),
                ProtobufRegion::from(pattern_endpoint.component.into_payload()),
            )));
        }

        note_epathmap_region_preflight_bypass();
        note_epathmap_direct_expr_decodes(2);
        // Decode target first to preserve the historical malformed-input error
        // precedence even though the pattern endpoint is discovered first.
        let target = target_endpoint.into_epathmap();
        let pattern = pattern_endpoint.into_epathmap();
        return Job::PathInit(Box::new((target, pattern)));
    }
    if let Some(pattern_expr) = singleton_component_region::<Expr>(&pattern) {
        let pattern_tag = canonical_expr_parts(pattern_expr.as_slice()).map(|parts| parts.tag());
        if let Some(pattern_tag) = pattern_tag {
            return match singleton_component_region::<Expr>(&target) {
                Some(target_expr) => match encoded_expr_outer_route(
                    canonical_expr_oneof_tag(target_expr.as_slice()),
                    Some(pattern_tag),
                ) {
                    EncodedExprOuterRoute::RejectProvenDifferent => Job::Return(false),
                    EncodedExprOuterRoute::Delegate => {
                        frames.push(Frame::RestoreOnFailure {
                            checkpoint: context.checkpoint(),
                        });
                        Job::EncodedExpr(target_expr, pattern_expr)
                    }
                },
                _ => Job::Return(false),
            };
        }
    }

    // Unary AND/OR are identities and unary NOT contributes only parity plus
    // binding isolation. Peel an arbitrary mixed spine directly from the
    // authoritative endpoint, then retain at most one NOT continuation. This
    // transition is matcher proof control, never Rholang execution ordering.
    if let Some((terminal, negated, has_not)) = pattern.canonical_unary_connective_spine() {
        if has_not {
            frames.push(Frame::NotSpine {
                checkpoint: context.checkpoint(),
                negated,
            });
        }
        return match_job(MatchPair::Par(target, terminal));
    }

    let mut expressions = pattern.take_occurrences::<Expr>();
    let remainder_level = expressions.iter().find_map(|expr| match expr {
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
    let wildcard = expressions.iter().any(|expr| {
        matches!(expr, Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(Wildcard(_)),
                }),
            })),
        })
    });

    expressions.retain(|expr| {
        !matches!(expr, Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(FreeVar(_) | Wildcard(_)),
                }),
            })),
        })
    });
    pattern.replace_exprs(expressions);

    // A Par carrying only one connective and no remainder can only accept the
    // whole target. The general `sub_pars` machine may enumerate proper
    // subsets, but every such branch subsequently fails when the nonempty
    // complement is matched against the empty residual pattern. Transitioning
    // directly is therefore equivalent and prevents a depth-d unary
    // connective chain from recomputing bounds over all d suffixes (O(d^2)).
    if remainder_level.is_none()
        && !wildcard
        && whole_target_is_connective_projection_identity(&target)
    {
        if let Some(connective) = singleton_component_region::<Connective>(&pattern) {
            #[cfg(test)]
            update_list_relation_stats(|stats| {
                stats.authoritative_connective_root_copies += 1;
                stats.authoritative_connective_root_bytes += connective.as_slice().len();
            });
            return Job::EncodedConnective {
                target: Rc::new(target),
                pattern: connective,
            };
        }
    }

    let count = ParCount::new(&pattern);
    let min_remainder = count.clone();
    let max_remainder = if wildcard || remainder_level.is_some() {
        ParCount::unbounded()
    } else {
        count.clone()
    };
    let connectives = pattern.take_occurrences::<Connective>();
    let individual: Vec<_> = connectives
        .iter()
        .map(|connective| count.min_max_con_ref(connective))
        .collect();
    let mut remainder_bounds = vec![(min_remainder, max_remainder)];
    for bounds in individual.iter().rev() {
        let last = remainder_bounds
            .last()
            .expect("the connective remainder fold is seeded");
        remainder_bounds.push((bounds.0.add(&last.0), bounds.1.add(&last.1)));
    }
    remainder_bounds.pop();
    remainder_bounds.reverse();

    let plans = connectives
        .into_iter()
        .zip(individual)
        .zip(remainder_bounds)
        .map(|((connective, bounds), remainder_bounds)| ConnectivePlan {
            connective,
            bounds,
            remainder_bounds,
        })
        .collect::<Vec<_>>();

    Job::ParConnectives(Box::new(ParConnectiveState {
        target,
        pattern,
        plans: plans.into_iter(),
        remainder_level,
        wildcard,
    }))
}

fn evaluate_encoded_par(target: EncodedRegion, pattern: EncodedRegion) -> Job {
    if target.as_slice() == pattern.as_slice()
        && canonical_par_connective_used(pattern.as_slice()) == Some(false)
    {
        return Job::Return(true);
    }

    let target_if =
        canonical_single_if_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_if =
        canonical_single_if_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_if), Some(pattern_if)) = (target_if, pattern_if) {
        return Job::EncodedIf(target_if, pattern_if);
    }
    let target_new =
        canonical_single_new_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_new =
        canonical_single_new_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_new), Some(pattern_new)) = (target_new, pattern_new) {
        return Job::EncodedNew(target_new, pattern_new);
    }
    let target_send =
        canonical_single_send_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_send =
        canonical_single_send_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_send), Some(pattern_send)) = (target_send, pattern_send) {
        return Job::EncodedSend(target_send, pattern_send);
    }
    let target_receive =
        canonical_single_receive_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_receive =
        canonical_single_receive_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_receive), Some(pattern_receive)) = (target_receive, pattern_receive) {
        return Job::EncodedReceive(target_receive, pattern_receive);
    }
    let target_match =
        canonical_single_match_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_match =
        canonical_single_match_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_match), Some(pattern_match)) = (target_match, pattern_match) {
        return Job::EncodedMatch(target_match, pattern_match);
    }
    let target_expr =
        canonical_single_expr_from_par(target.as_slice()).and_then(|body| target.child(body));
    let pattern_expr =
        canonical_single_expr_from_par(pattern.as_slice()).and_then(|body| pattern.child(body));
    if let (Some(target_expr), Some(pattern_expr)) = (target_expr, pattern_expr) {
        let pattern_tag = canonical_expr_parts(pattern_expr.as_slice()).map(|parts| parts.tag());
        if let Some(pattern_tag) = pattern_tag {
            return match encoded_expr_outer_route(
                canonical_expr_oneof_tag(target_expr.as_slice()),
                Some(pattern_tag),
            ) {
                EncodedExprOuterRoute::RejectProvenDifferent => Job::Return(false),
                EncodedExprOuterRoute::Delegate => Job::EncodedExpr(target_expr, pattern_expr),
            };
        }
    }

    // The ParBag importer already validated these canonical messages. Decode
    // only at a terminal or at a shape that the byte cursor cannot prove; the
    // complete generated decoder remains the semantic authority for both.
    let target = models::rust::rholang::protobuf_decoder::decode_par(target.as_slice())
        .expect("an authoritative nested recursive target Par is canonical");
    let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern.as_slice())
        .expect("an authoritative nested recursive pattern Par is canonical");
    match_job(MatchPair::Par(target, pattern))
}

fn evaluate_encoded_new(target: EncodedRegion, pattern: EncodedRegion) -> Job {
    let Some(target_parts) = canonical_new_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_new(target.as_slice())
            .expect("an authoritative New target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_new(pattern.as_slice())
            .expect("an authoritative New pattern endpoint is canonical");
        return evaluate_new(target, pattern);
    };
    let Some(pattern_parts) = canonical_new_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_new(target.as_slice())
            .expect("an authoritative New target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_new(pattern.as_slice())
            .expect("an authoritative New pattern endpoint is canonical");
        return evaluate_new(target, pattern);
    };
    if target_parts.bind_count != pattern_parts.bind_count {
        return Job::Return(false);
    }
    let Some(target_body) = target.child(target_parts.body) else {
        unreachable!("canonical New body is a subrange of its endpoint")
    };
    let Some(pattern_body) = pattern.child(pattern_parts.body) else {
        unreachable!("canonical New body is a subrange of its endpoint")
    };
    Job::EncodedPar(target_body, pattern_body)
}

fn evaluate_encoded_send(
    target: EncodedRegion,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_parts) = canonical_send_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_send(target.as_slice())
            .expect("an authoritative Send target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_send(pattern.as_slice())
            .expect("an authoritative Send pattern endpoint is canonical");
        return evaluate_send(target, pattern, frames);
    };
    let Some(pattern_parts) = canonical_send_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_send(target.as_slice())
            .expect("an authoritative Send target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_send(pattern.as_slice())
            .expect("an authoritative Send pattern endpoint is canonical");
        return evaluate_send(target, pattern, frames);
    };
    if target_parts.persistent != pattern_parts.persistent
        || target_parts.data.len() != pattern_parts.data.len()
    {
        return Job::Return(false);
    }
    let Some(target_channel) = target.child(target_parts.channel) else {
        unreachable!("canonical Send channel is a subrange of its endpoint")
    };
    let Some(pattern_channel) = pattern.child(pattern_parts.channel) else {
        unreachable!("canonical Send channel is a subrange of its endpoint")
    };
    if target_parts.data.is_empty() {
        return Job::EncodedPar(target_channel, pattern_channel);
    }
    let mut jobs = Vec::with_capacity(target_parts.data.len() + 1);
    jobs.push(Job::EncodedPar(target_channel, pattern_channel));
    for (target_data, pattern_data) in target_parts.data.into_iter().zip(pattern_parts.data) {
        let Some(target_data) = target.child(target_data) else {
            unreachable!("canonical Send datum is a subrange of its endpoint")
        };
        let Some(pattern_data) = pattern.child(pattern_data) else {
            unreachable!("canonical Send datum is a subrange of its endpoint")
        };
        jobs.push(Job::EncodedPar(target_data, pattern_data));
    }
    start_jobs(jobs, frames)
}

fn evaluate_encoded_receive(
    target: EncodedRegion,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_parts) = canonical_receive_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_receive(target.as_slice())
            .expect("an authoritative Receive target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_receive(pattern.as_slice())
            .expect("an authoritative Receive pattern endpoint is canonical");
        return evaluate_receive(target, pattern, frames);
    };
    let Some(pattern_parts) = canonical_receive_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_receive(target.as_slice())
            .expect("an authoritative Receive target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_receive(pattern.as_slice())
            .expect("an authoritative Receive pattern endpoint is canonical");
        return evaluate_receive(target, pattern, frames);
    };
    if target_parts.persistent != pattern_parts.persistent
        || target_parts.binds.len() != pattern_parts.binds.len()
    {
        return Job::Return(false);
    }
    let Some(target_body) = target.child(target_parts.body) else {
        unreachable!("canonical Receive body is a subrange of its endpoint")
    };
    let Some(pattern_body) = pattern.child(pattern_parts.body) else {
        unreachable!("canonical Receive body is a subrange of its endpoint")
    };
    if target_parts.binds.is_empty() {
        return Job::EncodedPar(target_body, pattern_body);
    }

    let targets = target_parts
        .binds
        .into_iter()
        .map(|body| {
            models::rust::rholang::protobuf_decoder::decode_receive_bind(body)
                .expect("an authoritative ReceiveBind target is canonical")
        })
        .map(MatchValue::ReceiveBind)
        .collect();
    let patterns = pattern_parts
        .binds
        .into_iter()
        .map(|body| {
            models::rust::rholang::protobuf_decoder::decode_receive_bind(body)
                .expect("an authoritative ReceiveBind pattern is canonical")
        })
        .map(MatchValue::ReceiveBind)
        .collect();
    start_jobs(
        vec![
            Job::ListInit(ListRequest::new(
                targets,
                patterns,
                Merger::Identity,
                None,
                false,
            )),
            Job::EncodedPar(target_body, pattern_body),
        ],
        frames,
    )
}

fn evaluate_encoded_match(
    target: EncodedRegion,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_parts) = canonical_match_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_match(target.as_slice())
            .expect("an authoritative Match target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_match(pattern.as_slice())
            .expect("an authoritative Match pattern endpoint is canonical");
        return evaluate_match(target, pattern, frames);
    };
    let Some(pattern_parts) = canonical_match_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_match(target.as_slice())
            .expect("an authoritative Match target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_match(pattern.as_slice())
            .expect("an authoritative Match pattern endpoint is canonical");
        return evaluate_match(target, pattern, frames);
    };
    if target_parts.cases.len() != pattern_parts.cases.len() {
        return Job::Return(false);
    }
    let Some(target_root) = target.child(target_parts.target) else {
        unreachable!("canonical Match target is a subrange of its endpoint")
    };
    let Some(pattern_root) = pattern.child(pattern_parts.target) else {
        unreachable!("canonical Match target is a subrange of its endpoint")
    };
    if target_parts.cases.is_empty() {
        return Job::EncodedPar(target_root, pattern_root);
    }

    let mut jobs = Vec::with_capacity(target_parts.cases.len() + 1);
    jobs.push(Job::EncodedPar(target_root, pattern_root));
    jobs.extend(target_parts.cases.into_iter().zip(pattern_parts.cases).map(
        |(target, pattern)| {
            let target = models::rust::rholang::protobuf_decoder::decode_match_case(target)
                .expect("an authoritative MatchCase target is canonical");
            let pattern = models::rust::rholang::protobuf_decoder::decode_match_case(pattern)
                .expect("an authoritative MatchCase pattern is canonical");
            match_job(MatchPair::MatchCase(target, pattern))
        },
    ));
    start_jobs(jobs, frames)
}

fn encoded_child(parent: &EncodedRegion, child: &[u8], label: &'static str) -> EncodedRegion {
    parent
        .child(child)
        .unwrap_or_else(|| unreachable!("canonical {label} is a subrange of its endpoint"))
}

fn encoded_collection_remainder(remainder: Option<&[u8]>) -> Option<(Option<i32>, bool)> {
    match remainder {
        None => Some((None, false)),
        Some(remainder) => canonical_collection_remainder(remainder),
    }
}

fn evaluate_encoded_expr(
    target: EncodedRegion,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_parts) = canonical_expr_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_expr(target.as_slice())
            .expect("an authoritative Expr target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_slice())
            .expect("an authoritative Expr pattern endpoint is canonical");
        return evaluate_expr(target, pattern, frames);
    };
    let Some(pattern_parts) = canonical_expr_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_expr(target.as_slice())
            .expect("an authoritative Expr target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_slice())
            .expect("an authoritative Expr pattern endpoint is canonical");
        return evaluate_expr(target, pattern, frames);
    };

    match (target_parts, pattern_parts) {
        (
            CanonicalExprParts::Unary {
                tag: target_tag,
                child: target_child,
            },
            CanonicalExprParts::Unary {
                tag: pattern_tag,
                child: pattern_child,
            },
        ) if target_tag == pattern_tag => Job::EncodedPar(
            encoded_child(&target, target_child, "unary Expr child"),
            encoded_child(&pattern, pattern_child, "unary Expr child"),
        ),
        (
            CanonicalExprParts::Binary {
                tag: target_tag,
                left: target_left,
                right: target_right,
            },
            CanonicalExprParts::Binary {
                tag: pattern_tag,
                left: pattern_left,
                right: pattern_right,
            },
        ) if target_tag == pattern_tag => {
            let left = (
                encoded_child(&target, target_left, "binary Expr left child"),
                encoded_child(&pattern, pattern_left, "binary Expr left child"),
            );
            let right = (
                encoded_child(&target, target_right, "binary Expr right child"),
                encoded_child(&pattern, pattern_right, "binary Expr right child"),
            );
            frames.push(Frame::EncodedSibling {
                sibling: Box::new(right),
            });
            Job::EncodedPar(left.0, left.1)
        }
        (
            CanonicalExprParts::Sequence {
                tag: target_tag,
                children: target_children,
                ..
            },
            CanonicalExprParts::Sequence {
                tag: pattern_tag,
                children: pattern_children,
                remainder: None,
            },
        ) if matches!(target_tag, 20 | 21)
            && target_tag == pattern_tag
            && target_children.len() == pattern_children.len() =>
        {
            let mut jobs = target_children.into_iter().zip(pattern_children).map(
                |(target_child, pattern_child)| {
                    Job::EncodedPar(
                        encoded_child(&target, target_child, "ordered Expr child"),
                        encoded_child(&pattern, pattern_child, "ordered Expr child"),
                    )
                },
            );
            let Some(first) = jobs.next() else {
                return Job::Return(true);
            };
            let mut remaining: Vec<_> = jobs.collect();
            if !remaining.is_empty() {
                remaining.reverse();
                frames.push(Frame::Sequence { remaining });
            }
            first
        }
        (
            CanonicalExprParts::Sequence {
                tag: 22,
                children: target_children,
                ..
            },
            CanonicalExprParts::Sequence {
                tag: 22,
                children: pattern_children,
                remainder,
            },
        ) => {
            let Some((level, wildcard)) = encoded_collection_remainder(remainder) else {
                let target =
                    models::rust::rholang::protobuf_decoder::decode_expr(target.as_slice())
                        .expect("an authoritative ESet target endpoint is canonical");
                let pattern =
                    models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_slice())
                        .expect("an authoritative ESet pattern endpoint is canonical");
                return evaluate_expr(target, pattern, frames);
            };
            Job::ListInit(ListRequest::new(
                target_children
                    .into_iter()
                    .map(|child| {
                        MatchValue::EncodedPar(encoded_child(&target, child, "ESet member"))
                    })
                    .collect(),
                pattern_children
                    .into_iter()
                    .map(|child| {
                        MatchValue::EncodedPar(encoded_child(&pattern, child, "ESet member"))
                    })
                    .collect(),
                Merger::ESet,
                level,
                wildcard,
            ))
        }
        (
            CanonicalExprParts::Map {
                entries: target_entries,
                ..
            },
            CanonicalExprParts::Map {
                entries: pattern_entries,
                remainder,
            },
        ) => {
            let Some((level, wildcard)) = encoded_collection_remainder(remainder) else {
                let target =
                    models::rust::rholang::protobuf_decoder::decode_expr(target.as_slice())
                        .expect("an authoritative EMap target endpoint is canonical");
                let pattern =
                    models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_slice())
                        .expect("an authoritative EMap pattern endpoint is canonical");
                return evaluate_expr(target, pattern, frames);
            };
            Job::ListInit(ListRequest::new(
                target_entries
                    .into_iter()
                    .map(|(key, value)| {
                        MatchValue::EncodedParPair((
                            encoded_child(&target, key, "EMap key"),
                            encoded_child(&target, value, "EMap value"),
                        ))
                    })
                    .collect(),
                pattern_entries
                    .into_iter()
                    .map(|(key, value)| {
                        MatchValue::EncodedParPair((
                            encoded_child(&pattern, key, "EMap key"),
                            encoded_child(&pattern, value, "EMap value"),
                        ))
                    })
                    .collect(),
                Merger::EMap,
                level,
                wildcard,
            ))
        }
        (
            CanonicalExprParts::Method {
                method_name: target_name,
                children: target_children,
            },
            CanonicalExprParts::Method {
                method_name: pattern_name,
                children: pattern_children,
            },
        ) if target_name == pattern_name && target_children.len() == pattern_children.len() => {
            let mut jobs = target_children.into_iter().zip(pattern_children).map(
                |(target_child, pattern_child)| {
                    Job::EncodedPar(
                        encoded_child(&target, target_child, "method Expr child"),
                        encoded_child(&pattern, pattern_child, "method Expr child"),
                    )
                },
            );
            let Some(first) = jobs.next() else {
                return Job::Return(true);
            };
            let mut remaining: Vec<_> = jobs.collect();
            if !remaining.is_empty() {
                remaining.reverse();
                frames.push(Frame::Sequence { remaining });
            }
            first
        }
        (
            CanonicalExprParts::Matches {
                target: target_root,
                pattern: target_pattern,
            },
            CanonicalExprParts::Matches {
                target: pattern_root,
                pattern: pattern_pattern,
            },
        ) if target_pattern == pattern_pattern => Job::EncodedPar(
            encoded_child(&target, target_root, "EMatches target"),
            encoded_child(&pattern, pattern_root, "EMatches target"),
        ),
        _ => {
            let target = models::rust::rholang::protobuf_decoder::decode_expr(target.as_slice())
                .expect("an authoritative Expr target endpoint is canonical");
            let pattern = models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_slice())
                .expect("an authoritative Expr pattern endpoint is canonical");
            evaluate_expr(target, pattern, frames)
        }
    }
}

fn protobuf_child(parent: &ProtobufRegion, child: &[u8]) -> Option<ProtobufRegion> {
    let root_start = parent.as_bytes().as_ptr() as usize;
    let child_start = child.as_ptr() as usize;
    let start = child_start.checked_sub(root_start)?;
    let end = start.checked_add(child.len())?;
    parent.subregion(models::rust::rholang::protobuf_cursor::CanonicalByteRange { start, end })
}

fn compare_protobuf_par_eq_regions(
    target: ProtobufRegion,
    pattern: ProtobufRegion,
) -> Option<bool> {
    let mut pending = vec![ConcreteRegionTask::Par {
        target,
        pattern,
        equation: ConcreteParEquation::ParEq,
    }];
    while let Some(task) = pending.pop() {
        let matched = match task {
            ConcreteRegionTask::Par {
                target,
                pattern,
                equation,
            } => compare_concrete_par_regions(target, pattern, equation, &mut pending),
            ConcreteRegionTask::Component {
                target,
                pattern,
                family,
                equation,
            } => {
                compare_concrete_component_regions(target, pattern, family, equation, &mut pending)
            }
        }?;
        if !matched {
            return Some(false);
        }
    }
    Some(true)
}

fn evaluate_protobuf_par(
    context: &mut FreeMapTransaction<'_>,
    target: ProtobufRegion,
    pattern: ProtobufRegion,
) -> Job {
    if target == pattern && canonical_par_connective_used(pattern.as_bytes()) == Some(false) {
        return Job::Return(true);
    }

    if let Some(free_var) = canonical_exact_free_var_par(pattern.as_bytes()) {
        if free_var.connective_used() {
            note_epathmap_leaf_decode();
            let target = models::rust::rholang::protobuf_decoder::decode_par(target.as_bytes())
                .expect("an authoritative EPathMap value Par is canonical");
            if sole_free_var_capture_is_admissible(context, &target, free_var.level()) {
                context.write(free_var.level(), target);
                return Job::Return(true);
            }
            let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern.as_bytes())
                .expect("an authoritative EPathMap pattern Par is canonical");
            return match_job(MatchPair::Par(target, pattern));
        }
    }

    let target_expr = canonical_single_expr_from_par(target.as_bytes())
        .and_then(|expr| protobuf_child(&target, expr));
    let pattern_expr = canonical_single_expr_from_par(pattern.as_bytes())
        .and_then(|expr| protobuf_child(&pattern, expr));
    if let Some(pattern_expr) = pattern_expr {
        if canonical_expr_oneof_tag(pattern_expr.as_bytes()) == Some(32) {
            return match target_expr {
                Some(target_expr)
                    if canonical_expr_oneof_tag(target_expr.as_bytes()) == Some(32) =>
                {
                    Job::ProtobufExpr(Box::new((target_expr, pattern_expr)))
                }
                _ => Job::Return(false),
            };
        }
    }

    note_epathmap_full_decode_fallback();
    let target = models::rust::rholang::protobuf_decoder::decode_par(target.as_bytes())
        .expect("an authoritative EPathMap value Par is canonical");
    let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern.as_bytes())
        .expect("an authoritative EPathMap pattern Par is canonical");
    match_job(MatchPair::Par(target, pattern))
}

fn sole_epathmap_map_value(plan: &ProtobufCutPlan) -> Option<ProtobufRegion> {
    let [group] = plan.epathmaps() else {
        return None;
    };
    if group.mode() != EPathMapMode::Map || group.cut_range().len() != 1 {
        return None;
    }
    let index = group.cut_range().start;
    let path = plan.cut_path(index)?;
    path.iter()
        .any(|step| step.kind == ProtobufRegionStepKind::EPathMapValue)
        .then(|| plan.cuts()[index].region().clone())
}

fn decode_shallow_epathmap_expr(plan: &ProtobufCutPlan) -> Option<EPathMap> {
    let expr = models::rust::rholang::protobuf_decoder::decode_expr(plan.shallow_bytes()).ok()?;
    match expr.expr_instance {
        Some(EPathmapBody(map)) => Some(map),
        _ => None,
    }
}

fn fallback_protobuf_expr(
    target: ProtobufRegion,
    pattern: ProtobufRegion,
    frames: &mut FrameStack,
) -> Job {
    note_epathmap_full_decode_fallback();
    note_epathmap_direct_expr_decodes(2);
    let target = models::rust::rholang::protobuf_decoder::decode_expr(target.as_bytes())
        .expect("an authoritative EPathMap target Expr is canonical");
    let pattern = models::rust::rholang::protobuf_decoder::decode_expr(pattern.as_bytes())
        .expect("an authoritative EPathMap pattern Expr is canonical");
    evaluate_expr(target, pattern, frames)
}

fn evaluate_protobuf_expr(
    context: &mut FreeMapTransaction<'_>,
    target: ProtobufRegion,
    pattern: ProtobufRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_hint) = canonical_epathmap_root_route_hint(target.as_bytes()) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    let Some(pattern_hint) = canonical_epathmap_root_route_hint(pattern.as_bytes()) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    // The shared-region route exists to keep recursive PathMap<Par> value
    // chains linear. A set has no recursive value, while a non-singleton map
    // would make `cut_at_par` build one cut/path per value only to fail the
    // cardinality check and decode the complete maps again. The EPM prefix
    // exposes map value count without walking the ACT arena, so reject those
    // unprofitable routes before allocating or traversing width. This is only
    // dispatch: the eligible singleton route still performs the complete
    // canonical framing and topology validation below.
    if target_hint.mode() != EPathMapMode::Map
        || pattern_hint.mode() != EPathMapMode::Map
        || target_hint.map_value_count() != 1
        || pattern_hint.map_value_count() != 1
        || pattern_hint.has_remainder()
    {
        note_epathmap_region_preflight_bypass();
        return fallback_protobuf_expr(target, pattern, frames);
    }
    note_epathmap_region_cut_attempt();
    let Ok(target_plan) = cut_at_par(target.clone(), &EXPR_RHO_MATCH_PROGRAM) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    note_epathmap_region_cut_attempt();
    let Ok(pattern_plan) = cut_at_par(pattern.clone(), &EXPR_RHO_MATCH_PROGRAM) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    let Some(target_map) = decode_shallow_epathmap_expr(&target_plan) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    let Some(pattern_map) = decode_shallow_epathmap_expr(&pattern_plan) else {
        return fallback_protobuf_expr(target, pattern, frames);
    };
    if pattern_map.remainder.is_some() || target_map.len() != 1 || pattern_map.len() != 1 {
        return fallback_protobuf_expr(target, pattern, frames);
    }
    let target_mode = target_map.mode();
    let pattern_mode = pattern_map.mode();
    if target_mode != pattern_mode {
        return Job::Return(false);
    }

    let Some(target_entry) = take_single_path_entry(target_map) else {
        return Job::Return(false);
    };
    let Some(pattern_entry) = take_single_path_entry(pattern_map) else {
        return Job::Return(false);
    };
    note_epathmap_region_level();
    match (target_entry, pattern_entry) {
        (OwnedEPathMapEntry::Set(target_key), OwnedEPathMapEntry::Set(pattern_key)) => {
            let metadata = EntryTrie::encoded_key_metadata(&pattern_key)
                .expect("set pattern key is a canonical Par path");
            if !metadata.connective_used {
                return Job::Return(target_key == pattern_key);
            }
            frames.push(Frame::RestoreOnFailure {
                checkpoint: context.checkpoint(),
            });
            match_job(MatchPair::Par(
                decode_trie_path(&target_key).expect("set target key is a canonical Par path"),
                decode_trie_path(&pattern_key).expect("set pattern key is a canonical Par path"),
            ))
        }
        (
            OwnedEPathMapEntry::Map {
                key: target_key, ..
            },
            OwnedEPathMapEntry::Map {
                key: pattern_key, ..
            },
        ) => {
            let Some(target_value) = sole_epathmap_map_value(&target_plan) else {
                return fallback_protobuf_expr(target, pattern, frames);
            };
            let Some(pattern_value) = sole_epathmap_map_value(&pattern_plan) else {
                return fallback_protobuf_expr(target, pattern, frames);
            };
            let metadata = EntryTrie::encoded_key_metadata(&pattern_key)
                .expect("map pattern key is a canonical Par path");
            if !metadata.connective_used {
                if target_key != pattern_key {
                    return Job::Return(false);
                }
                if canonical_par_connective_used(pattern_value.as_bytes()) == Some(false) {
                    let matched = compare_protobuf_par_eq_regions(
                        target_value.clone(),
                        pattern_value.clone(),
                    )
                    .unwrap_or_else(|| {
                        let target = models::rust::rholang::protobuf_decoder::decode_par(
                            target_value.as_bytes(),
                        )
                        .expect("an authoritative exact map value is canonical");
                        let pattern = models::rust::rholang::protobuf_decoder::decode_par(
                            pattern_value.as_bytes(),
                        )
                        .expect("an authoritative exact map pattern is canonical");
                        target == pattern
                    });
                    return Job::Return(matched);
                }
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                Job::ProtobufPar(Box::new((target_value, pattern_value)))
            } else {
                frames.push(Frame::RestoreOnFailure {
                    checkpoint: context.checkpoint(),
                });
                frames.push(Frame::ProtobufSibling {
                    sibling: Box::new((target_value, pattern_value)),
                });
                match_job(MatchPair::Par(
                    decode_trie_path(&target_key).expect("map target key is a canonical Par path"),
                    decode_trie_path(&pattern_key)
                        .expect("map pattern key is a canonical Par path"),
                ))
            }
        }
        _ => Job::Return(false),
    }
}

fn step_par_connectives(mut state: ParConnectiveState, frames: &mut FrameStack) -> Job {
    let Some(plan) = state.plans.next() else {
        return start_par_fields(state, frames);
    };
    let candidates = sub_pars(
        &state.target,
        &plan.bounds.0,
        &plan.bounds.1,
        &plan.remainder_bounds.0,
        &plan.remainder_bounds.1,
    );
    let free_map_is_unreachable = matches!(
        plan.connective.connective_instance,
        Some(ConnBool(_))
            | Some(ConnInt(_))
            | Some(ConnString(_))
            | Some(ConnUri(_))
            | Some(ConnByteArray(_))
            | Some(VarRefBody(_))
            | None
    );
    let candidates: Box<dyn Iterator<Item = (Par, Par)>> = Box::new(candidates);
    Job::ConnectiveCandidates(Box::new(ConnectiveCandidates {
        parent: state,
        connective: Some(plan.connective),
        candidates: candidates.peekable(),
        free_map_is_unreachable,
    }))
}

fn step_connective_candidates(
    context: &mut FreeMapTransaction<'_>,
    mut state: ConnectiveCandidates,
    frames: &mut FrameStack,
) -> Job {
    let Some((candidate, remainder)) = state.candidates.next() else {
        return Job::Return(false);
    };
    let checkpoint = (!state.free_map_is_unreachable).then(|| context.checkpoint());
    // Every candidate except the last needs a reusable connective. Move the
    // final one so singleton candidate searches never clone a recursively
    // nested connective merely to satisfy continuation ownership.
    let connective = if state.candidates.peek().is_none() {
        state
            .connective
            .take()
            .expect("a candidate search retains its connective")
    } else {
        state
            .connective
            .as_ref()
            .expect("a non-final candidate retains its connective")
            .clone()
    };
    frames.push(Frame::ConnectiveCandidate {
        state: Box::new(state),
        checkpoint,
        remainder: Box::new(remainder),
    });
    match_job(MatchPair::Connective(candidate, connective))
}

fn start_par_fields(state: ParConnectiveState, frames: &mut FrameStack) -> Job {
    let mut target = state.target;
    let mut pattern = state.pattern;
    let level = state.remainder_level;
    let wildcard = state.wildcard;
    let requests = vec![
        ListRequest::new(
            target
                .take_occurrences::<Send>()
                .into_iter()
                .map(MatchValue::Send)
                .collect(),
            pattern
                .take_occurrences::<Send>()
                .into_iter()
                .map(MatchValue::Send)
                .collect(),
            Merger::ParSends,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<Receive>()
                .into_iter()
                .map(MatchValue::Receive)
                .collect(),
            pattern
                .take_occurrences::<Receive>()
                .into_iter()
                .map(MatchValue::Receive)
                .collect(),
            Merger::ParReceives,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<New>()
                .into_iter()
                .map(MatchValue::New)
                .collect(),
            pattern
                .take_occurrences::<New>()
                .into_iter()
                .map(MatchValue::New)
                .collect(),
            Merger::ParNews,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<Expr>()
                .into_iter()
                .map(MatchValue::Expr)
                .collect(),
            pattern
                .take_occurrences::<Expr>()
                .into_iter()
                .map(MatchValue::Expr)
                .collect(),
            Merger::ParExprs,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<Match>()
                .into_iter()
                .map(MatchValue::Match)
                .collect(),
            pattern
                .take_occurrences::<Match>()
                .into_iter()
                .map(MatchValue::Match)
                .collect(),
            Merger::ParMatches,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<Bundle>()
                .into_iter()
                .map(MatchValue::Bundle)
                .collect(),
            pattern
                .take_occurrences::<Bundle>()
                .into_iter()
                .map(MatchValue::Bundle)
                .collect(),
            Merger::ParBundles,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<GUnforgeable>()
                .into_iter()
                .map(MatchValue::Unforgeable)
                .collect(),
            pattern
                .take_occurrences::<GUnforgeable>()
                .into_iter()
                .map(MatchValue::Unforgeable)
                .collect(),
            Merger::ParUnforgeables,
            level,
            wildcard,
        ),
        ListRequest::new(
            target
                .take_occurrences::<If>()
                .into_iter()
                .map(MatchValue::If)
                .collect(),
            pattern
                .take_occurrences::<If>()
                .into_iter()
                .map(MatchValue::If)
                .collect(),
            Merger::ParConditionals,
            level,
            wildcard,
        ),
    ];
    start_jobs(requests.into_iter().map(Job::ListInit).collect(), frames)
}

fn evaluate_connective(
    context: &mut FreeMapTransaction<'_>,
    target: Par,
    pattern: Connective,
    frames: &mut FrameStack,
) -> Job {
    match pattern.connective_instance {
        Some(ConnAndBody(ConnectiveBody { ps })) => {
            let checkpoint = context.checkpoint();
            frames.push(Frame::RestoreOnFailure { checkpoint });
            start_and_branch(target, ps.into_iter(), frames)
        }
        Some(ConnOrBody(ConnectiveBody { ps })) => {
            start_or_branch(context, target, ps.into_iter(), frames)
        }
        Some(ConnNotBody(pattern)) => {
            let checkpoint = context.checkpoint();
            frames.push(Frame::Not { checkpoint });
            match_job(MatchPair::Par(target, pattern))
        }
        Some(VarRefBody(_)) | None => Job::Return(false),
        Some(ConnBool(_)) => Job::Return(single_expr_oneof_tag(&target) == Some(1)),
        Some(ConnInt(_)) => Job::Return(single_expr_oneof_tag(&target) == Some(2)),
        Some(ConnString(_)) => Job::Return(single_expr_oneof_tag(&target) == Some(3)),
        Some(ConnUri(_)) => Job::Return(single_expr_oneof_tag(&target) == Some(4)),
        Some(ConnByteArray(_)) => Job::Return(single_expr_oneof_tag(&target) == Some(25)),
    }
}

#[cfg(test)]
fn record_authoritative_frame_depth(frames: &FrameStack) {
    update_list_relation_stats(|stats| {
        stats.authoritative_max_frame_depth = stats.authoritative_max_frame_depth.max(frames.len());
    });
}

/// Match one authoritative canonical Connective endpoint without projecting
/// its recursive `Par` children. The endpoint is owned once by an
/// [`EncodedRegion`]; every descendant is a thin Arc/range and every branch
/// continuation is an explicit PDA frame.
fn evaluate_encoded_connective(
    context: &mut FreeMapTransaction<'_>,
    target: EncodedTarget,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    #[cfg(test)]
    update_list_relation_stats(|stats| {
        stats.authoritative_connective_evaluations += 1;
        stats.authoritative_encoded_connective_nodes += 1;
    });

    let Some(parts) = canonical_connective_parts(pattern.as_slice()) else {
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.authoritative_resident_connective_fallbacks += 1;
        });
        let pattern = <Connective as prost::Message>::decode(pattern.as_slice())
            .expect("an authoritative Connective endpoint is canonical");
        return evaluate_connective(context, into_owned_encoded_target(target), pattern, frames);
    };

    match parts {
        CanonicalConnectiveParts::And(children) => {
            let body = pattern
                .child(children.message())
                .expect("canonical AND body is a subrange of its endpoint");
            let children = children
                .bind_owned(body)
                .expect("canonical AND children bind to their exact owned body");
            let checkpoint = context.checkpoint();
            frames.push(Frame::RestoreOnFailure { checkpoint });
            #[cfg(test)]
            record_authoritative_frame_depth(frames);
            start_encoded_and_branch(context, target, children, frames)
        }
        CanonicalConnectiveParts::Or(children) => {
            let body = pattern
                .child(children.message())
                .expect("canonical OR body is a subrange of its endpoint");
            let children = children
                .bind_owned(body)
                .expect("canonical OR children bind to their exact owned body");
            start_encoded_or_branch(context, target, children, frames)
        }
        CanonicalConnectiveParts::Not(child) => {
            let child = pattern
                .child(child)
                .expect("canonical NOT child is a subrange of its endpoint");
            #[cfg(test)]
            update_list_relation_stats(|stats| stats.authoritative_not_frames += 1);
            frames.push(Frame::Not {
                checkpoint: context.checkpoint(),
            });
            #[cfg(test)]
            record_authoritative_frame_depth(frames);
            encoded_connective_child_job(context, target, child)
        }
        CanonicalConnectiveParts::VarRef | CanonicalConnectiveParts::Absent => Job::Return(false),
        CanonicalConnectiveParts::Predicate { tag, .. } => {
            let expected_expr_tag = match tag {
                5 => 1,
                6 => 2,
                7 => 3,
                8 => 4,
                9 => 25,
                _ => unreachable!("canonical predicate Connective tag is in 5..=9"),
            };
            Job::Return(single_expr_oneof_tag(target.as_ref()) == Some(expected_expr_tag))
        }
    }
}

fn single_expr_oneof_tag(target: &Par) -> Option<u32> {
    if target.family_cardinality::<Expr>() != 1 || !target.all_components_are::<Expr>() {
        return None;
    }
    let mut tag = None;
    target.visit_encoded::<Expr>(|entry| {
        if entry.multiplicity().get() == 1 {
            tag = canonical_expr_oneof_tag(entry.payload());
        }
    });
    tag
}

fn encoded_connective_child_job(
    context: &mut FreeMapTransaction<'_>,
    target: EncodedTarget,
    pattern: EncodedRegion,
) -> Job {
    if let Some(free_var) = canonical_exact_free_var_par(pattern.as_slice()) {
        if free_var.connective_used()
            && sole_free_var_capture_is_admissible(context, target.as_ref(), free_var.level())
        {
            #[cfg(test)]
            update_list_relation_stats(|stats| {
                stats.authoritative_exact_free_var_captures += 1;
            });
            context.write(free_var.level(), into_owned_encoded_target(target));
            return Job::Return(true);
        }
    }

    if canonical_par_connective_used(pattern.as_slice()) == Some(true) {
        if let Some(connective) = canonical_single_connective_from_par(pattern.as_slice()) {
            let connective = pattern
                .child(connective)
                .expect("canonical child Connective is a subrange of its Par");
            return Job::EncodedConnective {
                target,
                pattern: connective,
            };
        }
    }

    if exact_scalar_route_enabled() {
        if let Some(pattern_scalar) = canonical_exact_scalar_par(pattern.as_slice()) {
            if !pattern_scalar.connective_used() {
                if let Some(equal) =
                    exact_scalar_par_matches(target.as_ref(), pattern_scalar.expr())
                {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| {
                        stats.authoritative_exact_scalar_attempts += 1;
                        if equal {
                            stats.authoritative_exact_scalar_equal += 1;
                        } else {
                            stats.authoritative_exact_scalar_mismatch += 1;
                        }
                    });
                    return Job::Return(equal);
                }
            }
        }
    }

    #[cfg(test)]
    update_list_relation_stats(|stats| {
        stats.authoritative_exact_scalar_unproved += 1;
        stats.authoritative_terminal_par_decodes += 1;
    });
    let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern.as_slice())
        .unwrap_or_else(|error| {
            let preview_len = pattern.as_slice().len().min(32);
            panic!(
                "an authoritative connective child Par is canonical ({} bytes, first {preview_len}: {:02x?}): {error:?}",
                pattern.as_slice().len(),
                &pattern.as_slice()[..preview_len]
            )
        });
    match_job(MatchPair::Par(into_owned_encoded_target(target), pattern))
}

fn into_owned_encoded_target(target: EncodedTarget) -> Par {
    Rc::try_unwrap(target).unwrap_or_else(|shared| shared.as_ref().clone())
}

/// Return a proven scalar-equality result, or `None` when the target's encoded
/// Expr is outside the deliberately narrow byte-native proof domain.
///
/// Shape differences are conclusive mismatches. An exact singleton Expr whose
/// variant cannot be validated as a ground scalar delegates to the generated
/// decoder instead of guessing from a shallow protobuf tag.
fn exact_scalar_par_matches(target: &Par, pattern_expr: &[u8]) -> Option<bool> {
    if target.connective_used()
        || target.family_cardinality::<Expr>() != 1
        || !target.all_components_are::<Expr>()
    {
        return Some(false);
    }

    let mut equal = None;
    target.visit_encoded::<Expr>(|entry| {
        if entry.multiplicity().get() == 1 {
            equal = canonical_exact_scalar_expr(entry.payload())
                .map(|target_expr| target_expr == pattern_expr);
        }
    });
    equal
}

fn start_encoded_and_branch(
    context: &mut FreeMapTransaction<'_>,
    target: EncodedTarget,
    mut children: EncodedConnectiveChildren,
    frames: &mut FrameStack,
) -> Job {
    let Some(pattern) = children.next() else {
        return Job::Return(true);
    };
    let pattern = children
        .body()
        .child_range(pattern)
        .expect("canonical AND child is a subrange of its body");
    if children.is_empty() {
        encoded_connective_child_job(context, target, pattern)
    } else {
        #[cfg(test)]
        update_list_relation_stats(|stats| stats.authoritative_and_frames += 1);
        frames.push(Frame::EncodedAndBranch {
            target: Rc::clone(&target),
            children,
        });
        #[cfg(test)]
        record_authoritative_frame_depth(frames);
        encoded_connective_child_job(context, target, pattern)
    }
}

fn start_encoded_or_branch(
    context: &mut FreeMapTransaction<'_>,
    target: EncodedTarget,
    mut children: EncodedConnectiveChildren,
    frames: &mut FrameStack,
) -> Job {
    let Some(pattern) = children.next() else {
        return Job::Return(false);
    };
    let pattern = children
        .body()
        .child_range(pattern)
        .expect("canonical OR child is a subrange of its body");
    let checkpoint = context.checkpoint();
    let retained_target = (!children.is_empty()).then(|| Rc::clone(&target));
    #[cfg(test)]
    update_list_relation_stats(|stats| stats.authoritative_or_frames += 1);
    frames.push(Frame::EncodedOrBranch {
        target: retained_target,
        children,
        checkpoint,
    });
    #[cfg(test)]
    record_authoritative_frame_depth(frames);
    encoded_connective_child_job(context, target, pattern)
}

fn start_and_branch(
    target: Par,
    mut patterns: std::vec::IntoIter<Par>,
    frames: &mut FrameStack,
) -> Job {
    let Some(pattern) = patterns.next() else {
        return Job::Return(true);
    };
    if patterns.len() == 0 {
        // The final conjunct consumes the sole remaining target owner. This
        // makes singleton AND chains linear even when the target is large.
        match_job(MatchPair::Par(target, pattern))
    } else {
        frames.push(Frame::AndBranch {
            target: Box::new(target.clone()),
            patterns,
        });
        match_job(MatchPair::Par(target, pattern))
    }
}

fn start_or_branch(
    context: &mut FreeMapTransaction<'_>,
    target: Par,
    mut patterns: std::vec::IntoIter<Par>,
    frames: &mut FrameStack,
) -> Job {
    // Move the next branch out of the connective. Cloning through an
    // `Arc<[Par]>` here made a depth-d singleton OR chain copy every
    // remaining subtree, turning an otherwise iterative walk into O(d^2)
    // allocation and an 8 GiB failure at depth 20,000.
    let Some(pattern) = patterns.next() else {
        return Job::Return(false);
    };
    let checkpoint = context.checkpoint();
    let retained_target = (patterns.len() != 0).then(|| Box::new(target.clone()));
    frames.push(Frame::OrBranch {
        target: retained_target,
        patterns,
        checkpoint,
    });
    match_job(MatchPair::Par(target, pattern))
}

fn evaluate_send(target: Send, pattern: Send, frames: &mut FrameStack) -> Job {
    if target.persistent != pattern.persistent {
        return Job::Return(false);
    }
    let mut jobs = vec![match_job(MatchPair::Par(
        target.chan.expect("Send.chan (target)"),
        pattern.chan.expect("Send.chan (pattern)"),
    ))];
    let Some(data_jobs) = ordered_pairs(
        target.data,
        pattern.data,
        None,
        OrderedRemainder::None,
        frames,
    ) else {
        return Job::Return(false);
    };
    jobs.extend(data_jobs);
    start_jobs(jobs, frames)
}

fn evaluate_receive(target: Receive, pattern: Receive, frames: &mut FrameStack) -> Job {
    if target.persistent != pattern.persistent {
        return Job::Return(false);
    }
    start_jobs(
        vec![
            Job::ListInit(ListRequest::new(
                target
                    .binds
                    .into_iter()
                    .map(MatchValue::ReceiveBind)
                    .collect(),
                pattern
                    .binds
                    .into_iter()
                    .map(MatchValue::ReceiveBind)
                    .collect(),
                Merger::Identity,
                None,
                false,
            )),
            match_job(MatchPair::Par(
                target.body.expect("Receive.body (target)"),
                pattern.body.expect("Receive.body (pattern)"),
            )),
        ],
        frames,
    )
}

fn evaluate_new(target: New, pattern: New) -> Job {
    if target.bind_count != pattern.bind_count {
        Job::Return(false)
    } else {
        match_job(MatchPair::Par(
            target.p.expect("New.p (target)"),
            pattern.p.expect("New.p (pattern)"),
        ))
    }
}

enum OrderedRemainder {
    None,
    EList(Option<Var>),
}

fn ordered_pairs(
    targets: Vec<Par>,
    patterns: Vec<Par>,
    remainder: Option<Var>,
    bind: OrderedRemainder,
    frames: &mut FrameStack,
) -> Option<Vec<Job>> {
    if patterns.len() > targets.len() {
        return None;
    }
    let matched = patterns.len();
    let surplus = targets[matched..].to_vec();
    if !surplus.is_empty() {
        match remainder.as_ref().and_then(|var| var.var_instance.as_ref()) {
            Some(Wildcard(_)) => {}
            Some(FreeVar(_)) if surplus.iter().all(|par| par.locally_free().is_empty()) => {}
            _ => return None,
        }
    }
    if targets.len() != patterns.len() && remainder.is_none() {
        return None;
    }

    if let OrderedRemainder::EList(Some(Var {
        var_instance: Some(FreeVar(level)),
    })) = bind
    {
        frames.push(Frame::BindOnSuccess {
            level,
            value: Box::new(new_elist_par(
                surplus,
                Vec::new(),
                false,
                None,
                Vec::new(),
                false,
            )),
        });
    }

    Some(
        targets
            .into_iter()
            .take(matched)
            .zip(patterns)
            .map(|(target, pattern)| match_job(MatchPair::Par(target, pattern)))
            .collect(),
    )
}

fn evaluate_expr(target: Expr, pattern: Expr, frames: &mut FrameStack) -> Job {
    match (target.expr_instance, pattern.expr_instance) {
        (Some(EListBody(target)), Some(EListBody(pattern))) => {
            let remainder = pattern.remainder.clone();
            match ordered_pairs(
                target.ps,
                pattern.ps,
                remainder.clone(),
                OrderedRemainder::EList(remainder),
                frames,
            ) {
                Some(jobs) => start_jobs(jobs, frames),
                None => Job::Return(false),
            }
        }
        (Some(ETupleBody(target)), Some(ETupleBody(pattern))) => {
            match ordered_pairs(target.ps, pattern.ps, None, OrderedRemainder::None, frames) {
                Some(jobs) => start_jobs(jobs, frames),
                None => Job::Return(false),
            }
        }
        (Some(ESetBody(target)), Some(ESetBody(pattern))) => {
            let remainder = pattern.remainder.as_ref();
            let wildcard = matches!(
                remainder.and_then(|var| var.var_instance.as_ref()),
                Some(Wildcard(_))
            );
            let level = match remainder.and_then(|var| var.var_instance.as_ref()) {
                Some(FreeVar(level)) => Some(*level),
                _ => None,
            };
            let targets = ParSetTypeMapper::eset_to_par_set(target).ps.sorted_pars;
            let patterns = ParSetTypeMapper::eset_to_par_set(pattern).ps.sorted_pars;
            Job::ListInit(ListRequest::new(
                targets.into_iter().map(MatchValue::Par).collect(),
                patterns.into_iter().map(MatchValue::Par).collect(),
                Merger::ESet,
                level,
                wildcard,
            ))
        }
        (Some(EMapBody(target)), Some(EMapBody(pattern))) => {
            let remainder = pattern.remainder.as_ref();
            let wildcard = matches!(
                remainder.and_then(|var| var.var_instance.as_ref()),
                Some(Wildcard(_))
            );
            let level = match remainder.and_then(|var| var.var_instance.as_ref()) {
                Some(FreeVar(level)) => Some(*level),
                _ => None,
            };
            let targets = ParMapTypeMapper::emap_to_par_map(target).ps.sorted_list;
            let patterns = ParMapTypeMapper::emap_to_par_map(pattern).ps.sorted_list;
            Job::ListInit(ListRequest::new(
                targets.into_iter().map(MatchValue::ParPair).collect(),
                patterns.into_iter().map(MatchValue::ParPair).collect(),
                Merger::EMap,
                level,
                wildcard,
            ))
        }
        (Some(EPathmapBody(target)), Some(EPathmapBody(pattern))) => {
            Job::PathInit(Box::new((target, pattern)))
        }
        (Some(EVarBody(EVar { v: target })), Some(EVarBody(EVar { v: pattern }))) => {
            Job::Return(target == pattern)
        }
        (Some(ENotBody(ENot { p: target })), Some(ENotBody(ENot { p: pattern }))) => {
            match_job(MatchPair::Par(
                target.expect("ENot.p (target)"),
                pattern.expect("ENot.p (pattern)"),
            ))
        }
        (Some(ENegBody(ENeg { p: target })), Some(ENegBody(ENeg { p: pattern }))) => {
            match_job(MatchPair::Par(
                target.expect("ENeg.p (target)"),
                pattern.expect("ENeg.p (pattern)"),
            ))
        }
        (Some(EMethodBody(target)), Some(EMethodBody(pattern))) => {
            if target.method_name != pattern.method_name
                || target.arguments.len() != pattern.arguments.len()
            {
                return Job::Return(false);
            }
            let mut jobs = vec![match_job(MatchPair::Par(
                target.target.expect("EMethod.target (target)"),
                pattern.target.expect("EMethod.target (pattern)"),
            ))];
            jobs.extend(
                target
                    .arguments
                    .into_iter()
                    .zip(pattern.arguments)
                    .map(|(target, pattern)| match_job(MatchPair::Par(target, pattern))),
            );
            start_jobs(jobs, frames)
        }
        (Some(EMatchesBody(target)), Some(EMatchesBody(pattern))) => {
            if target.pattern != pattern.pattern {
                Job::Return(false)
            } else {
                match_job(MatchPair::Par(
                    target.target.expect("EMatches.target (target)"),
                    pattern.target.expect("EMatches.target (pattern)"),
                ))
            }
        }
        (target, pattern) => evaluate_binary_expr(target, pattern, frames),
    }
}

fn binary_jobs(
    target1: Option<Par>,
    target2: Option<Par>,
    pattern1: Option<Par>,
    pattern2: Option<Par>,
    name: &str,
    frames: &mut FrameStack,
) -> Job {
    start_jobs(
        vec![
            match_job(MatchPair::Par(
                target1.unwrap_or_else(|| panic!("{name}.p1 (target)")),
                pattern1.unwrap_or_else(|| panic!("{name}.p1 (pattern)")),
            )),
            match_job(MatchPair::Par(
                target2.unwrap_or_else(|| panic!("{name}.p2 (target)")),
                pattern2.unwrap_or_else(|| panic!("{name}.p2 (pattern)")),
            )),
        ],
        frames,
    )
}

macro_rules! binary_expr_arms {
    ($target:ident, $pattern:ident, $frames:ident; $(($variant:ident, $body:ident, $name:literal)),* $(,)?) => {
        match ($target, $pattern) {
            $((Some($variant($body { p1: t1, p2: t2 })), Some($variant($body { p1, p2 }))) =>
                binary_jobs(t1, t2, p1, p2, $name, $frames),)*
            _ => Job::Return(false),
        }
    };
}

fn evaluate_binary_expr(
    target: Option<ExprInstance>,
    pattern: Option<ExprInstance>,
    frames: &mut FrameStack,
) -> Job {
    binary_expr_arms!(target, pattern, frames;
        (EMultBody, EMult, "EMult"),
        (EDivBody, EDiv, "EDiv"),
        (EModBody, EMod, "EMod"),
        (EPercentPercentBody, EPercentPercent, "EPercentPercent"),
        (EPlusBody, EPlus, "EPlus"),
        (EPlusPlusBody, EPlusPlus, "EPlusPlus"),
        (EMinusMinusBody, EMinusMinus, "EMinusMinus"),
        (EMinusBody, EMinus, "EMinus"),
        (ELtBody, ELt, "ELt"),
        (ELteBody, ELte, "ELte"),
        (EGtBody, EGt, "EGt"),
        (EGteBody, EGte, "EGte"),
        (EEqBody, EEq, "EEq"),
        (ENeqBody, ENeq, "ENeq"),
        (EAndBody, EAnd, "EAnd"),
        (EOrBody, EOr, "EOr"),
    )
}

fn evaluate_match(target: Match, pattern: Match, frames: &mut FrameStack) -> Job {
    if target.cases.len() != pattern.cases.len() {
        return Job::Return(false);
    }
    let mut jobs = vec![match_job(MatchPair::Par(
        target.target.expect("Match.target (target)"),
        pattern.target.expect("Match.target (pattern)"),
    ))];
    jobs.extend(
        target
            .cases
            .into_iter()
            .zip(pattern.cases)
            .map(|(target, pattern)| match_job(MatchPair::MatchCase(target, pattern))),
    );
    start_jobs(jobs, frames)
}

fn evaluate_unforgeable(target: GUnforgeable, pattern: GUnforgeable) -> Job {
    match pattern.unf_instance {
        Some(GPrivateBody(_))
        | Some(GDeployIdBody(_))
        | Some(GDeployerIdBody(_))
        | Some(GSysAuthTokenBody(_)) => Job::Return(target == pattern),
        None => Job::Return(false),
    }
}

fn evaluate_if(target: If, pattern: If, frames: &mut FrameStack) -> Job {
    // `connective_used` is derived metadata, not an `If` child to match.  This
    // evaluator is reached precisely when the pattern is connective-aware, so
    // its bit may be true while the corresponding concrete target bit is
    // false.  Requiring equality here would reject the child connective before
    // the PDA gets a chance to interpret it.
    start_jobs(
        pair_jobs([
            MatchPair::Par(
                target.condition.expect("If.condition (target)"),
                pattern.condition.expect("If.condition (pattern)"),
            ),
            MatchPair::Par(
                target.if_true.expect("If.if_true (target)"),
                pattern.if_true.expect("If.if_true (pattern)"),
            ),
            MatchPair::Par(
                target.if_false.expect("If.if_false (target)"),
                pattern.if_false.expect("If.if_false (pattern)"),
            ),
        ]),
        frames,
    )
}

fn evaluate_encoded_if(
    target: EncodedRegion,
    pattern: EncodedRegion,
    frames: &mut FrameStack,
) -> Job {
    let Some(target_parts) = canonical_if_parts(target.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_if(target.as_slice())
            .expect("an authoritative If target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_if(pattern.as_slice())
            .expect("an authoritative If pattern endpoint is canonical");
        return evaluate_if(target, pattern, frames);
    };
    let Some(pattern_parts) = canonical_if_parts(pattern.as_slice()) else {
        let target = models::rust::rholang::protobuf_decoder::decode_if(target.as_slice())
            .expect("an authoritative If target endpoint is canonical");
        let pattern = models::rust::rholang::protobuf_decoder::decode_if(pattern.as_slice())
            .expect("an authoritative If pattern endpoint is canonical");
        return evaluate_if(target, pattern, frames);
    };

    let Some(target_condition) = target.child(target_parts.condition) else {
        unreachable!("canonical If condition is a subrange of its endpoint")
    };
    let Some(target_true) = target.child(target_parts.if_true) else {
        unreachable!("canonical If true branch is a subrange of its endpoint")
    };
    let Some(target_false) = target.child(target_parts.if_false) else {
        unreachable!("canonical If false branch is a subrange of its endpoint")
    };
    let Some(pattern_condition) = pattern.child(pattern_parts.condition) else {
        unreachable!("canonical If condition is a subrange of its endpoint")
    };
    let Some(pattern_true) = pattern.child(pattern_parts.if_true) else {
        unreachable!("canonical If true branch is a subrange of its endpoint")
    };
    let Some(pattern_false) = pattern.child(pattern_parts.if_false) else {
        unreachable!("canonical If false branch is a subrange of its endpoint")
    };

    // This is the matcher's deterministic proof-search continuation, not a
    // Rholang scheduling relation. Source processes remain a parallel bag.
    // One compact branch frame replaces a heap Vec of three heterogeneous
    // Jobs, so encoded traversal never inherits the decoded MatchPair size.
    frames.push(Frame::EncodedIf {
        branches: Box::new(EncodedIfBranches {
            if_true: Some((target_true, pattern_true)),
            if_false: (target_false, pattern_false),
        }),
    });
    Job::EncodedPar(target_condition, pattern_condition)
}

fn evaluate_receive_bind(target: ReceiveBind, pattern: ReceiveBind) -> Job {
    if target.patterns != pattern.patterns {
        Job::Return(false)
    } else {
        match_job(MatchPair::Par(
            target.source.expect("ReceiveBind.source (target)"),
            pattern.source.expect("ReceiveBind.source (pattern)"),
        ))
    }
}

fn evaluate_match_case(target: MatchCase, pattern: MatchCase) -> Job {
    if target.pattern != pattern.pattern {
        Job::Return(false)
    } else {
        match_job(MatchPair::Par(
            target.source.expect("MatchCase.source (target)"),
            pattern.source.expect("MatchCase.source (pattern)"),
        ))
    }
}

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd)]
enum MatchValue {
    Par(Par),
    ParPair((Par, Par)),
    EncodedPar(EncodedRegion),
    EncodedParPair((EncodedRegion, EncodedRegion)),
    Send(Send),
    Receive(Receive),
    New(New),
    Expr(Expr),
    Match(Match),
    Bundle(Bundle),
    Unforgeable(GUnforgeable),
    If(If),
    ReceiveBind(ReceiveBind),
}

impl MatchValue {
    fn shares_cow_bincode_identity(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Par(left), Self::Par(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::ParPair((left_key, left_value)), Self::ParPair((right_key, right_value))) => {
                shares_cow_bincode_identity(ParCursor::from(left_key), ParCursor::from(right_key))
                    && shares_cow_bincode_identity(
                        ParCursor::from(left_value),
                        ParCursor::from(right_value),
                    )
            }
            (Self::EncodedPar(left), Self::EncodedPar(right)) => left == right,
            (
                Self::EncodedParPair((left_key, left_value)),
                Self::EncodedParPair((right_key, right_value)),
            ) => left_key == right_key && left_value == right_value,
            (Self::Send(left), Self::Send(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::Receive(left), Self::Receive(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::New(left), Self::New(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::Expr(left), Self::Expr(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::Match(left), Self::Match(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::Bundle(left), Self::Bundle(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::Unforgeable(left), Self::Unforgeable(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::If(left), Self::If(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            (Self::ReceiveBind(left), Self::ReceiveBind(right)) => {
                shares_cow_bincode_identity(ParCursor::from(left), ParCursor::from(right))
            }
            _ => false,
        }
    }

    fn rho_path_structure(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(match self {
            Self::ParPair(_) | Self::EncodedParPair(_) => 2,
            _ => 1,
        });
        match self {
            Self::Par(value) => output.push(&encode_par_match_atom(value)),
            Self::ParPair((key, value)) => {
                output.push(&encode_par_match_atom(key));
                output.push(&encode_par_match_atom(value));
            }
            Self::EncodedPar(value) => output.push(value.as_slice()),
            Self::EncodedParPair((key, value)) => {
                output.push(key.as_slice());
                output.push(value.as_slice());
            }
            Self::Send(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Receive(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::New(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Expr(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Match(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Bundle(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Unforgeable(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::If(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::ReceiveBind(value) => output.push(&encode_match_path(ParCursor::from(value))),
        }
        output.finish()
    }

    /// Collision-free identity for every unordered `Par` field type. Each
    /// A resident generated-schema root uses the one-walk, stack-safe canonical
    /// bincode emitter. A borrowed collection member uses its already-validated
    /// canonical protobuf body. Both use the same injective sequence framing as
    /// structure, and a list never mixes the two representations.
    fn rho_path_identity(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(match self {
            Self::ParPair(_) | Self::EncodedParPair(_) => 2,
            _ => 1,
        });
        macro_rules! push_identity {
            ($value:expr) => {
                bincode_encoder::with_encoded($value, |encoded| output.push(encoded))
            };
        }
        match self {
            Self::Par(value) => push_identity!(value),
            Self::ParPair((key, value)) => {
                push_identity!(key);
                push_identity!(value);
            }
            Self::EncodedPar(value) => output.push(value.as_slice()),
            Self::EncodedParPair((key, value)) => {
                output.push(key.as_slice());
                output.push(value.as_slice());
            }
            Self::Send(value) => push_identity!(value),
            Self::Receive(value) => push_identity!(value),
            Self::New(value) => push_identity!(value),
            Self::Expr(value) => push_identity!(value),
            Self::Match(value) => push_identity!(value),
            Self::Bundle(value) => push_identity!(value),
            Self::Unforgeable(value) => push_identity!(value),
            Self::If(value) => push_identity!(value),
            Self::ReceiveBind(value) => push_identity!(value),
        }
        output.finish()
    }

    fn rho_path_query(
        &self,
        context: &SpatialMatcherContext,
        captures: &mut PathMap<()>,
    ) -> Option<RhoPathQuery> {
        if matches!(self, Self::EncodedPar(_) | Self::EncodedParPair(_)) {
            // The byte-native collection relation remains authoritative. A
            // future protobuf-to-RMP2 cursor may add conservative pruning;
            // `None` means the current proof search scans the full domain.
            return None;
        }
        let exact = !self.connective_used(context) && self.locally_free_is_empty(context);
        let atoms = match self {
            Self::Par(value) => vec![rho_path_query_atom(value, captures)?],
            Self::ParPair((key, value)) => vec![
                rho_path_query_atom(key, captures)?,
                rho_path_query_atom(value, captures)?,
            ],
            Self::EncodedPar(_) | Self::EncodedParPair(_) => unreachable!(),
            Self::Send(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::Receive(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::New(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::Expr(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::Match(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::Bundle(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::Unforgeable(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::If(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
            Self::ReceiveBind(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
        };
        Some(RhoPathQuery::structural_sequence(atoms, false))
    }

    /// Compile the conservative byte-native candidate domain for one AC row.
    ///
    /// Ordinary values retain the generated single-query compiler. A `Par`
    /// whose only semantic content is one connective receives a small algebra
    /// over those same queries: disjunction is candidate-set union and
    /// conjunction is candidate-set intersection. The compilation walk is an
    /// explicit heap PDA, so arbitrarily nested connective formulas do not
    /// consume native stack. Unsupported positive arms denote `All`; this can
    /// only admit extra candidates. Negation also denotes `All`, because the
    /// complement of a conservative superset would be unsound. The spatial
    /// matcher remains the sole edge authority.
    fn rho_path_candidate_plan(
        &self,
        context: &SpatialMatcherContext,
        captures: &mut PathMap<()>,
    ) -> ListCandidatePlan {
        match self {
            Self::Par(pattern) => compile_par_candidate_plan(pattern, captures),
            Self::EncodedPar(_) | Self::EncodedParPair(_) => ListCandidatePlan::all(),
            _ => self
                .rho_path_query(context, captures)
                .map_or_else(ListCandidatePlan::all, ListCandidatePlan::query),
        }
    }

    fn connective_used(&self, _context: &SpatialMatcherContext) -> bool {
        match self {
            MatchValue::Par(value) => value.connective_used(),
            MatchValue::ParPair((key, value)) => key.connective_used() || value.connective_used(),
            MatchValue::EncodedPar(value) => canonical_par_connective_used(value.as_slice())
                .expect("an authoritative encoded collection member is canonical"),
            MatchValue::EncodedParPair((key, value)) => {
                canonical_par_connective_used(key.as_slice())
                    .expect("an authoritative encoded map key is canonical")
                    || canonical_par_connective_used(value.as_slice())
                        .expect("an authoritative encoded map value is canonical")
            }
            MatchValue::Send(value) => value.connective_used,
            MatchValue::Receive(value) => value.connective_used,
            MatchValue::New(value) => value.p.as_ref().expect("New.p").connective_used(),
            MatchValue::Expr(value) => expr_connective_used(value),
            MatchValue::Match(value) => value.connective_used,
            MatchValue::If(value) => value.connective_used,
            MatchValue::Bundle(_) | MatchValue::Unforgeable(_) => false,
            MatchValue::ReceiveBind(value) => value
                .source
                .as_ref()
                .expect("ReceiveBind.source")
                .connective_used(),
        }
    }

    fn locally_free_is_empty(&self, context: &SpatialMatcherContext) -> bool {
        match self {
            MatchValue::Par(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::ParPair(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::EncodedPar(value) => canonical_par_locally_free_is_empty(value.as_slice())
                .expect("an authoritative encoded collection member is canonical"),
            MatchValue::EncodedParPair((key, value)) => {
                canonical_par_locally_free_is_empty(key.as_slice())
                    .expect("an authoritative encoded map key is canonical")
                    && canonical_par_locally_free_is_empty(value.as_slice())
                        .expect("an authoritative encoded map value is canonical")
            }
            MatchValue::Send(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Receive(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::New(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Expr(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Match(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Bundle(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Unforgeable(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::If(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::ReceiveBind(value) => context.locally_free(value.clone(), 0).is_empty(),
        }
    }

    fn semantic_eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::EncodedPar(target), Self::EncodedPar(pattern)) => {
                target == pattern || decode_encoded_par(target) == decode_encoded_par(pattern)
            }
            (
                Self::EncodedParPair((target_key, target_value)),
                Self::EncodedParPair((pattern_key, pattern_value)),
            ) => {
                (target_key == pattern_key
                    || decode_encoded_par(target_key) == decode_encoded_par(pattern_key))
                    && (target_value == pattern_value
                        || decode_encoded_par(target_value) == decode_encoded_par(pattern_value))
            }
            _ => self == other,
        }
    }

    fn is_encoded_collection_value(&self) -> bool {
        matches!(self, Self::EncodedPar(_) | Self::EncodedParPair(_))
    }

    fn into_job(self, pattern: MatchValue, frames: &mut FrameStack) -> Option<Job> {
        match (self, pattern) {
            (MatchValue::Par(target), MatchValue::Par(pattern)) => {
                Some(match_job(MatchPair::Par(target, pattern)))
            }
            (MatchValue::ParPair(target), MatchValue::ParPair(pattern)) => {
                Some(match_job(MatchPair::ParPair(target, pattern)))
            }
            (MatchValue::EncodedPar(target), MatchValue::EncodedPar(pattern)) => {
                Some(Job::EncodedPar(target, pattern))
            }
            (
                MatchValue::EncodedParPair((target_key, target_value)),
                MatchValue::EncodedParPair((pattern_key, pattern_value)),
            ) => {
                frames.push(Frame::EncodedSibling {
                    sibling: Box::new((target_value, pattern_value)),
                });
                Some(Job::EncodedPar(target_key, pattern_key))
            }
            (MatchValue::Send(target), MatchValue::Send(pattern)) => {
                Some(match_job(MatchPair::Send(target, pattern)))
            }
            (MatchValue::Receive(target), MatchValue::Receive(pattern)) => {
                Some(match_job(MatchPair::Receive(target, pattern)))
            }
            (MatchValue::New(target), MatchValue::New(pattern)) => {
                Some(match_job(MatchPair::New(target, pattern)))
            }
            (MatchValue::Expr(target), MatchValue::Expr(pattern)) => {
                Some(match_job(MatchPair::Expr(target, pattern)))
            }
            (MatchValue::Match(target), MatchValue::Match(pattern)) => {
                Some(match_job(MatchPair::Match(target, pattern)))
            }
            (MatchValue::Bundle(target), MatchValue::Bundle(pattern)) => {
                Some(match_job(MatchPair::Bundle(target, pattern)))
            }
            (MatchValue::Unforgeable(target), MatchValue::Unforgeable(pattern)) => {
                Some(match_job(MatchPair::Unforgeable(target, pattern)))
            }
            (MatchValue::If(target), MatchValue::If(pattern)) => {
                Some(match_job(MatchPair::If(target, pattern)))
            }
            (MatchValue::ReceiveBind(target), MatchValue::ReceiveBind(pattern)) => {
                Some(match_job(MatchPair::ReceiveBind(target, pattern)))
            }
            _ => None,
        }
    }
}

fn decode_encoded_par(value: &EncodedRegion) -> Par {
    models::rust::rholang::protobuf_decoder::decode_par(value.as_slice())
        .expect("an authoritative encoded collection member is canonical")
}

#[derive(Clone, Copy)]
enum CandidateNary {
    Union,
    Intersection,
}

enum CandidateCompileWork<'a> {
    EnterResident(Par),
    EnterEncodedPar(&'a [u8]),
    EnterEncodedConnective(&'a [u8]),
    EnterEncodedChildren {
        children: CanonicalConnectiveChildren<'a>,
        kind: CandidateNary,
    },
    FinishUnion(usize),
    FinishIntersection(usize),
}

/// Candidate-set expression produced by the connective compiler.
///
/// `All` is the deliberate conservative fallback and `Empty` is reserved for
/// connectives whose authoritative matcher always refuses (`VarRef`/absent or
/// an empty disjunction). Query leaves contain only the already-proved RMP2
/// compiler output; this layer adds no byte interpretation of its own. Child
/// edges are arena indices, so inspecting and dropping a deep branching plan
/// never re-enters Rust's native call stack.
enum CandidatePlanNodeKind {
    All,
    Empty,
    Query(RhoPathQuery),
    Union(Box<[CandidatePlanNodeId]>),
    Intersection(Box<[CandidatePlanNodeId]>),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct CandidatePlanNodeId(usize);

struct CandidatePlanNode {
    kind: CandidatePlanNodeKind,
    needs_target_index: bool,
}

struct CandidatePlanBuilder {
    nodes: Vec<CandidatePlanNode>,
    // Scheduling metadata is builder-only.  The finished plan retains only
    // semantic nodes; evaluation and long-lived caches never pay for it.
    retention_pressure: Vec<usize>,
    work_weight: Vec<usize>,
}

impl CandidatePlanBuilder {
    fn new() -> Self {
        Self {
            nodes: Vec::new(),
            retention_pressure: Vec::new(),
            work_weight: Vec::new(),
        }
    }

    fn push(&mut self, kind: CandidatePlanNodeKind) -> CandidatePlanNodeId {
        let needs_target_index = match &kind {
            CandidatePlanNodeKind::All | CandidatePlanNodeKind::Empty => false,
            CandidatePlanNodeKind::Query(query) => query.retained_prefix().len() > 1,
            CandidatePlanNodeKind::Union(children)
            | CandidatePlanNodeKind::Intersection(children) => children
                .iter()
                .any(|child| self.nodes[child.0].needs_target_index),
        };
        let work_weight = match &kind {
            CandidatePlanNodeKind::All
            | CandidatePlanNodeKind::Empty
            | CandidatePlanNodeKind::Query(_) => 1,
            CandidatePlanNodeKind::Union(children)
            | CandidatePlanNodeKind::Intersection(children) => {
                children.iter().fold(1usize, |sum, child| {
                    sum.saturating_add(self.work_weight[child.0])
                })
            }
        };
        // Ershov/Sethi-Ullman pressure for the streaming one-accumulator
        // evaluator.  The greatest-pressure child is already first when an
        // n-ary node reaches `push`: the first child has no retained parent
        // accumulator, every later child has one, and a finite/finite lattice
        // operation can transiently own left, right and result.
        let retention_pressure = match &kind {
            CandidatePlanNodeKind::All => 0,
            CandidatePlanNodeKind::Empty => 1,
            CandidatePlanNodeKind::Query(_) => 1,
            CandidatePlanNodeKind::Union(children)
            | CandidatePlanNodeKind::Intersection(children) => {
                debug_assert!(children.len() >= 2);
                let first = self.retention_pressure[children[0].0];
                let rest = children[1..]
                    .iter()
                    .map(|child| self.retention_pressure[child.0])
                    .max()
                    .unwrap_or(0);
                first.max(rest.saturating_add(1)).max(3)
            }
        };
        let id = CandidatePlanNodeId(self.nodes.len());
        self.nodes.push(CandidatePlanNode {
            kind,
            needs_target_index,
        });
        self.retention_pressure.push(retention_pressure);
        self.work_weight.push(work_weight);
        #[cfg(test)]
        update_list_relation_stats(|stats| stats.candidate_plan_nodes += 1);
        id
    }

    fn all(&mut self) -> CandidatePlanNodeId { self.push(CandidatePlanNodeKind::All) }

    fn empty(&mut self) -> CandidatePlanNodeId { self.push(CandidatePlanNodeKind::Empty) }

    fn query(&mut self, query: RhoPathQuery) -> CandidatePlanNodeId {
        #[cfg(feature = "spatial-matcher-candidate-mask-differential")]
        if FORCE_OCCURRENCE_CANDIDATE_MASKS.with(Cell::get) {
            return self.push(CandidatePlanNodeKind::Query(query));
        }
        if query.retained_prefix().len() <= 1 {
            self.all()
        } else {
            self.push(CandidatePlanNodeKind::Query(query))
        }
    }

    fn is_all(&self, id: CandidatePlanNodeId) -> bool {
        matches!(self.nodes[id.0].kind, CandidatePlanNodeKind::All)
    }

    fn is_empty(&self, id: CandidatePlanNodeId) -> bool {
        matches!(self.nodes[id.0].kind, CandidatePlanNodeKind::Empty)
    }

    /// Put only the greatest-pressure child first.  Selecting one maximum is
    /// linear in arity and is sufficient for a one-accumulator fold; every
    /// later child necessarily runs under exactly one retained accumulator.
    /// Work breaks pressure ties to reduce time-integrated retention.  A
    /// strict comparison preserves source order on an exact tie.
    fn place_pressure_first(&self, operands: &mut [CandidatePlanNodeId]) {
        #[cfg(feature = "spatial-matcher-candidate-mask-differential")]
        if FORCE_OCCURRENCE_CANDIDATE_MASKS.with(Cell::get) {
            return;
        }
        #[cfg(test)]
        if FORCE_CANDIDATE_NODE_COUNT_ORDER.with(Cell::get) {
            let mut heaviest = 0;
            for candidate in 1..operands.len() {
                if self.work_weight[operands[candidate].0] > self.work_weight[operands[heaviest].0]
                {
                    heaviest = candidate;
                }
            }
            operands.swap(0, heaviest);
            return;
        }

        let mut greatest = 0;
        for candidate in 1..operands.len() {
            let candidate_id = operands[candidate].0;
            let greatest_id = operands[greatest].0;
            if (
                self.retention_pressure[candidate_id],
                self.work_weight[candidate_id],
            ) > (
                self.retention_pressure[greatest_id],
                self.work_weight[greatest_id],
            ) {
                greatest = candidate;
            }
        }
        operands.swap(0, greatest);
    }

    fn union(&mut self, mut operands: Vec<CandidatePlanNodeId>) -> CandidatePlanNodeId {
        if operands.iter().copied().any(|operand| self.is_all(operand)) {
            return self.all();
        }
        operands.retain(|operand| !self.is_empty(*operand));
        match operands.len() {
            0 => self.empty(),
            1 => operands.pop().expect("one union operand remains"),
            _ => {
                self.place_pressure_first(&mut operands);
                self.push(CandidatePlanNodeKind::Union(operands.into_boxed_slice()))
            }
        }
    }

    fn intersection(&mut self, mut operands: Vec<CandidatePlanNodeId>) -> CandidatePlanNodeId {
        if operands
            .iter()
            .copied()
            .any(|operand| self.is_empty(operand))
        {
            return self.empty();
        }
        operands.retain(|operand| !self.is_all(*operand));
        match operands.len() {
            0 => self.all(),
            1 => operands.pop().expect("one intersection operand remains"),
            _ => {
                self.place_pressure_first(&mut operands);
                self.push(CandidatePlanNodeKind::Intersection(
                    operands.into_boxed_slice(),
                ))
            }
        }
    }

    fn finish(self, root: CandidatePlanNodeId) -> ListCandidatePlan {
        ListCandidatePlan {
            nodes: self.nodes,
            root,
        }
    }
}

struct ListCandidatePlan {
    nodes: Vec<CandidatePlanNode>,
    root: CandidatePlanNodeId,
}

impl ListCandidatePlan {
    fn singleton(kind: CandidatePlanNodeKind) -> Self {
        let mut builder = CandidatePlanBuilder::new();
        let root = builder.push(kind);
        builder.finish(root)
    }

    fn all() -> Self { Self::singleton(CandidatePlanNodeKind::All) }

    fn query(query: RhoPathQuery) -> Self { Self::singleton(CandidatePlanNodeKind::Query(query)) }

    fn node(&self, id: CandidatePlanNodeId) -> &CandidatePlanNode { &self.nodes[id.0] }

    fn needs_target_index(&self) -> bool { self.node(self.root).needs_target_index }

    fn is_empty(&self) -> bool { matches!(self.node(self.root).kind, CandidatePlanNodeKind::Empty) }
}

fn is_active_single_connective_carrier(pattern: &Par) -> bool {
    pattern.connective_used()
        && pattern.family_cardinality::<Connective>() == 1
        && pattern.all_components_are::<Connective>()
}

fn active_single_connective_carrier(pattern: &Par) -> Option<Connective> {
    is_active_single_connective_carrier(pattern).then(|| {
        pattern
            .first::<Connective>()
            .expect("resident Par Connective endpoints are canonical")
            .expect("the family cardinality proved one Connective")
            .0
    })
}

fn compile_par_candidate_plan(root: &Par, captures: &mut PathMap<()>) -> ListCandidatePlan {
    #[cfg(feature = "spatial-matcher-connective-domain-differential")]
    if is_active_single_connective_carrier(root) && FORCE_CONNECTIVE_DOMAIN_ALL.with(Cell::get) {
        return ListCandidatePlan::all();
    }

    if let Some(terminal) = root.canonical_unary_connective_terminal() {
        return rho_path_query_atom(&terminal, captures)
            .map(|atom| ListCandidatePlan::query(RhoPathQuery::structural_sequence([atom], false)))
            .unwrap_or_else(ListCandidatePlan::all);
    }

    if is_active_single_connective_carrier(root) {
        let mut encoded = None;
        root.visit_encoded::<Connective>(|entry| {
            if entry.multiplicity().get() == 1 {
                encoded = Some(compile_candidate_plan(
                    vec![CandidateCompileWork::EnterEncodedConnective(
                        entry.payload(),
                    )],
                    captures,
                ));
            }
        });
        if let Some(plan) = encoded {
            return plan;
        }
    }

    compile_candidate_plan(
        vec![CandidateCompileWork::EnterResident(root.clone())],
        captures,
    )
}

fn compile_candidate_plan<'a>(
    mut work: Vec<CandidateCompileWork<'a>>,
    captures: &mut PathMap<()>,
) -> ListCandidatePlan {
    let mut builder = CandidatePlanBuilder::new();
    let mut values: Vec<CandidatePlanNodeId> = Vec::new();

    while let Some(next) = work.pop() {
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.candidate_max_work = stats.candidate_max_work.max(work.len() + 1);
            stats.candidate_max_values = stats.candidate_max_values.max(values.len());
        });
        match next {
            CandidateCompileWork::EnterResident(pattern) => {
                let Some(connective) = active_single_connective_carrier(&pattern) else {
                    let node = rho_path_query_atom(&pattern, captures)
                        .map(|atom| builder.query(RhoPathQuery::structural_sequence([atom], false)))
                        .unwrap_or_else(|| builder.all());
                    values.push(node);
                    continue;
                };
                #[cfg(test)]
                update_list_relation_stats(|stats| {
                    stats.candidate_resident_connective_decodes += 1;
                });

                match connective.connective_instance {
                    Some(ConnOrBody(ConnectiveBody { ps })) => {
                        work.push(CandidateCompileWork::FinishUnion(ps.len()));
                        work.extend(
                            ps.into_iter()
                                .rev()
                                .map(CandidateCompileWork::EnterResident),
                        );
                    }
                    Some(ConnAndBody(ConnectiveBody { ps })) => {
                        work.push(CandidateCompileWork::FinishIntersection(ps.len()));
                        work.extend(
                            ps.into_iter()
                                .rev()
                                .map(CandidateCompileWork::EnterResident),
                        );
                    }
                    // A conservative child query Q satisfies success ⊆ Q.
                    // Complementing it would reverse that inclusion, so NOT
                    // deliberately contributes no pruning.
                    Some(ConnNotBody(_))
                    | Some(ConnBool(_))
                    | Some(ConnInt(_))
                    | Some(ConnString(_))
                    | Some(ConnUri(_))
                    | Some(ConnByteArray(_)) => values.push(builder.all()),
                    // These arms are unconditional refusal in
                    // `evaluate_connective`, hence their candidate set really
                    // is empty rather than merely unknown.
                    Some(VarRefBody(_)) | None => values.push(builder.empty()),
                }
            }
            CandidateCompileWork::EnterEncodedPar(pattern) => {
                let connective = (canonical_par_connective_used(pattern) == Some(true))
                    .then(|| canonical_single_connective_from_par(pattern))
                    .flatten();
                if let Some(connective) = connective {
                    work.push(CandidateCompileWork::EnterEncodedConnective(connective));
                } else {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| stats.candidate_terminal_par_decodes += 1);
                    let pattern = models::rust::rholang::protobuf_decoder::decode_par(pattern)
                        .expect("an authoritative connective child Par is canonical");
                    let node = rho_path_query_atom(&pattern, captures)
                        .map(|atom| builder.query(RhoPathQuery::structural_sequence([atom], false)))
                        .unwrap_or_else(|| builder.all());
                    values.push(node);
                }
            }
            CandidateCompileWork::EnterEncodedConnective(connective) => {
                #[cfg(test)]
                update_list_relation_stats(|stats| {
                    stats.candidate_encoded_connective_nodes += 1;
                });
                let Some(parts) = canonical_connective_parts(connective) else {
                    // The bytes came from an authoritative ParBag endpoint.
                    // If a future schema extension is not yet proven by the
                    // cursor, delegate through the complete resident path.
                    let decoded = <Connective as prost::Message>::decode(connective)
                        .expect("an authoritative Connective endpoint is canonical");
                    let pattern = Par::default()
                        .with_connectives(vec![decoded])
                        .with_connective_used(true);
                    work.push(CandidateCompileWork::EnterResident(pattern));
                    continue;
                };
                match parts {
                    CanonicalConnectiveParts::Or(children) => {
                        work.push(CandidateCompileWork::FinishUnion(children.len()));
                        work.push(CandidateCompileWork::EnterEncodedChildren {
                            children,
                            kind: CandidateNary::Union,
                        });
                    }
                    CanonicalConnectiveParts::And(children) => {
                        work.push(CandidateCompileWork::FinishIntersection(children.len()));
                        work.push(CandidateCompileWork::EnterEncodedChildren {
                            children,
                            kind: CandidateNary::Intersection,
                        });
                    }
                    CanonicalConnectiveParts::Not(_)
                    | CanonicalConnectiveParts::Predicate { .. } => values.push(builder.all()),
                    CanonicalConnectiveParts::VarRef | CanonicalConnectiveParts::Absent => {
                        values.push(builder.empty());
                    }
                }
            }
            CandidateCompileWork::EnterEncodedChildren { mut children, kind } => {
                if let Some(child) = children.next() {
                    work.push(CandidateCompileWork::EnterEncodedChildren { children, kind });
                    work.push(CandidateCompileWork::EnterEncodedPar(child));
                }
            }
            CandidateCompileWork::FinishUnion(arity) => {
                let split = values
                    .len()
                    .checked_sub(arity)
                    .expect("connective union has one value per child");
                let operands = values.split_off(split);
                let root = builder.union(operands);
                values.push(root);
            }
            CandidateCompileWork::FinishIntersection(arity) => {
                let split = values
                    .len()
                    .checked_sub(arity)
                    .expect("connective intersection has one value per child");
                let operands = values.split_off(split);
                let root = builder.intersection(operands);
                values.push(root);
            }
        }
    }

    debug_assert_eq!(values.len(), 1, "candidate compiler leaves one root value");
    let root = values
        .pop()
        .expect("candidate compiler emits its root value");
    builder.finish(root)
}

fn expr_connective_used(expr: &Expr) -> bool {
    match expr.expr_instance.as_ref() {
        Some(
            GBool(_) | GInt(_) | GDouble(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
            | GUri(_) | GByteArray(_),
        )
        | None => false,
        Some(EListBody(value)) => value.connective_used,
        Some(ETupleBody(value)) => value.connective_used,
        Some(ESetBody(value)) => value.connective_used,
        Some(EMapBody(value)) => value.connective_used,
        Some(EPathmapBody(value)) => value.connective_used,
        Some(EZipperBody(value)) => value.connective_used,
        Some(EVarBody(EVar { v })) => matches!(
            v.as_ref()
                .expect("EVar.v")
                .var_instance
                .as_ref()
                .expect("Var.var_instance"),
            FreeVar(_) | Wildcard(_)
        ),
        Some(ENotBody(ENot { p })) => p.as_ref().expect("ENot.p").connective_used(),
        Some(ENegBody(ENeg { p })) => p.as_ref().expect("ENeg.p").connective_used(),
        Some(EMultBody(EMult { p1, p2 }))
        | Some(EDivBody(EDiv { p1, p2 }))
        | Some(EModBody(EMod { p1, p2 }))
        | Some(EPercentPercentBody(EPercentPercent { p1, p2 }))
        | Some(EPlusBody(EPlus { p1, p2 }))
        | Some(EPlusPlusBody(EPlusPlus { p1, p2 }))
        | Some(EMinusMinusBody(EMinusMinus { p1, p2 }))
        | Some(EMinusBody(EMinus { p1, p2 }))
        | Some(ELtBody(ELt { p1, p2 }))
        | Some(ELteBody(ELte { p1, p2 }))
        | Some(EGtBody(EGt { p1, p2 }))
        | Some(EGteBody(EGte { p1, p2 }))
        | Some(EEqBody(EEq { p1, p2 }))
        | Some(ENeqBody(ENeq { p1, p2 }))
        | Some(EAndBody(EAnd { p1, p2 }))
        | Some(EOrBody(EOr { p1, p2 })) => {
            p1.as_ref().expect("binary expression p1").connective_used()
                || p2.as_ref().expect("binary expression p2").connective_used()
        }
        Some(EMethodBody(value)) => value.connective_used,
        Some(EMatchesBody(EMatches { target, .. })) => {
            target.as_ref().expect("EMatches.target").connective_used()
        }
    }
}

#[derive(Clone, Copy)]
enum Merger {
    Identity,
    ParSends,
    ParReceives,
    ParNews,
    ParExprs,
    ParMatches,
    ParBundles,
    ParUnforgeables,
    ParConditionals,
    ESet,
    EMap,
}

impl Merger {
    fn merge(self, par: Par, values: Vec<MatchValue>) -> Par {
        macro_rules! typed {
            ($variant:ident) => {
                values
                    .into_iter()
                    .map(|value| match value {
                        MatchValue::$variant(value) => value,
                        _ => panic!(concat!("list merger expected ", stringify!($variant))),
                    })
                    .collect::<Vec<_>>()
            };
        }

        match self {
            Merger::Identity => par,
            Merger::ParSends => par.with_sends(typed!(Send)),
            Merger::ParReceives => par.with_receives(typed!(Receive)),
            Merger::ParNews => par.with_news(typed!(New)),
            Merger::ParExprs => par.with_exprs(typed!(Expr)),
            Merger::ParMatches => par.with_matches(typed!(Match)),
            Merger::ParBundles => par.with_bundles(typed!(Bundle)),
            Merger::ParUnforgeables => par.with_unforgeables(typed!(Unforgeable)),
            Merger::ParConditionals => par.with_conditionals(typed!(If)),
            Merger::ESet => par.with_exprs(vec![new_eset_expr(
                values
                    .into_iter()
                    .map(|value| match value {
                        MatchValue::Par(value) => value,
                        MatchValue::EncodedPar(value) => decode_encoded_par(&value),
                        _ => panic!("ESet merger expected Par"),
                    })
                    .collect(),
                Vec::new(),
                false,
                None,
            )]),
            Merger::EMap => par.with_exprs(vec![new_emap_expr(
                values
                    .into_iter()
                    .map(|value| {
                        let (key, value) = match value {
                            MatchValue::ParPair(value) => value,
                            MatchValue::EncodedParPair((key, value)) => {
                                (decode_encoded_par(&key), decode_encoded_par(&value))
                            }
                            _ => panic!("EMap merger expected Par pair"),
                        };
                        KeyValuePair {
                            key: Some(key),
                            value: Some(value),
                        }
                    })
                    .collect(),
                Vec::new(),
                false,
                None,
            )]),
        }
    }
}

struct ListRequest {
    targets: Vec<MatchValue>,
    patterns: Vec<MatchValue>,
    merger: Merger,
    remainder: Option<i32>,
    wildcard: bool,
}

impl ListRequest {
    fn new(
        targets: Vec<MatchValue>,
        patterns: Vec<MatchValue>,
        merger: Merger,
        remainder: Option<i32>,
        wildcard: bool,
    ) -> Self {
        Self {
            targets,
            patterns,
            merger,
            remainder,
            wildcard,
        }
    }
}

enum ListCandidateDomain {
    All,
    Indexed(Box<[u64]>),
}

impl ListCandidateDomain {
    fn len(&self, target_count: usize) -> usize {
        match self {
            Self::All => target_count,
            Self::Indexed(positions) => positions.len(),
        }
    }

    fn target_at(&self, candidate_index: usize) -> usize {
        match self {
            Self::All => candidate_index,
            Self::Indexed(positions) => usize::try_from(positions[candidate_index])
                .expect("a list target position originated as usize"),
        }
    }
}

#[derive(Clone, Copy)]
struct ListTargetGroupId(NonZeroU64);

impl ListTargetGroupId {
    fn path(self) -> [u8; 8] { self.0.get().to_be_bytes() }

    fn index(self) -> usize {
        usize::try_from(self.0.get() - 1)
            .expect("a target group id originated as an addressable Vec index")
    }
}

struct ListTargetGroup {
    key: Vec<u8>,
    structure: Vec<u8>,
    positions: Vec<u64>,
}

/// Derived prefix-compressed target catalog for one unordered list match.
///
/// The authoritative trie is a [`Bag`]: a `PathMap<Multiplicity>` whose key is
/// the framed RMP2 structure followed by a collision-free identity. Resident
/// values use stack-safe canonical bincode; borrowed collection values use
/// their canonical protobuf region. Structurally equal values therefore share
/// trie prefixes while byte-identical values share one multiplicity cell. A parallel
/// fixed-width group id reaches the only position vectors retained by this
/// derived matcher index. Those positions are an ordering witness, not the
/// membership representation; the Bag remains authoritative for cardinality.
struct ListTargetIndex {
    bag: Bag,
    group_ids: PathMap<ListTargetGroupId>,
    groups: Vec<ListTargetGroup>,
    position_groups: Box<[ListTargetGroupId]>,
}

impl ListTargetIndex {
    fn from_targets(targets: &[MatchValue]) -> Option<Self> {
        let mut bag = Bag::new();
        let mut group_ids: PathMap<ListTargetGroupId> = PathMap::new();
        let mut groups: Vec<ListTargetGroup> = Vec::new();
        let mut position_groups = Vec::with_capacity(targets.len());
        let mut previous: Option<(&MatchValue, ListTargetGroupId)> = None;
        for (position, target) in targets.iter().enumerate() {
            let position = u64::try_from(position).ok()?;

            // Canonically normalized AC fields place byte-identical terms next
            // to one another. Semantic `Ord`/`Eq` cannot prove wire identity:
            // in particular, `Par` intentionally ignores root `locally_free`.
            // The generated lockstep cursor instead compares every scalar and
            // accepts opaque Bag/PathMap fields only when they share an
            // immutable CoW root. The ordinary exact-key lookup below remains
            // authoritative when that conservative proof is unavailable.
            if let Some((previous_target, id)) = previous {
                if previous_target.shares_cow_bincode_identity(target) {
                    let group_index = usize::try_from(id.0.get() - 1).ok()?;
                    let group = groups.get_mut(group_index)?;
                    bag.insert(&group.key).ok()?;
                    group.positions.push(position);
                    position_groups.push(id);
                    previous = Some((target, id));
                    continue;
                }
            }

            #[cfg(test)]
            update_list_relation_stats(|stats| stats.target_key_encodes += 1);
            let structure = target.rho_path_structure();
            let identity = target.rho_path_identity();
            let key = RhoPathV1::relative_datum_key(&structure, &identity);
            bag.insert(&key).ok()?;
            let id = if let Some(id) = group_ids.get(&key).copied() {
                let group_index = usize::try_from(id.0.get() - 1).ok()?;
                groups.get_mut(group_index)?.positions.push(position);
                id
            } else {
                let raw_id = u64::try_from(groups.len()).ok()?.checked_add(1)?;
                let id = ListTargetGroupId(NonZeroU64::new(raw_id)?);
                groups.push(ListTargetGroup {
                    key: key.clone(),
                    structure,
                    positions: vec![position],
                });
                group_ids.insert(&key, id);
                id
            };
            position_groups.push(id);
            previous = Some((target, id));
        }
        debug_assert_eq!(
            bag.cardinality(),
            u64::try_from(targets.len()).expect("target count fits u64")
        );
        debug_assert_eq!(bag.distinct_len(), groups.len());
        Some(Self {
            bag,
            group_ids,
            groups,
            position_groups: position_groups.into_boxed_slice(),
        })
    }

    fn group_at_position(&self, position: usize) -> &ListTargetGroup {
        let id = self.position_groups[position];
        &self.groups[id.index()]
    }

    fn group_mask_matching(&self, query: &RhoPathQuery) -> Option<PathMap<()>> {
        let structural_prefix = query.retained_prefix();
        if structural_prefix.len() <= 1 {
            return None;
        }
        let prefix = RhoPathV1::relative_datum_structure_prefix(&structural_prefix);
        let mut result = PathMap::new();
        self.bag.visit_prefix(&prefix, |key, multiplicity| {
            let id = self
                .group_ids
                .get(key)
                .copied()
                .expect("every Bag identity has a target group");
            let group = &self.groups[id.index()];
            debug_assert_eq!(
                usize::try_from(multiplicity.get()).ok(),
                Some(group.positions.len())
            );
            if query.matches_structure(&group.structure) {
                result.insert(id.path(), ());
            }
        });
        Some(result)
    }

    #[cfg(feature = "spatial-matcher-candidate-mask-differential")]
    fn occurrence_mask_matching_control(&self, query: &RhoPathQuery) -> Option<PathMap<()>> {
        let structural_prefix = query.retained_prefix();
        if structural_prefix.len() <= 1 {
            return None;
        }
        let prefix = RhoPathV1::relative_datum_structure_prefix(&structural_prefix);
        let mut result = PathMap::new();
        self.bag.visit_prefix(&prefix, |key, multiplicity| {
            let id = self
                .group_ids
                .get(key)
                .copied()
                .expect("every Bag identity has a target group");
            let group = &self.groups[id.index()];
            debug_assert_eq!(
                usize::try_from(multiplicity.get()).ok(),
                Some(group.positions.len())
            );
            if query.matches_structure(&group.structure) {
                for position in &group.positions {
                    result.insert(position.to_be_bytes(), ());
                }
            }
        });
        Some(result)
    }

    /// Immutable benchmark-only reconstruction of the pre-fix evaluator.
    /// It expands duplicate occurrences into every leaf mask, retains all
    /// sibling masks through postorder, and sorts materialized positions.
    #[cfg(feature = "spatial-matcher-candidate-mask-differential")]
    fn positions_matching_plan_occurrence_control(
        &self,
        root: &ListCandidatePlan,
    ) -> Option<Box<[u64]>> {
        enum Work {
            Enter(CandidatePlanNodeId),
            FinishUnion(usize),
            FinishIntersection(usize),
        }

        let mut work = vec![Work::Enter(root.root)];
        let mut values: Vec<Option<PathMap<()>>> = Vec::new();
        while let Some(next) = work.pop() {
            match next {
                Work::Enter(id) => match &root.node(id).kind {
                    CandidatePlanNodeKind::All => values.push(None),
                    CandidatePlanNodeKind::Empty => values.push(Some(PathMap::new())),
                    CandidatePlanNodeKind::Query(query) => {
                        values.push(self.occurrence_mask_matching_control(query));
                    }
                    CandidatePlanNodeKind::Union(operands) => {
                        work.push(Work::FinishUnion(operands.len()));
                        work.extend(operands.iter().rev().copied().map(Work::Enter));
                    }
                    CandidatePlanNodeKind::Intersection(operands) => {
                        work.push(Work::FinishIntersection(operands.len()));
                        work.extend(operands.iter().rev().copied().map(Work::Enter));
                    }
                },
                Work::FinishUnion(arity) => {
                    let split = values
                        .len()
                        .checked_sub(arity)
                        .expect("candidate union has one domain per operand");
                    let operands = values.split_off(split);
                    if operands.iter().any(Option::is_none) {
                        values.push(None);
                    } else {
                        let mut operands = operands.into_iter().flatten();
                        let mut result = operands.next().unwrap_or_default();
                        for operand in operands {
                            result = result.join(&operand);
                        }
                        values.push(Some(result));
                    }
                }
                Work::FinishIntersection(arity) => {
                    let split = values
                        .len()
                        .checked_sub(arity)
                        .expect("candidate intersection has one domain per operand");
                    let mut operands = values.split_off(split).into_iter().flatten();
                    let Some(mut result) = operands.next() else {
                        values.push(None);
                        continue;
                    };
                    for operand in operands {
                        result = result.meet(&operand);
                    }
                    values.push(Some(result));
                }
            }
        }

        debug_assert_eq!(values.len(), 1, "candidate plan leaves one root domain");
        let mask = values
            .pop()
            .expect("candidate plan emits its root domain")?;
        let mut positions = mask
            .into_iter()
            .map(|(key, ())| {
                let bytes: [u8; 8] = key
                    .as_slice()
                    .try_into()
                    .expect("candidate position paths are fixed-width u64 keys");
                u64::from_be_bytes(bytes)
            })
            .collect::<Vec<_>>();
        positions.sort_unstable();
        Some(positions.into_boxed_slice())
    }

    /// Evaluate candidate union/intersection directly as PathMap lattice
    /// operations. Fixed-width big-endian group-id keys are prefix-free, so
    /// the generic trie lattice is exactly finite-set union/intersection here.
    /// `None` denotes the universal domain and remains an algebraic identity
    /// for intersection / annihilator for union. Duplicate occurrences remain
    /// compressed to one group endpoint throughout the plan and expand exactly
    /// once, in original target order, after the root mask is known.
    fn positions_matching_plan(&self, root: &ListCandidatePlan) -> Option<Box<[u64]>> {
        #[cfg(feature = "spatial-matcher-candidate-mask-differential")]
        if FORCE_OCCURRENCE_CANDIDATE_MASKS.with(Cell::get) {
            return self.positions_matching_plan_occurrence_control(root);
        }

        enum GroupMaskKind {
            All,
            Finite(PathMap<()>),
        }

        struct GroupMask {
            kind: GroupMaskKind,
            // Observational only.  Cache the count once when a lattice result
            // is born instead of repeatedly walking the trie in test builds.
            #[cfg(test)]
            endpoints: usize,
        }

        impl GroupMask {
            fn all() -> Self {
                Self {
                    kind: GroupMaskKind::All,
                    #[cfg(test)]
                    endpoints: 0,
                }
            }

            fn finite(mask: PathMap<()>) -> Self {
                #[cfg(test)]
                let endpoints = mask.val_count();
                Self {
                    kind: GroupMaskKind::Finite(mask),
                    #[cfg(test)]
                    endpoints,
                }
            }

            #[cfg(test)]
            fn is_finite(&self) -> bool { matches!(&self.kind, GroupMaskKind::Finite(_)) }

            fn is_absorbing(&self, kind: CandidateNary) -> bool {
                match (kind, &self.kind) {
                    (CandidateNary::Union, GroupMaskKind::All) => true,
                    (CandidateNary::Intersection, GroupMaskKind::Finite(mask)) => mask.is_empty(),
                    _ => false,
                }
            }
        }

        #[derive(Default)]
        struct Retention {
            #[cfg(test)]
            live_masks: usize,
            #[cfg(test)]
            live_endpoints: usize,
        }

        impl Retention {
            fn retain(&mut self, mask: &GroupMask) {
                #[cfg(not(test))]
                let _ = mask;
                #[cfg(test)]
                {
                    if !mask.is_finite() {
                        return;
                    }
                    self.live_masks += 1;
                    self.live_endpoints += mask.endpoints;
                    #[cfg(test)]
                    update_list_relation_stats(|stats| {
                        stats.candidate_peak_live_masks =
                            stats.candidate_peak_live_masks.max(self.live_masks);
                        stats.candidate_peak_live_mask_endpoints = stats
                            .candidate_peak_live_mask_endpoints
                            .max(self.live_endpoints);
                    });
                }
            }

            fn release(&mut self, mask: &GroupMask) {
                #[cfg(not(test))]
                let _ = mask;
                #[cfg(test)]
                {
                    if !mask.is_finite() {
                        return;
                    }
                    self.live_masks -= 1;
                    self.live_endpoints -= mask.endpoints;
                }
            }

            #[cfg(test)]
            fn release_counts(&mut self, finite: bool, endpoints: usize) {
                if finite {
                    self.live_masks -= 1;
                    self.live_endpoints -= endpoints;
                }
            }

            fn combine(
                &mut self,
                kind: CandidateNary,
                left: GroupMask,
                right: GroupMask,
            ) -> GroupMask {
                #[cfg(test)]
                let left_finite = left.is_finite();
                #[cfg(test)]
                let right_finite = right.is_finite();
                #[cfg(test)]
                let left_endpoints = left.endpoints;
                #[cfg(test)]
                let right_endpoints = right.endpoints;
                #[cfg(test)]
                let moved_identity =
                    matches!(kind, CandidateNary::Intersection) && (left_finite ^ right_finite);

                let result = match (left.kind, right.kind, kind) {
                    (GroupMaskKind::All, _, CandidateNary::Union)
                    | (_, GroupMaskKind::All, CandidateNary::Union) => GroupMask::all(),
                    (GroupMaskKind::All, right, CandidateNary::Intersection) => GroupMask {
                        kind: right,
                        #[cfg(test)]
                        endpoints: right_endpoints,
                    },
                    (left, GroupMaskKind::All, CandidateNary::Intersection) => GroupMask {
                        kind: left,
                        #[cfg(test)]
                        endpoints: left_endpoints,
                    },
                    (
                        GroupMaskKind::Finite(left),
                        GroupMaskKind::Finite(right),
                        CandidateNary::Union,
                    ) => GroupMask::finite(left.join(&right)),
                    (
                        GroupMaskKind::Finite(left),
                        GroupMaskKind::Finite(right),
                        CandidateNary::Intersection,
                    ) => GroupMask::finite(left.meet(&right)),
                };

                // Moving one finite identity through `All ∩ X` retains the
                // same logical mask. Every other arm creates a new logical
                // result (or drops all finite inputs), so update the test-only
                // retention instrument around the actual ownership change.
                #[cfg(test)]
                if !moved_identity {
                    self.retain(&result);
                    self.release_counts(left_finite, left_endpoints);
                    self.release_counts(right_finite, right_endpoints);
                }
                result
            }
        }

        enum Work {
            Enter(CandidatePlanNodeId),
            // First-child descents carry no PathMap-sized optional payload.
            ResumeFirst {
                node: CandidatePlanNodeId,
                kind: CandidateNary,
                next_operand: usize,
            },
            ResumeRest {
                node: CandidatePlanNodeId,
                kind: CandidateNary,
                next_operand: usize,
            },
        }
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.candidate_plan_work_frame_bytes = stats
                .candidate_plan_work_frame_bytes
                .max(std::mem::size_of::<Work>());
        });

        let mut work = vec![Work::Enter(root.root)];
        // Keep PathMap-bearing values out of `Work`: a Rust enum is as large
        // as its largest variant, so an inline accumulator would make every
        // first-descent frame PathMap-sized even when that variant is absent.
        let mut accumulators: Vec<GroupMask> = Vec::new();
        let mut returned: Option<GroupMask> = None;
        let mut retention = Retention::default();
        while let Some(next) = work.pop() {
            match next {
                Work::Enter(id) => match &root.node(id).kind {
                    CandidatePlanNodeKind::All => returned = Some(GroupMask::all()),
                    CandidatePlanNodeKind::Empty => {
                        let mask = GroupMask::finite(PathMap::new());
                        retention.retain(&mask);
                        returned = Some(mask);
                    }
                    CandidatePlanNodeKind::Query(query) => {
                        let mask = self
                            .group_mask_matching(query)
                            .map_or_else(GroupMask::all, GroupMask::finite);
                        retention.retain(&mask);
                        returned = Some(mask);
                    }
                    CandidatePlanNodeKind::Union(operands)
                    | CandidatePlanNodeKind::Intersection(operands) => {
                        let kind = match &root.node(id).kind {
                            CandidatePlanNodeKind::Union(_) => CandidateNary::Union,
                            CandidatePlanNodeKind::Intersection(_) => CandidateNary::Intersection,
                            _ => unreachable!(),
                        };
                        debug_assert!(operands.len() >= 2);
                        work.push(Work::ResumeFirst {
                            node: id,
                            kind,
                            next_operand: 1,
                        });
                        work.push(Work::Enter(operands[0]));
                    }
                },
                Work::ResumeFirst {
                    node,
                    kind,
                    next_operand,
                } => {
                    let accumulator = returned
                        .take()
                        .expect("candidate child returns one group mask");
                    if accumulator.is_absorbing(kind) {
                        returned = Some(accumulator);
                        continue;
                    }
                    let operands = match &root.node(node).kind {
                        CandidatePlanNodeKind::Union(operands)
                        | CandidatePlanNodeKind::Intersection(operands) => operands,
                        _ => unreachable!("candidate resume belongs to an n-ary node"),
                    };
                    if next_operand == operands.len() {
                        returned = Some(accumulator);
                    } else {
                        accumulators.push(accumulator);
                        #[cfg(test)]
                        update_list_relation_stats(|stats| {
                            stats.candidate_peak_accumulator_stack = stats
                                .candidate_peak_accumulator_stack
                                .max(accumulators.len());
                        });
                        work.push(Work::ResumeRest {
                            node,
                            kind,
                            next_operand: next_operand + 1,
                        });
                        work.push(Work::Enter(operands[next_operand]));
                    }
                }
                Work::ResumeRest {
                    node,
                    kind,
                    next_operand,
                } => {
                    let child = returned
                        .take()
                        .expect("candidate child returns one group mask");
                    let accumulator = accumulators
                        .pop()
                        .expect("candidate continuation retains one group mask");
                    let accumulator = retention.combine(kind, accumulator, child);
                    if accumulator.is_absorbing(kind) {
                        returned = Some(accumulator);
                        continue;
                    }
                    let operands = match &root.node(node).kind {
                        CandidatePlanNodeKind::Union(operands)
                        | CandidatePlanNodeKind::Intersection(operands) => operands,
                        _ => unreachable!("candidate resume belongs to an n-ary node"),
                    };
                    if next_operand == operands.len() {
                        returned = Some(accumulator);
                    } else {
                        accumulators.push(accumulator);
                        #[cfg(test)]
                        update_list_relation_stats(|stats| {
                            stats.candidate_peak_accumulator_stack = stats
                                .candidate_peak_accumulator_stack
                                .max(accumulators.len());
                        });
                        work.push(Work::ResumeRest {
                            node,
                            kind,
                            next_operand: next_operand + 1,
                        });
                        work.push(Work::Enter(operands[next_operand]));
                    }
                }
            }
        }
        debug_assert!(accumulators.is_empty());

        let root_mask = returned.expect("candidate plan emits its root domain");
        let GroupMaskKind::Finite(mask) = &root_mask.kind else {
            return None;
        };

        // Decode each fixed-width group id once.  This avoids the former
        // O(target occurrences) series of duplicate trie lookups and gives the
        // extraction policy the exact sparse/dense cardinalities.
        use pathmap::zipper::{ZipperIteration, ZipperMoving};
        let mut matched_groups = Vec::new();
        let mut matched_occurrences = 0usize;
        let mut groups_already_ordered = true;
        let mut previous_group_last = None;
        let mut zipper = mask.read_zipper();
        while zipper.to_next_val() {
            let bytes: [u8; 8] = zipper
                .path()
                .try_into()
                .expect("candidate group-mask paths are fixed-width u64 identifiers");
            let raw = u64::from_be_bytes(bytes);
            let id = ListTargetGroupId(
                NonZeroU64::new(raw).expect("candidate group identifiers are nonzero"),
            );
            let positions = &self.groups[id.index()].positions;
            if let (Some(previous), Some(first)) = (previous_group_last, positions.first().copied())
            {
                groups_already_ordered &= previous < first;
            }
            previous_group_last = positions.last().copied().or(previous_group_last);
            matched_occurrences = matched_occurrences
                .checked_add(positions.len())
                .expect("matched occurrences cannot exceed target cardinality");
            matched_groups.push(id);
        }
        debug_assert!(matched_occurrences <= self.position_groups.len());

        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.candidate_root_group_endpoints = stats
                .candidate_root_group_endpoints
                .max(matched_groups.len());
        });

        let total_occurrences = self.position_groups.len();
        let positions = if matched_occurrences == 0 {
            #[cfg(test)]
            update_list_relation_stats(|stats| stats.candidate_root_empty_expansions += 1);
            Vec::new()
        } else if matched_occurrences == total_occurrences {
            #[cfg(test)]
            update_list_relation_stats(|stats| stats.candidate_root_all_expansions += 1);
            (0..total_occurrences)
                .map(|position| {
                    u64::try_from(position).expect("a target position fits the Bag cardinality")
                })
                .collect()
        } else if matched_groups.len() == 1 {
            #[cfg(test)]
            update_list_relation_stats(|stats| stats.candidate_root_single_group_expansions += 1);
            self.groups[matched_groups[0].index()].positions.clone()
        } else {
            // Output copying is common to both lanes.  Compare the remaining
            // portable work units: sparse sorting K log K versus one dense T
            // scan plus a bit-packed G membership witness and M marks.  The
            // coefficient-free crossover is benchmarked independently and is
            // semantic-neutral: both lanes emit identical ordered positions.
            let sort_levels = usize::BITS as usize
                - matched_occurrences.saturating_sub(1).leading_zeros() as usize;
            let sparse_units = matched_occurrences.saturating_mul(sort_levels.max(1));
            let dense_units = total_occurrences
                .saturating_add(self.groups.len().div_ceil(usize::BITS as usize))
                .saturating_add(matched_groups.len());
            let use_dense = {
                #[cfg(test)]
                {
                    match CANDIDATE_EXPANSION_MODE.with(Cell::get) {
                        CandidateExpansionMode::Adaptive => {
                            !groups_already_ordered && dense_units <= sparse_units
                        }
                        CandidateExpansionMode::Sparse => false,
                        CandidateExpansionMode::Dense => true,
                    }
                }
                #[cfg(not(test))]
                {
                    !groups_already_ordered && dense_units <= sparse_units
                }
            };
            if use_dense {
                #[cfg(test)]
                update_list_relation_stats(|stats| stats.candidate_root_dense_expansions += 1);
                let mut selected = vec![false; self.groups.len()];
                for id in matched_groups {
                    selected[id.index()] = true;
                }
                self.position_groups
                    .iter()
                    .enumerate()
                    .filter(|(_, id)| selected[id.index()])
                    .map(|(position, _)| {
                        u64::try_from(position).expect("a target position fits the Bag cardinality")
                    })
                    .collect()
            } else {
                #[cfg(test)]
                update_list_relation_stats(|stats| stats.candidate_root_sparse_expansions += 1);
                let mut positions = Vec::with_capacity(matched_occurrences);
                for id in matched_groups {
                    let group_positions = &self.groups[id.index()].positions;
                    positions.extend_from_slice(group_positions);
                }
                if !groups_already_ordered {
                    positions.sort_unstable();
                }
                positions
            }
        };
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.candidate_final_expanded_positions = stats
                .candidate_final_expanded_positions
                .max(positions.len());
        });
        retention.release(&root_mask);
        #[cfg(test)]
        {
            debug_assert_eq!(retention.live_masks, 0);
            debug_assert_eq!(retention.live_endpoints, 0);
        }
        Some(positions.into_boxed_slice())
    }
}

enum ListAssignment {
    Term {
        pattern_index: usize,
        free_map: EdgeResult<BindingDelta>,
    },
    Remainder,
}

#[derive(Clone, Copy)]
enum ListSearchPattern {
    Term(usize),
    Remainder,
}

enum ListSearchCursor {
    Term {
        pattern_index: usize,
        relation_cursor: RelationCursor,
    },
    Remainder {
        next_target: usize,
    },
}

struct ListSearchFrame {
    cursor: ListSearchCursor,
    pending: Option<(usize, ListAssignment)>,
}

impl ListSearchFrame {
    fn pattern(&self) -> ListSearchPattern {
        match self.cursor {
            ListSearchCursor::Term { pattern_index, .. } => ListSearchPattern::Term(pattern_index),
            ListSearchCursor::Remainder { .. } => ListSearchPattern::Remainder,
        }
    }
}

/// Per-augmenting-search visitation state.
///
/// The target trie Bag is authoritative in both lanes. When the target really
/// is a multiset (at least one multiplicity exceeds one), visiting the next
/// byte-identical target increments that identity's Bag multiplicity. A
/// set-shaped target has no multiplicity to compress, so the one-bit position
/// lane is strictly smaller and avoids a trie lookup on every rejected edge.
/// The group's original position vector is only the deterministic ordering
/// witness needed to reproduce the predecessor Kuhn traversal exactly.
enum ListSeenTargets {
    Multiplicities(Bag),
    Positions(Vec<bool>),
}

impl ListSeenTargets {
    fn new(target_index: Option<&ListTargetIndex>, target_count: usize) -> Self {
        match target_index {
            Some(index) if index.bag.distinct_len() < target_count => {
                Self::Multiplicities(Bag::new())
            }
            _ => Self::Positions(vec![false; target_count]),
        }
    }

    fn reset(&mut self) {
        match self {
            Self::Multiplicities(seen) => *seen = Bag::new(),
            Self::Positions(seen) => seen.fill(false),
        }
    }

    fn contains(&self, target_index: Option<&ListTargetIndex>, target_position: usize) -> bool {
        match self {
            Self::Multiplicities(seen) => {
                let target_index = target_index
                    .expect("multiplicity visitation requires the authoritative target Bag");
                let group = target_index.group_at_position(target_position);
                let position = u64::try_from(target_position)
                    .expect("an indexed target position was already represented as u64");
                let ordinal = group
                    .positions
                    .binary_search(&position)
                    .expect("a target group's ordering witness contains every member");
                let ordinal = u64::try_from(ordinal)
                    .expect("a target-group ordinal fits the fixed-width multiplicity domain");
                ordinal < seen.count(&group.key)
            }
            Self::Positions(seen) => seen[target_position],
        }
    }

    fn insert(&mut self, target_index: Option<&ListTargetIndex>, target_position: usize) {
        match self {
            Self::Multiplicities(seen) => {
                let target_index = target_index
                    .expect("multiplicity visitation requires the authoritative target Bag");
                let group = target_index.group_at_position(target_position);
                let position = u64::try_from(target_position)
                    .expect("an indexed target position was already represented as u64");
                let ordinal = group
                    .positions
                    .binary_search(&position)
                    .expect("a target group's ordering witness contains every member");
                let ordinal = u64::try_from(ordinal)
                    .expect("a target-group ordinal fits the fixed-width multiplicity domain");
                let already_seen = seen.count(&group.key);
                assert_eq!(
                    ordinal, already_seen,
                    "candidate domains must visit byte-identical targets in predecessor order"
                );
                seen.insert(&group.key)
                    .expect("seen multiplicity cannot exceed target multiplicity");
                debug_assert!(seen.is_subbag_of(&target_index.bag));
            }
            Self::Positions(seen) => seen[target_position] = true,
        }
    }
}

struct ListMachine {
    targets: Arc<[MatchValue]>,
    target_index: Option<ListTargetIndex>,
    patterns: Arc<[MatchValue]>,
    candidate_domains: Box<[ListCandidateDomain]>,
    assignments: Vec<Option<ListAssignment>>,
    root_pattern: usize,
    seen_targets: ListSeenTargets,
    search: Vec<ListSearchFrame>,
    relation: LazyRelation<BindingDelta>,
    empty_delta: Rc<BindingDelta>,
    merger: Merger,
    remainder: Option<i32>,
    wildcard: bool,
}

fn init_list(
    context: &mut FreeMapTransaction<'_>,
    mut request: ListRequest,
    frames: &mut FrameStack,
) -> Job {
    let pattern_len = request.patterns.len();
    let target_len = request.targets.len();
    let exact = !request.wildcard && request.remainder.is_none();
    if (exact && pattern_len != target_len) || pattern_len > target_len {
        return Job::Return(false);
    }
    if exact && pattern_len == 1 {
        let target = request.targets.pop().expect("the singleton target exists");
        let pattern = request
            .patterns
            .pop()
            .expect("the singleton pattern exists");
        if !pattern.connective_used(context.context()) {
            return Job::Return(target.semantic_eq(&pattern));
        }
        frames.push(Frame::RestoreOnFailure {
            checkpoint: context.checkpoint(),
        });
        return target
            .into_job(pattern, frames)
            .unwrap_or(Job::Return(false));
    }
    if pattern_len == 0 && target_len == 0 && request.remainder.is_none() {
        return Job::Return(true);
    }
    if pattern_len == 0 {
        if let Some(level) = request.remainder {
            if !request
                .targets
                .iter()
                .all(|value| value.locally_free_is_empty(context.context()))
            {
                return Job::Return(false);
            }
            let prior = context
                .free_map()
                .get(&level)
                .cloned()
                .unwrap_or_else(|| vector_par(Vec::new(), false));
            context.write(level, request.merger.merge(prior, request.targets));
            return Job::Return(true);
        }
    }

    // Remainders are the Bag difference after the structural obligations have
    // matched. Representing each residual occurrence as a universal filler
    // vertex creates a dense artificial relation and Θ((T-P)·T) initial work.
    // Quotient those indistinguishable filler vertices into `Remainder`
    // occupancy: seed the same first locally-free positions that predecessor
    // Kuhn traversal selected, and use one generic heap-PDA search state when
    // a term displaces an occupant. `finish_list` then computes authoritative
    // residual multiplicities with Bag subtraction while original positions
    // remain only the deterministic ordering witness.
    let patterns = request.patterns;
    let mut captures = PathMap::new();
    let candidate_plans = patterns
        .iter()
        .map(|pattern| pattern.rho_path_candidate_plan(context.context(), &mut captures))
        .collect::<Box<[_]>>();
    #[cfg(feature = "matcher-decline-counters")]
    dynamic_root_decline_counters::count_rows(&patterns, &candidate_plans);
    let needs_target_index = request.remainder.is_some()
        || candidate_plans
            .iter()
            .any(ListCandidatePlan::needs_target_index);
    #[cfg(test)]
    if needs_target_index {
        update_list_relation_stats(|stats| stats.target_index_builds += 1);
    }
    let target_index = needs_target_index
        .then(|| ListTargetIndex::from_targets(&request.targets))
        .flatten();
    let candidate_domains = candidate_plans
        .iter()
        .map(|plan| {
            if plan.is_empty() {
                Some(Vec::new().into_boxed_slice())
            } else {
                target_index
                    .as_ref()
                    .and_then(|index| index.positions_matching_plan(plan))
            }
            .map_or(ListCandidateDomain::All, ListCandidateDomain::Indexed)
        })
        .collect::<Box<[_]>>();
    // Constraint exhaustion is order-independent. Refusing here avoids
    // evaluating earlier broad rows when a later byte-native domain is
    // already empty, while leaving every successful matching and binding
    // decision to the historical root order below.
    if candidate_domains
        .iter()
        .any(|domain| domain.len(target_len) == 0)
    {
        return Job::Return(false);
    }
    let relation = LazyRelation::new(std::iter::repeat_n(true, patterns.len()));

    let seen_targets = ListSeenTargets::new(target_index.as_ref(), target_len);
    let mut assignments = (0..target_len).map(|_| None).collect::<Vec<_>>();
    if request.remainder.is_some() {
        let mut remaining = target_len - pattern_len;
        for (target, assignment) in request.targets.iter().zip(assignments.iter_mut()) {
            if remaining == 0 {
                break;
            }
            if target.locally_free_is_empty(context.context()) {
                *assignment = Some(ListAssignment::Remainder);
                remaining -= 1;
            }
        }
        if remaining != 0 {
            return Job::Return(false);
        }
    }
    let mut machine = ListMachine {
        targets: request.targets.into(),
        target_index,
        assignments,
        patterns: patterns.into(),
        candidate_domains,
        root_pattern: 0,
        seen_targets,
        search: Vec::new(),
        relation,
        empty_delta: Rc::new(BindingDelta::empty()),
        merger: request.merger,
        remainder: request.remainder,
        wildcard: request.wildcard,
    };
    machine.start_next_root();
    Job::List(Box::new(machine))
}

impl ListMachine {
    fn push_search(&mut self, pattern: ListSearchPattern) {
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.augmenting_frames += 1;
            if let ListSearchPattern::Term(pattern_index) = pattern {
                if self.relation.has_scanned_targets(pattern_index) {
                    stats.relation_row_reuses += 1;
                }
            }
        });
        let cursor = match pattern {
            ListSearchPattern::Term(pattern_index) => ListSearchCursor::Term {
                pattern_index,
                relation_cursor: self.relation.cursor(pattern_index),
            },
            ListSearchPattern::Remainder => ListSearchCursor::Remainder { next_target: 0 },
        };
        self.search.push(ListSearchFrame {
            cursor,
            pending: None,
        });
    }

    fn start_next_root(&mut self) {
        if self.root_pattern < self.patterns.len() {
            self.seen_targets.reset();
            self.push_search(ListSearchPattern::Term(self.root_pattern));
        }
    }

    fn accept_term_edge(self, target_index: usize, free_map: EdgeResult<BindingDelta>) -> Job {
        let ListSearchPattern::Term(pattern_index) = self
            .search
            .last()
            .expect("an accepted term edge has a search frame")
            .pattern()
        else {
            unreachable!("a remainder frame cannot accept a term edge")
        };
        self.accept_edge(target_index, ListAssignment::Term {
            pattern_index,
            free_map,
        })
    }

    fn accept_remainder_edge(self, target_index: usize) -> Job {
        self.accept_edge(target_index, ListAssignment::Remainder)
    }

    fn accept_edge(mut self, target_index: usize, assignment: ListAssignment) -> Job {
        self.seen_targets
            .insert(self.target_index.as_ref(), target_index);
        let displaced = self.assignments[target_index]
            .as_ref()
            .map(|previous| match previous {
                ListAssignment::Term { pattern_index, .. } => {
                    ListSearchPattern::Term(*pattern_index)
                }
                ListAssignment::Remainder => ListSearchPattern::Remainder,
            });
        match displaced {
            None => {
                self.assignments[target_index] = Some(assignment);
                self.search.pop();
                while let Some(mut parent) = self.search.pop() {
                    let (parent_target, parent_assignment) = parent
                        .pending
                        .take()
                        .expect("an augmenting list parent has one displaced edge");
                    self.assignments[parent_target] = Some(parent_assignment);
                }
                self.root_pattern += 1;
                self.start_next_root();
                Job::List(Box::new(self))
            }
            Some(displaced) => {
                self.search
                    .last_mut()
                    .expect("an accepted list edge has a search frame")
                    .pending = Some((target_index, assignment));
                self.push_search(displaced);
                Job::List(Box::new(self))
            }
        }
    }
}

fn step_list(
    context: &mut FreeMapTransaction<'_>,
    mut machine: ListMachine,
    frames: &mut FrameStack,
) -> Job {
    if machine.root_pattern == machine.patterns.len() && machine.search.is_empty() {
        return finish_list(context, machine);
    }

    loop {
        let Some(search_pattern) = machine.search.last().map(ListSearchFrame::pattern) else {
            return Job::Return(false);
        };

        if matches!(search_pattern, ListSearchPattern::Remainder) {
            let target_index = loop {
                let next = {
                    let frame = machine
                        .search
                        .last_mut()
                        .expect("the remainder search has a frame");
                    let ListSearchCursor::Remainder { next_target } = &mut frame.cursor else {
                        unreachable!("a remainder search has a remainder cursor")
                    };
                    if *next_target == machine.targets.len() {
                        None
                    } else {
                        let current = *next_target;
                        *next_target += 1;
                        Some(current)
                    }
                };
                let Some(target_index) = next else {
                    break None;
                };
                if machine
                    .seen_targets
                    .contains(machine.target_index.as_ref(), target_index)
                {
                    continue;
                }
                #[cfg(test)]
                update_list_relation_stats(|stats| stats.edge_evaluations += 1);
                if machine.targets[target_index].locally_free_is_empty(context.context()) {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| {
                        stats.successful_edge_evaluations += 1;
                    });
                    break Some(target_index);
                }
            };
            if let Some(target_index) = target_index {
                return machine.accept_remainder_edge(target_index);
            }
            machine.search.pop();
            if let Some(parent) = machine.search.last_mut() {
                parent.pending = None;
                continue;
            }
            return Job::Return(false);
        }

        let ListSearchPattern::Term(pattern_index) = search_pattern else {
            unreachable!("the remainder search returned above")
        };

        loop {
            let cached = {
                let frame = machine
                    .search
                    .last_mut()
                    .expect("the relational list row has a search frame");
                let ListSearchCursor::Term {
                    relation_cursor, ..
                } = &mut frame.cursor
                else {
                    unreachable!("a term search has a relational cursor")
                };
                machine.relation.next_cached(pattern_index, relation_cursor)
            };
            match cached {
                Some((target_index, produced))
                    if !machine
                        .seen_targets
                        .contains(machine.target_index.as_ref(), target_index) =>
                {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| stats.cached_edge_visits += 1);
                    return machine.accept_term_edge(target_index, EdgeResult::Shared(produced));
                }
                Some(_) => {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| stats.cached_edge_visits += 1);
                    continue;
                }
                None => break,
            }
        }

        let candidate_domain = &machine.candidate_domains[pattern_index];
        let Some(candidate_index) = machine
            .relation
            .next_unscanned(pattern_index, candidate_domain.len(machine.targets.len()))
        else {
            machine.search.pop();
            if let Some(parent) = machine.search.last_mut() {
                parent.pending = None;
                continue;
            }
            return Job::Return(false);
        };
        let target_index = candidate_domain.target_at(candidate_index);

        let candidate_was_seen = machine
            .seen_targets
            .contains(machine.target_index.as_ref(), target_index);
        #[cfg(test)]
        update_list_relation_stats(|stats| stats.edge_evaluations += 1);
        let pattern = &machine.patterns[pattern_index];
        if !pattern.connective_used(context.context()) {
            if machine.targets[target_index].semantic_eq(pattern) {
                #[cfg(test)]
                update_list_relation_stats(|stats| {
                    stats.successful_edge_evaluations += 1;
                });
                let produced = machine.relation.record_shared_success(
                    pattern_index,
                    target_index,
                    Rc::clone(&machine.empty_delta),
                );
                if !candidate_was_seen {
                    return machine.accept_term_edge(target_index, EdgeResult::Shared(produced));
                }
            }
        } else {
            let target = machine.targets[target_index].clone();
            let pattern = pattern.clone();
            let checkpoint = context.checkpoint();
            frames.push(Frame::ListEdge {
                machine: Box::new(machine),
                target_index,
                candidate_was_seen,
                checkpoint,
            });
            return target
                .into_job(pattern, frames)
                .unwrap_or(Job::Return(false));
        }
    }
}

pub(super) fn apply_update_deltas(
    context: &mut FreeMapTransaction<'_>,
    maps: Vec<BindingDelta>,
) -> bool {
    let maps = maps
        .into_iter()
        .map(BindingDelta::into_map)
        .collect::<Vec<_>>();
    if validate_update_deltas(context.free_map(), &maps).is_none() {
        return false;
    }
    // Preserve the predecessor's ordered `BTreeMap::extend` fold exactly, but
    // route every write through the active undo journal so an enclosing AND,
    // OR, NOT, list edge, or PathMap edge can still restore its checkpoint.
    for map in maps {
        context.extend(map);
    }
    true
}

fn finish_list(context: &mut FreeMapTransaction<'_>, machine: ListMachine) -> Job {
    let ListMachine {
        targets,
        target_index,
        assignments,
        mut relation,
        merger,
        remainder,
        wildcard,
        ..
    } = machine;
    relation.clear();

    let bag_remainder = remainder.and_then(|_| {
        target_index.as_ref().map(|target_index| {
            let mut consumed = Bag::new();
            for (target_position, assignment) in assignments.iter().enumerate() {
                if !matches!(assignment, Some(ListAssignment::Term { .. })) {
                    continue;
                }
                consumed
                    .insert(&target_index.group_at_position(target_position).key)
                    .expect("a successful assignment cannot exceed target multiplicity");
            }
            debug_assert!(consumed.is_subbag_of(&target_index.bag));
            target_index.bag.subtract(&consumed)
        })
    });

    let mut remainder_target_indices = vec![false; targets.len()];
    if let (Some(mut residual), Some(target_index)) = (bag_remainder, target_index.as_ref()) {
        for (position, (assignment, is_remainder)) in assignments
            .iter()
            .zip(remainder_target_indices.iter_mut())
            .enumerate()
        {
            if matches!(assignment, Some(ListAssignment::Remainder)) {
                let key = &target_index.group_at_position(position).key;
                assert!(
                    residual.contains(key),
                    "an unassigned position must remain in the authoritative Bag residual"
                );
                let _ = residual.remove(key);
                *is_remainder = true;
            }
        }
        debug_assert!(residual.is_empty());
    } else {
        for (target_index, assignment) in assignments.iter().enumerate() {
            if matches!(assignment, Some(ListAssignment::Remainder)) {
                remainder_target_indices[target_index] = true;
            }
        }
    }
    let remainder_sorted = targets
        .iter()
        .enumerate()
        .filter(|(target_index, _)| remainder_target_indices[*target_index])
        .map(|(_, target)| target)
        .cloned()
        .collect::<Vec<_>>();
    if remainder.is_some()
        && !remainder_sorted
            .iter()
            .all(|target| target.locally_free_is_empty(context.context()))
    {
        return Job::Return(false);
    }
    let mut ordered: Vec<_> = assignments
        .into_iter()
        .enumerate()
        .filter_map(|(target_index, assignment)| match assignment {
            Some(ListAssignment::Term { free_map, .. }) => Some((target_index, free_map)),
            Some(ListAssignment::Remainder) | None => None,
        })
        .collect();
    let encoded_collection = targets.iter().all(MatchValue::is_encoded_collection_value);
    ordered.sort_by(|(left_index, _), (right_index, _)| {
        if encoded_collection {
            // Canonical ESet/EMap protobuf children are already in the
            // predecessor's semantic order. This index is only a deterministic
            // binding-fold witness; it does not order Rholang execution.
            left_index.cmp(right_index)
        } else {
            match targets[*left_index].cmp(&targets[*right_index]) {
                Ordering::Equal => left_index.cmp(right_index),
                order => order,
            }
        }
    });
    let maps = ordered
        .into_iter()
        .map(|(_, free_map)| free_map.into_owned())
        .collect::<Vec<_>>();
    if !apply_update_deltas(context, maps) {
        return Job::Return(false);
    }

    match remainder {
        None if wildcard || remainder_sorted.is_empty() => Job::Return(true),
        None => Job::Return(false),
        Some(level) => {
            let prior = context
                .free_map()
                .get(&level)
                .cloned()
                .unwrap_or_else(|| vector_par(Vec::new(), false));
            context.write(level, merger.merge(prior, remainder_sorted));
            Job::Return(true)
        }
    }
}

#[derive(Clone)]
enum PathPattern {
    Remainder,
    Set(Arc<Par>),
    /// A dynamic map value under a concrete canonical key. Keeping the key in
    /// its encoded form makes candidate rejection a direct PathMap-key byte
    /// comparison and avoids decoding either side's key.
    MapExactKey {
        encoded_key: Arc<[u8]>,
        value: Arc<Par>,
    },
    /// A dynamic key requires the full semantic pair matcher.
    MapDynamicKey(Arc<(Par, Par)>),
}

#[derive(Clone)]
struct PathAssignment {
    pattern: PathPattern,
    free_map: BindingDelta,
}

struct PathSearchFrame {
    pattern: PathPattern,
    next_target: Vec<u8>,
    pending: Option<(Vec<u8>, BindingDelta)>,
}

#[derive(Clone, Copy)]
enum PathRemainder {
    None,
    Wildcard,
    Free(i32),
}

struct PathMachine {
    target: EPathMap,
    dynamic_patterns: std::vec::IntoIter<PathPattern>,
    assignments: pathmap::PathMap<PathAssignment>,
    remainder: PathRemainder,
    remainder_roots: usize,
    roots_finished: bool,
    seen: pathmap::PathMap<()>,
    search: Vec<PathSearchFrame>,
}

fn init_path(
    context: &mut FreeMapTransaction<'_>,
    target: EPathMap,
    pattern: EPathMap,
    frames: &mut FrameStack,
) -> Job {
    let target_mode = target.mode();
    let pattern_mode = pattern.mode();
    if pattern_mode != EPathMapMode::Empty && target_mode != pattern_mode {
        return Job::Return(false);
    }
    let remainder = match pattern
        .remainder
        .as_ref()
        .and_then(|var| var.var_instance.as_ref())
    {
        Some(Wildcard(_)) => PathRemainder::Wildcard,
        Some(FreeVar(level)) => PathRemainder::Free(*level),
        _ => PathRemainder::None,
    };
    // The overwhelmingly common nested-map shape has one binding on each
    // level. Moving the two owned PathMap entries directly into the PDA avoids
    // cloning the complete remaining Par suffix once per level (Θ(d²) bytes
    // and time for a depth-d value chain). Concrete singleton entries compare
    // in their canonical representation; dynamic entries descend directly
    // into the matcher. General unordered matching keeps the retry-capable
    // augmenting machine below.
    if matches!(remainder, PathRemainder::None) && target.len() == 1 && pattern.len() == 1 {
        return init_single_path(context, target, pattern, frames);
    }
    let Some((target, dynamic_patterns)) = prepare_path_entries(&target, &pattern) else {
        return Job::Return(false);
    };
    let dynamic_pattern_count = dynamic_patterns.len();
    let target_count = target.len();
    match remainder {
        PathRemainder::None if dynamic_pattern_count != target_count => return Job::Return(false),
        PathRemainder::Wildcard | PathRemainder::Free(_)
            if dynamic_pattern_count > target_count =>
        {
            return Job::Return(false);
        }
        _ => {}
    }

    let mut machine = PathMachine {
        target,
        dynamic_patterns: dynamic_patterns.into_iter(),
        assignments: pathmap::PathMap::new(),
        remainder,
        remainder_roots: if matches!(remainder, PathRemainder::Free(_)) {
            target_count - dynamic_pattern_count
        } else {
            0
        },
        roots_finished: false,
        seen: pathmap::PathMap::new(),
        search: Vec::new(),
    };
    machine.start_next_root();
    Job::Path(Box::new(machine))
}

fn take_single_path_entry(map: EPathMap) -> Option<OwnedEPathMapEntry> {
    let mut entries = map.into_owned_parts().entries;
    let first = entries.next()?;
    #[cfg(debug_assertions)]
    let exactly_one_entry = entries.next().is_none();
    #[cfg(debug_assertions)]
    debug_assert!(
        exactly_one_entry,
        "singleton fast path received multiple entries"
    );
    Some(first)
}

fn init_single_path(
    context: &mut FreeMapTransaction<'_>,
    target: EPathMap,
    pattern: EPathMap,
    frames: &mut FrameStack,
) -> Job {
    let Some(target) = take_single_path_entry(target) else {
        return Job::Return(false);
    };
    let Some(pattern) = take_single_path_entry(pattern) else {
        return Job::Return(false);
    };
    let pair = match (target, pattern) {
        (OwnedEPathMapEntry::Set(target), OwnedEPathMapEntry::Set(pattern)) => {
            let metadata = EntryTrie::encoded_key_metadata(&pattern)
                .expect("set pattern key is a canonical Par path");
            if !metadata.connective_used {
                // A concrete set member is its canonical PathMap key. The
                // compressed bytes are therefore the complete equality test.
                return Job::Return(target == pattern);
            }
            let decoded_pattern =
                decode_trie_path(&pattern).expect("dynamic set pattern key must decode once");
            MatchPair::Par(
                decode_trie_path(&target).expect("set target key is a canonical Par path"),
                decoded_pattern,
            )
        }
        (
            OwnedEPathMapEntry::Map {
                key: target_key,
                value: target_value,
            },
            OwnedEPathMapEntry::Map {
                key: pattern_key,
                value: pattern_value,
            },
        ) => {
            let metadata = EntryTrie::encoded_key_metadata(&pattern_key)
                .expect("map pattern key is a canonical Par path");
            if !metadata.connective_used {
                // Canonical PathMap key bytes are the equality representation.
                // A concrete singleton key therefore needs neither Par decoding
                // nor a second PDA continuation: compare the compressed bytes
                // and descend directly into the dynamic map value.
                if target_key != pattern_key {
                    return Job::Return(false);
                }
                if !pattern_value.connective_used() {
                    return Job::Return(target_value == pattern_value);
                }
                MatchPair::Par(target_value, pattern_value)
            } else {
                let decoded_pattern_key = decode_trie_path(&pattern_key)
                    .expect("dynamic map pattern key must decode once");
                MatchPair::ParPair(
                    (
                        decode_trie_path(&target_key)
                            .expect("map target key is a canonical Par path"),
                        target_value,
                    ),
                    (decoded_pattern_key, pattern_value),
                )
            }
        }
        _ => return Job::Return(false),
    };
    frames.push(Frame::RestoreOnFailure {
        checkpoint: context.checkpoint(),
    });
    match_job(pair)
}

impl PathMachine {
    fn start_next_root(&mut self) {
        let root = if self.remainder_roots != 0 {
            self.remainder_roots -= 1;
            Some(PathPattern::Remainder)
        } else {
            self.dynamic_patterns.next()
        };

        if root.is_none() {
            self.roots_finished = true;
        }

        if let Some(pattern) = root {
            self.seen = pathmap::PathMap::new();
            self.search.push(PathSearchFrame {
                pattern,
                next_target: Vec::new(),
                pending: None,
            });
        }
    }

    fn accept_edge(mut self, target_key: Vec<u8>, free_map: BindingDelta) -> Job {
        self.seen.insert(&target_key, ());
        let frame = self
            .search
            .last_mut()
            .expect("an accepted PathMap edge has a search frame");
        match self
            .assignments
            .get(&target_key)
            .map(|assignment| assignment.pattern.clone())
        {
            None => {
                self.assignments.insert(&target_key, PathAssignment {
                    pattern: frame.pattern.clone(),
                    free_map,
                });
                self.search.pop();
                while let Some(mut parent) = self.search.pop() {
                    let (parent_target, parent_map) = parent
                        .pending
                        .take()
                        .expect("an augmenting PathMap parent has one displaced edge");
                    self.assignments.insert(&parent_target, PathAssignment {
                        pattern: parent.pattern,
                        free_map: parent_map,
                    });
                }
                self.start_next_root();
                Job::Path(Box::new(self))
            }
            Some(displaced) => {
                frame.pending = Some((target_key, free_map));
                self.search.push(PathSearchFrame {
                    pattern: displaced,
                    next_target: Vec::new(),
                    pending: None,
                });
                Job::Path(Box::new(self))
            }
        }
    }
}

fn step_path(
    context: &mut FreeMapTransaction<'_>,
    mut machine: PathMachine,
    frames: &mut FrameStack,
) -> Job {
    if machine.roots_finished && machine.search.is_empty() {
        return finish_path(context, machine);
    }

    loop {
        let Some(frame) = machine.search.last_mut() else {
            return Job::Return(false);
        };
        let Some(target_key) = machine.target.next_value_key(&frame.next_target) else {
            machine.search.pop();
            if let Some(parent) = machine.search.last_mut() {
                parent.pending = None;
                continue;
            }
            return Job::Return(false);
        };
        frame.next_target = target_key.clone();
        if machine.seen.contains(&target_key) {
            continue;
        }

        match frame.pattern.clone() {
            PathPattern::Remainder => {
                if path_target_is_concrete(&machine.target, &target_key) {
                    return machine.accept_edge(target_key, BindingDelta::empty());
                }
            }
            PathPattern::Set(pattern) => {
                let target = decode_trie_path(&target_key)
                    .expect("set-mode EPathMap keys are canonical Par paths");
                let checkpoint = context.checkpoint();
                frames.push(Frame::PathEdge {
                    machine: Box::new(machine),
                    target_key,
                    checkpoint,
                });
                return match_job(MatchPair::Par(target, pattern.as_ref().clone()));
            }
            PathPattern::MapExactKey {
                encoded_key,
                value: pattern_value,
            } => {
                if target_key.as_slice() != encoded_key.as_ref() {
                    continue;
                }
                let target_value = machine
                    .target
                    .entry_trie()
                    .get_map_value_by_encoded_key(&target_key)
                    .expect("PathMap target mode was checked")
                    .expect("a PathMap candidate key names a value")
                    .clone();
                let checkpoint = context.checkpoint();
                frames.push(Frame::PathEdge {
                    machine: Box::new(machine),
                    target_key,
                    checkpoint,
                });
                return match_job(MatchPair::Par(target_value, pattern_value.as_ref().clone()));
            }
            PathPattern::MapDynamicKey(pattern) => {
                let key = decode_trie_path(&target_key)
                    .expect("map-mode EPathMap keys are canonical Par paths");
                let value = machine
                    .target
                    .entry_trie()
                    .get_map_value_by_encoded_key(&target_key)
                    .expect("PathMap target mode was checked")
                    .expect("a PathMap candidate key names a value")
                    .clone();
                let checkpoint = context.checkpoint();
                frames.push(Frame::PathEdge {
                    machine: Box::new(machine),
                    target_key,
                    checkpoint,
                });
                return match_job(MatchPair::ParPair((key, value), pattern.as_ref().clone()));
            }
        }
    }
}

fn path_target_is_concrete(target: &EPathMap, key: &[u8]) -> bool {
    let key_metadata =
        EntryTrie::encoded_key_metadata(key).expect("EPathMap target keys are canonical Par paths");
    match target.mode() {
        EPathMapMode::Empty => false,
        EPathMapMode::Set => key_metadata.locally_free_is_empty,
        EPathMapMode::Map => {
            let value = target
                .entry_trie()
                .get_map_value_by_encoded_key(key)
                .expect("PathMap target mode was checked")
                .expect("a PathMap candidate key names a value");
            key_metadata.locally_free_is_empty && value.locally_free().is_empty()
        }
    }
}

fn finish_path(context: &mut FreeMapTransaction<'_>, machine: PathMachine) -> Job {
    let mut remainder_keys =
        matches!(machine.remainder, PathRemainder::Free(_)).then(pathmap::PathMap::new);
    let mut maps = Vec::with_capacity(machine.assignments.val_count());
    for (target_key, assignment) in machine.assignments {
        if matches!(assignment.pattern, PathPattern::Remainder) {
            if let Some(keys) = &mut remainder_keys {
                keys.insert(&target_key, ());
            }
        }
        maps.push(assignment.free_map);
    }

    if !apply_update_deltas(context, maps) {
        return Job::Return(false);
    }
    if let PathRemainder::Free(level) = machine.remainder {
        let entries = machine
            .target
            .entry_trie()
            .select_encoded_keys(&remainder_keys.expect("free remainder collected a key mask"));
        let remainder_map = EPathMap::new(entries, Vec::new(), false, None);
        let bound = context
            .free_map()
            .get(&level)
            .cloned()
            .unwrap_or_default()
            .with_exprs(vec![Expr {
                expr_instance: Some(EPathmapBody(remainder_map)),
            }]);
        context.write(level, bound);
    }
    Job::Return(true)
}

fn prepare_path_entries(
    target: &EPathMap,
    pattern: &EPathMap,
) -> Option<(EPathMap, Vec<PathPattern>)> {
    // Canonical key bytes are the authoritative trie representation. Exact
    // entries flow into one mode-independent PathMap<()> mask without a
    // decode/re-encode round trip. Dynamic patterns cross the semantic boundary
    // exactly once here and are retained by the PDA for every retry.
    let mut exact_mask = pathmap::PathMap::new();
    let mut dynamic_patterns = Vec::new();
    let mut valid = true;

    match pattern.mode() {
        EPathMapMode::Empty => {}
        EPathMapMode::Set => {
            pattern
                .entry_trie()
                .for_each_raw_set_entry(|encoded| {
                    let metadata = EntryTrie::encoded_key_metadata(encoded)
                        .expect("set-mode EPathMap keys are canonical Par paths");
                    if metadata.connective_used {
                        let entry = decode_trie_path(encoded)
                            .expect("dynamic set pattern key must decode once");
                        dynamic_patterns.push(PathPattern::Set(Arc::new(entry)));
                    } else {
                        valid &= target.contains_encoded_key(encoded);
                        exact_mask.insert(encoded, ());
                    }
                })
                .expect("PathMap pattern mode was checked");
        }
        EPathMapMode::Map => {
            pattern
                .entry_trie()
                .for_each_raw_map_entry(|encoded, pattern_value| {
                    let metadata = EntryTrie::encoded_key_metadata(encoded)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    if metadata.connective_used {
                        let key = decode_trie_path(encoded)
                            .expect("dynamic map pattern key must decode once");
                        dynamic_patterns.push(PathPattern::MapDynamicKey(Arc::new((
                            key,
                            pattern_value.clone(),
                        ))));
                    } else if pattern_value.connective_used() {
                        dynamic_patterns.push(PathPattern::MapExactKey {
                            encoded_key: Arc::from(encoded),
                            value: Arc::new(pattern_value.clone()),
                        });
                    } else {
                        valid &= target
                            .entry_trie()
                            .get_map_value_by_encoded_key(encoded)
                            .ok()
                            .flatten()
                            == Some(pattern_value);
                        exact_mask.insert(encoded, ());
                    }
                })
                .expect("PathMap pattern mode was checked");
        }
    }
    if !valid {
        return None;
    }
    let entries = target.entry_trie().subtract_encoded_keys(&exact_mask);
    Some((
        EPathMap::new(
            entries,
            target.locally_free.clone(),
            target.connective_used,
            target.remainder.clone(),
        ),
        dynamic_patterns,
    ))
}

#[cfg(test)]
#[path = "../../../../tests/support/region_occurrence.rs"]
mod region_occurrence_tests;

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn machine_layout_stays_compact() {
        // These enums are stored in depth- and width-proportional contiguous
        // vectors. Before the large states were boxed they were 1,504 and
        // 1,112 bytes respectively, so a sequence of seven 64-byte
        // ListRequests reserved more than 10 KiB. Keep this bound explicit:
        // adding an inline schema payload to either enum is a memory-complexity
        // regression even when the traversal remains stack-safe.
        assert!(
            std::mem::size_of::<Job>() <= 64,
            "Job grew to {} bytes",
            std::mem::size_of::<Job>()
        );
        assert!(
            std::mem::size_of::<Frame>() <= 64,
            "Frame grew to {} bytes",
            std::mem::size_of::<Frame>()
        );
        #[cfg(target_pointer_width = "64")]
        assert!(
            std::mem::size_of::<EncodedConnectiveChildren>() <= 40,
            "owned encoded-connective children grew to {} bytes",
            std::mem::size_of::<EncodedConnectiveChildren>()
        );
    }
}
