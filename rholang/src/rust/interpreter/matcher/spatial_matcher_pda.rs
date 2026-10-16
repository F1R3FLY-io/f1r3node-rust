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
use super::has_locally_free::HasLocallyFree;
#[cfg(test)]
use super::lazy_relation::RelationalMatchStats;
use super::lazy_relation::{EdgeResult, LazyRelation, RelationCursor};
use super::list_match::aggregate_updates_metered;
use super::match_pars::match_pars;
use super::par_count::ParCount;
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

impl MatchPair {
    fn reserve_inspections(&self, context: &SpatialMatcherContext) -> Option<()> {
        macro_rules! inspect {
            ($target:expr, $pattern:expr) => {{
                context.inspect_blocks($target)?;
                context.inspect_blocks($pattern)
            }};
        }
        match self {
            Self::Par(target, pattern) => inspect!(target, pattern),
            Self::ParPair(target, pattern) => inspect!(target, pattern),
            Self::Connective(target, pattern) => inspect!(target, pattern),
            Self::Bundle(target, pattern) => inspect!(target, pattern),
            Self::Send(target, pattern) => inspect!(target, pattern),
            Self::Receive(target, pattern) => inspect!(target, pattern),
            Self::New(target, pattern) => inspect!(target, pattern),
            Self::Expr(target, pattern) => inspect!(target, pattern),
            Self::Match(target, pattern) => inspect!(target, pattern),
            Self::Unforgeable(target, pattern) => inspect!(target, pattern),
            Self::ReceiveBind(target, pattern) => inspect!(target, pattern),
            Self::MatchCase(target, pattern) => inspect!(target, pattern),
            Self::If(target, pattern) => inspect!(target, pattern),
            Self::CostSignedTerm(target, pattern) => inspect!(target, pattern),
            Self::CostStack(target, pattern) => inspect!(target, pattern),
        }
    }
}

fn drive(context: &mut SpatialMatcherContext, root: MatchPair) -> Option<()> {
    drive_job(context, match_job(root))
}

fn drive_job(context: &mut SpatialMatcherContext, mut job: Job) -> Option<()> {
    let mut frames = FrameStack::new();

    loop {
        job = match job {
            Job::Match(pair) => {
                pair.reserve_inspections(context)?;
                evaluate_pair(context, *pair, &mut frames)
            }
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
                if context
                    .reserve_free_map_insert(context.free_map.len())
                    .is_none()
                {
                    return Job::Return(false);
                }
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
                for _ in 0..produced.len() {
                    if context
                        .reserve_free_map_search(context.free_map.len())
                        .is_none()
                    {
                        return Job::Return(false);
                    }
                }
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

fn cost_stack_closed_metered(context: &SpatialMatcherContext, root: &CostStack) -> Option<bool> {
    use models::rhoapi::cost_signature::Value;
    enum Pending<'a> {
        Stack(&'a CostStack),
        Signature(&'a CostSignature),
    }
    let mut pending = Vec::new();
    context.reserve_vec(&mut pending, 1)?;
    pending.push(Pending::Stack(root));
    while let Some(next) = pending.pop() {
        context.reserve(1, 0, 0)?;
        match next {
            Pending::Stack(stack) => {
                let Some(children) = stack.cells.len().checked_add(stack.parallel_tail.len())
                else {
                    return context
                        .reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
                };
                context.reserve_vec(&mut pending, children)?;
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
                    context.reserve_vec(&mut pending, compound.elements.len())?;
                    pending.extend(compound.elements.iter().map(Pending::Signature));
                }
                _ => {}
            },
        }
    }
    Some(true)
}

fn bind_field_owned<T: Clone + shared::rust::clone_backing::CloneBacking>(
    context: &mut SpatialMatcherContext,
    values: Vec<T>,
    level: i32,
    closed: impl Fn(&SpatialMatcherContext, &T) -> Option<bool>,
    assign: impl Fn(&mut Par, Vec<T>),
) -> Option<()> {
    for value in &values {
        context.inspect_blocks(value)?;
        if !closed(context, value)? {
            return None;
        }
    }
    context.reserve_free_map_search(context.free_map.len())?;
    if let Some(bound) = context.free_map.get_mut(&level) {
        assign(bound, values);
    } else {
        context.reserve_free_map_insert(context.free_map.len())?;
        let mut bound = Par::default();
        assign(&mut bound, values);
        context.free_map.insert(level, bound);
    }
    Some(())
}

fn bind_field_borrowed<T: Clone + shared::rust::clone_backing::CloneBacking>(
    context: &mut SpatialMatcherContext,
    values: &[T],
    level: i32,
    closed: impl Fn(&SpatialMatcherContext, &T) -> Option<bool>,
    assign: impl Fn(&mut Par, Vec<T>),
) -> Option<()> {
    for value in values {
        context.inspect_blocks(value)?;
        if !closed(context, value)? {
            return None;
        }
    }
    context.reserve_blocks_slice_copy_and_cleanup(values)?;
    bind_field_owned(context, values.to_vec(), level, closed, assign)
}

fn closed_by_value<T: Clone>(
    context: &SpatialMatcherContext,
    value: &T,
    check: impl Fn(&SpatialMatcherContext, T) -> Vec<u8>,
) -> Option<bool>
where
    T: shared::rust::clone_backing::CloneBacking,
{
    context.reserve_blocks_copy_and_cleanup(value)?;
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
    field!(cost_stacks, |c: &SpatialMatcherContext,
                         value: &CostStack| {
        cost_stack_closed_metered(c, value)
    });
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
    field!(cost_stacks, |c: &SpatialMatcherContext,
                         value: &CostStack| {
        cost_stack_closed_metered(c, value)
    });
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

fn step_par_connectives(
    context: &mut SpatialMatcherContext,
    mut state: ParConnectiveState,
    frames: &mut FrameStack,
) -> Job {
    let Some(plan) = state.plans.get(state.next) else {
        return start_par_fields(state, frames);
    };
    if context.reserve(1, 0, 0).is_none()
        || context
            .reserve_blocks_copy_and_cleanup(&plan.connective)
            .is_none()
    {
        return Job::Return(false);
    }
    let plan = plan.clone();
    state.next += 1;
    let Some(work) = context.work() else {
        return Job::Return(false);
    };
    let Some(candidates) = sub_pars(
        &state.target,
        &plan.bounds.0,
        &plan.bounds.1,
        &plan.remainder_bounds.0,
        &plan.remainder_bounds.1,
        work,
    ) else {
        return Job::Return(false);
    };
    let Some(candidates): Option<Vec<_>> = candidates.collect() else {
        return Job::Return(false);
    };
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
    if context.reserve_blocks_copy_and_cleanup(&pair.0).is_none()
        || context.reserve_blocks_copy_and_cleanup(&pair.1).is_none()
    {
        return Job::Return(false);
    }
    let (candidate, remainder) = pair.clone();
    state.next += 1;
    if !state.free_map_is_unreachable
        && context
            .reserve_blocks_copy_and_cleanup(&context.free_map)
            .is_none()
    {
        return Job::Return(false);
    }
    let snapshot = (!state.free_map_is_unreachable).then(|| context.free_map.clone());
    if context
        .reserve_blocks_copy_and_cleanup(&state.connective)
        .is_none()
    {
        return Job::Return(false);
    }
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
}

impl MatchValue {
    fn inspect(&self, context: &SpatialMatcherContext) -> Option<()> {
        match self {
            MatchValue::Par(value) => context.inspect_blocks(value),
            MatchValue::ParPair(value) => context.inspect_blocks(value),
            MatchValue::Send(value) => context.inspect_blocks(value),
            MatchValue::Receive(value) => context.inspect_blocks(value),
            MatchValue::New(value) => context.inspect_blocks(value),
            MatchValue::Expr(value) => context.inspect_blocks(value),
            MatchValue::Match(value) => context.inspect_blocks(value),
            MatchValue::Bundle(value) => context.inspect_blocks(value),
            MatchValue::Unforgeable(value) => context.inspect_blocks(value),
            MatchValue::ReceiveBind(value) => context.inspect_blocks(value),
        }
    }

    fn reserve_copy(&self, context: &SpatialMatcherContext) -> Option<()> {
        match self {
            MatchValue::Par(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::ParPair(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Send(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Receive(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::New(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Expr(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Match(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Bundle(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::Unforgeable(value) => context.reserve_blocks_copy_and_cleanup(value),
            MatchValue::ReceiveBind(value) => context.reserve_blocks_copy_and_cleanup(value),
        }
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
    ESet,
    EMap,
}

impl Merger {
    /// Prepare a complete replacement field before changing an existing
    /// binding. No metering refusal can observe a partially assigned Par.
    fn merge_in_place(
        self,
        par: &mut Par,
        values: Vec<MatchValue>,
        work: &super::spatial_matcher::MatcherWork<'_>,
    ) -> Option<()> {
        macro_rules! typed {
            ($variant:ident) => {{
                let mut collected = Vec::new();
                work.reserve_vec(&mut collected, values.len())?;
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
            Merger::ESet => {
                let mut unique = Vec::new();
                for element in typed!(Par) {
                    work.inspect_blocks(&element)?;
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
                par.exprs = exprs;
            }
            Merger::EMap => {
                let mut unique = Vec::new();
                for (key, value) in typed!(ParPair) {
                    work.inspect_blocks(&key)?;
                    work.inspect_blocks(&unique)?;
                    if let Some(index) = unique
                        .iter()
                        .position(|(prior, _): &(Par, Par)| prior == &key)
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
                par.exprs = exprs;
            }
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
            if request
                .targets
                .iter()
                .any(|value| value.reserve_copy(context).is_none())
            {
                return Job::Return(false);
            }
            if !request
                .targets
                .iter()
                .all(|value| value.locally_free_is_empty(context))
            {
                return Job::Return(false);
            }
            let Some(work) = context.work() else {
                return Job::Return(false);
            };
            if work
                .reserve_free_map_search(context.free_map.len())
                .is_none()
            {
                return Job::Return(false);
            }
            if let Some(prior) = context.free_map.get_mut(&level) {
                if request
                    .merger
                    .merge_in_place(prior, request.targets, &work)
                    .is_none()
                {
                    return Job::Return(false);
                }
            } else {
                let mut prior = vector_par(Vec::new(), false);
                if request
                    .merger
                    .merge_in_place(&mut prior, request.targets, &work)
                    .is_none()
                    || work
                        .reserve_free_map_insert(context.free_map.len())
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

/// Legacy projection law: retain every original occurrence whose VALUE is
/// equal to one of the index-distinct targets assigned to a remainder row.
/// This runs only after the complete relation has been selected; it never
/// prunes an alternative edge during search.
fn project_remainder_value_closed(
    context: &SpatialMatcherContext,
    targets: &[MatchValue],
    remainder_target_indices: &[bool],
) -> Option<Vec<MatchValue>> {
    if targets.len() != remainder_target_indices.len() {
        return context.reject(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected);
    }
    let mut projected = Vec::new();
    for target in targets {
        let mut selected = false;
        for (matched_index, matched) in targets.iter().enumerate() {
            if !remainder_target_indices[matched_index] {
                continue;
            }
            context.reserve(1, 0, 0)?;
            target.inspect(context)?;
            matched.inspect(context)?;
            if target == matched {
                selected = true;
                break;
            }
        }
        if selected {
            target.reserve_copy(context)?;
            context.reserve_vec(&mut projected, 1)?;
            projected.push(target.clone());
        }
    }
    Some(projected)
}

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
    let Some(remainder_sorted) =
        project_remainder_value_closed(context, &targets, &remainder_target_indices)
    else {
        return Job::Return(false);
    };
    let maps = ordered
        .into_iter()
        .map(|(_, assignment)| assignment.free_map.into_owned())
        .collect::<Vec<_>>();
    let Some(work) = context.work() else {
        return Job::Return(false);
    };
    if context
        .reserve_blocks_copy_and_cleanup(&context.free_map)
        .is_none()
    {
        return Job::Return(false);
    }
    let Some(mut updated) = aggregate_updates_metered(context.free_map.clone(), maps, &work) else {
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
            if work.reserve_free_map_search(updated.len()).is_none() {
                return Job::Return(false);
            }
            if let Some(prior) = updated.get_mut(&level) {
                if merger
                    .merge_in_place(prior, remainder_sorted, &work)
                    .is_none()
                {
                    return Job::Return(false);
                }
            } else {
                let mut prior = vector_par(Vec::new(), false);
                if merger
                    .merge_in_place(&mut prior, remainder_sorted, &work)
                    .is_none()
                    || work.reserve_free_map_insert(updated.len()).is_none()
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

    let Some(work) = context.work() else {
        return Job::Return(false);
    };
    if context
        .reserve_blocks_copy_and_cleanup(&context.free_map)
        .is_none()
    {
        return Job::Return(false);
    }
    let Some(updated) = aggregate_updates_metered(context.free_map.clone(), maps, &work) else {
        return Job::Return(false);
    };
    if let PathRemainder::Free(level) = machine.remainder {
        if context.reserve_free_map_search(updated.len()).is_none()
            || context.reserve_free_map_insert(updated.len()).is_none()
        {
            return Job::Return(false);
        }
        if let Some(existing) = updated.get(&level) {
            if context.reserve_blocks_copy_and_cleanup(existing).is_none() {
                return Job::Return(false);
            }
        }
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

    #[test]
    fn in_place_remainder_refusal_preserves_existing_binding() {
        let calls = std::cell::Cell::new(0usize);
        let reject = |_: usize, _: usize, _: usize| {
            let next = calls.get() + 1;
            calls.set(next);
            if next <= 2 {
                Ok(())
            } else {
                Err(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected)
            }
        };
        let context = SpatialMatcherContext::with_meter(&reject).expect("context construction");
        let mut prior = Par::default();
        prior.exprs.push(Expr {
            expr_instance: Some(GInt(7)),
        });
        let snapshot = prior.clone();
        let work = context.work().expect("work lease");
        assert!(Merger::ESet
            .merge_in_place(&mut prior, vec![MatchValue::Par(Par::default())], &work,)
            .is_none());
        assert_eq!(prior, snapshot);
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(64))]

        #[test]
        fn remainder_projection_is_value_closure_with_original_multiplicity(
            source in proptest::collection::vec((proptest::prelude::any::<i8>(), proptest::prelude::any::<bool>()), 0..24),
        ) {
            let targets: Vec<_> = source.iter().map(|(value, _)| {
                MatchValue::Expr(Expr { expr_instance: Some(GInt(i64::from(*value))) })
            }).collect();
            let selected: Vec<_> = source.iter().map(|(_, selected)| *selected).collect();
            let expected: Vec<_> = source.iter()
                .filter(|(value, _)| source.iter().any(|(candidate, selected)| *selected && candidate == value))
                .map(|(value, _)| i64::from(*value))
                .collect();
            let context = SpatialMatcherContext::new();
            let actual = project_remainder_value_closed(&context, &targets, &selected)
                .expect("unmetered projection")
                .into_iter()
                .map(|value| match value {
                    MatchValue::Expr(Expr { expr_instance: Some(GInt(number)) }) => number,
                    _ => panic!("numeric projection changed variant"),
                })
                .collect::<Vec<_>>();
            proptest::prop_assert_eq!(actual, expected);
        }
    }

    #[test]
    fn remainder_projection_refuses_before_partial_publication() {
        let calls = std::cell::Cell::new(0usize);
        let meter = |_: usize, _: usize, _: usize| {
            let next = calls.get() + 1;
            calls.set(next);
            if next == 1 {
                Ok(())
            } else {
                Err(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected)
            }
        };
        let context = SpatialMatcherContext::with_meter(&meter).expect("context reserve");
        let targets = vec![MatchValue::Expr(Expr {
            expr_instance: Some(GInt(7)),
        })];
        let before = context.free_map.clone();
        assert!(project_remainder_value_closed(&context, &targets, &[true]).is_none());
        assert_eq!(context.free_map, before);
        assert_eq!(
            context.take_error(),
            Some(rspace_plus_plus::rspace::errors::RSpaceError::HostWorkRejected)
        );
    }

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
    }
}
