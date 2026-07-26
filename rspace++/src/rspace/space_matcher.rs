// See rspace/src/main/scala/coop/rchain/rspace/SpaceMatcher.scala

use std::collections::HashMap;

use super::r#match::Match;
use super::rspace_interface::ISpace;
use crate::rspace::internal::{ConsumeCandidate, Datum, ProduceCandidate, WaitingContinuation};
use crate::rspace::metrics_constants::{
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_PAIR_CONSTRUCTION_NS_METRIC,
    RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, RSPACE_MATCHER_GUARD_BACKTRACK_METRIC,
    RSPACE_MATCHER_SPATIAL_BACKTRACK_METRIC, RSPACE_METRICS_SOURCE,
};

type MatchingDataCandidate<C, A> = (ConsumeCandidate<C, A>, Vec<(Datum<A>, i32)>);

/// Why a subtree of the candidate search failed — and therefore WHICH of the two
/// incompletenesses the search is repairing when it backtracks past it.
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

pub trait SpaceMatcher<C, P, A, K>: ISpace<C, P, A, K>
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
    /// [`SpaceMatcher::extract_guarded_data_candidates`], which drives
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
    /// whose `Produce` source carries two owned hashes) where the scan itself is
    /// linear — measured at 197 ms → 17 ms for a 1000-datum exhaustive scan.
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
            // P4.2: the matcher BORROWS the pattern and the Arc-shared datum
            // payload — a failing attempt copies nothing at this boundary
            // (pre-P4.2: one full pattern clone + one full payload clone per
            // candidate per attempt; the `rspace.matcher.clone_ns` timer that
            // measured them is retired with the clones).
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
                    // (pre-P4.1: a deep copy per candidate).
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
    /// P4.2: the pairs are BORROWED (`(&C, &P)`) — callers zip references
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
    /// [`SpaceMatcher::extract_guarded_data_candidates`] instead.
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

    /// THE candidate selector: the complete, guard-aware search for one datum
    /// per bind of `continuation`.
    ///
    /// # What it computes
    ///
    /// Write the receive's binds as `(c_0, p_0) … (c_{l-1}, p_{l-1})` and let
    /// `pool_j` be the candidate list of channel `c_j` at the moment bind `j`
    /// is filled (the pool of `c_j` minus whatever earlier binds speculatively
    /// removed from it). A **selection** assigns each bind `j` a position
    /// `t_j` in `pool_j`. A selection is **admissible** when
    ///
    /// 1. every `pool_j[t_j]` matches `p_j` spatially, and
    /// 2. `check_commit(continuation, matched_0 … matched_{l-1})` holds of the
    ///    bind-transformed payloads the spatial matcher produced.
    ///
    /// This function returns the candidates of the **lexicographically least
    /// admissible selection** under `(t_0, t_1, …, t_{l-1})`, or `None` when
    /// there is none.
    ///
    /// # Why (defect D1)
    ///
    /// The predecessor of this function tested condition 2 at exactly ONE
    /// selection per continuation — the one where every `t_j` is the least
    /// spatial match — and on rejection abandoned the continuation entirely.
    /// A guard was therefore candidate APPROVAL and never candidate SELECTION,
    /// so a single rejected pick stranded a rendezvous that a different resting
    /// datum would have satisfied: with `@"offer"!(55) | @"offer"!(42)` resting
    /// and `for(@px <- @"offer" where px <= 45)` installed, the COMM fired only
    /// when the pool order happened to present the 42 first. The rho calculus
    /// admits no such stuck state — an enabled COMM is enabled — so the search
    /// is completed here rather than the guard being weakened.
    ///
    /// The selection ORDER is unchanged by that completion: the first
    /// admissible selection in the same lexicographic order is still the
    /// answer. What changed is that the search no longer stops at the first
    /// SPATIALLY admissible selection when the guard rejects it.
    ///
    /// # ★ The second, wider repair — read this before reviewing for consensus
    ///
    /// A search that backtracks is also complete in a way the predecessor was
    /// not even without any guard. The predecessor filled the binds left to
    /// right, each with its first spatial match, and abandoned the continuation
    /// the moment a later bind found nothing — even when a different choice at
    /// an earlier bind would have left that later bind satisfiable. For
    /// `for(@x <- c; @"k" <- c)` with `c` holding `"k"` and `"other"`, the
    /// wildcard bind can take the `"k"` and strand the receive, although
    /// `(x = "other", "k")` is a perfectly good rendezvous. That is the same
    /// defect wearing different clothes, and this search repairs it too.
    ///
    /// The two repairs have very different reach, and conflating them would be
    /// the kind of quiet widening this codebase does not do:
    ///
    /// | repair | affects | counter |
    /// |---|---|---|
    /// | guard rejection ⇒ next datum (defect D1) | only receives that carry a `where` guard — new syntax, so no program written against the pre-guard language can change behaviour | `rspace.matcher.guard_backtrack` |
    /// | spatial dead-end ⇒ next datum | ANY multi-bind receive whose earlier bind can swallow a datum a later bind needed — ordinary guard-free Rholang | `rspace.matcher.spatial_backtrack` |
    ///
    /// Both are consensus-visible: each makes a COMM fire that previously did
    /// not. They are separated by [`SelectionOutcome`] and counted apart so the
    /// second can be reviewed, and if consensus review decides to narrow the
    /// change to defect D1 alone, one place decides it — refusing to advance
    /// `cursor` in [`SpaceMatcher::search_candidate_selection`] when the
    /// subtree returned [`SelectionOutcome::NoSpatialMatch`] reproduces the
    /// predecessor's behaviour exactly for guard-free receives.
    ///
    /// # Determinism
    ///
    /// The enumeration is a pure function of the pools as received, so the
    /// selection is determined by the pool ORDER — which is
    /// [`crate::rspace::candidate_order`]'s canonical order in the play space
    /// and in the replay space alike. Replay additionally filters each pool
    /// down to the produces the recorded COMM consumed; filtering removes
    /// elements without reordering the survivors, so replay's pool is a
    /// SUBSEQUENCE of play's. Let `S` be play's selection. `S` survives the
    /// filter (its data are exactly the ones the COMM consumed). Any admissible
    /// selection lexicographically smaller than `S` in the filtered pool is
    /// also present, and admissible, in the unfiltered pool — where play would
    /// have reached it first. There is none, so replay selects `S`. Play and
    /// replay agree by construction rather than by the trace assertion.
    ///
    /// # Cost
    ///
    /// Every leaf reached is one `check_commit`. When the continuation carries
    /// no guard the default `check_commit` accepts the FIRST leaf, so the work
    /// is exactly the predecessor's — one spatial call per bind, stopping at
    /// each bind's first match — and backtracking is never entered at all
    /// unless a later bind finds nothing.
    ///
    /// With a guard, the bill is bounded by `Π_j |pool_j|` leaves and
    /// `Σ_j Π_{i≤j} |pool_i|` calls to `Match::get`, reached only when the guard
    /// refuses everything. Measured on this crate's test matcher by
    /// `the_cost_of_a_complete_guarded_search_is_bounded_and_measured`
    /// (wall time is the whole `consume`, candidate ordering included):
    ///
    /// | receive | store | `Match::get` | `check_commit` | release | debug |
    /// |---|---|---|---|---|---|
    /// | guarded, one bind, guard refuses all | 1000 on one channel | 1000 | 1000 | 1.01 ms | 17.0 ms |
    /// | guarded, two binds, guard refuses all | 60 × 60 | 3660 | 3600 | 1.17 ms | 11.5 ms |
    /// | UNGUARDED, two binds | 60 × 60 | 2 | 1 | 0.10 ms | 1.9 ms |
    ///
    /// A guarded receive over a thousand resting data thus costs about a
    /// millisecond of matcher time in release, and the unguarded row — every
    /// program that predates the `where` syntax — is one spatial call per bind.
    ///
    /// The single-bind row is the shape that carries almost every guard in
    /// practice, and it is linear: `l = 1` makes the worst case the plain scan.
    /// The two-bind row is the quadratic corner, and it is entered only by a
    /// guarded join over two well-populated channels. The unguarded row is the
    /// hot path of every existing program, and it is untouched.
    fn extract_guarded_data_candidates(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        continuation: &K,
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Option<Vec<ConsumeCandidate<C, A>>> {
        let mut chosen: Vec<ConsumeCandidate<C, A>> =
            Vec::with_capacity(channel_pattern_pairs.len());

        match self.search_candidate_selection(
            matcher,
            channel_pattern_pairs,
            continuation,
            channel_to_indexed_data,
            0,
            &mut chosen,
        ) {
            SelectionOutcome::Admissible => Some(chosen),
            SelectionOutcome::GuardRejected | SelectionOutcome::NoSpatialMatch => None,
        }
    }

    /// One level of [`SpaceMatcher::extract_guarded_data_candidates`]'s
    /// depth-first search: fill bind `level` with each of its spatial matches
    /// in pool order and recurse, until the leaf's guard accepts.
    ///
    /// `chosen` accumulates the candidates in BIND order — the order
    /// `check_commit` reads them in, and the order in which the receive's de
    /// Bruijn indices were assigned — and is left holding exactly the
    /// admissible selection on [`SelectionOutcome::Admissible`], and exactly
    /// what it held on entry on either failure.
    ///
    /// `channel_to_indexed_data` is likewise restored on failure: each level
    /// puts its own pool back when its scan is exhausted, and a level's
    /// descendants only ever leave entries this level then overwrites. A
    /// caller iterating continuations therefore sees an untouched map after a
    /// continuation fails to match, exactly as the rollback list it replaces
    /// guaranteed.
    ///
    /// The recursion depth is the receive's arity — the number of binds
    /// written in the source term (`for(a <- x; b <- y; …)`) — so it is fixed
    /// by the program text and is not data-dependent.
    ///
    /// The returned [`SelectionOutcome`] distinguishes the two reasons a
    /// subtree can fail, so the caller can count them apart; see that type for
    /// why the distinction matters.
    fn search_candidate_selection(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channel_pattern_pairs: &[(&C, &P)],
        continuation: &K,
        channel_to_indexed_data: &mut HashMap<C, Vec<(Datum<A>, i32)>>,
        level: usize,
        chosen: &mut Vec<ConsumeCandidate<C, A>>,
    ) -> SelectionOutcome {
        // Leaf: every bind is filled, so the guard can finally be asked. This
        // is the ONE place a commit guard is consulted on a candidate
        // selection. P4.2: it reads the matched payloads through borrows.
        if level == channel_pattern_pairs.len() {
            let matched: Vec<&A> = chosen.iter().map(|candidate| &*candidate.datum.a).collect();
            return match matcher.check_commit(continuation, &matched) {
                true => SelectionOutcome::Admissible,
                false => SelectionOutcome::GuardRejected,
            };
        }

        let (channel, pattern) = channel_pattern_pairs[level];

        // This level's pool, snapshotted so the scan survives the mutations the
        // descent makes to the same map entry (a join may bind the same channel
        // twice, in which case the descendants read what this level wrote).
        // Cost parity: the predecessor cloned the pool once per bind too — as
        // the rollback entry it recorded before mutating.
        let pool = match channel_to_indexed_data.get(channel) {
            Some(indexed_data) => indexed_data.clone(),
            // No pool for this channel at all: the bind cannot be filled, so no
            // selection exists. (The predecessor pushed `None` into its
            // accumulator, which failed the whole set the same way.)
            None => return SelectionOutcome::NoSpatialMatch,
        };

        // Did ANY complete selection under this level reach the guard? If none
        // did, this level's failure is spatial, not a guard veto.
        let mut any_leaf_reached = false;

        // The last bind has no binds after it, so nothing reads a residual pool
        // from it. Skipping that construction is what keeps an exhausted
        // single-bind scan — the shape almost every guard in practice has —
        // linear rather than quadratic.
        let is_last_bind = level + 1 == channel_pattern_pairs.len();

        let mut cursor = 0usize;
        while let Some((position, candidate)) =
            self.next_spatial_match(matcher, channel.clone(), &pool, pattern, cursor)
        {
            // Speculatively remove the chosen datum for the benefit of the
            // remaining binds (the residual is the pool minus it, or the whole
            // pool when the datum is persistent).
            if !is_last_bind {
                let residual = Self::residual_pool(&pool, position, candidate.datum.persist);
                channel_to_indexed_data.insert(channel.clone(), residual);
            }
            chosen.push(candidate);

            let outcome = self.search_candidate_selection(
                matcher,
                channel_pattern_pairs,
                continuation,
                channel_to_indexed_data,
                level + 1,
                chosen,
            );
            if outcome == SelectionOutcome::Admissible {
                return SelectionOutcome::Admissible;
            }

            // The subtree under this choice holds no admissible selection:
            // undo the choice and resume the scan at the NEXT spatial match of
            // this bind. This is the step the predecessor was missing — it
            // abandoned the whole continuation here instead.
            chosen.pop();
            match outcome {
                SelectionOutcome::GuardRejected => {
                    any_leaf_reached = true;
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
                SelectionOutcome::Admissible => unreachable!("returned above"),
            }
            cursor = position + 1;
        }

        // Exhausted: restore this level's pool for whoever scans next.
        channel_to_indexed_data.insert(channel.clone(), pool);
        match any_leaf_reached {
            true => SelectionOutcome::GuardRejected,
            false => SelectionOutcome::NoSpatialMatch,
        }
    }

    fn extract_first_match(
        &self,
        matcher: &Box<dyn Match<P, A, K>>,
        channels: Vec<C>,
        match_candidates: Vec<(WaitingContinuation<P, K>, i32)>,
        mut channel_to_index_data: HashMap<C, Vec<(Datum<A>, i32)>>,
    ) -> Option<ProduceCandidate<C, P, A, K>> {
        metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CALLS_METRIC, "source" => RSPACE_METRICS_SOURCE)
            .increment(1);
        for (cont, index) in &match_candidates {
            metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_CANDIDATES_ITERATED_METRIC, "source" => RSPACE_METRICS_SOURCE)
                .increment(1);
            // P4.2: zip REFERENCES — pre-P4.2 every candidate continuation
            // cloned every channel and every pattern into an owned pair list.
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
            match self.extract_guarded_data_candidates(
                matcher,
                &channel_pattern_pairs,
                &cont.continuation,
                &mut channel_to_index_data,
            ) {
                Some(data_candidates) => {
                    metrics::counter!(RSPACE_MATCHER_EXTRACT_FIRST_MATCH_SUCCESS_METRIC, "source" => RSPACE_METRICS_SOURCE)
                        .increment(1);
                    return Some(ProduceCandidate {
                        channels,
                        continuation: cont.clone(),
                        continuation_index: *index,
                        data_candidates,
                    });
                }
                None => continue,
            }
        }
        None
    }
}
