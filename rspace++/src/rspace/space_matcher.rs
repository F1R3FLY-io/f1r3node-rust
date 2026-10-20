// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;
use std::sync::Arc;

use serde::Serialize;

use super::r#match::{Match, PreparedCommit};
use crate::rspace::candidate_order::order_candidates_with_index;
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::hot_store::HotStore;
use crate::rspace::internal::{ConsumeCandidate, Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_MATCHER_GUARD_BACKTRACK_METRIC,
    RSPACE_MATCHER_SPATIAL_BACKTRACK_METRIC, RSPACE_METRICS_SOURCE,
};
use crate::rspace::serializers::serializers::CandidateOrderingBytes;

type MatchingDataCandidate<C, A> = (ConsumeCandidate<C, A>, Vec<(Datum<A>, i32)>);

struct CandidateWalkFrame<'a, C, P, A: Clone> {
    channel: &'a C,
    pattern: &'a P,
    pool: Vec<(Datum<A>, i32)>,
    level: usize,
    cursor: usize,
    any_leaf_reached: bool,
    is_last_bind: bool,
}

#[derive(Clone, Copy)]
struct CandidateWalkResult {
    stopped: bool,
    any_leaf_reached: bool,
}

/// Why a subtree of the candidate search failed — and therefore WHICH of the
/// two incompletenesses the search is repairing when it backtracks past it.
///
/// The two are worth separating because their blast radii differ sharply:
/// a `where` guard is new syntax, so [`SelectionOutcome::GuardRejected`]
/// backtracking can only change the behaviour of a program that uses guards,
/// whereas [`SelectionOutcome::NoSpatialMatch`] backtracking changes the
/// behaviour of any multi-bind receive whose earlier bind can swallow the only
/// datum a later bind could have matched — ordinary, guard-free Rholang. Both
/// are the same defect (an enabled COMM left unfired), and the search that
/// repairs one repairs the other; the distinction is carried here so it can be
/// counted, reviewed and reasoned about separately rather than discovered.
///
/// Crate-visible because it appears in the signature of a method of the
/// (crate-private module's) public `SpaceMatcher` trait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionOutcome {
    /// A complete selection was found and the commit guard accepted it.
    Admissible,
    /// Complete selections were reached, and the commit guard refused every one
    /// of them. THIS is defect D1's case.
    GuardRejected,
    /// No complete selection was reachable at all: some bind had no spatial
    /// match left in its pool, so the guard was never consulted.
    NoSpatialMatch,
}

/// Run the candidate-selection pushdown automaton shared by the committing
/// selector and the read-only enabled-rendezvous enumerator.
///
/// One heap frame represents one receive bind. Native-stack usage is constant
/// in receive arity; the explicit frame stack grows linearly with the number
/// of binds. Returning `true` from `on_admissible` stops at the current leaf
/// and deliberately preserves `chosen` plus the residual pools, which is the
/// committing selector's contract. Returning `false` continues the traversal;
/// exhaustive completion restores both structures to their entry state.
fn walk_candidate_selections<S, C, P, A, K, OnAdmissible, OnBacktrack>(
    space: &S,
    matcher: &Box<dyn Match<P, A, K>>,
    channel_pattern_pairs: &[(&C, &P)],
    continuation: &K,
    channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    level: usize,
    chosen: &mut Vec<ConsumeCandidate<C, A>>,
    mut on_admissible: OnAdmissible,
    mut on_backtrack: OnBacktrack,
) -> CandidateWalkResult
where
    S: SpaceMatcher<C, P, A, K> + ?Sized,
    C: Clone + std::hash::Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
    OnAdmissible: FnMut(&[ConsumeCandidate<C, A>], PreparedCommit) -> bool,
    OnBacktrack: FnMut(SelectionOutcome),
{
    enum State {
        Descend(usize),
        Scan,
        Return(CandidateWalkResult),
    }

    let mut frames: Vec<CandidateWalkFrame<'_, C, P, A>> = Vec::new();
    let mut state = State::Descend(level);
    loop {
        match state {
            State::Descend(current_level) => {
                if current_level == channel_pattern_pairs.len() {
                    let matched: Vec<&A> =
                        chosen.iter().map(|candidate| &*candidate.datum.a).collect();
                    let stopped = matcher
                        .prepare_commit(continuation, &matched)
                        .is_some_and(|permit| on_admissible(chosen, permit));
                    state = State::Return(CandidateWalkResult {
                        stopped,
                        any_leaf_reached: true,
                    });
                    continue;
                }

                let (channel, pattern) = channel_pattern_pairs[current_level];
                let Some(pool) = channel_to_indexed_data.get(channel).cloned() else {
                    state = State::Return(CandidateWalkResult {
                        stopped: false,
                        any_leaf_reached: false,
                    });
                    continue;
                };
                frames.push(CandidateWalkFrame {
                    channel,
                    pattern,
                    pool,
                    level: current_level,
                    cursor: 0,
                    any_leaf_reached: false,
                    is_last_bind: current_level + 1 == channel_pattern_pairs.len(),
                });
                state = State::Scan;
            }
            State::Scan => {
                let next = {
                    let frame = frames.last().expect("candidate walk scan without a frame");
                    space.next_spatial_match(
                        matcher,
                        frame.channel.clone(),
                        &frame.pool,
                        frame.pattern,
                        frame.cursor,
                    )
                };
                if let Some((position, candidate)) = next {
                    let frame = frames
                        .last_mut()
                        .expect("candidate walk scan without a frame");
                    frame.cursor = position + 1;
                    if !frame.is_last_bind {
                        let residual = <S as SpaceMatcher<C, P, A, K>>::residual_pool(
                            &frame.pool,
                            position,
                            candidate.datum.persist,
                        );
                        channel_to_indexed_data.insert(frame.channel.clone(), residual);
                    }
                    chosen.push(candidate);
                    state = State::Descend(frame.level + 1);
                } else {
                    let frame = frames.pop().expect("candidate walk scan without a frame");
                    if !frame.is_last_bind {
                        channel_to_indexed_data.insert(frame.channel.clone(), frame.pool);
                    }
                    state = State::Return(CandidateWalkResult {
                        stopped: false,
                        any_leaf_reached: frame.any_leaf_reached,
                    });
                }
            }
            State::Return(result) => {
                if result.stopped {
                    return result;
                }
                let Some(parent) = frames.last_mut() else {
                    return result;
                };
                chosen
                    .pop()
                    .expect("candidate walk returned without a parent choice");
                let outcome = if result.any_leaf_reached {
                    SelectionOutcome::GuardRejected
                } else {
                    SelectionOutcome::NoSpatialMatch
                };
                on_backtrack(outcome);
                parent.any_leaf_reached |= result.any_leaf_reached;
                state = State::Scan;
            }
        }
    }
}

fn order_repeated_channels<C, P, A>(
    channel_pattern_pairs: &[(&C, &P)],
    channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
) where
    C: Clone + std::hash::Hash + Eq,
    A: Clone,
{
    let compare = |(left, left_index): &(Datum<A>, i32), (right, right_index): &(Datum<A>, i32)| {
        (*left_index != -1)
            .cmp(&(*right_index != -1))
            .then_with(|| left.source.hash.cmp(&right.source.hash))
    };
    for (position, &(channel, _)) in channel_pattern_pairs.iter().enumerate() {
        let first = !channel_pattern_pairs[..position]
            .iter()
            .any(|(earlier, _)| *earlier == channel);
        let repeated = channel_pattern_pairs[position + 1..]
            .iter()
            .any(|(later, _)| *later == channel);
        if !(first && repeated) {
            continue;
        }
        if let Some(data) = channel_to_indexed_data.get_mut(channel) {
            if data
                .windows(2)
                .any(|pair| compare(&pair[0], &pair[1]) == std::cmp::Ordering::Greater)
            {
                rollback.push((channel.clone(), data.clone()));
                data.sort_by(compare);
            }
        }
    }
}

pub trait SpaceMatcher<C, P, A, K>
where
    C: Clone + std::hash::Hash + Eq + Send + Sync,
    P: Clone + Send + Sync,
    A: Clone + Send + Sync,
    K: Clone + Send + Sync,
{
    /// The first datum of `data` that matches `pattern` **spatially**, with the
    /// pool the remaining binds would then see.
    ///
    /// ⚠ The commit guard (`Match::check_commit`) is deliberately NOT consulted
    /// here, and cannot be: a guard is a predicate over the bindings of EVERY
    /// bind of the receive, and this function sees exactly one channel. Guard
    /// participation in candidate *selection* is the job of
    /// [`SpaceMatcher::extract_prepared_data_candidates`], which drives
    /// [`SpaceMatcher::next_spatial_match`] directly so it can enumerate a
    /// channel's spatial matches one at a time.
    fn find_matching_data_candidate(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel: C,
        data: &[(Datum<A>, i32)],
        pattern: &P,
    ) -> Option<MatchingDataCandidate<C, A>> {
        self.next_spatial_match(matcher, channel, data, pattern, 0)
            .map(|(position, candidate)| {
                let residual = Self::residual_pool(data, position, candidate.datum.persist);
                (candidate, residual)
            })
    }

    /// The scan alone, resumable: the first datum of `data` at or after `start`
    /// that matches `pattern` spatially, as a [`ConsumeCandidate`], with the
    /// POSITION it was found at so a caller can resume at `position + 1`.
    ///
    /// The returned position indexes `data`; it is NOT the datum's store index
    /// (that rides along inside the candidate as `datum_index`, and for the
    /// datum a `produce` injects into its own pool it is `-1`).
    ///
    /// The residual pool is deliberately NOT built here. A search that TRIES
    /// candidates needs a residual only for the ones it descends through;
    /// building one per candidate EXAMINED made an exhausted single-bind scan
    /// quadratic (a fresh `n-1` element pool each time, every element a `Datum`
    /// whose `Produce` source carries two owned hashes) where the scan itself
    /// is linear — measured at 197 ms → 17 ms for a 1000-datum exhaustive
    /// scan.
    fn next_spatial_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel: C,
        data: &[(Datum<A>, i32)],
        pattern: &P,
        start: usize,
    ) -> Option<(usize, ConsumeCandidate<C, A>)> {
        if start >= data.len() {
            return None;
        }
        for (offset, (datum, data_index)) in data[start..].iter().enumerate() {
            let idx = start + offset;
            metrics::counter!("rspace.matcher.get_calls", "source" => "rspace").increment(1);
            // The matcher borrows the pattern and the Arc-shared datum
            // payload — a failing attempt copies nothing at this boundary
            // (the earlier value-shaped path made one full pattern clone plus
            // one full payload clone per candidate per attempt; the
            // `rspace.matcher.clone_ns` timer that measured them is retired
            // with the clones).
            let t_match = std::time::Instant::now();
            let match_result = matcher.get(pattern, &datum.a);
            metrics::counter!("rspace.matcher.fold_match_ns", "source" => "rspace")
                .increment(t_match.elapsed().as_nanos() as u64);

            if let Some(mat) = match_result {
                return Some((idx, ConsumeCandidate {
                    channel,
                    datum: Datum {
                        // The MATCHED payload (possibly bind-transformed
                        // by the matcher) — a fresh value, Arc'd once.
                        a: std::sync::Arc::new(mat),
                        persist: datum.persist,
                        source: datum.source.clone(),
                    },
                    // The stored payload as removed — an Arc bump
                    // (before Arc-shaped transport: a deep copy per candidate).
                    removed_datum: std::sync::Arc::clone(&datum.a),
                    datum_index: *data_index,
                }));
            }
        }
        None
    }

    /// The pool the REMAINING binds see once the datum at `position` has been
    /// speculatively taken: `data` without it, or the whole of `data` when the
    /// datum persists (a persistent datum satisfies any number of binds).
    fn residual_pool(
        data: &[(Datum<A>, i32)],
        position: usize,
        persist: bool,
    ) -> Vec<(Datum<A>, i32)> {
        match persist {
            true => data.to_vec(),
            false => {
                let mut remaining = Vec::with_capacity(data.len() - 1);
                remaining.extend_from_slice(&data[..position]);
                remaining.extend_from_slice(&data[position + 1..]);
                remaining
            }
        }
    }

    /// Attempts to match all channel-pattern pairs against the data map.
    /// Records mutations in `rollback` so the caller can undo them on failure.
    ///
    /// The pairs are borrowed (`(&C, &P)`) — callers zip references
    /// into the channels/patterns they already hold instead of cloning a
    /// pair list per candidate continuation (extract_first_match previously
    /// cloned every channel and every pattern per candidate iterated).
    fn extract_data_candidates_rollback(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        rollback: &mut Vec<(C, Vec<(Datum<A>, i32)>)>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
        order_repeated_channels(channel_pattern_pairs, channel_to_indexed_data, rollback);
        let mut acc = Vec::with_capacity(channel_pattern_pairs.len());

        for &(channel, pattern) in channel_pattern_pairs {
            let maybe_tuple: Option<MatchingDataCandidate<C, A>> =
                match channel_to_indexed_data.get(channel) {
                    Some(indexed_data) => self.find_matching_data_candidate(
                        matcher,
                        channel.clone(),
                        indexed_data,
                        pattern,
                    ),
                    None => None,
                };

            match maybe_tuple {
                Some((cand, rem)) => {
                    acc.push(Some(cand));
                    // Save the original before mutating
                    if let Some(original) = channel_to_indexed_data.get(channel) {
                        rollback.push((channel.clone(), original.clone()));
                    }
                    channel_to_indexed_data.insert(channel.clone(), rem);
                }
                None => {
                    acc.push(None);
                }
            }
        }

        acc
    }

    /// Non-rollback version. Spatial matching ONLY — the commit guard is not
    /// consulted, so this may only be used where a guard cannot apply.
    ///
    /// The one remaining production caller is the `install` path
    /// (`locked_install_internal` in both spaces), a startup-time assertion
    /// that no datum is waiting for a system continuation. It answers a purely
    /// spatial question ("is anything here?"), it rejects any match rather than
    /// firing a COMM, and system continuations carry no guard. Every path that
    /// can actually FIRE a COMM goes through
    /// [`SpaceMatcher::extract_prepared_data_candidates`] instead.
    fn extract_data_candidates(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Vec<Option<ConsumeCandidate<C, A>>> {
        let mut rollback = Vec::new();
        self.extract_data_candidates_rollback(
            matcher,
            channel_pattern_pairs,
            channel_to_indexed_data,
            &mut rollback,
        )
    }

    // Disabled by the merge of feature/cost-accounted-rho (lint fix, finding
    // RT-08): no code calls this method, at HEAD or after the merge. Every path
    // that can fire a COMM calls `extract_prepared_data_candidates` directly.
    // fn extract_guarded_data_candidates(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channel_pattern_pairs: &[(&C, &P)],
    //     continuation: &K,
    //     channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    // ) -> Option<Vec<ConsumeCandidate<C, A>>> {
    //     self.extract_prepared_data_candidates(
    //         matcher,
    //         channel_pattern_pairs,
    //         continuation,
    //         channel_to_indexed_data,
    //     )
    //     .map(|(chosen, _)| chosen)
    // }

    fn extract_prepared_data_candidates(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        continuation: &K,
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Option<(Vec<ConsumeCandidate<C, A>>, PreparedCommit)> {
        // DR-129 E1 orders each repeated channel before the complete search
        // (defect D1). On failure the walker restores the ordered pools, and
        // the rollback then restores the arrival order (risk R3).
        let mut rollback = Vec::new();
        order_repeated_channels(channel_pattern_pairs, channel_to_indexed_data, &mut rollback);
        let mut chosen: Vec<ConsumeCandidate<C, A>> =
            Vec::with_capacity(channel_pattern_pairs.len());
        let mut permit = None;

        match self.search_prepared_candidate_selection(
            matcher,
            channel_pattern_pairs,
            continuation,
            channel_to_indexed_data,
            0,
            &mut chosen,
            &mut permit,
        ) {
            SelectionOutcome::Admissible => {
                Some((chosen, permit.expect("admissible prepared selection")))
            }
            SelectionOutcome::GuardRejected | SelectionOutcome::NoSpatialMatch => {
                for (channel, original) in rollback.into_iter().rev() {
                    channel_to_indexed_data.insert(channel, original);
                }
                None
            }
        }
    }

    // Disabled by the merge of feature/cost-accounted-rho (lint fix, finding
    // RT-08): no code calls this method, at HEAD or after the merge. A
    // committing caller goes through `extract_prepared_data_candidates`.
    // /// The complete search without DR-129's E1 ordering. A committing caller
    // /// must go through [`SpaceMatcher::extract_prepared_data_candidates`],
    // /// which orders each repeated channel first.
    // fn search_candidate_selection(
    //     &self,
    //     matcher: &Box<dyn Match<P, A, K>>,
    //     channel_pattern_pairs: &[(&C, &P)],
    //     continuation: &K,
    //     channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    //     level: usize,
    //     chosen: &mut Vec<ConsumeCandidate<C, A>>,
    // ) -> SelectionOutcome {
    //     self.search_prepared_candidate_selection(
    //         matcher,
    //         channel_pattern_pairs,
    //         continuation,
    //         channel_to_indexed_data,
    //         level,
    //         chosen,
    //         &mut None,
    //     )
    // }

    /// The complete search with its commit permit, without DR-129's E1
    /// ordering. A committing caller must go through
    /// [`SpaceMatcher::extract_prepared_data_candidates`], which orders each
    /// repeated channel first.
    fn search_prepared_candidate_selection(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        continuation: &K,
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        level: usize,
        chosen: &mut Vec<ConsumeCandidate<C, A>>,
        permit: &mut Option<PreparedCommit>,
    ) -> SelectionOutcome {
        let result = walk_candidate_selections(
            self,
            matcher,
            channel_pattern_pairs,
            continuation,
            channel_to_indexed_data,
            level,
            chosen,
            |_, prepared| {
                *permit = Some(prepared);
                true
            },
            |outcome| match outcome {
                SelectionOutcome::GuardRejected => {
                    metrics::counter!(
                        RSPACE_MATCHER_GUARD_BACKTRACK_METRIC, "source" => RSPACE_METRICS_SOURCE
                    )
                    .increment(1);
                }
                SelectionOutcome::NoSpatialMatch => {
                    metrics::counter!(
                        RSPACE_MATCHER_SPATIAL_BACKTRACK_METRIC, "source" => RSPACE_METRICS_SOURCE
                    )
                    .increment(1);
                }
                SelectionOutcome::Admissible => unreachable!(),
            },
        );
        if result.stopped {
            SelectionOutcome::Admissible
        } else if result.any_leaf_reached {
            SelectionOutcome::GuardRejected
        } else {
            SelectionOutcome::NoSpatialMatch
        }
    }

    fn extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(std::sync::Arc<WaitingContinuation<P, K>>, i32)>,
        mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Option<ProduceCandidate<C, P, A, K>> {
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        for (cont, index) in &match_candidates {
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            // Zip references; the earlier value-shaped path cloned every
            // candidate continuation cloned every channel and every
            // pattern into an owned pair list.
            let __pair_start = std::time::Instant::now();
            let channel_pattern_pairs: Vec<(&C, &P)> =
                channels.iter().zip(cont.patterns.iter()).collect();
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(__pair_start.elapsed().as_nanos() as u64);

            // The complete, guard-aware search for THIS continuation. It
            // returns the lexicographically least selection that satisfies
            // every spatial bind AND the commit guard, and restores
            // `channel_to_index_data` in full when there is none — so the next
            // continuation sees the same pools this one did.
            //
            // Defect D1: the predecessor of this call took one spatial pick per
            // bind, asked the guard once, and on rejection `continue`d to the
            // next CONTINUATION — never to the next DATUM. A guarded receive
            // with a satisfying datum resting could therefore be left stuck.
            match self.extract_prepared_data_candidates(
                matcher,
                &channel_pattern_pairs,
                &cont.continuation,
                &mut channel_to_index_data,
            ) {
                Some((data_candidates, commit)) => {
                    metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                    return Some(ProduceCandidate {
                        channels,
                        continuation: cont.as_ref().clone(),
                        continuation_index: *index,
                        data_candidates,
                        commit: Some(commit),
                    });
                }
                None => continue,
            }
        }
        None
    }

    // ══════════════════════════════════════════════════════════════════════
    // THE ENABLED-RENDEZVOUS QUERY — read-only, no store mutation, no event
    // ══════════════════════════════════════════════════════════════════════

    /// **Every** admissible selection under `channel_pattern_pairs`, in exactly
    /// the depth-first order the committing selector visits them.
    ///
    /// Selector and enumerator invoke the same explicit-frame automaton with
    /// different leaf callbacks: the selector stops at the first admissible
    /// leaf; this callback records it and keeps scanning. On the same pools
    /// they therefore agree on the first element by construction — `out[0]`
    /// is the lexicographically least admissible selection of those pools.
    /// Changed by R2-04 (complete guard search, defect D1): play shuffles the
    /// candidates of each channel before it selects (DR-129), so a real
    /// `consume` on this state takes some member of `out`, not always
    /// `out[0]`. That membership is the bridge between speculative
    /// enumeration and ordinary execution.
    /// `t1_every_selector_choice_is_an_enumerated_selection` in
    /// `rspace++/tests/enabled_rendezvous_spec.rs` asserts it, and it asserts
    /// that fresh runs take every member.
    ///
    /// The shared machine is the single source of candidate ordering,
    /// residual-pool mutation, rollback, and guard consultation. Only the leaf
    /// policy differs, so no duplicate traversal can drift from consensus
    /// selection semantics.
    ///
    /// # Read-only
    ///
    /// Nothing here touches the hot store, the event log or the produce
    /// counter. `channel_to_indexed_data` is the caller's private copy of
    /// the pools; it is mutated during the descent and restored on the way
    /// out, exactly as the selector does.
    ///
    /// # Cost
    ///
    /// Bounded by `Π_j |pool_j|` leaves and `Σ_j Π_{i≤j} |pool_i|` calls to
    /// `Match::get` — the selector's guard-refuses-everything worst case, paid
    /// unconditionally because there is no early exit. For the single-bind
    /// shape (`l = 1`) that is one linear scan of the channel's pool.
    fn enumerate_admissible_selections(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        continuation: &K,
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        level: usize,
        chosen: &mut Vec<ConsumeCandidate<C, A>>,
        out: &mut Vec<Vec<ConsumeCandidate<C, A>>>,
    ) {
        let _ = walk_candidate_selections(
            self,
            matcher,
            channel_pattern_pairs,
            continuation,
            channel_to_indexed_data,
            level,
            chosen,
            |selection, _permit| {
                out.push(selection.to_vec());
                false
            },
            |_| {},
        );
    }

    /// `E(S)` — the **enabled rendezvous set** of the state `store` currently
    /// holds: every (waiting continuation × admissible data selection) pair
    /// that a COMM could fire right now.
    ///
    /// # What a caller gets
    ///
    /// A [`ProduceCandidate`] per enabled rendezvous, carrying the channel
    /// group, the firing [`WaitingContinuation`], its **store index**, and the
    /// selected [`ConsumeCandidate`]s with **their** store indices. That is
    /// precisely the argument
    /// [`crate::rspace::rspace::RSpace::process_match_found`] takes, so an
    /// enumerated rendezvous can be fired verbatim — the enumeration and the
    /// firing address the store through the same indices, computed once.
    ///
    /// # Determinism — this is a consensus surface
    ///
    /// Three orderings compose, and all three are total and content-derived:
    ///
    /// 1. **Channel groups** are visited in ascending `Vec<C>` order (`C:
    ///    Ord`). The group set is read out of a `HashMap`, whose iteration
    ///    order is seed-dependent, so it is sorted before use. Without this,
    ///    two validators enumerating the same state would produce the same
    ///    *set* in different *orders*, and any trace that names a rendezvous by
    ///    position would diverge.
    /// 2. **Continuations within a group** are ordered by
    ///    [`order_candidates_with_index`] — THE canonical candidate order, the
    ///    same one `produce` uses — with the store index as tie breaker.
    /// 3. **Selections within a continuation** are ordered by the depth-first
    ///    descent of [`SpaceMatcher::enumerate_admissible_selections`] over
    ///    canonically ordered data pools.
    ///
    /// # ⚠ The head of a group's selections is NOT "what an ordinary run does"
    ///
    /// Changed by R2-04 (complete guard search, defect D1): it is what a
    /// **`consume` arriving at this state** does on canonically ordered pools.
    /// Play shuffles the candidates of each channel first (DR-129), so an
    /// ordinary `consume` takes some member of the set. An
    /// ordinary **`produce`** splices its arriving datum into the pool at index
    /// `-1` (`RSpace::extract_produce_candidate`), ahead of the canonical
    /// order, so the produce regime's least admissible selection is
    /// generally a different member of the same set. The *set* is the same
    /// — admissibility is monotone in the pool — and this query computes
    /// that set. A caller that wants to reproduce a particular execution
    /// must name the selection it wants; it must not assume element 0.
    ///
    /// # Installed continuations
    ///
    /// `HotStore::get_continuations` prepends the group's installed (system)
    /// continuation, and `order_candidates_with_index` indexes the combined
    /// vector — which is the indexing `HotStore::remove_continuation` expects
    /// (it subtracts one when an installed entry is present). System processes
    /// are therefore enumerated like any other rendezvous, which is what makes
    /// a speculatively staged `stdout!` visible as enabled.
    fn enumerate_enabled_rendezvous(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        store: &Arc<Box<dyn HotStore<C, P, A, K>>>,
    ) -> Result<Vec<ProduceCandidate<C, P, A, K>>, RSpaceError>
    where
        C: Ord + Serialize,
        P: Serialize + StableHashSerialize,
        A: Serialize + StableHashSerialize,
        K: Serialize + StableHashSerialize,
        Datum<A>: CandidateOrderingBytes,
        WaitingContinuation<P, K>: CandidateOrderingBytes,
    {
        let state = store.snapshot();

        // (1) The channel groups, deterministically ordered. A group with an
        // installed continuation but no ordinary one still holds a rendezvous,
        // so both maps contribute keys.
        let mut groups: Vec<Vec<C>> = Vec::with_capacity(
            state
                .continuations
                .iter()
                .map(|shard| shard.len())
                .sum::<usize>() +
                state
                    .installed_continuations
                    .iter()
                    .map(|shard| shard.len())
                    .sum::<usize>(),
        );
        groups.extend(
            state
                .continuations
                .iter()
                .flat_map(|shard| shard.keys().cloned()),
        );
        groups.extend(
            state
                .installed_continuations
                .iter()
                .flat_map(|shard| shard.keys().cloned()),
        );
        groups.sort();
        groups.dedup();

        // (2) The data pools, one canonical ordering per channel, computed once
        // and shared by every continuation that binds that channel.
        let mut pools: HashMap<C, Vec<(Datum<A>, i32)>> = HashMap::new();
        for channels in groups.iter() {
            for channel in channels.iter() {
                if !pools.contains_key(channel) {
                    pools.insert(
                        channel.clone(),
                        order_candidates_with_index(store.get_data(channel)?),
                    );
                }
            }
        }

        let mut enabled: Vec<ProduceCandidate<C, P, A, K>> = Vec::new();
        for channels in groups.iter() {
            let candidates = order_candidates_with_index(store.get_continuations(channels)?);
            for (cont, continuation_index) in candidates.iter() {
                let channel_pattern_pairs: Vec<(&C, &P)> =
                    channels.iter().zip(cont.patterns.iter()).collect();

                // A FRESH pool map per continuation: the descent mutates and
                // restores it, but no continuation may observe a sibling's
                // residue.
                let mut per_continuation: HashMap<C, Vec<(Datum<A>, i32)>> =
                    HashMap::with_capacity(channels.len());
                for channel in channels.iter() {
                    if let Some(pool) = pools.get(channel) {
                        per_continuation.insert(channel.clone(), pool.clone());
                    }
                }

                let mut chosen: Vec<ConsumeCandidate<C, A>> =
                    Vec::with_capacity(channel_pattern_pairs.len());
                let mut selections: Vec<Vec<ConsumeCandidate<C, A>>> = Vec::new();
                self.enumerate_admissible_selections(
                    matcher,
                    &channel_pattern_pairs,
                    &cont.continuation,
                    &mut per_continuation,
                    0,
                    &mut chosen,
                    &mut selections,
                );

                enabled.reserve(selections.len());
                for data_candidates in selections {
                    enabled.push(ProduceCandidate {
                        channels: channels.clone(),
                        continuation: cont.clone(),
                        continuation_index: *continuation_index,
                        data_candidates,
                        commit: None,
                    });
                }
            }
        }
        Ok(enabled)
    }
}
