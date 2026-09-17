#[cfg(verus_keep_ghost)]
use vstd::prelude::*;

#[cfg(verus_keep_ghost)]
verus! {
    broadcast use vstd::seq_lib::lemma_seq_contains_after_push;

    proof fn bounded_unique_queue(queue: Seq<usize>, vertices: usize)
        requires
            queue.no_duplicates(),
            forall |node: usize| queue.contains(node) ==> node < vertices,
        ensures queue.len() <= vertices,
    {
        queue.unique_seq_to_set();
        queue.to_set_ensures();
        vstd::set_lib::range_set_properties::<usize>(0, vertices);
        let available = Set::<usize>::range(0, vertices);
        assert(queue.to_set().subset_of(available));
        vstd::set_lib::lemma_len_subset(queue.to_set(), available);
    }

    proof fn fresh_vertex_has_queue_space(queue: Seq<usize>, vertices: usize, fresh: usize)
        requires
            queue.no_duplicates(),
            forall |node: usize| queue.contains(node) ==> node < vertices,
            fresh < vertices,
            !queue.contains(fresh),
        ensures queue.len() < vertices,
    {
        bounded_unique_queue(queue.push(fresh), vertices);
    }
}

use super::funding_graph::ResidualEdge;

#[cfg_attr(verus_keep_ghost, verus_verify)]
#[cfg_attr(verus_keep_ghost, verus_spec(
    requires
        edge.to < old(parents)@.len(),
        index < usize::MAX,
        old(queue)@.len() < usize::MAX,
        old(queue)@.no_duplicates(),
        forall |node: usize| old(queue)@.contains(node) ==> node < old(parents)@.len(),
        forall |i: usize| i < old(parents)@.len() ==>
            (old(parents)@[i as int] != usize::MAX <==> old(queue)@.contains(i)),
    ensures
        final(parents)@.len() == old(parents)@.len(),
        final(queue)@.no_duplicates(),
        final(queue)@.len() <= final(parents)@.len(),
        forall |node: usize| final(queue)@.contains(node) ==> node < final(parents)@.len(),
        forall |i: usize| i < old(parents)@.len() ==>
            (final(parents)@[i as int] != usize::MAX <==> final(queue)@.contains(i)),
        if edge.capacity != 0 && old(parents)@[edge.to as int] == usize::MAX {
            final(parents)@[edge.to as int] == index
            && final(queue)@ == old(queue)@.push(edge.to)
            && (forall |i: int| 0 <= i < old(parents)@.len() && i != edge.to as int
                ==> final(parents)@[i] == old(parents)@[i])
        } else {
            final(parents)@ == old(parents)@ && final(queue)@ == old(queue)@
        },
))]
pub(super) fn discover_parent(
    edge: &ResidualEdge,
    index: usize,
    parents: &mut [usize],
    queue: &mut Vec<usize>,
) {
    if edge.capacity != 0 && parents[edge.to] == usize::MAX {
        #[cfg(verus_keep_ghost)]
        proof! {
            fresh_vertex_has_queue_space(queue@, parents.len(), edge.to);
        }
        parents[edge.to] = index;
        queue.push(edge.to);
    }
    #[cfg(verus_keep_ghost)]
    proof! {
        bounded_unique_queue(queue@, parents.len());
    }
}
