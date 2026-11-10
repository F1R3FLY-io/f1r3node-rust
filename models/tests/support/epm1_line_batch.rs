use proptest::prelude::*;

use super::*;
use crate::rust::rholang::canonical_bytes_arena::{CanonicalBytesArena, CanonicalLocatedByte};

fn assert_authority(
    actual: &IndexedCanonicalEpm1TopologyCursor<'_>,
    expected: &IndexedCanonicalEpm1TopologyCursor<'_>,
) {
    assert_eq!(actual.mode, expected.mode);
    assert_eq!(actual.value_count, expected.value_count);
    assert_eq!(actual.length, expected.length);
    assert_eq!(actual.value_table_start, expected.value_table_start);
    assert_eq!(actual.value_reader.offset(), expected.value_reader.offset());
    assert_eq!(
        actual.value_count_provenance,
        expected.value_count_provenance
    );
    assert_eq!(actual.empty_state, expected.empty_state);
    assert_eq!(actual.empty_provenance, expected.empty_provenance);
    assert_eq!(actual.stats(), expected.stats());
    assert_eq!(
        std::mem::discriminant(&actual.terminal),
        std::mem::discriminant(&expected.terminal)
    );
}

fn event(
    event: IndexedCanonicalEpm1TopologyEvent<'_>,
) -> (
    u8,
    Option<u8>,
    bool,
    Option<Vec<CanonicalLocatedByte>>,
    CanonicalByteProvenance,
) {
    match event {
        IndexedCanonicalEpm1TopologyEvent::Edge { byte, provenance } => {
            (0, Some(byte), false, None, provenance)
        }
        IndexedCanonicalEpm1TopologyEvent::Value {
            has_value,
            value_body,
            provenance,
            ..
        } => (
            1,
            None,
            has_value,
            value_body.map(|body| body.located_bytes().collect()),
            provenance,
        ),
        IndexedCanonicalEpm1TopologyEvent::Leave { provenance } => {
            (2, None, false, None, provenance)
        }
    }
}

fn compare(input: IndexedByteRegion<'_>, requested: usize) -> (usize, usize) {
    let mut actual = IndexedCanonicalEpm1TopologyCursor::new(input.clone()).unwrap();
    let mut expected = IndexedCanonicalEpm1TopologyCursor::new(input).unwrap();
    let retained = actual.source_window().unwrap();
    assert_eq!(
        retained.located_bytes().collect::<Vec<_>>(),
        actual
            .trie
            .as_ref()
            .unwrap()
            .source()
            .located_bytes()
            .collect::<Vec<_>>()
    );
    let mut batches = (0, 0);
    loop {
        let work_source = actual.active_line_work_source();
        let event_index = actual.topology_events;
        let source_position = actual.topology_source_position();
        let committed = {
            actual
                .line_span()
                .map(|span| {
                    let work = work_source
                        .as_ref()
                        .expect("active EPathMap line has ACT work");
                    assert_eq!(work.event_index, event_index);
                    assert_eq!(Some(work.position), source_position);
                    assert_eq!(work.source_range, span.source_range());
                    assert_eq!(work.direction, span.direction());
                    let direction = span.direction();
                    let width = requested.min(span.capacity());
                    assert!(width > 0);
                    let mut last = None;
                    for _ in 0..width {
                        let next = expected.next_event().unwrap().unwrap();
                        match direction {
                            CanonicalCompactTrieLineDirection::Forward => {
                                assert!(matches!(
                                    next,
                                    IndexedCanonicalEpm1TopologyEvent::Edge { .. }
                                ));
                                let absent = expected.next_event().unwrap().unwrap();
                                assert!(matches!(
                                    absent,
                                    IndexedCanonicalEpm1TopologyEvent::Value {
                                        has_value: false,
                                        value_body: None,
                                        ..
                                    }
                                ));
                                last = Some(event(absent).4);
                            }
                            CanonicalCompactTrieLineDirection::Unwind => {
                                assert!(matches!(
                                    next,
                                    IndexedCanonicalEpm1TopologyEvent::Leave { .. }
                                ));
                                last = Some(event(next).4);
                            }
                        }
                    }
                    let source = span.prepare(width).unwrap().commit().0;
                    assert_eq!(Some(source), last);
                    match direction {
                        CanonicalCompactTrieLineDirection::Forward => batches.0 += 1,
                        CanonicalCompactTrieLineDirection::Unwind => batches.1 += 1,
                    }
                })
                .is_some()
        };
        if !committed {
            let left = actual.next_event().unwrap().map(event);
            let right = expected.next_event().unwrap().map(event);
            assert_eq!(left, right);
            if left.is_none() {
                assert_authority(&actual, &expected);
                break;
            }
        }
        assert_authority(&actual, &expected);
    }
    assert!(actual.line_span().is_none());
    batches
}

fn snapshot(map: bool, width: usize) -> Vec<u8> {
    let mut key = vec![0x71; width];
    if map {
        let mut trie = PathMap::new();
        trie.insert(&key, Par::default());
        key.push(0x72);
        trie.insert(&key, Par::default());
        encode(&EPathMapRepr::Map(trie))
    } else {
        let mut trie = PathMap::new();
        trie.insert(&key, ());
        key.push(0x72);
        trie.insert(&key, ());
        encode(&EPathMapRepr::<Par>::Set(trie))
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn prepared_epm_lines_match_scalar_authority(map in any::<bool>(), width in 8usize..300, take in 1usize..400) {
        let bytes = snapshot(map, width);
        let counts = compare(IndexedByteRegion::from_borrowed(&bytes, 73, 1000), take);
        prop_assert!(counts.0 > 0 && counts.1 > 0);
    }
}

#[test]
fn prepared_epm_lines_keep_frozen_and_discontinuous_source_provenance() {
    for map in [false, true] {
        let bytes = snapshot(map, 193);
        let mut arena = CanonicalBytesArena::new();
        let mut handle = arena.empty();
        let mut composed = SourceTreeWindow::empty();
        for (i, byte) in bytes.iter().copied().enumerate() {
            let source = (i % 7) as u32;
            let offset = if i % 13 == 0 {
                usize::MAX
            } else {
                103 - i % 97
            };
            handle = arena
                .append(
                    handle,
                    CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(source, offset)),
                )
                .unwrap();
            composed = composed
                .concat(&SourceTreeWindow::from_borrowed(&bytes[i..i + 1], source, offset).unwrap())
                .unwrap();
        }
        let frozen = SourceTreeWindow::from_frozen(arena.freeze(handle));
        for root in [frozen, composed] {
            for take in [1, 7, usize::MAX] {
                let counts = compare(
                    IndexedByteRegion::from_source(
                        root.clone(),
                        CanonicalByteProvenance::new(99, 101),
                    ),
                    take,
                );
                assert!(counts.0 > 0 && counts.1 > 0);
            }
        }
    }
}

#[test]
fn prepared_epm_declines_preserve_cursor_and_value_authority() {
    let bytes = snapshot(true, 31);
    let input = IndexedByteRegion::from_borrowed(&bytes, 3, 9);
    let mut actual = IndexedCanonicalEpm1TopologyCursor::new(input.clone()).unwrap();
    let mut expected = IndexedCanonicalEpm1TopologyCursor::new(input).unwrap();
    while actual.line_span().is_none() {
        assert_eq!(
            actual.next_event().unwrap().map(event),
            expected.next_event().unwrap().map(event)
        );
    }
    let range = actual.line_span().unwrap().source_range();
    for width in [0, range.len() + 1, usize::MAX] {
        assert!(actual.line_span().unwrap().prepare(width).is_none());
        assert_authority(&actual, &expected);
        assert_eq!(actual.line_span().unwrap().source_range(), range);
    }
    drop(actual.line_span().unwrap().prepare(1).unwrap());
    assert_authority(&actual, &expected);
    assert_eq!(actual.line_span().unwrap().source_range(), range);
    for remaining in [0, 1, 2, 6] {
        actual.topology_events = usize::MAX - remaining;
        let capacity = actual.line_span().unwrap().capacity();
        assert_eq!(capacity, range.len().min(remaining / 2));
        let work = actual.active_line_work_source().unwrap();
        assert_eq!(work.event_index, actual.topology_events);
        assert_eq!(work.source_range, range);
        assert_eq!(work.remaining_work_events, range.len().checked_mul(2));
        assert_eq!(work.admissible_source_width, capacity);
        assert_eq!(work.admissible_work_events, capacity * 2);
        assert!(actual
            .line_span()
            .unwrap()
            .prepare(remaining / 2 + 1)
            .is_none());
        assert_eq!(actual.topology_events, usize::MAX - remaining);
        assert_eq!(actual.line_span().unwrap().source_range(), range);
    }
    actual.topology_events = expected.topology_events;
    assert_eq!(
        actual.next_event().unwrap().map(event),
        expected.next_event().unwrap().map(event)
    );
    loop {
        if actual
            .line_span()
            .is_some_and(|span| span.direction() == CanonicalCompactTrieLineDirection::Unwind)
        {
            break;
        }
        let left = actual.next_event().unwrap().map(event);
        assert!(left.is_some());
        assert_eq!(left, expected.next_event().unwrap().map(event));
    }
    let reverse = actual.line_span().unwrap().source_range();
    drop(actual.line_span().unwrap().prepare(1).unwrap());
    assert_authority(&actual, &expected);
    assert_eq!(actual.line_span().unwrap().source_range(), reverse);
    actual.topology_events = usize::MAX;
    assert_eq!(actual.line_span().unwrap().capacity(), 0);
    assert_eq!(
        actual
            .active_line_work_source()
            .unwrap()
            .admissible_source_width,
        0
    );
    assert!(actual.line_span().unwrap().prepare(1).is_none());
    assert_eq!(actual.line_span().unwrap().source_range(), reverse);
    actual.topology_events = usize::MAX - 1;
    expected.topology_events = usize::MAX - 1;
    assert_eq!(actual.line_span().unwrap().capacity(), 1);
    let work = actual.active_line_work_source().unwrap();
    assert_eq!(work.source_range, reverse);
    assert_eq!(work.admissible_source_width, 1);
    assert_eq!(work.admissible_work_events, 1);
    assert!(actual.line_span().unwrap().prepare(2).is_none());
    assert_authority(&actual, &expected);
    assert_eq!(actual.line_span().unwrap().source_range(), reverse);
    drop(actual.line_span().unwrap().prepare(1).unwrap());
    assert_authority(&actual, &expected);
    let prepared = actual.line_span().unwrap().prepare(1).unwrap();
    let last = event(expected.next_event().unwrap().unwrap()).4;
    assert_eq!(prepared.commit().0, last);
    assert_authority(&actual, &expected);
    assert_eq!(actual.topology_events, usize::MAX);
    for terminal in [
        IndexedCanonicalEpm1Terminal::Done,
        IndexedCanonicalEpm1Terminal::Failed(TrieCodecError::new("test refusal")),
    ] {
        actual.terminal = terminal;
        assert!(actual.line_span().is_none());
    }
    let empty_bytes = encode(&EPathMapRepr::<Par>::Empty);
    let mut empty = IndexedCanonicalEpm1TopologyCursor::new(IndexedByteRegion::from_borrowed(
        &empty_bytes,
        1,
        0,
    ))
    .unwrap();
    assert!(empty.line_span().is_none());
    assert!(empty.source_window().is_none());
}
