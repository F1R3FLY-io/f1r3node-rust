#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg_attr(verus_keep_ghost, verus_verify)]
pub(super) struct ResidualEdge {
    pub(super) to: usize,
    pub(super) next: usize,
    pub(super) capacity: u64,
}

#[cfg(verus_keep_ghost)]
verus! {
    pub(super) open spec fn residual_owner(edges: Seq<ResidualEdge>, index: usize) -> usize {
        edges[(index ^ 1) as int].to
    }

    pub(super) open spec fn adjacency_well_formed(edges: Seq<ResidualEdge>, heads: Seq<usize>) -> bool {
        &&& forall |node: int| 0 <= node < heads.len() ==>
            heads[node] == usize::MAX || heads[node] < edges.len()
        &&& forall |index: int| 0 <= index < edges.len() ==>
            edges[index].to < heads.len()
            && (edges[index].next == usize::MAX || edges[index].next < index)
    }
}

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        from < old(heads)@.len(),
        to < old(heads)@.len(),
        old(edges)@.len() <= usize::MAX - 2,
    ensures
        final(edges)@.len() == old(edges)@.len() + 2,
        final(heads)@.len() == old(heads)@.len(),
        forall |i: int| 0 <= i < old(edges)@.len() ==> final(edges)@[i] == old(edges)@[i],
        final(edges)@[old(edges)@.len() as int].to == to,
        final(edges)@[old(edges)@.len() as int].next == old(heads)@[from as int],
        final(edges)@[old(edges)@.len() as int].capacity == capacity,
        final(edges)@[old(edges)@.len() as int + 1].to == from,
        final(edges)@[old(edges)@.len() as int + 1].next ==
            (if from == to { old(edges)@.len() as int } else { old(heads)@[to as int] as int }),
        final(edges)@[old(edges)@.len() as int + 1].capacity == 0,
        final(heads)@[to as int] == old(edges)@.len() + 1,
        from != to ==> final(heads)@[from as int] == old(edges)@.len(),
        forall |i: int| 0 <= i < old(heads)@.len() && i != from as int && i != to as int
            ==> final(heads)@[i] == old(heads)@[i],
        adjacency_well_formed(old(edges)@, old(heads)@) ==>
            adjacency_well_formed(final(edges)@, final(heads)@),
        old(edges)@.len() % 2 == 0 ==> (
            final(edges)@.len() % 2 == 0
            && residual_owner(final(edges)@, old(edges)@.len() as usize) == from
            && residual_owner(final(edges)@, (old(edges)@.len() + 1) as usize) == to
            && (forall |i: usize| i < old(edges)@.len() ==>
                (i ^ 1) < old(edges)@.len()
                && residual_owner(final(edges)@, i) == residual_owner(old(edges)@, i))
        ),
))]
pub(super) fn add_edge(
    edges: &mut Vec<ResidualEdge>,
    heads: &mut [usize],
    from: usize,
    to: usize,
    capacity: u64,
) {
    let forward = edges.len();
    #[cfg(verus_keep_ghost)]
    proof! {
        if forward % 2 == 0 {
            assert(forward < usize::MAX);
            assert((forward ^ 1) == (forward + 1) as usize) by(bit_vector)
                requires forward % 2 == 0;
            assert((((forward + 1) as usize) ^ 1) == forward) by(bit_vector)
                requires forward % 2 == 0;
            assert forall |i: usize| i < forward implies (i ^ 1) < forward by {
                assert((i ^ 1) < forward) by(bit_vector)
                    requires i < forward, forward % 2 == 0;
            }
        }
    }
    edges.push(ResidualEdge {
        to,
        next: heads[from],
        capacity,
    });
    heads[from] = forward;
    edges.push(ResidualEdge {
        to: from,
        next: heads[to],
        capacity: 0,
    });
    heads[to] = forward + 1;
}
