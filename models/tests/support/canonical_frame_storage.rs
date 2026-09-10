use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;

use super::*;

#[test]
fn compact_checkpoint_addresses_preserve_slot_tag_and_native_width() {
    for slot in [0, 1, 17, (usize::MAX - 1) / 2] {
        let direct = PathStateId::new(slot, false);
        let linked = PathStateId::new(slot, true);
        assert_eq!(direct.slot(), slot);
        assert_eq!(linked.slot(), slot);
        assert!(!direct.is_linked());
        assert!(linked.is_linked());
        assert_ne!(direct, linked);
    }
    assert_eq!(
        std::mem::size_of::<PathStateId>(),
        std::mem::size_of::<usize>()
    );
    assert_eq!(
        std::mem::size_of::<CanonicalPathReducerHandle>(),
        std::mem::size_of::<CanonicalBytesHandle>() + std::mem::size_of::<usize>()
    );
    assert!(std::mem::size_of::<InnerPath>() >= 2);
    assert!(std::mem::size_of::<PersistentFocusedPath>() >= 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn absolute_payload_native_residual_matches_widened_arithmetic(
        start in any::<usize>(), declared in any::<u64>(), stamp in any::<usize>(),
    ) {
        let expected = if stamp < start {
            None
        } else {
            (start as u128 + declared as u128).checked_sub(stamp as u128)
                .and_then(|value| u64::try_from(value).ok())
        };
        prop_assert_eq!(ObligationStatus::Payload.remaining(declared, start, stamp), expected);
        let pending = ObligationStatus::AwaitingResolution { payload_end: source(17) };
        prop_assert_eq!(pending.remaining(declared, start, stamp), Some(0));
    }

    #[test]
    fn compact_checkpoint_publication_preserves_both_historical_arenas(
        cases in proptest::collection::vec((any::<u16>(), any::<bool>(), any::<bool>()), 0..97),
    ) {
        let mut storage = PersistentCheckpoints::new();
        let mut retained = Vec::new();
        for (seed, linked, fault) in cases {
            let current = InnerPath {
                escape_allowed: seed & 1 != 0,
                stack: Some(FrameId(seed as usize)),
                status: if seed & 2 != 0 { PathStatus::Dead(CanonicalPathAdmissionError::new(CanonicalPathAdmissionErrorKind::InvalidUtf8, source(seed))) } else { PathStatus::Open },
                completed_segments: u64::MAX - seed as u64,
                first_tag: Some((seed as u8, CanonicalPathSource::in_source(u32::MAX, usize::MAX))),
                stable: seed & 4 != 0,
            };
            let state = PersistentFocusedPath {
                current,
                owners: linked.then_some(PersistentRegionOwnerId(seed as usize)),
                stamp: usize::MAX - seed as usize,
                fault: fault.then_some(CanonicalPathAdmissionError::new(CanonicalPathAdmissionErrorKind::ReservedTag(seed as u8), CanonicalPathSource::in_source(u32::MAX, usize::MAX))),
            };
            let before = storage.clone();
            let frozen = state.fault.is_some() || matches!(state.current.status, PathStatus::Dead(_));
            let linked = linked || frozen;
            let id = storage.push(state.clone());
            assert_eq!(id.is_linked(), linked);
            assert_eq!(storage.len(), before.len() + 1);
            assert_eq!(&storage.direct[..before.direct.len()], before.direct.as_slice());
            assert_eq!(&storage.linked[..before.linked.len()], before.linked.as_slice());
            if linked { assert_eq!(storage.direct, before.direct); }
            else { assert_eq!(storage.linked, before.linked); }
            retained.push((id, state));
            for (id, expected) in &retained {
                for stamp in [0, 7, usize::MAX] {
                    let actual = storage.get(*id, stamp);
                    let mut selected = expected.clone();
                    if selected.fault.is_none() && !matches!(selected.current.status, PathStatus::Dead(_)) {
                        selected.stamp = stamp;
                    }
                    assert_eq!(actual, selected);
                }
            }
        }
        let mut plain = PersistentCheckpoints::new();
        for stamp in 0..100 {
            plain.push(PersistentFocusedPath { current: InnerPath::new(true), owners: None, stamp, fault: None });
        }
        assert_eq!(plain.linked.capacity(), 0);
    }
}

#[test]
fn compact_checkpoint_dead_private_operations_preserve_exact_graph_and_stamp() {
    let mut actual = CanonicalPathReducer::new();
    let mut expected = EagerCanonicalPathReducer::new();
    let mut arena = CanonicalBytesArena::new();
    let mut reference_arena = CanonicalBytesArena::new();
    let mut right = actual.root(&arena);
    let mut left = expected.root(&reference_arena);
    for (offset, byte) in [EPATHMAP, 3, 2, EPATHMAP_MAP, 0].into_iter().enumerate() {
        let byte = CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(37, offset * 29));
        right = actual.edge(&mut arena, right, byte).unwrap().0;
        left = expected.edge(&mut reference_arena, left, byte).unwrap().0;
    }
    let id = actual.active_obligation(right).unwrap().obligation.id;
    let refuse = CanonicalPathObligationResolution::NestedEpm1 {
        mode: NestedEpmMode::Map,
        entries_stable: false,
    };
    for _ in 0..2 {
        right = actual
            .resolve_obligation(&arena, right, id, refuse)
            .unwrap();
        left = expected
            .resolve_obligation(&reference_arena, left, id, refuse)
            .unwrap();
        assert_focused_persistent_matches(
            &expected,
            &reference_arena,
            left,
            &actual,
            &arena,
            right,
        );
    }
    assert!(right.state.is_linked());
    let saved_stamp = actual.state(right).stamp;
    for operation in 0..6 {
        let byte =
            CanonicalLocatedByte::new(0x99, CanonicalByteProvenance::new(41, 997 + operation));
        right = actual.edge(&mut arena, right, byte).unwrap().0;
        left = expected.edge(&mut reference_arena, left, byte).unwrap().0;
        assert!(actual.dead_error(&actual.state(right)).is_some());
        let (a, b) = if operation % 3 == 0 {
            (
                actual.reject_obligation(&arena, right, id, source(81)),
                expected.reject_obligation(&reference_arena, left, id, source(81)),
            )
        } else {
            let resolution = CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: operation % 3 == 2,
            };
            (
                actual.resolve_obligation(&arena, right, id, resolution),
                expected.resolve_obligation(&reference_arena, left, id, resolution),
            )
        };
        right = a.unwrap();
        left = b.unwrap();
        assert!(actual.dead_error(&actual.state(right)).is_some());
        assert!(right.state.is_linked());
        assert_eq!(actual.state(right).stamp, saved_stamp);
        assert_focused_persistent_matches(
            &expected,
            &reference_arena,
            left,
            &actual,
            &arena,
            right,
        );
    }
}

struct EagerFrameHistory(FrameHistory);

impl std::ops::Deref for EagerFrameHistory {
    type Target = FrameHistory;
    fn deref(&self) -> &Self::Target { &self.0 }
}

impl std::ops::Index<usize> for EagerFrameHistory {
    type Output = Frame;
    fn index(&self, index: usize) -> &Self::Output { &self.0[index] }
}

impl FrameStore for EagerFrameHistory {
    type Span = CanonicalBytesHandle;
    const EAGER_PAYLOADS: bool = true;

    fn new() -> Self { Self(FrameHistory::new()) }
    fn push(&mut self, frame: Frame) -> FrameId { self.0.push(frame) }
    fn retained_bytes(&self) -> usize { self.0.retained_bytes() }
}

pub(crate) struct EagerCanonicalPathReducer {
    states: Vec<InnerPath>,
    machine: CanonicalPathMachine<EagerFrameHistory>,
}

impl EagerCanonicalPathReducer {
    pub(crate) fn new() -> Self {
        Self {
            states: vec![InnerPath::new(true)],
            machine: CanonicalPathMachine::new(),
        }
    }

    pub(crate) fn root(&self, arena: &CanonicalBytesArena) -> CanonicalPathReducerHandle {
        CanonicalPathReducerHandle {
            state: PathStateId(0),
            bytes: arena.empty(),
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_state_bytes(&self) -> usize {
        self.states.capacity() * std::mem::size_of::<InnerPath>()
    }

    #[cfg(test)]
    pub(crate) fn retained_frame_bytes(&self) -> usize { self.machine.frames.retained_bytes() }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats {
        CanonicalPathAdmissionStats {
            states: self.states.len(),
            frames: self.machine.frames.0.slots.len(),
            ..self.machine.stats
        }
    }

    pub(crate) fn active_obligation(
        &self,
        handle: CanonicalPathReducerHandle,
    ) -> Option<CanonicalPathObligationProgress> {
        self.machine
            .active_obligation_in(&self.states[handle.state.0], handle.bytes.len())
    }

    pub(crate) fn completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
    ) -> Result<Option<CanonicalPathCompletedObligation>, CanonicalPathAdmissionError> {
        self.machine.completed_obligation(
            &ArenaPathBytes {
                arena,
                bytes: handle.bytes,
            },
            &self.states[handle.state.0],
        )
    }
    pub(crate) fn freeze_completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
    ) -> Result<Option<FrozenCanonicalPathObligation>, CanonicalPathAdmissionError> {
        Ok(self.completed_obligation(arena, handle)?.map(|completed| {
            FrozenCanonicalPathObligation {
                obligation: completed.obligation,
                payload: arena.freeze(completed.payload),
                payload_end: completed.payload_end,
            }
        }))
    }

    pub(crate) fn edge(
        &mut self,
        arena: &mut CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        located: CanonicalLocatedByte,
    ) -> Result<(CanonicalPathReducerHandle, CanonicalPathTransition), CanonicalPathAdmissionError>
    {
        let current = &self.states[handle.state.0];
        let byte = self.machine.prepare_edge(current, located)?;
        let bytes = arena.append(handle.bytes, located).map_err(|_| {
            CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::SizeOverflow,
                byte.source,
            )
        })?;
        let (state, transition) =
            self.machine
                .advance(&ArenaPathBytes { arena, bytes }, current, byte);
        let id = PathStateId(self.states.len());
        self.states.push(state);
        Ok((CanonicalPathReducerHandle { state: id, bytes }, transition))
    }

    pub(crate) fn value(
        &self,
        handle: CanonicalPathReducerHandle,
        has_value: bool,
        source: CanonicalPathSource,
    ) -> Option<Result<CanonicalPathSummary, CanonicalPathAdmissionError>> {
        has_value.then(|| {
            self.machine
                .finish_path(&self.states[handle.state.0], source)
        })
    }

    pub(crate) fn resolve_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<CanonicalPathReducerHandle, CanonicalPathAdmissionError> {
        let state = self.machine.resolved_state(
            &ArenaPathBytes {
                arena,
                bytes: handle.bytes,
            },
            &self.states[handle.state.0],
            id,
            resolution,
        )?;
        let next = PathStateId(self.states.len());
        self.states.push(state);
        Ok(CanonicalPathReducerHandle {
            state: next,
            bytes: handle.bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn reject_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        id: CanonicalPathObligationId,
        source: CanonicalPathSource,
    ) -> Result<CanonicalPathReducerHandle, CanonicalPathAdmissionError> {
        let mut state = self.states[handle.state.0].clone();
        self.machine.reject_path_obligation(
            &ArenaPathBytes {
                arena,
                bytes: handle.bytes,
            },
            &mut state,
            id,
            source,
        )?;
        let next = PathStateId(self.states.len());
        self.states.push(state);
        Ok(CanonicalPathReducerHandle {
            state: next,
            bytes: handle.bytes,
        })
    }
}

pub(crate) struct EagerCanonicalPathTopologyAdmission {
    reducer: EagerCanonicalPathReducer,
    arena: CanonicalBytesArena,
    current: CanonicalPathReducerHandle,
    restore: Vec<CanonicalPathReducerHandle>,
    root_left: bool,
}

impl Default for EagerCanonicalPathTopologyAdmission {
    fn default() -> Self { Self::new() }
}

impl EagerCanonicalPathTopologyAdmission {
    pub(crate) fn new() -> Self {
        let arena = CanonicalBytesArena::new();
        let reducer = EagerCanonicalPathReducer::new();
        let current = reducer.root(&arena);
        Self {
            reducer,
            arena,
            current,
            restore: Vec::new(),
            root_left: false,
        }
    }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats { self.reducer.stats() }

    pub(crate) fn active_obligation(&self) -> Option<CanonicalPathObligationProgress> {
        self.reducer.active_obligation(self.current)
    }

    pub(crate) fn freeze_completed_obligation(
        &self,
    ) -> Result<Option<FrozenCanonicalPathObligation>, CanonicalPathAdmissionError> {
        self.reducer
            .freeze_completed_obligation(&self.arena, self.current)
    }

    pub(crate) fn edge(
        &mut self,
        byte: LocatedPathByte,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
        if self.root_left {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::TopologyAfterRootLeave,
                byte.source,
            ));
        }
        let previous = self.current;
        let (current, transition) = self.reducer.edge(
            &mut self.arena,
            previous,
            CanonicalLocatedByte::new(
                byte.byte,
                CanonicalByteProvenance::new(byte.source.source, byte.source.offset),
            ),
        )?;
        self.restore.push(previous);
        self.current = current;
        Ok(transition)
    }

    pub(crate) fn leave(
        &mut self,
        source: CanonicalPathSource,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
        if let Some(previous) = self.restore.pop() {
            self.current = previous;
            return Ok(CanonicalPathTransition::Restored {
                active_obligation: self.active_obligation(),
            });
        }
        if self.root_left {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::TopologyUnderflow,
                source,
            ));
        }
        self.root_left = true;
        Ok(CanonicalPathTransition::Restored {
            active_obligation: None,
        })
    }

    pub(crate) fn value(
        &self,
        has_value: bool,
        source: CanonicalPathSource,
    ) -> Option<Result<CanonicalPathSummary, CanonicalPathAdmissionError>> {
        self.reducer.value(self.current, has_value, source)
    }
    pub(crate) fn finish(
        &self,
        source: CanonicalPathSource,
    ) -> Result<(), CanonicalPathAdmissionError> {
        if let Some(obligation) = self.active_obligation() {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                obligation.obligation.tag_source,
            ));
        }
        if !self.root_left || !self.restore.is_empty() {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::TopologyIncomplete,
                source,
            ));
        }
        Ok(())
    }

    pub(crate) fn resolve_obligation(
        &mut self,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        self.current =
            self.reducer
                .resolve_obligation(&self.arena, self.current, id, resolution)?;
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn reject_obligation(
        &mut self,
        id: CanonicalPathObligationId,
        source: CanonicalPathSource,
    ) -> Result<(), CanonicalPathAdmissionError> {
        self.current = self
            .reducer
            .reject_obligation(&self.arena, self.current, id, source)?;
        Ok(())
    }
}

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
        initialize_reference_payload(&self.current, &mut self.machine.frames.slots, self.consumed);
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
        initialize_reference_payload(&self.current, &mut self.machine.frames.slots, consumed);
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
        realize_selected_payload(&current, &mut frames, self.graph_stamp);
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
            let mut admission = EagerCanonicalPathTopologyAdmission::new();
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
        let mut reducer = EagerCanonicalPathReducer::new();
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
        assert!(reducer
            .freeze_completed_obligation(&arena, handle)
            .unwrap()
            .is_none());
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
                    status: ObligationStatus::Countdown { remaining },
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
        let selected_bytes = arena.located_bytes(handle.bytes).collect::<Vec<_>>();
        let selected_frames = reducer.machine.frames.slots.clone();
        let selected_clock = reducer.machine.next_obligation;
        let shared = std::cell::RefCell::new((&mut reducer, &mut arena));
        let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
            cases: 32,
            ..ProptestConfig::default()
        });
        runner
            .run(
                &proptest::collection::vec((any::<u8>(), 9u32..500, 0usize..8192), 0..65),
                |suffix| {
                    let mut shared = shared.borrow_mut();
                    let (reducer, arena) = &mut *shared;
                    let retained_states = reducer.states.clone();
                    let mut fork = handle;
                    let mut expected_bytes = selected_bytes.clone();
                    for (byte, source, offset) in suffix {
                        let located = CanonicalLocatedByte::new(
                            byte,
                            CanonicalByteProvenance::new(source, offset),
                        );
                        let (next, event) = reducer.edge(arena, fork, located).unwrap();
                        fork = next;
                        expected_bytes.push(located);
                        assert_eq!(event, CanonicalPathTransition::Continue);
                        assert_eq!(reducer.states[fork.state.0], original);
                        assert_eq!(reducer.machine.frames.slots, selected_frames);
                        assert_eq!(reducer.machine.next_obligation, selected_clock);
                        assert_eq!(reducer.active_obligation(fork), None);
                        assert!(reducer
                            .freeze_completed_obligation(&arena, fork)
                            .unwrap()
                            .is_none());
                        assert_eq!(
                            reducer.value(fork, true, CanonicalPathSource::new(0)),
                            Some(Err(error))
                        );
                        assert_eq!(
                            reducer.value(fork, false, CanonicalPathSource::new(0)),
                            None
                        );
                    }
                    assert_eq!(
                        arena.located_bytes(fork.bytes).collect::<Vec<_>>(),
                        expected_bytes
                    );
                    assert_eq!(
                        arena.located_bytes(handle.bytes).collect::<Vec<_>>(),
                        selected_bytes
                    );
                    assert_eq!(
                        &reducer.states[..retained_states.len()],
                        retained_states.as_slice()
                    );
                    let before_failure = (reducer.stats(), reducer.machine.next_obligation);
                    let refused = reducer
                        .resolve_obligation(
                            &arena,
                            fork,
                            emitted.id,
                            CanonicalPathObligationResolution::NestedEpm1 {
                                mode: NestedEpmMode::Map,
                                entries_stable: true,
                            },
                        )
                        .unwrap_err();
                    assert_eq!(
                        refused,
                        CanonicalPathAdmissionError::new(
                            CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                            if payload_started {
                                emitted.tag_source
                            } else {
                                CanonicalPathSource::new(0)
                            },
                        )
                    );
                    assert_eq!(
                        (reducer.stats(), reducer.machine.next_obligation),
                        before_failure
                    );
                    assert_eq!(reducer.machine.frames.slots, selected_frames);
                    assert_eq!(reducer.states[fork.state.0], original);
                    Ok(())
                },
            )
            .unwrap();
        drop(shared);
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

fn persistent_owner_fixture(layers: &[(u64, u64)], seed: u16) -> (FrameHistory, InnerPath) {
    let mut history = FrameHistory::new();
    let mut tail = None;
    for kind in 0..6 {
        tail = Some(history.push(frame(kind, seed.wrapping_add(u16::from(kind)), tail)));
    }
    let shared_tail = tail;
    let mut current = InnerPath {
        escape_allowed: false,
        stack: tail,
        status: PathStatus::SplitComplete,
        completed_segments: u64::from(seed) + 1,
        first_tag: Some((ESCAPE, source(seed))),
        stable: false,
    };
    for (index, &(remaining, path_remaining)) in layers.iter().enumerate().rev() {
        let id = history.push(Frame::Region {
            remaining,
            previous: Some(CanonicalBytesArena::new().empty()),
            phase: RegionPhase::Path {
                declared_len: path_remaining,
                remaining: path_remaining,
                start_depth: index + 17,
                path: current,
            },
            next: shared_tail,
        });
        current = InnerPath {
            escape_allowed: index % 2 == 0,
            stack: Some(id),
            status: if index % 3 == 0 {
                PathStatus::Dead(error(seed))
            } else {
                PathStatus::Open
            },
            completed_segments: index as u64 + 5,
            first_tag: Some((EPATHMAP, source(seed.wrapping_add(index as u16)))),
            stable: index % 2 != 0,
        };
    }
    (history, current)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn persistent_owner_capture_and_forcing_preserve_exact_shared_histories(
        layers in proptest::collection::vec((1u16..1024, 1u16..1024), 1..40),
        seed in any::<u16>(),
        advance in any::<u16>(),
    ) {
        let layers: Vec<_> = layers.into_iter().map(|(a, b)| (u64::from(a), u64::from(b))).collect();
        let (mut history, original) = persistent_owner_fixture(&layers, seed);
        let mut byte_arena = CanonicalBytesArena::new();
        let mut span = byte_arena.empty();
        let located: Vec<_> = (0..3).map(|offset| CanonicalLocatedByte::new(
            (seed as u8).wrapping_add(offset as u8),
            CanonicalByteProvenance::new(13, usize::from(seed) + offset * 17),
        )).collect();
        for byte in &located {
            span = byte_arena.append(span, *byte).unwrap();
        }
        for frame in &mut history.slots {
            if let Frame::Region { previous, .. } = frame {
                *previous = Some(span);
            }
        }
        let physical = history.slots.clone();
        let stamp = usize::from(seed);
        let mut owners = PersistentRegionOwners::new();
        let mut focus = original.clone();
        let mut root = None;
        let mut roots = Vec::new();
        while let Some((next, child)) = owners.capture(&history, &focus, root, stamp) {
            assert_eq!(history.slots, physical);
            let cell = &owners.entries[next.0];
            assert_eq!(cell.owner, focus);
            assert_eq!(cell.parent, root);
            assert!(cell.parent.is_none_or(|parent| parent.0 < next.0));
            roots.push(next);
            let mut all = next;
            let mut path = next;
            let mut outermost = next;
            let mut cursor = cell.parent;
            while let Some(parent) = cursor {
                let older = &owners.entries[parent.0];
                if older.region_end.min(older.path_end) <= owners.entries[all.0].region_end.min(owners.entries[all.0].path_end) {
                    all = parent;
                }
                if older.path_end < owners.entries[path.0].path_end {
                    path = parent;
                }
                outermost = parent;
                cursor = older.parent;
            }
            assert_eq!(cell.all_boundary, RegionBoundary { end: owners.entries[all.0].region_end.min(owners.entries[all.0].path_end), owner: all });
            assert_eq!(cell.path_boundary, RegionBoundary { end: owners.entries[path.0].path_end, owner: path });
            assert_eq!(cell.outermost, outermost);
            root = Some(next);
            focus = child;
        }
        assert_eq!(roots.len(), layers.len());
        let retained = owners.entries.clone();
        let k = usize::from(seed) % roots.len();
        let old = &retained[roots[k].0];
        let (fork, fork_focus) = owners.capture(&history, &old.owner, old.parent, stamp).unwrap();
        assert_eq!(&owners.entries[..retained.len()], retained.as_slice());
        let restored = owners.materialize(&mut history, fork_focus, Some(fork), stamp);
        assert_eq!(&history.slots[..physical.len()], physical.as_slice());
        assert_eq!(history.slots.len(), physical.len() + k + 1);
        assert_eq!(
            super::opaque_interior_tests::normalize_graph(restored.clone(), &history.slots),
            super::opaque_interior_tests::normalize_graph(original.clone(), &physical),
        );
        let restored_history = history.slots.clone();
        let width = layers.iter().map(|(a, b)| (*a).min(*b)).min().unwrap();
        let delta = u64::from(advance) % (width + 1);
        let mut projected = physical.clone();
        for owner in &retained {
            let Frame::Region { remaining, phase: RegionPhase::Path { remaining: path_remaining, .. }, .. } = &mut projected[owner.owner.stack.unwrap().0] else { unreachable!() };
            *remaining -= delta;
            *path_remaining -= delta;
        }
        let materialized = owners.materialize(&mut history, focus, root, stamp + delta as usize);
        assert_eq!(history.slots.len(), restored_history.len() + layers.len());
        assert_eq!(&history.slots[..restored_history.len()], restored_history.as_slice());
        assert_eq!(
            super::opaque_interior_tests::normalize_graph(materialized, &history.slots),
            super::opaque_interior_tests::normalize_graph(original, &projected),
        );
        assert_eq!(
            super::opaque_interior_tests::normalize_graph(restored.clone(), &history.slots),
            super::opaque_interior_tests::normalize_graph(restored, &restored_history),
        );
        let extended = byte_arena.append(span, CanonicalLocatedByte::new(
            0xff, CanonicalByteProvenance::new(29, 9001),
        )).unwrap();
        assert_eq!(byte_arena.located_bytes(span).collect::<Vec<_>>(), located);
        assert_eq!(byte_arena.located_bytes(extended).count(), located.len() + 1);
    }
}

#[test]
fn persistent_owner_endpoints_preserve_full_width_and_opposite_ties() {
    let (mut history, original) = persistent_owner_fixture(&[(u64::MAX, u64::MAX); 2], 3);
    let mut owners = PersistentRegionOwners::new();
    let (outer, child) = owners
        .capture(&history, &original, None, usize::MAX)
        .unwrap();
    let (inner, focus) = owners
        .capture(&history, &child, Some(outer), usize::MAX)
        .unwrap();
    assert_eq!(owners.entries[inner.0].all_boundary.owner, outer);
    assert_eq!(owners.entries[inner.0].path_boundary.owner, inner);
    assert_eq!(
        owners.entries[inner.0].region_end,
        usize::MAX as u128 + u128::from(u64::MAX)
    );
    let before = history.slots.clone();
    let restored = owners.materialize(&mut history, focus, Some(inner), usize::MAX);
    assert_eq!(
        super::opaque_interior_tests::normalize_graph(restored, &history.slots),
        super::opaque_interior_tests::normalize_graph(original, &before),
    );
}

#[test]
fn persistent_owner_capture_forcing_and_drop_use_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 20_000;
            let (mut history, mut focus) = persistent_owner_fixture(&vec![(1, 1); depth], 7);
            let original_frames = history.slots.len();
            let mut owners = PersistentRegionOwners::new();
            let mut root = None;
            while let Some((next, child)) = owners.capture(&history, &focus, root, 0) {
                root = Some(next);
                focus = child;
            }
            assert_eq!(owners.entries.len(), depth);
            assert_eq!(history.slots.len(), original_frames);
            let restored = owners.materialize(&mut history, focus, root, 1);
            assert_eq!(history.slots.len(), original_frames + depth);
            assert!(restored.stack.is_some());
            drop(restored);
            drop(owners);
            drop(history);
        })
        .unwrap()
        .join()
        .unwrap();
}

fn initialize_reference_payload<Span>(root: &InnerPath, frames: &mut [Frame<Span>], stamp: usize) {
    let mut top = root.stack;
    while let Some(id) = top {
        match &mut frames[id.0] {
            Frame::Obligation {
                obligation,
                status,
                payload_start,
                ..
            } => {
                if *status == ObligationStatus::Payload {
                    assert_eq!(
                        stamp, *payload_start,
                        "reference markers may only initialize at payload birth"
                    );
                    *status = ObligationStatus::Countdown {
                        remaining: obligation.payload_len,
                    };
                }
                break;
            }
            Frame::Region {
                phase: RegionPhase::Path { path, .. },
                ..
            } => top = path.stack,
            _ => break,
        }
    }
}

fn realize_selected_payload<Span>(root: &InnerPath, frames: &mut [Frame<Span>], stamp: usize) {
    let mut top = root.stack;
    while let Some(id) = top {
        match &mut frames[id.0] {
            Frame::Obligation {
                obligation,
                status,
                payload_start,
                ..
            } => {
                if *status == ObligationStatus::Payload {
                    assert!(stamp >= *payload_start);
                    let end = *payload_start as u128 + u128::from(obligation.payload_len);
                    let remaining = u64::try_from(end.checked_sub(stamp as u128).unwrap()).unwrap();
                    *status = ObligationStatus::Countdown { remaining };
                }
                break;
            }
            Frame::Region {
                phase: RegionPhase::Path { path, .. },
                ..
            } => top = path.stack,
            _ => break,
        }
    }
}

fn normalize_located_graph(
    root: InnerPath,
    frames: &[Frame],
    arena: &CanonicalBytesArena,
) -> (
    InnerPath,
    Vec<Frame>,
    Vec<Option<Vec<CanonicalLocatedByte>>>,
) {
    let (root, mut frames) = super::opaque_interior_tests::normalize_graph(root, frames);
    let spans = frames
        .iter_mut()
        .filter_map(|frame| match frame {
            Frame::Region { previous, .. } => Some(
                previous
                    .take()
                    .map(|span| arena.located_bytes(span).collect::<Vec<_>>()),
            ),
            _ => None,
        })
        .collect();
    (root, frames, spans)
}

#[test]
fn located_graph_oracle_preserves_span_presence_bytes_and_provenance() {
    let mut left = CanonicalBytesArena::new();
    let mut right = CanonicalBytesArena::new();
    let original = CanonicalLocatedByte::new(0x19, CanonicalByteProvenance::new(7, 31));
    let left_span = left.append(left.empty(), original).unwrap();
    right.append(right.empty(), original).unwrap();
    let right_span = right.append(right.empty(), original).unwrap();
    assert_ne!(left_span, right_span);
    let mut root = InnerPath::new(true);
    root.stack = Some(FrameId(0));
    let frame = |previous| Frame::Region {
        remaining: 7,
        previous,
        phase: RegionPhase::PathLength(VarintState::default()),
        next: None,
    };
    let baseline = normalize_located_graph(root.clone(), &[frame(Some(left_span))], &left);
    assert_eq!(
        baseline,
        normalize_located_graph(root.clone(), &[frame(Some(right_span))], &right)
    );
    for changed in [
        CanonicalLocatedByte::new(0xe3, original.provenance),
        CanonicalLocatedByte::new(original.byte, CanonicalByteProvenance::new(8, 31)),
        CanonicalLocatedByte::new(original.byte, CanonicalByteProvenance::new(7, 32)),
    ] {
        let span = right.append(right.empty(), changed).unwrap();
        assert_ne!(
            baseline,
            normalize_located_graph(root.clone(), &[frame(Some(span))], &right)
        );
    }
    assert_ne!(
        normalize_located_graph(root.clone(), &[frame(None)], &left),
        normalize_located_graph(root, &[frame(Some(left.empty()))], &left),
    );
}

fn assert_focused_persistent_matches(
    reference: &EagerCanonicalPathReducer,
    reference_arena: &CanonicalBytesArena,
    expected: CanonicalPathReducerHandle,
    focused: &CanonicalPathReducer,
    focused_arena: &CanonicalBytesArena,
    actual: CanonicalPathReducerHandle,
) {
    assert_eq!(
        reference.active_obligation(expected),
        focused.active_obligation(actual)
    );
    let completed =
        |result: Result<Option<CanonicalPathCompletedObligation>, CanonicalPathAdmissionError>,
         arena: &CanonicalBytesArena| {
            result.map(|payload| {
                payload.map(|payload| {
                    (
                        payload.obligation,
                        payload.payload_end,
                        arena.located_bytes(payload.payload).collect::<Vec<_>>(),
                    )
                })
            })
        };
    assert_eq!(
        completed(
            reference.completed_obligation(reference_arena, expected),
            reference_arena
        ),
        completed(
            focused.completed_obligation(focused_arena, actual),
            focused_arena
        ),
    );
    for has_value in [false, true] {
        assert_eq!(
            reference.value(expected, has_value, source(91)),
            focused.value(actual, has_value, source(91))
        );
    }
    assert_eq!(
        reference.machine.next_obligation,
        focused.machine.next_obligation
    );
    assert_eq!(
        reference_arena
            .located_bytes(expected.bytes)
            .collect::<Vec<_>>(),
        focused_arena
            .located_bytes(actual.bytes)
            .collect::<Vec<_>>()
    );
    let state = focused.state(actual);
    let mut frames = FrameHistory {
        slots: focused.machine.frames.slots.clone(),
    };
    let mut root = focused.owners.materialize(
        &mut frames,
        state.current.clone(),
        state.owners,
        state.stamp,
    );
    if let Some(error) = state.fault {
        root.kill(error);
    }
    realize_selected_payload(&root, &mut frames.slots, state.stamp);
    assert_eq!(
        normalize_located_graph(root, &frames.slots, focused_arena),
        normalize_located_graph(
            reference.states[expected.state.0].clone(),
            &reference.machine.frames.slots,
            reference_arena,
        ),
    );
}

#[test]
fn ownerless_rejected_payload_retains_hidden_countdown_after_dead_bytes() {
    let mut reference = EagerCanonicalPathReducer::new();
    let mut focused = CanonicalPathReducer::new();
    let mut reference_arena = CanonicalBytesArena::new();
    let mut arena = CanonicalBytesArena::new();
    let mut expected = reference.root(&reference_arena);
    let mut actual = focused.root(&arena);
    for (index, byte) in [EPATHMAP_MAP, 4].into_iter().enumerate() {
        let located =
            CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(17, 103 + index * 19));
        expected = reference
            .edge(&mut reference_arena, expected, located)
            .unwrap()
            .0;
        actual = focused.edge(&mut arena, actual, located).unwrap().0;
    }
    let id = reference.active_obligation(expected).unwrap().obligation.id;
    let original = actual;
    expected = reference
        .reject_obligation(&reference_arena, expected, id, source(53))
        .unwrap();
    actual = focused
        .reject_obligation(&arena, actual, id, source(53))
        .unwrap();
    let retained = reference.states[expected.state.0].clone();
    let clock = reference.machine.next_obligation;
    for index in 0..32 {
        let located = CanonicalLocatedByte::new(
            index,
            CanonicalByteProvenance::new(91, 997 - index as usize * 7),
        );
        expected = reference
            .edge(&mut reference_arena, expected, located)
            .unwrap()
            .0;
        actual = focused.edge(&mut arena, actual, located).unwrap().0;
        assert_eq!(reference.states[expected.state.0], retained);
        assert_eq!(reference.machine.next_obligation, clock);
        assert_eq!(focused.active_obligation(actual), None);
        assert_eq!(focused.active_obligation(original).unwrap().remaining, 4);
        let Frame::Obligation {
            status: ObligationStatus::Countdown { remaining },
            ..
        } = &reference.machine.frames[retained.stack.unwrap().0]
        else {
            panic!("retained payload")
        };
        assert_eq!(*remaining, 4);
        assert_focused_persistent_matches(
            &reference,
            &reference_arena,
            expected,
            &focused,
            &arena,
            actual,
        );
    }
    let mut mutated = reference.machine.frames.slots.clone();
    let Frame::Obligation {
        status: ObligationStatus::Countdown { remaining },
        ..
    } = &mut mutated[retained.stack.unwrap().0]
    else {
        panic!("retained payload")
    };
    *remaining = 2;
    assert_ne!(
        super::opaque_interior_tests::normalize_graph(retained.clone(), &mutated),
        super::opaque_interior_tests::normalize_graph(retained, &reference.machine.frames.slots),
    );
}

#[test]
fn scalar_payload_lengths_preserve_exact_boundary_sources_and_fresh_ids() {
    assert_eq!(
        std::mem::size_of::<EagerFrameHistory>(),
        std::mem::size_of::<FrameHistory>()
    );
    for length in [0u8, 1, 2, 31, 127] {
        let mut reference = EagerCanonicalPathReducer::new();
        let mut focused = CanonicalPathReducer::new();
        let mut reference_arena = CanonicalBytesArena::new();
        let mut arena = CanonicalBytesArena::new();
        let mut expected = reference.root(&reference_arena);
        let mut actual = focused.root(&arena);
        for (index, byte) in [EPATHMAP_MAP, length].into_iter().enumerate() {
            let located =
                CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(7, 101 + index * 23));
            let (left, a) = reference
                .edge(&mut reference_arena, expected, located)
                .unwrap();
            let (right, b) = focused.edge(&mut arena, actual, located).unwrap();
            expected = left;
            actual = right;
            assert_eq!(a, b);
        }
        let active = reference.active_obligation(expected).unwrap();
        let initial_frames = focused.machine.frames.slots.len();
        let initial_leaf = focused.state(actual).current.stack;
        assert_eq!(active.remaining, u64::from(length));
        assert_eq!(active.awaiting_resolution, length == 0);
        assert_eq!(
            reference.machine.next_obligation,
            active.obligation.id.0 + 1
        );
        for index in 0..length {
            let located = CanonicalLocatedByte::new(
                index,
                CanonicalByteProvenance::new(11 + u32::from(index), 4096 - usize::from(index) * 17),
            );
            let (left, a) = reference
                .edge(&mut reference_arena, expected, located)
                .unwrap();
            let (right, b) = focused.edge(&mut arena, actual, located).unwrap();
            expected = left;
            actual = right;
            assert_eq!(a, b);
            assert_eq!(b, CanonicalPathTransition::ObligationByte {
                id: active.obligation.id,
                byte: LocatedPathByte::in_source(
                    located.byte,
                    located.provenance.source,
                    located.provenance.offset
                ),
                last: index + 1 == length,
            });
            assert_eq!(
                focused.machine.frames.slots.len(),
                initial_frames + usize::from(index + 1 == length)
            );
            if index + 1 != length {
                assert_eq!(focused.state(actual).current.stack, initial_leaf);
            }
            assert_eq!(
                focused.active_obligation(actual).unwrap().remaining,
                u64::from(length - index - 1)
            );
            assert_focused_persistent_matches(
                &reference,
                &reference_arena,
                expected,
                &focused,
                &arena,
                actual,
            );
        }
        let completed = focused
            .completed_obligation(&arena, actual)
            .unwrap()
            .unwrap();
        assert_eq!(completed.payload.len(), usize::from(length));
        assert_eq!(
            completed.payload_end,
            if length == 0 {
                CanonicalPathSource::in_source(7, 124)
            } else {
                CanonicalPathSource::in_source(
                    10 + u32::from(length),
                    4096 - usize::from(length - 1) * 17,
                )
            }
        );
        assert_focused_persistent_matches(
            &reference,
            &reference_arena,
            expected,
            &focused,
            &arena,
            actual,
        );
    }
}

#[test]
fn full_width_payload_lengths_keep_exact_residuals_and_historical_forks() {
    for length in [128, 255, 65_536, u64::MAX - 1, u64::MAX] {
        let mut reference = EagerCanonicalPathReducer::new();
        let mut focused = CanonicalPathReducer::new();
        let mut reference_arena = CanonicalBytesArena::new();
        let mut arena = CanonicalBytesArena::new();
        let mut expected = reference.root(&reference_arena);
        let mut actual = focused.root(&arena);
        let mut header = vec![EPATHMAP_MAP];
        prost::encoding::encode_varint(length, &mut header);
        for (index, byte) in header.into_iter().enumerate() {
            let located =
                CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(23, 101 + index * 13));
            expected = reference
                .edge(&mut reference_arena, expected, located)
                .unwrap()
                .0;
            actual = focused.edge(&mut arena, actual, located).unwrap().0;
        }
        let original = (expected, actual);
        let id = focused.active_obligation(actual).unwrap().obligation.id;
        let initial_frames = focused.machine.frames.slots.len();
        for branch in 0..3 {
            (expected, actual) = original;
            for index in 0..17 {
                let located = CanonicalLocatedByte::new(
                    index as u8,
                    CanonicalByteProvenance::new(31 + branch, 901 - index * 11),
                );
                expected = reference
                    .edge(&mut reference_arena, expected, located)
                    .unwrap()
                    .0;
                actual = focused.edge(&mut arena, actual, located).unwrap().0;
                let progress = focused.active_obligation(actual).unwrap();
                assert_eq!(progress.obligation.id, id);
                assert_eq!(progress.remaining, length - index as u64 - 1);
                assert!(!progress.awaiting_resolution);
                assert_eq!(focused.machine.frames.slots.len(), initial_frames);
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    expected,
                    &focused,
                    &arena,
                    actual,
                );
                assert_eq!(
                    focused.active_obligation(original.1).unwrap().remaining,
                    length
                );
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    original.0,
                    &focused,
                    &arena,
                    original.1,
                );
            }
        }
    }
}

#[test]
fn persistent_opaque_state_reuse_preserves_nested_prefixes_and_divergent_bytes() {
    for depth in [0, 1, 8] {
        for width in [0, 1, 2, 17, 129] {
            let mut bytes = vec![EPATHMAP_MAP];
            prost::encoding::encode_varint(width, &mut bytes);
            bytes.resize(bytes.len() + width as usize, 0xa7);
            for _ in 0..depth {
                let mut body = Vec::new();
                prost::encoding::encode_varint(bytes.len() as u64, &mut body);
                body.extend_from_slice(&bytes);
                bytes = vec![EPATHMAP];
                prost::encoding::encode_varint(body.len() as u64, &mut bytes);
                bytes.extend_from_slice(&body);
            }
            let mut reference = EagerCanonicalPathReducer::new();
            let mut focused = CanonicalPathReducer::new();
            let mut reference_arena = CanonicalBytesArena::new();
            let mut arena = CanonicalBytesArena::new();
            let mut expected = reference.root(&reference_arena);
            let mut actual = focused.root(&arena);
            let mut history = vec![(expected, actual)];
            for (index, byte) in bytes.into_iter().enumerate() {
                let interior = focused.active_obligation(actual).is_some_and(|progress| {
                    !progress.awaiting_resolution && progress.remaining > 1
                });
                let saved = actual;
                let allocation = (
                    focused.states.len(),
                    focused.machine.frames.slots.len(),
                    focused.owners.entries.len(),
                );
                let clock = focused.machine.next_obligation;
                let located = CanonicalLocatedByte::new(
                    byte,
                    CanonicalByteProvenance::new(31 + index as u32 % 3, 901 + index * 17),
                );
                let (left, a) = reference
                    .edge(&mut reference_arena, expected, located)
                    .unwrap();
                let (right, b) = focused.edge(&mut arena, actual, located).unwrap();
                expected = left;
                actual = right;
                assert_eq!(a, b);
                if interior {
                    assert_eq!(focused.machine.next_obligation, clock);
                    assert_eq!(actual.state, saved.state);
                    assert_ne!(actual.bytes, saved.bytes);
                    assert_eq!(
                        (
                            focused.states.len(),
                            focused.machine.frames.slots.len(),
                            focused.owners.entries.len()
                        ),
                        allocation
                    );
                }
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    expected,
                    &focused,
                    &arena,
                    actual,
                );
                history.push((expected, actual));
            }
            for (left, right) in history {
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    left,
                    &focused,
                    &arena,
                    right,
                );
                let Some(progress) = focused.active_obligation(right) else {
                    continue;
                };
                if progress.remaining <= 1 || progress.awaiting_resolution {
                    continue;
                }
                let mut forks = Vec::new();
                for byte in [0x19, 0xe3] {
                    let located =
                        CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(71, 997));
                    let (expected, a) =
                        reference.edge(&mut reference_arena, left, located).unwrap();
                    let (actual, b) = focused.edge(&mut arena, right, located).unwrap();
                    assert_eq!(a, b);
                    assert_eq!(actual.state, right.state);
                    assert_eq!(
                        focused.active_obligation(actual).unwrap().obligation.id,
                        progress.obligation.id
                    );
                    assert_focused_persistent_matches(
                        &reference,
                        &reference_arena,
                        expected,
                        &focused,
                        &arena,
                        actual,
                    );
                    forks.push(actual);
                }
                assert_eq!(forks[0].state, forks[1].state);
                assert_eq!(forks[0].bytes.len(), forks[1].bytes.len());
                assert_ne!(forks[0], forks[1]);
                assert!(!arena.content_eq(forks[0].bytes, forks[1].bytes));
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn persistent_opaque_reuse_guard_matches_widened_native_boundaries(
        start in prop_oneof![any::<usize>(), Just(0usize), Just(usize::MAX - 1), Just(usize::MAX)],
        consumed in 0usize..32,
        declared in prop_oneof![0u64..64, Just(u64::MAX)],
    ) {
        let stamp = start.wrapping_add(consumed);
        for status in [PathStatus::Open, PathStatus::SplitComplete, PathStatus::Dead(error(7))] {
            for fault in [None, Some(error(11))] {
                for phase in [ObligationStatus::Payload,
                    ObligationStatus::AwaitingResolution { payload_end: source(17) },
                    ObligationStatus::Countdown { remaining: declared }] {
                    let mut reducer = CanonicalPathReducer::new();
                    let id = CanonicalPathObligationId(31);
                    let top = reducer.machine.frames.push(Frame::Obligation {
                        obligation: CanonicalPathObligation {
                            id, kind: CanonicalPathObligationKind::NestedEpm1,
                            payload_len: declared, tag_source: source(23),
                        },
                        status: phase, payload_start: start, next: None,
                    });
                    let mut current = InnerPath::new(true);
                    current.stack = Some(top);
                    current.status = status;
                    let state = PersistentFocusedPath { current, owners: None, stamp, fault };
                    let next = stamp.wrapping_add(1);
                    let expected = fault.is_none() && status == PathStatus::Open
                        && phase == ObligationStatus::Payload && stamp >= start
                        && stamp != usize::MAX
                        && (next as u128) < start as u128 + declared as u128;
                    prop_assert_eq!(reducer.reusable_payload(&state, next), expected.then_some(id));
                    prop_assert_eq!(reducer.reusable_payload(&state, stamp.wrapping_add(2)), None);
                }
            }
        }
    }
}

#[test]
fn persistent_opaque_reuse_excludes_every_malformed_owner_boundary() {
    for depth in [1, 3, 8] {
        for room in [0u8, 1, 2, 5] {
            let mut bytes = vec![EPATHMAP, 3 + room, 2 + room, EPATHMAP_MAP, 12];
            bytes.resize(bytes.len() + room as usize, 0xa7);
            for _ in 1..depth {
                let mut body = Vec::new();
                prost::encoding::encode_varint(bytes.len() as u64, &mut body);
                body.extend_from_slice(&bytes);
                bytes = vec![EPATHMAP];
                prost::encoding::encode_varint(body.len() as u64, &mut bytes);
                bytes.extend_from_slice(&body);
            }
            bytes.extend_from_slice(&[0x19, 0xe3, 0xff]);
            let mut reference = EagerCanonicalPathReducer::new();
            let mut focused = CanonicalPathReducer::new();
            let mut reference_arena = CanonicalBytesArena::new();
            let mut arena = CanonicalBytesArena::new();
            let mut expected = reference.root(&reference_arena);
            let mut actual = focused.root(&arena);
            let mut history = Vec::new();
            for (offset, byte) in bytes.into_iter().enumerate() {
                let state = focused.state(actual);
                let boundary = state
                    .owners
                    .map(|root| focused.owners.entries[root.0].all_boundary.end);
                let saved = actual;
                let located =
                    CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(43, offset * 7));
                let (left, a) = reference
                    .edge(&mut reference_arena, expected, located)
                    .unwrap();
                let (right, b) = focused.edge(&mut arena, actual, located).unwrap();
                expected = left;
                actual = right;
                assert_eq!(a, b);
                if boundary.is_some_and(|end| actual.bytes.len() as u128 >= end) {
                    assert_ne!(actual.state, saved.state);
                }
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    expected,
                    &focused,
                    &arena,
                    actual,
                );
                history.push((expected, actual));
            }
            for (left, right) in history {
                assert_focused_persistent_matches(
                    &reference,
                    &reference_arena,
                    left,
                    &focused,
                    &arena,
                    right,
                );
            }
        }
    }
}

fn exercise_focused_persistent(bytes: &[u8], resolution_choice: u8, fork_stride: usize) {
    let mut reference = EagerCanonicalPathReducer::new();
    let mut focused = CanonicalPathReducer::new();
    let mut reference_arena = CanonicalBytesArena::new();
    let mut focused_arena = CanonicalBytesArena::new();
    let mut expected = reference.root(&reference_arena);
    let mut actual = focused.root(&focused_arena);
    let mut history = vec![(expected, actual)];
    for (offset, &byte) in bytes.iter().enumerate() {
        if fork_stride != 0 && offset % fork_stride == 0 {
            (expected, actual) = history[offset / fork_stride % history.len()];
        }
        let located = CanonicalLocatedByte::new(
            byte,
            CanonicalByteProvenance::new(17 + offset as u32 % 3, 111 + offset * 19),
        );
        let left = reference.edge(&mut reference_arena, expected, located);
        let right = focused.edge(&mut focused_arena, actual, located);
        match (left, right) {
            (Ok((next, event)), Ok((other, observed))) => {
                assert_eq!(event, observed);
                expected = next;
                actual = other;
                history.push((expected, actual));
            }
            (Err(left), Err(right)) => assert_eq!(left, right),
            mismatch => panic!("edge mismatch: {mismatch:?}"),
        }
        assert_focused_persistent_matches(
            &reference,
            &reference_arena,
            expected,
            &focused,
            &focused_arena,
            actual,
        );
        assert_focused_private_branches(
            &mut reference,
            &reference_arena,
            expected,
            &mut focused,
            &focused_arena,
            actual,
        );
        let pending = reference.active_obligation(expected);
        if let Some(pending) = pending {
            let resolution = match resolution_choice % 5 {
                0 => CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: false,
                },
                1 => CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: true,
                },
                2 => CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                },
                3 => CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: false,
                },
                _ => CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Set,
                    entries_stable: false,
                },
            };
            let before = (
                focused.states.clone(),
                focused.machine.frames.slots.clone(),
                focused.owners.entries.clone(),
                focused.machine.next_obligation,
            );
            let left = reference.resolve_obligation(
                &reference_arena,
                expected,
                pending.obligation.id,
                resolution,
            );
            let right = focused.resolve_obligation(
                &focused_arena,
                actual,
                pending.obligation.id,
                resolution,
            );
            match (left, right) {
                (Ok(next), Ok(other)) => {
                    expected = next;
                    actual = other;
                    history.push((expected, actual));
                }
                (Err(left), Err(right)) => {
                    assert_eq!(left, right);
                    assert_eq!(
                        (
                            focused.states.clone(),
                            focused.machine.frames.slots.clone(),
                            focused.owners.entries.clone(),
                            focused.machine.next_obligation
                        ),
                        before
                    );
                }
                mismatch => panic!("resolution mismatch: {mismatch:?}"),
            }
            assert_focused_persistent_matches(
                &reference,
                &reference_arena,
                expected,
                &focused,
                &focused_arena,
                actual,
            );
        }
    }
    for (expected, actual) in history.into_iter().rev() {
        assert_focused_persistent_matches(
            &reference,
            &reference_arena,
            expected,
            &focused,
            &focused_arena,
            actual,
        );
    }
}

fn assert_focused_private_branches(
    reference: &mut EagerCanonicalPathReducer,
    reference_arena: &CanonicalBytesArena,
    expected: CanonicalPathReducerHandle,
    focused: &mut CanonicalPathReducer,
    focused_arena: &CanonicalBytesArena,
    actual: CanonicalPathReducerHandle,
) {
    for id in [
        CanonicalPathObligationId(reference.machine.next_obligation.saturating_sub(1)),
        CanonicalPathObligationId(reference.machine.next_obligation),
    ] {
        for reject in [false, true] {
            let before = (
                focused.states.clone(),
                focused.machine.frames.slots.clone(),
                focused.owners.entries.clone(),
                focused.machine.next_obligation,
            );
            let caller = CanonicalPathSource::in_source(73, 811);
            let resolution = CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: true,
            };
            let (left, right) = if reject {
                (
                    reference.reject_obligation(reference_arena, expected, id, caller),
                    focused.reject_obligation(focused_arena, actual, id, caller),
                )
            } else {
                (
                    reference.resolve_obligation(reference_arena, expected, id, resolution),
                    focused.resolve_obligation(focused_arena, actual, id, resolution),
                )
            };
            match (left, right) {
                (Ok(next), Ok(other)) => {
                    assert_focused_persistent_matches(
                        reference,
                        reference_arena,
                        next,
                        focused,
                        focused_arena,
                        other,
                    );
                    assert_eq!(focused.states.len(), before.0.len() + 1);
                    if reject {
                        assert_eq!(focused.machine.frames.slots, before.1);
                        assert_eq!(focused.owners.entries, before.2);
                    }
                }
                (Err(left), Err(right)) => {
                    assert_eq!(left, right);
                    assert_eq!(focused.states, before.0);
                    assert_eq!(focused.machine.frames.slots, before.1);
                    assert_eq!(focused.owners.entries, before.2);
                }
                mismatch => panic!("private operation mismatch, reject={reject}: {mismatch:?}"),
            }
            assert_eq!(
                &focused.states.direct[..before.0.direct.len()],
                before.0.direct.as_slice()
            );
            assert_eq!(
                &focused.states.linked[..before.0.linked.len()],
                before.0.linked.as_slice()
            );
            assert_eq!(
                &focused.machine.frames.slots[..before.1.len()],
                before.1.as_slice()
            );
            assert_eq!(
                &focused.owners.entries[..before.2.len()],
                before.2.as_slice()
            );
            assert_eq!(focused.machine.next_obligation, before.3);
            assert_focused_persistent_matches(
                reference,
                reference_arena,
                expected,
                focused,
                focused_arena,
                actual,
            );
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn focused_persistent_generated_bytes_and_historical_forks_match_eager(
        bytes in proptest::collection::vec(any::<u8>(), 0..128),
        resolution in 0u8..5,
        fork_stride in 0usize..17,
    ) {
        exercise_focused_persistent(&bytes, resolution, fork_stride);
    }
}

#[test]
fn focused_persistent_nested_frames_and_obligations_match_eager() {
    for leaf in [
        vec![GBYTE_ARRAY, 3, 1, 2, 3],
        vec![ESCAPE, 2, 0xaa, 0xbb],
        vec![EPATHMAP_MAP, 2, 0xaa, 0xbb],
        vec![EPATHMAP, 3, 2, EPATHMAP_MAP, 0],
        vec![EPATHMAP, 5, 4, EPATHMAP, 4, 1, GBOOL_TRUE],
        vec![EPATHMAP, 4, 1, GBOOL_TRUE, 1, GBOOL_TRUE],
        vec![EPATHMAP, 4, 1, GBOOL_TRUE, 1, GBOOL_FALSE],
        vec![EPATHMAP, 4, 1, GBOOL_FALSE, 1, GBOOL_TRUE],
        vec![GSTRING, 2, 0xc2, 0xff],
        vec![ELIST, 2, GBOOL_FALSE, GBOOL_TRUE],
        vec![EPATHMAP, 4, 3, EPATHMAP_MAP, 2, 0xaa],
        vec![EPATHMAP, 3, 2, EPATHMAP_MAP, 2],
    ] {
        let mut bytes = leaf;
        for _ in 0..8 {
            for resolution in 0..5 {
                exercise_focused_persistent(&bytes, resolution, 0);
                let mut extended = bytes.clone();
                extended.extend_from_slice(&[0xaf, 0x33, 0x9d]);
                exercise_focused_persistent(&extended, resolution, 0);
            }
            let mut body = Vec::new();
            prost::encoding::encode_varint(bytes.len() as u64, &mut body);
            body.extend_from_slice(&bytes);
            bytes = vec![EPATHMAP];
            prost::encoding::encode_varint(body.len() as u64, &mut bytes);
            bytes.extend_from_slice(&body);
        }
    }
}

#[test]
fn focused_persistent_outer_refusal_keeps_inner_completion_effects() {
    let key = [EPATHMAP, 5, 4, EPATHMAP, 4, 1, GBOOL_TRUE];
    let mut focused = CanonicalPathReducer::new();
    let mut reference = EagerCanonicalPathReducer::new();
    let mut arena = CanonicalBytesArena::new();
    let mut reference_arena = CanonicalBytesArena::new();
    let mut actual = focused.root(&arena);
    let mut expected = reference.root(&reference_arena);
    for (offset, byte) in key[..key.len() - 1].iter().copied().enumerate() {
        let located =
            CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(19, offset * 17));
        actual = focused.edge(&mut arena, actual, located).unwrap().0;
        expected = reference
            .edge(&mut reference_arena, expected, located)
            .unwrap()
            .0;
    }
    let before = focused.state(actual);
    let saved_frames = focused.machine.frames.slots.clone();
    let saved_owners = focused.owners.entries.clone();
    let stamp = before.stamp;
    let final_byte = CanonicalLocatedByte::new(GBOOL_TRUE, CanonicalByteProvenance::new(29, 719));
    let (next, event) = focused.edge(&mut arena, actual, final_byte).unwrap();
    let (other, expected_event) = reference
        .edge(&mut reference_arena, expected, final_byte)
        .unwrap();
    assert_eq!(event, expected_event);
    assert_eq!(focused.owners.entries, saved_owners);
    assert_eq!(
        &focused.machine.frames.slots[..saved_frames.len()],
        saved_frames.as_slice()
    );
    assert!(focused.machine.frames.slots[saved_frames.len()..]
        .iter()
        .any(|frame| {
            matches!(frame, Frame::Region {
                remaining: 2,
                phase: RegionPhase::PathLength(_),
                ..
            })
        }));
    let mut selected = before.clone();
    selected.fault = Some(CanonicalPathAdmissionError::new(
        CanonicalPathAdmissionErrorKind::Truncated,
        CanonicalPathSource::in_source(29, 719),
    ));
    assert_eq!(focused.state(next), selected);
    assert_eq!(focused.state(actual), before);
    assert_eq!(focused.state(next).stamp, stamp);
    assert_focused_persistent_matches(&reference, &reference_arena, other, &focused, &arena, next);
}

pub(crate) fn nested_persistent_key(depth: usize, width: usize) -> Vec<u8> {
    let mut leaf = vec![GBYTE_ARRAY];
    prost::encoding::encode_varint(width as u64, &mut leaf);
    leaf.resize(leaf.len() + width, 0);
    let mut total = leaf.len();
    let mut headers = Vec::with_capacity(depth);
    for _ in 0..depth {
        let path_length = prost::encoding::encoded_len_varint(total as u64);
        let region_length = total + path_length;
        headers.push((region_length, total));
        total += 1 + prost::encoding::encoded_len_varint(region_length as u64) + path_length;
    }
    let mut bytes = Vec::with_capacity(total);
    for (region_length, path_length) in headers.into_iter().rev() {
        bytes.push(EPATHMAP);
        prost::encoding::encode_varint(region_length as u64, &mut bytes);
        prost::encoding::encode_varint(path_length as u64, &mut bytes);
    }
    bytes.extend(leaf);
    bytes
}

#[test]
fn focused_persistent_shared_prefix_has_no_repeated_ancestor_work() {
    for width in [1usize, 16, 128, 1_024] {
        for depth in [0usize, 1, 8, 32] {
            let bytes = nested_persistent_key(depth, width);
            let mut reducer = CanonicalPathReducer::new();
            let mut arena = CanonicalBytesArena::new();
            let mut current = reducer.root(&arena);
            for (offset, byte) in bytes[..bytes.len() - 1].iter().copied().enumerate() {
                let located = CanonicalLocatedByte::new(
                    byte,
                    CanonicalByteProvenance::new(7, 101 + offset * 13),
                );
                current = reducer.edge(&mut arena, current, located).unwrap().0;
                assert!(reducer.active_obligation(current).is_none());
                assert!(reducer
                    .freeze_completed_obligation(&arena, current)
                    .unwrap()
                    .is_none());
            }
            let prefix = current;
            let saved = reducer.state(prefix);
            for branch in 0..4u8 {
                let located = CanonicalLocatedByte::new(
                    branch,
                    CanonicalByteProvenance::new(7, 101 + (bytes.len() - 1) * 13),
                );
                let endpoint = reducer.edge(&mut arena, prefix, located).unwrap().0;
                assert!(
                    reducer
                        .value(endpoint, true, source(9_999))
                        .unwrap()
                        .unwrap()
                        .canonical_path_stable
                );
                assert!(reducer.active_obligation(prefix).is_none());
                assert_eq!(reducer.state(prefix), saved);
            }
            let stats = reducer.stats();
            let work = reducer.machine.owner_work.get();
            assert_eq!(stats.edges, bytes.len() + 3);
            assert_eq!(stats.states, stats.edges + 1);
            assert_eq!(stats.region_descents, 0);
            assert_eq!(work.rebuilds, 0);
            assert_eq!(work.active_queries, 0);
            assert_eq!(work.completed_queries, 0);
            assert_eq!(work.reifications, 0);
            assert_eq!(reducer.owners.entries.len(), depth);
            assert!(work.settlements <= stats.edges + 4 * depth);
            assert!(stats.frames <= 2 * stats.edges);
            eprintln!(
                "focused persistent topology: width={width}, depth={depth}, branches=4, encoded={}, edges={}, states={}, frames={}, owners={}, settlements={}, state_and_owner_backing={}, frame_backing={}, handle_bytes={}",
                bytes.len(),
                stats.edges,
                stats.states,
                stats.frames,
                reducer.owners.entries.len(),
                work.settlements,
                reducer.retained_state_bytes(),
                reducer.retained_frame_bytes(),
                std::mem::size_of::<CanonicalPathReducerHandle>()
            );
        }
    }
}

#[test]
fn focused_persistent_deep_traversal_and_drop_use_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let depth = 20_000;
            let bytes = nested_persistent_key(depth, 1);
            let mut reducer = CanonicalPathReducer::new();
            let mut arena = CanonicalBytesArena::new();
            let mut current = reducer.root(&arena);
            for (offset, byte) in bytes.iter().copied().enumerate() {
                current = reducer
                    .edge(
                        &mut arena,
                        current,
                        CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(13, offset)),
                    )
                    .unwrap()
                    .0;
            }
            assert!(
                reducer
                    .value(current, true, CanonicalPathSource::new(bytes.len()))
                    .unwrap()
                    .unwrap()
                    .canonical_path_stable
            );
            assert_eq!(reducer.owners.entries.len(), depth);
            assert_eq!(reducer.stats().region_descents, 0);
            assert_eq!(reducer.machine.owner_work.get().reifications, 0);
            drop(reducer);
            drop(arena);
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn focused_persistent_topology_resolution_rejection_and_restore_match_eager() {
    for key in [
        vec![EPATHMAP, 3, 2, EPATHMAP_MAP, 0],
        vec![EPATHMAP, 5, 4, EPATHMAP_MAP, 2, 0x31, 0x32],
        vec![EPATHMAP, 4, 3, EPATHMAP_MAP, 2, 0x31],
    ] {
        for reject in [false, true] {
            let mut actual = CanonicalPathTopologyAdmission::new();
            let mut expected = EagerCanonicalPathTopologyAdmission::new();
            let mut frozen = Vec::new();
            for (offset, byte) in key.iter().copied().enumerate() {
                let located =
                    LocatedPathByte::in_source(byte, 31 + offset as u32 % 2, 501 + offset * 29);
                assert_eq!(actual.edge(located), expected.edge(located));
                assert_eq!(actual.active_obligation(), expected.active_obligation());
                if let Some(left) = actual.freeze_completed_obligation().unwrap() {
                    let right = expected.freeze_completed_obligation().unwrap().unwrap();
                    assert_eq!(left.obligation, right.obligation);
                    assert_eq!(left.payload_end, right.payload_end);
                    let bytes = right.payload.located_bytes().collect::<Vec<_>>();
                    assert_eq!(left.payload.located_bytes().collect::<Vec<_>>(), bytes);
                    let id = left.obligation.id;
                    frozen.push((left, bytes));
                    if reject {
                        assert_eq!(
                            actual.reject_obligation(id, source(707)),
                            expected.reject_obligation(id, source(707))
                        );
                    } else {
                        let resolution = CanonicalPathObligationResolution::NestedEpm1 {
                            mode: NestedEpmMode::Map,
                            entries_stable: true,
                        };
                        assert_eq!(
                            actual.resolve_obligation(id, resolution),
                            expected.resolve_obligation(id, resolution)
                        );
                    }
                } else {
                    assert!(expected.freeze_completed_obligation().unwrap().is_none());
                }
                assert_focused_persistent_matches(
                    &expected.reducer,
                    &expected.arena,
                    expected.current,
                    &actual.reducer,
                    &actual.arena,
                    actual.current,
                );
            }
            for _ in 0..=key.len() {
                assert_eq!(actual.leave(source(808)), expected.leave(source(808)));
                assert_eq!(
                    actual.value(true, source(909)),
                    expected.value(true, source(909))
                );
                assert_eq!(
                    actual
                        .freeze_completed_obligation()
                        .unwrap()
                        .map(|p| p.obligation),
                    expected
                        .freeze_completed_obligation()
                        .unwrap()
                        .map(|p| p.obligation)
                );
            }
            assert_eq!(actual.finish(source(911)), expected.finish(source(911)));
            assert!(actual.finish(source(911)).is_ok());
            assert_eq!(actual.leave(source(912)), expected.leave(source(912)));
            assert_eq!(
                actual.edge(LocatedPathByte::new(0, 913)),
                expected.edge(LocatedPathByte::new(0, 913))
            );
            drop(actual);
            drop(expected);
            for (payload, bytes) in frozen {
                assert_eq!(payload.payload.located_bytes().collect::<Vec<_>>(), bytes);
            }
        }
    }
}

#[test]
fn topology_restore_runs_preserve_page_crossing_prefixes_and_divergent_continuations() {
    for depth in [0, 3] {
        for width in [2usize, 33, 129, 1_025] {
            let mut key = vec![EPATHMAP_MAP];
            prost::encoding::encode_varint(width as u64, &mut key);
            key.resize(key.len() + width, 0xa7);
            for _ in 0..depth {
                let mut body = Vec::new();
                prost::encoding::encode_varint(key.len() as u64, &mut body);
                body.extend_from_slice(&key);
                key = vec![EPATHMAP];
                prost::encoding::encode_varint(body.len() as u64, &mut key);
                key.extend_from_slice(&body);
            }
            let mut actual = CanonicalPathTopologyAdmission::new();
            let mut expected = EagerCanonicalPathTopologyAdmission::new();
            let mut undo = Vec::new();
            let mut frozen = Vec::new();
            for (offset, byte) in key.iter().copied().enumerate() {
                if [3, 31, 32, 63].contains(&offset) {
                    let snapshot = actual.arena.freeze(actual.current.bytes);
                    let contents = snapshot.located_bytes().collect::<Vec<_>>();
                    frozen.push((snapshot, contents));
                }
                undo.push(actual.current);
                let located =
                    LocatedPathByte::in_source(byte, offset as u32 % 7, 103 + offset * 19);
                assert_eq!(actual.edge(located), expected.edge(located));
                let runs = 1 + undo
                    .windows(2)
                    .filter(|pair| pair[0].state != pair[1].state)
                    .count();
                assert_eq!(actual.restore.len(), runs);
                assert_focused_persistent_matches(
                    &expected.reducer,
                    &expected.arena,
                    expected.current,
                    &actual.reducer,
                    &actual.arena,
                    actual.current,
                );
            }
            let pending = actual.active_obligation().unwrap();
            assert!(pending.awaiting_resolution);
            let before = (actual.stats(), actual.current, actual.restore.clone());
            let byte = LocatedPathByte::in_source(0xee, 97, 301);
            assert_eq!(actual.edge(byte), expected.edge(byte));
            assert_eq!(
                (actual.stats(), actual.current, actual.restore.clone()),
                before
            );
            let resolution = CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: true,
            };
            let wrong = CanonicalPathObligationId(pending.obligation.id.0 + 1);
            assert_eq!(
                actual.resolve_obligation(wrong, resolution),
                expected.resolve_obligation(wrong, resolution)
            );
            assert_eq!(
                (actual.stats(), actual.current, actual.restore.clone()),
                before
            );
            while let Some(saved) = undo.pop() {
                let stores = (saved.bytes.len() % 32 == 0).then(|| {
                    (
                        actual.reducer.states.clone(),
                        actual.reducer.machine.frames.slots.clone(),
                        actual.reducer.owners.entries.clone(),
                    )
                });
                let allocation = (
                    actual.stats(),
                    actual.arena.stats(),
                    actual.reducer.machine.next_obligation,
                );
                assert_eq!(actual.leave(source(901)), expected.leave(source(901)));
                assert_eq!(
                    (
                        actual.stats(),
                        actual.arena.stats(),
                        actual.reducer.machine.next_obligation,
                    ),
                    allocation,
                );
                if let Some((states, frames, owners)) = stores {
                    assert_eq!(actual.reducer.states, states);
                    assert_eq!(actual.reducer.machine.frames.slots, frames);
                    assert_eq!(actual.reducer.owners.entries, owners);
                }
                assert_eq!(actual.current.state, saved.state);
                assert_eq!(actual.current.bytes.len(), saved.bytes.len());
                assert_eq!(
                    actual
                        .arena
                        .located_bytes(actual.current.bytes)
                        .collect::<Vec<_>>(),
                    actual.arena.located_bytes(saved.bytes).collect::<Vec<_>>(),
                );
                let runs = if undo.is_empty() {
                    0
                } else {
                    1 + undo
                        .windows(2)
                        .filter(|pair| pair[0].state != pair[1].state)
                        .count()
                };
                assert_eq!(actual.restore.len(), runs);
                assert_focused_persistent_matches(
                    &expected.reducer,
                    &expected.arena,
                    expected.current,
                    &actual.reducer,
                    &actual.arena,
                    actual.current,
                );
                if actual
                    .active_obligation()
                    .is_some_and(|progress| progress.remaining > 1)
                {
                    for byte in [0x19, 0xe3] {
                        let located = LocatedPathByte::in_source(byte, 101, 307);
                        assert_eq!(actual.edge(located), expected.edge(located));
                        assert_focused_persistent_matches(
                            &expected.reducer,
                            &expected.arena,
                            expected.current,
                            &actual.reducer,
                            &actual.arena,
                            actual.current,
                        );
                        assert_eq!(actual.leave(source(903)), expected.leave(source(903)));
                        assert_eq!(actual.restore.len(), runs);
                    }
                }
            }
            assert_eq!(actual.leave(source(905)), expected.leave(source(905)));
            assert_eq!(actual.finish(source(907)), expected.finish(source(907)));
            assert_eq!(actual.leave(source(909)), expected.leave(source(909)));
            assert_eq!(actual.edge(byte), expected.edge(byte));
            drop(actual);
            drop(expected);
            for (snapshot, contents) in frozen {
                assert_eq!(snapshot.located_bytes().collect::<Vec<_>>(), contents);
            }
        }
    }
}

#[test]
fn topology_restore_runs_preserve_private_effects_faults_and_fresh_births() {
    for action in 0..3 {
        let mut body = Vec::new();
        for width in 129..=130 {
            let mut key = vec![EPATHMAP_MAP];
            prost::encoding::encode_varint((width + usize::from(action == 2)) as u64, &mut key);
            key.resize(key.len() + width, 0xa7);
            prost::encoding::encode_varint(key.len() as u64, &mut body);
            body.extend(key);
            if action == 2 {
                break;
            }
        }
        let mut key = vec![EPATHMAP];
        prost::encoding::encode_varint(body.len() as u64, &mut key);
        key.extend(body);
        if action == 2 {
            key.extend_from_slice(&[0x19, 0xe3, 0xff]);
        }
        let mut actual = CanonicalPathTopologyAdmission::new();
        let mut expected = EagerCanonicalPathTopologyAdmission::new();
        for round in 0..2 {
            let floor = actual.reducer.machine.next_obligation;
            let mut births = Vec::new();
            let mut completed = 0;
            for (offset, byte) in key.iter().copied().enumerate() {
                let located = LocatedPathByte::in_source(byte, 71 + round, 503 + offset * 11);
                let transition = actual.edge(located);
                assert_eq!(transition, expected.edge(located));
                if let Ok(CanonicalPathTransition::ObligationStarted(obligation)) = transition {
                    assert!(obligation.id.0 >= floor);
                    assert!(!births.contains(&obligation.id));
                    births.push(obligation.id);
                }
                if let Some(payload) = actual.freeze_completed_obligation().unwrap() {
                    let reference = expected.freeze_completed_obligation().unwrap().unwrap();
                    assert_eq!(payload.obligation, reference.obligation);
                    assert_eq!(payload.payload_end, reference.payload_end);
                    assert_eq!(
                        payload.payload.located_bytes().collect::<Vec<_>>(),
                        reference.payload.located_bytes().collect::<Vec<_>>()
                    );
                    let undo = actual.restore.clone();
                    let before = actual.current;
                    if action == 1 {
                        assert_eq!(
                            actual.reject_obligation(payload.obligation.id, source(709)),
                            expected.reject_obligation(payload.obligation.id, source(709))
                        );
                    } else {
                        let resolution = CanonicalPathObligationResolution::NestedEpm1 {
                            mode: NestedEpmMode::Map,
                            entries_stable: true,
                        };
                        assert_eq!(
                            actual.resolve_obligation(payload.obligation.id, resolution),
                            expected.resolve_obligation(payload.obligation.id, resolution)
                        );
                    }
                    assert_eq!(actual.restore, undo);
                    assert_eq!(actual.current.bytes, before.bytes);
                    assert_ne!(actual.current.state, before.state);
                    completed += 1;
                }
                assert_focused_persistent_matches(
                    &expected.reducer,
                    &expected.arena,
                    expected.current,
                    &actual.reducer,
                    &actual.arena,
                    actual.current,
                );
            }
            assert_eq!(births.len(), if action == 0 { 2 } else { 1 });
            assert_eq!(completed, match action {
                0 => 2,
                1 => 1,
                _ => 0,
            });
            let clock = actual.reducer.machine.next_obligation;
            assert!(clock > floor);
            for _ in 0..key.len() {
                assert_eq!(actual.leave(source(711)), expected.leave(source(711)));
                assert_eq!(actual.reducer.machine.next_obligation, clock);
                assert_focused_persistent_matches(
                    &expected.reducer,
                    &expected.arena,
                    expected.current,
                    &actual.reducer,
                    &actual.arena,
                    actual.current,
                );
            }
            assert_eq!(actual.current.bytes.len(), 0);
            assert!(actual.restore.is_empty());
        }
        assert_eq!(actual.leave(source(713)), expected.leave(source(713)));
        assert_eq!(actual.finish(source(715)), expected.finish(source(715)));
    }
}
