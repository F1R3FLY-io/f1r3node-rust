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
use std::rc::Rc;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rhoapi_ext::OwnedEPathMapEntry;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;

use super::exports::*;
// Changed by the merge of feature/cost-accounted-rho (finding R3-17): the list
// values of the three added Par fields read their flags by reference.
// use super::has_locally_free::HasLocallyFree;
use super::has_locally_free::{HasLocallyFree, HasLocallyFreeRef};
#[cfg(test)]
use super::lazy_relation::RelationalMatchStats;
use super::lazy_relation::{EdgeResult, LazyRelation, RelationCursor};
use super::list_match::aggregate_updates;
use super::match_pars::match_pars;
use super::par_count::ParCount;
// Changed by the merge of feature/cost-accounted-rho (finding R1-51): the
// list mergers below call the shared in-place remainder mergers.
// use super::spatial_matcher::SpatialMatcherContext;
use super::spatial_matcher::{merge_map_remainder, merge_set_remainder, SpatialMatcherContext};
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
pda_entry!(match_if, If, If);
pda_entry!(match_cost_signed_term, CostSignedTerm, CostSignedTerm);
pda_entry!(match_cost_stack, CostStack, CostStack);

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
    If(If, If),
    CostSignedTerm(CostSignedTerm, CostSignedTerm),
    CostStack(CostStack, CostStack),
}

fn drive(context: &mut SpatialMatcherContext, root: MatchPair) -> Option<()> {
    drive_job(context, match_job(root))
}

fn drive_job(context: &mut SpatialMatcherContext, mut job: Job) -> Option<()> {
    let mut frames = FrameStack::new();

    loop {
        job = match job {
            Job::Match(pair) => evaluate_pair(context, *pair, &mut frames),
            Job::ParConnectives(state) => step_par_connectives(context, *state, &mut frames),
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
                let pattern_index = machine
                    .search
                    .last()
                    .expect("a suspended list edge has a search frame")
                    .pattern_index;
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
                    (*machine).accept_edge(target_index, EdgeResult::Shared(produced))
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
        MatchPair::Par(target, pattern) => {
            if let Some(level) = free_variable_level(&pattern) {
                bind_free_variable_owned(context, target, level)
            } else {
                evaluate_par(target, pattern)
            }
        }
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
        MatchPair::If(target, pattern) => Job::Return(target == pattern),
        MatchPair::CostSignedTerm(target, pattern) => Job::Return(target == pattern),
        MatchPair::CostStack(target, pattern) => Job::Return(target == pattern),
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

/// Exactly the sole free-variable expression, with no other pattern field.
/// The explicit field census includes cost terms and parallel stack tails.
fn free_variable_level(pattern: &Par) -> Option<i32> {
    if !pattern.connective_used
        || !pattern.sends.is_empty()
        || !pattern.receives.is_empty()
        || !pattern.news.is_empty()
        || !pattern.matches.is_empty()
        || !pattern.bundles.is_empty()
        || !pattern.unforgeables.is_empty()
        || !pattern.connectives.is_empty()
        || !pattern.conditionals.is_empty()
        || !pattern.cost_signed_terms.is_empty()
        || !pattern.cost_stacks.is_empty()
    {
        return None;
    }
    match pattern.exprs.as_slice() {
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

fn cost_stack_closed(root: &CostStack) -> Option<bool> {
    use models::rhoapi::cost_signature::Value;
    enum Pending<'a> {
        Stack(&'a CostStack),
        Signature(&'a CostSignature),
    }
    let mut pending = Vec::new();
    pending.push(Pending::Stack(root));
    while let Some(next) = pending.pop() {
        match next {
            Pending::Stack(stack) => {
                pending.extend(stack.parallel_tail.iter().map(Pending::Stack));
                pending.extend(stack.cells.iter().map(Pending::Signature));
            }
            Pending::Signature(signature) => match &signature.value {
                Some(Value::BoundLevel(level)) if *level >= 0 => return Some(false),
                Some(Value::Quote(par)) | Some(Value::Name(par))
                    if !par.locally_free.is_empty() =>
                {
                    return Some(false)
                }
                Some(Value::Compound(compound)) => {
                    pending.extend(compound.elements.iter().map(Pending::Signature));
                }
                _ => {}
            },
        }
    }
    Some(true)
}

fn bind_field_owned<T: Clone>(
    context: &mut SpatialMatcherContext,
    values: Vec<T>,
    level: i32,
    closed: impl Fn(&SpatialMatcherContext, &T) -> Option<bool>,
    assign: impl Fn(&mut Par, Vec<T>),
) -> Option<()> {
    for value in &values {
        if !closed(context, value)? {
            return None;
        }
    }
    if let Some(bound) = context.free_map.get_mut(&level) {
        assign(bound, values);
    } else {
        let mut bound = Par::default();
        assign(&mut bound, values);
        context.free_map.insert(level, bound);
    }
    Some(())
}

fn bind_field_borrowed<T: Clone>(
    context: &mut SpatialMatcherContext,
    values: &[T],
    level: i32,
    closed: impl Fn(&SpatialMatcherContext, &T) -> Option<bool>,
    assign: impl Fn(&mut Par, Vec<T>),
) -> Option<()> {
    for value in values {
        if !closed(context, value)? {
            return None;
        }
    }
    bind_field_owned(context, values.to_vec(), level, closed, assign)
}

fn closed_by_value<T: Clone>(
    context: &SpatialMatcherContext,
    value: &T,
    check: impl Fn(&SpatialMatcherContext, T) -> Vec<u8>,
) -> Option<bool> {
    Some(check(context, value.clone()).is_empty())
}

fn bind_free_variable_owned(
    context: &mut SpatialMatcherContext,
    mut target: Par,
    level: i32,
) -> Job {
    macro_rules! field {
        ($name:ident, $closed:expr) => {
            if bind_field_owned(
                context,
                std::mem::take(&mut target.$name),
                level,
                $closed,
                |par, values| par.$name = values,
            )
            .is_none()
            {
                return Job::Return(false);
            }
        };
    }
    field!(sends, |_, value: &Send| Some(value.locally_free.is_empty()));
    field!(receives, |_, value: &Receive| Some(
        value.locally_free.is_empty()
    ));
    field!(news, |_, value: &New| Some(value.locally_free.is_empty()));
    field!(exprs, |c: &SpatialMatcherContext, value: &Expr| {
        closed_by_value(c, value, |c, v| c.locally_free(v, 0))
    });
    field!(matches, |_, value: &Match| Some(
        value.locally_free.is_empty()
    ));
    field!(bundles, |_, value: &Bundle| Some(
        value
            .body
            .as_ref()
            .expect("Bundle.body")
            .locally_free
            .is_empty()
    ));
    field!(unforgeables, |_, _: &GUnforgeable| Some(true));
    field!(conditionals, |c: &SpatialMatcherContext, value: &If| {
        closed_by_value(c, value, |c, v| c.locally_free(v, 0))
    });
    field!(
        cost_signed_terms,
        |c: &SpatialMatcherContext, value: &CostSignedTerm| closed_by_value(c, value, |c, v| c
            .locally_free(v, 0))
    );
    // Changed by the merge of feature/cost-accounted-rho: the context carried
    // only the meter of `cost_stack_closed_metered`, which DR-131 removed.
    // field!(cost_stacks, |c: &SpatialMatcherContext,
    //                      value: &CostStack| {
    //     cost_stack_closed(value)
    // });
    field!(cost_stacks, |_, value: &CostStack| cost_stack_closed(value));
    Job::Return(true)
}

pub(super) fn bind_free_variable_borrowed(
    context: &mut SpatialMatcherContext,
    target: &Par,
    level: i32,
) -> Option<()> {
    macro_rules! field {
        ($name:ident, $closed:expr) => {
            bind_field_borrowed(context, &target.$name, level, $closed, |par, values| {
                par.$name = values
            })?;
        };
    }
    field!(sends, |_, value: &Send| Some(value.locally_free.is_empty()));
    field!(receives, |_, value: &Receive| Some(
        value.locally_free.is_empty()
    ));
    field!(news, |_, value: &New| Some(value.locally_free.is_empty()));
    field!(exprs, |c: &SpatialMatcherContext, value: &Expr| {
        closed_by_value(c, value, |c, v| c.locally_free(v, 0))
    });
    field!(matches, |_, value: &Match| Some(
        value.locally_free.is_empty()
    ));
    field!(bundles, |_, value: &Bundle| Some(
        value
            .body
            .as_ref()
            .expect("Bundle.body")
            .locally_free
            .is_empty()
    ));
    field!(unforgeables, |_, _: &GUnforgeable| Some(true));
    field!(conditionals, |c: &SpatialMatcherContext, value: &If| {
        closed_by_value(c, value, |c, v| c.locally_free(v, 0))
    });
    field!(
        cost_signed_terms,
        |c: &SpatialMatcherContext, value: &CostSignedTerm| closed_by_value(c, value, |c, v| c
            .locally_free(v, 0))
    );
    // Changed by the merge of feature/cost-accounted-rho: the context carried
    // only the meter of `cost_stack_closed_metered`, which DR-131 removed.
    // field!(cost_stacks, |c: &SpatialMatcherContext,
    //                      value: &CostStack| {
    //     cost_stack_closed(value)
    // });
    field!(cost_stacks, |_, value: &CostStack| cost_stack_closed(value));
    Some(())
}

pub(super) fn free_variable_level_borrowed(pattern: &Par) -> Option<i32> {
    free_variable_level(pattern)
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

// Changed by the merge of feature/cost-accounted-rho: the context carried only
// the meter of this step (`reserve`, `reserve_blocks_copy_and_cleanup` and
// `work`), which DR-131 removed. The step keeps the frame-stack signature.
// fn step_par_connectives(
//     context: &mut SpatialMatcherContext,
fn step_par_connectives(
    _context: &mut SpatialMatcherContext,
    mut state: ParConnectiveState,
    frames: &mut FrameStack,
) -> Job {
    let Some(plan) = state.plans.get(state.next) else {
        return start_par_fields(state, frames);
    };
    let plan = plan.clone();
    state.next += 1;
    let Some(candidates) = sub_pars(
        &state.target,
        &plan.bounds.0,
        &plan.bounds.1,
        &plan.remainder_bounds.0,
        &plan.remainder_bounds.1,
    ) else {
        return Job::Return(false);
    };
    let candidates: Vec<_> = candidates.collect();
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
    let Some(pair) = state.candidates.get(state.next) else {
        return Job::Return(false);
    };
    let (candidate, remainder) = pair.clone();
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
        // Added by the merge of feature/cost-accounted-rho (finding R3-17):
        // base and feature/cost-accounted-rho match all ten Par fields, in
        // this order. Before, the PDA dropped the last three fields, so a
        // pattern without them matched a target with them, and a remainder
        // did not bind them.
        ListRequest::new(
            std::mem::take(&mut target.conditionals)
                .into_iter()
                .map(|value| MatchValue::If(Box::new(value)))
                .collect(),
            std::mem::take(&mut pattern.conditionals)
                .into_iter()
                .map(|value| MatchValue::If(Box::new(value)))
                .collect(),
            Merger::ParConditionals,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.cost_signed_terms)
                .into_iter()
                .map(MatchValue::CostSignedTerm)
                .collect(),
            std::mem::take(&mut pattern.cost_signed_terms)
                .into_iter()
                .map(MatchValue::CostSignedTerm)
                .collect(),
            Merger::ParCostSignedTerms,
            level,
            wildcard,
        ),
        ListRequest::new(
            std::mem::take(&mut target.cost_stacks)
                .into_iter()
                .map(MatchValue::CostStack)
                .collect(),
            std::mem::take(&mut pattern.cost_stacks)
                .into_iter()
                .map(MatchValue::CostStack)
                .collect(),
            Merger::ParCostStacks,
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
        | Some(GSysAuthTokenBody(_))
        | Some(GAuthorityIdBody(_))
        | Some(GPrincipalIdBody(_)) => Job::Return(target == pattern),
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
    // Added by the merge of feature/cost-accounted-rho (finding R3-17): the
    // values of the three Par fields that the general path did not match.
    // `If` holds three `Par`s, so its variant is boxed. Inline, it would make
    // each element of each list request about half again as large.
    If(Box<If>),
    CostSignedTerm(CostSignedTerm),
    CostStack(CostStack),
}

impl MatchValue {
    // Changed by the merge of feature/cost-accounted-rho (finding R3-17): the
    // three added variants read their flags through the context.
    // fn connective_used(&self, _context: &SpatialMatcherContext) -> bool {
    fn connective_used(&self, context: &SpatialMatcherContext) -> bool {
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
            // Added by the merge of feature/cost-accounted-rho (finding
            // R3-17): the rules of base's `HasLocallyFree`, which walk a cost
            // stack's parallel tails too. These kinds can hold a connective,
            // so the arms must not answer `false` as the Bundle arm does.
            MatchValue::If(value) => context.connective_used_ref(value.as_ref()),
            MatchValue::CostSignedTerm(value) => context.connective_used_ref(value),
            MatchValue::CostStack(value) => context.connective_used_ref(value),
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
            // Added by the merge of feature/cost-accounted-rho (finding
            // R3-17): the same rules, by reference.
            MatchValue::If(value) => context.locally_free_is_empty(value.as_ref(), 0),
            MatchValue::CostSignedTerm(value) => context.locally_free_is_empty(value, 0),
            MatchValue::CostStack(value) => context.locally_free_is_empty(value, 0),
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
            // Added by the merge of feature/cost-accounted-rho (finding R3-17):
            // without these arms, the `_` arm below would make every pair of
            // the three kinds fail, with no error.
            (MatchValue::If(target), MatchValue::If(pattern)) => {
                Some(MatchPair::If(*target, *pattern))
            }
            (MatchValue::CostSignedTerm(target), MatchValue::CostSignedTerm(pattern)) => {
                Some(MatchPair::CostSignedTerm(target, pattern))
            }
            (MatchValue::CostStack(target), MatchValue::CostStack(pattern)) => {
                Some(MatchPair::CostStack(target, pattern))
            }
            _ => None,
        }
    }
}

fn expr_connective_used(expr: &Expr) -> bool {
    match expr.expr_instance.as_ref() {
        Some(
            GBool(_) | GInt(_) | GUint64(_) | GInt32(_) | GUint32(_) | GUint16(_) | GUint8(_)
            | GDouble(_) | GFloat32(_) | GBigInt(_) | GBigRat(_) | GFixedPoint(_) | GString(_)
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
    // Added by the merge of feature/cost-accounted-rho (finding R3-17).
    ParConditionals,
    ParCostSignedTerms,
    ParCostStacks,
    ESet,
    EMap,
}

impl Merger {
    fn merge_in_place(self, par: &mut Par, values: Vec<MatchValue>) -> Option<()> {
        macro_rules! typed {
            ($variant:ident) => {{
                let mut collected = Vec::new();
                for value in values {
                    collected.push(match value {
                        MatchValue::$variant(value) => value,
                        _ => panic!(concat!("list merger expected ", stringify!($variant))),
                    });
                }
                collected
            }};
        }

        match self {
            Merger::Identity => {}
            Merger::ParSends => par.sends = typed!(Send),
            Merger::ParReceives => par.receives = typed!(Receive),
            Merger::ParNews => par.news = typed!(New),
            Merger::ParExprs => par.exprs = typed!(Expr),
            Merger::ParMatches => par.matches = typed!(Match),
            Merger::ParBundles => par.bundles = typed!(Bundle),
            Merger::ParUnforgeables => par.unforgeables = typed!(Unforgeable),
            // Added by the merge of feature/cost-accounted-rho (finding R3-17).
            Merger::ParConditionals => {
                par.conditionals = typed!(If).into_iter().map(|value| *value).collect()
            }
            Merger::ParCostSignedTerms => par.cost_signed_terms = typed!(CostSignedTerm),
            Merger::ParCostStacks => par.cost_stacks = typed!(CostStack),
            // Changed by the merge of feature/cost-accounted-rho (finding R1-51): this
            // arm duplicated `merge_set_remainder`, theirs' D-D1b (DR-103) in-place
            // merge. The duplicate left that function without a production caller,
            // and `remainder_merge_matches_legacy_copy_merge` pinned only the dead
            // copy. The result is the same. Only the `Vec` capacity differs.
            // Merger::ESet => {
            //     let mut unique = Vec::new();
            //     for element in typed!(Par) {
            //         if !unique.contains(&element) {
            //             unique.push(element);
            //         }
            //     }
            //     let mut exprs = Vec::new();
            //     exprs.push(Expr {
            //         expr_instance: Some(ESetBody(ESet {
            //             ps: unique,
            //             locally_free: Vec::new(),
            //             connective_used: false,
            //             remainder: None,
            //         })),
            //     });
            //     par.exprs = exprs;
            // }
            Merger::ESet => merge_set_remainder(par, typed!(Par)),
            // Changed by the merge of feature/cost-accounted-rho (finding R1-51): this
            // arm duplicated `merge_map_remainder` (see the `ESet` arm above).
            // Merger::EMap => {
            //     let mut unique = Vec::new();
            //     for (key, value) in typed!(ParPair) {
            //         if let Some(index) = unique
            //             .iter()
            //             .position(|(prior, _): &(Par, Par)| prior == &key)
            //         {
            //             unique[index].1 = value;
            //         } else {
            //             unique.push((key, value));
            //         }
            //     }
            //     let mut kvs = Vec::new();
            //     for (key, value) in unique {
            //         kvs.push(KeyValuePair {
            //             key: Some(key),
            //             value: Some(value),
            //         });
            //     }
            //     let mut exprs = Vec::new();
            //     exprs.push(Expr {
            //         expr_instance: Some(EMapBody(EMap {
            //             kvs,
            //             locally_free: Vec::new(),
            //             connective_used: false,
            //             remainder: None,
            //         })),
            //     });
            //     par.exprs = exprs;
            // }
            Merger::EMap => merge_map_remainder(par, typed!(ParPair)),
        }
        Some(())
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

#[derive(Clone)]
enum ListPattern {
    Term(MatchValue),
    Remainder,
}

struct ListAssignment {
    pattern_index: usize,
    free_map: EdgeResult<FreeMap>,
}

struct ListSearchFrame {
    pattern_index: usize,
    next_target: usize,
    relation_cursor: RelationCursor,
    pending: Option<(usize, EdgeResult<FreeMap>)>,
}

struct ListMachine {
    targets: Arc<[MatchValue]>,
    patterns: Arc<[ListPattern]>,
    assignments: Vec<Option<ListAssignment>>,
    root_pattern: usize,
    seen_targets: Vec<bool>,
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
            if let Some(prior) = context.free_map.get_mut(&level) {
                if request
                    .merger
                    .merge_in_place(prior, request.targets)
                    .is_none()
                {
                    return Job::Return(false);
                }
            } else {
                let mut prior = vector_par(Vec::new(), false);
                if request
                    .merger
                    .merge_in_place(&mut prior, request.targets)
                    .is_none()
                {
                    return Job::Return(false);
                }
                context.free_map.insert(level, prior);
            }
            return Job::Return(true);
        }
    }

    let remainder_count = request
        .remainder
        .map(|_| target_len - pattern_len)
        .unwrap_or(0);
    let mut patterns = Vec::with_capacity(remainder_count + pattern_len);
    if let Some(level) = request.remainder {
        let _ = level;
        patterns.extend((0..remainder_count).map(|_| ListPattern::Remainder));
    }
    patterns.extend(request.patterns.into_iter().map(ListPattern::Term));
    let relation = LazyRelation::new(
        patterns
            .iter()
            .map(|pattern| matches!(pattern, ListPattern::Term(_))),
    );

    let mut machine = ListMachine {
        targets: request.targets.into(),
        assignments: (0..target_len).map(|_| None).collect(),
        patterns: patterns.into(),
        root_pattern: 0,
        seen_targets: vec![false; target_len],
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
    fn push_search(&mut self, pattern_index: usize) {
        #[cfg(test)]
        update_list_relation_stats(|stats| {
            stats.augmenting_frames += 1;
            if self.relation.is_cacheable(pattern_index)
                && self.relation.has_scanned_targets(pattern_index)
            {
                stats.relation_row_reuses += 1;
            }
        });
        self.search.push(ListSearchFrame {
            pattern_index,
            next_target: 0,
            relation_cursor: self.relation.cursor(pattern_index),
            pending: None,
        });
    }

    fn start_next_root(&mut self) {
        if self.root_pattern < self.patterns.len() {
            self.seen_targets.fill(false);
            self.push_search(self.root_pattern);
        }
    }

    fn accept_edge(mut self, target_index: usize, free_map: EdgeResult<FreeMap>) -> Job {
        self.seen_targets[target_index] = true;
        let frame = self
            .search
            .last_mut()
            .expect("an accepted list edge has a search frame");
        match self.assignments[target_index].as_ref() {
            None => {
                self.assignments[target_index] = Some(ListAssignment {
                    pattern_index: frame.pattern_index,
                    free_map,
                });
                self.search.pop();
                while let Some(mut parent) = self.search.pop() {
                    let (parent_target, parent_map) = parent
                        .pending
                        .take()
                        .expect("an augmenting list parent has one displaced edge");
                    self.assignments[parent_target] = Some(ListAssignment {
                        pattern_index: parent.pattern_index,
                        free_map: parent_map,
                    });
                }
                self.root_pattern += 1;
                self.start_next_root();
                Job::List(Box::new(self))
            }
            Some(previous) => {
                let displaced = previous.pattern_index;
                frame.pending = Some((target_index, free_map));
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
        let Some(pattern_index) = machine.search.last().map(|frame| frame.pattern_index) else {
            return Job::Return(false);
        };

        let cacheable = machine.relation.is_cacheable(pattern_index);
        let target_index = if cacheable {
            loop {
                let cached = {
                    let frame = machine
                        .search
                        .last_mut()
                        .expect("the relational list row has a search frame");
                    machine
                        .relation
                        .next_cached(pattern_index, &mut frame.relation_cursor)
                };
                match cached {
                    Some((target_index, produced)) if !machine.seen_targets[target_index] => {
                        #[cfg(test)]
                        update_list_relation_stats(|stats| stats.cached_edge_visits += 1);
                        return machine.accept_edge(target_index, EdgeResult::Shared(produced));
                    }
                    Some(_) => {
                        #[cfg(test)]
                        update_list_relation_stats(|stats| stats.cached_edge_visits += 1);
                        continue;
                    }
                    None => break,
                }
            }

            let Some(target_index) = machine
                .relation
                .next_unscanned(pattern_index, machine.targets.len())
            else {
                machine.search.pop();
                if let Some(parent) = machine.search.last_mut() {
                    parent.pending = None;
                    continue;
                }
                return Job::Return(false);
            };
            target_index
        } else {
            let frame = machine
                .search
                .last_mut()
                .expect("the non-cacheable list row has a search frame");
            if frame.next_target == machine.targets.len() {
                machine.search.pop();
                if let Some(parent) = machine.search.last_mut() {
                    parent.pending = None;
                    continue;
                }
                return Job::Return(false);
            }
            let target_index = frame.next_target;
            frame.next_target += 1;
            if machine.seen_targets[target_index] {
                continue;
            }
            target_index
        };

        let candidate_was_seen = machine.seen_targets[target_index];
        #[cfg(test)]
        update_list_relation_stats(|stats| stats.edge_evaluations += 1);
        match &machine.patterns[pattern_index] {
            ListPattern::Remainder => {
                if machine.targets[target_index].locally_free_is_empty(context) {
                    #[cfg(test)]
                    update_list_relation_stats(|stats| {
                        stats.successful_edge_evaluations += 1;
                    });
                    let produced = new_free_map();
                    return machine.accept_edge(target_index, EdgeResult::Owned(produced));
                }
            }
            ListPattern::Term(pattern) if !pattern.connective_used(context) => {
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
                        return machine.accept_edge(target_index, EdgeResult::Shared(produced));
                    }
                }
            }
            ListPattern::Term(pattern) => {
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
}

// Disabled by the merge of feature/cost-accounted-rho: `finish_list` again
// keeps the positional remainder of CBR-052 (406506646), so this value
// closure has no caller. It gave a remainder every target equal to a
// selected one, which `par_remainder_preserves_the_exact_multiplicity_of_identical_sends`
// refuses.
// /// Legacy projection law: retain every original occurrence whose VALUE is
// /// equal to one of the index-distinct targets assigned to a remainder row.
// /// This runs only after the complete relation has been selected; it never
// /// prunes an alternative edge during search.
// fn project_remainder_value_closed(
//     _context: &SpatialMatcherContext,
//     targets: &[MatchValue],
//     remainder_target_indices: &[bool],
// ) -> Option<Vec<MatchValue>> {
//     if targets.len() != remainder_target_indices.len() {
//         return None;
//     }
//     let mut projected = Vec::new();
//     for target in targets {
//         let mut selected = false;
//         for (matched_index, matched) in targets.iter().enumerate() {
//             if !remainder_target_indices[matched_index] {
//                 continue;
//             }
//             if target == matched {
//                 selected = true;
//                 break;
//             }
//         }
//         if selected {
//             projected.push(target.clone());
//         }
//     }
//     Some(projected)
// }

fn finish_list(context: &mut SpatialMatcherContext, machine: ListMachine) -> Job {
    let ListMachine {
        targets,
        patterns,
        assignments,
        mut relation,
        merger,
        remainder,
        wildcard,
        ..
    } = machine;
    relation.clear();

    let mut ordered: Vec<_> = assignments
        .into_iter()
        .enumerate()
        .filter_map(|(target_index, assignment)| {
            assignment.map(|assignment| (target_index, assignment))
        })
        .collect();
    ordered.sort_by(|(left_index, _), (right_index, _)| {
        match targets[*left_index].cmp(&targets[*right_index]) {
            Ordering::Equal => left_index.cmp(right_index),
            order => order,
        }
    });
    let mut remainder_target_indices = vec![false; targets.len()];
    for (target_index, assignment) in &ordered {
        if matches!(patterns[assignment.pattern_index], ListPattern::Remainder) {
            remainder_target_indices[*target_index] = true;
        }
    }
    // Changed by the merge of feature/cost-accounted-rho: the checkpoint merge
    // aed9f4427 replaced the positional remainder of 406506646 (CBR-052) with
    // a value closure, so one duplicate that the remainder selected expanded
    // to every equal target. The remainder again holds only the positions that
    // the assignment gives to the remainder row.
    // let Some(remainder_sorted) =
    //     project_remainder_value_closed(context, &targets, &remainder_target_indices)
    // else {
    //     return Job::Return(false);
    // };
    let remainder_sorted = targets
        .iter()
        .enumerate()
        .filter(|(target_index, _)| remainder_target_indices[*target_index])
        .map(|(_, target)| target)
        .cloned()
        .collect::<Vec<_>>();
    let maps = ordered
        .into_iter()
        .map(|(_, assignment)| assignment.free_map.into_owned())
        .collect::<Vec<_>>();
    let Some(mut updated) = aggregate_updates(context.free_map.clone(), maps) else {
        return Job::Return(false);
    };

    match remainder {
        None if wildcard || remainder_sorted.is_empty() => {
            context.free_map = updated;
            Job::Return(true)
        }
        None => {
            context.free_map = updated;
            Job::Return(false)
        }
        Some(level) => {
            if let Some(prior) = updated.get_mut(&level) {
                if merger.merge_in_place(prior, remainder_sorted).is_none() {
                    return Job::Return(false);
                }
            } else {
                let mut prior = vector_par(Vec::new(), false);
                if merger
                    .merge_in_place(&mut prior, remainder_sorted)
                    .is_none()
                {
                    return Job::Return(false);
                }
                updated.insert(level, prior);
            }
            context.free_map = updated;
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
    if let PathRemainder::Free(level) = machine.remainder {
        let bound = updated
            .get(&level)
            .cloned()
            .unwrap_or_default()
            .with_exprs(vec![Expr {
                expr_instance: Some(EPathmapBody(remainder_map)),
            }]);
        let mut updated = updated;
        updated.insert(level, bound);
        context.free_map = updated;
    } else {
        context.free_map = updated;
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
    use models::rhoapi::cost_signature::Value as CostValue;
    use models::rhoapi::var::VarInstance;

    use super::*;

    fn free_pattern(level: i32) -> Par {
        let mut pattern = Par::default();
        pattern.connective_used = true;
        pattern.exprs.push(Expr {
            expr_instance: Some(EVarBody(EVar {
                v: Some(Var {
                    var_instance: Some(VarInstance::FreeVar(level)),
                }),
            })),
        });
        pattern
    }

    #[test]
    fn nested_cost_stack_tail_is_not_missed_by_free_variable_binding() {
        let mut target = Par::default();
        target.cost_stacks.push(CostStack {
            cells: Vec::new(),
            parallel_tail: vec![CostStack {
                cells: vec![CostSignature {
                    value: Some(CostValue::BoundLevel(0)),
                }],
                parallel_tail: Vec::new(),
            }],
        });
        let mut owned = SpatialMatcherContext::new();
        assert!(matches!(
            bind_free_variable_owned(&mut owned, target.clone(), 3),
            Job::Return(false)
        ));
        assert!(owned
            .free_map
            .get(&3)
            .is_some_and(|par| par.cost_stacks.is_empty()));
        let mut borrowed = SpatialMatcherContext::new();
        assert!(bind_free_variable_borrowed(&mut borrowed, &target, 3).is_none());
        assert!(borrowed
            .free_map
            .get(&3)
            .is_some_and(|par| par.cost_stacks.is_empty()));

        let mut quoted = Par::default();
        quoted.connective_used = true;
        let nested_connective = CostStack {
            cells: Vec::new(),
            parallel_tail: vec![CostStack {
                cells: vec![CostSignature {
                    value: Some(CostValue::Quote(quoted)),
                }],
                parallel_tail: Vec::new(),
            }],
        };
        assert!(owned.connective_used(nested_connective));

        let mut closed = Par::default();
        closed.cost_stacks.push(CostStack {
            cells: Vec::new(),
            parallel_tail: vec![CostStack::default()],
        });
        let mut owned = SpatialMatcherContext::new();
        let mut borrowed = SpatialMatcherContext::new();
        assert!(matches!(
            bind_free_variable_owned(&mut owned, closed.clone(), 3),
            Job::Return(true)
        ));
        assert!(bind_free_variable_borrowed(&mut borrowed, &closed, 3).is_some());
        assert_eq!(owned.free_map, borrowed.free_map);
        assert_eq!(free_variable_level(&free_pattern(3)), Some(3));
    }

    // Disabled by the merge of feature/cost-accounted-rho: this property pins
    // the value closure of the checkpoint merge aed9f4427, which `finish_list`
    // no longer uses. The positional rule of CBR-052 is pinned by
    // `par_remainder_preserves_the_exact_multiplicity_of_identical_sends` and
    // by the "Par remainder over byte-identical sends" corpus case.
    // proptest::proptest! {
    //     #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]
    //
    //     #[test]
    //     fn remainder_projection_is_value_closure_with_original_multiplicity(
    //         source in proptest::collection::vec((proptest::prelude::any::<i8>(), proptest::prelude::any::<bool>()), 0..24),
    //     ) {
    //         let targets: Vec<_> = source.iter().map(|(value, _)| {
    //             MatchValue::Expr(Expr { expr_instance: Some(GInt(i64::from(*value))) })
    //         }).collect();
    //         let selected: Vec<_> = source.iter().map(|(_, selected)| *selected).collect();
    //         let expected: Vec<_> = source.iter()
    //             .filter(|(value, _)| source.iter().any(|(candidate, selected)| *selected && candidate == value))
    //             .map(|(value, _)| i64::from(*value))
    //             .collect();
    //         let context = SpatialMatcherContext::new();
    //         let actual = project_remainder_value_closed(&context, &targets, &selected)
    //             .expect("unmetered projection")
    //             .into_iter()
    //             .map(|value| match value {
    //                 MatchValue::Expr(Expr { expr_instance: Some(GInt(number)) }) => number,
    //                 _ => panic!("numeric projection changed variant"),
    //             })
    //             .collect::<Vec<_>>();
    //         proptest::prop_assert_eq!(actual, expected);
    //     }
    // }

    #[test]
    fn sized_numeric_grounds_never_mark_connectives() {
        for instance in [
            GUint64(u64::MAX),
            GInt32(i32::MIN),
            GUint32(u32::MAX),
            GUint16(u16::MAX.into()),
            GUint8(u8::MAX.into()),
        ] {
            let expr = Expr {
                expr_instance: Some(instance),
            };
            assert!(!expr_connective_used(&expr));
        }
    }

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
        // Added by the merge of feature/cost-accounted-rho (finding R3-17):
        // the boxed `If` variant keeps a list value no larger than a receive,
        // which holds two `Par`s and was the largest variant before the
        // change. An inline `If` holds three `Par`s.
        assert!(
            std::mem::size_of::<MatchValue>() <= std::mem::size_of::<Receive>() + 8,
            "MatchValue grew to {} bytes",
            std::mem::size_of::<MatchValue>()
        );
    }

    // Added by the merge of feature/cost-accounted-rho: that branch tests the
    // free-variable fast path (D-D3, DR-105) and the by-reference reads (D-D2,
    // DR-104) on its recursive matcher. The PDA has both optimizations, so
    // these ports test the same claims on the PDA entries. The general path is
    // `drive_job(context, evaluate_par(..))`. It skips the fast-path check of
    // `evaluate_pair`, as the `LEGACY_FREE_VARIABLE_PATH` flag of that branch
    // did.

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
        Some(models::par_from_default! {
            locally_free: locally(open),
            ..Par::default()
        })
    }

    fn open_or_closed_expr(open: bool) -> Expr {
        if open {
            new_boundvar_expr(0)
        } else {
            new_gint_expr(3)
        }
    }

    /// A cost stack that is closed, open in a root cell, or open only in a
    /// parallel tail. F1R3Lang's `CostStack` nests through its tails, so the
    /// port adds the third shape to the two shapes of the original test.
    fn cost_stack(shape: u8) -> CostStack {
        let open_cell = || CostSignature {
            value: Some(CostValue::BoundLevel(0)),
        };
        match shape {
            0 => CostStack {
                cells: Vec::new(),
                parallel_tail: Vec::new(),
            },
            1 => CostStack {
                cells: vec![open_cell()],
                parallel_tail: Vec::new(),
            },
            _ => CostStack {
                cells: Vec::new(),
                parallel_tail: vec![CostStack {
                    cells: vec![open_cell()],
                    parallel_tail: Vec::new(),
                }],
            },
        }
    }

    /// A target whose ten fields hold closed or open elements, with a random
    /// shell. An open element has a locally free variable.
    fn free_variable_target() -> impl proptest::strategy::Strategy<Value = Par> {
        use proptest::strategy::Strategy;

        let field = || proptest::collection::vec(proptest::prelude::any::<bool>(), 0..3);
        (
            (field(), field(), field(), field(), field()),
            (field(), field(), field(), field()),
            proptest::collection::vec(0u8..3, 0..3),
            proptest::prelude::any::<bool>(),
            proptest::collection::vec(0u8..2, 0..3),
        )
            .prop_map(
                |(
                    (sends, receives, news, exprs, matches),
                    (bundles, unforgeables, conditionals, signed),
                    stacks,
                    connective_used,
                    locally_free,
                )| {
                    models::par_from_default! {
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
                        cost_stacks: stacks.into_iter().map(cost_stack).collect(),
                        locally_free,
                        connective_used,
                        ..Par::default()
                    }
                },
            )
    }

    /// A stored binding with a shell that the merge never writes.
    fn shell_binding() -> Par {
        models::par_from_default! {
            exprs: vec![new_gint_expr(9)],
            locally_free: vec![1, 0],
            connective_used: true,
            connectives: vec![Connective::default()],
            ..Par::default()
        }
    }

    /// The owned entry, with the fast path or with the general path.
    fn run_owned(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
        general: bool,
    ) -> (Option<()>, FreeMap) {
        let mut context = SpatialMatcherContext::new();
        context.free_map = free_map.clone();
        let result = if general {
            drive_job(&mut context, evaluate_par(target.clone(), pattern.clone()))
        } else {
            match_par(&mut context, target.clone(), pattern.clone())
        };
        (result, context.free_map)
    }

    /// The borrowed entry. It has only the fast path.
    fn run_borrowed(
        target: &Par,
        pattern: &Par,
        free_map: &FreeMap,
    ) -> (Option<Vec<Par>>, FreeMap) {
        use super::super::fold_match::FoldMatch;

        let mut context = SpatialMatcherContext::new();
        context.free_map = free_map.clone();
        let result = context.fold_match(
            std::slice::from_ref(target),
            std::slice::from_ref(pattern),
            None,
        );
        (result, context.free_map)
    }

    /// Ported from feature/cost-accounted-rho (D-D3, DR-105): the predicate
    /// accepts exactly one free variable.
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

    /// Ported from feature/cost-accounted-rho (D-D3, DR-105): a field with a
    /// locally free element stops the binding, and the fields before it stay
    /// merged, as on the general path. The Par/Par order puts bundles before
    /// unforgeables.
    #[test]
    fn free_variable_failure_leaves_the_general_partial_binding() {
        let level = 1;
        let pattern = free_variable(level, Vec::new());
        let target = models::par_from_default! {
            sends: vec![Send::default()],
            exprs: vec![new_gint_expr(4)],
            bundles: vec![Bundle {
                body: body(true),
                ..Default::default()
            }],
            unforgeables: vec![GUnforgeable::default()],
            ..Par::default()
        };
        let mut expected = vector_par(Vec::new(), false);
        expected.sends = target.sends.clone();
        expected.exprs = target.exprs.clone();
        let expected_map: FreeMap = [(level, expected)].into_iter().collect();
        for general in [true, false] {
            let (result, free_map) = run_owned(&target, &pattern, &FreeMap::new(), general);
            assert!(result.is_none());
            assert_eq!(free_map, expected_map, "owned, general {general}");
        }
        let (result, free_map) = run_borrowed(&target, &pattern, &FreeMap::new());
        assert!(result.is_none());
        assert_eq!(free_map, expected_map, "borrowed");
        let first_open = models::par_from_default! {
            sends: vec![Send {
                locally_free: vec![1],
                ..Default::default()
            }],
            exprs: vec![new_gint_expr(4)],
            ..Par::default()
        };
        let existing: FreeMap = [(level, shell_binding())].into_iter().collect();
        for general in [true, false] {
            assert_eq!(
                run_owned(&first_open, &pattern, &existing, general),
                (None, existing.clone()),
                "owned, general {general}"
            );
        }
        assert_eq!(
            run_borrowed(&first_open, &pattern, &existing),
            (None, existing.clone()),
            "borrowed"
        );
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// Ported from feature/cost-accounted-rho (D-D3, DR-105): on the owned
        /// and the borrowed entry, the fast path for a free-variable pattern
        /// gives the result and the free map of the general path. This also
        /// holds when a field has a locally free element, and with or without
        /// a stored binding at the level. The borrowed entry has no general
        /// path of its own, so the test compares it with the owned general
        /// path, as the original test does.
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
                free_map.insert(level + 1 + offset as i32, models::par_from_default! {
                    exprs: vec![new_gint_expr(value)],
                    ..Par::default()
                });
            }
            if existing {
                free_map.insert(level, shell_binding());
            }
            let general = run_owned(&target, &pattern, &free_map, true);
            proptest::prop_assert_eq!(run_owned(&target, &pattern, &free_map, false), general.clone());
            let borrowed = run_borrowed(&target, &pattern, &free_map);
            proptest::prop_assert_eq!(&borrowed.1, &general.1);
            proptest::prop_assert_eq!(borrowed.0.is_some(), general.0.is_some());
        }

        /// Ported from feature/cost-accounted-rho (D-D2, DR-104): a fold that
        /// compares ground pairs by reference gives the result and the free map
        /// of the walk that copies every pair into the owned entry.
        #[test]
        fn fold_match_by_reference_equals_cloned_pair_walk(
            targets in proptest::collection::vec((0i64..3, 1usize..3), 0..5),
            patterns in proptest::collection::vec((proptest::prelude::any::<bool>(), 0i64..3, 1usize..3), 0..5),
        ) {
            use super::super::fold_match::FoldMatch;

            let targets: Vec<Par> = targets
                .into_iter()
                .map(|(value, size)| models::par_from_default! {
                    exprs: vec![new_gint_expr(value); size],
                    ..Par::default()
                })
                .collect();
            let patterns: Vec<Par> = patterns
                .into_iter()
                .enumerate()
                .map(|(level, (free, value, size))| {
                    if free {
                        free_variable(level as i32, Vec::new())
                    } else {
                        models::par_from_default! {
                            exprs: vec![new_gint_expr(value); size],
                            ..Par::default()
                        }
                    }
                })
                .collect();
            let mut by_reference = SpatialMatcherContext::new();
            let folded = by_reference.fold_match(&targets, &patterns, None);
            let mut copied = SpatialMatcherContext::new();
            let walked = (|| {
                for (target, pattern) in targets.iter().zip(&patterns) {
                    match_par(&mut copied, target.clone(), pattern.clone())?;
                }
                (targets.len() == patterns.len()).then(Vec::new)
            })();
            proptest::prop_assert_eq!(folded, walked);
            proptest::prop_assert_eq!(by_reference.free_map, copied.free_map);
        }
    }

    fn ground_par(size: usize) -> Par {
        models::par_from_default! {
            exprs: vec![new_gint_expr(7); size],
            ..Par::default()
        }
    }

    fn large_string_par(length: usize) -> Par {
        models::par_from_default! {
            exprs: vec![new_gstring_expr("s".repeat(length))],
            ..Par::default()
        }
    }

    /// Ported from feature/cost-accounted-rho, where DR-131 kept the check
    /// without its charge: a ground pattern matches only an equal target, on
    /// the borrowed entry and on the owned entry.
    #[test]
    fn ground_comparison_matches_only_an_equal_target() {
        let pattern = ground_par(3);
        for size in [1usize, 3, 4_096] {
            let mut borrowed = SpatialMatcherContext::new();
            assert_eq!(
                borrowed
                    .spatial_match_par_ref(&ground_par(size), &pattern)
                    .is_some(),
                size == 3,
                "borrowed {size}"
            );
            let mut owned = SpatialMatcherContext::new();
            assert_eq!(
                match_par(&mut owned, ground_par(size), pattern.clone()).is_some(),
                size == 3,
                "owned {size}"
            );
        }
    }

    /// Ported from feature/cost-accounted-rho, where DR-131 kept the checks
    /// without their charges. A string connective matches a target whose only
    /// expression is a string. It does not match a target with two
    /// expressions. A variable-reference connective does not match.
    #[test]
    fn scalar_connective_tests_only_a_single_expression() {
        let target = large_string_par(4_096);
        let string = Connective {
            connective_instance: Some(ConnString(true)),
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(
            match_connective(&mut context, target.clone(), string.clone()),
            Some(())
        );
        let two = models::par_from_default! {
            exprs: vec![new_gstring_expr("a".to_owned()); 2],
            ..Par::default()
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(match_connective(&mut context, two, string), None);
        let reference = Connective {
            connective_instance: Some(VarRefBody(VarRef::default())),
        };
        let mut context = SpatialMatcherContext::new();
        assert_eq!(match_connective(&mut context, target, reference), None);
    }

    /// Ported from feature/cost-accounted-rho (D-D2, DR-104): the in-place
    /// filter allocates nothing. The merge keeps the filter `retain_no_frees`
    /// in `models/src/rust/utils.rs`, where `MatcherReadsByReference.v` binds
    /// it, but its tests lived in the recursive matcher that the explicit
    /// frame stack replaces (name audit, plan step 1.7).
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

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// Ported from feature/cost-accounted-rho (D-D2, DR-104): the in-place
        /// filter keeps the expressions that `no_frees_exprs` keeps, in the
        /// same order.
        #[test]
        fn retained_no_frees_equals_cloned_filter(
            exprs in proptest::collection::vec(free_or_ground_expr(), 0..12),
        ) {
            let expected = no_frees_exprs(&exprs);
            let mut retained = exprs;
            retain_no_frees(&mut retained);
            proptest::prop_assert_eq!(retained, expected);
        }
    }

    // Added by the merge of feature/cost-accounted-rho (finding R3-17): the
    // general path matches the conditionals, the cost-signed terms and the
    // cost stacks. Base, dev and feature/cost-accounted-rho match an element
    // of these kinds only against an equal element.

    fn ground_signature() -> CostSignature {
        CostSignature {
            value: Some(CostValue::Ground(b"g".to_vec())),
        }
    }

    /// For each kind: its name, a Par that holds one closed term of the kind,
    /// and a Par that holds one term of the kind with a free variable inside.
    fn closed_and_open_terms() -> [(&'static str, Par, Par); 3] {
        let one = || new_gint_par(1, Vec::new(), false);
        let two = || new_gint_par(2, Vec::new(), false);
        let free = || new_freevar_par(0, Vec::new());
        let conditional = |condition: Par| If {
            connective_used: condition.connective_used,
            condition: Some(condition),
            if_true: Some(one()),
            if_false: Some(two()),
            locally_free: Vec::new(),
        };
        let signed = |body: Par| CostSignedTerm {
            body: Some(body),
            signature: Some(ground_signature()),
        };
        let stack = |cell: CostSignature| CostStack {
            cells: vec![cell],
            parallel_tail: Vec::new(),
        };
        [
            (
                "conditional",
                models::par_from_default! {
                    conditionals: vec![conditional(new_gbool_par(true, Vec::new(), false))],
                    ..Par::default()
                },
                models::par_from_default! {
                    conditionals: vec![conditional(free())],
                    connective_used: true,
                    ..Par::default()
                },
            ),
            (
                "cost-signed term",
                models::par_from_default! {
                    cost_signed_terms: vec![signed(one())],
                    ..Par::default()
                },
                models::par_from_default! {
                    cost_signed_terms: vec![signed(free())],
                    connective_used: true,
                    ..Par::default()
                },
            ),
            (
                "cost stack",
                models::par_from_default! {
                    cost_stacks: vec![stack(ground_signature())],
                    ..Par::default()
                },
                models::par_from_default! {
                    cost_stacks: vec![stack(CostSignature {
                        value: Some(CostValue::Quote(free())),
                    })],
                    connective_used: true,
                    ..Par::default()
                },
            ),
        ]
    }

    /// `par` with the ground expression `1` added.
    fn with_one(par: &Par) -> Par {
        let mut par = par.clone();
        par.exprs.push(new_gint_expr(1));
        par
    }

    /// `par` with the free variable at level 0 added.
    fn with_free_variable(par: &Par) -> Par {
        let mut par = par.clone();
        par.exprs.push(new_freevar_expr(0));
        par.connective_used = true;
        par
    }

    fn run_general_path(target: Par, pattern: Par) -> (Option<()>, FreeMap) {
        let mut context = SpatialMatcherContext::new();
        let result = match_par(&mut context, target, pattern);
        (result, context.free_map)
    }

    /// Test (1): the pattern `{x | 1}` against `{1 | T}` binds `x` to `T`.
    #[test]
    fn general_path_remainder_binds_conditional_and_cost_terms() {
        for (kind, closed, _) in closed_and_open_terms() {
            let pattern = with_free_variable(&with_one(&Par::default()));
            let expected: FreeMap = [(0, closed.clone())].into_iter().collect();
            assert_eq!(
                run_general_path(with_one(&closed), pattern),
                (Some(()), expected),
                "{kind}"
            );
        }
    }

    /// Test (2): a pattern that holds `T` and a free variable does not match
    /// a target without `T`.
    #[test]
    fn general_path_requires_the_conditional_and_cost_terms_of_the_pattern() {
        for (kind, closed, _) in closed_and_open_terms() {
            let target = with_one(&Par::default());
            assert_eq!(
                run_general_path(target, with_free_variable(&closed)).0,
                None,
                "{kind}"
            );
        }
    }

    /// Test (3): a pattern with `connective_used` and no remainder does not
    /// match a target with one extra `T`. Without the extra `T`, it matches.
    #[test]
    fn general_path_without_a_remainder_refuses_an_extra_conditional_or_cost_term() {
        let seven = new_gint_par(7, Vec::new(), false);
        let pattern = new_elist_par(
            vec![new_freevar_par(0, Vec::new())],
            Vec::new(),
            true,
            None,
            Vec::new(),
            true,
        );
        let target = new_elist_par(
            vec![seven.clone()],
            Vec::new(),
            false,
            None,
            Vec::new(),
            false,
        );
        let expected: FreeMap = [(0, seven)].into_iter().collect();
        assert_eq!(
            run_general_path(target.clone(), pattern.clone()),
            (Some(()), expected)
        );
        for (kind, closed, _) in closed_and_open_terms() {
            let mut extra = target.clone();
            extra.conditionals = closed.conditionals.clone();
            extra.cost_signed_terms = closed.cost_signed_terms.clone();
            extra.cost_stacks = closed.cost_stacks.clone();
            assert_eq!(run_general_path(extra, pattern.clone()).0, None, "{kind}");
        }
    }

    /// Test (4): an element of the three kinds matches only an equal element,
    /// as base's rule gives. A pattern term of the kind of `T`, with a free
    /// variable inside, therefore does not bind against `T`. A pattern term
    /// equal to `T` matches `T`.
    #[test]
    fn general_path_matches_conditional_and_cost_terms_by_equality() {
        for (kind, closed, open) in closed_and_open_terms() {
            assert_eq!(
                run_general_path(closed.clone(), open).0,
                None,
                "{kind} with a free variable inside"
            );
            let expected: FreeMap = [(0, with_one(&Par::default()))].into_iter().collect();
            assert_eq!(
                run_general_path(with_one(&closed), with_free_variable(&closed)),
                (Some(()), expected),
                "{kind} equal to the target"
            );
        }
    }

    /// Test (5): `{_ | 1}` matches `{1 | T}`.
    #[test]
    fn general_path_wildcard_absorbs_conditional_and_cost_terms() {
        let mut pattern = with_one(&Par::default());
        pattern.exprs.push(new_wildcard_expr());
        pattern.connective_used = true;
        for (kind, closed, _) in closed_and_open_terms() {
            assert_eq!(
                run_general_path(with_one(&closed), pattern.clone()),
                (Some(()), FreeMap::new()),
                "{kind}"
            );
        }
    }

    // Added by the merge of feature/cost-accounted-rho (finding R1-35): this
    // property replaces the value-closure property that is commented out
    // above. A Par remainder keeps exactly the targets that the pattern terms
    // leave (CBR-052, `MatcherRemainderClosure.positions_partition_the_targets`).
    fn integer_par(values: &[i64]) -> Par {
        let mut par = Par::default();
        par.exprs = values.iter().map(|value| new_gint_expr(*value)).collect();
        par
    }

    fn is_subsequence(kept: &[i64], original: &[i64]) -> bool {
        let mut rest = original.iter();
        kept.iter()
            .all(|value| rest.any(|candidate| candidate == value))
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(256))]

        /// The pattern holds the values at the taken positions and a free
        /// variable. The remainder must hold the other values, with their
        /// multiplicity and in target order. Few distinct values make
        /// duplicates common, so a pattern term and the remainder often
        /// split equal targets. The value closure fails on such a split.
        #[test]
        fn par_remainder_binds_the_unmatched_occurrences_in_target_order(
            cells in proptest::collection::vec(
                (0i64..3, proptest::prelude::any::<bool>()),
                0..10,
            ),
        ) {
            let values: Vec<i64> = cells.iter().map(|(value, _)| *value).collect();
            let taken: Vec<i64> = cells
                .iter()
                .filter(|(_, take)| *take)
                .map(|(value, _)| *value)
                .collect();
            let mut expected: Vec<i64> = cells
                .iter()
                .filter(|(_, take)| !*take)
                .map(|(value, _)| *value)
                .collect();
            let pattern = with_free_variable(&integer_par(&taken));
            let (result, free_map) = run_general_path(integer_par(&values), pattern);
            proptest::prop_assert_eq!(result, Some(()));
            let remainder = free_map.get(&0).expect("the remainder binds level 0");
            let mut bound: Vec<i64> = remainder
                .exprs
                .iter()
                .map(|expr| match expr.expr_instance {
                    Some(GInt(value)) => value,
                    _ => panic!("the remainder holds only the target integers"),
                })
                .collect();
            proptest::prop_assert!(
                is_subsequence(&bound, &values),
                "the remainder {:?} is not in the order of the targets {:?}",
                bound,
                values
            );
            bound.sort_unstable();
            expected.sort_unstable();
            proptest::prop_assert_eq!(bound, expected);
        }
    }
}
