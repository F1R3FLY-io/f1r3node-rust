//! Sparse, lazy rows for a pure bipartite edge relation.
//!
//! A cacheable row remembers only successful edges and one frontier covering
//! the already evaluated target prefix. Failed pairs therefore cost one
//! integer per row rather than a dense bitmap. A non-cacheable row retains no
//! results; callers use that mode for cheap, deliberately dense filler
//! patterns whose result values would otherwise dominate memory.

use std::rc::Rc;

/// Deterministic work counters for a lazy relational bipartite match.
/// Counters are observational only and never participate in matching or cost
/// accounting.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RelationalMatchStats {
    pub edge_evaluations: usize,
    pub successful_edge_evaluations: usize,
    pub cached_edge_visits: usize,
    pub relation_row_reuses: usize,
    pub augmenting_frames: usize,
}

/// Result ownership shared between a retained relation edge and a live
/// assignment. Clearing the relation before finalization makes
/// [`into_owned`](Self::into_owned) move the result without cloning.
pub(super) enum EdgeResult<R> {
    Owned(R),
    Shared(Rc<R>),
}

impl<R: Clone> EdgeResult<R> {
    pub(super) fn into_owned(self) -> R {
        match self {
            Self::Owned(result) => result,
            Self::Shared(result) => {
                Rc::try_unwrap(result).unwrap_or_else(|shared| shared.as_ref().clone())
            }
        }
    }
}

struct RelationEdge<R> {
    target_index: usize,
    result: Rc<R>,
}

struct RelationRow<R> {
    cacheable: bool,
    scanned_targets: usize,
    edges: Vec<RelationEdge<R>>,
}

/// Per-frame cursor over the successful edges that existed when the frame was
/// created. The snapshot bound prevents a frame from replaying an edge it just
/// discovered before advancing to the remaining target suffix.
pub(super) struct RelationCursor {
    next_cached_edge: usize,
    cached_edge_limit: usize,
}

/// Sparse relation state. Retained storage is `O(P + E)`, where `P` is the
/// number of patterns and `E` is the number of successful cacheable edges.
pub(super) struct LazyRelation<R> {
    rows: Vec<RelationRow<R>>,
}

impl<R> LazyRelation<R> {
    pub(super) fn new(cacheable_rows: impl IntoIterator<Item = bool>) -> Self {
        Self {
            rows: cacheable_rows
                .into_iter()
                .map(|cacheable| RelationRow {
                    cacheable,
                    scanned_targets: 0,
                    edges: Vec::new(),
                })
                .collect(),
        }
    }

    pub(super) fn is_cacheable(&self, row_index: usize) -> bool { self.rows[row_index].cacheable }

    pub(super) fn has_scanned_targets(&self, row_index: usize) -> bool {
        self.rows[row_index].scanned_targets != 0
    }

    pub(super) fn cursor(&self, row_index: usize) -> RelationCursor {
        RelationCursor {
            next_cached_edge: 0,
            cached_edge_limit: self.rows[row_index].edges.len(),
        }
    }

    pub(super) fn next_cached(
        &self,
        row_index: usize,
        cursor: &mut RelationCursor,
    ) -> Option<(usize, Rc<R>)> {
        if cursor.next_cached_edge == cursor.cached_edge_limit {
            return None;
        }
        let edge = &self.rows[row_index].edges[cursor.next_cached_edge];
        cursor.next_cached_edge += 1;
        Some((edge.target_index, Rc::clone(&edge.result)))
    }

    /// Claim the next pair in the row's never-before-evaluated target suffix.
    /// The frontier advances before evaluation so an asynchronous caller can
    /// suspend in its edge PDA without ever evaluating the pair twice.
    pub(super) fn next_unscanned(
        &mut self,
        row_index: usize,
        target_count: usize,
    ) -> Option<usize> {
        let row = &mut self.rows[row_index];
        if row.scanned_targets == target_count {
            return None;
        }
        let target_index = row.scanned_targets;
        row.scanned_targets += 1;
        Some(target_index)
    }

    /// Retain one successful edge without cloning its result payload.
    pub(super) fn record_success(
        &mut self,
        row_index: usize,
        target_index: usize,
        result: R,
    ) -> Rc<R> {
        self.record_shared_success(row_index, target_index, Rc::new(result))
    }

    /// Retain a successful edge whose result is already shared. This lets a
    /// caller intern common results such as an empty binding delta.
    pub(super) fn record_shared_success(
        &mut self,
        row_index: usize,
        target_index: usize,
        result: Rc<R>,
    ) -> Rc<R> {
        self.rows[row_index].edges.push(RelationEdge {
            target_index,
            result: Rc::clone(&result),
        });
        result
    }

    /// Release relation ownership before assignments are finalized, allowing
    /// retained results to be moved out through `Rc::try_unwrap`.
    pub(super) fn clear(&mut self) {
        for row in &mut self.rows {
            row.edges.clear();
        }
    }
}
