use super::*;
use proptest::prelude::*;

fn located(index: usize) -> CanonicalLocatedByte {
    CanonicalLocatedByte::new(index as u8, CanonicalByteProvenance::new(
        (index % 7) as u32, 103 + index * 3,
    ))
}

fn append_range(arena: &mut CanonicalBytesArena, mut handle: CanonicalBytesHandle,
    range: Range<usize>) -> CanonicalBytesHandle {
    for index in range { handle = arena.append(handle, located(index)).unwrap(); }
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
    assert_eq!(exported.contiguous_bytes(0..84).unwrap().as_ptr(), input[13..].as_ptr());
    for (i, byte) in exported.located_bytes().enumerate() {
        assert_eq!(byte, CanonicalLocatedByte::new(input[13 + i],
            CanonicalByteProvenance::new(9, 713 + i)));
    }
}

#[test]
fn source_append_refusals_and_empty_ranges_do_not_mutate_storage() {
    let input = [5, 8, 13];
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(SourceTreeWindow::from_borrowed_range(
        &input, 0..3, 0, 0).unwrap()).unwrap();
    let handle = arena.append(arena.empty(), located(0)).unwrap();
    let before = arena.stats();
    let nodes = arena.backend.nodes.len();
    assert_eq!(arena.append_source(handle, slot, 2..2).unwrap(), handle);
    assert!(matches!(arena.append_source(handle, CanonicalSourceSlot(usize::MAX), 0..1),
        Err(CanonicalBytesArenaError::UnknownSource { .. })));
    for range in [2..1, 0..4, usize::MAX..usize::MAX] {
        assert!(matches!(arena.append_source(handle, slot, range),
            Err(CanonicalBytesArenaError::SliceOutsideSequence { .. })));
    }
    let overflowing = CanonicalBytesHandle { sequence: Some(SequenceSpan {
        root: handle.sequence.unwrap().root, start: 0, len: usize::MAX,
    }) };
    assert_eq!(arena.append_source(overflowing, slot, 0..1),
        Err(CanonicalBytesArenaError::LengthOverflow));
    assert_eq!(arena.stats(), before);
    assert_eq!(arena.backend.nodes.len(), nodes);
    assert!(arena.source_cache.is_empty());
    assert_eq!(Arc::strong_count(&arena.backend.byte_pages[0]), 1);
}

#[test]
fn source_offsets_subtract_prefix_before_adding_large_root_position() {
    let input = [42];
    let mut source = SourceTreeWindow::from_borrowed_range(&input, 0..1, 7, usize::MAX).unwrap();
    for _ in 0..usize::BITS - 1 { source = source.concat(&source).unwrap(); }
    let len = source.len();
    let mut arena = CanonicalBytesArena::new();
    let slot = arena.register_source(source).unwrap();
    let head = arena.append_source(arena.empty(), slot, 0..len).unwrap();
    let middle = arena.append(head, located(1)).unwrap();
    let tail = arena.append_source(middle, slot, len - 1..len).unwrap();
    let selected = arena.slice(tail, len..len + 2).unwrap();
    let oracle = [located(1), CanonicalLocatedByte::new(42, CanonicalByteProvenance::new(7, usize::MAX))];
    assert_eq!(arena.located_bytes(selected).collect::<Vec<_>>(), oracle);
    assert_eq!(arena.retained_source_window(selected).unwrap().located_bytes().collect::<Vec<_>>(), oracle);
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
    let handle = arena.append_source(arena.empty(), slot, 0..original.len()).unwrap();
    let generated = arena.append(handle, located(123)).unwrap();
    let full = arena.append_source(generated, slot, 1..7).unwrap();
    let oracle = expected.iter().copied().chain([located(123)])
        .chain(expected[1..7].iter().copied()).collect::<Vec<_>>();
    assert_eq!(arena.stats().payload_slots, 1);
    let retained = arena.retained_source_window(full).unwrap();
    for start in 0..=oracle.len() {
        for end in start..=oracle.len() {
            let selected = arena.slice(full, start..end).unwrap();
            assert_eq!(arena.located_bytes(selected).collect::<Vec<_>>(), oracle[start..end]);
            assert_eq!(arena.retained_source_window(selected).unwrap().located_bytes()
                .collect::<Vec<_>>(), oracle[start..end]);
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
        for i in 0..CAPACITY * 3 + 7 { handle = arena.append(handle, located(i)).unwrap(); }
        let prefix = arena.slice(handle, 3..CAPACITY + 1).unwrap();
        let _fork = arena.append(prefix, located(200)).unwrap();
        for (index, node) in arena.nodes.iter().enumerate() {
            let SequenceNodeKind::Segment { previous, used, .. } = node.kind else { continue; };
            let start = previous.map_or(0, |p| p.len);
            let legacy = arena.freeze(CanonicalBytesHandle { sequence: Some(SequenceSpan {
                root: SequenceNodeId(index), start, len: used,
            }) });
            let direct = arena.freeze_owned_segment(SequenceNodeId(index));
            assert!(direct.content_eq(&legacy));
            assert_eq!(direct.len(), used);
            for offset in 0..used { assert_eq!(direct.provenance_at(offset), legacy.provenance_at(offset)); }
            assert_eq!(direct.data.pages.len(), 1);
            assert_eq!(direct.data.leaves.len(), 1);
        }
    }
    check::<32>();
    check::<64>();
    check::<128>();
    check::<256>();
}

#[test]
fn mixed_source_descriptors_export_and_drop_on_an_unchanged_small_stack() {
    std::thread::Builder::new().stack_size(64 * 1024).spawn(|| {
        let input = [1, 2, 3, 4];
        let mut arena = CanonicalBytesArena::new();
        let slot = arena.register_source(SourceTreeWindow::from_borrowed_range(
            &input, 0..4, 2, 100).unwrap()).unwrap();
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
    }).unwrap().join().unwrap();
}

#[test]
fn retained_cache_exports_every_range_without_rebuilding_segment_indexes() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let handle = append_range(&mut arena, empty, 0..193);
    let oracle = (0..193).map(located).collect::<Vec<_>>();
    let full = arena.retained_source_window(handle).unwrap();
    assert_eq!(full.located_bytes().collect::<Vec<_>>(), oracle);
    let qualified = arena.stats();
    assert_eq!(qualified.source_cache_nodes, qualified.segments);
    assert_eq!(qualified.source_segment_exports, qualified.segments);
    assert_eq!(qualified.byte_pages, 4);
    for start in 0..=193 {
        for end in start..=193 {
            let slice = arena.slice(handle, start..end).unwrap();
            let selected = arena.retained_source_window(slice).unwrap();
            assert_eq!(selected.located_bytes().collect::<Vec<_>>(), oracle[start..end]);
        }
    }
    let finished = arena.stats();
    assert_eq!(finished.source_cache_nodes, qualified.source_cache_nodes);
    assert_eq!(finished.source_segment_exports, qualified.source_segment_exports);
    assert_eq!(finished.byte_pages, qualified.byte_pages);
    assert!(finished.source_cache_hits > 0);
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
        SequenceSpan { len: 13, ..original },
        SequenceSpan { start: usize::MAX, len: 1, ..original },
        SequenceSpan { root: SequenceNodeId(usize::MAX), ..original },
    ] {
        assert!(arena.retained_source_window(CanonicalBytesHandle { sequence: Some(span) }).is_none());
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
    assert_eq!(arena.retained_source_window(full).unwrap().located_bytes().collect::<Vec<_>>(),
        (0..25).map(located).collect::<Vec<_>>());
    drop(arena);
    assert_eq!(before.located_bytes().collect::<Vec<_>>(), (3..9).map(located).collect::<Vec<_>>());
    assert_eq!(selected.located_bytes().collect::<Vec<_>>(), expected);
}

#[test]
fn trimmed_cache_export_retains_only_intersecting_generated_pages() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let original = append_range(&mut arena, empty, 0..192);
    let pages = arena.backend.byte_pages.iter().map(Arc::downgrade).collect::<Vec<_>>();
    let provenance = arena.backend.provenance_pages.iter().map(Arc::downgrade).collect::<Vec<_>>();
    let handle = arena.slice(original, 70..72).unwrap();
    let selected = arena.retained_source_window(handle).unwrap();
    drop(arena);
    for (index, (bytes, origins)) in pages.iter().zip(&provenance).enumerate() {
        assert_eq!(bytes.upgrade().is_some(), index == 1);
        assert_eq!(origins.upgrade().is_some(), index == 1);
    }
    assert_eq!(selected.located_bytes().collect::<Vec<_>>(), vec![located(70), located(71)]);
    drop(selected);
    assert!(pages.iter().all(|page| page.upgrade().is_none()));
    assert!(provenance.iter().all(|page| page.upgrade().is_none()));
}

#[test]
fn old_cache_hits_do_not_allocate_slots_after_unrelated_growth() {
    let mut arena = CanonicalBytesArena::new();
    let empty = arena.empty();
    let original = append_range(&mut arena, empty, 0..12);
    drop(arena.retained_source_window(original).unwrap());
    let id = original.sequence.unwrap().root.0;
    let count = arena.source_cache.len();
    let capacity = arena.source_cache.capacity();
    let qualified = arena.stats();
    let extended = arena.append(original, located(12)).unwrap();
    assert_ne!(extended.sequence.unwrap().root, original.sequence.unwrap().root);
    for index in 20..200 { arena.append(empty, located(index)).unwrap(); }
    assert!(arena.backend.nodes.len() > count);
    assert_eq!(arena.backend.nodes[id].len, 12);
    let SequenceNodeKind::Segment { used, .. } = arena.backend.nodes[id].kind else { unreachable!() };
    assert_eq!(used, 12);
    assert_eq!(arena.retained_source_window(original).unwrap().located_bytes().collect::<Vec<_>>(),
        (0..12).map(located).collect::<Vec<_>>());
    assert_eq!(arena.source_cache.len(), count);
    assert_eq!(arena.source_cache.capacity(), capacity);
    assert_eq!(arena.stats().source_cache_nodes, qualified.source_cache_nodes);
    assert_eq!(arena.stats().source_segment_exports, qualified.source_segment_exports);
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
fn retained_source_cache_expands_and_drops_deep_descriptors_on_a_small_stack() {
    std::thread::Builder::new().stack_size(64 * 1024).spawn(|| {
        let mut arena = CanonicalBytesArena::new();
        let empty = arena.empty();
        let handle = append_range(&mut arena, empty, 0..100_000);
        let selected = arena.retained_source_window(handle).unwrap();
        assert_eq!(selected.len(), 100_000);
        assert_eq!(selected.byte_at(99_999), Some(99_999usize as u8));
        let before = arena.stats();
        let repeated = arena.retained_source_window(handle).unwrap();
        assert!(selected.shares_root_with(&repeated));
        assert_eq!(arena.stats().source_segment_exports, before.source_segment_exports);
        drop(arena);
        assert_eq!(selected.provenance_at(99_999), Some(located(99_999).provenance));
        drop(selected);
        drop(repeated);
    }).unwrap().join().unwrap();
}
