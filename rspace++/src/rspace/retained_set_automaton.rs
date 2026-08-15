use std::num::NonZeroU64;

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyValues};

use super::internal::WaitingContinuation;
use super::r#match::Match;

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
                let path = matcher.retained_pattern_path(pattern).unwrap_or_default();
                insert_terminal(&mut segments[segment], &mut terminals, &path, candidate_id);
            }
        }
        Self {
            segments,
            terminals,
            candidate_count: candidates.len(),
        }
    }

    pub fn matching_candidates(&self, datum_path: &[u8], bind_positions: &[usize]) -> Vec<bool> {
        let mut selected = vec![false; self.candidate_count];
        for position in bind_positions {
            let Some(segment) = self.segments.get(*position) else {
                continue;
            };
            collect_prefix_terminals(segment, &self.terminals, datum_path, &mut selected);
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
) {
    let terminal = TerminalId::from_index(terminals.len());
    terminals.push(Terminal {
        candidate,
        previous: segment.get(path).copied(),
    });
    segment.insert(path, terminal);
}

fn collect_prefix_terminals(
    segment: &PathMap<TerminalId>,
    terminals: &[Terminal],
    datum_path: &[u8],
    selected: &mut [bool],
) {
    let mut zipper = segment.read_zipper();
    mark(zipper.get_val().copied(), terminals, selected);
    for byte in datum_path {
        if !zipper.descend_to_existing_byte(*byte) {
            break;
        }
        mark(zipper.get_val().copied(), terminals, selected);
    }
}

fn mark(mut terminal: Option<TerminalId>, terminals: &[Terminal], selected: &mut [bool]) {
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
