#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

use super::funding_graph::ResidualEdge;

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verus_spec(result =>
    requires
        *old(cursor) == usize::MAX || *old(cursor) < edges@.len(),
        forall |i: int| 0 <= i < edges@.len() ==>
            edges@[i].next == usize::MAX || edges@[i].next < i,
    ensures
        *final(cursor) == usize::MAX || *final(cursor) < edges@.len(),
        match result {
            None => *old(cursor) == usize::MAX && *final(cursor) == usize::MAX,
            Some(index) => index == *old(cursor) && index < edges@.len()
                && *final(cursor) == edges@[index as int].next
                && (*final(cursor) == usize::MAX || *final(cursor) < index),
        },
))]
#[inline]
pub(super) fn advance_adjacency(edges: &[ResidualEdge], cursor: &mut usize) -> Option<usize> {
    if *cursor == usize::MAX {
        return None;
    }
    let current = *cursor;
    *cursor = edges[current].next;
    Some(current)
}
