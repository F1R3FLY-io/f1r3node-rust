use std::sync::Arc;

use pathmap::PathMap;
use pathmap::zipper::{ZipperMoving, ZipperReadOnlyValues};

use super::internal::WaitingContinuation;
use super::r#match::Match;

#[derive(Debug)]
pub struct RetainedSetAutomaton {
    segments: Vec<PathMap<Arc<[usize]>>>,
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
        for (candidate_index, candidate) in candidates.iter().enumerate() {
            for (segment, pattern) in candidate.patterns.iter().enumerate() {
                let path = matcher.retained_pattern_path(pattern).unwrap_or_default();
                insert_terminal(&mut segments[segment], &path, candidate_index);
            }
        }
        Self {
            segments,
            candidate_count: candidates.len(),
        }
    }

    pub fn matching_candidates(&self, datum_path: &[u8], bind_positions: &[usize]) -> Vec<bool> {
        let mut selected = vec![false; self.candidate_count];
        for position in bind_positions {
            let Some(segment) = self.segments.get(*position) else {
                continue;
            };
            collect_prefix_terminals(segment, datum_path, &mut selected);
        }
        selected
    }

    pub fn candidate_count(&self) -> usize { self.candidate_count }
}

fn insert_terminal(segment: &mut PathMap<Arc<[usize]>>, path: &[u8], candidate_index: usize) {
    let mut terminals = segment
        .get(path)
        .map(|existing| existing.iter().copied().collect::<Vec<_>>())
        .unwrap_or_default();
    if terminals.last().copied() != Some(candidate_index) {
        terminals.push(candidate_index);
    }
    segment.insert(path, terminals.into());
}

fn collect_prefix_terminals(
    segment: &PathMap<Arc<[usize]>>,
    datum_path: &[u8],
    selected: &mut [bool],
) {
    let mut zipper = segment.read_zipper();
    mark(zipper.get_val(), selected);
    for byte in datum_path {
        if !zipper.descend_to_existing_byte(*byte) {
            break;
        }
        mark(zipper.get_val(), selected);
    }
}

fn mark(terminals: Option<&Arc<[usize]>>, selected: &mut [bool]) {
    if let Some(terminals) = terminals {
        for terminal in terminals.iter().copied() {
            if let Some(slot) = selected.get_mut(terminal) {
                *slot = true;
            }
        }
    }
}
