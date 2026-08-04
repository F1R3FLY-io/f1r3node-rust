//! Stack-safe spatial matching machine.
//!
//! Spatial matching is one mutually recursive semantic operation even though
//! the source-level entry points are split across `Par`, `Expr`, `Send`,
//! `Receive`, connectives, ordered folds, unordered collection matching and
//! EPathMap. This module defunctionalizes that whole call graph into one
//! `Job`/`Frame` pushdown automaton. A nested pattern therefore grows heap work
//! storage, never the native call stack.

use std::cmp::Ordering;
use std::sync::Arc;

use models::rhoapi::expr::ExprInstance;
use models::rust::canonical_path::decode_trie_path;
use models::rust::epathmap_trie_codec::EPathMapMode;
use models::rust::par_map_type_mapper::ParMapTypeMapper;
use models::rust::par_set_type_mapper::ParSetTypeMapper;
use models::rust::rholang::implicits::{single_expr, vector_par};
use models::rust::utils::*;

use super::exports::*;
use super::has_locally_free::HasLocallyFree;
use super::list_match::aggregate_updates;
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
    Match(MatchPair),
    ParConnectives(ParConnectiveState),
    ConnectiveCandidates(ConnectiveCandidates),
    ListInit(ListRequest),
    List(ListMachine),
    PathInit(EPathMap, EPathMap),
    Path(PathMachine),
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
        target: Par,
        patterns: Arc<[Par]>,
        next: usize,
        snapshot: FreeMap,
    },
    Not {
        snapshot: FreeMap,
    },
    BindOnSuccess {
        level: i32,
        value: Par,
    },
    ConnectiveCandidate {
        state: ConnectiveCandidates,
        snapshot: Option<FreeMap>,
        remainder: Par,
    },
    ListEdge {
        machine: ListMachine,
        target_index: usize,
        snapshot: FreeMap,
    },
    PathEdge {
        machine: PathMachine,
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
    let mut job = Job::Match(root);
    let mut frames = FrameStack::new();

    loop {
        job = match job {
            Job::Match(pair) => evaluate_pair(context, pair, &mut frames),
            Job::ParConnectives(state) => step_par_connectives(state, &mut frames),
            Job::ConnectiveCandidates(state) => {
                step_connective_candidates(context, state, &mut frames)
            }
            Job::ListInit(request) => init_list(context, request, &mut frames),
            Job::List(machine) => step_list(context, machine, &mut frames),
            Job::PathInit(target, pattern) => init_path(target, pattern),
            Job::Path(machine) => step_path(context, machine, &mut frames),
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
                start_or_branch(context, target, patterns, next, frames)
            }
        }
        Frame::Not { snapshot } => {
            context.free_map = snapshot;
            Job::Return(!result)
        }
        Frame::BindOnSuccess { level, value } => {
            if result {
                context.free_map.insert(level, value);
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
                parent.target = remainder;
                Job::ParConnectives(parent)
            } else {
                Job::ConnectiveCandidates(state)
            }
        }
        Frame::ListEdge {
            machine,
            target_index,
            snapshot,
        } => {
            let produced = std::mem::replace(&mut context.free_map, snapshot);
            if result {
                machine.accept_edge(target_index, produced)
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
                machine.accept_edge(target_key, produced)
            } else {
                Job::Path(machine)
            }
        }
    }
}

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
    pairs.into_iter().map(Job::Match).collect()
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

    Job::ParConnectives(ParConnectiveState {
        target,
        pattern,
        plans: plans.into(),
        next: 0,
        remainder_level,
        wildcard,
    })
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
    Job::ConnectiveCandidates(ConnectiveCandidates {
        parent: state,
        connective: plan.connective,
        candidates,
        next: 0,
        free_map_is_unreachable,
    })
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
        state,
        snapshot,
        remainder,
    });
    Job::Match(MatchPair::Connective(candidate, connective))
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
                    .map(|pattern| Job::Match(MatchPair::Par(target.clone(), pattern)))
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
            Job::Match(MatchPair::Par(target, pattern))
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
        target: target.clone(),
        patterns,
        next: next + 1,
        snapshot,
    });
    Job::Match(MatchPair::Par(target, pattern))
}

fn evaluate_send(target: Send, pattern: Send, frames: &mut FrameStack) -> Job {
    if target.persistent != pattern.persistent {
        return Job::Return(false);
    }
    let mut jobs = vec![Job::Match(MatchPair::Par(
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
            Job::Match(MatchPair::Par(
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
        Job::Match(MatchPair::Par(
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
            value: new_elist_par(surplus, Vec::new(), false, None, Vec::new(), false),
        });
    }

    Some(
        targets
            .into_iter()
            .take(matched)
            .zip(patterns)
            .map(|(target, pattern)| Job::Match(MatchPair::Par(target, pattern)))
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
        (Some(EPathmapBody(target)), Some(EPathmapBody(pattern))) => Job::PathInit(target, pattern),
        (Some(EVarBody(EVar { v: target })), Some(EVarBody(EVar { v: pattern }))) => {
            Job::Return(target == pattern)
        }
        (Some(ENotBody(ENot { p: target })), Some(ENotBody(ENot { p: pattern }))) => {
            Job::Match(MatchPair::Par(
                target.expect("ENot.p (target)"),
                pattern.expect("ENot.p (pattern)"),
            ))
        }
        (Some(ENegBody(ENeg { p: target })), Some(ENegBody(ENeg { p: pattern }))) => {
            Job::Match(MatchPair::Par(
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
            let mut jobs = vec![Job::Match(MatchPair::Par(
                target.target.expect("EMethod.target (target)"),
                pattern.target.expect("EMethod.target (pattern)"),
            ))];
            jobs.extend(
                target
                    .arguments
                    .into_iter()
                    .zip(pattern.arguments)
                    .map(|(target, pattern)| Job::Match(MatchPair::Par(target, pattern))),
            );
            start_jobs(jobs, frames)
        }
        (Some(EMatchesBody(target)), Some(EMatchesBody(pattern))) => {
            if target.pattern != pattern.pattern {
                Job::Return(false)
            } else {
                Job::Match(MatchPair::Par(
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
            Job::Match(MatchPair::Par(
                target1.unwrap_or_else(|| panic!("{name}.p1 (target)")),
                pattern1.unwrap_or_else(|| panic!("{name}.p1 (pattern)")),
            )),
            Job::Match(MatchPair::Par(
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
    let mut jobs = vec![Job::Match(MatchPair::Par(
        target.target.expect("Match.target (target)"),
        pattern.target.expect("Match.target (pattern)"),
    ))];
    jobs.extend(
        target
            .cases
            .into_iter()
            .zip(pattern.cases)
            .map(|(target, pattern)| Job::Match(MatchPair::MatchCase(target, pattern))),
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
        Job::Match(MatchPair::Par(
            target.source.expect("ReceiveBind.source (target)"),
            pattern.source.expect("ReceiveBind.source (pattern)"),
        ))
    }
}

fn evaluate_match_case(target: MatchCase, pattern: MatchCase) -> Job {
    if target.pattern != pattern.pattern {
        Job::Return(false)
    } else {
        Job::Match(MatchPair::Par(
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

#[derive(Clone)]
enum ListPattern {
    Term(MatchValue),
    Remainder,
}

struct ListAssignment {
    pattern_index: usize,
    free_map: FreeMap,
}

struct ListSearchFrame {
    pattern_index: usize,
    next_target: usize,
    pending: Option<(usize, FreeMap)>,
}

struct ListMachine {
    targets: Arc<[MatchValue]>,
    patterns: Arc<[ListPattern]>,
    assignments: Vec<Option<ListAssignment>>,
    root_pattern: usize,
    seen_targets: Vec<bool>,
    search: Vec<ListSearchFrame>,
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
        return Job::Match(pair);
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

    let mut machine = ListMachine {
        targets: request.targets.into(),
        assignments: (0..target_len).map(|_| None).collect(),
        patterns: patterns.into(),
        root_pattern: 0,
        seen_targets: vec![false; target_len],
        search: Vec::new(),
        merger: request.merger,
        remainder: request.remainder,
        wildcard: request.wildcard,
    };
    machine.start_next_root();
    Job::List(machine)
}

impl ListMachine {
    fn start_next_root(&mut self) {
        if self.root_pattern < self.patterns.len() {
            self.seen_targets.fill(false);
            self.search.push(ListSearchFrame {
                pattern_index: self.root_pattern,
                next_target: 0,
                pending: None,
            });
        }
    }

    fn accept_edge(mut self, target_index: usize, free_map: FreeMap) -> Job {
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
                Job::List(self)
            }
            Some(previous) => {
                let displaced = previous.pattern_index;
                frame.pending = Some((target_index, free_map));
                self.search.push(ListSearchFrame {
                    pattern_index: displaced,
                    next_target: 0,
                    pending: None,
                });
                Job::List(self)
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
        let Some(frame) = machine.search.last_mut() else {
            return Job::Return(false);
        };
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
        let pattern = machine.patterns[frame.pattern_index].clone();
        let target = machine.targets[target_index].clone();
        match pattern {
            ListPattern::Remainder => {
                if target.locally_free_is_empty(context) {
                    let produced = context.free_map.clone();
                    return machine.accept_edge(target_index, produced);
                }
            }
            ListPattern::Term(pattern) if !pattern.connective_used(context) => {
                if target == pattern {
                    let produced = context.free_map.clone();
                    return machine.accept_edge(target_index, produced);
                }
            }
            ListPattern::Term(pattern) => {
                let Some(pair) = target.into_pair(pattern) else {
                    continue;
                };
                let snapshot = context.free_map.clone();
                frames.push(Frame::ListEdge {
                    machine,
                    target_index,
                    snapshot,
                });
                return Job::Match(pair);
            }
        }
    }
}

fn finish_list(context: &mut SpatialMatcherContext, machine: ListMachine) -> Job {
    let mut ordered: Vec<_> = machine
        .assignments
        .iter()
        .enumerate()
        .filter_map(|(target_index, assignment)| {
            assignment
                .as_ref()
                .map(|assignment| (target_index, assignment))
        })
        .collect();
    ordered.sort_by(|(left_index, _), (right_index, _)| {
        match machine.targets[*left_index].cmp(&machine.targets[*right_index]) {
            Ordering::Equal => left_index.cmp(right_index),
            order => order,
        }
    });
    let maps = ordered
        .iter()
        .map(|(_, assignment)| assignment.free_map.clone())
        .collect::<Vec<_>>();
    let Some(updated) = aggregate_updates(context.free_map.clone(), maps) else {
        return Job::Return(false);
    };

    let remainder_values = ordered
        .iter()
        .filter_map(|(target_index, assignment)| {
            matches!(
                machine.patterns[assignment.pattern_index],
                ListPattern::Remainder
            )
            .then(|| machine.targets[*target_index].clone())
        })
        .collect::<Vec<_>>();
    let remainder_sorted = machine
        .targets
        .iter()
        .filter(|target| remainder_values.contains(target))
        .cloned()
        .collect::<Vec<_>>();
    context.free_map = updated;

    match machine.remainder {
        None if machine.wildcard || remainder_sorted.is_empty() => Job::Return(true),
        None => Job::Return(false),
        Some(level) => {
            let prior = context
                .free_map
                .get(&level)
                .cloned()
                .unwrap_or_else(|| vector_par(Vec::new(), false));
            context
                .free_map
                .insert(level, machine.merger.merge(prior, remainder_sorted));
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

fn init_path(target: EPathMap, pattern: EPathMap) -> Job {
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
    Job::Path(machine)
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
                Job::Path(self)
            }
            Some(displaced) => {
                frame.pending = Some((target_key, free_map));
                self.search.push(PathSearchFrame {
                    pattern: displaced,
                    next_target: Vec::new(),
                    pending: None,
                });
                Job::Path(self)
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
                    machine,
                    target_key,
                    snapshot,
                });
                return Job::Match(MatchPair::Par(target, pattern.as_ref().clone()));
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
                    machine,
                    target_key,
                    snapshot,
                });
                return Job::Match(MatchPair::ParPair((key, value), pattern.as_ref().clone()));
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
