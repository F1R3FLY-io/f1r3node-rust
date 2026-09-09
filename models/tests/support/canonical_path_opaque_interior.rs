use std::collections::BTreeMap;

use proptest::prelude::*;

use super::*;

type RangeFrame = Frame<CanonicalByteRange>;
type PhysicalSnapshot = (
    InnerPath,
    usize,
    Vec<RangeFrame>,
    Vec<FrameId>,
    Vec<FrameId>,
    Vec<FrameId>,
    u64,
    CanonicalPathAdmissionStats,
    usize,
    Vec<FocusedRegionOwner>,
);

fn origin(index: usize) -> CanonicalByteProvenance {
    CanonicalByteProvenance::new(5 + (index % 7) as u32, 23 + index * 19)
}

fn input(bytes: &[u8]) -> IndexedByteRegion<'static> {
    let mut arena = CanonicalBytesArena::new();
    let mut handle = arena.empty();
    for (index, byte) in bytes.iter().copied().enumerate() {
        handle = arena
            .append(handle, CanonicalLocatedByte::new(byte, origin(index)))
            .unwrap();
    }
    IndexedByteRegion::from_frozen(arena.freeze(handle), CanonicalByteProvenance::new(91, 811))
}

fn synthetic(
    from: usize,
    payload: u64,
    owners: &[(u64, u64)],
    kind: CanonicalPathObligationKind,
    padding: usize,
) -> CanonicalPathStreamReducer {
    let mut reducer = CanonicalPathStreamReducer::new();
    for _ in 0..padding {
        let id = reducer.machine.frames.push(Frame::Tag {
            escape_allowed: false,
            next: None,
        });
        reducer.machine.frames.retire(id);
    }
    reducer.machine.frames.finish(false);
    let next = reducer.machine.frames.push(Frame::Kids {
        remaining: 3,
        next: None,
    });
    let leaf = reducer.machine.frames.push(Frame::Obligation {
        obligation: CanonicalPathObligation {
            id: CanonicalPathObligationId(73),
            kind,
            payload_len: payload,
            tag_source: CanonicalPathSource::in_source(31, 47),
        },
        status: ObligationStatus::Payload { remaining: payload },
        payload_start: from,
        next: Some(next),
    });
    let mut path = InnerPath {
        escape_allowed: true,
        stack: Some(leaf),
        status: PathStatus::Open,
        completed_segments: 2,
        first_tag: Some((ETUPLE, CanonicalPathSource::in_source(32, 48))),
        stable: false,
    };
    for (index, (region, remaining)) in owners.iter().copied().enumerate().rev() {
        let next = reducer.machine.frames.push(Frame::Kids {
            remaining: 2 + index as u64,
            next: None,
        });
        let owner = reducer.machine.frames.push(Frame::Region {
            remaining: region,
            previous: Some(CanonicalByteRange {
                start: 0,
                end: from.min(2),
            }),
            phase: RegionPhase::Path {
                declared_len: remaining,
                remaining,
                start_depth: from,
                path,
            },
            next: Some(next),
        });
        path = InnerPath {
            escape_allowed: index == 0,
            stack: Some(owner),
            status: PathStatus::Open,
            completed_segments: 1 + index as u64,
            first_tag: Some((
                ETUPLE,
                CanonicalPathSource::in_source(40 + index as u32, 91),
            )),
            stable: index % 2 == 0,
        };
    }
    reducer.current = path;
    reducer.consumed = from;
    reducer.graph_stamp = from;
    reducer.machine.next_obligation = 901;
    reducer.machine.stats.edges = from;
    reducer.machine.stats.max_topology_depth = from;
    reducer.machine.frames.finish(false);
    let initial = (
        reducer.current.clone(),
        reducer.machine.frames.slots.clone(),
    );
    reducer.focus_path();
    assert_eq!(reducer.projected_frame_graph(), initial);
    reducer.assert_frame_invariants();
    reducer
}

fn physical(reducer: &CanonicalPathStreamReducer) -> PhysicalSnapshot {
    let frames = &reducer.machine.frames;
    (
        reducer.current.clone(),
        reducer.consumed_len(),
        frames.slots.clone(),
        frames.free.clone(),
        frames.allocated.clone(),
        frames.retired.clone(),
        reducer.machine.next_obligation,
        reducer.stats(),
        reducer.graph_stamp,
        reducer.owners.clone(),
    )
}

pub(super) fn normalized(
    reducer: &CanonicalPathStreamReducer,
) -> (InnerPath, Vec<RangeFrame>, u64, usize, usize, usize) {
    reducer.assert_frame_invariants();
    let mut pending = Vec::new();
    let mut names = BTreeMap::new();
    let (mut root, projection) = reducer.projected_frame_graph();
    if let Some(id) = root.stack {
        names.insert(id.0, 0);
        pending.push(id);
        root.stack = Some(FrameId(0));
    }
    let mut frames = Vec::new();
    let mut at = 0;
    while at < pending.len() {
        let mut frame = projection[pending[at].0].clone();
        let mut rename = |slot: &mut Option<FrameId>| {
            if let Some(id) = *slot {
                let named = *names.entry(id.0).or_insert_with(|| {
                    let named = pending.len();
                    pending.push(id);
                    named
                });
                *slot = Some(FrameId(named));
            }
        };
        match &mut frame {
            Frame::Tag { next, .. }
            | Frame::Varint { next, .. }
            | Frame::Bytes { next, .. }
            | Frame::Kids { next, .. }
            | Frame::Region { next, .. }
            | Frame::Obligation { next, .. } => rename(next),
        }
        if let Frame::Region {
            phase: RegionPhase::Path { path, .. },
            ..
        } = &mut frame
        {
            rename(&mut path.stack);
        }
        frames.push(frame);
        at += 1;
    }
    (
        root,
        frames,
        reducer.machine.next_obligation,
        reducer.consumed_len(),
        reducer.stats().edges,
        reducer.stats().max_topology_depth,
    )
}

fn expected_width(from: usize, payload: u64, owners: &[(u64, u64)], input_len: usize) -> usize {
    let Some(available) = input_len.checked_sub(from) else {
        return 0;
    };
    let mut smallest = (available as u128).min(u128::from(payload));
    for (region, path) in owners {
        smallest = smallest.min(u128::from(*region));
        smallest = smallest.min(u128::from(*path));
    }
    usize::try_from(smallest.saturating_sub(1)).unwrap()
}

fn scalar_interior(
    reference: &mut CanonicalPathStreamReducer,
    source: &IndexedByteRegion<'_>,
    width: usize,
) {
    let from = reference.consumed_len();
    let id = reference.active_obligation().unwrap().obligation.id;
    for byte in source.located_bytes().skip(from).take(width) {
        assert_eq!(
            reference.edge(source, byte).unwrap(),
            CanonicalPathTransition::ObligationByte {
                id,
                byte: LocatedPathByte::in_source(
                    byte.byte,
                    byte.provenance.source,
                    byte.provenance.offset
                ),
                last: false,
            }
        );
        assert!(!reference.active_obligation().unwrap().awaiting_resolution);
        reference.assert_frame_invariants();
    }
    assert_eq!(reference.consumed_len(), from + width);
}

fn assert_refused(
    reducer: &mut CanonicalPathStreamReducer,
    input_len: usize,
    plan: OpaqueInteriorPlan,
) {
    let before = physical(reducer);
    assert!(!reducer.try_advance_opaque_interior(input_len, plan));
    assert_eq!(physical(reducer), before);
    reducer.assert_frame_invariants();
}

fn set_path_status(reducer: &mut CanonicalPathStreamReducer, depth: usize, status: PathStatus) {
    if let Some(owner) = reducer.owners.get_mut(depth) {
        owner.owner.status = status;
    } else {
        assert_eq!(depth, reducer.owners.len());
        reducer.current.status = status;
    }
    let mut all_open = true;
    for owner in &mut reducer.owners {
        all_open &= owner.owner.status == PathStatus::Open;
        owner.all_open = all_open;
    }
}

fn leaf_id(reducer: &CanonicalPathStreamReducer) -> FrameId {
    let mut path = &reducer.current;
    loop {
        let id = path.stack.unwrap();
        match &reducer.machine.frames.slots[id.0] {
            Frame::Region {
                phase: RegionPhase::Path { path: child, .. },
                ..
            } => path = child,
            _ => return id,
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn opaque_interior_plans_match_independent_all_owner_minimum(
        from in 0usize..9, payload in 0u64..97, available in 0usize..97,
        owners in prop::collection::vec((0u64..97, 0u64..97), 0..9),
        raw in any::<bool>(),
    ) {
        let kind = if raw { CanonicalPathObligationKind::RawParEscape } else { CanonicalPathObligationKind::NestedEpm1 };
        let reducer = synthetic(from, payload, &owners, kind, 0);
        let before = physical(&reducer);
        let width = expected_width(from, payload, &owners, from + available);
        let expected = (width != 0).then_some(OpaqueInteriorPlan { from, id: CanonicalPathObligationId(73), width });
        prop_assert_eq!(reducer.plan_opaque_interior(from + available), expected);
        prop_assert_eq!(physical(&reducer), before);
    }

    #[test]
    fn opaque_interior_matches_independent_scalar_steps_and_public_events(
        bytes in prop::collection::vec(any::<u8>(), 3..80),
        payload in 3u64..97,
        owners in prop::collection::vec((3u64..97, 3u64..97), 0..9),
        choice in any::<usize>(), raw in any::<bool>(),
    ) {
        let kind = if raw { CanonicalPathObligationKind::RawParEscape } else { CanonicalPathObligationKind::NestedEpm1 };
        let mut all = vec![1, 2, 3];
        all.extend_from_slice(&bytes);
        let source = input(&all);
        let mut bulk = synthetic(3, payload, &owners, kind, 0);
        let mut reference = synthetic(3, payload, &owners, kind, 5);
        prop_assert_eq!(normalized(&bulk), normalized(&reference));
        let mut plan = bulk.plan_opaque_interior(source.len()).unwrap();
        prop_assert!(plan.width >= 2);
        plan.width = 1 + choice % plan.width;
        prop_assert!(bulk.try_advance_opaque_interior(source.len(), plan));
        scalar_interior(&mut reference, &source, plan.width);
        prop_assert_eq!(normalized(&bulk), normalized(&reference));
        prop_assert_eq!(bulk.active_obligation(), reference.active_obligation());
        prop_assert!(bulk.completed_obligation(&source).unwrap().is_none());
        let next = source.located_bytes().nth(bulk.consumed_len()).unwrap();
        prop_assert_eq!(bulk.edge(&source, next), reference.edge(&source, next));
        prop_assert_eq!(normalized(&bulk), normalized(&reference));
        prop_assert_eq!(bulk.completed_obligation(&source), reference.completed_obligation(&source));
    }
}

#[test]
fn opaque_interior_narrower_chunks_compose_with_pool_reuse() {
    let source = input(&(0..99).map(|value| value as u8).collect::<Vec<_>>());
    let owners = [(80, 72), (68, 60), (58, 54)];
    let mut bulk = synthetic(3, 48, &owners, CanonicalPathObligationKind::NestedEpm1, 0);
    let mut reference = synthetic(3, 48, &owners, CanonicalPathObligationKind::NestedEpm1, 7);
    let mut occupied_peak = None;
    let mut batches = 0;
    while let Some(mut plan) = bulk.plan_opaque_interior(source.len()) {
        plan.width = plan.width.min(2);
        assert!(bulk.try_advance_opaque_interior(source.len(), plan));
        scalar_interior(&mut reference, &source, plan.width);
        assert_eq!(normalized(&bulk), normalized(&reference));
        let slots = bulk.machine.frames.slots.len();
        assert_eq!(*occupied_peak.get_or_insert(slots), slots);
        batches += 1;
    }
    assert_eq!(bulk.consumed_len(), 50);
    assert_eq!(batches, 24);
    let byte = source.located_bytes().nth(50).unwrap();
    assert_eq!(
        bulk.edge(&source, byte).unwrap(),
        reference.edge(&source, byte).unwrap()
    );
    assert_eq!(normalized(&bulk), normalized(&reference));
    let completed = bulk.completed_obligation(&source).unwrap().unwrap();
    assert_eq!(completed.payload, CanonicalByteRange { start: 3, end: 51 });
    assert_eq!(
        completed.payload_end,
        CanonicalPathSource::in_source(origin(50).source, origin(50).offset)
    );
    assert!(bulk.plan_opaque_interior(source.len()).is_none());
}

#[test]
fn opaque_interior_stale_and_invalid_plans_are_exact_noops() {
    let mut reducer = synthetic(
        3,
        12,
        &[(15, 10)],
        CanonicalPathObligationKind::NestedEpm1,
        0,
    );
    let plan = reducer.plan_opaque_interior(20).unwrap();
    for invalid in [
        OpaqueInteriorPlan { from: 4, ..plan },
        OpaqueInteriorPlan {
            id: CanonicalPathObligationId(74),
            ..plan
        },
        OpaqueInteriorPlan { width: 0, ..plan },
        OpaqueInteriorPlan {
            width: plan.width + 1,
            ..plan
        },
        OpaqueInteriorPlan {
            width: usize::MAX,
            ..plan
        },
    ] {
        assert_refused(&mut reducer, 20, invalid);
    }
    for input_len in [0, 2, 3, 4] {
        assert!(reducer.plan_opaque_interior(input_len).is_none());
        assert_refused(&mut reducer, input_len, plan);
    }
    assert!(reducer.try_advance_opaque_interior(20, OpaqueInteriorPlan { width: 1, ..plan }));
    assert_refused(&mut reducer, 20, plan);
}

#[test]
fn opaque_interior_mode_guards_cover_every_owner_and_leaf() {
    let owners = [(20, 18), (16, 14), (12, 10)];
    let error = CanonicalPathAdmissionError::new(
        CanonicalPathAdmissionErrorKind::RejectedObligation,
        CanonicalPathSource::in_source(91, 703),
    );
    for depth in 0..=owners.len() {
        for status in [PathStatus::Dead(error), PathStatus::SplitComplete] {
            let mut reducer = synthetic(3, 8, &owners, CanonicalPathObligationKind::NestedEpm1, 0);
            let plan = reducer.plan_opaque_interior(30).unwrap();
            set_path_status(&mut reducer, depth, status);
            assert!(reducer.plan_opaque_interior(30).is_none());
            assert_refused(&mut reducer, 30, plan);
        }
    }
    for pending in [false, true] {
        let mut reducer = synthetic(3, 8, &owners, CanonicalPathObligationKind::NestedEpm1, 0);
        let plan = reducer.plan_opaque_interior(30).unwrap();
        let id = leaf_id(&reducer);
        let Frame::Obligation { next, .. } = reducer.machine.frames.slots[id.0] else {
            unreachable!()
        };
        if pending {
            let Frame::Obligation { status, .. } = &mut reducer.machine.frames.slots[id.0] else {
                unreachable!()
            };
            *status = ObligationStatus::AwaitingResolution {
                payload_end: CanonicalPathSource::in_source(19, 711),
            };
        } else {
            reducer.machine.frames.slots[id.0] = Frame::Bytes {
                remaining: 8,
                utf8: Some(Utf8State::default()),
                next,
            };
        }
        assert!(reducer.plan_opaque_interior(30).is_none());
        assert_refused(&mut reducer, 30, plan);
    }
    let mut empty = CanonicalPathStreamReducer::new();
    assert!(empty.plan_opaque_interior(30).is_none());
    assert_refused(&mut empty, 30, OpaqueInteriorPlan {
        from: 0,
        id: CanonicalPathObligationId(0),
        width: 1,
    });
}

#[test]
fn opaque_interior_boundary_matrix_retains_scalar_refusal_and_final_anchor() {
    for depth in [0, 1, 2, 8] {
        for (payload, region, path, available) in [
            (1, 20, 20, 20),
            (2, 20, 20, 20),
            (4, 20, 20, 20),
            (20, 2, 20, 20),
            (20, 20, 2, 20),
            (4, 4, 4, 20),
            (20, 20, 20, 1),
            (20, 20, 20, 2),
            (20, 20, 20, 7),
        ] {
            let owners = vec![(region, path); depth];
            let all = (0..3 + available)
                .map(|value| value as u8)
                .collect::<Vec<_>>();
            let source = input(&all);
            let mut bulk = synthetic(
                3,
                payload,
                &owners,
                CanonicalPathObligationKind::NestedEpm1,
                0,
            );
            let mut reference = synthetic(
                3,
                payload,
                &owners,
                CanonicalPathObligationKind::NestedEpm1,
                4,
            );
            let expected = expected_width(3, payload, &owners, source.len());
            if let Some(plan) = bulk.plan_opaque_interior(source.len()) {
                assert_eq!(plan.width, expected);
                assert!(bulk.try_advance_opaque_interior(source.len(), plan));
                scalar_interior(&mut reference, &source, plan.width);
            } else {
                assert_eq!(expected, 0);
            }
            assert_eq!(normalized(&bulk), normalized(&reference));
            for _ in 0..2 {
                if bulk
                    .active_obligation()
                    .is_some_and(|value| value.awaiting_resolution)
                {
                    break;
                }
                let Some(byte) = source.located_bytes().nth(bulk.consumed_len()) else {
                    break;
                };
                assert_eq!(bulk.edge(&source, byte), reference.edge(&source, byte));
                assert_eq!(normalized(&bulk), normalized(&reference));
                assert_eq!(bulk.active_obligation(), reference.active_obligation());
                let completed = bulk.completed_obligation(&source).unwrap();
                assert_eq!(completed, reference.completed_obligation(&source).unwrap());
                if let Some(completed) = completed {
                    assert_eq!(
                        completed.payload_end,
                        CanonicalPathSource::in_source(
                            byte.provenance.source,
                            byte.provenance.offset
                        )
                    );
                }
            }
            let end = CanonicalPathSource::in_source(99, 901);
            assert_eq!(bulk.value(true, end), reference.value(true, end));
        }
    }
}

#[test]
fn opaque_interior_huge_lengths_are_clipped_before_conversion_or_addition() {
    let source = input(&[1; 11]);
    let owners = [(u64::MAX, u64::MAX); 8];
    let mut bulk = synthetic(
        3,
        u64::MAX,
        &owners,
        CanonicalPathObligationKind::NestedEpm1,
        0,
    );
    let mut reference = synthetic(
        3,
        u64::MAX,
        &owners,
        CanonicalPathObligationKind::NestedEpm1,
        3,
    );
    let plan = bulk.plan_opaque_interior(source.len()).unwrap();
    assert_eq!(plan.width, 7);
    assert!(bulk.try_advance_opaque_interior(source.len(), plan));
    scalar_interior(&mut reference, &source, plan.width);
    assert_eq!(normalized(&bulk), normalized(&reference));
    let final_byte = source.located_bytes().nth(10).unwrap();
    assert_eq!(
        bulk.edge(&source, final_byte),
        reference.edge(&source, final_byte)
    );
    assert_eq!(normalized(&bulk), normalized(&reference));
    assert!(bulk.plan_opaque_interior(source.len()).is_none());
    let end = CanonicalPathSource::in_source(99, 901);
    assert_eq!(bulk.value(true, end), reference.value(true, end));
    assert_eq!(
        bulk.value(true, end).unwrap().unwrap_err().kind,
        CanonicalPathAdmissionErrorKind::Truncated
    );
    for from in [usize::MAX - 8, usize::MAX - 1, usize::MAX] {
        let mut reducer = synthetic(
            from,
            u64::MAX,
            &owners,
            CanonicalPathObligationKind::NestedEpm1,
            0,
        );
        if let Some(plan) = reducer.plan_opaque_interior(usize::MAX) {
            assert_eq!(plan.width, 7);
            assert!(reducer.try_advance_opaque_interior(usize::MAX, plan));
            assert_eq!(reducer.consumed_len(), usize::MAX - 1);
            reducer.assert_frame_invariants();
        } else {
            assert!(usize::MAX - from <= 1);
        }
    }
}

#[test]
fn opaque_interior_keeps_persistent_forks_and_public_scalar_events_unchanged() {
    let bytes = [ESCAPE, 8, 11, 12, 13, 14, 15, 16, 17, 18];
    let source = input(&bytes);
    let mut arena = CanonicalBytesArena::new();
    let mut history = CanonicalPathReducer::new();
    let mut checkpoint = history.root(&arena);
    let mut stream = CanonicalPathStreamReducer::new();
    for byte in source.located_bytes().take(2) {
        let (next, event) = history.edge(&mut arena, checkpoint, byte).unwrap();
        assert_eq!(stream.edge(&source, byte).unwrap(), event);
        checkpoint = next;
    }
    let saved = checkpoint;
    let saved_progress = history.active_obligation(saved);
    let fork_byte = CanonicalLocatedByte::new(0xfe, CanonicalByteProvenance::new(93, 707));
    let (fork, fork_event) = history.edge(&mut arena, saved, fork_byte).unwrap();
    assert_eq!(fork_event, CanonicalPathTransition::ObligationByte {
        id: saved_progress.unwrap().obligation.id,
        byte: LocatedPathByte::in_source(0xfe, 93, 707),
        last: false,
    });
    let plan = stream.plan_opaque_interior(source.len()).unwrap();
    assert_eq!(plan.width, 7);
    assert!(stream.try_advance_opaque_interior(source.len(), plan));
    for byte in source.located_bytes().skip(2).take(7) {
        let (next, event) = history.edge(&mut arena, checkpoint, byte).unwrap();
        assert_eq!(event, CanonicalPathTransition::ObligationByte {
            id: plan.id,
            byte: LocatedPathByte::in_source(
                byte.byte,
                byte.provenance.source,
                byte.provenance.offset
            ),
            last: false,
        });
        checkpoint = next;
    }
    assert_eq!(
        history.active_obligation(checkpoint),
        stream.active_obligation()
    );
    assert_eq!(history.active_obligation(saved), saved_progress);
    assert_eq!(arena.bytes(saved.bytes()).collect::<Vec<_>>(), bytes[..2]);
    assert_eq!(arena.bytes(fork.bytes()).collect::<Vec<_>>(), vec![
        ESCAPE, 8, 0xfe
    ]);
    let last = source.located_bytes().nth(9).unwrap();
    let (next, event) = history.edge(&mut arena, checkpoint, last).unwrap();
    assert_eq!(event, stream.edge(&source, last).unwrap());
    assert!(matches!(event, CanonicalPathTransition::ObligationByte {
        last: true,
        ..
    }));
    checkpoint = next;
    let left = history
        .freeze_completed_obligation(&arena, checkpoint)
        .unwrap()
        .unwrap();
    let right = stream.completed_obligation(&source).unwrap().unwrap();
    assert_eq!(left.obligation, right.obligation);
    assert_eq!(left.payload_end, right.payload_end);
    assert_eq!(
        left.payload.located_bytes().collect::<Vec<_>>(),
        source
            .subview(right.payload)
            .unwrap()
            .located_bytes()
            .collect::<Vec<_>>()
    );
    assert_eq!(
        history.machine.next_obligation,
        stream.machine.next_obligation
    );
    let resolution = CanonicalPathObligationResolution::RawPar {
        canonical_path_stable: false,
    };
    checkpoint = history
        .resolve_obligation(&arena, checkpoint, plan.id, resolution)
        .unwrap();
    stream
        .resolve_obligation(&source, plan.id, resolution)
        .unwrap();
    let end = CanonicalPathSource::in_source(99, 811);
    assert_eq!(
        history.value(checkpoint, true, end),
        stream.value(true, end)
    );
    assert_eq!(history.active_obligation(saved), saved_progress);
    stream.assert_frame_invariants();
}
