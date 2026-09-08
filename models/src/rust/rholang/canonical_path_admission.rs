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
use crate::rust::rholang::canonical_bytes_arena::{
    CanonicalByteProvenance, CanonicalBytesArena, CanonicalBytesHandle, CanonicalLocatedByte,
    FrozenCanonicalBytes,
};

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
pub(crate) struct CanonicalPathCompletedObligation {
    pub(crate) obligation: CanonicalPathObligation,
    pub(crate) payload: CanonicalBytesHandle,
    /// Exact physical anchor at which the opaque payload became complete.
    ///
    /// Provenance can be discontinuous, so consumers must not synthesize this
    /// by incrementing the final payload byte's offset.
    pub(crate) payload_end: CanonicalPathSource,
}

/// Ownership-independent completed payload for a suspended nested validator.
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PathStateId(usize);

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
    Payload { remaining: u64 },
    AwaitingResolution { payload_end: CanonicalPathSource },
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Frame {
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
        previous: Option<CanonicalBytesHandle>,
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

pub(crate) struct CanonicalPathReducer {
    states: Vec<InnerPath>,
    machine: CanonicalPathMachine,
}

pub(crate) struct CanonicalPathStreamReducer {
    current: InnerPath,
    bytes: CanonicalBytesHandle,
    machine: CanonicalPathMachine,
}

struct CanonicalPathMachine {
    frames: Vec<Frame>,
    next_obligation: u64,
    stats: CanonicalPathAdmissionStats,
}

impl CanonicalPathReducer {
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
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats {
        CanonicalPathAdmissionStats {
            states: self.states.len(),
            frames: self.machine.frames.len(),
            ..self.machine.stats
        }
    }

    pub(crate) fn active_obligation(
        &self,
        handle: CanonicalPathReducerHandle,
    ) -> Option<CanonicalPathObligationProgress> {
        self.machine
            .active_obligation_in(&self.states[handle.state.0])
    }

    pub(crate) fn completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
    ) -> Result<Option<CanonicalPathCompletedObligation>, CanonicalPathAdmissionError> {
        self.machine
            .completed_obligation(arena, &self.states[handle.state.0], handle.bytes)
    }

    /// Freeze a completed opaque payload so a child PDA can own its indexed
    /// input while this reducer and arena continue accepting later trie edges.
    pub(crate) fn freeze_completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
    ) -> Result<Option<FrozenCanonicalPathObligation>, CanonicalPathAdmissionError> {
        self.machine
            .freeze_completed_obligation(arena, &self.states[handle.state.0], handle.bytes)
    }

    pub(crate) fn edge(
        &mut self,
        arena: &mut CanonicalBytesArena,
        handle: CanonicalPathReducerHandle,
        located: CanonicalLocatedByte,
    ) -> Result<(CanonicalPathReducerHandle, CanonicalPathTransition), CanonicalPathAdmissionError>
    {
        let (state, bytes, transition) =
            self.machine
                .advance(arena, &self.states[handle.state.0], handle.bytes, located)?;
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
            arena,
            &self.states[handle.state.0],
            handle.bytes,
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
        self.machine
            .reject_path_obligation(arena, handle.bytes, &mut state, id, source)?;
        let next = PathStateId(self.states.len());
        self.states.push(state);
        Ok(CanonicalPathReducerHandle {
            state: next,
            bytes: handle.bytes,
        })
    }
}

impl CanonicalPathStreamReducer {
    pub(crate) fn new(arena: &CanonicalBytesArena) -> Self {
        Self {
            current: InnerPath::new(true),
            bytes: arena.empty(),
            machine: CanonicalPathMachine::new(),
        }
    }

    #[cfg(test)]
    pub(crate) const fn retained_state_bytes(&self) -> usize { std::mem::size_of::<InnerPath>() }

    #[cfg(test)]
    pub(crate) const fn stats(&self) -> CanonicalPathAdmissionStats {
        CanonicalPathAdmissionStats {
            states: 1,
            frames: self.machine.frames.len(),
            ..self.machine.stats
        }
    }

    pub(crate) fn active_obligation(&self) -> Option<CanonicalPathObligationProgress> {
        self.machine.active_obligation_in(&self.current)
    }

    pub(crate) fn freeze_completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
    ) -> Result<Option<FrozenCanonicalPathObligation>, CanonicalPathAdmissionError> {
        self.machine
            .freeze_completed_obligation(arena, &self.current, self.bytes)
    }

    pub(crate) fn edge(
        &mut self,
        arena: &mut CanonicalBytesArena,
        located: CanonicalLocatedByte,
    ) -> Result<CanonicalPathTransition, CanonicalPathAdmissionError> {
        let (current, bytes, transition) =
            self.machine
                .advance(arena, &self.current, self.bytes, located)?;
        self.current = current;
        self.bytes = bytes;
        Ok(transition)
    }

    pub(crate) fn value(
        &self,
        has_value: bool,
        source: CanonicalPathSource,
    ) -> Option<Result<CanonicalPathSummary, CanonicalPathAdmissionError>> {
        has_value.then(|| self.machine.finish_path(&self.current, source))
    }

    pub(crate) fn resolve_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        self.current =
            self.machine
                .resolved_state(arena, &self.current, self.bytes, id, resolution)?;
        Ok(())
    }
}

impl CanonicalPathMachine {
    fn new() -> Self {
        Self {
            frames: Vec::new(),
            next_obligation: 0,
            stats: CanonicalPathAdmissionStats::default(),
        }
    }

    fn advance(
        &mut self,
        arena: &mut CanonicalBytesArena,
        current: &InnerPath,
        previous_bytes: CanonicalBytesHandle,
        located: CanonicalLocatedByte,
    ) -> Result<
        (InnerPath, CanonicalBytesHandle, CanonicalPathTransition),
        CanonicalPathAdmissionError,
    > {
        let byte = LocatedPathByte {
            byte: located.byte,
            source: CanonicalPathSource::from_provenance(located.provenance),
        };
        if self
            .active_obligation_in(current)
            .is_some_and(|obligation| obligation.awaiting_resolution)
        {
            return Err(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::UnresolvedObligation,
                byte.source,
            ));
        }
        let bytes = arena.append(previous_bytes, located).map_err(|_| {
            CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::SizeOverflow,
                byte.source,
            )
        })?;
        self.stats.edges += 1;
        self.stats.max_topology_depth = self.stats.max_topology_depth.max(bytes.len());
        let mut state = current.clone();
        let transition = self.feed_path(arena, bytes, &mut state, byte);
        Ok((state, bytes, transition))
    }

    fn resolved_state(
        &mut self,
        arena: &CanonicalBytesArena,
        current: &InnerPath,
        bytes: CanonicalBytesHandle,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<InnerPath, CanonicalPathAdmissionError> {
        let mut state = current.clone();
        self.resolve_path_obligation(arena, bytes, &mut state, id, resolution)?;
        Ok(state)
    }

    fn completed_obligation<'a>(
        &'a self,
        arena: &CanonicalBytesArena,
        mut path: &'a InnerPath,
        bytes: CanonicalBytesHandle,
    ) -> Result<Option<CanonicalPathCompletedObligation>, CanonicalPathAdmissionError> {
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
                    let payload =
                        arena
                            .slice(bytes, *payload_start..bytes.len())
                            .map_err(|_| {
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
                } => path = child,
                _ => return Ok(None),
            }
        }
    }

    fn freeze_completed_obligation(
        &self,
        arena: &CanonicalBytesArena,
        current: &InnerPath,
        bytes: CanonicalBytesHandle,
    ) -> Result<Option<FrozenCanonicalPathObligation>, CanonicalPathAdmissionError> {
        Ok(self
            .completed_obligation(arena, current, bytes)?
            .map(|completed| FrozenCanonicalPathObligation {
                obligation: completed.obligation,
                payload: arena.freeze(completed.payload),
                payload_end: completed.payload_end,
            }))
    }

    fn frame(&self, id: FrameId) -> Frame { self.frames[id.0].clone() }

    fn push_frame(&mut self, frame: Frame) -> FrameId {
        let id = FrameId(self.frames.len());
        self.frames.push(frame);
        id
    }

    fn push_varint(&mut self, role: VarintRole, next: Option<FrameId>) -> FrameId {
        self.push_frame(Frame::Varint {
            role,
            state: VarintState::default(),
            next,
        })
    }

    fn feed_path(
        &mut self,
        arena: &CanonicalBytesArena,
        bytes: CanonicalBytesHandle,
        root: &mut InnerPath,
        byte: LocatedPathByte,
    ) -> CanonicalPathTransition {
        struct RegionContext {
            owner: InnerPath,
            remaining: u64,
            previous: Option<CanonicalBytesHandle>,
            declared_len: u64,
            path_remaining: u64,
            start_depth: usize,
            next: Option<FrameId>,
        }

        let mut current = root.clone();
        let mut contexts = Vec::<RegionContext>::new();
        let transition = loop {
            if matches!(current.status, PathStatus::Dead(_)) {
                break CanonicalPathTransition::Continue;
            }
            let Some(top) = current.stack else {
                break self.feed_path_shallow(bytes, &mut current, byte);
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
                break self.feed_path_shallow(bytes, &mut current, byte);
            };

            if remaining == 0 || path_remaining == 0 {
                current.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                    byte.source,
                ));
                break CanonicalPathTransition::Continue;
            }
            contexts.push(RegionContext {
                owner: current,
                remaining: remaining - 1,
                previous,
                declared_len,
                path_remaining: path_remaining - 1,
                start_depth,
                next,
            });
            current = path;
        };

        while let Some(context) = contexts.pop() {
            let mut owner = context.owner;
            if let PathStatus::Dead(error) = current.status {
                owner.kill(error);
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
                current = owner;
                continue;
            }

            // A sibling validator resolves an opaque payload only after this
            // edge returns.  Preserve a zero-remaining region frame so that a
            // last-byte obligation can resume and finalize its framed path.
            if self
                .active_obligation_in(&current)
                .is_some_and(|obligation| obligation.awaiting_resolution)
            {
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
                current = owner;
                continue;
            }

            let endpoint = byte.source;
            self.finish_region_path(
                arena,
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
            current = owner;
        }
        *root = current;
        transition
    }

    fn feed_path_shallow(
        &mut self,
        bytes: CanonicalBytesHandle,
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
        self.feed_frame(bytes, path, byte)
    }

    fn feed_frame(
        &mut self,
        bytes: CanonicalBytesHandle,
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
                    self.finish_varint(path, role, value, next, byte.source, bytes.len())
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
                                    start_depth: bytes.len(),
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
                ObligationStatus::Payload { remaining } => {
                    let remaining = remaining - 1;
                    path.stack = Some(self.push_frame(Frame::Obligation {
                        obligation,
                        status: if remaining == 0 {
                            ObligationStatus::AwaitingResolution {
                                payload_end: byte.source,
                            }
                        } else {
                            ObligationStatus::Payload { remaining }
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
                        ObligationStatus::Payload { remaining: value }
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
        if let PathStatus::Dead(error) = path.status {
            return Err(error);
        }
        if path.stack.is_some() {
            return Err(CanonicalPathAdmissionError::new(
                if self
                    .active_obligation_in(path)
                    .is_some_and(|o| o.awaiting_resolution)
                {
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

    fn active_obligation_in(&self, root: &InnerPath) -> Option<CanonicalPathObligationProgress> {
        let mut path = root;
        loop {
            if matches!(path.status, PathStatus::Dead(_)) {
                return None;
            }
            let top = path.stack?;
            match &self.frames[top.0] {
                Frame::Obligation {
                    obligation, status, ..
                } => {
                    return Some(CanonicalPathObligationProgress {
                        obligation: *obligation,
                        remaining: match status {
                            ObligationStatus::Payload { remaining } => *remaining,
                            ObligationStatus::AwaitingResolution { .. } => 0,
                        },
                        awaiting_resolution: matches!(
                            status,
                            ObligationStatus::AwaitingResolution { .. }
                        ),
                    });
                }
                Frame::Region {
                    phase: RegionPhase::Path { path: child, .. },
                    ..
                } => path = child,
                _ => return None,
            }
        }
    }

    fn resolve_path_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        bytes: CanonicalBytesHandle,
        root: &mut InnerPath,
        id: CanonicalPathObligationId,
        resolution: CanonicalPathObligationResolution,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let mut current = root.clone();
        let mut owners = Vec::<(InnerPath, Frame)>::new();
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
                    owners.push((current, frame));
                    current = child;
                }
                Frame::Obligation {
                    obligation,
                    status: ObligationStatus::AwaitingResolution { .. },
                    next,
                    ..
                } if obligation.id == id => {
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
                        self.bubble(&mut current, stable);
                    }
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
                        CanonicalPathSource::new(0),
                    ));
                }
            }
        }
        self.rebuild_region_owners(arena, bytes, &mut current, owners);
        *root = current;
        Ok(())
    }

    #[cfg(test)]
    fn reject_path_obligation(
        &mut self,
        arena: &CanonicalBytesArena,
        bytes: CanonicalBytesHandle,
        root: &mut InnerPath,
        id: CanonicalPathObligationId,
        source: CanonicalPathSource,
    ) -> Result<(), CanonicalPathAdmissionError> {
        let mut current = root.clone();
        let mut owners = Vec::<(InnerPath, Frame)>::new();
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
                    owners.push((current, frame));
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
        self.rebuild_region_owners(arena, bytes, &mut current, owners);
        *root = current;
        Ok(())
    }

    fn rebuild_region_owners(
        &mut self,
        arena: &CanonicalBytesArena,
        bytes: CanonicalBytesHandle,
        current: &mut InnerPath,
        mut owners: Vec<(InnerPath, Frame)>,
    ) {
        while let Some((mut owner, frame)) = owners.pop() {
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
                    arena
                        .provenance_at(bytes, bytes.len() - 1)
                        .expect("a suspended region path consumed a final edge"),
                );
                self.finish_region_path(
                    arena,
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
            *current = owner;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_region_path(
        &mut self,
        arena: &CanonicalBytesArena,
        bytes: CanonicalBytesHandle,
        owner: &mut InnerPath,
        child: &InnerPath,
        remaining: u64,
        declared_len: u64,
        start_depth: usize,
        previous: Option<CanonicalBytesHandle>,
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
        let completed = match arena.slice(bytes, start_depth..bytes.len()) {
            Ok(completed) => completed,
            Err(_) => {
                owner.kill(CanonicalPathAdmissionError::new(
                    CanonicalPathAdmissionErrorKind::SizeOverflow,
                    endpoint,
                ));
                return;
            }
        };
        if u64::try_from(completed.len()).ok() != Some(declared_len) {
            owner.kill(CanonicalPathAdmissionError::new(
                CanonicalPathAdmissionErrorKind::RegionFrameOverrun,
                endpoint,
            ));
            return;
        }
        if let Some(previous) = previous {
            let order = arena.compare(previous, completed);
            if order != std::cmp::Ordering::Less {
                let mut source = endpoint;
                for (left, right) in arena.bytes(previous).zip(arena.located_bytes(completed)) {
                    if left != right.byte {
                        source = CanonicalPathSource::from_provenance(right.provenance);
                        break;
                    }
                }
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

pub(crate) struct CanonicalPathTopologyAdmission {
    reducer: CanonicalPathReducer,
    arena: CanonicalBytesArena,
    current: CanonicalPathReducerHandle,
    restore: Vec<CanonicalPathReducerHandle>,
    root_left: bool,
}

impl Default for CanonicalPathTopologyAdmission {
    fn default() -> Self { Self::new() }
}

impl CanonicalPathTopologyAdmission {
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
