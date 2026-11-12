use proptest::prelude::*;

use super::*;

fn origin(index: usize) -> CanonicalByteProvenance {
    CanonicalByteProvenance::new(3 + (index % 7) as u32, 17 + index * 11)
}

fn populated(bytes: &[u8]) -> (CanonicalBytesArena<'_>, CanonicalBytesHandle) {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, byte) in bytes.iter().copied().enumerate() {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, origin(index)))
            .unwrap();
    }
    (arena, handle)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn indexed_byte_policy_matches_accumulated_consumed_prefix(
        bytes in prop::collection::vec(any::<u8>(), 0..160),
        consumed in any::<usize>(), first in any::<usize>(), second in any::<usize>(),
    ) {
        let consumed = consumed % (bytes.len() + 1);
        let a = first % (consumed + 1);
        let b = second % (consumed + 1);
        let range = a.min(b)..a.max(b);
        let (full_arena, full) = populated(&bytes);
        let input = IndexedByteRegion::from_frozen(full_arena.freeze(full), origin(bytes.len()));
        let (prefix_arena, prefix) = populated(&bytes[..consumed]);
        let indexed = IndexedPathBytes { input: &input, consumed };
        let reference = ArenaPathBytes { arena: &prefix_arena, bytes: prefix };
        let indexed_span = indexed.slice(range.clone()).unwrap();
        let reference_span = reference.slice(range.clone()).unwrap();
        prop_assert_eq!(indexed.consumed_len(), reference.consumed_len());
        prop_assert_eq!(indexed.span_len(indexed_span), reference.span_len(reference_span));
        prop_assert_eq!(
            input.subview(indexed_span).unwrap().located_bytes().collect::<Vec<_>>(),
            prefix_arena.located_bytes(reference_span).collect::<Vec<_>>()
        );
        for index in 0..consumed {
            prop_assert_eq!(indexed.provenance_at(index), Some(origin(index)));
            prop_assert_eq!(indexed.provenance_at(index), reference.provenance_at(index));
        }
        prop_assert_eq!(indexed.provenance_at(consumed), None);
        if consumed < bytes.len() {
            let future = CanonicalByteRange { start: consumed, end: consumed + 1 };
            prop_assert!(input.subview(future).is_some());
            prop_assert_eq!(indexed.slice(future.start..future.end), None);
        }
    }
}

#[test]
fn indexed_byte_policy_rejects_invalid_bounds_without_endpoint_substitution() {
    let bytes = [1, 2, 3, 4];
    let input = IndexedByteRegion::from_borrowed(&bytes, 7, 19);
    let policy = IndexedPathBytes {
        input: &input,
        consumed: 2,
    };
    for range in [3..2, 0..3, usize::MAX..usize::MAX, 0..usize::MAX] {
        assert_eq!(policy.slice(range), None);
    }
    assert_eq!(
        policy.slice(2..2),
        Some(CanonicalByteRange { start: 2, end: 2 })
    );
    assert_eq!(
        policy.provenance_at(1),
        Some(CanonicalByteProvenance::new(7, 20))
    );
    assert_eq!(policy.provenance_at(2), None);
    assert_eq!(policy.provenance_at(usize::MAX), None);
    let invalid = IndexedPathBytes {
        input: &input,
        consumed: usize::MAX,
    };
    assert_eq!(invalid.slice(0..0), None);
    assert_eq!(invalid.provenance_at(0), None);
    let shifted = IndexedByteRegion::from_borrowed(&bytes, 7, usize::MAX);
    let overflow = IndexedPathBytes {
        input: &shifted,
        consumed: 2,
    };
    assert_eq!(overflow.provenance_at(1), None);
    let mut reducer = CanonicalPathStreamReducer::new();
    reducer.consumed = usize::MAX;
    let before = reducer.stats();
    let error = reducer
        .edge(&input, CanonicalLocatedByte::new(1, origin(0)))
        .unwrap_err();
    assert_eq!(error.kind, CanonicalPathAdmissionErrorKind::SizeOverflow);
    assert_eq!(reducer.consumed_len(), usize::MAX);
    assert_eq!(reducer.stats(), before);
}

#[test]
fn indexed_byte_policy_orders_content_and_reports_current_mismatch_or_endpoint() {
    let bytes = [10, 40, 10, 20, 10, 20, 10];
    let (arena, handle) = populated(&bytes);
    let input = IndexedByteRegion::from_frozen(arena.freeze(handle), origin(bytes.len()));
    let indexed = IndexedPathBytes {
        input: &input,
        consumed: bytes.len(),
    };
    let reference = ArenaPathBytes {
        arena: &arena,
        bytes: handle,
    };
    for (left, right, expected, mismatch) in [
        (0..2, 2..4, std::cmp::Ordering::Greater, Some(origin(3))),
        (2..4, 0..2, std::cmp::Ordering::Less, Some(origin(1))),
        (2..4, 4..6, std::cmp::Ordering::Equal, None),
        (2..4, 6..7, std::cmp::Ordering::Greater, None),
        (6..7, 2..4, std::cmp::Ordering::Less, None),
    ] {
        let il = indexed.slice(left.clone()).unwrap();
        let ir = indexed.slice(right.clone()).unwrap();
        let al = reference.slice(left).unwrap();
        let ar = reference.slice(right).unwrap();
        assert_eq!(indexed.compare(il, ir), expected);
        assert_eq!(reference.compare(al, ar), expected);
        assert_eq!(indexed.first_mismatch_source(il, ir), mismatch);
        assert_eq!(reference.first_mismatch_source(al, ar), mismatch);
        let endpoint = CanonicalByteProvenance::new(91, 702);
        assert_eq!(
            indexed.first_mismatch_source(il, ir).unwrap_or(endpoint),
            mismatch.unwrap_or(endpoint)
        );
    }
}

#[test]
fn indexed_byte_policy_captured_range_survives_source_owner_and_frame_replacement() {
    let bytes = [10, 20, 30, 40];
    let child = {
        let (arena, handle) = populated(&bytes);
        let input = IndexedByteRegion::from_frozen(arena.freeze(handle), origin(bytes.len()));
        let policy = IndexedPathBytes {
            input: &input,
            consumed: 3,
        };
        let mut storage = FrameStorage::<CanonicalByteRange>::new();
        let captured = policy.slice(1..3).unwrap();
        let id = storage.push(Frame::Region {
            remaining: 1,
            previous: Some(captured),
            phase: RegionPhase::PathLength(VarintState::default()),
            next: None,
        });
        storage.finish(false);
        let child = input.subview_with_end(captured, origin(2)).unwrap();
        storage.retire(id);
        storage.finish(false);
        let replacement = storage.push(Frame::Tag {
            escape_allowed: true,
            next: None,
        });
        assert_eq!(id, replacement);
        storage.finish(false);
        child
    };
    assert_eq!(child.located_bytes().collect::<Vec<_>>(), vec![
        CanonicalLocatedByte::new(20, origin(1)),
        CanonicalLocatedByte::new(30, origin(2)),
    ]);
    assert_eq!(child.end_position().physical, origin(2));
}

#[test]
fn indexed_byte_policy_pending_preflight_preserves_consumption_but_dead_edges_advance() {
    let bytes = [ESCAPE, 0, 0xff];
    let input = IndexedByteRegion::from_borrowed(&bytes, 7, 19);
    let mut reducer = CanonicalPathStreamReducer::new();
    for byte in input.located_bytes().take(2) {
        reducer.edge(&input, byte).unwrap();
    }
    let pending = reducer.active_obligation().unwrap();
    let before = reducer.stats();
    let attempted = CanonicalLocatedByte::new(0xff, CanonicalByteProvenance::new(9, 777));
    let error = reducer.edge(&input, attempted).unwrap_err();
    assert_eq!(
        error.kind,
        CanonicalPathAdmissionErrorKind::UnresolvedObligation
    );
    assert_eq!(error.source, CanonicalPathSource::in_source(9, 777));
    assert_eq!(reducer.consumed_len(), 2);
    assert_eq!(reducer.stats(), before);
    let completed = reducer.completed_obligation(&input).unwrap().unwrap();
    assert_eq!(completed.payload, CanonicalByteRange { start: 2, end: 2 });
    assert_eq!(completed.payload_end, CanonicalPathSource::in_source(7, 20));
    reducer
        .resolve_obligation(
            &input,
            pending.obligation.id,
            CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: true,
            },
        )
        .unwrap();
    let refused = reducer
        .value(true, CanonicalPathSource::new(0))
        .unwrap()
        .unwrap_err();
    reducer
        .edge(&input, input.located_bytes().nth(2).unwrap())
        .unwrap();
    assert_eq!(reducer.consumed_len(), 3);
    assert_eq!(
        reducer.value(true, CanonicalPathSource::new(0)),
        Some(Err(refused))
    );
    let before = reducer.stats();
    let error = reducer.edge(&input, attempted).unwrap_err();
    assert_eq!(error.kind, CanonicalPathAdmissionErrorKind::SizeOverflow);
    assert_eq!(reducer.consumed_len(), 3);
    assert_eq!(reducer.stats(), before);
}
