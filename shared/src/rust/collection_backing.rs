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

pub fn persistent_insert_backing<K, V>(entries: usize) -> Option<(usize, usize)> {
    let levels = 32_usize.div_ceil(3).checked_add(1)?;
    let count = entries.checked_add(1)?;
    let nodes = if entries == 0 {
        1
    } else {
        levels.checked_mul(count.min(33))?.checked_add(2)?
    };
    let alignment = align_of::<(K, V, u64, [usize; 4])>();
    let slot = size_of::<(K, V, u64, [usize; 4])>().checked_add(alignment.checked_mul(2)?)?;
    let padding = alignment.checked_mul(8)?;
    let node = slot
        .checked_mul(32)?
        .checked_add(padding)?
        .checked_add(128)?;
    let collision = count
        .max(4)
        .checked_mul(4)?
        .checked_mul(size_of::<(K, V)>())?;
    let bytes = nodes.checked_mul(node)?.checked_add(collision)?;
    let operations = count
        .checked_mul(levels)?
        .checked_mul(32)?
        .checked_add(nodes.checked_mul(32)?)?;
    Some((operations, bytes))
}
