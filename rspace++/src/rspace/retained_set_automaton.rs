use std::num::NonZeroU64;

use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyValues};

use super::bag::{Bag, BagReadCursor};
use super::internal::WaitingContinuation;
use super::r#match::Match;
use super::rho_path::{RhoPathQuery, RhoPathV1};

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct CandidateId(u64);

#[derive(Debug, Clone, Copy)]
#[repr(transparent)]
struct TerminalId(NonZeroU64);

impl TerminalId {
    fn from_index(index: usize) -> Self {
        let ordinal = u64::try_from(index)
            .expect("a Rust Vec terminal index always fits in a u64")
            .checked_add(1)
            .expect("an allocated terminal arena cannot exceed u64::MAX entries");
        Self(NonZeroU64::new(ordinal).expect("a one-based terminal identifier is nonzero"))
    }

    fn index(self) -> usize {
        usize::try_from(self.0.get() - 1)
            .expect("a compiled terminal identifier always fits in usize")
    }
}

#[derive(Debug)]
struct Terminal {
    candidate: CandidateId,
    previous: Option<TerminalId>,
    query: Option<RhoPathQuery>,
}

/// Immutable representation and eligibility facts for one compiled retained
/// automaton revision.
///
/// A compiled automaton contains only the live continuation slice supplied to
/// [`RetainedSetAutomaton::compile`]. It therefore has no tombstones or dead
/// transitions: a continuation-version change replaces the complete derived
/// value atomically. Keeping that invariant observable prevents an unmeasured
/// dead/live rebuild threshold from being introduced later.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RetainedSetAutomatonStats {
    pub candidate_count: usize,
    pub indexed_candidate_count: usize,
    pub pattern_count: usize,
    pub indexed_pattern_count: usize,
    pub fallback_pattern_count: usize,
    pub terminal_count: usize,
    pub distinct_terminal_path_count: usize,
    pub shared_terminal_count: usize,
    pub live_terminal_count: usize,
    pub dead_terminal_count: usize,
    pub maximum_terminal_depth: usize,
}

#[derive(Debug)]
pub struct RetainedSetAutomaton {
    segments: Vec<PathMap<TerminalId>>,
    terminal_depths: Vec<Vec<usize>>,
    terminals: Vec<Terminal>,
    candidate_count: usize,
    stats: RetainedSetAutomatonStats,
    #[cfg(feature = "retained-flat-dispatch-experiment")]
    flat: Option<flat_dispatch_experiment::FlatDispatch>,
}

impl RetainedSetAutomaton {
    pub fn compile<P: Clone, A, K: Clone>(
        matcher: &dyn Match<P, A, K>,
        candidates: &[WaitingContinuation<P, K>],
    ) -> Self {
        let segment_count = candidates
            .iter()
            .map(|candidate| candidate.patterns.len())
            .max()
            .unwrap_or(0);
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        let mut flat_facts = matcher.retained_flat_dispatch_experiment().then(|| {
            flat_dispatch_experiment::FlatFacts::for_candidates(candidates, segment_count)
        });
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        let (segment_count, bind_position_count) = match flat_facts {
            Some(_) => (0, segment_count),
            None => (segment_count, segment_count),
        };
        let mut segments = (0..segment_count)
            .map(|_| PathMap::new())
            .collect::<Vec<_>>();
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        let segment_count = bind_position_count;
        let mut terminal_depths = vec![Vec::new(); segment_count];
        let mut terminals = Vec::new();
        let mut indexed_candidate_count = 0usize;
        let mut pattern_count = 0usize;
        let mut indexed_pattern_count = 0usize;
        let mut fallback_pattern_count = 0usize;
        let mut maximum_terminal_depth = 0usize;
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let candidate_id = CandidateId(
                u64::try_from(candidate_index)
                    .expect("a Rust Vec candidate index always fits in a u64"),
            );
            let mut candidate_is_indexed = false;
            for (segment, pattern) in candidate.patterns.iter().enumerate() {
                let (path, query) = if matcher.supports_rho_path_candidate_pruning() {
                    match matcher
                        .rho_path_query(pattern)
                        .filter(RhoPathQuery::is_selective)
                    {
                        Some(query) => {
                            let path = query.retained_prefix();
                            (path, Some(query))
                        }
                        None => (matcher.retained_pattern_path(pattern).unwrap_or_default(), None),
                    }
                } else {
                    (matcher.retained_pattern_path(pattern).unwrap_or_default(), None)
                };
                // Candidate-pool Bag keys begin with the escaped datum-
                // structure frame. Store the automaton in that same byte
                // domain so a retained segment and the authoritative Bag can
                // be traversed as one zipper product. Fresh paths are framed
                // on demand below; no singleton Bag is constructed for them.
                let path = RhoPathV1::relative_datum_structure_prefix(&path);
                pattern_count += 1;
                if path.is_empty() {
                    fallback_pattern_count += 1;
                } else {
                    indexed_pattern_count += 1;
                    candidate_is_indexed = true;
                }
                maximum_terminal_depth = maximum_terminal_depth.max(path.len());
                terminal_depths[segment].push(path.len());
                #[cfg(feature = "retained-flat-dispatch-experiment")]
                if let Some(facts) = flat_facts.as_mut() {
                    facts.push(segment, path, &mut terminals, candidate_id, query);
                    continue;
                }
                insert_terminal(&mut segments[segment], &mut terminals, &path, candidate_id, query);
            }
            indexed_candidate_count += usize::from(candidate_is_indexed);
        }
        for depths in &mut terminal_depths {
            depths.sort_unstable();
            depths.dedup();
        }
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        let flat = flat_facts.map(|facts| facts.build(&mut terminals));
        let distinct_terminal_path_count =
            segments.iter().map(|segment| segment.iter().count()).sum();
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        let distinct_terminal_path_count = flat
            .as_ref()
            .map_or(distinct_terminal_path_count, |tables| tables.terminal_state_count());
        let terminal_count = terminals.len();
        let stats = RetainedSetAutomatonStats {
            candidate_count: candidates.len(),
            indexed_candidate_count,
            pattern_count,
            indexed_pattern_count,
            fallback_pattern_count,
            terminal_count,
            distinct_terminal_path_count,
            shared_terminal_count: terminal_count.saturating_sub(distinct_terminal_path_count),
            live_terminal_count: terminal_count,
            dead_terminal_count: 0,
            maximum_terminal_depth,
        };
        Self {
            segments,
            terminal_depths,
            terminals,
            candidate_count: candidates.len(),
            stats,
            #[cfg(feature = "retained-flat-dispatch-experiment")]
            flat,
        }
    }

    pub fn matching_candidates(&self, datum_path: &[u8], bind_positions: &[usize]) -> Vec<bool> {
        self.matching_candidates_with_resting(datum_path, bind_positions, None)
    }

    /// Candidate superset accepted by either the fresh datum or a resting
    /// occurrence in the authoritative Bag.
    ///
    /// The fresh datum remains a single byte path. Resting data is intersected
    /// directly with each relevant retained PathMap segment, sharing common
    /// prefixes and never decoding or enumerating complete records. A terminal
    /// reached by the product is conservatively retained; its residual query
    /// is evaluated later by the ordinary candidate scan. The fresh path can
    /// evaluate that residual immediately because its complete structure is
    /// already borrowed here.
    pub fn matching_candidates_with_resting(
        &self,
        datum_path: &[u8],
        bind_positions: &[usize],
        resting: Option<&Bag>,
    ) -> Vec<bool> {
        #[cfg(feature = "retained-flat-dispatch-experiment")]
        if let Some(flat) = &self.flat {
            return flat.matching_candidates_with_resting(
                self,
                datum_path,
                bind_positions,
                resting,
            );
        }
        let mut selected = vec![false; self.candidate_count];
        for position in bind_positions {
            let Some(segment) = self.segments.get(*position) else {
                continue;
            };
            collect_prefix_terminals(segment, &self.terminals, datum_path, &mut selected);
            if let Some(resting) = resting {
                collect_product_prefix_terminals(
                    segment,
                    &self.terminal_depths[*position],
                    &self.terminals,
                    resting,
                    &mut selected,
                );
            }
        }
        selected
    }

    pub fn candidate_count(&self) -> usize { self.candidate_count }

    pub const fn stats(&self) -> RetainedSetAutomatonStats { self.stats }
}

fn insert_terminal(
    segment: &mut PathMap<TerminalId>,
    terminals: &mut Vec<Terminal>,
    path: &[u8],
    candidate: CandidateId,
    query: Option<RhoPathQuery>,
) {
    let terminal = TerminalId::from_index(terminals.len());
    terminals.push(Terminal {
        candidate,
        previous: segment.get(path).copied(),
        query,
    });
    segment.insert(path, terminal);
}

fn collect_prefix_terminals(
    segment: &PathMap<TerminalId>,
    terminals: &[Terminal],
    raw_datum_path: &[u8],
    selected: &mut [bool],
) {
    let mut zipper = segment.read_zipper();
    mark(zipper.get_val().copied(), terminals, raw_datum_path, selected);
    for byte in RhoPathV1::relative_datum_structure_prefix_bytes(raw_datum_path) {
        if !zipper.descend_to_existing_byte(byte) {
            break;
        }
        mark(zipper.get_val().copied(), terminals, raw_datum_path, selected);
    }
}

/// Intersect one retained transition trie with an authoritative datum Bag.
///
/// Both zippers occupy the same escaped structure-prefix byte domain. The
/// explicit `levels` stack is a depth-first product automaton: a branch is
/// entered only when both tries have that byte, and each common prefix is
/// visited once. Reaching a retained terminal proves that at least one stored
/// datum has the terminal's exact prefix. Residual wildcard/capture checks are
/// deliberately deferred to `CandidateScan`, so this operation can introduce
/// false positives but cannot hide a spatial match.
fn collect_product_prefix_terminals(
    segment: &PathMap<TerminalId>,
    terminal_depths: &[usize],
    terminals: &[Terminal],
    resting: &Bag,
    selected: &mut [bool],
) {
    if resting.is_empty() {
        return;
    }

    resting.with_read_cursor(|data| {
        collect_product_prefix_terminals_with_cursor(
            segment,
            terminal_depths,
            terminals,
            resting,
            data,
            selected,
        );
    });
}

fn collect_product_prefix_terminals_with_cursor(
    segment: &PathMap<TerminalId>,
    terminal_depths: &[usize],
    terminals: &[Terminal],
    resting: &Bag,
    mut data: BagReadCursor<'_, 'static>,
    selected: &mut [bool],
) {
    let mut transitions = segment.read_zipper();
    mark_without_residual(transitions.get_val().copied(), terminals, selected);

    // A single distinct Bag key is the common datum-first case, including
    // arbitrary multiplicity at that key. When at most one non-root terminal
    // depth can prefix that key, one exact PathMap lookup avoids walking a
    // long shared byte prefix. The generated, deduplicated depth schedule does
    // not materialize candidates or replace either PathMap. Multiple eligible
    // depths deliberately fall through to the linear zipper product instead
    // of performing a potentially superlinear sequence of prefix lookups.
    // The root terminal was handled above.
    if resting.distinct_len() == 1 {
        let has_value = data.to_next_val();
        assert!(has_value, "a nonempty singleton Bag has one value");
        let path = data.path();
        let first_non_root = terminal_depths.partition_point(|depth| *depth == 0);
        let end = terminal_depths.partition_point(|depth| *depth <= path.len());
        match terminal_depths.get(first_non_root..end).unwrap_or_default() {
            [] => return,
            [depth] => {
                mark_without_residual(segment.get(&path[..*depth]).copied(), terminals, selected);
                return;
            }
            _ => data.reset(),
        }
    }

    let mut levels = vec![(transitions.child_mask() & data.child_mask()).iter()];
    while let Some(children) = levels.last_mut() {
        if let Some(byte) = children.next() {
            transitions.descend_to_byte(byte);
            data.descend_to_byte(byte);
            mark_without_residual(transitions.get_val().copied(), terminals, selected);
            levels.push((transitions.child_mask() & data.child_mask()).iter());
            continue;
        }

        levels.pop();
        if !levels.is_empty() {
            let transition_ascended = transitions.ascend_byte();
            let data_ascended = data.ascend_byte();
            debug_assert!(
                transition_ascended && data_ascended,
                "product zippers ascend after every matched descent"
            );
        }
    }
}

fn mark_without_residual(
    mut terminal: Option<TerminalId>,
    terminals: &[Terminal],
    selected: &mut [bool],
) {
    while let Some(terminal_id) = terminal {
        let node = terminals
            .get(terminal_id.index())
            .expect("a compiled terminal identifier always addresses its arena");
        let candidate_index = usize::try_from(node.candidate.0)
            .expect("a compiled candidate identifier always fits in usize");
        if let Some(slot) = selected.get_mut(candidate_index) {
            *slot = true;
        }
        terminal = node.previous;
    }
}

fn mark(
    mut terminal: Option<TerminalId>,
    terminals: &[Terminal],
    datum_path: &[u8],
    selected: &mut [bool],
) {
    while let Some(terminal_id) = terminal {
        let node = terminals
            .get(terminal_id.index())
            .expect("a compiled terminal identifier always addresses its arena");
        let candidate_index = usize::try_from(node.candidate.0)
            .expect("a compiled candidate identifier always fits in usize");
        if node
            .query
            .as_ref()
            .is_none_or(|query| query.matches_structure(datum_path))
        {
            if let Some(slot) = selected.get_mut(candidate_index) {
                *slot = true;
            }
        }
        terminal = node.previous;
    }
}

#[cfg(feature = "retained-flat-dispatch-experiment")]
#[doc(hidden)]
#[path = "../../tests/retained_set_automaton/flat_dispatch_experiment.rs"]
pub mod flat_dispatch_experiment;
