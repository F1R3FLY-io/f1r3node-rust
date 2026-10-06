#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

use super::funding_arithmetic::checked_residual_transfer;
use super::funding_graph::ResidualEdge;

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verus_spec(result =>
    requires
        index < old(edges)@.len(),
        (index ^ 1) < old(edges)@.len(),
    ensures
        final(edges)@.len() == old(edges)@.len(),
        forall |i: int| 0 <= i < old(edges)@.len() ==>
            final(edges)@[i].to == old(edges)@[i].to && final(edges)@[i].next == old(edges)@[i].next,
        match result {
            Some(predecessor) =>
                predecessor == old(edges)@[(index ^ 1) as int].to
                && amount <= old(edges)@[index as int].capacity
                && old(edges)@[(index ^ 1) as int].capacity as int + amount as int <= u64::MAX as int
                && final(edges)@[index as int].capacity as int + amount as int == old(edges)@[index as int].capacity as int
                && final(edges)@[(index ^ 1) as int].capacity as int == old(edges)@[(index ^ 1) as int].capacity as int + amount as int
                && (forall |i: int| 0 <= i < old(edges)@.len() && i != index as int && i != (index ^ 1) as int
                    ==> final(edges)@[i] == old(edges)@[i]),
            None => final(edges)@ == old(edges)@
                && (amount > old(edges)@[index as int].capacity
                    || old(edges)@[(index ^ 1) as int].capacity as int + amount as int > u64::MAX as int),
        },
))]
pub(super) fn augment_edge(edges: &mut [ResidualEdge], index: usize, amount: u64) -> Option<usize> {
    #[cfg(verus_keep_ghost)]
    proof! {
        assert(index != (index ^ 1)) by(bit_vector);
    }
    let (forward, reverse) =
        checked_residual_transfer(edges[index].capacity, edges[index ^ 1].capacity, amount)?;
    edges[index].capacity = forward;
    edges[index ^ 1].capacity = reverse;
    Some(edges[index ^ 1].to)
}
