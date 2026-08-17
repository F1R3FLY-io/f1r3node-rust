use std::num::NonZeroU64;

use pathmap::PathMap;
use pathmap::zipper::{Zipper, ZipperMoving, ZipperReadOnlyValues};

use super::bag::Bag;
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

#[derive(Debug)]
pub struct RetainedSetAutomaton {
    segments: Vec<PathMap<TerminalId>>,
    terminals: Vec<Terminal>,
    candidate_count: usize,
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
        let mut segments = (0..segment_count)
            .map(|_| PathMap::new())
            .collect::<Vec<_>>();
        let mut terminals = Vec::new();
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            let candidate_id = CandidateId(
                u64::try_from(candidate_index)
                    .expect("a Rust Vec candidate index always fits in a u64"),
            );
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
                insert_terminal(&mut segments[segment], &mut terminals, &path, candidate_id, query);
            }
        }
        Self {
            segments,
            terminals,
            candidate_count: candidates.len(),
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
        let mut selected = vec![false; self.candidate_count];
        for position in bind_positions {
            let Some(segment) = self.segments.get(*position) else {
                continue;
            };
            collect_prefix_terminals(segment, &self.terminals, datum_path, &mut selected);
            if let Some(resting) = resting {
                collect_product_prefix_terminals(segment, &self.terminals, resting, &mut selected);
            }
        }
        selected
    }

    pub fn candidate_count(&self) -> usize { self.candidate_count }
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
    terminals: &[Terminal],
    resting: &Bag,
    selected: &mut [bool],
) {
    if resting.is_empty() {
        return;
    }

    let mut transitions = segment.read_zipper();
    let mut data = resting.as_pathmap().read_zipper();
    mark_without_residual(transitions.get_val().copied(), terminals, selected);
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
            debug_assert!(transitions.ascend_byte());
            debug_assert!(data.ascend_byte());
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
