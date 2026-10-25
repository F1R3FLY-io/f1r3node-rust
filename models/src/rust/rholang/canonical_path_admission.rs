//! Persistent, byte-at-a-time admission for canonical PathMap keys.
//!
//! The compact PathMap reader walks a trie, not a collection of materialized
//! endpoint keys.  This transducer mirrors that walk: `edge` advances one
//! immutable parser state, `leave` restores the parent handle without copying
//! its continuation stack, and `value` asks whether the current prefix is one
//! complete canonical path.  Invalid prefixes are retained as dead states so
//! value-free navigation topology remains admissible.
//!
//! The two byte languages owned elsewhere are explicit suspension points.
//! Escape payloads (`0x0f`) and value-bearing EPM1 snapshots (`0x10`) are lent
//! byte-by-byte to their authoritative validators and must be resolved before
//! the enclosing segment can complete.

use crate::rust::canonical_path::CanonicalTopLevelPathForm;
#[cfg(test)]
use crate::rust::rholang::canonical_bytes_arena::FrozenCanonicalBytes;
use crate::rust::rholang::canonical_bytes_arena::{
    CanonicalByteProvenance, CanonicalBytesArena, CanonicalBytesArenaError, CanonicalBytesHandle,
    CanonicalLocatedByte, CanonicalSourceSlot,
};
use crate::rust::rholang::canonical_source_tree::SourceTreeWindow;
use crate::rust::rholang::indexed_protobuf::IndexedByteRegion;
use crate::rust::rholang::protobuf_cursor::CanonicalByteRange;

const TERM: u8 = 0x00;
const GBOOL_FALSE: u8 = 0x01;
const GBOOL_TRUE: u8 = 0x02;
const GINT: u8 = 0x03;
const GSTRING: u8 = 0x04;
const GURI: u8 = 0x05;
const GBYTE_ARRAY: u8 = 0x06;
const GDOUBLE: u8 = 0x07;
const GBIG_INT: u8 = 0x08;
const GBIG_RAT: u8 = 0x09;
const GFIXED_POINT: u8 = 0x0a;
const ELIST: u8 = 0x0b;
const ETUPLE: u8 = 0x0c;
const EPATHMAP: u8 = 0x0d;
const GPRIVATE: u8 = 0x0e;
const ESCAPE: u8 = 0x0f;
const EPATHMAP_MAP: u8 = 0x10;

/// Physical provenance of one logical trie edge or endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathSource {
    pub(crate) source: u32,
    pub(crate) offset: usize,
}

impl CanonicalPathSource {
    pub(crate) const fn new(offset: usize) -> Self { Self::in_source(0, offset) }

    pub(crate) const fn in_source(source: u32, offset: usize) -> Self { Self { source, offset } }

    const fn from_provenance(provenance: CanonicalByteProvenance) -> Self {
        Self::in_source(provenance.source, provenance.offset)
    }
}

/// One byte and the physical snapshot location that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LocatedPathByte {
    pub(crate) byte: u8,
    pub(crate) source: CanonicalPathSource,
}

impl LocatedPathByte {
    #[cfg(test)]
    pub(crate) const fn new(byte: u8, offset: usize) -> Self { Self::in_source(byte, 0, offset) }

    pub(crate) const fn in_source(byte: u8, source: u32, offset: usize) -> Self {
        Self {
            byte,
            source: CanonicalPathSource::in_source(source, offset),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalPathAdmissionErrorKind {
    EmptyPath,
    Truncated,
    NonMinimalVarint,
    VarintOverflow,
    ReservedTag(u8),
    TerminatorMisplaced,
    BareTopLevelList,
    UnterminatedMultiSegment,
    InvalidUtf8,
    ScaleOutOfRange,
    EscapeNested,
    EmptyPathFrame,
    RegionFrameOverrun,
    DuplicatePathInRegion,
    NonAscendingRegion,
    EscapeOfStablePar,
    NestedEpmNotMap,
    NestedEpmNotStable,
    RejectedObligation,
    UnresolvedObligation,
    TopologyUnderflow,
    TopologyAfterRootLeave,
    TopologyIncomplete,
    SizeOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathAdmissionError {
    pub(crate) kind: CanonicalPathAdmissionErrorKind,
    pub(crate) source: CanonicalPathSource,
}

impl CanonicalPathAdmissionError {
    const fn new(kind: CanonicalPathAdmissionErrorKind, source: CanonicalPathSource) -> Self {
        Self { kind, source }
    }
}

impl std::fmt::Display for CanonicalPathAdmissionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{:?} at snapshot source {} byte {}",
            self.kind, self.source.source, self.source.offset
        )
    }
}

impl std::error::Error for CanonicalPathAdmissionError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct CanonicalPathObligationId(u64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalPathObligationKind {
    RawParEscape,
    NestedEpm1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathObligation {
    pub(crate) id: CanonicalPathObligationId,
    pub(crate) kind: CanonicalPathObligationKind,
    pub(crate) payload_len: u64,
    pub(crate) tag_source: CanonicalPathSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathObligationProgress {
    pub(crate) obligation: CanonicalPathObligation,
    pub(crate) remaining: u64,
    pub(crate) awaiting_resolution: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathCompletedObligation<Span = CanonicalBytesHandle> {
    pub(crate) obligation: CanonicalPathObligation,
    pub(crate) payload: Span,
    /// Exact physical anchor at which the opaque payload became complete.
    ///
    /// Provenance can be discontinuous, so consumers must not synthesize this
    /// by incrementing the final payload byte's offset.
    pub(crate) payload_end: CanonicalPathSource,
}

/// Ownership-independent completed payload for a suspended nested validator.
#[cfg(test)]
#[derive(Clone, Debug)]
pub(crate) struct FrozenCanonicalPathObligation {
    pub(crate) obligation: CanonicalPathObligation,
    pub(crate) payload: FrozenCanonicalBytes,
    pub(crate) payload_end: CanonicalPathSource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NestedEpmMode {
    Empty,
    Set,
    Map,
}

/// Successful evidence returned by the authoritative nested validator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalPathObligationResolution {
    RawPar {
        canonical_path_stable: bool,
    },
    NestedEpm1 {
        mode: NestedEpmMode,
        entries_stable: bool,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CanonicalPathTransition {
    Continue,
    ObligationStarted(CanonicalPathObligation),
    ObligationByte {
        id: CanonicalPathObligationId,
        byte: LocatedPathByte,
        last: bool,
    },
    Restored {
        active_obligation: Option<CanonicalPathObligationProgress>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathSummary {
    pub(crate) form: CanonicalTopLevelPathForm,
    pub(crate) canonical_path_stable: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalPathAdmissionStats {
    pub(crate) edges: usize,
    pub(crate) states: usize,
    pub(crate) frames: usize,
    pub(crate) max_topology_depth: usize,
    #[cfg(test)]
    pub(crate) bulk_operations: usize,
    #[cfg(test)]
    pub(crate) bulk_bytes: usize,
    #[cfg(test)]
    pub(crate) region_descents: usize,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct CanonicalPathOwnerWork {
    pub(crate) active_queries: usize,
    pub(crate) completed_queries: usize,
    pub(crate) plans: usize,
    pub(crate) resolutions: usize,
    pub(crate) rebuilds: usize,
    pub(crate) detachments: usize,
    pub(crate) reifications: usize,
    pub(crate) settlements: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PathStateId(usize);

impl PathStateId {
    fn new(slot: usize, linked: bool) -> Self {
        Self(
            slot.checked_mul(2)
                .and_then(|value| value.checked_add(usize::from(linked)))
                .expect("a nonzero-sized checkpoint Vec bounds its tagged slot"),
        )
    }

    const fn slot(self) -> usize { self.0 / 2 }

    const fn is_linked(self) -> bool { self.0 & 1 != 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameId(usize);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct VarintState {
    value: u64,
    shift: u32,
    bytes: u8,
}

impl VarintState {
    fn feed(self, byte: LocatedPathByte) -> Result<VarintFeed, CanonicalPathAdmissionError> {
        let group = byte.byte & 0x7f;
        if self.shift > 63 || self.shift == 63 && group > 1 {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::VarintOverflow,
                byte.source,
            ));
        }
        let value = self.value | u64::from(group) << self.shift;
        let bytes = self.bytes + 1;
        if byte.byte & 0x80 != 0 {
            return Ok(VarintFeed::More(Self {
                value,
                shift: self.shift + 7,
                bytes,
            }));
        }
        if byte.byte == 0 && bytes > 1 {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::NonMinimalVarint,
                byte.source,
            ));
        }
        Ok(VarintFeed::Complete(value))
    }
}

enum VarintFeed {
    More(VarintState),
    Complete(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Utf8State {
    remaining: u8,
    next_min: u8,
    next_max: u8,
}

impl Utf8State {
    fn feed(self, byte: u8) -> Option<Self> {
        if self.remaining != 0 {
            if !(self.next_min..=self.next_max).contains(&byte) {
                return None;
            }
            return Some(Self {
                remaining: self.remaining - 1,
                next_min: 0x80,
                next_max: 0xbf,
            });
        }
        Some(match byte {
            0x00..=0x7f => Self::default(),
            0xc2..=0xdf => Self {
                remaining: 1,
                next_min: 0x80,
                next_max: 0xbf,
            },
            0xe0 => Self {
                remaining: 2,
                next_min: 0xa0,
                next_max: 0xbf,
            },
            0xe1..=0xec | 0xee..=0xef => Self {
                remaining: 2,
                next_min: 0x80,
                next_max: 0xbf,
            },
            0xed => Self {
                remaining: 2,
                next_min: 0x80,
                next_max: 0x9f,
            },
            0xf0 => Self {
                remaining: 3,
                next_min: 0x90,
                next_max: 0xbf,
            },
            0xf1..=0xf3 => Self {
                remaining: 3,
                next_min: 0x80,
                next_max: 0xbf,
            },
            0xf4 => Self {
                remaining: 3,
                next_min: 0x80,
                next_max: 0x8f,
            },
            _ => return None,
        })
    }

    const fn complete(self) -> bool { self.remaining == 0 }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VarintRole {
    Scalar,
    Scale,
    Bytes {
        utf8: bool,
    },
    Count,
    RegionLength,
    ObligationLength {
        kind: CanonicalPathObligationKind,
        tag_source: CanonicalPathSource,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum RegionPhase {
    PathLength(VarintState),
    Path {
        declared_len: u64,
        remaining: u64,
        start_depth: usize,
        path: InnerPath,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObligationStatus {
    Payload,
    #[cfg(test)]
    Countdown {
        remaining: u64,
    },
    AwaitingResolution {
        payload_end: CanonicalPathSource,
    },
}

impl ObligationStatus {
    fn remaining(self, declared: u64, start: usize, stamp: usize) -> Option<u64> {
        match self {
            Self::Payload => declared.checked_sub(u64::try_from(stamp.checked_sub(start)?).ok()?),
            #[cfg(test)]
            Self::Countdown { remaining } => Some(remaining),
            Self::AwaitingResolution { .. } => Some(0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Frame<Span = CanonicalBytesHandle> {
    Tag {
        escape_allowed: bool,
        next: Option<FrameId>,
    },
    Varint {
        role: VarintRole,
        state: VarintState,
        next: Option<FrameId>,
    },
    Bytes {
        remaining: u64,
        utf8: Option<Utf8State>,
        next: Option<FrameId>,
    },
    Kids {
        remaining: u64,
        next: Option<FrameId>,
    },
    Region {
        remaining: u64,
        previous: Option<Span>,
        phase: RegionPhase,
        next: Option<FrameId>,
    },
    Obligation {
        obligation: CanonicalPathObligation,
        status: ObligationStatus,
        payload_start: usize,
        next: Option<FrameId>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PathStatus {
    Open,
    SplitComplete,
    Dead(CanonicalPathAdmissionError),
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InnerPath {
    escape_allowed: bool,
    stack: Option<FrameId>,
    status: PathStatus,
    completed_segments: u64,
    first_tag: Option<(u8, CanonicalPathSource)>,
    stable: bool,
}

impl InnerPath {
    const fn new(escape_allowed: bool) -> Self {
        Self {
            escape_allowed,
            stack: None,
            status: PathStatus::Open,
            completed_segments: 0,
            first_tag: None,
            stable: true,
        }
    }

    fn kill(&mut self, error: CanonicalPathAdmissionError) { self.status = PathStatus::Dead(error) }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CanonicalPathReducerHandle {
    state: PathStateId,
    bytes: CanonicalBytesHandle,
}

impl CanonicalPathReducerHandle {
    #[cfg(test)]
    pub(crate) const fn bytes(self) -> CanonicalBytesHandle { self.bytes }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistentFocusedPath {
    current: InnerPath,
    owners: Option<PersistentRegionOwnerId>,
    stamp: usize,
    fault: Option<CanonicalPathAdmissionError>,
}

#[cfg_attr(test, derive(Clone, Debug, PartialEq, Eq))]
struct PersistentCheckpoints {
    direct: Vec<InnerPath>,
    linked: Vec<PersistentFocusedPath>,
}

impl PersistentCheckpoints {
    fn new() -> Self {
        const {
            assert!(std::mem::size_of::<InnerPath>() >= 2);
        }
        const {
            assert!(std::mem::size_of::<PersistentFocusedPath>() >= 2);
        }
        Self {
            direct: vec![InnerPath::new(true)],
            linked: Vec::new(),
        }
    }

    #[cfg(test)]
    const fn len(&self) -> usize { self.direct.len() + self.linked.len() }

    fn get(&self, id: PathStateId, stamp: usize) -> PersistentFocusedPath {
        if id.is_linked() {
            let mut state = self.linked[id.slot()].clone();
            if state.fault.is_none() && !matches!(state.current.status, PathStatus::Dead(_)) {
                state.stamp = stamp;
            }
            state
        } else {
            PersistentFocusedPath {
                current: self.direct[id.slot()].clone(),
                owners: None,
                stamp,
                fault: None,
            }
        }
    }

    fn push(&mut self, state: PersistentFocusedPath) -> PathStateId {
        if state.owners.is_some()
            || state.fault.is_some()
            || matches!(state.current.status, PathStatus::Dead(_))
        {
            let id = PathStateId::new(self.linked.len(), true);
            self.linked.push(state);
            id
        } else {
            let id = PathStateId::new(self.direct.len(), false);
            self.direct.push(state.current);
            id
        }
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        self.direct.capacity() * std::mem::size_of::<InnerPath>()
            + self.linked.capacity() * std::mem::size_of::<PersistentFocusedPath>()
    }
}

pub(crate) struct CanonicalPathReducer {
    states: PersistentCheckpoints,
    owners: PersistentRegionOwners,
    machine: CanonicalPathMachine<FrameHistory>,
}

impl CanonicalPathReducer {
    pub(crate) fn new() -> Self {
        Self {
            states: PersistentCheckpoints::new(),
            owners: PersistentRegionOwners::new(),
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
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats {
        CanonicalPathAdmissionStats {
            states: self.states.len(),
            frames: self.machine.frames.slots.len(),
            ..self.machine.stats
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_state_bytes(&self) -> usize {
        self.states.retained_bytes()
            + self.owners.entries.capacity() * std::mem::size_of::<PersistentRegionOwner>()
    }

    fn state(&self, handle: CanonicalPathReducerHandle) -> PersistentFocusedPath {
        self.states.get(handle.state, handle.bytes.len())
    }

    #[cfg(test)]
    pub(crate) fn retained_frame_bytes(&self) -> usize { self.machine.frames.retained_bytes() }

    #[cfg(test)]
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

    fn root_path<'a>(&'a self, state: &'a PersistentFocusedPath) -> &'a InnerPath {
        match state.owners {
            Some(root) => &self.owners.entries[self.owners.entries[root.0].outermost.0].owner,
            None => &state.current,
        }
    }

    fn dead_error(&self, state: &PersistentFocusedPath) -> Option<CanonicalPathAdmissionError> {
        state.fault.or_else(|| match self.root_path(state).status {
            PathStatus::Dead(error) => Some(error),
            _ => None,
        })
    }

    fn focus(&mut self, state: &mut PersistentFocusedPath) {
        while state.fault.is_none() && state.current.status == PathStatus::Open {
            let Some((owner, child)) = self.owners.capture(
                &self.machine.frames,
                &state.current,
                state.owners,
                state.stamp,
            ) else {
                break;
            };
            state.owners = Some(owner);
            state.current = child;
            #[cfg(test)]
            self.machine.record_owner_work(|work| work.detachments += 1);
        }
    }

    pub(crate) fn active_obligation(
        &self,
        handle: CanonicalPathReducerHandle,
    ) -> Option<CanonicalPathObligationProgress> {
        let state = self.state(handle);
        if self.dead_error(&state).is_some() {
            return None;
        }
        self.machine
            .active_obligation_in(&state.current, state.stamp)
    }

    pub(crate) fn completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
    ) -> Result<Option<CanonicalPathCompletedObligation>, CanonicalPathAdmissionError> {
        let state = self.state(handle);
        if self.dead_error(&state).is_some() {
            return Ok(None);
        }
        self.machine.completed_obligation(
            &ArenaPathBytes {
                arena,
                bytes: handle.bytes,
            },
            &state.current,
        )
    }

    pub(crate) fn value(
        &self,
        handle: CanonicalPathReducerHandle,
        has_value: bool,
        source: CanonicalPathSource,
    ) -> Option<Result<CanonicalPathSummary, CanonicalPathAdmissionError>> {
        has_value.then(|| {
            let state = self.state(handle);
            if let Some(error) = self.dead_error(&state) {
                return Err(error);
            }
            self.machine.finish_path_with_pending(
                self.root_path(&state),
                source,
                self.active_obligation(handle)
                    .is_some_and(|value| value.awaiting_resolution),
            )
        })
    }

    fn settle(
        &mut self,
        bytes: &ArenaPathBytes<'_>,
        state: &mut PersistentFocusedPath,
        endpoint: CanonicalPathSource,
    ) {
        loop {
            if matches!(state.current.status, PathStatus::Dead(_))
                || state.current.stack.is_some_and(|top| {
                    matches!(self.machine.frames.slots[top.0], Frame::Obligation {
                        status: ObligationStatus::AwaitingResolution { .. },
                        ..
                    })
                })
            {
                return;
            }
            let Some(root) = state.owners else { return };
            #[cfg(test)]
            self.machine.record_owner_work(|work| work.settlements += 1);
            let saved = &self.owners.entries[root.0];
            if saved.path_boundary.end != state.stamp as u128 {
                return;
            }
            if saved.path_boundary.owner != root {
                state.current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::Truncated,
                    endpoint,
                ));
                return;
            }
            let mut owner = saved.owner.clone();
            let remaining = u64::try_from(
                saved
                    .region_end
                    .checked_sub(state.stamp as u128)
                    .expect("completion stays inside the selected Region"),
            )
            .expect("selected residual originated as u64");
            let Frame::Region {
                previous,
                phase:
                    RegionPhase::Path {
                        declared_len,
                        start_depth,
                        ..
                    },
                next,
                ..
            } = &self.machine.frames.slots[owner.stack.expect("a held owner retains its frame").0]
            else {
                unreachable!("held immutable Region phase")
            };
            let (previous, declared_len, start_depth, next) =
                (*previous, *declared_len, *start_depth, *next);
            let parent = saved.parent;
            self.machine.finish_region_path(
                bytes,
                &mut owner,
                &state.current,
                remaining,
                declared_len,
                start_depth,
                previous,
                next,
                endpoint,
            );
            state.current = owner;
            state.owners = parent;
        }
    }

    fn select(
        &mut self,
        before: &PersistentFocusedPath,
        mut candidate: PersistentFocusedPath,
    ) -> PersistentFocusedPath {
        if let PathStatus::Dead(error) = candidate.current.status {
            if before.owners.is_some() {
                let mut selected = before.clone();
                selected.fault = Some(error);
                return selected;
            }
        }
        self.focus(&mut candidate);
        candidate
    }

    fn reusable_payload(
        &self,
        state: &PersistentFocusedPath,
        next_stamp: usize,
    ) -> Option<CanonicalPathObligationId> {
        if state.stamp.checked_add(1)? != next_stamp {
            return None;
        }
        self.reusable_payload_prefix(state, 1).map(|(id, _)| id)
    }

    fn reusable_payload_prefix(
        &self,
        state: &PersistentFocusedPath,
        available: usize,
    ) -> Option<(CanonicalPathObligationId, usize)> {
        if state.fault.is_some() || state.current.status != PathStatus::Open {
            return None;
        }
        let Frame::Obligation {
            obligation,
            status: ObligationStatus::Payload,
            payload_start,
            ..
        } = &self.machine.frames.slots[state.current.stack?.0]
        else {
            return None;
        };
        let remaining = ObligationStatus::Payload.remaining(
            obligation.payload_len,
            *payload_start,
            state.stamp,
        )?;
        let mut width = available
            .min(usize::try_from(remaining.checked_sub(1)?).unwrap_or(usize::MAX))
            .min(usize::MAX - state.stamp);
        if let Some(root) = state.owners {
            let distance = self.owners.entries[root.0]
                .all_boundary
                .end
                .checked_sub(state.stamp as u128)?
                .checked_sub(1)?;
            width = width.min(usize::try_from(distance).unwrap_or(usize::MAX));
        }
        (width != 0).then_some((obligation.id, width))
    }

    pub(crate) fn edge(
        &mut self,
        arena: &mut CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        located: CanonicalLocatedByte,
    ) -> Result<(CanonicalPathReducerHandle, CanonicalPathTransition), CanonicalPathAdmissionError>
    {
        let before = self.state(handle);
        let dead = self.dead_error(&before).is_some();
        let byte = if dead {
            LocatedPathByte {
                byte: located.byte,
                source: CanonicalPathSource::from_provenance(located.provenance),
            }
        } else {
            self.machine.prepare_edge(&before.current, located)?
        };
        let bytes = arena.append(handle.bytes, located).map_err(|_| {
            CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::SizeOverflow,
                byte.source,
            )
        })?;
        self.machine.stats.edges += 1;
        self.machine.stats.max_topology_depth =
            self.machine.stats.max_topology_depth.max(bytes.len());
        if let Some(id) = self.reusable_payload(&before, bytes.len()) {
            return Ok((
                CanonicalPathReducerHandle {
                    state: handle.state,
                    bytes,
                },
                CanonicalPathTransition::ObligationByte {
                    id,
                    byte,
                    last: false,
                },
            ));
        }
        let (state, event) = if dead {
            (before, CanonicalPathTransition::Continue)
        } else {
            let mut candidate = before.clone();
            candidate.stamp = bytes.len();
            let event = if before.owners.is_some_and(|root| {
                self.owners.entries[root.0].all_boundary.end < candidate.stamp as u128
            }) {
                candidate.current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                    byte.source,
                ));
                CanonicalPathTransition::Continue
            } else {
                self.machine
                    .feed_path_shallow(bytes.len(), &mut candidate.current, byte)
            };
            self.settle(
                &ArenaPathBytes { arena, bytes },
                &mut candidate,
                byte.source,
            );
            (self.select(&before, candidate), event)
        };
        let id = self.states.push(state);
        Ok((CanonicalPathReducerHandle { state: id, bytes }, event))
    }

    fn private_preflight(
        &self,
        state: &PersistentFocusedPath,
        id: CanonicalPathObligationId,
        awaiting: bool,
        fallback: CanonicalPathSource,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let mut current = &state.current;
        loop {
            let Some(top) = current.stack else {
                return Err(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    fallback,
                ));
            };
            match &self.machine.frames.slots[top.0] {
                Frame::Region {
                    phase: RegionPhase::Path { path, .. },
                    ..
                } => current = path,
                Frame::Obligation {
                    obligation, status, ..
                } => {
                    return if obligation.id == id
                        && (!awaiting
                            || matches!(status, ObligationStatus::AwaitingResolution { .. }))
                    {
                        Ok(())
                    } else {
                        Err(CanonicalPathAdmissionError::new(
                            CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                            obligation.tag_source,
                        ))
                    };
                }
                _ => {
                    return Err(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                        fallback,
                    ));
                }
            }
        }
    }

    fn materialize(&mut self, state: &PersistentFocusedPath) -> InnerPath {
        #[cfg(test)]
        let before = self.machine.frames.slots.len();
        let mut root = self.owners.materialize(
            &mut self.machine.frames,
            state.current.clone(),
            state.owners,
            state.stamp,
        );
        if let Some(error) = state.fault {
            root.kill(error);
        }
        #[cfg(test)]
        self.machine.record_owner_work(|work| {
            work.reifications += self.machine.frames.slots.len() - before
        });
        root
    }

    pub(crate) fn resolve_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<CanonicalPathReducerHandle, CanonicalPathAdmissionError> {
        let before = self.state(handle);
        self.private_preflight(&before, id, true, CanonicalPathSource::new(0))?;
        let bytes = ArenaPathBytes {
            arena,
            bytes: handle.bytes,
        };
        let state = if self.dead_error(&before).is_some() {
            let root = self.materialize(&before);
            let current = self.machine.resolved_state(&bytes, &root, id, resolution)?;
            let mut state = PersistentFocusedPath {
                current,
                owners: None,
                stamp: before.stamp,
                fault: None,
            };
            self.focus(&mut state);
            state
        } else {
            let mut candidate = before.clone();
            let frame = self.machine.frame(
                candidate
                    .current
                    .stack
                    .expect("preflight selected an obligation"),
            );
            self.machine
                .resolve_leaf_obligation(&mut candidate.current, frame, id, resolution)?;
            let endpoint = CanonicalPathSource::from_provenance(
                bytes
                    .provenance_at(handle.bytes.len() - 1)
                    .expect("a pending child has consumed its length"),
            );
            self.settle(&bytes, &mut candidate, endpoint);
            self.select(&before, candidate)
        };
        let next = self.states.push(state);
        Ok(CanonicalPathReducerHandle {
            state: next,
            bytes: handle.bytes,
        })
    }

    #[cfg(test)]
    pub(crate) fn reject_obligation(
        &mut self,
        _arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        id: CanonicalPathObligationId,
        source: CanonicalPathSource,
    ) -> Result<CanonicalPathReducerHandle, CanonicalPathAdmissionError> {
        let mut state = self.state(handle);
        self.private_preflight(&state, id, false, source)?;
        state.fault = Some(CanonicalPathAdmissionError::new(
            CanonicalPathAdmissionErrorKind::RejectedObligation,
            source,
        ));
        let next = self.states.push(state);
        Ok(CanonicalPathReducerHandle {
            state: next,
            bytes: handle.bytes,
        })
    }
}

pub(crate) struct CanonicalPathStreamReducer {
    current: InnerPath,
    consumed: usize,
    graph_stamp: usize,
    owners: Vec<FocusedRegionOwner>,
    machine: CanonicalPathMachine<FrameStorage<CanonicalByteRange>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegionBoundary<Owner = usize> {
    end: u128,
    owner: Owner,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FocusedRegionOwner {
    owner: InnerPath,
    region_end: u128,
    path_end: u128,
    all_boundary: RegionBoundary,
    path_boundary: RegionBoundary,
    all_open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpaqueInteriorPlan {
    pub(crate) from: usize,
    pub(crate) id: CanonicalPathObligationId,
    pub(crate) width: usize,
}

enum PathAdvance<'a, B> {
    Byte { bytes: &'a B, byte: LocatedPathByte },
    OpaqueInterior { width: u64 },
}

impl<B> PathAdvance<'_, B> {
    fn width(&self) -> u64 {
        match self {
            Self::Byte { .. } => 1,
            Self::OpaqueInterior { width } => *width,
        }
    }

    fn scalar(&self) -> (&B, LocatedPathByte) {
        match self {
            Self::Byte { bytes, byte } => (bytes, *byte),
            Self::OpaqueInterior { .. } => {
                unreachable!("a strict interior never reaches a boundary")
            }
        }
    }
}

trait PathByteView {
    type Span: Copy + std::fmt::Debug + Eq;

    fn consumed_len(&self) -> usize;
    fn slice(&self, range: std::ops::Range<usize>) -> Option<Self::Span>;
    fn span_len(&self, span: Self::Span) -> usize;
    fn compare(&self, left: Self::Span, right: Self::Span) -> std::cmp::Ordering;
    fn first_mismatch_source(
        &self,
        left: Self::Span,
        right: Self::Span,
    ) -> Option<CanonicalByteProvenance>;
    fn provenance_at(&self, index: usize) -> Option<CanonicalByteProvenance>;
}

struct ArenaPathBytes<'a> {
    arena: &'a CanonicalBytesArena<'a>,
    bytes: CanonicalBytesHandle,
}

impl PathByteView for ArenaPathBytes<'_> {
    type Span = CanonicalBytesHandle;

    fn consumed_len(&self) -> usize { self.bytes.len() }
    fn slice(&self, range: std::ops::Range<usize>) -> Option<Self::Span> {
        self.arena.slice(self.bytes, range).ok()
    }
    fn span_len(&self, span: Self::Span) -> usize { span.len() }
    fn compare(&self, left: Self::Span, right: Self::Span) -> std::cmp::Ordering {
        self.arena.compare(left, right)
    }
    fn first_mismatch_source(
        &self,
        left: Self::Span,
        right: Self::Span,
    ) -> Option<CanonicalByteProvenance> {
        self.arena
            .bytes(left)
            .zip(self.arena.located_bytes(right))
            .find_map(|(left, right)| (left != right.byte).then_some(right.provenance))
    }
    fn provenance_at(&self, index: usize) -> Option<CanonicalByteProvenance> {
        self.arena.provenance_at(self.bytes, index)
    }
}

struct IndexedPathBytes<'a, 'src> {
    input: &'a IndexedByteRegion<'src>,
    consumed: usize,
}

impl<'src> IndexedPathBytes<'_, 'src> {
    fn view(&self, span: CanonicalByteRange) -> IndexedByteRegion<'src> {
        self.input
            .subview(span)
            .expect("a checked path span stays inside its source")
    }
}

impl PathByteView for IndexedPathBytes<'_, '_> {
    type Span = CanonicalByteRange;

    fn consumed_len(&self) -> usize { self.consumed }
    fn slice(&self, range: std::ops::Range<usize>) -> Option<Self::Span> {
        (range.start <= range.end
            && range.end <= self.consumed
            && self.consumed <= self.input.len())
        .then_some(CanonicalByteRange {
            start: range.start,
            end: range.end,
        })
    }
    fn span_len(&self, span: Self::Span) -> usize { span.end - span.start }
    fn compare(&self, left: Self::Span, right: Self::Span) -> std::cmp::Ordering {
        self.view(left).compare(&self.view(right))
    }
    fn first_mismatch_source(
        &self,
        left: Self::Span,
        right: Self::Span,
    ) -> Option<CanonicalByteProvenance> {
        self.view(left)
            .bytes()
            .zip(self.view(right).located_bytes())
            .find_map(|(left, right)| (left != right.byte).then_some(right.provenance))
    }
    fn provenance_at(&self, index: usize) -> Option<CanonicalByteProvenance> {
        if self.consumed > self.input.len() || index >= self.consumed {
            return None;
        }
        self.input
            .position_at(index)
            .map(|position| position.physical)
    }
}

#[derive(Clone, Copy, Default)]
struct FrameCheckpoint {
    allocated: usize,
    retired: usize,
}

struct FrameStorage<Span = CanonicalBytesHandle> {
    slots: Vec<Frame<Span>>,
    free: Vec<FrameId>,
    allocated: Vec<FrameId>,
    retired: Vec<FrameId>,
}

impl<Span> FrameStorage<Span> {
    fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            allocated: Vec::new(),
            retired: Vec::new(),
        }
    }

    #[cfg(test)]
    const fn len(&self) -> usize { self.slots.len() }

    fn push(&mut self, frame: Frame<Span>) -> FrameId {
        let id = if let Some(id) = self.free.pop() {
            self.slots[id.0] = frame;
            id
        } else {
            let id = FrameId(self.slots.len());
            self.slots.push(frame);
            id
        };
        self.allocated.push(id);
        id
    }

    fn checkpoint(&self) -> FrameCheckpoint {
        FrameCheckpoint {
            allocated: self.allocated.len(),
            retired: self.retired.len(),
        }
    }

    fn retire(&mut self, id: FrameId) { self.retired.push(id); }

    fn select(
        &mut self,
        checkpoint: FrameCheckpoint,
        previous: Option<FrameId>,
        selected: Option<FrameId>,
    ) {
        if previous == selected {
            self.retired.truncate(checkpoint.retired);
            self.retired
                .extend_from_slice(&self.allocated[checkpoint.allocated..]);
        } else if let Some(previous) = previous {
            self.retire(previous);
        }
    }

    fn finish(&mut self, restore: bool) {
        if restore {
            self.retired.clear();
            self.retired.extend_from_slice(&self.allocated);
        }
        self.free.append(&mut self.retired);
        self.allocated.clear();
    }

    #[cfg(test)]
    fn retained_bytes(&self) -> usize {
        self.slots.capacity() * std::mem::size_of::<Frame<Span>>()
            + (self.free.capacity() + self.allocated.capacity() + self.retired.capacity())
                * std::mem::size_of::<FrameId>()
    }
}

impl<Span> std::ops::Index<usize> for FrameStorage<Span> {
    type Output = Frame<Span>;
    fn index(&self, index: usize) -> &Self::Output { &self.slots[index] }
}

trait FrameStore: std::ops::Index<usize, Output = Frame<Self::Span>> {
    type Span: Copy + std::fmt::Debug + Eq;

    #[cfg(test)]
    const EAGER_PAYLOADS: bool = false;

    fn new() -> Self;
    fn push(&mut self, frame: Frame<Self::Span>) -> FrameId;
    fn checkpoint(&self) -> FrameCheckpoint { FrameCheckpoint::default() }
    fn retire(&mut self, _id: FrameId) {}
    fn select(
        &mut self,
        _checkpoint: FrameCheckpoint,
        _previous: Option<FrameId>,
        _selected: Option<FrameId>,
    ) {
    }
    fn finish(&mut self, _restore: bool) {}
    #[cfg(test)]
    fn retained_bytes(&self) -> usize;
}

struct FrameHistory {
    slots: Vec<Frame>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PersistentRegionOwnerId(usize);

#[derive(Clone, Debug, PartialEq, Eq)]
struct PersistentRegionOwner {
    owner: InnerPath,
    parent: Option<PersistentRegionOwnerId>,
    region_end: u128,
    path_end: u128,
    all_boundary: RegionBoundary<PersistentRegionOwnerId>,
    path_boundary: RegionBoundary<PersistentRegionOwnerId>,
    outermost: PersistentRegionOwnerId,
}

struct PersistentRegionOwners {
    entries: Vec<PersistentRegionOwner>,
}

impl PersistentRegionOwners {
    fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    fn capture(
        &mut self,
        frames: &FrameHistory,
        current: &InnerPath,
        parent: Option<PersistentRegionOwnerId>,
        stamp: usize,
    ) -> Option<(PersistentRegionOwnerId, InnerPath)> {
        let Frame::Region {
            remaining,
            phase:
                RegionPhase::Path {
                    remaining: path_remaining,
                    path,
                    ..
                },
            ..
        } = &frames.slots[current.stack?.0]
        else {
            return None;
        };
        let id = PersistentRegionOwnerId(self.entries.len());
        let region_end = stamp as u128 + u128::from(*remaining);
        let path_end = stamp as u128 + u128::from(*path_remaining);
        let mut all_boundary = RegionBoundary {
            end: region_end.min(path_end),
            owner: id,
        };
        let mut path_boundary = RegionBoundary {
            end: path_end,
            owner: id,
        };
        let outermost = if let Some(parent) = parent {
            let previous = &self.entries[parent.0];
            if previous.all_boundary.end <= all_boundary.end {
                all_boundary = previous.all_boundary;
            }
            if previous.path_boundary.end < path_boundary.end {
                path_boundary = previous.path_boundary;
            }
            previous.outermost
        } else {
            id
        };
        self.entries.push(PersistentRegionOwner {
            owner: current.clone(),
            parent,
            region_end,
            path_end,
            all_boundary,
            path_boundary,
            outermost,
        });
        Some((id, path.clone()))
    }

    fn materialize(
        &self,
        frames: &mut FrameHistory,
        mut current: InnerPath,
        mut root: Option<PersistentRegionOwnerId>,
        stamp: usize,
    ) -> InnerPath {
        while let Some(id) = root {
            let saved = &self.entries[id.0];
            let held = saved
                .owner
                .stack
                .expect("a captured owner retains its Region frame");
            let Frame::Region {
                previous,
                phase:
                    RegionPhase::Path {
                        declared_len,
                        start_depth,
                        ..
                    },
                next,
                ..
            } = &frames.slots[held.0]
            else {
                unreachable!("immutable owner history retains its active Region phase")
            };
            let frame = Frame::Region {
                remaining: u64::try_from(
                    saved
                        .region_end
                        .checked_sub(stamp as u128)
                        .expect("the selected graph stamp precedes the Region end"),
                )
                .expect("the selected Region residual originated as u64"),
                previous: *previous,
                phase: RegionPhase::Path {
                    declared_len: *declared_len,
                    remaining: u64::try_from(
                        saved
                            .path_end
                            .checked_sub(stamp as u128)
                            .expect("the selected graph stamp precedes the path end"),
                    )
                    .expect("the selected path residual originated as u64"),
                    start_depth: *start_depth,
                    path: current,
                },
                next: *next,
            };
            current = saved.owner.clone();
            current.stack = Some(frames.push(frame));
            root = saved.parent;
        }
        current
    }
}

impl std::ops::Index<usize> for FrameHistory {
    type Output = Frame;
    fn index(&self, index: usize) -> &Self::Output { &self.slots[index] }
}

impl FrameStore for FrameHistory {
    type Span = CanonicalBytesHandle;

    fn new() -> Self { Self { slots: Vec::new() } }
    fn push(&mut self, frame: Frame) -> FrameId {
        let id = FrameId(self.slots.len());
        self.slots.push(frame);
        id
    }
    #[cfg(test)]
    fn retained_bytes(&self) -> usize { self.slots.capacity() * std::mem::size_of::<Frame>() }
}

impl<Span: Copy + std::fmt::Debug + Eq> FrameStore for FrameStorage<Span> {
    type Span = Span;

    fn new() -> Self { FrameStorage::new() }
    fn push(&mut self, frame: Frame<Span>) -> FrameId { FrameStorage::push(self, frame) }
    fn checkpoint(&self) -> FrameCheckpoint { FrameStorage::checkpoint(self) }
    fn retire(&mut self, id: FrameId) { FrameStorage::retire(self, id) }
    fn select(
        &mut self,
        checkpoint: FrameCheckpoint,
        previous: Option<FrameId>,
        selected: Option<FrameId>,
    ) {
        FrameStorage::select(self, checkpoint, previous, selected)
    }
    fn finish(&mut self, restore: bool) { FrameStorage::finish(self, restore) }
    #[cfg(test)]
    fn retained_bytes(&self) -> usize { FrameStorage::retained_bytes(self) }
}

struct CanonicalPathMachine<S: FrameStore> {
    frames: S,
    next_obligation: u64,
    stats: CanonicalPathAdmissionStats,
    #[cfg(test)]
    owner_work: std::cell::Cell<CanonicalPathOwnerWork>,
}

impl CanonicalPathStreamReducer {
    pub(crate) fn new() -> Self {
        Self {
            current: InnerPath::new(true),
            consumed: 0,
            graph_stamp: 0,
            owners: Vec::new(),
            machine: CanonicalPathMachine::new(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn consumed_len(&self) -> usize { self.consumed }

    #[cfg(test)]
    pub(crate) fn retained_state_bytes(&self) -> usize {
        std::mem::size_of::<InnerPath>()
            + self.owners.capacity() * std::mem::size_of::<FocusedRegionOwner>()
    }

    #[cfg(test)]
    pub(crate) fn owner_work(&self) -> CanonicalPathOwnerWork { self.machine.owner_work.get() }

    #[cfg(test)]
    pub(crate) fn retained_frame_bytes(&self) -> usize { self.machine.frames.retained_bytes() }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats {
        CanonicalPathAdmissionStats {
            states: 1,
            frames: self.machine.frames.len(),
            ..self.machine.stats
        }
    }

    pub(crate) fn active_obligation(&self) -> Option<CanonicalPathObligationProgress> {
        self.machine
            .active_obligation_in(&self.current, self.graph_stamp)
    }

    pub(crate) fn plan_opaque_interior(&self, input_len: usize) -> Option<OpaqueInteriorPlan> {
        let available = input_len.checked_sub(self.consumed)?;
        let mut bound = u64::try_from(available).unwrap_or(u64::MAX);
        if let Some(owner) = self.owners.last() {
            if !owner.all_open {
                return None;
            }
            bound = bound.min(
                u64::try_from(
                    owner
                        .all_boundary
                        .end
                        .checked_sub(self.graph_stamp as u128)?,
                )
                .ok()?,
            );
        }
        if self.current.status != PathStatus::Open {
            return None;
        }
        let Frame::Obligation {
            obligation,
            status,
            payload_start,
            ..
        } = &self.machine.frames[self.current.stack?.0]
        else {
            return None;
        };
        if matches!(status, ObligationStatus::AwaitingResolution { .. }) {
            return None;
        }
        let remaining =
            status.remaining(obligation.payload_len, *payload_start, self.graph_stamp)?;
        let width = usize::try_from(bound.min(remaining).saturating_sub(1)).ok()?;
        (width != 0).then_some(OpaqueInteriorPlan {
            from: self.consumed,
            id: obligation.id,
            width,
        })
    }

    pub(crate) fn try_advance_opaque_interior(
        &mut self,
        input_len: usize,
        plan: OpaqueInteriorPlan,
    ) -> bool {
        let Some(fresh) = self.plan_opaque_interior(input_len) else {
            return false;
        };
        if plan.from != fresh.from
            || plan.id != fresh.id
            || plan.width == 0
            || plan.width > fresh.width
        {
            return false;
        }
        let mut current = self.current.clone();
        self.machine
            .feed_path_advance_shallow::<IndexedPathBytes<'_, '_>>(
                &mut current,
                &PathAdvance::OpaqueInterior {
                    width: u64::try_from(plan.width)
                        .expect("a planned width is bounded by a u64 payload"),
                },
            );
        self.machine
            .frames
            .finish(self.owners.is_empty() && current.stack == self.current.stack);
        self.current = current;
        self.consumed += plan.width;
        self.graph_stamp += plan.width;
        self.machine.stats.edges += plan.width;
        self.machine.stats.max_topology_depth =
            self.machine.stats.max_topology_depth.max(self.consumed);
        #[cfg(test)]
        {
            self.machine.stats.bulk_operations += 1;
            self.machine.stats.bulk_bytes += plan.width;
        }
        true
    }

    pub(crate) fn completed_obligation(
        &self,
        input: &IndexedByteRegion<'_>,
    ) -> Result<
        Option<CanonicalPathCompletedObligation<CanonicalByteRange>>,
        CanonicalPathAdmissionError,
    > {
        self.machine.completed_obligation(
            &IndexedPathBytes {
                input,
                consumed: self.consumed,
            },
            &self.current,
        )
    }

    pub(crate) fn edge(
        &mut self,
        input: &IndexedByteRegion<'_>,
        located: CanonicalLocatedByte,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
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
        let bytes = IndexedPathBytes { input, consumed };
        if self.owners.is_empty() && matches!(self.current.status, PathStatus::Dead(_)) {
            let (current, transition) = self.machine.advance(&bytes, &self.current, byte);
            self.current = current;
            self.consumed = consumed;
            return Ok(transition);
        }
        self.machine.stats.edges += 1;
        self.machine.stats.max_topology_depth = self.machine.stats.max_topology_depth.max(consumed);
        let mut current = self.current.clone();
        let transition = if self
            .owners
            .last()
            .is_some_and(|owner| owner.all_boundary.end < consumed as u128)
        {
            current.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                byte.source,
            ));
            CanonicalPathTransition::Continue
        } else {
            self.machine.feed_path_shallow(consumed, &mut current, byte)
        };
        let (current, remaining) = self.settle_focus(&bytes, current, consumed, byte.source);
        self.select_focus(current, remaining, consumed);
        self.consumed = consumed;
        Ok(transition)
    }

    pub(crate) fn value(
        &self,
        has_value: bool,
        source: CanonicalPathSource,
    ) -> Option<Result<CanonicalPathSummary, CanonicalPathAdmissionError>> {
        has_value.then(|| {
            if let Some(owner) = self.owners.first() {
                self.machine.finish_path_with_pending(
                    &owner.owner,
                    source,
                    self.machine.awaiting_obligation_in(&self.current),
                )
            } else {
                self.machine.finish_path(&self.current, source)
            }
        })
    }

    pub(crate) fn resolve_obligation(
        &mut self,
        input: &IndexedByteRegion<'_>,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let bytes = IndexedPathBytes {
            input,
            consumed: self.consumed,
        };
        if self.owners.is_empty() && matches!(self.current.status, PathStatus::Dead(_)) {
            self.current = self
                .machine
                .resolved_state(&bytes, &self.current, id, resolution)?;
            return Ok(());
        }
        let top = self.current.stack.ok_or_else(|| {
            CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                CanonicalPathSource::new(0),
            )
        })?;
        let frame = self.machine.frame(top);
        let mut current = self.current.clone();
        self.machine
            .resolve_leaf_obligation(&mut current, frame, id, resolution)?;
        let endpoint = CanonicalPathSource::from_provenance(
            bytes
                .provenance_at(self.consumed - 1)
                .expect("a pending child has consumed its length"),
        );
        let (current, remaining) = self.settle_focus(&bytes, current, self.graph_stamp, endpoint);
        self.select_focus(current, remaining, self.graph_stamp);
        Ok(())
    }

    fn focus_path(&mut self) {
        while self.current.status == PathStatus::Open {
            let Some(id) = self.current.stack else { break };
            let Frame::Region {
                remaining,
                phase:
                    RegionPhase::Path {
                        remaining: path_remaining,
                        path,
                        ..
                    },
                ..
            } = self.machine.frame(id)
            else {
                break;
            };
            let index = self.owners.len();
            let region_end = self.graph_stamp as u128 + u128::from(remaining);
            let path_end = self.graph_stamp as u128 + u128::from(path_remaining);
            let mut all_boundary = RegionBoundary {
                end: region_end.min(path_end),
                owner: index,
            };
            let mut path_boundary = RegionBoundary {
                end: path_end,
                owner: index,
            };
            let mut all_open = self.current.status == PathStatus::Open;
            if let Some(previous) = self.owners.last() {
                if previous.all_boundary.end <= all_boundary.end {
                    all_boundary = previous.all_boundary;
                }
                if previous.path_boundary.end < path_boundary.end {
                    path_boundary = previous.path_boundary;
                }
                all_open &= previous.all_open;
            }
            self.owners.push(FocusedRegionOwner {
                owner: self.current.clone(),
                region_end,
                path_end,
                all_boundary,
                path_boundary,
                all_open,
            });
            let Frame::Region {
                phase: RegionPhase::Path { path: held, .. },
                ..
            } = &mut self.machine.frames.slots[id.0]
            else {
                unreachable!()
            };
            held.stack = None;
            self.current = path;
            #[cfg(test)]
            self.machine.record_owner_work(|work| work.detachments += 1);
        }
    }

    fn materialize_owners(
        &mut self,
        mut current: InnerPath,
        start: usize,
        end: usize,
        stamp: usize,
    ) -> InnerPath {
        for index in (start..end).rev() {
            let saved = &self.owners[index];
            let id = saved
                .owner
                .stack
                .expect("a suspended owner holds its Region slot");
            let Frame::Region {
                remaining,
                phase:
                    RegionPhase::Path {
                        remaining: path_remaining,
                        path,
                        ..
                    },
                ..
            } = &mut self.machine.frames.slots[id.0]
            else {
                unreachable!("a held owner retains its Region metadata")
            };
            *remaining = u64::try_from(
                saved
                    .region_end
                    .checked_sub(stamp as u128)
                    .expect("selected graph stamp precedes its Region end"),
            )
            .expect("saved Region residual originated as u64");
            *path_remaining = u64::try_from(
                saved
                    .path_end
                    .checked_sub(stamp as u128)
                    .expect("selected graph stamp precedes its path end"),
            )
            .expect("saved path residual originated as u64");
            *path = current;
            current = saved.owner.clone();
            #[cfg(test)]
            self.machine
                .record_owner_work(|work| work.reifications += 1);
        }
        current
    }

    fn finish_focused_owner(
        &mut self,
        bytes: &IndexedPathBytes<'_, '_>,
        child: &InnerPath,
        index: usize,
        stamp: usize,
        endpoint: CanonicalPathSource,
    ) -> InnerPath {
        let saved = &self.owners[index];
        let mut owner = saved.owner.clone();
        let id = owner
            .stack
            .expect("a suspended owner holds its Region slot");
        let remaining = u64::try_from(
            saved
                .region_end
                .checked_sub(stamp as u128)
                .expect("completion stays inside its Region"),
        )
        .expect("saved residual originated as u64");
        let Frame::Region {
            previous,
            phase:
                RegionPhase::Path {
                    declared_len,
                    start_depth,
                    ..
                },
            next,
            ..
        } = self.machine.frame(id)
        else {
            unreachable!()
        };
        let checkpoint = self.machine.frames.checkpoint();
        self.machine.finish_region_path(
            bytes,
            &mut owner,
            child,
            remaining,
            declared_len,
            start_depth,
            previous,
            next,
            endpoint,
        );
        self.machine
            .frames
            .select(checkpoint, Some(id), owner.stack);
        owner
    }

    fn settle_focus(
        &mut self,
        bytes: &IndexedPathBytes<'_, '_>,
        mut current: InnerPath,
        stamp: usize,
        endpoint: CanonicalPathSource,
    ) -> (InnerPath, usize) {
        let mut remaining = self.owners.len();
        loop {
            if matches!(current.status, PathStatus::Dead(_))
                || self.machine.awaiting_obligation_in(&current)
                || remaining == 0
            {
                return (current, remaining);
            }
            #[cfg(test)]
            self.machine.record_owner_work(|work| work.settlements += 1);
            let due = self.owners[remaining - 1].path_boundary;
            if due.end != stamp as u128 {
                return (current, remaining);
            }
            if due.owner + 1 != remaining {
                current = self.materialize_owners(current, due.owner + 1, remaining, stamp);
            }
            current = self.finish_focused_owner(bytes, &current, due.owner, stamp, endpoint);
            remaining = due.owner;
        }
    }

    fn select_focus(&mut self, mut current: InnerPath, remaining: usize, stamp: usize) {
        if let PathStatus::Dead(error) = current.status {
            if !self.owners.is_empty() {
                self.machine.frames.finish(true);
                current = self.materialize_owners(
                    self.current.clone(),
                    0,
                    self.owners.len(),
                    self.graph_stamp,
                );
                current.kill(error);
                self.current = current;
                self.owners.clear();
                return;
            }
        }
        self.machine
            .frames
            .finish(self.owners.is_empty() && current.stack == self.current.stack);
        self.current = current;
        self.owners.truncate(remaining);
        self.graph_stamp = stamp;
        self.focus_path();
    }
}

impl<S: FrameStore> CanonicalPathMachine<S> {
    fn new() -> Self {
        Self {
            frames: S::new(),
            next_obligation: 0,
            stats: CanonicalPathAdmissionStats::default(),
            #[cfg(test)]
            owner_work: std::cell::Cell::default(),
        }
    }

    #[cfg(test)]
    fn record_owner_work(&self, record: impl FnOnce(&mut CanonicalPathOwnerWork)) {
        let mut work = self.owner_work.get();
        record(&mut work);
        self.owner_work.set(work);
    }

    fn prepare_edge(
        &self,
        current: &InnerPath,
        located: CanonicalLocatedByte,
    ) -> Result<LocatedPathByte, CanonicalPathAdmissionError> {
        let byte = LocatedPathByte {
            byte: located.byte,
            source: CanonicalPathSource::from_provenance(located.provenance),
        };
        if self.awaiting_obligation_in(current) {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                byte.source,
            ));
        }
        Ok(byte)
    }

    fn advance<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        current: &InnerPath,
        byte: LocatedPathByte,
    ) -> (InnerPath, CanonicalPathTransition) {
        self.stats.edges += 1;
        self.stats.max_topology_depth = self.stats.max_topology_depth.max(bytes.consumed_len());
        let mut state = current.clone();
        let transition = self.feed_path(&mut state, PathAdvance::Byte { bytes, byte });
        self.frames.finish(state.stack == current.stack);
        (state, transition)
    }

    fn resolved_state<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        current: &InnerPath,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<InnerPath, CanonicalPathAdmissionError> {
        let mut state = current.clone();
        let result = self.resolve_path_obligation(bytes, &mut state, id, resolution);
        self.frames
            .finish(result.is_err() || state.stack == current.stack);
        result?;
        Ok(state)
    }

    fn completed_obligation<'a, B: PathByteView<Span = S::Span>>(
        &'a self,
        bytes: &B,
        mut path: &'a InnerPath,
    ) -> Result<Option<CanonicalPathCompletedObligation<S::Span>>, CanonicalPathAdmissionError>
    {
        loop {
            if matches!(path.status, PathStatus::Dead(_)) {
                return Ok(None);
            }
            let Some(top) = path.stack else {
                return Ok(None);
            };
            match &self.frames[top.0] {
                Frame::Obligation {
                    obligation,
                    status: ObligationStatus::AwaitingResolution { payload_end },
                    payload_start,
                    ..
                } => {
                    let payload = bytes
                        .slice(*payload_start..bytes.consumed_len())
                        .ok_or_else(|| {
                            CanonicalPathAdmissionError::new(
                                CanonicalPathAdmissionErrorKind::SizeOverflow,
                                obligation.tag_source,
                            )
                        })?;
                    return Ok(Some(CanonicalPathCompletedObligation {
                        obligation: *obligation,
                        payload,
                        payload_end: *payload_end,
                    }));
                }
                Frame::Region {
                    phase: RegionPhase::Path { path: child, .. },
                    ..
                } => {
                    #[cfg(test)]
                    self.record_owner_work(|work| work.completed_queries += 1);
                    path = child;
                }
                _ => return Ok(None),
            }
        }
    }

    fn frame(&self, id: FrameId) -> Frame<S::Span> { self.frames[id.0].clone() }

    fn push_frame(&mut self, frame: Frame<S::Span>) -> FrameId { self.frames.push(frame) }

    fn push_varint(&mut self, role: VarintRole, next: Option<FrameId>) -> FrameId {
        self.push_frame(Frame::Varint {
            role,
            state: VarintState::default(),
            next,
        })
    }

    fn feed_path<B: PathByteView<Span = S::Span>>(
        &mut self,
        root: &mut InnerPath,
        advance: PathAdvance<'_, B>,
    ) -> CanonicalPathTransition {
        struct RegionContext<Span> {
            owner: InnerPath,
            checkpoint: FrameCheckpoint,
            remaining: u64,
            previous: Option<Span>,
            declared_len: u64,
            path_remaining: u64,
            start_depth: usize,
            next: Option<FrameId>,
        }

        let width = advance.width();
        let mut current = root.clone();
        let mut contexts = Vec::<RegionContext<S::Span>>::new();
        let transition = loop {
            if matches!(current.status, PathStatus::Dead(_)) {
                break CanonicalPathTransition::Continue;
            }
            let Some(top) = current.stack else {
                break self.feed_path_advance_shallow(&mut current, &advance);
            };
            let Frame::Region {
                remaining,
                previous,
                phase:
                    RegionPhase::Path {
                        declared_len,
                        remaining: path_remaining,
                        start_depth,
                        path,
                    },
                next,
            } = self.frame(top)
            else {
                break self.feed_path_advance_shallow(&mut current, &advance);
            };

            if remaining < width || path_remaining < width {
                let (_, byte) = advance.scalar();
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                    byte.source,
                ));
                break CanonicalPathTransition::Continue;
            }
            #[cfg(test)]
            {
                self.stats.region_descents += 1;
            }
            contexts.push(RegionContext {
                owner: current,
                checkpoint: self.frames.checkpoint(),
                remaining: remaining - width,
                previous,
                declared_len,
                path_remaining: path_remaining - width,
                start_depth,
                next,
            });
            current = path;
        };

        while let Some(context) = contexts.pop() {
            #[cfg(test)]
            self.record_owner_work(|work| work.rebuilds += 1);
            let mut owner = context.owner;
            let previous = owner.stack;
            if let PathStatus::Dead(error) = current.status {
                owner.kill(error);
                self.frames
                    .select(context.checkpoint, previous, owner.stack);
                current = owner;
                continue;
            }
            if context.path_remaining != 0 {
                let region = self.push_frame(Frame::Region {
                    remaining: context.remaining,
                    previous: context.previous,
                    phase: RegionPhase::Path {
                        declared_len: context.declared_len,
                        remaining: context.path_remaining,
                        start_depth: context.start_depth,
                        path: current,
                    },
                    next: context.next,
                });
                owner.stack = Some(region);
                self.frames
                    .select(context.checkpoint, previous, owner.stack);
                current = owner;
                continue;
            }

            // A sibling validator resolves an opaque payload only after this
            // edge returns.  Preserve a zero-remaining region frame so that a
            // last-byte obligation can resume and finalize its framed path.
            if self.awaiting_obligation_in(&current) {
                let region = self.push_frame(Frame::Region {
                    remaining: context.remaining,
                    previous: context.previous,
                    phase: RegionPhase::Path {
                        declared_len: context.declared_len,
                        remaining: 0,
                        start_depth: context.start_depth,
                        path: current,
                    },
                    next: context.next,
                });
                owner.stack = Some(region);
                self.frames
                    .select(context.checkpoint, previous, owner.stack);
                current = owner;
                continue;
            }

            let (bytes, byte) = advance.scalar();
            let endpoint = byte.source;
            self.finish_region_path(
                bytes,
                &mut owner,
                &current,
                context.remaining,
                context.declared_len,
                context.start_depth,
                context.previous,
                context.next,
                endpoint,
            );
            self.frames
                .select(context.checkpoint, previous, owner.stack);
            current = owner;
        }
        *root = current;
        transition
    }

    fn feed_path_advance_shallow<B: PathByteView<Span = S::Span>>(
        &mut self,
        path: &mut InnerPath,
        advance: &PathAdvance<'_, B>,
    ) -> CanonicalPathTransition {
        match advance {
            PathAdvance::Byte { bytes, byte } => {
                self.feed_path_shallow(bytes.consumed_len(), path, *byte)
            }
            PathAdvance::OpaqueInterior { width } => {
                let top = path
                    .stack
                    .expect("a planned interior has an active payload");
                match &self.frames[top.0] {
                    Frame::Obligation {
                        status: ObligationStatus::Payload,
                        ..
                    } => {
                        let _ = width;
                    }
                    #[cfg(test)]
                    Frame::Obligation {
                        obligation,
                        status: ObligationStatus::Countdown { remaining },
                        payload_start,
                        next,
                    } => {
                        let (obligation, remaining, payload_start, next) =
                            (*obligation, *remaining, *payload_start, *next);
                        let checkpoint = self.frames.checkpoint();
                        let previous = path.stack;
                        path.stack = Some(self.push_frame(Frame::Obligation {
                            obligation,
                            status: ObligationStatus::Countdown {
                                remaining: remaining - width,
                            },
                            payload_start,
                            next,
                        }));
                        self.frames.select(checkpoint, previous, path.stack);
                    }
                    _ => unreachable!("only opaque payloads admit interior plans"),
                }
                CanonicalPathTransition::Continue
            }
        }
    }

    fn feed_path_shallow(
        &mut self,
        path_len: usize,
        path: &mut InnerPath,
        byte: LocatedPathByte,
    ) -> CanonicalPathTransition {
        match path.status {
            PathStatus::Dead(_) => return CanonicalPathTransition::Continue,
            PathStatus::SplitComplete => {
                path.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::TerminatorMisplaced,
                    byte.source,
                ));
                return CanonicalPathTransition::Continue;
            }
            PathStatus::Open => {}
        }

        if path.stack.is_none() {
            if byte.byte == TERM {
                path.status = PathStatus::SplitComplete;
                return CanonicalPathTransition::Continue;
            }
            if path.first_tag.is_none() {
                path.first_tag = Some((byte.byte, byte.source));
            }
            let tag = self.push_frame(Frame::Tag {
                escape_allowed: path.escape_allowed,
                next: None,
            });
            path.stack = Some(tag);
        }
        self.feed_frame(path_len, path, byte)
    }

    fn feed_frame(
        &mut self,
        path_len: usize,
        path: &mut InnerPath,
        byte: LocatedPathByte,
    ) -> CanonicalPathTransition {
        let checkpoint = self.frames.checkpoint();
        let previous = path.stack;
        let transition = self.feed_frame_inner(path_len, path, byte);
        self.frames.select(checkpoint, previous, path.stack);
        transition
    }

    fn feed_frame_inner(
        &mut self,
        path_len: usize,
        path: &mut InnerPath,
        byte: LocatedPathByte,
    ) -> CanonicalPathTransition {
        let top = path.stack.expect("a shallow feed has one active frame");
        match self.frame(top) {
            Frame::Tag {
                escape_allowed,
                next,
            } => self.feed_tag(path, byte, escape_allowed, next),
            Frame::Varint { role, state, next } => match state.feed(byte) {
                Err(error) => {
                    path.kill(error);
                    CanonicalPathTransition::Continue
                }
                Ok(VarintFeed::More(state)) => {
                    path.stack = Some(self.push_frame(Frame::Varint { role, state, next }));
                    CanonicalPathTransition::Continue
                }
                Ok(VarintFeed::Complete(value)) => {
                    self.finish_varint(path, role, value, next, byte.source, path_len)
                }
            },
            Frame::Bytes {
                remaining,
                utf8,
                next,
            } => {
                let utf8 = match utf8 {
                    Some(state) => match state.feed(byte.byte) {
                        Some(state) => Some(state),
                        None => {
                            path.kill(CanonicalPathAdmissionError::new(
                                CanonicalPathAdmissionErrorKind::InvalidUtf8,
                                byte.source,
                            ));
                            return CanonicalPathTransition::Continue;
                        }
                    },
                    None => None,
                };
                let remaining = remaining - 1;
                if remaining != 0 {
                    path.stack = Some(self.push_frame(Frame::Bytes {
                        remaining,
                        utf8,
                        next,
                    }));
                } else if utf8.is_some_and(|state| !state.complete()) {
                    path.kill(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::InvalidUtf8,
                        byte.source,
                    ));
                } else {
                    path.stack = next;
                    self.bubble(path, true);
                }
                CanonicalPathTransition::Continue
            }
            Frame::Kids { .. } => {
                path.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::Truncated,
                    byte.source,
                ));
                CanonicalPathTransition::Continue
            }
            Frame::Region {
                remaining,
                previous,
                phase: RegionPhase::PathLength(state),
                next,
            } => {
                if remaining == 0 {
                    path.kill(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                        byte.source,
                    ));
                    return CanonicalPathTransition::Continue;
                }
                let remaining = remaining - 1;
                match state.feed(byte) {
                    Err(error) => path.kill(error),
                    Ok(VarintFeed::More(state)) => {
                        if remaining == 0 {
                            path.kill(CanonicalPathAdmissionError::new(
                                CanonicalPathAdmissionErrorKind::Truncated,
                                byte.source,
                            ));
                        } else {
                            path.stack = Some(self.push_frame(Frame::Region {
                                remaining,
                                previous,
                                phase: RegionPhase::PathLength(state),
                                next,
                            }));
                        }
                    }
                    Ok(VarintFeed::Complete(length)) => {
                        if length == 0 {
                            path.kill(CanonicalPathAdmissionError::new(
                                CanonicalPathAdmissionErrorKind::EmptyPathFrame,
                                byte.source,
                            ));
                        } else if length > remaining {
                            path.kill(CanonicalPathAdmissionError::new(
                                CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                                byte.source,
                            ));
                        } else {
                            path.stack = Some(self.push_frame(Frame::Region {
                                remaining,
                                previous,
                                phase: RegionPhase::Path {
                                    declared_len: length,
                                    remaining: length,
                                    start_depth: path_len,
                                    path: InnerPath::new(false),
                                },
                                next,
                            }));
                        }
                    }
                }
                CanonicalPathTransition::Continue
            }
            Frame::Region { .. } => unreachable!("active region paths are handled iteratively"),
            Frame::Obligation {
                obligation,
                status,
                payload_start,
                next,
            } => match status {
                ObligationStatus::Payload => {
                    let remaining = status
                        .remaining(obligation.payload_len, payload_start, path_len)
                        .expect("an active payload byte lies within its declared length");
                    if remaining == 0 {
                        path.stack = Some(self.push_frame(Frame::Obligation {
                            obligation,
                            status: ObligationStatus::AwaitingResolution {
                                payload_end: byte.source,
                            },
                            payload_start,
                            next,
                        }));
                    }
                    CanonicalPathTransition::ObligationByte {
                        id: obligation.id,
                        byte,
                        last: remaining == 0,
                    }
                }
                #[cfg(test)]
                ObligationStatus::Countdown { remaining } => {
                    let remaining = remaining - 1;
                    path.stack = Some(self.push_frame(Frame::Obligation {
                        obligation,
                        status: if remaining == 0 {
                            ObligationStatus::AwaitingResolution {
                                payload_end: byte.source,
                            }
                        } else {
                            ObligationStatus::Countdown { remaining }
                        },
                        payload_start,
                        next,
                    }));
                    CanonicalPathTransition::ObligationByte {
                        id: obligation.id,
                        byte,
                        last: remaining == 0,
                    }
                }
                ObligationStatus::AwaitingResolution { .. } => {
                    unreachable!("edge checks this state")
                }
            },
        }
    }

    fn feed_tag(
        &mut self,
        path: &mut InnerPath,
        byte: LocatedPathByte,
        escape_allowed: bool,
        next: Option<FrameId>,
    ) -> CanonicalPathTransition {
        match byte.byte {
            GBOOL_FALSE | GBOOL_TRUE => {
                path.stack = next;
                self.bubble(path, true);
            }
            GINT => path.stack = Some(self.push_varint(VarintRole::Scalar, next)),
            GSTRING | GURI => {
                path.stack = Some(self.push_varint(VarintRole::Bytes { utf8: true }, next))
            }
            GBYTE_ARRAY | GBIG_INT | GPRIVATE => {
                path.stack = Some(self.push_varint(VarintRole::Bytes { utf8: false }, next))
            }
            GDOUBLE => {
                path.stack = Some(self.push_frame(Frame::Bytes {
                    remaining: 8,
                    utf8: None,
                    next,
                }));
            }
            GBIG_RAT => {
                let denominator = self.push_varint(VarintRole::Bytes { utf8: false }, next);
                path.stack =
                    Some(self.push_varint(VarintRole::Bytes { utf8: false }, Some(denominator)));
            }
            GFIXED_POINT => {
                let scale = self.push_varint(VarintRole::Scale, next);
                path.stack = Some(self.push_varint(VarintRole::Bytes { utf8: false }, Some(scale)));
            }
            ELIST | ETUPLE => path.stack = Some(self.push_varint(VarintRole::Count, next)),
            EPATHMAP => path.stack = Some(self.push_varint(VarintRole::RegionLength, next)),
            ESCAPE if escape_allowed => {
                path.stack = Some(self.push_varint(
                    VarintRole::ObligationLength {
                        kind: CanonicalPathObligationKind::RawParEscape,
                        tag_source: byte.source,
                    },
                    next,
                ));
            }
            ESCAPE => path.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::EscapeNested,
                byte.source,
            )),
            EPATHMAP_MAP => {
                path.stack = Some(self.push_varint(
                    VarintRole::ObligationLength {
                        kind: CanonicalPathObligationKind::NestedEpm1,
                        tag_source: byte.source,
                    },
                    next,
                ));
            }
            TERM => path.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::TerminatorMisplaced,
                byte.source,
            )),
            reserved => path.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::ReservedTag(reserved),
                byte.source,
            )),
        }
        CanonicalPathTransition::Continue
    }

    fn finish_varint(
        &mut self,
        path: &mut InnerPath,
        role: VarintRole,
        value: u64,
        next: Option<FrameId>,
        source: CanonicalPathSource,
        path_len: usize,
    ) -> CanonicalPathTransition {
        match role {
            VarintRole::Scalar => {
                path.stack = next;
                self.bubble(path, true);
                CanonicalPathTransition::Continue
            }
            VarintRole::Scale => {
                if u32::try_from(value).is_err() {
                    path.kill(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::ScaleOutOfRange,
                        source,
                    ));
                } else {
                    path.stack = next;
                    self.bubble(path, true);
                }
                CanonicalPathTransition::Continue
            }
            VarintRole::Bytes { utf8 } => {
                if value == 0 {
                    path.stack = next;
                    self.bubble(path, true);
                } else {
                    path.stack = Some(self.push_frame(Frame::Bytes {
                        remaining: value,
                        utf8: utf8.then(Utf8State::default),
                        next,
                    }));
                }
                CanonicalPathTransition::Continue
            }
            VarintRole::Count => {
                if value == 0 {
                    path.stack = next;
                    self.bubble(path, true);
                } else {
                    let kids = self.push_frame(Frame::Kids {
                        remaining: value,
                        next,
                    });
                    path.stack = Some(self.push_frame(Frame::Tag {
                        escape_allowed: false,
                        next: Some(kids),
                    }));
                }
                CanonicalPathTransition::Continue
            }
            VarintRole::RegionLength => {
                if value == 0 {
                    path.stack = next;
                    self.bubble(path, true);
                } else {
                    path.stack = Some(self.push_frame(Frame::Region {
                        remaining: value,
                        previous: None,
                        phase: RegionPhase::PathLength(VarintState::default()),
                        next,
                    }));
                }
                CanonicalPathTransition::Continue
            }
            VarintRole::ObligationLength { kind, tag_source } => {
                let obligation = CanonicalPathObligation {
                    id: CanonicalPathObligationId(self.next_obligation),
                    kind,
                    payload_len: value,
                    tag_source,
                };
                self.next_obligation += 1;
                path.stack = Some(self.push_frame(Frame::Obligation {
                    obligation,
                    status: if value == 0 {
                        ObligationStatus::AwaitingResolution {
                            payload_end: source,
                        }
                    } else {
                        #[cfg(test)]
                        if S::EAGER_PAYLOADS {
                            ObligationStatus::Countdown { remaining: value }
                        } else {
                            ObligationStatus::Payload
                        }
                        #[cfg(not(test))]
                        ObligationStatus::Payload
                    },
                    payload_start: path_len,
                    next,
                }));
                CanonicalPathTransition::ObligationStarted(obligation)
            }
        }
    }

    fn bubble(&mut self, path: &mut InnerPath, segment_stable: bool) {
        loop {
            let Some(top) = path.stack else {
                path.completed_segments += 1;
                path.stable &= segment_stable;
                return;
            };
            match self.frame(top) {
                Frame::Kids { remaining, next } => {
                    self.frames.retire(top);
                    if remaining == 1 {
                        path.stack = next;
                        continue;
                    }
                    let kids = self.push_frame(Frame::Kids {
                        remaining: remaining - 1,
                        next,
                    });
                    path.stack = Some(self.push_frame(Frame::Tag {
                        escape_allowed: false,
                        next: Some(kids),
                    }));
                    return;
                }
                _ => return,
            }
        }
    }

    fn finish_path(
        &self,
        path: &InnerPath,
        source: CanonicalPathSource,
    ) -> Result<CanonicalPathSummary, CanonicalPathAdmissionError> {
        self.finish_path_with_pending(path, source, self.awaiting_obligation_in(path))
    }

    fn finish_path_with_pending(
        &self,
        path: &InnerPath,
        source: CanonicalPathSource,
        pending: bool,
    ) -> Result<CanonicalPathSummary, CanonicalPathAdmissionError> {
        if let PathStatus::Dead(error) = path.status {
            return Err(error);
        }
        if path.stack.is_some() {
            return Err(CanonicalPathAdmissionError::new(
                if pending {
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation
                } else {
                    CanonicalPathAdmissionErrorKind::Truncated
                },
                source,
            ));
        }
        if path.status == PathStatus::SplitComplete {
            return Ok(CanonicalPathSummary {
                form: CanonicalTopLevelPathForm::Split,
                canonical_path_stable: path.stable,
            });
        }
        match path.completed_segments {
            0 => Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::EmptyPath,
                source,
            )),
            1 => {
                let (first, first_source) = path.first_tag.expect("one segment has a first tag");
                if first == ELIST {
                    Err(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::BareTopLevelList,
                        first_source,
                    ))
                } else {
                    Ok(CanonicalPathSummary {
                        form: CanonicalTopLevelPathForm::Bare,
                        canonical_path_stable: path.stable,
                    })
                }
            }
            _ => Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::UnterminatedMultiSegment,
                source,
            )),
        }
    }

    fn active_obligation_in(
        &self,
        root: &InnerPath,
        stamp: usize,
    ) -> Option<CanonicalPathObligationProgress> {
        let Frame::Obligation {
            obligation,
            status,
            payload_start,
            ..
        } = self.active_obligation_frame(root)?
        else {
            unreachable!()
        };
        Some(CanonicalPathObligationProgress {
            obligation: *obligation,
            remaining: status
                .remaining(obligation.payload_len, *payload_start, stamp)
                .expect("selected payload stamp lies within its declared length"),
            awaiting_resolution: matches!(status, ObligationStatus::AwaitingResolution { .. }),
        })
    }

    fn awaiting_obligation_in(&self, root: &InnerPath) -> bool {
        matches!(
            self.active_obligation_frame(root),
            Some(Frame::Obligation {
                status: ObligationStatus::AwaitingResolution { .. },
                ..
            })
        )
    }

    fn active_obligation_frame(&self, root: &InnerPath) -> Option<&Frame<S::Span>> {
        let mut path = root;
        loop {
            if matches!(path.status, PathStatus::Dead(_)) {
                return None;
            }
            let top = path.stack?;
            match &self.frames[top.0] {
                frame @ Frame::Obligation { .. } => return Some(frame),
                Frame::Region {
                    phase: RegionPhase::Path { path: child, .. },
                    ..
                } => {
                    #[cfg(test)]
                    self.record_owner_work(|work| work.active_queries += 1);
                    path = child;
                }
                _ => return None,
            }
        }
    }

    fn resolve_path_obligation<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        root: &mut InnerPath,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let mut current = root.clone();
        let mut owners = Vec::<(InnerPath, Frame<S::Span>, FrameCheckpoint)>::new();
        loop {
            let top = current.stack.ok_or_else(|| {
                CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    CanonicalPathSource::new(0),
                )
            })?;
            match self.frame(top) {
                frame @ Frame::Region {
                    phase: RegionPhase::Path { .. },
                    ..
                } => {
                    let Frame::Region {
                        phase: RegionPhase::Path { path, .. },
                        ..
                    } = &frame
                    else {
                        unreachable!()
                    };
                    let child = path.clone();
                    #[cfg(test)]
                    self.record_owner_work(|work| work.resolutions += 1);
                    owners.push((current, frame, self.frames.checkpoint()));
                    current = child;
                }
                frame => {
                    self.resolve_leaf_obligation(&mut current, frame, id, resolution)?;
                    break;
                }
            }
        }
        self.rebuild_region_owners(bytes, &mut current, owners);
        *root = current;
        Ok(())
    }

    fn resolve_leaf_obligation(
        &mut self,
        current: &mut InnerPath,
        frame: Frame<S::Span>,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let (obligation, next) = match frame {
            Frame::Obligation {
                obligation,
                status: ObligationStatus::AwaitingResolution { .. },
                next,
                ..
            } if obligation.id == id => (obligation, next),
            Frame::Obligation { obligation, .. } => {
                return Err(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    obligation.tag_source,
                ));
            }
            _ => {
                return Err(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    CanonicalPathSource::new(0),
                ));
            }
        };
        let checkpoint = self.frames.checkpoint();
        let previous = current.stack;
        let stable = match (obligation.kind, resolution) {
            (
                CanonicalPathObligationKind::RawParEscape,
                CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: false,
                },
            ) => false,
            (
                CanonicalPathObligationKind::RawParEscape,
                CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: true,
                },
            ) => {
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::EscapeOfStablePar,
                    obligation.tag_source,
                ));
                false
            }
            (
                CanonicalPathObligationKind::NestedEpm1,
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                },
            ) => true,
            (
                CanonicalPathObligationKind::NestedEpm1,
                CanonicalPathObligationResolution::NestedEpm1 { mode, .. },
            ) if mode != NestedEpmMode::Map => {
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::NestedEpmNotMap,
                    obligation.tag_source,
                ));
                false
            }
            (
                CanonicalPathObligationKind::NestedEpm1,
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: false,
                },
            ) => {
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::NestedEpmNotStable,
                    obligation.tag_source,
                ));
                false
            }
            _ => {
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::RejectedObligation,
                    obligation.tag_source,
                ));
                false
            }
        };
        if matches!(current.status, PathStatus::Open) {
            current.stack = next;
            self.bubble(current, stable);
        }
        self.frames.select(checkpoint, previous, current.stack);
        Ok(())
    }

    #[cfg(test)]
    fn reject_path_obligation<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        root: &mut InnerPath,
        id: CanonicalPathObligationId,
        source: CanonicalPathSource,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let mut current = root.clone();
        let mut owners = Vec::<(InnerPath, Frame<S::Span>, FrameCheckpoint)>::new();
        loop {
            let top = current.stack.ok_or_else(|| {
                CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                    source,
                )
            })?;
            match self.frame(top) {
                frame @ Frame::Region {
                    phase: RegionPhase::Path { .. },
                    ..
                } => {
                    let Frame::Region {
                        phase: RegionPhase::Path { path, .. },
                        ..
                    } = &frame
                    else {
                        unreachable!()
                    };
                    let child = path.clone();
                    self.record_owner_work(|work| work.resolutions += 1);
                    owners.push((current, frame, self.frames.checkpoint()));
                    current = child;
                }
                Frame::Obligation { obligation, .. } if obligation.id == id => {
                    current.kill(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::RejectedObligation,
                        source,
                    ));
                    break;
                }
                Frame::Obligation { obligation, .. } => {
                    return Err(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                        obligation.tag_source,
                    ));
                }
                _ => {
                    return Err(CanonicalPathAdmissionError::new(
                        CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                        source,
                    ));
                }
            }
        }
        self.rebuild_region_owners(bytes, &mut current, owners);
        *root = current;
        Ok(())
    }

    fn rebuild_region_owners<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        current: &mut InnerPath,
        mut owners: Vec<(InnerPath, Frame<S::Span>, FrameCheckpoint)>,
    ) {
        while let Some((mut owner, frame, checkpoint)) = owners.pop() {
            #[cfg(test)]
            self.record_owner_work(|work| work.rebuilds += 1);
            let previous_owner = owner.stack;
            let Frame::Region {
                remaining,
                previous,
                phase:
                    RegionPhase::Path {
                        declared_len,
                        remaining: path_remaining,
                        start_depth,
                        ..
                    },
                next,
            } = frame
            else {
                unreachable!("obligation owner is an active region path")
            };
            if let PathStatus::Dead(error) = current.status {
                owner.kill(error);
            } else if path_remaining == 0 {
                let endpoint = CanonicalPathSource::from_provenance(
                    bytes
                        .provenance_at(bytes.consumed_len() - 1)
                        .expect("a suspended region path consumed a final edge"),
                );
                self.finish_region_path(
                    bytes,
                    &mut owner,
                    current,
                    remaining,
                    declared_len,
                    start_depth,
                    previous,
                    next,
                    endpoint,
                );
            } else {
                owner.stack = Some(self.push_frame(Frame::Region {
                    remaining,
                    previous,
                    phase: RegionPhase::Path {
                        declared_len,
                        remaining: path_remaining,
                        start_depth,
                        path: current.clone(),
                    },
                    next,
                }));
            }
            self.frames.select(checkpoint, previous_owner, owner.stack);
            *current = owner;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_region_path<B: PathByteView<Span = S::Span>>(
        &mut self,
        bytes: &B,
        owner: &mut InnerPath,
        child: &InnerPath,
        remaining: u64,
        declared_len: u64,
        start_depth: usize,
        previous: Option<S::Span>,
        next: Option<FrameId>,
        endpoint: CanonicalPathSource,
    ) {
        if let Err(error) = self.finish_path(child, endpoint) {
            owner.kill(error);
            return;
        }
        if !child.stable {
            owner.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::EscapeNested,
                endpoint,
            ));
            return;
        }
        let completed = match bytes.slice(start_depth..bytes.consumed_len()) {
            Some(completed) => completed,
            None => {
                owner.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::SizeOverflow,
                    endpoint,
                ));
                return;
            }
        };
        if u64::try_from(bytes.span_len(completed)).ok() != Some(declared_len) {
            owner.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                endpoint,
            ));
            return;
        }
        if let Some(previous) = previous {
            let order = bytes.compare(previous, completed);
            if order != std::cmp::Ordering::Less {
                let source = bytes
                    .first_mismatch_source(previous, completed)
                    .map(CanonicalPathSource::from_provenance)
                    .unwrap_or(endpoint);
                owner.kill(CanonicalPathAdmissionError::new(
                    if order == std::cmp::Ordering::Equal {
                        CanonicalPathAdmissionErrorKind::DuplicatePathInRegion
                    } else {
                        CanonicalPathAdmissionErrorKind::NonAscendingRegion
                    },
                    source,
                ));
                return;
            }
        }

        if remaining == 0 {
            owner.stack = next;
            self.bubble(owner, true);
        } else {
            let region = self.push_frame(Frame::Region {
                remaining,
                previous: Some(completed),
                phase: RegionPhase::PathLength(VarintState::default()),
                next,
            });
            owner.stack = Some(region);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CanonicalPathRestoreRun {
    latest: CanonicalPathReducerHandle,
    first_depth: usize,
}

pub(crate) struct CanonicalPathTopologyAdmission<'src> {
    reducer: CanonicalPathReducer,
    arena: CanonicalBytesArena<'src>,
    current: CanonicalPathReducerHandle,
    restore: Vec<CanonicalPathRestoreRun>,
    root_left: bool,
    source: Option<(CanonicalSourceSlot, usize)>,
}

impl Default for CanonicalPathTopologyAdmission<'_> {
    fn default() -> Self { Self::new() }
}

impl<'src> CanonicalPathTopologyAdmission<'src> {
    pub(crate) fn new() -> Self {
        let arena = CanonicalBytesArena::new();
        let reducer = CanonicalPathReducer::new();
        let current = reducer.root(&arena);
        Self {
            reducer,
            arena,
            current,
            restore: Vec::new(),
            root_left: false,
            source: None,
        }
    }

    pub(crate) fn new_with_source(
        source: Option<SourceTreeWindow<'src>>,
    ) -> Result<Self, CanonicalBytesArenaError> {
        let mut topology = Self::new();
        if let Some(source) = source {
            let len = source.len();
            let slot = topology.arena.register_source(source)?;
            topology.source = Some((slot, len));
        }
        Ok(topology)
    }

    pub(crate) fn forward_capacity(&self, available: usize) -> usize {
        if self.root_left || self.source.is_none() {
            return 0;
        }
        let state = self.reducer.state(self.current);
        let available = available
            .min(usize::MAX - self.current.bytes.len())
            .min(usize::MAX - self.reducer.machine.stats.edges);
        #[cfg(test)]
        let available = if self.reducer.machine.stats.bulk_operations == usize::MAX {
            0
        } else {
            available.min(usize::MAX - self.reducer.machine.stats.bulk_bytes)
        };
        self.reducer
            .reusable_payload_prefix(&state, available)
            .map_or(0, |(_, width)| width)
    }

    pub(crate) fn try_forward_source(
        &mut self,
        range: std::ops::Range<usize>,
    ) -> Result<bool, CanonicalBytesArenaError> {
        if self.root_left {
            return Ok(false);
        }
        let Some((source, len)) = self.source else {
            return Ok(false);
        };
        if range.start > range.end || range.end > len {
            return Err(CanonicalBytesArenaError::SliceOutsideSequence {
                start: range.start,
                end: range.end,
                len,
            });
        }
        let width = range.end - range.start;
        if width == 0 || self.forward_capacity(width) != width {
            return Ok(false);
        }
        let previous = self.current;
        let coalesce = self
            .restore
            .last()
            .is_some_and(|run| run.latest.state == previous.state);
        if !coalesce {
            self.restore
                .try_reserve(1)
                .map_err(|_| CanonicalBytesArenaError::LengthOverflow)?;
        }
        let bytes = self.arena.append_source(previous.bytes, source, range)?;
        let latest = if width == 1 {
            previous
        } else {
            CanonicalPathReducerHandle {
                state: previous.state,
                bytes: self
                    .arena
                    .slice(bytes, 0..bytes.len() - 1)
                    .expect("a positive checked append retains its strict predecessor prefix"),
            }
        };
        if coalesce {
            self.restore
                .last_mut()
                .expect("the checked history run remains present")
                .latest = latest;
        } else {
            self.restore.push(CanonicalPathRestoreRun {
                latest,
                first_depth: previous.bytes.len(),
            });
        }
        self.current = CanonicalPathReducerHandle {
            state: previous.state,
            bytes,
        };
        self.reducer.machine.stats.edges += width;
        self.reducer.machine.stats.max_topology_depth = self
            .reducer
            .machine
            .stats
            .max_topology_depth
            .max(bytes.len());
        #[cfg(test)]
        {
            self.reducer.machine.stats.bulk_operations += 1;
            self.reducer.machine.stats.bulk_bytes += width;
        }
        Ok(true)
    }

    pub(crate) fn reverse_capacity(&self, available: usize) -> usize {
        if self.root_left {
            return 0;
        }
        self.restore
            .last()
            .and_then(|run| run.latest.bytes.len().checked_sub(run.first_depth))
            .map_or(0, |last_offset| {
                available.min(last_offset.saturating_add(1))
            })
    }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats { self.reducer.stats() }

    #[cfg(test)]
    pub(crate) fn checkpoint_resources(&self) -> (usize, usize, usize, CanonicalPathOwnerWork) {
        (
            self.reducer.retained_state_bytes(),
            self.reducer.retained_frame_bytes(),
            self.restore.capacity() * std::mem::size_of::<CanonicalPathRestoreRun>(),
            self.reducer.machine.owner_work.get(),
        )
    }

    pub(crate) fn active_obligation(&self) -> Option<CanonicalPathObligationProgress> {
        self.reducer.active_obligation(self.current)
    }

    pub(crate) fn retained_completed_obligation(
        &mut self,
    ) -> Result<
        Option<CanonicalPathCompletedObligation<SourceTreeWindow<'src>>>,
        CanonicalPathAdmissionError,
    > {
        Ok(self
            .reducer
            .completed_obligation(&self.arena, self.current)?
            .map(|completed| CanonicalPathCompletedObligation {
                obligation: completed.obligation,
                payload: self
                    .arena
                    .retained_source_window(completed.payload)
                    .expect("completed payload is a checked historical arena span"),
                payload_end: completed.payload_end,
            }))
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
        match self.restore.last_mut() {
            Some(run) if run.latest.state == previous.state => {
                debug_assert_eq!(
                    previous.bytes.len().checked_sub(1),
                    Some(run.latest.bytes.len()),
                );
                run.latest = previous;
            }
            _ => self.restore.push(CanonicalPathRestoreRun {
                latest: previous,
                first_depth: previous.bytes.len(),
            }),
        }
        self.current = current;
        Ok(transition)
    }

    pub(crate) fn try_leave_run(&mut self, width: usize) -> Option<CanonicalPathTransition> {
        if width == 0 || self.root_left {
            return None;
        }
        let run = *self.restore.last()?;
        let depth = run.latest.bytes.len();
        let last_offset = depth.checked_sub(run.first_depth)?;
        let offset = width - 1;
        if offset > last_offset {
            return None;
        }
        let selected = if width == 1 {
            run.latest
        } else {
            CanonicalPathReducerHandle {
                state: run.latest.state,
                bytes: self.arena.slice(run.latest.bytes, 0..depth - offset).ok()?,
            }
        };
        let remaining = if offset == last_offset {
            None
        } else {
            Some(CanonicalPathReducerHandle {
                state: run.latest.state,
                bytes: self.arena.slice(run.latest.bytes, 0..depth - width).ok()?,
            })
        };
        if let Some(latest) = remaining {
            self.restore.last_mut()?.latest = latest;
        } else {
            self.restore.pop();
        }
        self.current = selected;
        Some(CanonicalPathTransition::Restored {
            active_obligation: self.active_obligation(),
        })
    }

    pub(crate) fn leave(
        &mut self,
        source: CanonicalPathSource,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
        if let Some(run) = self.restore.last_mut() {
            let previous = run.latest;
            if previous.bytes.len() == run.first_depth {
                self.restore.pop();
            } else {
                debug_assert!(run.first_depth < previous.bytes.len());
                run.latest.bytes = self
                    .arena
                    .slice(previous.bytes, 0..previous.bytes.len() - 1)
                    .expect("a nonsingleton restore run retains its strict predecessor prefix");
            }
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

    /// Verify that the ACT event stream returned through the root and left no
    /// nested validator suspended. This is called exactly once when the
    /// topology cursor reaches its terminal event.
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

#[cfg(test)]
#[path = "../../../tests/support/canonical_frame_storage.rs"]
pub(crate) mod frame_storage_tests;

#[cfg(test)]
#[path = "../../../tests/support/canonical_path_byte_policy.rs"]
mod byte_policy_tests;

#[cfg(test)]
#[path = "../../../tests/support/canonical_path_opaque_interior.rs"]
mod opaque_interior_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::canonical_path;

    fn uv(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        loop {
            let group = (value & 0x7f) as u8;
            value >>= 7;
            if value == 0 {
                bytes.push(group);
                return bytes;
            }
            bytes.push(group | 0x80);
        }
    }

    fn linear_result(bytes: &[u8]) -> Result<CanonicalPathSummary, CanonicalPathAdmissionError> {
        let mut admission = CanonicalPathTopologyAdmission::new();
        assert!(admission
            .value(false, CanonicalPathSource::new(0))
            .is_none());
        for (offset, byte) in bytes.iter().copied().enumerate() {
            let transition = admission
                .edge(LocatedPathByte::new(byte, offset))
                .expect("obligation-free driver use");
            assert_eq!(transition, CanonicalPathTransition::Continue);
            assert!(admission
                .value(false, CanonicalPathSource::new(offset))
                .is_none());
        }
        admission
            .value(true, CanonicalPathSource::new(bytes.len()))
            .expect("one value endpoint")
    }

    fn reducer_result(bytes: &[u8]) -> Result<CanonicalPathSummary, CanonicalPathAdmissionError> {
        let mut arena = CanonicalBytesArena::new();
        let mut reducer = CanonicalPathReducer::new();
        let mut state = reducer.root(&arena);
        for (offset, byte) in bytes.iter().copied().enumerate() {
            (state, _) = reducer.edge(
                &mut arena,
                state,
                CanonicalLocatedByte::new(byte, CanonicalByteProvenance::new(7, offset)),
            )?;
        }
        reducer
            .value(state, true, CanonicalPathSource::new(bytes.len()))
            .expect("one value endpoint")
    }

    #[test]
    fn obligation_free_paths_differential_against_canonical_path_validator() {
        let mut string = vec![GSTRING, 3];
        string.extend_from_slice(b"rho");
        let tuple = vec![ETUPLE, 2, GBOOL_TRUE, GINT, 0];
        let nested_list = vec![ETUPLE, 1, ELIST, 1, GBOOL_FALSE];
        let mut split = string.clone();
        split.extend_from_slice(&[GBOOL_FALSE, TERM]);
        let empty_map = vec![EPATHMAP, 0];

        let mut region_body = Vec::new();
        region_body.extend(uv(1));
        region_body.push(GBOOL_FALSE);
        region_body.extend(uv(1));
        region_body.push(GBOOL_TRUE);
        let mut region = vec![EPATHMAP];
        region.extend(uv(region_body.len() as u64));
        region.extend(region_body);

        let valid = [
            vec![TERM],
            vec![GBOOL_FALSE],
            vec![GINT, 0],
            string,
            vec![GURI, 3, b'r', b'h', b'o'],
            vec![GBYTE_ARRAY, 2, 0, 0xff],
            vec![GDOUBLE, 0, 1, 2, 3, 4, 5, 6, 7],
            vec![GBIG_INT, 1, 0xff],
            vec![GPRIVATE, 2, 0x12, 0x34],
            tuple,
            nested_list,
            split,
            empty_map,
            region,
            vec![GFIXED_POINT, 0, 0],
            vec![GBIG_RAT, 0, 0],
        ];
        for bytes in valid {
            assert!(
                canonical_path::validate_canonical_path(&bytes).is_ok(),
                "old: {bytes:?}"
            );
            assert!(linear_result(&bytes).is_ok(), "new: {bytes:?}");
            assert_eq!(reducer_result(&bytes), linear_result(&bytes));
        }

        let invalid = [
            vec![],
            vec![ELIST, 0],
            vec![GBOOL_FALSE, GBOOL_TRUE],
            vec![GINT],
            vec![GINT, 0x80, 0],
            vec![GSTRING, 1, 0xff],
            vec![ETUPLE, 1, TERM],
            vec![TERM, GBOOL_FALSE],
            vec![0x11],
        ];
        for bytes in invalid {
            assert!(
                canonical_path::validate_canonical_path(&bytes).is_err(),
                "old: {bytes:?}"
            );
            assert!(linear_result(&bytes).is_err(), "new: {bytes:?}");
            assert_eq!(
                reducer_result(&bytes).unwrap_err().kind,
                linear_result(&bytes).unwrap_err().kind
            );
        }
    }

    #[test]
    fn reducer_handles_fork_restore_and_divergent_obligations() {
        let mut arena = CanonicalBytesArena::new();
        let mut reducer = CanonicalPathReducer::new();
        let root = reducer.root(&arena);
        let (escape, _) = reducer
            .edge(
                &mut arena,
                root,
                CanonicalLocatedByte::new(ESCAPE, CanonicalByteProvenance::new(1, 10)),
            )
            .unwrap();

        let (left_length, left_started) = reducer
            .edge(
                &mut arena,
                escape,
                CanonicalLocatedByte::new(1, CanonicalByteProvenance::new(1, 11)),
            )
            .unwrap();
        let CanonicalPathTransition::ObligationStarted(left_obligation) = left_started else {
            panic!("left obligation starts")
        };
        let (left, _) = reducer
            .edge(
                &mut arena,
                left_length,
                CanonicalLocatedByte::new(0xaa, CanonicalByteProvenance::new(1, 12)),
            )
            .unwrap();

        let (right_length, right_started) = reducer
            .edge(
                &mut arena,
                escape,
                CanonicalLocatedByte::new(2, CanonicalByteProvenance::new(2, 20)),
            )
            .unwrap();
        let CanonicalPathTransition::ObligationStarted(right_obligation) = right_started else {
            panic!("right obligation starts")
        };
        let (right_half, _) = reducer
            .edge(
                &mut arena,
                right_length,
                CanonicalLocatedByte::new(0xbb, CanonicalByteProvenance::new(2, 21)),
            )
            .unwrap();
        let (right, _) = reducer
            .edge(
                &mut arena,
                right_half,
                CanonicalLocatedByte::new(0xcc, CanonicalByteProvenance::new(2, 22)),
            )
            .unwrap();

        assert_ne!(left_obligation.id, right_obligation.id);
        assert_eq!(reducer.active_obligation(escape), None);
        assert_eq!(arena.bytes(escape.bytes()).collect::<Vec<_>>(), vec![
            ESCAPE
        ]);
        assert_eq!(
            arena
                .bytes(
                    reducer
                        .completed_obligation(&arena, left)
                        .unwrap()
                        .unwrap()
                        .payload
                )
                .collect::<Vec<_>>(),
            vec![0xaa]
        );
        assert_eq!(
            arena
                .bytes(
                    reducer
                        .completed_obligation(&arena, right)
                        .unwrap()
                        .unwrap()
                        .payload
                )
                .collect::<Vec<_>>(),
            vec![0xbb, 0xcc]
        );

        let left = reducer
            .resolve_obligation(
                &arena,
                left,
                left_obligation.id,
                CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: false,
                },
            )
            .unwrap();
        let right = reducer
            .resolve_obligation(
                &arena,
                right,
                right_obligation.id,
                CanonicalPathObligationResolution::RawPar {
                    canonical_path_stable: false,
                },
            )
            .unwrap();
        assert!(reducer
            .value(left, true, CanonicalPathSource::new(13))
            .unwrap()
            .is_ok());
        assert!(reducer
            .value(right, true, CanonicalPathSource::new(23))
            .unwrap()
            .is_ok());
    }

    #[test]
    fn completed_obligation_retains_frozen_payload_provenance() {
        let mut arena = CanonicalBytesArena::new();
        let mut reducer = CanonicalPathReducer::new();
        let mut state = reducer.root(&arena);
        for located in [
            CanonicalLocatedByte::new(EPATHMAP_MAP, CanonicalByteProvenance::new(8, 100)),
            CanonicalLocatedByte::new(2, CanonicalByteProvenance::new(8, 104)),
            CanonicalLocatedByte::new(0x31, CanonicalByteProvenance::new(9, 3)),
            CanonicalLocatedByte::new(0x32, CanonicalByteProvenance::new(9, 7)),
        ] {
            (state, _) = reducer.edge(&mut arena, state, located).unwrap();
        }
        let completed = reducer
            .freeze_completed_obligation(&arena, state)
            .unwrap()
            .unwrap();
        assert_eq!(
            completed.obligation.kind,
            CanonicalPathObligationKind::NestedEpm1
        );
        assert_eq!(completed.payload.located_bytes().collect::<Vec<_>>(), vec![
            CanonicalLocatedByte::new(0x31, CanonicalByteProvenance::new(9, 3)),
            CanonicalLocatedByte::new(0x32, CanonicalByteProvenance::new(9, 7)),
        ]);
        assert_eq!(completed.payload_end, CanonicalPathSource::in_source(9, 7));
        let _ = arena
            .append(
                state.bytes(),
                CanonicalLocatedByte::new(0xff, CanonicalByteProvenance::new(10, 1)),
            )
            .unwrap();
        drop(arena);
        assert_eq!(completed.payload.bytes().collect::<Vec<_>>(), vec![
            0x31, 0x32
        ]);
    }

    #[test]
    fn empty_obligation_retains_the_length_field_as_its_eof_anchor() {
        let mut arena = CanonicalBytesArena::new();
        let mut reducer = CanonicalPathReducer::new();
        let mut state = reducer.root(&arena);
        for located in [
            CanonicalLocatedByte::new(ESCAPE, CanonicalByteProvenance::new(3, 20)),
            CanonicalLocatedByte::new(0, CanonicalByteProvenance::new(4, 99)),
        ] {
            (state, _) = reducer.edge(&mut arena, state, located).unwrap();
        }
        let completed = reducer
            .freeze_completed_obligation(&arena, state)
            .unwrap()
            .unwrap();
        assert!(completed.payload.is_empty());
        assert_eq!(completed.payload_end, CanonicalPathSource::in_source(4, 99));
    }

    #[test]
    fn pure_reducer_is_linear_and_heap_total_at_twenty_thousand_levels() {
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let depth = 20_000;
                let mut arena = CanonicalBytesArena::new();
                let mut reducer = CanonicalPathReducer::new();
                let mut state = reducer.root(&arena);
                let mut offset = 0;
                for _ in 0..depth {
                    for byte in [ETUPLE, 1] {
                        (state, _) = reducer
                            .edge(
                                &mut arena,
                                state,
                                CanonicalLocatedByte::new(
                                    byte,
                                    CanonicalByteProvenance::new(3, offset),
                                ),
                            )
                            .unwrap();
                        offset += 1;
                    }
                }
                (state, _) = reducer
                    .edge(
                        &mut arena,
                        state,
                        CanonicalLocatedByte::new(
                            GBOOL_TRUE,
                            CanonicalByteProvenance::new(3, offset),
                        ),
                    )
                    .unwrap();
                assert!(reducer
                    .value(state, true, CanonicalPathSource::new(offset + 1))
                    .unwrap()
                    .is_ok());
                let edges = offset + 1;
                let reducer_stats = reducer.stats();
                let arena_stats = arena.stats();
                assert_eq!(reducer_stats.edges, edges);
                assert_eq!(reducer_stats.states, edges + 1);
                assert!(reducer_stats.frames <= edges * 3);
                assert!(arena_stats.content_nodes <= edges);
                assert!(arena_stats.provenance_nodes <= edges);
            })
            .unwrap();
        handle.join().unwrap();
    }

    #[test]
    fn bare_value_prefix_can_fork_into_distinct_split_values_without_state_copy() {
        let mut admission = CanonicalPathTopologyAdmission::new();
        admission
            .edge(LocatedPathByte::new(GBOOL_FALSE, 10))
            .unwrap();
        assert_eq!(
            admission
                .value(true, CanonicalPathSource::new(11))
                .unwrap()
                .unwrap()
                .form,
            CanonicalTopLevelPathForm::Bare
        );
        let before_fork = admission.stats();

        admission
            .edge(LocatedPathByte::new(GBOOL_TRUE, 12))
            .unwrap();
        admission.edge(LocatedPathByte::new(TERM, 13)).unwrap();
        assert_eq!(
            admission
                .value(true, CanonicalPathSource::new(14))
                .unwrap()
                .unwrap()
                .form,
            CanonicalTopLevelPathForm::Split
        );
        admission.leave(CanonicalPathSource::new(13)).unwrap();
        admission.leave(CanonicalPathSource::new(12)).unwrap();
        let after_restore = admission.stats();
        assert_eq!(after_restore.states, before_fork.states + 2);
        assert_eq!(after_restore.frames, before_fork.frames + 1);

        admission.edge(LocatedPathByte::new(GINT, 15)).unwrap();
        admission.edge(LocatedPathByte::new(0, 16)).unwrap();
        admission.edge(LocatedPathByte::new(TERM, 17)).unwrap();
        assert!(admission
            .value(true, CanonicalPathSource::new(18))
            .unwrap()
            .is_ok());
    }

    #[test]
    fn malformed_value_free_topology_is_retained_but_not_admitted_as_a_value() {
        let mut admission = CanonicalPathTopologyAdmission::new();
        admission.edge(LocatedPathByte::new(0xff, 41)).unwrap();
        assert!(admission
            .value(false, CanonicalPathSource::new(42))
            .is_none());
        let error = admission
            .value(true, CanonicalPathSource::new(42))
            .unwrap()
            .unwrap_err();
        assert_eq!(
            error.kind,
            CanonicalPathAdmissionErrorKind::ReservedTag(0xff)
        );
        assert_eq!(error.source.offset, 41);
        admission.leave(CanonicalPathSource::new(43)).unwrap();
        admission
            .edge(LocatedPathByte::new(GBOOL_TRUE, 44))
            .unwrap();
        assert!(admission
            .value(true, CanonicalPathSource::new(45))
            .unwrap()
            .is_ok());
    }

    #[test]
    fn terminal_check_requires_the_topology_to_return_through_the_root() {
        let mut admission = CanonicalPathTopologyAdmission::new();
        assert_eq!(
            admission
                .finish(CanonicalPathSource::new(40))
                .unwrap_err()
                .kind,
            CanonicalPathAdmissionErrorKind::TopologyIncomplete
        );
        admission
            .edge(LocatedPathByte::new(GBOOL_TRUE, 41))
            .unwrap();
        admission.leave(CanonicalPathSource::new(42)).unwrap();
        assert_eq!(
            admission
                .finish(CanonicalPathSource::new(43))
                .unwrap_err()
                .kind,
            CanonicalPathAdmissionErrorKind::TopologyIncomplete
        );
        admission.leave(CanonicalPathSource::new(44)).unwrap();
        admission.finish(CanonicalPathSource::new(45)).unwrap();
    }

    #[test]
    fn utf8_fixed_scale_and_nested_region_order_have_exact_failures() {
        let utf8 = linear_result(&[GSTRING, 2, 0xc2, b'A']).unwrap_err();
        assert_eq!(utf8.kind, CanonicalPathAdmissionErrorKind::InvalidUtf8);
        assert_eq!(utf8.source.offset, 3);

        let mut scale = vec![GFIXED_POINT, 0];
        scale.extend(uv(u64::from(u32::MAX) + 1));
        let scale = linear_result(&scale).unwrap_err();
        assert_eq!(scale.kind, CanonicalPathAdmissionErrorKind::ScaleOutOfRange);

        let mut body = Vec::new();
        body.extend(uv(1));
        body.push(GBOOL_TRUE);
        body.extend(uv(1));
        body.push(GBOOL_FALSE);
        let mut path = vec![EPATHMAP];
        path.extend(uv(body.len() as u64));
        path.extend(body);
        let order = linear_result(&path).unwrap_err();
        assert_eq!(
            order.kind,
            CanonicalPathAdmissionErrorKind::NonAscendingRegion
        );
    }

    #[test]
    fn obligations_stream_restore_and_require_exact_resolution() {
        let mut admission = CanonicalPathTopologyAdmission::new();
        admission.edge(LocatedPathByte::new(ESCAPE, 0)).unwrap();
        let CanonicalPathTransition::ObligationStarted(obligation) =
            admission.edge(LocatedPathByte::new(1, 1)).unwrap()
        else {
            panic!("escape length starts an obligation")
        };
        assert_eq!(obligation.kind, CanonicalPathObligationKind::RawParEscape);
        assert_eq!(
            admission.edge(LocatedPathByte::new(0xaa, 2)).unwrap(),
            CanonicalPathTransition::ObligationByte {
                id: obligation.id,
                byte: LocatedPathByte::new(0xaa, 2),
                last: true,
            }
        );
        admission.leave(CanonicalPathSource::new(2)).unwrap();
        assert_eq!(admission.active_obligation().unwrap().remaining, 1);
        admission.edge(LocatedPathByte::new(0xbb, 3)).unwrap();
        admission
            .resolve_obligation(obligation.id, CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: false,
            })
            .unwrap();
        let summary = admission
            .value(true, CanonicalPathSource::new(4))
            .unwrap()
            .unwrap();
        assert!(!summary.canonical_path_stable);

        let mut alias = CanonicalPathTopologyAdmission::new();
        alias.edge(LocatedPathByte::new(ESCAPE, 10)).unwrap();
        let CanonicalPathTransition::ObligationStarted(obligation) =
            alias.edge(LocatedPathByte::new(0, 11)).unwrap()
        else {
            panic!("zero-length escape still starts an obligation")
        };
        alias
            .resolve_obligation(obligation.id, CanonicalPathObligationResolution::RawPar {
                canonical_path_stable: true,
            })
            .unwrap();
        assert_eq!(
            alias
                .value(true, CanonicalPathSource::new(12))
                .unwrap()
                .unwrap_err()
                .kind,
            CanonicalPathAdmissionErrorKind::EscapeOfStablePar
        );

        let mut map = CanonicalPathTopologyAdmission::new();
        map.edge(LocatedPathByte::new(EPATHMAP_MAP, 20)).unwrap();
        let CanonicalPathTransition::ObligationStarted(obligation) =
            map.edge(LocatedPathByte::new(0, 21)).unwrap()
        else {
            panic!("zero-length EPM starts an obligation")
        };
        map.resolve_obligation(
            obligation.id,
            CanonicalPathObligationResolution::NestedEpm1 {
                mode: NestedEpmMode::Map,
                entries_stable: true,
            },
        )
        .unwrap();
        assert!(map
            .value(true, CanonicalPathSource::new(22))
            .unwrap()
            .is_ok());
    }

    #[test]
    fn nested_epm_obligation_resumes_and_finalizes_its_enclosing_region_path() {
        // 0D region R = uv(2) ++ [0x10, uv(0)].  The obligation begins on
        // the final byte of both the entry path and the region itself.
        let bytes = [EPATHMAP, 3, 2, EPATHMAP_MAP, 0];
        let mut admission = CanonicalPathTopologyAdmission::new();
        let mut obligation = None;
        for (offset, byte) in bytes.into_iter().enumerate() {
            if let CanonicalPathTransition::ObligationStarted(found) =
                admission.edge(LocatedPathByte::new(byte, offset)).unwrap()
            {
                obligation = Some(found);
            }
        }
        let obligation = obligation.expect("nested EPM suspension is visible to the driver");
        admission
            .resolve_obligation(
                obligation.id,
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Map,
                    entries_stable: true,
                },
            )
            .unwrap();
        assert!(admission
            .value(true, CanonicalPathSource::new(5))
            .unwrap()
            .is_ok());

        let mut wrong_mode = CanonicalPathTopologyAdmission::new();
        wrong_mode
            .edge(LocatedPathByte::new(EPATHMAP_MAP, 10))
            .unwrap();
        let CanonicalPathTransition::ObligationStarted(obligation) =
            wrong_mode.edge(LocatedPathByte::new(0, 11)).unwrap()
        else {
            panic!("zero-length nested EPM starts an obligation")
        };
        wrong_mode
            .resolve_obligation(
                obligation.id,
                CanonicalPathObligationResolution::NestedEpm1 {
                    mode: NestedEpmMode::Empty,
                    entries_stable: true,
                },
            )
            .unwrap();
        assert_eq!(
            wrong_mode
                .value(true, CanonicalPathSource::new(12))
                .unwrap()
                .unwrap_err()
                .kind,
            CanonicalPathAdmissionErrorKind::NestedEpmNotMap
        );

        let mut rejected = CanonicalPathTopologyAdmission::new();
        rejected
            .edge(LocatedPathByte::new(EPATHMAP_MAP, 20))
            .unwrap();
        let CanonicalPathTransition::ObligationStarted(obligation) =
            rejected.edge(LocatedPathByte::new(0, 21)).unwrap()
        else {
            panic!("zero-length nested EPM starts an obligation")
        };
        rejected
            .reject_obligation(obligation.id, CanonicalPathSource::new(22))
            .unwrap();
        // A rejected branch is dead rather than structurally fatal: topology
        // below it may still be traversed when it has no value endpoint.
        rejected.edge(LocatedPathByte::new(GBOOL_TRUE, 23)).unwrap();
        assert!(rejected
            .value(false, CanonicalPathSource::new(24))
            .is_none());

        // Exercise the other non-Map discriminator independently.
        let _ = NestedEpmMode::Set;
    }

    #[test]
    fn deeply_nested_tuple_is_heap_total_on_a_tiny_native_stack() {
        let handle = std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(|| {
                let depth = 20_000;
                let mut admission = CanonicalPathTopologyAdmission::new();
                let mut offset = 0;
                for _ in 0..depth {
                    admission
                        .edge(LocatedPathByte::new(ETUPLE, offset))
                        .unwrap();
                    offset += 1;
                    admission.edge(LocatedPathByte::new(1, offset)).unwrap();
                    offset += 1;
                }
                admission
                    .edge(LocatedPathByte::new(GBOOL_TRUE, offset))
                    .unwrap();
                let result = admission
                    .value(true, CanonicalPathSource::new(offset + 1))
                    .unwrap()
                    .unwrap();
                assert_eq!(result.form, CanonicalTopLevelPathForm::Bare);
                assert!(result.canonical_path_stable);
            })
            .unwrap();
        handle.join().unwrap();
    }
}
