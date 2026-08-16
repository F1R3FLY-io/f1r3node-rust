//! Stack-safe spatial matching machine.
//!
//! Spatial matching is one mutually recursive semantic operation even though
//! the source-level entry points are split across `Par`, `Expr`, `Send`,
//! `Receive`, connectives, ordered folds, unordered collection matching and
//! EPathMap. This module defunctionalizes that whole call graph into one
//! `Job`/`Frame` pushdown automaton. A nested pattern therefore grows heap work
//! storage, never the native call stack.

#[cfg(test)]
use std::cell::Cell;
use std::cmp::Ordering;
use std::num::NonZeroU64;
use std::rc::Rc;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rhoapi_ext::OwnedEPathMapEntry;
use models::rust::rholang::bincode_encoder;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::rholang::match_path::encode_match_path;
use models::rust::rholang::match_schema_tables::ParCursor;
use models::rust::utils::*;
use pathmap::PathMap;
use rspace_plus_plus::rspace::bag::Bag;
use rspace_plus_plus::rspace::rho_path::{RhoPathQuery, RhoPathV1, StructuralSequenceEncoder};

use super::exports::*;
use super::has_locally_free::HasLocallyFree;
#[cfg(test)]
use super::lazy_relation::RelationalMatchStats;
use super::lazy_relation::{EdgeResult, LazyRelation, RelationCursor};
use super::list_match::aggregate_updates;
use super::match_pars::match_pars;
use super::par_count::ParCount;
use super::r#match::{rho_path_query_atom, rho_path_query_cursor_atom};
use super::spatial_matcher::SpatialMatcherContext;
use super::sub_pars::sub_pars;

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
}

#[cfg(test)]
fn update_list_relation_stats(update: impl FnOnce(&mut RelationalMatchStats)) {
    LIST_RELATION_STATS.with(|stats| {
        let mut current = stats.get();
        update(&mut current);
        stats.set(current);
    });
}

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

#[cfg(test)]
pub(super) fn send_target_index_shape(targets: Vec<Send>) -> (u128, usize) {
    let targets = targets
        .into_iter()
        .map(MatchValue::Send)
        .collect::<Vec<_>>();
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
pda_entry!(match_receive_bind, ReceiveBind, ReceiveBind);
pda_entry!(match_case, MatchCase, MatchCase);

enum Job {
    Match(Box<MatchPair>),
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
        snapshot: FreeMap,
    },
    OrBranch {
        target: Box<Par>,
        patterns: Arc<[Par]>,
        next: usize,
        snapshot: FreeMap,
    },
    Not {
        snapshot: FreeMap,
    },
    BindOnSuccess {
        level: i32,
        value: Box<Par>,
    },
    ConnectiveCandidate {
        state: Box<ConnectiveCandidates>,
        snapshot: Option<FreeMap>,
        remainder: Box<Par>,
    },
    ListEdge {
        machine: Box<ListMachine>,
        target_index: usize,
        candidate_was_seen: bool,
        snapshot: FreeMap,
    },
    PathEdge {
        machine: Box<PathMachine>,
        target_key: Vec<u8>,
        snapshot: FreeMap,
    },
}

/// Contiguous heap continuation stack. `Frame` never owns another `Frame`, so
/// normal unwinding and early failure both dismantle it iteratively.
struct FrameStack(Vec<Frame>);

impl FrameStack {
    fn new() -> Self { Self(Vec::new()) }

    fn push(&mut self, frame: Frame) { self.0.push(frame); }

    fn pop(&mut self) -> Option<Frame> { self.0.pop() }
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
    ReceiveBind(ReceiveBind, ReceiveBind),
    MatchCase(MatchCase, MatchCase),
}

fn drive(context: &mut SpatialMatcherContext, root: MatchPair) -> Option<()> {
    drive_job(context, match_job(root))
}

fn drive_job(context: &mut SpatialMatcherContext, mut job: Job) -> Option<()> {
    let mut frames = FrameStack::new();

    loop {
        job = match job {
            Job::Match(pair) => evaluate_pair(context, *pair, &mut frames),
            Job::ParConnectives(state) => step_par_connectives(*state, &mut frames),
            Job::ConnectiveCandidates(state) => {
                step_connective_candidates(context, *state, &mut frames)
            }
            Job::ListInit(request) => init_list(context, request, &mut frames),
            Job::List(machine) => step_list(context, *machine, &mut frames),
            Job::PathInit(pair) => {
                let (target, pattern) = *pair;
                init_path(context, target, pattern, &mut frames)
            }
            Job::Path(machine) => step_path(context, *machine, &mut frames),
            Job::Return(result) => match frames.pop() {
                Some(frame) => resume(context, frame, result, &mut frames),
                None => return result.then_some(()),
            },
        };
    }
}

fn resume(
    context: &mut SpatialMatcherContext,
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
        Frame::RestoreOnFailure { snapshot } => {
            if !result {
                context.free_map = snapshot;
            }
            Job::Return(result)
        }
        Frame::OrBranch {
            target,
            patterns,
            next,
            snapshot,
        } => {
            context.free_map = snapshot;
            if result {
                Job::Return(true)
            } else {
                start_or_branch(context, *target, patterns, next, frames)
            }
        }
        Frame::Not { snapshot } => {
            context.free_map = snapshot;
            Job::Return(!result)
        }
        Frame::BindOnSuccess { level, value } => {
            if result {
                context.free_map.insert(level, *value);
            }
            Job::Return(result)
        }
        Frame::ConnectiveCandidate {
            state,
            snapshot,
            remainder,
        } => {
            if let Some(snapshot) = snapshot {
                if !result {
                    context.free_map = snapshot;
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
            snapshot,
        } => {
            let mut produced = std::mem::replace(&mut context.free_map, snapshot);
            if result {
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
                // The final fold already owns the attempt baseline. Retain
                // only writes that differ from it instead of duplicating the
                // complete FreeMap once per successful relation edge.
                produced.retain(|level, value| context.free_map.get(level) != Some(value));
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
                Job::List(machine)
            }
        }
        Frame::PathEdge {
            machine,
            target_key,
            snapshot,
        } => {
            let produced = std::mem::replace(&mut context.free_map, snapshot);
            if result {
                (*machine).accept_edge(target_key, produced)
            } else {
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
    context: &mut SpatialMatcherContext,
    pair: MatchPair,
    frames: &mut FrameStack,
) -> Job {
    match pair {
        MatchPair::Par(target, pattern) => evaluate_par(target, pattern),
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
        MatchPair::ReceiveBind(target, pattern) => evaluate_receive_bind(target, pattern),
        MatchPair::MatchCase(target, pattern) => evaluate_match_case(target, pattern),
    }
}

#[derive(Clone)]
struct ConnectivePlan {
    connective: Connective,
    bounds: (ParCount, ParCount),
    remainder_bounds: (ParCount, ParCount),
}

struct ParConnectiveState {
    target: Par,
    pattern: Par,
    plans: Arc<[ConnectivePlan]>,
    next: usize,
    remainder_level: Option<i32>,
    wildcard: bool,
}

struct ConnectiveCandidates {
    parent: ParConnectiveState,
    connective: Connective,
    candidates: Vec<(Par, Par)>,
    next: usize,
    free_map_is_unreachable: bool,
}

fn evaluate_par(target: Par, mut pattern: Par) -> Job {
    if !pattern.connective_used {
        return Job::Return(match_pars(&target, &pattern));
    }

    let remainder_level = pattern.exprs.iter().find_map(|expr| match expr {
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
    let wildcard = pattern.exprs.iter().any(|expr| {
        matches!(expr, Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(Wildcard(_)),
                }),
            })),
        })
    });

    pattern.exprs.retain(|expr| {
        !matches!(expr, Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(FreeVar(_) | Wildcard(_)),
                }),
            })),
        })
    });
    let count = ParCount::new(&pattern);
    let min_remainder = count.clone();
    let max_remainder = if wildcard || remainder_level.is_some() {
        count._max()
    } else {
        count.clone()
    };
    let individual: Vec<_> = pattern
        .connectives
        .iter()
        .cloned()
        .map(|connective| count.min_max_con(connective))
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

    let plans = std::mem::take(&mut pattern.connectives)
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
        plans: plans.into(),
        next: 0,
        remainder_level,
        wildcard,
    }))
}

fn step_par_connectives(mut state: ParConnectiveState, frames: &mut FrameStack) -> Job {
    let Some(plan) = state.plans.get(state.next).cloned() else {
        return start_par_fields(state, frames);
    };
    state.next += 1;
    let candidates = sub_pars(
        &state.target,
        &plan.bounds.0,
        &plan.bounds.1,
        &plan.remainder_bounds.0,
        &plan.remainder_bounds.1,
    )
    .collect();
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
    Job::ConnectiveCandidates(Box::new(ConnectiveCandidates {
        parent: state,
        connective: plan.connective,
        candidates,
        next: 0,
        free_map_is_unreachable,
    }))
}

fn step_connective_candidates(
    context: &mut SpatialMatcherContext,
    mut state: ConnectiveCandidates,
    frames: &mut FrameStack,
) -> Job {
    let Some((candidate, remainder)) = state.candidates.get(state.next).cloned() else {
        return Job::Return(false);
    };
    state.next += 1;
    let snapshot = (!state.free_map_is_unreachable).then(|| context.free_map.clone());
    let connective = state.connective.clone();
    frames.push(Frame::ConnectiveCandidate {
        state: Box::new(state),
        snapshot,
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
            std::mem::take(&mut target.sends)
                .into_iter()
                .map(MatchValue::Send)
                .collect(),
            std::mem::take(&mut pattern.sends)
                .into_iter()
                .map(MatchValue::Send)
                .collect(),
            Merger::ParSends,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.receives)
                .into_iter()
                .map(MatchValue::Receive)
                .collect(),
            std::mem::take(&mut pattern.receives)
                .into_iter()
                .map(MatchValue::Receive)
                .collect(),
            Merger::ParReceives,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.news)
                .into_iter()
                .map(MatchValue::New)
                .collect(),
            std::mem::take(&mut pattern.news)
                .into_iter()
                .map(MatchValue::New)
                .collect(),
            Merger::ParNews,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.exprs)
                .into_iter()
                .map(MatchValue::Expr)
                .collect(),
            std::mem::take(&mut pattern.exprs)
                .into_iter()
                .map(MatchValue::Expr)
                .collect(),
            Merger::ParExprs,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.matches)
                .into_iter()
                .map(MatchValue::Match)
                .collect(),
            std::mem::take(&mut pattern.matches)
                .into_iter()
                .map(MatchValue::Match)
                .collect(),
            Merger::ParMatches,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.bundles)
                .into_iter()
                .map(MatchValue::Bundle)
                .collect(),
            std::mem::take(&mut pattern.bundles)
                .into_iter()
                .map(MatchValue::Bundle)
                .collect(),
            Merger::ParBundles,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.unforgeables)
                .into_iter()
                .map(MatchValue::Unforgeable)
                .collect(),
            std::mem::take(&mut pattern.unforgeables)
                .into_iter()
                .map(MatchValue::Unforgeable)
                .collect(),
            Merger::ParUnforgeables,
            level,
            wildcard,
        ),
    ];
    start_jobs(requests.into_iter().map(Job::ListInit).collect(), frames)
}

fn evaluate_connective(
    context: &mut SpatialMatcherContext,
    target: Par,
    pattern: Connective,
    frames: &mut FrameStack,
) -> Job {
    match pattern.connective_instance {
        Some(ConnAndBody(ConnectiveBody { ps })) => {
            let snapshot = context.free_map.clone();
            frames.push(Frame::RestoreOnFailure { snapshot });
            start_jobs(
                ps.into_iter()
                    .map(|pattern| match_job(MatchPair::Par(target.clone(), pattern)))
                    .collect(),
                frames,
            )
        }
        Some(ConnOrBody(ConnectiveBody { ps })) => {
            start_or_branch(context, target, ps.into(), 0, frames)
        }
        Some(ConnNotBody(pattern)) => {
            let snapshot = context.free_map.clone();
            frames.push(Frame::Not { snapshot });
            match_job(MatchPair::Par(target, pattern))
        }
        Some(VarRefBody(_)) | None => Job::Return(false),
        Some(ConnBool(_)) => Job::Return(matches!(
            single_expr(&target),
            Some(Expr {
                expr_instance: Some(GBool(_)),
            })
        )),
        Some(ConnInt(_)) => Job::Return(matches!(
            single_expr(&target),
            Some(Expr {
                expr_instance: Some(GInt(_)),
            })
        )),
        Some(ConnString(_)) => Job::Return(matches!(
            single_expr(&target),
            Some(Expr {
                expr_instance: Some(GString(_)),
            })
        )),
        Some(ConnUri(_)) => Job::Return(matches!(
            single_expr(&target),
            Some(Expr {
                expr_instance: Some(GUri(_)),
            })
        )),
        Some(ConnByteArray(_)) => Job::Return(matches!(
            single_expr(&target),
            Some(Expr {
                expr_instance: Some(GByteArray(_)),
            })
        )),
    }
}

fn start_or_branch(
    context: &mut SpatialMatcherContext,
    target: Par,
    patterns: Arc<[Par]>,
    next: usize,
    frames: &mut FrameStack,
) -> Job {
    let Some(pattern) = patterns.get(next).cloned() else {
        return Job::Return(false);
    };
    let snapshot = context.free_map.clone();
    frames.push(Frame::OrBranch {
        target: Box::new(target.clone()),
        patterns,
        next: next + 1,
        snapshot,
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
            Some(FreeVar(_)) if surplus.iter().all(|par| par.locally_free.is_empty()) => {}
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
    Send(Send),
    Receive(Receive),
    New(New),
    Expr(Expr),
    Match(Match),
    Bundle(Bundle),
    Unforgeable(GUnforgeable),
    ReceiveBind(ReceiveBind),
}

impl MatchValue {
    fn rho_path_structure(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(match self {
            Self::ParPair(_) => 2,
            _ => 1,
        });
        match self {
            Self::Par(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::ParPair((key, value)) => {
                output.push(&encode_match_path(ParCursor::from(key)));
                output.push(&encode_match_path(ParCursor::from(value)));
            }
            Self::Send(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Receive(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::New(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Expr(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Match(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Bundle(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::Unforgeable(value) => output.push(&encode_match_path(ParCursor::from(value))),
            Self::ReceiveBind(value) => output.push(&encode_match_path(ParCursor::from(value))),
        }
        output.finish()
    }

    /// Collision-free identity for every unordered `Par` field type. Each
    /// generated-schema root uses the one-walk, stack-safe canonical bincode
    /// emitter and the same injective sequence framing as structure.
    fn rho_path_identity(&self) -> Vec<u8> {
        let mut output = StructuralSequenceEncoder::with_capacity(match self {
            Self::ParPair(_) => 2,
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
            Self::Send(value) => push_identity!(value),
            Self::Receive(value) => push_identity!(value),
            Self::New(value) => push_identity!(value),
            Self::Expr(value) => push_identity!(value),
            Self::Match(value) => push_identity!(value),
            Self::Bundle(value) => push_identity!(value),
            Self::Unforgeable(value) => push_identity!(value),
            Self::ReceiveBind(value) => push_identity!(value),
        }
        output.finish()
    }

    fn rho_path_query(
        &self,
        context: &SpatialMatcherContext,
        captures: &mut PathMap<()>,
    ) -> Option<RhoPathQuery> {
        let exact = !self.connective_used(context) && self.locally_free_is_empty(context);
        let atoms = match self {
            Self::Par(value) => vec![rho_path_query_atom(value, captures)?],
            Self::ParPair((key, value)) => vec![
                rho_path_query_atom(key, captures)?,
                rho_path_query_atom(value, captures)?,
            ],
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
            Self::ReceiveBind(value) => vec![rho_path_query_cursor_atom(
                ParCursor::from(value),
                exact,
                captures,
            )?],
        };
        Some(RhoPathQuery::structural_sequence(atoms, false))
    }

    fn connective_used(&self, _context: &SpatialMatcherContext) -> bool {
        match self {
            MatchValue::Par(value) => value.connective_used,
            MatchValue::ParPair((key, value)) => key.connective_used || value.connective_used,
            MatchValue::Send(value) => value.connective_used,
            MatchValue::Receive(value) => value.connective_used,
            MatchValue::New(value) => value.p.as_ref().expect("New.p").connective_used,
            MatchValue::Expr(value) => expr_connective_used(value),
            MatchValue::Match(value) => value.connective_used,
            MatchValue::Bundle(_) | MatchValue::Unforgeable(_) => false,
            MatchValue::ReceiveBind(value) => {
                value
                    .source
                    .as_ref()
                    .expect("ReceiveBind.source")
                    .connective_used
            }
        }
    }

    fn locally_free_is_empty(&self, context: &SpatialMatcherContext) -> bool {
        match self {
            MatchValue::Par(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::ParPair(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Send(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Receive(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::New(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Expr(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Match(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Bundle(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::Unforgeable(value) => context.locally_free(value.clone(), 0).is_empty(),
            MatchValue::ReceiveBind(value) => context.locally_free(value.clone(), 0).is_empty(),
        }
    }

    fn into_pair(self, pattern: MatchValue) -> Option<MatchPair> {
        match (self, pattern) {
            (MatchValue::Par(target), MatchValue::Par(pattern)) => {
                Some(MatchPair::Par(target, pattern))
            }
            (MatchValue::ParPair(target), MatchValue::ParPair(pattern)) => {
                Some(MatchPair::ParPair(target, pattern))
            }
            (MatchValue::Send(target), MatchValue::Send(pattern)) => {
                Some(MatchPair::Send(target, pattern))
            }
            (MatchValue::Receive(target), MatchValue::Receive(pattern)) => {
                Some(MatchPair::Receive(target, pattern))
            }
            (MatchValue::New(target), MatchValue::New(pattern)) => {
                Some(MatchPair::New(target, pattern))
            }
            (MatchValue::Expr(target), MatchValue::Expr(pattern)) => {
                Some(MatchPair::Expr(target, pattern))
            }
            (MatchValue::Match(target), MatchValue::Match(pattern)) => {
                Some(MatchPair::Match(target, pattern))
            }
            (MatchValue::Bundle(target), MatchValue::Bundle(pattern)) => {
                Some(MatchPair::Bundle(target, pattern))
            }
            (MatchValue::Unforgeable(target), MatchValue::Unforgeable(pattern)) => {
                Some(MatchPair::Unforgeable(target, pattern))
            }
            (MatchValue::ReceiveBind(target), MatchValue::ReceiveBind(pattern)) => {
                Some(MatchPair::ReceiveBind(target, pattern))
            }
            _ => None,
        }
    }
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
        Some(ENotBody(ENot { p })) => p.as_ref().expect("ENot.p").connective_used,
        Some(ENegBody(ENeg { p })) => p.as_ref().expect("ENeg.p").connective_used,
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
            p1.as_ref().expect("binary expression p1").connective_used
                || p2.as_ref().expect("binary expression p2").connective_used
        }
        Some(EMethodBody(value)) => value.connective_used,
        Some(EMatchesBody(EMatches { target, .. })) => {
            target.as_ref().expect("EMatches.target").connective_used
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
            Merger::ESet => {
                par.with_exprs(vec![new_eset_expr(typed!(Par), Vec::new(), false, None)])
            }
            Merger::EMap => par.with_exprs(vec![new_emap_expr(
                typed!(ParPair)
                    .into_iter()
                    .map(|(key, value)| KeyValuePair {
                        key: Some(key),
                        value: Some(value),
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

struct ListTargetGroup {
    key: Vec<u8>,
    structure: Vec<u8>,
    positions: Vec<u64>,
}

/// Derived prefix-compressed target catalog for one unordered list match.
///
/// The authoritative trie is a [`Bag`]: a `PathMap<Multiplicity>` whose key is
/// the framed RMP1 structure followed by a collision-free, stack-safe bincode
/// identity. Structurally equal values therefore share trie prefixes while
/// semantically equal values share one multiplicity cell. A parallel
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
            // to one another. Their generated `Ord` is itself a stack-safe
            // worklist traversal and, unlike Rholang's AlwaysEqual
            // `PartialEq`, includes transient `locally_free` metadata. Thus
            // `Equal` here proves both RMP1 structure and canonical bincode
            // identity can be reused. The ordinary exact-key lookup below
            // remains authoritative for identical values that are not
            // adjacent.
            if let Some((previous_target, id)) = previous {
                if previous_target.cmp(target) == Ordering::Equal {
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
        debug_assert_eq!(bag.cardinality(), targets.len() as u128);
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
        let group_index = usize::try_from(id.0.get() - 1)
            .expect("a target group id originated as an addressable Vec index");
        &self.groups[group_index]
    }

    fn positions_matching(&self, query: &RhoPathQuery) -> Option<Box<[u64]>> {
        let structural_prefix = query.retained_prefix();
        if structural_prefix.len() <= 1 {
            return None;
        }
        let prefix = RhoPathV1::relative_datum_structure_prefix(&structural_prefix);
        let mut result = Vec::new();
        self.bag.visit_prefix(&prefix, |key, multiplicity| {
            let id = self
                .group_ids
                .get(key)
                .copied()
                .expect("every Bag identity has a target group");
            let group_index = usize::try_from(id.0.get() - 1)
                .expect("a target group id originated as an addressable Vec index");
            let group = &self.groups[group_index];
            debug_assert_eq!(
                usize::try_from(multiplicity.get()).ok(),
                Some(group.positions.len())
            );
            if query.matches_structure(&group.structure) {
                result.extend_from_slice(&group.positions);
            }
        });
        result.sort_unstable();
        Some(result.into_boxed_slice())
    }
}

enum ListAssignment {
    Term {
        pattern_index: usize,
        free_map: EdgeResult<FreeMap>,
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
    relation: LazyRelation<FreeMap>,
    empty_delta: Rc<FreeMap>,
    merger: Merger,
    remainder: Option<i32>,
    wildcard: bool,
}

fn init_list(
    context: &mut SpatialMatcherContext,
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
        if !pattern.connective_used(context) {
            return Job::Return(target == pattern);
        }
        let Some(pair) = target.into_pair(pattern) else {
            return Job::Return(false);
        };
        frames.push(Frame::RestoreOnFailure {
            snapshot: context.free_map.clone(),
        });
        return match_job(pair);
    }
    if pattern_len == 0 && target_len == 0 && request.remainder.is_none() {
        return Job::Return(true);
    }
    if pattern_len == 0 {
        if let Some(level) = request.remainder {
            if !request
                .targets
                .iter()
                .all(|value| value.locally_free_is_empty(context))
            {
                return Job::Return(false);
            }
            let prior = context
                .free_map
                .get(&level)
                .cloned()
                .unwrap_or_else(|| vector_par(Vec::new(), false));
            context
                .free_map
                .insert(level, request.merger.merge(prior, request.targets));
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
    let queries = patterns
        .iter()
        .map(|pattern| pattern.rho_path_query(context, &mut captures))
        .collect::<Box<[_]>>();
    let needs_target_index = request.remainder.is_some()
        || queries
            .iter()
            .flatten()
            .any(|query| query.retained_prefix().len() > 1);
    #[cfg(test)]
    if needs_target_index {
        update_list_relation_stats(|stats| stats.target_index_builds += 1);
    }
    let target_index = needs_target_index
        .then(|| ListTargetIndex::from_targets(&request.targets))
        .flatten();
    let candidate_domains = queries
        .iter()
        .map(|query| {
            query
                .as_ref()
                .and_then(|query| {
                    target_index
                        .as_ref()
                        .and_then(|index| index.positions_matching(query))
                })
                .map_or(ListCandidateDomain::All, ListCandidateDomain::Indexed)
        })
        .collect::<Box<[_]>>();
    let relation = LazyRelation::new(std::iter::repeat_n(true, patterns.len()));

    let seen_targets = ListSeenTargets::new(target_index.as_ref(), target_len);
    let mut assignments = (0..target_len).map(|_| None).collect::<Vec<_>>();
    if request.remainder.is_some() {
        let mut remaining = target_len - pattern_len;
        for (target, assignment) in request.targets.iter().zip(assignments.iter_mut()) {
            if remaining == 0 {
                break;
            }
            if target.locally_free_is_empty(context) {
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
        empty_delta: Rc::new(new_free_map()),
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

    fn accept_term_edge(self, target_index: usize, free_map: EdgeResult<FreeMap>) -> Job {
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
    context: &mut SpatialMatcherContext,
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
                if machine.targets[target_index].locally_free_is_empty(context) {
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
        if !pattern.connective_used(context) {
            if &machine.targets[target_index] == pattern {
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
            let Some(pair) = machine.targets[target_index]
                .clone()
                .into_pair(pattern.clone())
            else {
                continue;
            };
            let snapshot = context.free_map.clone();
            frames.push(Frame::ListEdge {
                machine: Box::new(machine),
                target_index,
                candidate_was_seen,
                snapshot,
            });
            return match_job(pair);
        }
    }
}

fn finish_list(context: &mut SpatialMatcherContext, machine: ListMachine) -> Job {
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
            .all(|target| target.locally_free_is_empty(context))
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
    ordered.sort_by(|(left_index, _), (right_index, _)| {
        match targets[*left_index].cmp(&targets[*right_index]) {
            Ordering::Equal => left_index.cmp(right_index),
            order => order,
        }
    });
    let maps = ordered
        .into_iter()
        .map(|(_, free_map)| free_map.into_owned())
        .collect::<Vec<_>>();
    let Some(updated) = aggregate_updates(context.free_map.clone(), maps) else {
        return Job::Return(false);
    };
    context.free_map = updated;

    match remainder {
        None if wildcard || remainder_sorted.is_empty() => Job::Return(true),
        None => Job::Return(false),
        Some(level) => {
            let prior = context
                .free_map
                .get(&level)
                .cloned()
                .unwrap_or_else(|| vector_par(Vec::new(), false));
            context
                .free_map
                .insert(level, merger.merge(prior, remainder_sorted));
            Job::Return(true)
        }
    }
}

#[derive(Clone)]
enum PathPattern {
    Remainder,
    Set(Arc<Par>),
    Map(Arc<(Par, Par)>),
}

#[derive(Clone)]
struct PathAssignment {
    pattern: PathPattern,
    free_map: FreeMap,
}

struct PathSearchFrame {
    pattern: PathPattern,
    next_target: Vec<u8>,
    pending: Option<(Vec<u8>, FreeMap)>,
}

#[derive(Clone, Copy)]
enum PathRemainder {
    None,
    Wildcard,
    Free(i32),
}

struct PathMachine {
    target: EPathMap,
    pattern: EPathMap,
    assignments: pathmap::PathMap<PathAssignment>,
    remainder: PathRemainder,
    remainder_roots: usize,
    dynamic_cursor: Vec<u8>,
    roots_finished: bool,
    seen: pathmap::PathMap<()>,
    search: Vec<PathSearchFrame>,
}

fn init_path(
    context: &mut SpatialMatcherContext,
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
    let Some((target, dynamic_patterns)) = prepare_exact_path_entries(&target, &pattern) else {
        return Job::Return(false);
    };
    let target_count = target.len();
    match remainder {
        PathRemainder::None if dynamic_patterns != target_count => return Job::Return(false),
        PathRemainder::Wildcard | PathRemainder::Free(_) if dynamic_patterns > target_count => {
            return Job::Return(false);
        }
        _ => {}
    }

    let mut machine = PathMachine {
        target,
        pattern,
        assignments: pathmap::PathMap::new(),
        remainder,
        remainder_roots: if matches!(remainder, PathRemainder::Free(_)) {
            target_count - dynamic_patterns
        } else {
            0
        },
        dynamic_cursor: Vec::new(),
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
    debug_assert!(
        entries.next().is_none(),
        "singleton fast path received multiple entries"
    );
    Some(first)
}

fn init_single_path(
    context: &mut SpatialMatcherContext,
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
            let decoded_pattern =
                decode_trie_path(&pattern).expect("set pattern key is a canonical Par path");
            if !decoded_pattern.connective_used {
                // A concrete set member is its canonical PathMap key. The
                // compressed bytes are therefore the complete equality test.
                return Job::Return(target == pattern);
            }
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
            let decoded_pattern_key =
                decode_trie_path(&pattern_key).expect("map pattern key is a canonical Par path");
            if !decoded_pattern_key.connective_used {
                // Canonical PathMap key bytes are the equality representation.
                // A concrete singleton key therefore needs neither Par decoding
                // nor a second PDA continuation: compare the compressed bytes
                // and descend directly into the dynamic map value.
                if target_key != pattern_key {
                    return Job::Return(false);
                }
                if !pattern_value.connective_used {
                    return Job::Return(target_value == pattern_value);
                }
                MatchPair::Par(target_value, pattern_value)
            } else {
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
        snapshot: context.free_map.clone(),
    });
    match_job(pair)
}

impl PathMachine {
    fn start_next_root(&mut self) {
        let root = if self.remainder_roots != 0 {
            self.remainder_roots -= 1;
            Some(PathPattern::Remainder)
        } else if let Some((key, pattern)) =
            next_dynamic_path_pattern(&self.pattern, &self.dynamic_cursor)
        {
            self.dynamic_cursor = key;
            Some(pattern)
        } else {
            self.roots_finished = true;
            None
        };

        if let Some(pattern) = root {
            self.seen = pathmap::PathMap::new();
            self.search.push(PathSearchFrame {
                pattern,
                next_target: Vec::new(),
                pending: None,
            });
        }
    }

    fn accept_edge(mut self, target_key: Vec<u8>, free_map: FreeMap) -> Job {
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
    context: &mut SpatialMatcherContext,
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
                    let produced = context.free_map.clone();
                    return machine.accept_edge(target_key, produced);
                }
            }
            PathPattern::Set(pattern) => {
                let target = decode_trie_path(&target_key)
                    .expect("set-mode EPathMap keys are canonical Par paths");
                let snapshot = context.free_map.clone();
                frames.push(Frame::PathEdge {
                    machine: Box::new(machine),
                    target_key,
                    snapshot,
                });
                return match_job(MatchPair::Par(target, pattern.as_ref().clone()));
            }
            PathPattern::Map(pattern) => {
                let key = decode_trie_path(&target_key)
                    .expect("map-mode EPathMap keys are canonical Par paths");
                let value = machine
                    .target
                    .entry_trie()
                    .get_map_value_by_encoded_key(&target_key)
                    .expect("PathMap target mode was checked")
                    .expect("a PathMap candidate key names a value")
                    .clone();
                let snapshot = context.free_map.clone();
                frames.push(Frame::PathEdge {
                    machine: Box::new(machine),
                    target_key,
                    snapshot,
                });
                return match_job(MatchPair::ParPair((key, value), pattern.as_ref().clone()));
            }
        }
    }
}

fn path_target_is_concrete(target: &EPathMap, key: &[u8]) -> bool {
    match target.mode() {
        EPathMapMode::Empty => false,
        EPathMapMode::Set => decode_trie_path(key)
            .expect("set-mode EPathMap keys are canonical Par paths")
            .locally_free
            .is_empty(),
        EPathMapMode::Map => {
            let key_par =
                decode_trie_path(key).expect("map-mode EPathMap keys are canonical Par paths");
            let value = target
                .entry_trie()
                .get_map_value_by_encoded_key(key)
                .expect("PathMap target mode was checked")
                .expect("a PathMap candidate key names a value");
            key_par.locally_free.is_empty() && value.locally_free.is_empty()
        }
    }
}

fn finish_path(context: &mut SpatialMatcherContext, machine: PathMachine) -> Job {
    let mut remainder_map = EPathMap::default();
    let mut maps = Vec::with_capacity(machine.assignments.val_count());
    for (target_key, assignment) in machine.assignments {
        if matches!(assignment.pattern, PathPattern::Remainder) {
            match machine.target.mode() {
                EPathMapMode::Empty => {}
                EPathMapMode::Set => {
                    remainder_map.insert_entry(
                        decode_trie_path(&target_key)
                            .expect("set-mode EPathMap keys are canonical Par paths"),
                    );
                }
                EPathMapMode::Map => {
                    let key = decode_trie_path(&target_key)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    let value = machine
                        .target
                        .entry_trie()
                        .get_map_value_by_encoded_key(&target_key)
                        .expect("PathMap target mode was checked")
                        .expect("an assigned PathMap key names a value")
                        .clone();
                    remainder_map
                        .insert_map_entry(key, value)
                        .expect("a fresh remainder preserves homogeneous map mode");
                }
            }
        }
        maps.push(assignment.free_map);
    }

    let Some(updated) = aggregate_updates(context.free_map.clone(), maps) else {
        return Job::Return(false);
    };
    context.free_map = updated;
    if let PathRemainder::Free(level) = machine.remainder {
        let bound = context
            .free_map
            .get(&level)
            .cloned()
            .unwrap_or_default()
            .with_exprs(vec![Expr {
                expr_instance: Some(EPathmapBody(remainder_map)),
            }]);
        context.free_map.insert(level, bound);
    }
    Job::Return(true)
}

fn prepare_exact_path_entries(target: &EPathMap, pattern: &EPathMap) -> Option<(EPathMap, usize)> {
    let mut exact_mask = EPathMap::default();
    let mut dynamic = 0usize;
    let mut valid = true;

    match pattern.mode() {
        EPathMapMode::Empty => {}
        EPathMapMode::Set => {
            pattern
                .entry_trie()
                .for_each_raw_set_entry(|encoded| {
                    let entry = decode_trie_path(encoded)
                        .expect("set-mode EPathMap keys are canonical Par paths");
                    if entry.connective_used {
                        dynamic += 1;
                    } else {
                        valid &= target.contains_encoded_key(encoded);
                        exact_mask.insert_entry(entry);
                    }
                })
                .expect("PathMap pattern mode was checked");
        }
        EPathMapMode::Map => {
            pattern
                .entry_trie()
                .for_each_raw_map_entry(|encoded, pattern_value| {
                    let key = decode_trie_path(encoded)
                        .expect("map-mode EPathMap keys are canonical Par paths");
                    if key.connective_used || pattern_value.connective_used {
                        dynamic += 1;
                    } else {
                        valid &= target
                            .entry_trie()
                            .get_map_value_by_encoded_key(encoded)
                            .ok()
                            .flatten()
                            == Some(pattern_value);
                        exact_mask
                            .insert_map_entry(key, Par::default())
                            .expect("an exact mask preserves homogeneous map mode");
                    }
                })
                .expect("PathMap pattern mode was checked");
        }
    }
    if !valid {
        return None;
    }
    let entries = target
        .entry_trie()
        .try_subtract(exact_mask.entry_trie())
        .ok()?;
    Some((
        EPathMap::new(
            entries,
            target.locally_free.clone(),
            target.connective_used,
            target.remainder.clone(),
        ),
        dynamic,
    ))
}

fn next_dynamic_path_pattern(pattern: &EPathMap, after: &[u8]) -> Option<(Vec<u8>, PathPattern)> {
    let mut cursor = after.to_vec();
    loop {
        let key = pattern.next_value_key(&cursor)?;
        cursor = key.clone();
        let decoded =
            decode_trie_path(&key).expect("EPathMap pattern keys are canonical Par paths");
        match pattern.mode() {
            EPathMapMode::Empty => return None,
            EPathMapMode::Set if decoded.connective_used => {
                return Some((key, PathPattern::Set(Arc::new(decoded))));
            }
            EPathMapMode::Set => {}
            EPathMapMode::Map => {
                let value = pattern
                    .entry_trie()
                    .get_map_value_by_encoded_key(&key)
                    .expect("PathMap pattern mode was checked")
                    .expect("a PathMap pattern key names a value");
                if decoded.connective_used || value.connective_used {
                    return Some((key, PathPattern::Map(Arc::new((decoded, value.clone())))));
                }
            }
        }
    }
}

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
    }
}
