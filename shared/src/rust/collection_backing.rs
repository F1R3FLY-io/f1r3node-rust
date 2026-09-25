use std::mem::{align_of, size_of};

pub fn tree_backing<K, V>(entries: usize) -> Option<(usize, usize)> {
    if entries == 0 {
        return Some((0, 0));
    }
    let nodes = 1 + (entries - 1) / 5;
    let alignment = align_of::<K>()
        .max(align_of::<V>())
        .max(align_of::<usize>())
        .max(align_of::<u16>());
    let slots = size_of::<K>()
        .checked_add(size_of::<V>())?
        .checked_mul(11)?;
    let pointers = size_of::<usize>().checked_mul(13)?;
    let padding = alignment.checked_mul(8)?;
    let node_bytes = slots
        .checked_add(pointers)?
        .checked_add(4)?
        .checked_add(padding)?;
    let bytes = nodes.checked_mul(node_bytes)?;
    let operations = entries.checked_add(nodes)?.checked_mul(2)?;
    Some((operations, bytes))
}

pub fn hash_backing<K, V>(capacity: usize) -> Option<(usize, usize)> {
    if capacity == 0 {
        return Some((0, 0));
    }
    let buckets = capacity.checked_add(1)?.checked_next_power_of_two()?.max(4);
    let alignment = align_of::<(K, V)>().max(64);
    let bytes = size_of::<(K, V)>()
        .checked_add(1)?
        .checked_mul(buckets)?
        .checked_add(alignment)?
        .checked_add(64)?;
    Some((buckets.checked_mul(2)?, bytes))
}
