use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_source_tree::tests::SourcePhysicalInventory;

impl FrozenCanonicalBytes {
    pub(crate) fn observe_physical_storage(&self, inventory: &mut SourcePhysicalInventory) {
        let data = &self.inner.data;
        let identity = Arc::as_ptr(data) as usize;
        if inventory.frozen_indexes.contains_key(&identity) {
            return;
        }
        let storage = std::mem::size_of_val(data.as_ref())
            + std::mem::size_of_val(data.pages.as_ref())
            + std::mem::size_of_val(data.leaves.as_ref())
            + std::mem::size_of_val(data.logical_pages.as_ref());
        inventory.frozen_indexes.insert(identity, storage);
        for page in &data.pages {
            inventory.byte_pages.insert(
                Arc::as_ptr(&page.bytes) as usize,
                (page.bytes.bytes.len(), page.bytes.bytes.capacity()),
            );
            inventory.provenance_pages.insert(
                Arc::as_ptr(&page.provenance) as usize,
                (
                    page.provenance.values.len(),
                    page.provenance.values.capacity(),
                ),
            );
        }
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ArenaPhysicalInventory {
    pub(crate) shared: SourcePhysicalInventory,
    pub(crate) descriptor_slots: usize,
    pub(crate) descriptor_capacity: usize,
    pub(crate) cache_slots: usize,
    pub(crate) cache_capacity: usize,
    pub(crate) weak_cache_entries: usize,
    pub(crate) cached_roots: usize,
    pub(crate) registry_roots: usize,
    pub(crate) completion_roots: usize,
    pub(crate) generated_payload_cells: usize,
    pub(crate) sealed_generated_byte_pages: usize,
    pub(crate) sealed_generated_provenance_pages: usize,
}

impl CanonicalBytesArena<'_> {
    pub(crate) fn physical_inventory(
        &self,
        completions: &[SourceTreeWindow<'_>],
    ) -> ArenaPhysicalInventory {
        let live_cache_roots = self
            .source_cache
            .iter()
            .flatten()
            .filter_map(|entry| entry.root.upgrade())
            .collect::<Vec<_>>();
        let mut inventory = ArenaPhysicalInventory {
            descriptor_slots: self.backend.nodes.len(),
            descriptor_capacity: self.backend.nodes.capacity(),
            cache_slots: self.source_cache.len(),
            cache_capacity: self.source_cache.capacity(),
            weak_cache_entries: self.source_cache.iter().flatten().count(),
            cached_roots: live_cache_roots.len(),
            registry_roots: self.backend.sources.len(),
            completion_roots: completions.len(),
            generated_payload_cells: self.stats.payload_slots,
            sealed_generated_byte_pages: self
                .backend
                .byte_pages
                .iter()
                .filter(|page| Arc::strong_count(page) > 1)
                .count(),
            sealed_generated_provenance_pages: self
                .backend
                .provenance_pages
                .iter()
                .filter(|page| Arc::strong_count(page) > 1)
                .count(),
            ..ArenaPhysicalInventory::default()
        };
        for source in self
            .backend
            .sources
            .iter()
            .chain(&live_cache_roots)
            .chain(completions)
        {
            inventory.shared.observe(source);
        }
        for page in &self.backend.byte_pages {
            inventory.shared.byte_pages.insert(
                Arc::as_ptr(page) as usize,
                (page.bytes.len(), page.bytes.capacity()),
            );
        }
        for page in &self.backend.provenance_pages {
            inventory.shared.provenance_pages.insert(
                Arc::as_ptr(page) as usize,
                (page.values.len(), page.values.capacity()),
            );
        }
        inventory
    }
}

#[test]
fn physical_inventory_deduplicates_registry_cache_and_completion_owners() {
    let mut original = CanonicalBytesArena::new();
    let empty = original.empty();
    let handle = append_range(&mut original, empty, 0..129);
    let frozen = original.freeze(handle);
    let source = SourceTreeWindow::from_frozen(frozen.clone());
    let mut arena = CanonicalBytesArena::new();
    let left = arena.register_source(source.clone()).unwrap();
    let right = arena.register_source(source.clone()).unwrap();
    let first = arena.append_source(arena.empty(), left, 0..129).unwrap();
    let second = arena.append_source(arena.empty(), right, 0..129).unwrap();
    let completions = [
        arena.retained_source_window(first).unwrap(),
        arena.retained_source_window(second).unwrap(),
        source,
    ];
    let inventory = arena.physical_inventory(&completions);
    assert_eq!(inventory.registry_roots, 2);
    assert_eq!(inventory.cached_roots, 2);
    assert_eq!(inventory.completion_roots, 3);
    assert_eq!(inventory.shared.nodes.len(), 1);
    assert_eq!(inventory.shared.frozen_indexes.len(), 1);
    assert_eq!(inventory.shared.byte_pages.len(), 3);
    assert_eq!(inventory.shared.provenance_pages.len(), 3);
    assert_eq!(inventory.generated_payload_cells, 0);
    assert_eq!(inventory.sealed_generated_byte_pages, 0);
    assert_eq!(arena.physical_inventory(&completions), inventory);
    drop(original);
    drop(arena);
    let mut retained = SourcePhysicalInventory::default();
    for root in &completions {
        retained.observe(root);
    }
    assert_eq!(retained, inventory.shared);
    assert_eq!(
        completions[0].located_bytes().collect::<Vec<_>>(),
        (0..129).map(located).collect::<Vec<_>>()
    );
}

#[test]
fn retained_prefix_physical_inventory_records_geometric_metadata_growth() {
    for width in [16, 64, 256] {
        let input = [19];
        let mut arena = CanonicalBytesArena::new();
        let slot = arena
            .register_source(SourceTreeWindow::from_borrowed(&input, 1, 900).unwrap())
            .unwrap();
        let mut handle = arena.empty();
        let mut completions = Vec::new();
        let mut expected = Vec::new();
        for index in 0..width {
            handle = arena.append_source(handle, slot, 0..1).unwrap();
            handle = arena.append(handle, located(index)).unwrap();
            expected.push(CanonicalLocatedByte::new(
                19,
                CanonicalByteProvenance::new(1, 900),
            ));
            expected.push(located(index));
            completions.push(arena.retained_source_window(handle).unwrap());
        }
        let inventory = arena.physical_inventory(&completions);
        let cache_only = arena.physical_inventory(&[]);
        assert!(cache_only.shared.nodes.is_subset(&inventory.shared.nodes));
        assert!(cache_only.weak_cache_entries <= cache_only.cache_slots);
        assert!(cache_only.cached_roots <= cache_only.weak_cache_entries);
        assert_eq!(cache_only.completion_roots, 0);
        let mut final_only = SourcePhysicalInventory::default();
        final_only.observe(completions.last().unwrap());
        assert_eq!(inventory.descriptor_slots, width * 2);
        assert!(inventory.cached_roots <= inventory.weak_cache_entries);
        assert_eq!(inventory.registry_roots, 1);
        assert_eq!(inventory.generated_payload_cells, width);
        assert_eq!(inventory.shared.byte_pages.len(), width);
        assert_eq!(inventory.sealed_generated_byte_pages, width);
        assert_eq!(inventory.sealed_generated_provenance_pages, width);
        assert_eq!(inventory.shared.frozen_indexes.len(), width);
        assert_eq!(inventory.shared.borrowed_slices.len(), 1);
        assert!(inventory.shared.nodes.len() > final_only.nodes.len());
        assert_eq!(arena.physical_inventory(&completions), inventory);
        println!("source_physical_prefixes width={width} descriptors={} cache_roots={} all_nodes={} final_nodes={} node_storage_bytes={} frozen_index_storage_bytes={} byte_capacity={} provenance_capacity_cells={} generated_cells={} sealed_pages={}",
            inventory.descriptor_slots, inventory.cached_roots, inventory.shared.nodes.len(),
            final_only.nodes.len(), inventory.shared.node_storage_bytes,
            inventory.shared.frozen_indexes.values().sum::<usize>(),
            inventory.shared.byte_pages.values().map(|(_, capacity)| capacity).sum::<usize>(),
            inventory.shared.provenance_pages.values().map(|(_, capacity)| capacity).sum::<usize>(),
            inventory.generated_payload_cells, inventory.sealed_generated_byte_pages);
        drop(arena);
        for (index, root) in completions.iter().enumerate() {
            assert_eq!(
                root.located_bytes().collect::<Vec<_>>(),
                expected[..2 * (index + 1)]
            );
        }
        let final_root = completions.pop().unwrap();
        drop(completions);
        let mut surviving = SourcePhysicalInventory::default();
        surviving.observe(&final_root);
        assert_eq!(surviving, final_only);
        assert!(surviving.nodes.len() < inventory.shared.nodes.len());
    }
}

#[test]
fn released_pre_carry_forks_do_not_amplify_cache_only_source_nodes() {
    const CARRY_PREDECESSORS: [usize; 4] = [2, 8, 26, 80];

    fn measure(forks_per_boundary: usize) -> (usize, usize, usize, usize) {
        let input = [11u8, 22, 33];
        let external = SourceTreeWindow::from_borrowed(&input, 17, 700).unwrap();
        let mut arena = CanonicalBytesArena::new();
        let source = arena.register_source(external.clone()).unwrap();
        let mut trunk = arena.empty();
        let mut expected = Vec::new();

        for width in 1..=*CARRY_PREDECESSORS.last().unwrap() {
            trunk = arena.append_source(trunk, source, 0..1).unwrap();
            expected.push(CanonicalLocatedByte::new(
                11,
                CanonicalByteProvenance::new(17, 700),
            ));
            if !CARRY_PREDECESSORS.contains(&width) {
                continue;
            }

            let mut historical = Vec::new();
            for fork_index in 0..forks_per_boundary {
                let source_offset = 1 + fork_index % 2;
                let fork = arena
                    .append_source(trunk, source, source_offset..source_offset + 1)
                    .unwrap();
                let mut oracle = expected.clone();
                oracle.push(CanonicalLocatedByte::new(
                    input[source_offset],
                    CanonicalByteProvenance::new(17, 700 + source_offset),
                ));
                let completion = arena.retained_source_window(fork).unwrap();
                assert_eq!(arena.located_bytes(fork).collect::<Vec<_>>(), oracle);
                assert_eq!(completion.located_bytes().collect::<Vec<_>>(), oracle);
                historical.push((fork, completion, oracle));
            }
            let during = arena.physical_inventory(
                &historical
                    .iter()
                    .map(|(_, root, _)| root.clone())
                    .collect::<Vec<_>>(),
            );
            assert_eq!(during.completion_roots, forks_per_boundary);
            for (handle, root, oracle) in &historical {
                assert_eq!(arena.located_bytes(*handle).collect::<Vec<_>>(), *oracle);
                assert_eq!(root.located_bytes().collect::<Vec<_>>(), *oracle);
            }
            drop(historical);
        }

        let final_root = arena.retained_source_window(trunk).unwrap();
        assert_eq!(arena.located_bytes(trunk).collect::<Vec<_>>(), expected);
        assert_eq!(final_root.located_bytes().collect::<Vec<_>>(), expected);

        let mut live = SourcePhysicalInventory::default();
        live.observe(&external);
        live.observe(&final_root);
        let cache_only = arena.physical_inventory(&[]);
        assert_eq!(cache_only.registry_roots, 1);
        assert_eq!(cache_only.completion_roots, 0);
        assert_eq!(cache_only.generated_payload_cells, 0);
        assert_eq!(cache_only.shared.borrowed_slices, live.borrowed_slices);
        assert_eq!(cache_only.shared.frozen_indexes, live.frozen_indexes);
        assert_eq!(cache_only.shared.byte_pages, live.byte_pages);
        assert_eq!(cache_only.shared.provenance_pages, live.provenance_pages);
        assert!(live.nodes.is_subset(&cache_only.shared.nodes));
        let cache_only_nodes = cache_only.shared.nodes.difference(&live.nodes).count();
        let cache_only_bytes = cache_only
            .shared
            .node_storage_bytes
            .checked_sub(live.node_storage_bytes)
            .unwrap();
        let counts = (
            live.nodes.len(),
            cache_only.shared.nodes.len(),
            cache_only_nodes,
            cache_only_bytes,
        );
        drop(arena);
        assert_eq!(final_root.located_bytes().collect::<Vec<_>>(), expected);
        assert_eq!(external.located_bytes().collect::<Vec<_>>(), [
            CanonicalLocatedByte::new(11, CanonicalByteProvenance::new(17, 700)),
            CanonicalLocatedByte::new(22, CanonicalByteProvenance::new(17, 701)),
            CanonicalLocatedByte::new(33, CanonicalByteProvenance::new(17, 702)),
        ]);
        counts
    }

    let light = measure(2);
    let heavy = measure(12);
    println!(
        "pre_carry_fork_storage light_live={} light_cached={} light_extra={} light_extra_bytes={} heavy_live={} heavy_cached={} heavy_extra={} heavy_extra_bytes={}",
        light.0, light.1, light.2, light.3, heavy.0, heavy.1, heavy.2, heavy.3
    );
    assert_eq!(
        light.0, heavy.0,
        "the final and external roots must be identical"
    );
    assert_eq!(light.2, 0, "light fork workload retained cache-only nodes");
    assert_eq!(heavy.2, 0, "heavy fork workload retained cache-only nodes");
    assert_eq!(
        light.3, 0,
        "light fork workload retained cache-only node bytes"
    );
    assert_eq!(
        heavy.3, 0,
        "heavy fork workload retained cache-only node bytes"
    );
}

fn located(index: usize) -> CanonicalLocatedByte {
    CanonicalLocatedByte::new(
        index as u8,
        CanonicalByteProvenance::new((index % 7) as u32, 103 + index * 3),
    )
}

fn append_range(
    arena: &mut CanonicalBytesArena,
    mut handle: CanonicalBytesHandle,
    range: Range<usize>,
) -> CanonicalBytesHandle {
    for index in range {
        handle = arena.append(handle, located(index)).unwrap();
    }
    handle
}

#[test]
fn source_segments_retain_borrowed_bytes_without_owned_payload_cells() {
    let input = (0..129).map(|i| i as u8).collect::<Vec<_>>();
    let source = SourceTreeWindow::from_borrowed_range(&input, 0..input.len(), 9, 700).unwrap();
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(source).unwrap();
    let handle = arena.append_source(arena.empty(), slot, 13..97).unwrap();
    assert_eq!(arena.stats().payload_slots, 0);
    assert_eq!(arena.stats().byte_pages, 0);
    assert_eq!(arena.stats().provenance_pages, 0);
    assert_eq!(arena.backend.nodes.len(), 1);
    let exported = arena.retained_source_window(handle).unwrap();
    assert_eq!(arena.stats().source_segment_exports, 0);
    drop(arena);
    assert_eq!(
        exported.contiguous_bytes(0..84).unwrap().as_ptr(),
        input[13..].as_ptr()
    );
    for (i, byte) in exported.located_bytes().enumerate() {
        assert_eq!(
            byte,
            CanonicalLocatedByte::new(input[13 + i], CanonicalByteProvenance::new(9, 713 + i))
        );
    }
}

#[test]
fn source_append_refusals_and_empty_ranges_do_not_mutate_storage() {
    let input = [5, 8, 13];
    let mut arena = CanonicalBytesArena::new();
    let slot = arena
        .register_source(SourceTreeWindow::from_borrowed_range(&input, 0..3, 0, 0).unwrap())
        .unwrap();
    let handle = arena.append(arena.empty(), located(0)).unwrap();
    let before = arena.stats();
    let nodes = arena.backend.nodes.len();
    assert_eq!(arena.append_source(handle, slot, 2..2).unwrap(), handle);
    assert!(matches!(
        arena.append_source(handle, CanonicalSourceSlot(usize::MAX), 0..1),
        Err(CanonicalBytesArenaError::UnknownSource { .. })
    ));
    for range in [2..1, 0..4, usize::MAX..usize::MAX] {
        assert!(matches!(
            arena.append_source(handle, slot, range),
            Err(CanonicalBytesArenaError::SliceOutsideSequence { .. })
        ));
    }
    let overflowing = CanonicalBytesHandle {
        sequence: Some(SequenceSpan {
            root: handle.sequence.unwrap().root,
            start: 0,
            len: usize::MAX,
        }),
    };
    assert_eq!(
        arena.append_source(overflowing, slot, 0..1),
        Err(CanonicalBytesArenaError::LengthOverflow)
    );
    assert_eq!(arena.stats(), before);
    assert_eq!(arena.backend.nodes.len(), nodes);
    assert!(arena.source_cache.is_empty());
    assert_eq!(Arc::strong_count(&arena.backend.byte_pages[0]), 1);
}

#[test]
fn source_offsets_subtract_prefix_before_adding_large_root_position() {
    let input = [42];
    let mut source = SourceTreeWindow::from_borrowed_range(&input, 0..1, 7, usize::MAX).unwrap();
    for _ in 0..usize::BITS - 1 {
        source = source.concat(&source).unwrap();
    }
    let len = source.len();
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(source).unwrap();
    let head = arena.append_source(arena.empty(), slot, 0..len).unwrap();
    let middle = arena.append(head, located(1)).unwrap();
    let tail = arena.append_source(middle, slot, len - 1..len).unwrap();
    let selected = arena.slice(tail, len..len + 2).unwrap();
    let oracle = [
        located(1),
        CanonicalLocatedByte::new(42, CanonicalByteProvenance::new(7, usize::MAX)),
    ];
    assert_eq!(arena.located_bytes(selected).collect::<Vec<_>>(), oracle);
    assert_eq!(
        arena
            .retained_source_window(selected)
            .unwrap()
            .located_bytes()
            .collect::<Vec<_>>(),
        oracle
    );
    assert_eq!(arena.stats().payload_slots, 1);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn mixed_source_histories_match_located_oracle(
        input in prop::collection::vec(any::<u8>(), 1..100),
        operations in prop::collection::vec((0u8..4, any::<usize>(), any::<usize>()), 1..90),
    ) {
        let mut arena = CanonicalBytesArena::new();
        let slot = arena.register_source(SourceTreeWindow::from_borrowed_range(
            &input, 0..input.len(), 11, 100).unwrap()).unwrap();
        let mut histories = vec![(arena.empty(), Vec::<CanonicalLocatedByte>::new())];
        let mut retained = Vec::new();
        for (kind, x, y) in operations {
            let (handle, mut oracle) = histories[x % histories.len()].clone();
            let next = match kind {
                0 => {
                    let byte = located(y % 200);
                    oracle.push(byte);
                    arena.append(handle, byte).unwrap()
                }
                1 => {
                    let a = x % (input.len() + 1);
                    let b = y % (input.len() + 1);
                    let range = a.min(b)..a.max(b);
                    oracle.extend(range.clone().map(|i| CanonicalLocatedByte::new(
                        input[i], CanonicalByteProvenance::new(11, 100 + i))));
                    arena.append_source(handle, slot, range).unwrap()
                }
                2 => {
                    let a = x % (oracle.len() + 1);
                    let b = y % (oracle.len() + 1);
                    let range = a.min(b)..a.max(b);
                    oracle = oracle[range.clone()].to_vec();
                    arena.slice(handle, range).unwrap()
                }
                _ => {
                    let (other, bytes) = &histories[y % histories.len()];
                    oracle.extend_from_slice(bytes);
                    arena.concat(handle, *other).unwrap()
                }
            };
            prop_assert_eq!(arena.located_bytes(next).collect::<Vec<_>>(), oracle.clone());
            let view = arena.retained_source_window(next).unwrap();
            prop_assert_eq!(view.located_bytes().collect::<Vec<_>>(), oracle.clone());
            retained.push((view, oracle.clone()));
            histories.push((next, oracle));
        }
        for (handle, oracle) in &histories {
            let actual = arena.located_bytes(*handle).collect::<Vec<_>>();
            prop_assert_eq!(actual.as_slice(), oracle.as_slice());
        }
        drop(arena);
        for (view, oracle) in retained {
            prop_assert_eq!(view.located_bytes().collect::<Vec<_>>(), oracle);
        }
    }
}

#[test]
fn mixed_original_composed_sources_preserve_every_selected_range() {
    let input = [3, 5, 8, 13];
    let borrowed = SourceTreeWindow::from_borrowed_range(&input, 0..4, 5, 99).unwrap();
    let frozen = {
        let mut producer = CanonicalBytesArena::new();
        let empty = producer.empty();
        let handle = append_range(&mut producer, empty, 50..120);
        SourceTreeWindow::from_frozen(producer.freeze(handle))
    };
    let original = borrowed.concat(&frozen).unwrap().concat(&borrowed).unwrap();
    let expected = original.located_bytes().collect::<Vec<_>>();
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(original.clone()).unwrap();
    assert!(arena.backend.sources[slot.0].shares_root_with(&original));
    let handle = arena
        .append_source(arena.empty(), slot, 0..original.len())
        .unwrap();
    let generated = arena.append(handle, located(123)).unwrap();
    let full = arena.append_source(generated, slot, 1..7).unwrap();
    let oracle = expected
        .iter()
        .copied()
        .chain([located(123)])
        .chain(expected[1..7].iter().copied())
        .collect::<Vec<_>>();
    assert_eq!(arena.stats().payload_slots, 1);
    let retained = arena.retained_source_window(full).unwrap();
    for start in 0..=oracle.len() {
        for end in start..=oracle.len() {
            let selected = arena.slice(full, start..end).unwrap();
            assert_eq!(
                arena.located_bytes(selected).collect::<Vec<_>>(),
                oracle[start..end]
            );
            assert_eq!(
                arena
                    .retained_source_window(selected)
                    .unwrap()
                    .located_bytes()
                    .collect::<Vec<_>>(),
                oracle[start..end]
            );
        }
    }
    drop(arena);
    drop(original);
    drop(frozen);
    assert_eq!(retained.located_bytes().collect::<Vec<_>>(), oracle);
}

#[test]
fn direct_owned_segment_exports_match_independent_legacy_freezer() {
    fn check<const CAPACITY: usize>() {
        let mut arena = PageBackedArena::<CAPACITY>::new();
        let mut handle = arena.empty();
        for i in 0..CAPACITY * 3 + 7 {
            handle = arena.append(handle, located(i)).unwrap();
        }
        let prefix = arena.slice(handle, 3..CAPACITY + 1).unwrap();
        let _fork = arena.append(prefix, located(200)).unwrap();
        for (index, node) in arena.nodes.iter().enumerate() {
            let SequenceNodeKind::Segment { previous, used, .. } = node.kind else {
                continue;
            };
            let start = previous.map_or(0, |p| p.len);
            let legacy = arena.freeze(CanonicalBytesHandle {
                sequence: Some(SequenceSpan {
                    root: SequenceNodeId(index),
                    start,
                    len: used,
                }),
            });
            let direct = arena.freeze_owned_segment(SequenceNodeId(index));
            assert!(direct.content_eq(&legacy));
            assert_eq!(direct.len(), used);
            for offset in 0..used {
                assert_eq!(direct.provenance_at(offset), legacy.provenance_at(offset));
            }
            assert_eq!(direct.data.pages.len(), 1);
            assert_eq!(direct.data.leaves.len(), 1);
        }
    }
    check::<32>();
    check::<64>();
    check::<128>();
    check::<256>();
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]
    #[test]
    fn production_single_leaf_slots_preserve_generated_bytes_and_origins(
        input in prop::collection::vec((any::<u8>(), any::<u32>(), any::<usize>()), 1..193),
        first in any::<u16>(),
        second in any::<u16>(),
        inner in any::<u16>(),
    ) {
        let mut arena = PageBackedArena::<DEFAULT_PAGE_BYTES>::new();
        let mut handle = arena.empty();
        for &(byte, source, offset) in &input {
            handle = arena.append(
                handle,
                CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(source, offset)),
            ).unwrap();
        }
        let mut segments = Vec::new();
        for (index, node) in arena.nodes.iter().enumerate() {
            let SequenceNodeKind::Segment { previous, used, .. } = node.kind else {
                continue;
            };
            segments.push((
                arena.freeze_owned_segment(SequenceNodeId(index)),
                previous.map_or(0, |span| span.len),
                used,
            ));
        }
        let _extension = arena.append(
            handle,
            CanonicalLocatedByte::new(0, CanonicalByteProvenance::new(0, 0)),
        ).unwrap();
        drop(arena);

        for (segment, start, used) in segments {
            let FrozenRange::One(view) = segment.range() else {
                panic!("production segment must retain a verified one-leaf range")
            };
            prop_assert_eq!(view.start(), 0);
            prop_assert_eq!(view.len(), used);
            prop_assert_eq!(view.data_len(), used);
            prop_assert_eq!(view.page_start(), segment.data.leaves[0].page_start);
            let page = &segment.data.pages[0];
            let bound = view.bind(
                page.bytes.bytes.as_slice(),
                page.provenance.values.as_slice(),
            ).unwrap();
            for offset in 0..used {
                let (byte, source, physical) = input[start + offset];
                let expected = CanonicalLocatedByte::new(
                    byte, CanonicalByteProvenance::new(source, physical),
                );
                prop_assert_eq!(bound.at(offset).map(|(byte, provenance)| {
                    CanonicalLocatedByte::new(byte, *provenance)
                }), Some(expected));
                prop_assert_eq!(segment.single_leaf_at(offset), Some(expected));
                prop_assert_eq!(segment.byte_at(offset), Some(expected.byte));
                prop_assert_eq!(segment.provenance_at(offset), Some(expected.provenance));
            }
            let a = usize::from(first) % (used + 1);
            let b = usize::from(second) % (used + 1);
            let (lo, hi) = (a.min(b), a.max(b));
            let expected_slice = input[start + lo..start + hi]
                .iter()
                .map(|(byte, _, _)| *byte)
                .collect::<Vec<_>>();
            prop_assert_eq!(bound.selected_slice(lo, hi), Some(expected_slice.as_slice()));
            let selected = segment.subview(lo..hi).unwrap();
            let inner_start = usize::from(inner) % (selected.len() + 1);
            let nested = selected.subview(inner_start..selected.len()).unwrap();
            let FrozenRange::One(nested_view) = nested.range() else {
                panic!("production subview must retain a verified one-leaf range")
            };
            prop_assert_eq!(nested_view.start(), lo + inner_start);
            prop_assert_eq!(nested_view.len(), hi - lo - inner_start);
            prop_assert_eq!(
                nested.contiguous_bytes(0..nested.len()),
                Some(&expected_slice[inner_start..]),
            );
            for offset in 0..nested.len() {
                let (byte, source, physical) = input[start + lo + inner_start + offset];
                let expected = CanonicalLocatedByte::new(
                    byte, CanonicalByteProvenance::new(source, physical),
                );
                prop_assert_eq!(nested.single_leaf_at(offset), Some(expected));
                prop_assert_eq!(nested.byte_at(offset), Some(expected.byte));
                prop_assert_eq!(nested.provenance_at(offset), Some(expected.provenance));
            }
            let cut = usize::from(first) % (used + 1);
            let frozen = FrozenCanonicalBytes {
                inner: segment.clone(),
            };
            let expected_located = input[start..start + used]
                .iter()
                .map(|(byte, source, physical)| {
                    CanonicalLocatedByte::new(
                        *byte,
                        CanonicalByteProvenance::new(*source, *physical),
                    )
                })
                .collect::<Vec<_>>();
            prop_assert_eq!(frozen.located_bytes().collect::<Vec<_>>(), expected_located.clone());
            let mut cursor = frozen.located_bytes();
            cursor.fast_forward(cut);
            prop_assert_eq!(cursor.len(), used - cut);
            prop_assert_eq!(cursor.collect::<Vec<_>>(), expected_located[cut..].to_vec());
            let left = frozen.subview(0..cut).unwrap();
            let right = frozen.subview(cut..used).unwrap();
            let joined = left.try_coalesce(&right).unwrap();
            let FrozenRange::One(joined_view) = joined.inner.range() else {
                panic!("production coalescing must retain a verified one-leaf range")
            };
            prop_assert_eq!(joined_view.start(), 0);
            prop_assert_eq!(joined_view.len(), used);
            let expected_all = input[start..start + used]
                .iter()
                .map(|(byte, _, _)| *byte)
                .collect::<Vec<_>>();
            let mut byte_cursor = frozen.bytes();
            prop_assert_eq!(byte_cursor.chunk(), Some(expected_all.as_slice()));
            byte_cursor.fast_forward(cut);
            prop_assert_eq!(
                byte_cursor.chunk(),
                (cut < used).then_some(&expected_all[cut..]),
            );
            prop_assert_eq!(byte_cursor.collect::<Vec<_>>(), expected_all[cut..].to_vec());
            prop_assert_eq!(joined.contiguous_bytes(0..used), Some(expected_all.as_slice()));
            let mut copied = vec![0; used];
            prop_assert!(frozen.copy_exact(0, &mut copied));
            prop_assert_eq!(copied, expected_all.clone());
            prop_assert!(frozen.content_eq(&joined));
            prop_assert_eq!(joined.located_bytes().collect::<Vec<_>>(), expected_located);
            prop_assert!(joined.shares_backing_with(&frozen));
            prop_assert_eq!(joined.len(), segment.len());
            for offset in 0..used {
                prop_assert_eq!(joined.byte_at(offset), segment.byte_at(offset));
                prop_assert_eq!(joined.provenance_at(offset), segment.provenance_at(offset));
            }
            prop_assert_eq!(segment.byte_at(used), None);
            prop_assert_eq!(segment.provenance_at(used), None);
            prop_assert_eq!(nested.byte_at(nested.len()), None);
            prop_assert_eq!(nested.provenance_at(nested.len()), None);
        }
    }
}

#[test]
fn mixed_source_descriptors_export_and_drop_on_an_unchanged_small_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let input = [1, 2, 3, 4];
            let mut arena = CanonicalBytesArena::new();
            let slot = arena
                .register_source(
                    SourceTreeWindow::from_borrowed_range(&input, 0..4, 2, 100).unwrap(),
                )
                .unwrap();
            let mut handle = arena.empty();
            for i in 0..4096 {
                handle = arena.append_source(handle, slot, 1..3).unwrap();
                handle = arena.append(handle, located(i)).unwrap();
            }
            let exported = arena.retained_source_window(handle).unwrap();
            assert_eq!(exported.len(), 4096 * 3);
            assert_eq!(arena.stats().payload_slots, 4096);
            assert_eq!(arena.bytes(handle).count(), exported.len());
            drop(arena);
            assert_eq!(exported.located_bytes().count(), 4096 * 3);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn retained_root_exports_every_range_and_releases_cache_only_ownership() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let handle = append_range(&mut arena, empty, 0..193);
    let oracle = (0..193).map(located).collect::<Vec<_>>();
    let full = arena.retained_source_window(handle).unwrap();
    assert_eq!(full.located_bytes().collect::<Vec<_>>(), oracle);
    let payload_slots = arena.stats().payload_slots;
    let page_count = arena.stats().byte_pages;
    assert_eq!(page_count, 193_usize.div_ceil(DEFAULT_PAGE_BYTES));
    for start in 0..=193 {
        for end in start..=193 {
            let slice = arena.slice(handle, start..end).unwrap();
            let selected = arena.retained_source_window(slice).unwrap();
            assert_eq!(
                selected.located_bytes().collect::<Vec<_>>(),
                oracle[start..end]
            );
        }
    }
    assert_eq!(full.located_bytes().collect::<Vec<_>>(), oracle);
    assert_eq!(arena.stats().payload_slots, payload_slots);
    assert_eq!(arena.stats().byte_pages, page_count);
    let active = arena.physical_inventory(&[full.clone()]);
    assert_eq!(active.sealed_generated_byte_pages, page_count);
    assert_eq!(active.sealed_generated_provenance_pages, page_count);
    drop(full);
    let released = arena.physical_inventory(&[]);
    assert_eq!(released.sealed_generated_byte_pages, 0);
    assert_eq!(released.sealed_generated_provenance_pages, 0);
    assert!(released.shared.nodes.is_empty());
    assert!(released.shared.frozen_indexes.is_empty());
    assert_eq!(arena.located_bytes(handle).collect::<Vec<_>>(), oracle);
}

#[test]
fn empty_and_invalid_exports_do_not_publish_cache_or_seal_pages() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let handle = append_range(&mut arena, empty, 0..12);
    let before = arena.stats();
    assert!(arena.retained_source_window(empty).unwrap().is_empty());
    let original = handle.sequence.unwrap();
    for span in [
        SequenceSpan {
            len: 13,
            ..original
        },
        SequenceSpan {
            start: usize::MAX,
            len: 1,
            ..original
        },
        SequenceSpan {
            root: SequenceNodeId(usize::MAX),
            ..original
        },
    ] {
        assert!(arena
            .retained_source_window(CanonicalBytesHandle {
                sequence: Some(span)
            })
            .is_none());
    }
    assert_eq!(arena.stats(), before);
    assert!(arena.source_cache.is_empty());
    assert_eq!(Arc::strong_count(&arena.backend.byte_pages[0]), 1);
    assert_eq!(Arc::strong_count(&arena.backend.provenance_pages[0]), 1);
}

#[test]
fn retained_publication_preserves_old_forks_and_counts_generated_page_seals() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let original = append_range(&mut arena, empty, 0..12);
    let prefix = arena.slice(original, 3..9).unwrap();
    let before = arena.retained_source_window(prefix).unwrap();
    let exported = arena.stats();
    let full = append_range(&mut arena, original, 12..25);
    let fork = arena.append(prefix, located(93)).unwrap();
    assert_eq!(arena.stats().byte_pages, exported.byte_pages + 1);
    let selected = arena.retained_source_window(fork).unwrap();
    let expected = (3..9).chain([93]).map(located).collect::<Vec<_>>();
    assert_eq!(selected.located_bytes().collect::<Vec<_>>(), expected);
    assert_eq!(
        arena
            .retained_source_window(full)
            .unwrap()
            .located_bytes()
            .collect::<Vec<_>>(),
        (0..25).map(located).collect::<Vec<_>>()
    );
    drop(arena);
    assert_eq!(
        before.located_bytes().collect::<Vec<_>>(),
        (3..9).map(located).collect::<Vec<_>>()
    );
    assert_eq!(selected.located_bytes().collect::<Vec<_>>(), expected);
}

#[test]
fn trimmed_cache_export_retains_only_intersecting_generated_pages() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let original = append_range(&mut arena, empty, 0..192);
    let pages = arena
        .backend
        .byte_pages
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let provenance = arena
        .backend
        .provenance_pages
        .iter()
        .map(Arc::downgrade)
        .collect::<Vec<_>>();
    let handle = arena.slice(original, 70..72).unwrap();
    let selected = arena.retained_source_window(handle).unwrap();
    drop(arena);
    for (index, (bytes, origins)) in pages.iter().zip(&provenance).enumerate() {
        assert_eq!(bytes.upgrade().is_some(), index == 1);
        assert_eq!(origins.upgrade().is_some(), index == 1);
    }
    assert_eq!(selected.located_bytes().collect::<Vec<_>>(), vec![
        located(70),
        located(71)
    ]);
    drop(selected);
    assert!(pages.iter().all(|page| page.upgrade().is_none()));
    assert!(provenance.iter().all(|page| page.upgrade().is_none()));
}

#[test]
fn dropped_export_reuses_page_and_preserves_old_handle_after_unrelated_growth() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let original = append_range(&mut arena, empty, 0..12);
    drop(arena.retained_source_window(original).unwrap());
    assert_eq!(Arc::strong_count(&arena.backend.byte_pages[0]), 1);
    assert_eq!(Arc::strong_count(&arena.backend.provenance_pages[0]), 1);
    let pages_before = arena.stats().byte_pages;
    let extended = arena.append(original, located(12)).unwrap();
    assert_eq!(arena.stats().byte_pages, pages_before);
    for index in 20..200 {
        arena.append(empty, located(index)).unwrap();
    }
    let payload_slots = arena.stats().payload_slots;
    let page_count = arena.stats().byte_pages;
    assert_eq!(
        arena
            .retained_source_window(original)
            .unwrap()
            .located_bytes()
            .collect::<Vec<_>>(),
        (0..12).map(located).collect::<Vec<_>>()
    );
    assert_eq!(
        arena
            .retained_source_window(extended)
            .unwrap()
            .located_bytes()
            .collect::<Vec<_>>(),
        (0..13).map(located).collect::<Vec<_>>()
    );
    assert_eq!(
        arena.located_bytes(original).collect::<Vec<_>>(),
        (0..12).map(located).collect::<Vec<_>>()
    );
    assert_eq!(
        arena.located_bytes(extended).collect::<Vec<_>>(),
        (0..13).map(located).collect::<Vec<_>>()
    );
    assert_eq!(arena.stats().payload_slots, payload_slots);
    assert_eq!(arena.stats().byte_pages, page_count);
    let released = arena.physical_inventory(&[]);
    assert_eq!(released.sealed_generated_byte_pages, 0);
    assert_eq!(released.sealed_generated_provenance_pages, 0);
}

#[test]
fn weak_source_cache_borrowed_subrange_and_page_aliasing_is_safe_under_miri() {
    let input = [4_u8, 5, 6, 7, 8];
    let source = SourceTreeWindow::from_borrowed(&input, 19, 300).unwrap();
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(source).unwrap();
    let borrowed = arena.append_source(arena.empty(), slot, 1..4).unwrap();
    drop(arena.retained_source_window(borrowed).unwrap());

    let first = arena.append(borrowed, located(8)).unwrap();
    drop(arena.retained_source_window(first).unwrap());
    assert_eq!(Arc::strong_count(&arena.backend.byte_pages[0]), 1);
    assert_eq!(Arc::strong_count(&arena.backend.provenance_pages[0]), 1);

    let second = arena.append(first, located(9)).unwrap();
    assert_eq!(arena.stats().byte_pages, 1);
    let old = arena.retained_source_window(first).unwrap();
    let current = arena.retained_source_window(second).unwrap();
    drop(arena);

    let mut expected = vec![
        CanonicalLocatedByte::new(5, CanonicalByteProvenance::new(19, 301)),
        CanonicalLocatedByte::new(6, CanonicalByteProvenance::new(19, 302)),
        CanonicalLocatedByte::new(7, CanonicalByteProvenance::new(19, 303)),
        located(8),
    ];
    assert_eq!(old.located_bytes().collect::<Vec<_>>(), expected);
    expected.push(located(9));
    assert_eq!(current.located_bytes().collect::<Vec<_>>(), expected);
}

#[test]
fn repeated_export_drop_append_one_byte_reuses_generated_pages() {
    let report = std::env::var_os("CANONICAL_SOURCE_CACHE_REPORT").is_some();
    let length = std::env::var("CANONICAL_SOURCE_CACHE_LEN")
        .map(|value| {
            value
                .parse::<usize>()
                .expect("CANONICAL_SOURCE_CACHE_LEN must be usize")
        })
        .unwrap_or(DEFAULT_PAGE_BYTES * 4 + 1);
    assert!(length > 0, "CANONICAL_SOURCE_CACHE_LEN must be positive");
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    let mut oracle = Vec::with_capacity(length);
    let started = std::time::Instant::now();
    for index in 0..length {
        let value = located(index);
        handle = arena.append(handle, value).unwrap();
        oracle.push(value);
        let view = arena.retained_source_window(handle).unwrap();
        assert_eq!(view.len(), oracle.len());
        assert_eq!(view.byte_at(index), Some(value.byte));
        assert_eq!(view.provenance_at(index), Some(value.provenance));
        if !report || index + 1 == length {
            assert_eq!(view.located_bytes().collect::<Vec<_>>(), oracle);
        }
        drop(view);
        if !report {
            let expected_pages = oracle.len().div_ceil(DEFAULT_PAGE_BYTES);
            assert_eq!(
                arena.stats().byte_pages,
                expected_pages,
                "a dropped export sealed a generated byte page at width {}",
                oracle.len()
            );
            assert_eq!(arena.stats().provenance_pages, expected_pages);
            assert_eq!(
                Arc::strong_count(arena.backend.byte_pages.last().unwrap()),
                1
            );
            assert_eq!(
                Arc::strong_count(arena.backend.provenance_pages.last().unwrap()),
                1
            );
        }
    }
    let elapsed_ns = started.elapsed().as_nanos();
    let inventory = arena.physical_inventory(&[]);
    assert_eq!(inventory.sealed_generated_byte_pages, 0);
    assert_eq!(inventory.sealed_generated_provenance_pages, 0);
    assert!(inventory.shared.nodes.is_empty());
    assert!(inventory.shared.frozen_indexes.is_empty());
    assert_eq!(arena.located_bytes(handle).collect::<Vec<_>>(), oracle);
    assert_eq!(arena.stats().payload_slots, length);
    assert_eq!(
        arena.stats().byte_pages,
        length.div_ceil(DEFAULT_PAGE_BYTES)
    );
    if report {
        println!("SOURCE_CACHE_HEADER\tlength\tpage_capacity\tpayload_cells\tbyte_pages\tprovenance_pages\tsealed_byte_pages\tsealed_provenance_pages\tretained_source_nodes\tweak_cache_entries\tbuilt_source_nodes\tsegment_exports\tcache_hits\telapsed_ns");
        println!(
            "SOURCE_CACHE_ROW\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            length,
            DEFAULT_PAGE_BYTES,
            inventory.generated_payload_cells,
            arena.stats().byte_pages,
            arena.stats().provenance_pages,
            inventory.sealed_generated_byte_pages,
            inventory.sealed_generated_provenance_pages,
            inventory.shared.nodes.len(),
            inventory.weak_cache_entries,
            arena.stats().source_cache_nodes,
            arena.stats().source_segment_exports,
            arena.stats().source_cache_hits,
            elapsed_ns,
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, ..ProptestConfig::default() })]
    #[test]
    fn retained_source_cache_matches_independent_historical_operation_traces(
        steps in prop::collection::vec((0u8..4, any::<u16>(), any::<u16>(), any::<u8>()), 1..100),
    ) {
        let mut arena = CanonicalBytesArena::new();
        let mut values = vec![(arena.empty(), Vec::<CanonicalLocatedByte>::new())];
        let mut exported = Vec::new();
        for (kind, first, second, byte) in steps {
            let (handle, oracle) = &values[usize::from(first) % values.len()];
            match kind {
                0 => {
                    let value = CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(
                        u32::from(second), usize::from(first),
                    ));
                    let mut next = oracle.clone();
                    next.push(value);
                    values.push((arena.append(*handle, value).unwrap(), next));
                }
                1 => {
                    let a = usize::from(second) % (oracle.len() + 1);
                    let b = usize::from(byte) % (oracle.len() + 1);
                    let range = a.min(b)..a.max(b);
                    values.push((arena.slice(*handle, range.clone()).unwrap(), oracle[range].to_vec()));
                }
                2 => {
                    let (right, suffix) = &values[usize::from(second) % values.len()];
                    if oracle.len() + suffix.len() <= 2048 {
                        let mut next = oracle.clone();
                        next.extend(suffix);
                        values.push((arena.concat(*handle, *right).unwrap(), next));
                    }
                }
                _ => {
                    let selected = arena.retained_source_window(*handle).unwrap();
                    prop_assert_eq!(selected.located_bytes().collect::<Vec<_>>(), oracle.as_slice());
                    exported.push((selected, oracle.clone()));
                }
            }
            for (old, expected) in &values {
                prop_assert_eq!(arena.located_bytes(*old).collect::<Vec<_>>(), expected.as_slice());
            }
        }
        for (old, expected) in &values {
            let selected = arena.retained_source_window(*old).unwrap();
            prop_assert_eq!(selected.located_bytes().collect::<Vec<_>>(), expected.as_slice());
        }
        drop(arena);
        for (selected, expected) in exported {
            prop_assert_eq!(selected.located_bytes().collect::<Vec<_>>(), expected);
        }
    }
}

#[test]
fn historical_root_survives_bounded_cache_collision_and_arena_drop() {
    const LEN: usize = DEFAULT_PAGE_BYTES * 257 + 1;
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let old = append_range(&mut arena, empty, 0..1);
    let latest = append_range(&mut arena, old, 1..LEN);
    let first = arena.retained_source_window(latest).unwrap();
    let historical = arena.retained_source_window(old).unwrap();
    assert_eq!(historical.located_bytes().collect::<Vec<_>>(), vec![
        located(0)
    ]);
    drop(first);
    drop(historical);
    assert_eq!(
        Arc::strong_count(arena.backend.byte_pages.last().unwrap()),
        1
    );
    assert_eq!(
        Arc::strong_count(arena.backend.provenance_pages.last().unwrap()),
        1
    );

    let latest = arena.retained_source_window(latest).unwrap();
    let old = arena.retained_source_window(old).unwrap();
    drop(arena);
    assert_eq!(old.located_bytes().collect::<Vec<_>>(), vec![located(0)]);
    assert_eq!(
        latest.located_bytes().collect::<Vec<_>>(),
        (0..LEN).map(located).collect::<Vec<_>>()
    );
}

#[test]
fn retained_source_cache_expands_and_drops_deep_descriptors_on_a_small_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut arena = CanonicalBytesArena::new();
            let empty = arena.empty();
            let handle = append_range(&mut arena, empty, 0..100_000);
            let selected = arena.retained_source_window(handle).unwrap();
            assert_eq!(selected.len(), 100_000);
            assert_eq!(selected.byte_at(99_999), Some(99_999usize as u8));
            let before = arena.stats();
            let repeated = arena.retained_source_window(handle).unwrap();
            assert!(selected.shares_root_with(&repeated));
            assert_eq!(
                arena.stats().source_segment_exports,
                before.source_segment_exports
            );
            drop(arena);
            assert_eq!(
                selected.provenance_at(99_999),
                Some(located(99_999).provenance)
            );
            drop(selected);
            drop(repeated);
        })
        .unwrap()
        .join()
        .unwrap();
}
