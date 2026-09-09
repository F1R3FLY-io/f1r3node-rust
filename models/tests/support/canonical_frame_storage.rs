use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;

impl CanonicalPathStreamReducer {
    pub(crate) fn assert_reference_equivalent(&self, reference: &Self) {
        assert_eq!(
            super::opaque_interior_tests::normalized(self),
            super::opaque_interior_tests::normalized(reference),
        );
        assert_eq!(self.active_obligation(), reference.active_obligation());
    }

    pub(crate) fn reference_edge(
        &mut self,
        input: &IndexedByteRegion<'_>,
        located: CanonicalLocatedByte,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
        assert!(self.owners.is_empty());
        let byte = self.machine.prepare_edge(&self.current, located)?;
        let consumed = self
            .consumed
            .checked_add(1)
            .filter(|next| *next <= input.len())
            .ok_or_else(|| {
                CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::SizeOverflow,
                    byte.source,
                )
            })?;
        let (current, event) =
            self.machine
                .advance(&IndexedPathBytes { input, consumed }, &self.current, byte);
        self.current = current;
        self.consumed = consumed;
        Ok(event)
    }

    pub(crate) fn reference_resolve(
        &mut self,
        input: &IndexedByteRegion<'_>,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        assert!(self.owners.is_empty());
        self.current = self.machine.resolved_state(
            &IndexedPathBytes {
                input,
                consumed: self.consumed,
            },
            &self.current,
            id,
            resolution,
        )?;
        Ok(())
    }

    pub(super) fn projected_frame_graph(&self) -> (InnerPath, Vec<Frame<CanonicalByteRange>>) {
        let mut frames = self.machine.frames.slots.clone();
        let mut current = self.current.clone();
        for saved in self.owners.iter().rev() {
            let id = saved.owner.stack.unwrap();
            let Frame::Region {
                remaining,
                phase:
                    RegionPhase::Path {
                        remaining: path_remaining,
                        path,
                        ..
                    },
                ..
            } = &mut frames[id.0]
            else {
                panic!("held owner must be a Region")
            };
            *remaining = u64::try_from(
                saved
                    .region_end
                    .checked_sub(self.graph_stamp as u128)
                    .unwrap(),
            )
            .unwrap();
            *path_remaining = u64::try_from(
                saved
                    .path_end
                    .checked_sub(self.graph_stamp as u128)
                    .unwrap(),
            )
            .unwrap();
            *path = current;
            current = saved.owner.clone();
        }
        (current, frames)
    }

    pub(crate) fn assert_frame_invariants(&self) {
        let frames = &self.machine.frames;
        assert!(frames.allocated.is_empty());
        assert!(frames.retired.is_empty());
        let mut seen = vec![false; frames.slots.len()];
        let mut pending: Vec<_> = self.current.stack.into_iter().collect();
        let mut minimum = None;
        let mut path_minimum = None;
        let mut all_open = true;
        for (index, owner) in self.owners.iter().enumerate() {
            let id = owner.owner.stack.unwrap();
            let Frame::Region {
                phase: RegionPhase::Path { path, .. },
                ..
            } = &frames.slots[id.0]
            else {
                panic!("a held owner is a Region")
            };
            assert!(
                path.stack.is_none(),
                "detached slot retains an aliased child"
            );
            pending.push(id);
            let boundary = RegionBoundary {
                end: owner.region_end.min(owner.path_end),
                owner: index,
            };
            if minimum.is_none_or(|previous: RegionBoundary| boundary.end < previous.end) {
                minimum = Some(boundary);
            }
            let path_boundary = RegionBoundary {
                end: owner.path_end,
                owner: index,
            };
            if path_minimum.is_none_or(|previous: RegionBoundary| path_boundary.end <= previous.end)
            {
                path_minimum = Some(path_boundary);
            }
            all_open &= owner.owner.status == PathStatus::Open;
            assert_eq!(Some(owner.all_boundary), minimum);
            assert_eq!(Some(owner.path_boundary), path_minimum);
            assert_eq!(owner.all_open, all_open);
        }
        while let Some(id) = pending.pop() {
            assert!(
                !std::mem::replace(&mut seen[id.0], true),
                "aliased live frame"
            );
            let frame = &frames.slots[id.0];
            pending.extend(next(frame));
            pending.extend(child(frame));
        }
        let (root, projection) = self.projected_frame_graph();
        let mut projected = vec![false; frames.slots.len()];
        let mut pending: Vec<_> = root.stack.into_iter().collect();
        while let Some(id) = pending.pop() {
            assert!(
                !std::mem::replace(&mut projected[id.0], true),
                "projected graph aliases a slot"
            );
            pending.extend(next(&projection[id.0]));
            pending.extend(child(&projection[id.0]));
        }
        assert_eq!(projected, seen, "projection changes occupied ownership");
        for id in &frames.free {
            assert!(
                !std::mem::replace(&mut seen[id.0], true),
                "free slot is live or duplicated"
            );
        }
        assert!(
            seen.iter().all(|seen| *seen),
            "unreachable frame was not retired"
        );
    }
}

fn next<Span>(frame: &Frame<Span>) -> Option<FrameId> {
    match frame {
        Frame::Tag { next, .. }
        | Frame::Varint { next, .. }
        | Frame::Bytes { next, .. }
        | Frame::Kids { next, .. }
        | Frame::Region { next, .. }
        | Frame::Obligation { next, .. } => *next,
    }
}

fn child<Span>(frame: &Frame<Span>) -> Option<FrameId> {
    match frame {
        Frame::Region {
            phase: RegionPhase::Path { path, .. },
            ..
        } => path.stack,
        _ => None,
    }
}

fn distinct(ids: &[FrameId]) -> Result<BTreeSet<usize>, &'static str> {
    let result: BTreeSet<_> = ids.iter().map(|id| id.0).collect();
    if result.len() != ids.len() {
        return Err("duplicate slot in journal");
    }
    Ok(result)
}

fn occupied(storage: &FrameStorage) -> Result<BTreeSet<usize>, &'static str> {
    let free = distinct(&storage.free)?;
    let allocated = distinct(&storage.allocated)?;
    let retired = distinct(&storage.retired)?;
    if free
        .iter()
        .chain(allocated.iter())
        .chain(retired.iter())
        .any(|id| *id >= storage.slots.len())
    {
        return Err("slot outside storage");
    }
    if !free.is_disjoint(&allocated) || !free.is_disjoint(&retired) {
        return Err("free slot remains in flight");
    }
    Ok((0..storage.slots.len())
        .filter(|id| !free.contains(id))
        .collect())
}

fn graph(storage: &FrameStorage, root: Option<FrameId>) -> Result<BTreeSet<usize>, &'static str> {
    let live = occupied(storage)?;
    let mut seen = BTreeSet::new();
    let mut pending: Vec<_> = root.into_iter().collect();
    while let Some(id) = pending.pop() {
        if !live.contains(&id.0) {
            return Err("selected graph references a free or absent cell");
        }
        if !seen.insert(id.0) {
            return Err("selected graph has aliased child or continuation");
        }
        let frame = &storage.slots[id.0];
        pending.extend(next(frame));
        pending.extend(child(frame));
    }
    Ok(seen)
}

fn held_unchanged(storage: &FrameStorage, held: &[(FrameId, Frame)]) -> Result<(), &'static str> {
    for (id, frame) in held {
        if storage.slots.get(id.0) != Some(frame) {
            return Err("held original cell changed");
        }
    }
    Ok(())
}

fn transaction(
    storage: &FrameStorage,
    held: &[(FrameId, Frame)],
    root: Option<FrameId>,
) -> Result<(), &'static str> {
    held_unchanged(storage, held)?;
    let held_ids: BTreeSet<_> = held.iter().map(|(id, _)| id.0).collect();
    let allocated = distinct(&storage.allocated)?;
    let retired = distinct(&storage.retired)?;
    if !held_ids.is_disjoint(&allocated) {
        return Err("original or in-flight slot reused before commit");
    }
    let live = occupied(storage)?;
    if live != held_ids.union(&allocated).copied().collect() {
        return Err("physical occupancy differs from originals plus reservations");
    }
    let selected = graph(storage, root)?;
    if !selected.is_disjoint(&retired) {
        return Err("selected cell is retired");
    }
    if live != selected.union(&retired).copied().collect() {
        return Err("retirement journal omits an unselected cell");
    }
    Ok(())
}

fn committed(storage: &FrameStorage, root: Option<FrameId>) -> Result<(), &'static str> {
    if !storage.allocated.is_empty() || !storage.retired.is_empty() {
        return Err("commit retained an in-flight journal");
    }
    if graph(storage, root)? != occupied(storage)? {
        return Err("committed occupancy differs from selected graph");
    }
    Ok(())
}

fn source(seed: u16) -> CanonicalPathSource {
    CanonicalPathSource::in_source(u32::from(seed) + 17, usize::from(seed) * 13 + 29)
}

fn frame(kind: u8, seed: u16, next: Option<FrameId>) -> Frame {
    match kind % 6 {
        0 => Frame::Tag {
            escape_allowed: seed & 1 != 0,
            next,
        },
        1 => Frame::Varint {
            role: VarintRole::ObligationLength {
                kind: CanonicalPathObligationKind::NestedEpm1,
                tag_source: source(seed),
            },
            state: VarintState {
                value: u64::from(seed),
                shift: 7,
                bytes: 1,
            },
            next,
        },
        2 => Frame::Bytes {
            remaining: u64::from(seed) + 1,
            utf8: Some(Utf8State {
                remaining: 1,
                next_min: 0x80,
                next_max: 0xbf,
            }),
            next,
        },
        3 => Frame::Kids {
            remaining: u64::from(seed) + 1,
            next,
        },
        4 => Frame::Region {
            remaining: u64::from(seed) + 7,
            previous: None,
            phase: RegionPhase::PathLength(VarintState {
                value: u64::from(seed),
                shift: 14,
                bytes: 2,
            }),
            next,
        },
        _ => Frame::Obligation {
            obligation: CanonicalPathObligation {
                id: CanonicalPathObligationId(u64::from(seed) + 1_000_003),
                kind: CanonicalPathObligationKind::RawParEscape,
                payload_len: u64::from(seed) + 31,
                tag_source: source(seed),
            },
            status: ObligationStatus::AwaitingResolution {
                payload_end: source(seed.wrapping_add(1)),
            },
            payload_start: usize::from(seed) + 53,
            next,
        },
    }
}

fn error(seed: u16) -> CanonicalPathAdmissionError {
    CanonicalPathAdmissionError::new(
        CanonicalPathAdmissionErrorKind::NestedEpmNotMap,
        source(seed),
    )
}

fn region(child: Option<FrameId>, next: Option<FrameId>, seed: u16) -> Frame {
    let mut path = InnerPath::new(false);
    path.stack = child;
    path.completed_segments = u64::from(seed) + 3;
    path.first_tag = Some((ESCAPE, source(seed)));
    path.stable = seed & 1 != 0;
    Frame::Region {
        remaining: u64::from(seed) + 101,
        previous: Some(CanonicalBytesArena::new().empty()),
        phase: RegionPhase::Path {
            declared_len: u64::from(seed) + 41,
            remaining: u64::from(seed) + 19,
            start_depth: usize::from(seed) + 7,
            path,
        },
        next,
    }
}

fn assert_reference(
    storage: &FrameStorage,
    root: Option<FrameId>,
    reference: &BTreeMap<usize, Frame>,
) {
    let mut pending: Vec<_> = root.into_iter().collect();
    let mut expected = BTreeSet::new();
    while let Some(id) = pending.pop() {
        assert!(expected.insert(id.0));
        let frame = &reference[&id.0];
        assert_eq!(&storage.slots[id.0], frame);
        pending.extend(next(frame));
        pending.extend(child(frame));
    }
    assert_eq!(graph(storage, root).unwrap(), expected);
}

fn push_expected(
    storage: &mut FrameStorage,
    reference: &mut BTreeMap<usize, Frame>,
    frame: Frame,
) -> FrameId {
    let id = storage.push(frame.clone());
    reference.insert(id.0, frame);
    id
}

fn run_edits(edits: &[(u8, bool, u16)]) {
    let mut storage = FrameStorage::new();
    let mut expected: Vec<(FrameId, Frame)> = Vec::new();
    let mut peak = 0;
    for &(operation, restore, seed) in edits {
        let held = expected.clone();
        for step in 0..if operation % 6 >= 3 { 3 } else { 1 } {
            let checkpoint = storage.checkpoint();
            let previous = expected.last().map(|(id, _)| *id);
            let action = match operation % 6 {
                0 => 0,
                1 => 1,
                2 | 3 | 5 => 2,
                _ => {
                    if step < 2 {
                        0
                    } else {
                        1
                    }
                }
            };
            if action == 1 && previous.is_some() {
                expected.pop();
                let selected = expected.last().map(|(id, _)| *id);
                storage.select(checkpoint, previous, selected);
            } else {
                let consumed = if action == 2 {
                    expected.pop().map(|(id, _)| id)
                } else {
                    None
                };
                let tail = expected.last().map(|(id, _)| *id);
                let value = frame((seed as u8).wrapping_add(step), seed, tail);
                let id = storage.push(value.clone());
                assert!(!held.iter().any(|(held_id, _)| *held_id == id));
                expected.push((id, value));
                storage.select(checkpoint, consumed, Some(id));
            }
            let root = expected.last().map(|(id, _)| *id);
            transaction(&storage, &held, root).unwrap();
            peak = peak.max(held.len() + storage.allocated.len());
            assert!(storage.slots.len() <= peak);
            for (id, value) in &expected {
                assert_eq!(&storage.slots[id.0], value);
            }
        }
        storage.finish(restore);
        if restore {
            expected = held;
        }
        committed(&storage, expected.last().map(|(id, _)| *id)).unwrap();
        for (id, value) in &expected {
            assert_eq!(&storage.slots[id.0], value);
        }
    }
}

#[test]
fn frame_storage_all_variants_preserve_rollback_metadata() {
    let mut storage = FrameStorage::new();
    let mut held = Vec::new();
    let mut root = None;
    for kind in 0..6 {
        let value = frame(kind, 100 + u16::from(kind), root);
        let id = storage.push(value.clone());
        held.push((id, value));
        root = Some(id);
    }
    storage.finish(false);
    let original_root = root;
    for kind in 0..6 {
        let checkpoint = storage.checkpoint();
        let previous = root;
        let tail = next(&storage.slots[previous.unwrap().0]);
        root = Some(storage.push(frame(kind, 500 + u16::from(kind), tail)));
        storage.select(checkpoint, previous, root);
        transaction(&storage, &held, root).unwrap();
    }
    assert_eq!(storage.allocated.len(), 6);
    assert_eq!(storage.retired.len(), 6);
    storage.finish(true);
    held_unchanged(&storage, &held).unwrap();
    committed(&storage, original_root).unwrap();
    run_edits(&[
        (0, false, 1),
        (0, false, 2),
        (1, false, 3),
        (2, false, 4),
        (3, false, 5),
        (4, false, 6),
        (5, true, 7),
    ]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn frame_storage_generated_edits_preserve_exact_ownership(
        edits in prop::collection::vec((0u8..6, any::<bool>(), any::<u16>()), 1..97),
    ) {
        run_edits(&edits);
    }

    #[test]
    fn frame_storage_nested_region_selection_preserves_held_graphs(
        rollbacks in prop::collection::vec(any::<bool>(), 1..9),
        seed in any::<u16>(),
        restore in any::<bool>(),
    ) {
        let mut storage = FrameStorage::new();
        let mut reference = BTreeMap::new();
        let leaf = push_expected(&mut storage, &mut reference, frame(5, seed, None));
        let mut root = leaf;
        let mut owners = Vec::new();
        for depth in 0..rollbacks.len() {
            let outer = push_expected(&mut storage, &mut reference, frame(0, depth as u16, None));
            let value = region(Some(root), Some(outer), seed.wrapping_add(depth as u16));
            let id = push_expected(&mut storage, &mut reference, value.clone());
            owners.push((id, value));
            root = id;
        }
        storage.finish(false);
        let original_root = root;
        let held: Vec<_> = reference.iter().map(|(id, value)| (FrameId(*id), value.clone())).collect();
        let checkpoints: Vec<_> = owners.iter().map(|_| storage.checkpoint()).collect();
        let checkpoint = storage.checkpoint();
        let replacement = push_expected(&mut storage, &mut reference, frame(5, seed.wrapping_add(17), None));
        storage.select(checkpoint, Some(leaf), Some(replacement));
        let mut selected = replacement;
        for (index, (saved, value)) in owners.iter().enumerate() {
            if rollbacks[index] {
                storage.select(checkpoints[index], Some(*saved), Some(*saved));
                selected = *saved;
            } else {
                let mut rebuilt = value.clone();
                if let Frame::Region { phase: RegionPhase::Path { path, .. }, .. } = &mut rebuilt {
                    path.stack = Some(selected);
                    if index % 2 == 0 {
                        path.kill(error(seed));
                    }
                }
                let id = push_expected(&mut storage, &mut reference, rebuilt);
                storage.select(checkpoints[index], Some(*saved), Some(id));
                selected = id;
            }
            held_unchanged(&storage, &held).unwrap();
            occupied(&storage).unwrap();
            assert_eq!(occupied(&storage).unwrap().len(), held.len() + storage.allocated.len());
        }
        transaction(&storage, &held, Some(selected)).unwrap();
        assert_reference(&storage, Some(selected), &reference);
        storage.finish(restore);
        let selected = if restore { original_root } else { selected };
        committed(&storage, Some(selected)).unwrap();
        assert_reference(&storage, Some(selected), &reference);
    }
}

#[test]
fn frame_storage_dead_selection_uses_stack_identity_not_status() {
    let mut storage = FrameStorage::new();
    let checkpoint = storage.checkpoint();
    let mut shallow = InnerPath::new(true);
    shallow.first_tag = Some((ESCAPE, source(71)));
    shallow.stable = false;
    let tag = storage.push(frame(0, 71, None));
    shallow.stack = Some(tag);
    let before_kill = shallow.clone();
    shallow.kill(error(71));
    storage.select(checkpoint, None, shallow.stack);
    transaction(&storage, &[], shallow.stack).unwrap();
    storage.finish(false);
    committed(&storage, shallow.stack).unwrap();
    assert_eq!(shallow.stack, before_kill.stack);
    assert_eq!(shallow.first_tag, before_kill.first_tag);
    assert_eq!(shallow.stable, before_kill.stable);

    let checkpoint = storage.checkpoint();
    let old_frame = storage.slots[tag.0].clone();
    let replacement = storage.push(frame(5, 91, None));
    shallow.stack = Some(replacement);
    assert!(matches!(shallow.status, PathStatus::Dead(_)));
    storage.select(checkpoint, Some(tag), shallow.stack);
    transaction(&storage, &[(tag, old_frame)], shallow.stack).unwrap();
    storage.finish(false);
    committed(&storage, shallow.stack).unwrap();
    assert!(storage.free.contains(&tag));
    assert!(!storage.free.contains(&replacement));
}

#[test]
fn frame_storage_region_opening_completion_and_saved_owner_are_exact() {
    let mut storage = FrameStorage::new();
    let mut reference = BTreeMap::new();
    let outer = push_expected(&mut storage, &mut reference, frame(0, 11, None));
    storage.finish(false);
    let held = vec![(outer, reference[&outer.0].clone())];
    let checkpoint = storage.checkpoint();
    let opened = push_expected(&mut storage, &mut reference, region(None, Some(outer), 12));
    storage.select(checkpoint, None, Some(opened));
    transaction(&storage, &held, Some(opened)).unwrap();
    storage.finish(false);

    let held = vec![
        (outer, reference[&outer.0].clone()),
        (opened, reference[&opened.0].clone()),
    ];
    let checkpoint = storage.checkpoint();
    let child = push_expected(&mut storage, &mut reference, frame(5, 13, None));
    let full = push_expected(
        &mut storage,
        &mut reference,
        region(Some(child), Some(outer), 14),
    );
    storage.select(checkpoint, Some(opened), Some(full));
    transaction(&storage, &held, Some(full)).unwrap();
    storage.finish(false);

    let held = vec![
        (outer, reference[&outer.0].clone()),
        (child, reference[&child.0].clone()),
        (full, reference[&full.0].clone()),
    ];
    let owner_checkpoint = storage.checkpoint();
    let child_checkpoint = storage.checkpoint();
    let replacement = push_expected(&mut storage, &mut reference, frame(5, 15, None));
    storage.select(child_checkpoint, Some(child), Some(replacement));
    let mut saved = InnerPath::new(false);
    saved.stack = Some(full);
    saved.first_tag = Some((EPATHMAP, source(16)));
    saved.kill(error(16));
    storage.select(owner_checkpoint, Some(full), saved.stack);
    assert_eq!(storage.retired, vec![replacement]);
    transaction(&storage, &held, saved.stack).unwrap();
    storage.finish(false);
    assert_reference(&storage, saved.stack, &reference);

    let owner_checkpoint = storage.checkpoint();
    let child_checkpoint = storage.checkpoint();
    storage.select(child_checkpoint, Some(child), None);
    storage.select(owner_checkpoint, Some(full), Some(outer));
    assert_eq!(storage.retired, vec![child, full]);
    transaction(&storage, &held, Some(outer)).unwrap();
    storage.finish(false);
    committed(&storage, Some(outer)).unwrap();
    assert_reference(&storage, Some(outer), &reference);
}

#[test]
fn frame_storage_checker_rejects_eager_reuse_aliasing_and_omitted_retirement() {
    let mut storage = FrameStorage::new();
    let original = frame(5, 81, None);
    let root = storage.push(original.clone());
    storage.finish(false);
    let held = vec![(root, original)];
    storage.slots[root.0] = frame(0, 82, None);
    assert_eq!(
        held_unchanged(&storage, &held),
        Err("held original cell changed")
    );
    storage.slots[root.0] = held[0].1.clone();
    let replacement = storage.push(frame(5, 83, None));
    assert_eq!(
        transaction(&storage, &held, Some(replacement)),
        Err("retirement journal omits an unselected cell")
    );
    storage.retire(root);
    transaction(&storage, &held, Some(replacement)).unwrap();
    storage.retire(root);
    assert_eq!(occupied(&storage), Err("duplicate slot in journal"));
    storage.retired.pop();
    storage.free.push(root);
    assert_eq!(occupied(&storage), Err("free slot remains in flight"));
    storage.free.pop();
    storage.finish(false);

    let mut alias = FrameStorage::new();
    let tail = alias.push(frame(0, 84, None));
    let owner = alias.push(region(Some(tail), Some(tail), 85));
    alias.finish(false);
    assert_eq!(
        graph(&alias, Some(owner)),
        Err("selected graph has aliased child or continuation")
    );

    let mut omitted = FrameStorage::new();
    let outer = omitted.push(frame(0, 86, None));
    let child = omitted.push(frame(5, 87, None));
    let owner = omitted.push(region(Some(child), Some(outer), 88));
    omitted.finish(false);
    let held: Vec<_> = omitted
        .slots
        .iter()
        .cloned()
        .enumerate()
        .map(|(id, frame)| (FrameId(id), frame))
        .collect();
    omitted.retire(owner);
    assert_eq!(
        transaction(&omitted, &held, Some(outer)),
        Err("retirement journal omits an unselected cell")
    );
    assert_eq!(
        transaction(&omitted, &held, None),
        Err("retirement journal omits an unselected cell")
    );
    omitted.retire(child);
    transaction(&omitted, &held, Some(outer)).unwrap();
}

#[test]
fn frame_storage_persistent_history_keeps_branch_cells_and_never_reuses() {
    assert_eq!(
        std::mem::size_of::<FrameHistory>(),
        std::mem::size_of::<Vec<Frame>>()
    );
    let mut storage = FrameHistory::new();
    let mut expected = Vec::new();
    let mut roots = Vec::new();
    for index in 0..96 {
        let previous = roots.get(index / 3).copied();
        let checkpoint = storage.checkpoint();
        let value = frame(index as u8, index as u16, previous);
        let id = storage.push(value.clone());
        assert_eq!(id.0, index);
        if let Some(previous) = previous {
            storage.retire(previous);
        }
        storage.select(checkpoint, previous, Some(id));
        storage.finish(index % 2 == 0);
        expected.push(value);
        roots.push(id);
        assert_eq!(storage.slots, expected);
        assert_eq!(storage.checkpoint().allocated, 0);
        assert_eq!(storage.checkpoint().retired, 0);
    }
    assert_eq!(storage.slots.len(), 96);
}

#[test]
fn frame_storage_committed_reuse_has_fixed_slot_and_retained_byte_plateau() {
    let mut storage = FrameStorage::new();
    let mut value = frame(5, 1, None);
    let mut root = storage.push(value.clone());
    storage.finish(false);
    let mut plateau = None;
    for cycle in 0..512u16 {
        let held = vec![(root, value.clone())];
        for step in 0..8u16 {
            let checkpoint = storage.checkpoint();
            let previous = root;
            value = frame(step as u8, cycle.wrapping_mul(8).wrapping_add(step), None);
            root = storage.push(value.clone());
            assert_ne!(root, previous);
            storage.select(checkpoint, Some(previous), Some(root));
            transaction(&storage, &held, Some(root)).unwrap();
        }
        assert_eq!(storage.allocated.len(), 8);
        assert_eq!(storage.retired.len(), 8);
        storage.finish(false);
        committed(&storage, Some(root)).unwrap();
        assert_eq!(storage.slots.len(), 9);
        assert_eq!(storage.free.len(), 8);
        if cycle == 2 {
            plateau = Some(storage.retained_bytes());
        }
        if let Some(bytes) = plateau {
            assert_eq!(storage.retained_bytes(), bytes);
        }
    }
}

#[test]
fn persistent_topology_shared_prefix_owner_work_baseline() {
    for width in [1usize, 16, 128, 1_024] {
        for depth in [0usize, 1, 8, 32] {
            let mut key = vec![GBYTE_ARRAY];
            prost::encoding::encode_varint(width as u64, &mut key);
            key.resize(key.len() + width, 0);
            let mut expected_descents = 0;
            for _ in 0..depth {
                expected_descents += key.len();
                let mut region = Vec::new();
                prost::encoding::encode_varint(key.len() as u64, &mut region);
                region.extend_from_slice(&key);
                key = vec![EPATHMAP];
                prost::encoding::encode_varint(region.len() as u64, &mut key);
                key.extend_from_slice(&region);
            }
            let located = |byte, offset| LocatedPathByte::in_source(byte, 7, 101 + offset * 13);
            let mut admission = CanonicalPathTopologyAdmission::new();
            for (offset, byte) in key[..key.len() - 1].iter().copied().enumerate() {
                admission.edge(located(byte, offset)).unwrap();
                assert!(admission.freeze_completed_obligation().unwrap().is_none());
            }
            let prefix = admission.current;
            let prefix_state = admission.reducer.states[prefix.state.0].clone();
            let mut endpoints = Vec::new();
            let mut restore_queries = 0;
            for branch in 0..4u8 {
                admission.edge(located(branch, key.len() - 1)).unwrap();
                assert!(admission.freeze_completed_obligation().unwrap().is_none());
                let endpoint = admission.current;
                let summary = admission
                    .value(true, CanonicalPathSource::new(9_999))
                    .unwrap()
                    .unwrap();
                assert!(summary.canonical_path_stable);
                endpoints.push((endpoint, admission.reducer.states[endpoint.state.0].clone()));
                let before = admission.stats();
                let queries = admission.reducer.machine.owner_work.get().active_queries;
                assert_eq!(
                    admission.leave(CanonicalPathSource::new(10_000)).unwrap(),
                    CanonicalPathTransition::Restored {
                        active_obligation: None
                    }
                );
                restore_queries +=
                    admission.reducer.machine.owner_work.get().active_queries - queries;
                assert_eq!(admission.stats(), before);
                assert_eq!(admission.current, prefix);
                assert_eq!(admission.reducer.states[prefix.state.0], prefix_state);
                for (index, (handle, state)) in endpoints.iter().enumerate() {
                    assert_eq!(&admission.reducer.states[handle.state.0], state);
                    let mut expected = key.clone();
                    *expected.last_mut().unwrap() = index as u8;
                    assert_eq!(
                        admission.arena.bytes(handle.bytes).collect::<Vec<_>>(),
                        expected
                    );
                }
            }
            let before_unwind = admission.stats();
            let work = admission.reducer.machine.owner_work.get();
            assert_eq!(before_unwind.edges, key.len() + 3);
            assert_eq!(before_unwind.states, before_unwind.edges + 1);
            assert_eq!(before_unwind.region_descents, expected_descents + 3 * depth);
            assert_eq!(work.rebuilds, before_unwind.region_descents);
            assert_eq!(restore_queries, 4 * depth);
            for _ in 0..key.len() {
                admission.leave(CanonicalPathSource::new(10_001)).unwrap();
            }
            admission.finish(CanonicalPathSource::new(10_002)).unwrap();
            assert_eq!(admission.stats(), before_unwind);
            eprintln!(
                "persistent topology: width={width}, depth={depth}, branches=4, encoded={}, edges={}, states={}, frames={}, feed_descents={}, rebuilds={}, active_queries={}, completed_queries={}, branch_restore_queries={restore_queries}, state_backing={}, frame_backing={}, handle_bytes={}",
                key.len(),
                before_unwind.edges,
                before_unwind.states,
                before_unwind.frames,
                before_unwind.region_descents,
                work.rebuilds,
                work.active_queries,
                work.completed_queries,
                admission.reducer.retained_state_bytes(),
                admission.reducer.retained_frame_bytes(),
                std::mem::size_of::<CanonicalPathReducerHandle>()
            );
        }
    }
}

#[test]
fn persistent_fault_private_preflight_uses_selected_graph_not_latest_event() {
    for payload_started in [false, true] {
        let key: &[u8] = if payload_started {
            &[EPATHMAP, 4, 3, EPATHMAP_MAP, 2, 0xaa]
        } else {
            &[EPATHMAP, 3, 2, EPATHMAP_MAP, 2]
        };
        let mut arena = CanonicalBytesArena::new();
        let mut reducer = CanonicalPathReducer::new();
        let mut handle = reducer.root(&arena);
        let mut emitted = None;
        for (offset, byte) in key.iter().copied().enumerate() {
            let (next, event) = reducer
                .edge(
                    &mut arena,
                    handle,
                    CanonicalLocatedByte::new(
                        byte,
                        CanonicalByteProvenance::new(7, 101 + offset * 13),
                    ),
                )
                .unwrap();
            handle = next;
            if let CanonicalPathTransition::ObligationStarted(obligation) = event {
                emitted = Some(obligation);
            }
        }
        let emitted = emitted.unwrap();
        assert_eq!(reducer.machine.next_obligation, emitted.id.0 + 1);
        assert_eq!(reducer.active_obligation(handle), None);
        assert!(
            reducer
                .freeze_completed_obligation(&arena, handle)
                .unwrap()
                .is_none()
        );
        let original = reducer.states[handle.state.0].clone();
        let error = reducer
            .value(handle, true, CanonicalPathSource::new(99))
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind, CanonicalPathAdmissionErrorKind::Truncated);
        assert_eq!(
            error.source,
            CanonicalPathSource::in_source(7, 101 + (key.len() - 1) * 13)
        );
        let mut hidden = &original;
        loop {
            match &reducer.machine.frames[hidden.stack.unwrap().0] {
                Frame::Region {
                    phase: RegionPhase::Path { path, .. },
                    ..
                } => hidden = path,
                Frame::Obligation {
                    obligation,
                    status: ObligationStatus::Payload { remaining },
                    ..
                } if payload_started => {
                    assert_eq!(*remaining, 2);
                    assert_eq!(*obligation, emitted);
                    break;
                }
                Frame::Varint {
                    role: VarintRole::ObligationLength { .. },
                    ..
                } if !payload_started => break,
                other => panic!("unexpected selected hidden terminal: {other:?}"),
            }
        }
        let before = (
            reducer.states.clone(),
            reducer.machine.frames.slots.clone(),
            reducer.stats(),
            reducer.machine.next_obligation,
        );
        let returned = reducer
            .resolve_obligation(
                &arena,
                handle,
                emitted.id,
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                },
            )
            .unwrap_err();
        assert_eq!(
            returned.kind,
            CanonicalPathAdmissionErrorKind::UnresolvedObligation
        );
        assert_eq!(
            returned.source,
            if payload_started {
                emitted.tag_source
            } else {
                CanonicalPathSource::new(0)
            }
        );
        assert_eq!(
            (
                reducer.states.clone(),
                reducer.machine.frames.slots.clone(),
                reducer.stats(),
                reducer.machine.next_obligation
            ),
            before
        );
        let caller = CanonicalPathSource::in_source(8, 919);
        let rejected = reducer.reject_obligation(&arena, handle, emitted.id, caller);
        if payload_started {
            let next = rejected.unwrap();
            let mut expected = original.clone();
            expected.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::RejectedObligation,
                caller,
            ));
            assert_eq!(reducer.states[next.state.0], expected);
            assert_eq!(reducer.machine.frames.slots, before.1);
            assert_eq!(reducer.states[handle.state.0], original);
            assert_eq!(reducer.states.len(), before.0.len() + 1);
            assert_eq!(reducer.machine.next_obligation, before.3);
        } else {
            assert_eq!(
                rejected.unwrap_err(),
                CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    caller
                )
            );
            assert_eq!(
                (
                    reducer.states.clone(),
                    reducer.machine.frames.slots.clone(),
                    reducer.stats(),
                    reducer.machine.next_obligation
                ),
                before
            );
        }
    }
}
