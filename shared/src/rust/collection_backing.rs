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

/// Backing growth of a B-tree that grows from `entries` to
/// `entries + additional` entries: the increment of [`tree_backing`].
/// Host-work usage only accumulates, so the charge for an insert into an
/// existing tree is this increment, not the new total; the increments of any
/// sequence of batches add up to the backing of the final tree (C13, DR-77,
/// `IncrementalTreeBacking.incremental_charges_telescope`).
pub fn tree_growth<K, V>(entries: usize, additional: usize) -> Option<(usize, usize)> {
    let total = entries.checked_add(additional)?;
    let (next_operations, next_bytes) = tree_backing::<K, V>(total)?;
    let (prior_operations, prior_bytes) = tree_backing::<K, V>(entries)?;
    Some((
        next_operations.checked_sub(prior_operations)?,
        next_bytes.checked_sub(prior_bytes)?,
    ))
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

/// Largest height of a standard `BTreeMap` with `entries` entries. A
/// `BTreeMap` uses B = 6: every node except the root holds at least five
/// keys, so a tree of height `h` holds at least `2 * 6^(h - 1) - 1` entries
/// (C3, DR-78; `OrderedLookupBound.btree_height_bound`).
pub fn tree_height_bound(entries: usize) -> usize {
    if entries == 0 {
        return 0;
    }
    let mut height = 1_usize;
    let mut power = 6_usize;
    loop {
        let Some(minimum) = power
            .checked_mul(2)
            .and_then(|doubled| doubled.checked_sub(1))
        else {
            return height;
        };
        if minimum > entries {
            return height;
        }
        height += 1;
        let Some(next) = power.checked_mul(6) else {
            return height;
        };
        power = next;
    }
}

/// Upper bound on the key comparisons of one `BTreeMap` search in a map with
/// `entries` entries: a search scans at most 11 keys on each level
/// (`OrderedLookupBound.search_within_size_bound`).
pub fn tree_search_bound(entries: usize) -> usize { tree_height_bound(entries).saturating_mul(11) }

/// Upper bound on the bytes that one standard `BTreeMap` insert moves or
/// writes on one level of the tree. A level moves or writes at most 12
/// key-value slots: a split moves the middle slot and the slots of the new
/// node, then shifts the receiving half and writes the new entry. A level also
/// moves or writes at most 12 edges and rewrites at most 12 child parent links
/// (a parent pointer and a parent index each). A split writes four node
/// lengths and the parent pointer of the node that it allocates. The last
/// level of an insert leaves room for the writes to the map's length and root
/// (D-D1a, DR-103; `FreeMapBindings.insert_level_moves_bounded` and
/// `FreeMapBindings.insert_path_within_charge`).
pub fn tree_insert_level_bytes<K, V>() -> Option<usize> {
    let slots = size_of::<K>()
        .checked_add(size_of::<V>())?
        .checked_mul(12)?;
    let edges = size_of::<usize>().checked_mul(12)?;
    let links = size_of::<usize>()
        .checked_add(size_of::<u16>())?
        .checked_mul(12)?;
    let header = size_of::<usize>().checked_add(size_of::<u16>().checked_mul(4)?)?;
    slots
        .checked_add(edges)?
        .checked_add(links)?
        .checked_add(header)
}

/// Upper bound on the bytes that one `BTreeMap` insert moves or writes when
/// the map holds `entries_after` entries after the insert. The insert touches
/// at most `tree_height_bound(entries_after)` levels, and each level is
/// bounded by [`tree_insert_level_bytes`].
pub fn tree_insert_moves<K, V>(entries_after: usize) -> Option<usize> {
    tree_height_bound(entries_after).checked_mul(tree_insert_level_bytes::<K, V>()?)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::cmp::Ordering;
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;

    thread_local! {
        static COMPARISONS: Cell<usize> = const { Cell::new(0) };
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct CountingKey(u32);

    impl PartialOrd for CountingKey {
        fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
    }

    impl Ord for CountingKey {
        fn cmp(&self, other: &Self) -> Ordering {
            COMPARISONS.with(|count| count.set(count.get() + 1));
            self.0.cmp(&other.0)
        }
    }

    fn counted<T>(action: impl FnOnce() -> T) -> (T, usize) {
        COMPARISONS.with(|count| count.set(0));
        let value = action();
        (value, COMPARISONS.with(Cell::get))
    }

    /// `FreeMapBindings.level_charge_value`: with an `i32` key and a 296-byte
    /// value, one level of an insert costs at most 3,832 bytes, and an insert
    /// is charged one level for each level of the tree after it.
    #[test]
    fn tree_insert_moves_at_the_height_boundaries() {
        type Value = [u8; 296];
        assert_eq!(tree_insert_level_bytes::<i32, Value>(), Some(3_832));
        for (entries, height) in [
            (0, 0),
            (1, 1),
            (10, 1),
            (11, 2),
            (70, 2),
            (71, 3),
            (430, 3),
            (431, 4),
        ] {
            assert_eq!(tree_height_bound(entries), height, "{entries}");
            assert_eq!(
                tree_insert_moves::<i32, Value>(entries),
                Some(height * 3_832),
                "{entries}"
            );
        }
    }

    /// `OrderedLookupBound.btree_size_lower_bound`: the bound changes exactly at
    /// the minimum root sizes `2 * 6^(h - 1) - 1`.
    #[test]
    fn tree_height_bound_changes_at_the_minimum_root_sizes() {
        assert_eq!(tree_height_bound(0), 0);
        let mut power = 1_usize;
        for height in 1..=8 {
            let minimum = 2 * power - 1;
            assert_eq!(tree_height_bound(minimum), height, "minimum size {minimum}");
            if height >= 2 {
                assert_eq!(tree_height_bound(minimum - 1), height - 1);
            }
            power *= 6;
        }
        assert!(tree_height_bound(usize::MAX) <= 25);
    }

    /// `OrderedLookupBound.linear_charge_example`.
    #[test]
    fn linear_lookup_charge_exceeds_the_bound() {
        assert_eq!(tree_height_bound(2_000), 4);
        assert_eq!(tree_search_bound(2_000), 44);
    }

    #[derive(Clone, Copy, Debug)]
    enum Construction {
        Random,
        Ascending,
        Descending,
        Bulk,
        RandomWithRemovals,
    }

    fn construction() -> impl Strategy<Value = Construction> {
        prop_oneof![
            Just(Construction::Random),
            Just(Construction::Ascending),
            Just(Construction::Descending),
            Just(Construction::Bulk),
            Just(Construction::RandomWithRemovals),
        ]
    }

    fn splitmix(state: &mut u64) -> u64 {
        *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = *state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A map built through one public construction path of `BTreeMap`:
    /// single inserts in random, ascending, or descending order, the bulk
    /// build of `collect`, or random inserts followed by removals that
    /// exercise the underflow repair.
    fn build(size: usize, seed: u64, construction: Construction) -> BTreeMap<CountingKey, ()> {
        let mut state = seed;
        let mut keys = Vec::with_capacity(size);
        for _ in 0..size {
            keys.push(splitmix(&mut state) as u32);
        }
        match construction {
            Construction::Ascending => keys.sort_unstable(),
            Construction::Descending => keys.sort_unstable_by(|left, right| right.cmp(left)),
            Construction::Random | Construction::Bulk | Construction::RandomWithRemovals => {}
        }
        let mut map = match construction {
            Construction::Bulk => keys.iter().map(|&key| (CountingKey(key), ())).collect(),
            Construction::Random
            | Construction::Ascending
            | Construction::Descending
            | Construction::RandomWithRemovals => {
                let mut map = BTreeMap::new();
                for &key in &keys {
                    map.insert(CountingKey(key), ());
                }
                map
            }
        };
        if let Construction::RandomWithRemovals = construction {
            for &key in &keys {
                if !splitmix(&mut state).is_multiple_of(3) {
                    map.remove(&CountingKey(key));
                }
            }
        }
        map
    }

    fn assert_lookups_within_bound(
        map: &BTreeMap<CountingKey, ()>,
        probes: &[u32],
    ) -> Result<(), TestCaseError> {
        let bound = tree_search_bound(map.len());
        let stride = (map.len() / 256).max(1);
        let present: Vec<u32> = map.keys().step_by(stride).map(|key| key.0).collect();
        for &probe in probes.iter().chain(present.iter()) {
            let (_, comparisons) = counted(|| map.get(&CountingKey(probe)).is_some());
            prop_assert!(
                comparisons <= bound,
                "{} comparisons > bound {} at {} entries",
                comparisons,
                bound,
                map.len()
            );
        }
        Ok(())
    }

    /// `OrderedLookupBound.search_within_size_bound` at the largest size of
    /// the property below, with every key probed after an ascending build.
    #[test]
    fn std_btree_get_comparisons_within_bound_at_the_largest_size() {
        let map = build(1 << 16, 7, Construction::Ascending);
        let bound = tree_search_bound(map.len());
        let mut largest = 0;
        for key in map.keys() {
            let (_, comparisons) = counted(|| map.contains_key(key));
            largest = largest.max(comparisons);
        }
        assert!(largest <= bound, "{largest} comparisons > bound {bound}");
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `OrderedLookupBound.search_within_size_bound`: one lookup in a
        /// standard `BTreeMap` with at most 2^16 entries makes at most
        /// `tree_search_bound(len)` key comparisons, for every public
        /// construction path above, for present and for absent keys.
        #[test]
        fn std_btree_get_comparisons_within_bound(
            size in prop_oneof![1usize..=64, 65usize..=4_096, 4_097usize..=(1 << 16)],
            seed in any::<u64>(),
            construction in construction(),
            probes in prop::collection::vec(any::<u32>(), 1..64),
        ) {
            let map = build(size, seed, construction);
            assert_lookups_within_bound(&map, &probes)?;
        }
    }
}
