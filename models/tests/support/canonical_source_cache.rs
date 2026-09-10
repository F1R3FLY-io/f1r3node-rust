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
