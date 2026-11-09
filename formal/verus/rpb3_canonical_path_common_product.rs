//! CanonicalPath's byte-loop macro over the live admission product.
//!
//! The parser continuation, physical byte anchors, and cursor counters are
//! retained across driver calls. Each loop event contributes only scalar
//! facts observable at `CanonicalPathFrame::step`: yielded index/byte/source,
//! checked opaque skip width, and resulting cursor counts. The selector reads
//! all parsed and skipped bytes from the retained indexed input and computes
//! its own next parser state. Proving that arbitrary successful Rust loop
//! iterations yield these observations is a separate source-extraction task.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_driver_source_refinement as path_frame,
    rpb3_canonical_path_root_source_refinement as root_source,
    rpb3_canonical_path_terminal_driver_observation as terminal_driver,
    rpb3_epathmap_event_refinement as epm, rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_terminal_refinement as terminal, rpb3_map_frame_refinement as map,
    rpb3_path_parser_refinement as parser, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as bridge, rpb3_wire_cursor_source_refinement as wire,
};

verus! {

pub struct CanonicalParserSlot {
    pub parser: parser::ParserImage,
    pub ghost bytes: Seq<u8>,
    pub ghost positions: Seq<parser::ParserSource>,
    pub cursor_traversed: usize,
}

pub struct LiveCanonicalPathCommonState {
    pub ghost live: path_live::LiveCanonicalPathState,
    pub ghost parser_frames: Seq<Option<CanonicalParserSlot>>,
    pub ghost selected_values: Seq<Option<parser::ParserGate>>,
}

pub open spec fn parser_slot_matches(
    state: LiveCanonicalPathCommonState,
    index: int,
) -> bool {
    match (state.live.path_frames[index], state.parser_frames[index]) {
        (None, None) => true,
        (Some(frame), Some(slot)) => {
            &&& frame.cursor as int <= slot.bytes.len()
            &&& slot.bytes.len() <= frame.input.bytes.len()
            &&& slot.parser.stamp as int == slot.bytes.len()
            &&& slot.positions.len() == slot.bytes.len()
            &&& slot.cursor_traversed as int <= slot.bytes.len()
            &&& forall|at: int| 0 <= at < slot.bytes.len() ==> {
                #[trigger] slot.bytes[at] == frame.input.bytes[at]
            }
            &&& forall|at: int| 0 <= at < slot.positions.len() ==> {
                &&& slot.positions[at].source == frame.input.positions[at].source
                &&& slot.positions[at].offset == frame.input.positions[at].offset
            }
        },
        _ => false,
    }
}

pub open spec fn live_canonical_path_common_at_driver_boundary(
    state: LiveCanonicalPathCommonState,
) -> bool {
    &&& live_canonical_path_common_wf(state)
    &&& forall|index: int| 0 <= index < state.parser_frames.len()
        ==> match (#[trigger] state.parser_frames[index], state.live.path_frames[index]) {
            (Some(slot), Some(frame)) => frame.cursor as int == slot.bytes.len(),
            _ => true,
        }
}

pub open spec fn live_canonical_path_common_wf(
    state: LiveCanonicalPathCommonState,
) -> bool {
    &&& path_live::live_canonical_path_wf(state.live)
    &&& state.parser_frames.len() == state.live.path_frames.len()
    &&& state.selected_values.len() == state.parser_frames.len()
    &&& forall|index: int| 0 <= index < state.parser_frames.len()
        ==> parser_slot_matches(state, index)
    &&& forall|index: int| 0 <= index < state.selected_values.len()
        ==> match #[trigger] state.selected_values[index] {
            None => true,
            Some(gate) => match (state.live.path_frames[index], state.parser_frames[index]) {
                (Some(frame), Some(slot)) => {
                    &&& slot.bytes.len() == frame.input.bytes.len()
                    &&& slot.parser.owners.len() == 0
                    &&& parser::parser_valid_finished_path(slot.parser.focus)
                    &&& gate == parser::parser_root_gate(slot.parser)
                },
                _ => false,
            },
        }
}

pub open spec fn canonical_path_source_boundary_matches(
    state: LiveCanonicalPathCommonState,
    index: int,
    observed: parser::ParserSourceSnapshot,
) -> bool {
    // The shape is shared with the topology parser source image, but a
    // CanonicalPathFrame owns a stream reducer. Callers must supply the
    // stream reducer's actual read-only projection here. The equivalence of
    // every successful Rust stream step with this modeled state is still an
    // explicit source-refinement obligation, not established by this test.
    if !live_canonical_path_common_wf(state)
        || index < 0 || index >= state.parser_frames.len()
        || state.parser_frames[index].is_none()
    { false } else {
        let slot = state.parser_frames[index].unwrap();
        &&& parser::source_parser_matches(observed, slot.parser)
        &&& observed.path_bytes.len() == slot.bytes.len()
        &&& forall|at: int| 0 <= at < observed.path_bytes.len() ==> {
            &&& observed.path_bytes[at].byte == slot.bytes[at]
            &&& observed.path_bytes[at].source == slot.positions[at]
        }
    }
}

pub open spec fn live_canonical_path_common_root_from_source(
    source: admission::AdmissionCanonicalPathRootSource,
) -> Option<LiveCanonicalPathCommonState> {
    match path_live::live_canonical_path_root_from_source(source) {
        None => None,
        Some(live) => Some(LiveCanonicalPathCommonState {
            live,
            parser_frames: seq![Some(CanonicalParserSlot {
                parser: parser::initial_parser(),
                bytes: Seq::empty(),
                positions: Seq::empty(),
                cursor_traversed: 0,
            })],
            selected_values: seq![None],
        }),
    }
}

pub proof fn live_canonical_path_common_root_is_wf(
    source: admission::AdmissionCanonicalPathRootSource,
)
    requires admission::admission_canonical_path_root_source_wf(source),
    ensures match live_canonical_path_common_root_from_source(source) {
        Some(root) => {
            &&& live_canonical_path_common_at_driver_boundary(root)
            &&& root.parser_frames.len() == 1
            &&& root.selected_values == seq![None]
            &&& root.parser_frames[0] == Some(CanonicalParserSlot {
                parser: parser::initial_parser(),
                bytes: Seq::empty(),
                positions: Seq::empty(),
                cursor_traversed: 0,
            })
            &&& root.live.base.source.product.bridge.admission.frames[0].frame_kind == 4
        },
        None => false,
    },
{
    path_live::live_canonical_path_root_is_wf(source);
    let root = live_canonical_path_common_root_from_source(source).unwrap();
    assert(root.live.path_frames.len() == 1);
    assert(root.live.path_frames[0].is_some());
    assert(root.live.path_frames[0].unwrap().cursor == 0);
    assert(root.parser_frames.len() == 1);
    assert(root.parser_frames[0].is_some());
    assert(root.parser_frames[0].unwrap().bytes.len() == 0);
    assert(root.parser_frames[0].unwrap().positions.len() == 0);
    assert(root.parser_frames[0].unwrap().parser.stamp == 0);
    assert(root.parser_frames[0].unwrap().cursor_traversed == 0);
    assert(root.live.path_frames[0].unwrap().input.bytes.len() >= 0);
    assert(parser_slot_matches(root, 0));
    assert forall|index: int| 0 <= index < root.parser_frames.len()
        implies parser_slot_matches(root, index) by {
        assert(index == 0);
    }
    assert forall|index: int| 0 <= index < root.parser_frames.len()
        implies match (#[trigger] root.parser_frames[index], root.live.path_frames[index]) {
            (Some(slot), Some(frame)) => frame.cursor as int == slot.bytes.len(),
            _ => true,
        } by {
        assert(index == 0);
    }
}

pub open spec fn canonical_path_internal_edge_next(
    before: LiveCanonicalPathCommonState,
    index: int,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_wf(before)
        || index < 0 || index >= before.parser_frames.len()
        || before.parser_frames[index].is_none()
        || before.live.path_frames[index].is_none()
        || before.live.outstanding[index].is_some()
        || before.selected_values[index].is_some()
    { None } else {
        let frame = before.live.path_frames[index].unwrap();
        let slot = before.parser_frames[index].unwrap();
        let at: int = slot.bytes.len() as int;
        if at >= frame.input.bytes.len()
            || at >= usize::MAX as int
            || parser::parser_waiting_obligation(slot.parser.focus)
        { None } else {
            let anchor = frame.input.positions[at];
            let source = parser::ParserSource {
                source: anchor.source,
                offset: anchor.offset,
            };
            let byte = frame.input.bytes[at];
            let step = parser::feed_parser_byte(
                slot.parser, byte, source, (at + 1) as usize,
            );
            if step.event is NoSuccessfulEdge
                || parser::parser_root_gate(step.image).dead
                || step.image.stamp as int != at + 1
            { None } else {
                Some(LiveCanonicalPathCommonState {
                    parser_frames: before.parser_frames.update(index, Some(CanonicalParserSlot {
                        parser: step.image,
                        bytes: slot.bytes.push(byte),
                        positions: slot.positions.push(source),
                        cursor_traversed: (slot.cursor_traversed + 1) as usize,
                    })),
                    ..before
                })
            }
        }
    }
}

pub open spec fn canonical_path_internal_value_next(
    before: LiveCanonicalPathCommonState,
    index: int,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_wf(before)
        || index < 0 || index >= before.parser_frames.len()
        || before.parser_frames[index].is_none()
        || before.live.path_frames[index].is_none()
        || before.live.outstanding[index].is_some()
        || before.selected_values[index].is_some()
    { None } else {
        let frame = before.live.path_frames[index].unwrap();
        let slot = before.parser_frames[index].unwrap();
        if slot.bytes.len() != frame.input.bytes.len()
            || slot.parser.owners.len() != 0
            || !parser::parser_valid_finished_path(slot.parser.focus)
        { None } else {
            Some(LiveCanonicalPathCommonState {
                selected_values: before.selected_values.update(
                    index, Some(parser::parser_root_gate(slot.parser)),
                ),
                ..before
            })
        }
    }
}

pub open spec fn parser_obligation_matches_push(
    slot: CanonicalParserSlot,
    source: admission::AdmissionCanonicalPathPushSource,
) -> bool {
    if slot.parser.focus.stack.len() == 0 { false } else {
        match slot.parser.focus.stack.last() {
            parser::ParserFrame::Obligation {
                id,
                kind,
                payload_len,
                payload_start,
                status: parser::ParserObligationStatus::AwaitingResolution { payload_end },
                ..
            } => {
                &&& id == source.reducer_obligation_id
                &&& payload_start == source.payload_start
                &&& payload_start as int + payload_len as int == source.payload_end as int
                &&& payload_end.source == source.terminal_anchor.source
                &&& payload_end.offset == source.terminal_anchor.offset
                &&& match (kind, source.kind) {
                    (parser::ParserObligationKind::RawParEscape,
                        admission::AdmissionCanonicalPathObligationKind::RawParEscape) => true,
                    (parser::ParserObligationKind::NestedEpm1,
                        admission::AdmissionCanonicalPathObligationKind::NestedEpm1) => true,
                    _ => false,
                }
            },
            _ => false,
        }
    }
}

pub open spec fn canonical_path_common_push_from_source(
    before: LiveCanonicalPathCommonState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_wf(before)
        || before.parser_frames.len() == 0
        || before.parser_frames.last().is_none()
        || before.selected_values.last().is_some()
        || before.parser_frames.last().unwrap().bytes.len()
            != source.cursor_after_step as int
        || !parser_obligation_matches_push(before.parser_frames.last().unwrap(), source)
    { None } else {
        match path_live::live_canonical_path_push_from_source(
            before.live, source, child, storage, child_image, child_reader,
        ) {
            None => None,
            Some(live) => Some(LiveCanonicalPathCommonState {
                live,
                parser_frames: before.parser_frames.push(None),
                selected_values: before.selected_values.push(None),
            }),
        }
    }
}

pub open spec fn parser_resolution_from_return(
    observed: path_live::LiveCanonicalPathReturnObservation,
) -> Option<parser::ParserResolution> {
    match (observed.reducer_waiting_kind, observed.summary) {
        (
            admission::AdmissionCanonicalPathObligationKind::RawParEscape,
            admission::AdmissionReturnedKind::Par { canonical_path_stable, .. },
        ) => Some(parser::ParserResolution::RawPar { canonical_path_stable }),
        (
            admission::AdmissionCanonicalPathObligationKind::NestedEpm1,
            admission::AdmissionReturnedKind::EPathMapSnapshot {
                mode, entries_canonical_path_stable,
            },
        ) => Some(parser::ParserResolution::NestedEpm1 {
            is_map: mode == 2,
            entries_stable: entries_canonical_path_stable,
        }),
        _ => None,
    }
}

pub open spec fn canonical_path_common_return_from_source(
    before: LiveCanonicalPathCommonState,
    observed: path_live::LiveCanonicalPathReturnObservation,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_at_driver_boundary(before)
        || before.parser_frames.len() < 2
        || before.parser_frames[before.parser_frames.len() - 2].is_none()
        || before.selected_values[before.selected_values.len() - 2].is_some()
        || before.live.outstanding[before.live.outstanding.len() - 2].is_none()
        || !parser_obligation_matches_push(
            before.parser_frames[before.parser_frames.len() - 2].unwrap(),
            before.live.outstanding[before.live.outstanding.len() - 2].unwrap(),
        )
        || parser_resolution_from_return(observed).is_none()
    { None } else {
        let parent_index = before.parser_frames.len() - 2;
        let parent = before.parser_frames[parent_index].unwrap();
        let next_parser = parser::resolve_parser_obligation(
            parent.parser, observed.reducer_waiting_id,
            parser_resolution_from_return(observed).unwrap(),
        );
        if parser::parser_root_gate(next_parser).dead { None } else {
            match path_live::live_canonical_path_return_from_source(before.live, observed) {
                None => None,
                Some(result) => Some(LiveCanonicalPathCommonState {
                    live: result.state,
                    parser_frames: before.parser_frames.drop_last().update(
                        parent_index,
                        Some(CanonicalParserSlot { parser: next_parser, ..parent }),
                    ),
                    selected_values: before.selected_values.drop_last(),
                }),
            }
        }
    }
}

pub enum CanonicalPathInternalAction {
    Edge { index: int },
    Value { index: int },
}

pub open spec fn canonical_path_internal_step(
    before: LiveCanonicalPathCommonState,
    after: LiveCanonicalPathCommonState,
    action: CanonicalPathInternalAction,
) -> bool {
    match action {
        CanonicalPathInternalAction::Edge { index } =>
            canonical_path_internal_edge_next(before, index) == Some(after),
        CanonicalPathInternalAction::Value { index } =>
            canonical_path_internal_value_next(before, index) == Some(after),
    }
}

pub open spec fn canonical_path_internal_trace(
    states: Seq<LiveCanonicalPathCommonState>,
    actions: Seq<CanonicalPathInternalAction>,
) -> bool {
    &&& states.len() == actions.len() + 1
    &&& live_canonical_path_common_at_driver_boundary(states[0])
    &&& forall|at: int| 0 <= at < actions.len()
        ==> canonical_path_internal_step(states[at], states[at + 1], actions[at])
}

/// A driver call scans the top CanonicalPath frame without returning to the
/// admission loop. Every scanned state is selected by the previous state's
/// actual indexed byte and physical provenance, rather than supplied as a
/// free poststate. The finite action sequence represents loop iterations;
/// the separate source relation below must still bind it to the Rust loop.
pub open spec fn canonical_path_driver_scan_trace(
    before: LiveCanonicalPathCommonState,
    states: Seq<LiveCanonicalPathCommonState>,
    actions: Seq<CanonicalPathInternalAction>,
) -> bool {
    &&& canonical_path_internal_trace(states, actions)
    &&& states[0] == before
    &&& before.live.path_frames.len() > 0
    &&& forall|at: int| 0 <= at < actions.len() ==> match #[trigger] actions[at] {
        CanonicalPathInternalAction::Edge { index }
        | CanonicalPathInternalAction::Value { index } =>
            index == before.live.path_frames.len() - 1,
    }
}

/// The executable model of one call's internal scan. Its only input besides
/// the retained prestate is the ordered Edge/Value control sequence; every
/// successor is computed from the current frame's indexed source.
pub open spec fn canonical_path_driver_scan_fold(
    before: LiveCanonicalPathCommonState,
    actions: Seq<CanonicalPathInternalAction>,
) -> Option<LiveCanonicalPathCommonState>
    decreases actions.len(),
{
    if actions.len() == 0 {
        Some(before)
    } else {
        let head = actions[0];
        let next = if before.live.path_frames.len() == 0 { None } else { match head {
            CanonicalPathInternalAction::Edge { index } =>
                if index == before.live.path_frames.len() - 1 {
                    canonical_path_internal_edge_next(before, index)
                } else { None },
            CanonicalPathInternalAction::Value { index } =>
                if index == before.live.path_frames.len() - 1 {
                    canonical_path_internal_value_next(before, index)
                } else { None },
        } };
        match next {
            None => None,
            Some(after) => canonical_path_driver_scan_fold(
                after, actions.subrange(1, actions.len() as int),
            ),
        }
    }
}

/// Scalar fields recorded at one successful iteration of the production
/// CanonicalPathFrame::step loop. `skipped` is the width passed to the checked
/// `try_advance_opaque_interior` and `cursor.skip_exact` pair, if any.
pub struct CanonicalPathLoopSourceEvent {
    pub step_index: usize,
    pub index: usize,
    pub byte: u8,
    pub source: parser::ParserSource,
    pub skipped: usize,
    pub yielded_after: usize,
    pub traversed_after: usize,
}

/// With no active Region owner, the production planner chooses the largest
/// interior width strictly before the final payload byte. The indexed input
/// can impose the other bound. A zero result means no plan is emitted.
pub open spec fn canonical_path_root_opaque_skip_exact(
    image: parser::ParserImage,
    available: int,
    skipped: usize,
) -> bool {
    if available > 1 && parser::parser_forward_capacity(image, 1) {
        skipped > 0
        && (skipped as int) < available
        && parser::parser_forward_capacity(image, skipped as int)
        && (skipped as int + 1 == available
            || !parser::parser_forward_capacity(image, skipped as int + 1))
    } else {
        skipped == 0
    }
}

/// Select one modeled loop iteration from the retained indexed input and the
/// event's scalar cursor facts. The byte and provenance are required to equal
/// the indexed source. A bulk skip is admitted only in an opaque obligation
/// interior with enough parser, Region, cursor, and usize capacity; its bytes
/// and anchors are sliced from that same indexed source.
pub open spec fn canonical_path_loop_event_next(
    before: LiveCanonicalPathCommonState,
    observed: CanonicalPathLoopSourceEvent,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_wf(before)
        || before.parser_frames.len() == 0
        || before.parser_frames.last().is_none()
        || before.live.path_frames.last().is_none()
    { None } else {
        let top = before.parser_frames.len() - 1;
        let slot = before.parser_frames[top].unwrap();
        let frame = before.live.path_frames[top].unwrap();
        let at: int = slot.bytes.len() as int;
        if observed.index as int != at
            || at >= frame.input.bytes.len()
            || observed.byte != frame.input.bytes[at]
            || observed.source != (parser::ParserSource {
                source: frame.input.positions[at].source,
                offset: frame.input.positions[at].offset,
            })
            || observed.yielded_after as int != at + 1 + observed.skipped as int
            || observed.yielded_after as int > frame.input.bytes.len()
            || observed.traversed_after > observed.yielded_after
        { None } else {
            match canonical_path_internal_edge_next(before, top) {
                None => None,
                Some(edge) => {
                    let selected = edge.parser_frames[top].unwrap();
                    let step = parser::feed_parser_byte(
                        slot.parser, observed.byte, observed.source,
                        (at + 1) as usize,
                    );
                    let available = frame.input.bytes.len() - selected.bytes.len();
                    if (step.event is ObligationStarted)
                        && selected.parser.owners.len() == 0
                        && !canonical_path_root_opaque_skip_exact(
                            selected.parser, available, observed.skipped,
                        )
                    { None } else if observed.skipped == 0 {
                        if observed.traversed_after != selected.cursor_traversed {
                            None
                        } else { Some(edge) }
                    } else {
                        let skip = observed.skipped as int;
                        let end = selected.bytes.len() + skip;
                        if !(step.event is ObligationStarted)
                            || !parser::parser_forward_capacity(selected.parser, skip)
                            || observed.traversed_after < selected.cursor_traversed
                            || observed.traversed_after as int
                                > selected.cursor_traversed as int + skip
                        { None } else {
                            let bytes = frame.input.bytes.subrange(
                                selected.bytes.len() as int, end,
                            );
                            let positions = Seq::new(observed.skipped as nat, |i: int| {
                                let anchor = frame.input.positions[
                                    selected.positions.len() + i
                                ];
                                parser::ParserSource {
                                    source: anchor.source,
                                    offset: anchor.offset,
                                }
                            });
                            Some(LiveCanonicalPathCommonState {
                                parser_frames: edge.parser_frames.update(top,
                                    Some(CanonicalParserSlot {
                                        parser: parser::forward_parser_payload(
                                            selected.parser, bytes,
                                        ),
                                        bytes: selected.bytes + bytes,
                                        positions: selected.positions + positions,
                                        cursor_traversed: observed.traversed_after,
                                    }),
                                ),
                                ..edge
                            })
                        }
                    }
                },
            }
        }
    }
}

/// The source event stream, in production iteration order, has no modeled
/// afterstates. This fold rejects an extra event after a completed obligation;
/// that is the point where CanonicalPathFrame::step returns Push.
pub open spec fn canonical_path_loop_source_fold(
    before: LiveCanonicalPathCommonState,
    events: Seq<CanonicalPathLoopSourceEvent>,
) -> Option<LiveCanonicalPathCommonState>
    decreases events.len(),
{
    if events.len() == 0 {
        Some(before)
    } else if before.parser_frames.len() == 0
        || before.parser_frames.last().is_none()
        || parser::parser_waiting_obligation(before.parser_frames.last().unwrap().parser.focus)
        || (events.len() > 1 && events[1].step_index != events[0].step_index)
    { None } else {
        match canonical_path_loop_event_next(before, events[0]) {
            None => None,
            Some(after) => {
                let tail = events.subrange(1, events.len() as int);
                if tail.len() > 0
                    && parser::parser_waiting_obligation(
                        after.parser_frames.last().unwrap().parser.focus,
                    )
                { None } else {
                    canonical_path_loop_source_fold(after, tail)
                }
            },
        }
    }
}

/// A source-observed Push is selected only after a finite scan from the
/// retained prestate. No scanned afterstate is supplied by the caller.
pub open spec fn canonical_path_common_push_macro_from_source(
    before: LiveCanonicalPathCommonState,
    scan_actions: Seq<CanonicalPathInternalAction>,
    source_snapshot: parser::ParserSourceSnapshot,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_at_driver_boundary(before) {
        None
    } else {
        match canonical_path_driver_scan_fold(before, scan_actions) {
            None => None,
            Some(scanned) => {
                let top = before.live.path_frames.len() - 1;
                if scanned.live != before.live
                    || !canonical_path_source_boundary_matches(
                        scanned, top as int, source_snapshot,
                    )
                { None } else {
                    canonical_path_common_push_from_source(
                        scanned, source, child, storage, child_image, child_reader,
                    )
                }
            },
        }
    }
}

/// Source-event Push macro. The event sequence is the loop's scalar trace,
/// with yielded byte/provenance, checked skip width, and cursor counters;
/// each modeled parser state is selected from the retained indexed input.
pub open spec fn canonical_path_common_push_macro_from_loop_source(
    before: LiveCanonicalPathCommonState,
    driver_step_index: usize,
    source_before: parser::ParserSourceSnapshot,
    loop_events: Seq<CanonicalPathLoopSourceEvent>,
    source_snapshot: parser::ParserSourceSnapshot,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_at_driver_boundary(before)
        || before.live.path_frames.len() == 0
        || loop_events.len() == 0
        || loop_events[0].step_index != driver_step_index
        || !canonical_path_source_boundary_matches(
            before, before.live.path_frames.len() - 1, source_before,
        ) {
        None
    } else {
        match canonical_path_loop_source_fold(before, loop_events) {
            None => None,
            Some(scanned) => {
                let top = before.live.path_frames.len() - 1;
                if scanned.live != before.live
                    || !canonical_path_source_boundary_matches(
                        scanned, top as int, source_snapshot,
                    )
                { None } else {
                    canonical_path_common_push_from_source(
                        scanned, source, child, storage, child_image, child_reader,
                    )
                }
            },
        }
    }
}

/// Commit the already-scanned EOF cursor to the live admission view. This is
/// a deterministic projection of the parser sidecar and indexed input. It is
/// needed because CanonicalPathFrame::step scans the entire tail internally;
/// there is no sequence of driver Continue actions to advance a root cursor.
pub open spec fn canonical_path_commit_selected_root(
    before: LiveCanonicalPathCommonState,
) -> Option<LiveCanonicalPathCommonState> {
    if !live_canonical_path_common_wf(before)
        || before.live.path_frames.len() != 1
        || before.live.path_frames[0].is_none()
        || before.parser_frames[0].is_none()
        || before.selected_values[0].is_none()
        || before.live.outstanding[0].is_some()
    { None } else {
        let old_path = before.live.path_frames[0].unwrap();
        let scanned = before.parser_frames[0].unwrap();
        let cursor = scanned.bytes.len();
        if cursor != old_path.input.bytes.len()
            || cursor > usize::MAX as int
            || cursor < old_path.cursor as int
        { None } else {
            let old_product = before.live.base.source.product;
            let old_admission = old_product.bridge.admission;
            let next_frame = admission::AdmissionFrameState {
                cursor: cursor as usize,
                ..old_admission.frames[0]
            };
            let next_admission = admission::AdmissionState {
                frames: seq![next_frame],
                ..old_admission
            };
            let next_bridge = bridge::ScoreAdmissionBridge {
                admission: next_admission,
                ..old_product.bridge
            };
            let next_product = crate::ProductScoreState {
                bridge: next_bridge,
                map: map::IntegratedAdmissionState {
                    admission: next_admission,
                    ..old_product.map
                },
                ..old_product
            };
            let next_runtime = runtime::RuntimeFrameImage {
                score: next_bridge,
                ..before.live.base.source.runtime
            };
            let next_base = crate::LiveRuntimeProductState {
                source: crate::LiveSourceProductState {
                    product: next_product,
                    runtime: next_runtime,
                    ..before.live.base.source
                },
                ..before.live.base
            };
            Some(LiveCanonicalPathCommonState {
                live: path_live::LiveCanonicalPathState {
                    base: next_base,
                    path_frames: seq![Some(path_frame::CanonicalPathFrameImage {
                        cursor: cursor as usize,
                        ..old_path
                    })],
                    ..before.live
                },
                ..before
            })
        }
    }
}

/// Production finish_path returns Split for a terminated multi-segment path,
/// and Bare for one non-list segment. The selected gate already establishes
/// that these are the only successful EOF forms.
pub open spec fn canonical_path_selected_summary(
    gate: parser::ParserGate,
) -> root_source::CanonicalPathSummaryImage {
    root_source::CanonicalPathSummaryImage {
        form: if gate.split_complete {
            root_source::CanonicalPathFormImage::Split
        } else {
            root_source::CanonicalPathFormImage::Bare
        },
        canonical_path_stable: gate.stable,
    }
}

/// The observed reducer result is checked against the selected parser value.
/// The snapshot match is a Rust-to-model boundary premise; this selector does
/// not claim that arbitrary production executions satisfy it yet.
pub open spec fn canonical_path_common_root_complete_from_source(
    before: LiveCanonicalPathCommonState,
    source_snapshot: parser::ParserSourceSnapshot,
    reducer: root_source::CanonicalPathReducerEofObservation,
) -> Option<terminal::LiveCanonicalPathTerminalOutcome> {
    if !canonical_path_source_boundary_matches(before, 0, source_snapshot)
        || before.selected_values[0].is_none()
        || reducer.value_result != Some(canonical_path_selected_summary(
            before.selected_values[0].unwrap(),
        ))
    { None } else {
        match canonical_path_commit_selected_root(before) {
            None => None,
            Some(committed) => terminal::live_canonical_path_terminal_complete_from_source(
                committed.live, reducer,
            ),
        }
    }
}

/// Complete is likewise selected from the retained prestate, a finite
/// deterministic internal scan, the parser source snapshot, and the actual
/// EOF reducer result. The Rust-to-model control-sequence extraction remains
/// an explicit proof obligation.
pub open spec fn canonical_path_common_root_complete_macro_from_source(
    before: LiveCanonicalPathCommonState,
    scan_actions: Seq<CanonicalPathInternalAction>,
    source_snapshot: parser::ParserSourceSnapshot,
    reducer: root_source::CanonicalPathReducerEofObservation,
) -> Option<terminal::LiveCanonicalPathTerminalOutcome> {
    if !live_canonical_path_common_at_driver_boundary(before) {
        None
    } else {
        match canonical_path_driver_scan_fold(before, scan_actions) {
            None => None,
            Some(scanned) => {
                if scanned.live != before.live {
                    None
                } else {
                    canonical_path_common_root_complete_from_source(
                        scanned, source_snapshot, reducer,
                    )
                }
            },
        }
    }
}

/// The source-observation form checks the actual EOF call and both ordered
/// checked stats additions before selecting a Complete. The reducer summary
/// is read from the observed return and compared to the parser-derived value.
pub open spec fn canonical_path_common_root_complete_macro_from_driver_observation(
    before: LiveCanonicalPathCommonState,
    scan_actions: Seq<CanonicalPathInternalAction>,
    source_snapshot: parser::ParserSourceSnapshot,
    observed: terminal_driver::CanonicalPathTerminalDriverObservation,
) -> Option<terminal::LiveCanonicalPathTerminalOutcome> {
    if before.live.path_frames.len() != 1
        || observed.stack_len_before != 1
        || observed.top_kind != 4
        || !observed.cursor_exhausted
        || !observed.waiting_absent
        || !observed.waiting_child_absent
        || observed.active_obligation
        || !observed.call_has_value
        || observed.stats_event != terminal_driver::canonical_path_terminal_checked_stats_event(
            observed,
        )
        || !match observed.stats_event {
            terminal_driver::CanonicalPathTerminalStatsEvent::Success { .. } => true,
            _ => false,
        }
        || observed.cursor_bytes_traversed > observed.cursor_bytes_yielded
        || observed.score_entries_before as int
            != before.live.base.source.product.bridge.transcript.entries.len()
        || observed.score_entries_after != observed.score_entries_before
    { None } else {
        match observed.value_event {
            terminal_driver::CanonicalPathValueCallEvent::ReturnedSummary { summary } => {
                let reducer = root_source::CanonicalPathReducerEofObservation {
                    has_value: observed.call_has_value,
                    at: observed.call_at,
                    active_obligation: observed.active_obligation,
                    value_result: Some(summary),
                };
                match canonical_path_driver_scan_fold(before, scan_actions) {
                    None => None,
                    Some(scanned) => {
                        if scanned.live.path_frames.len() != 1
                            || scanned.live.path_frames[0].is_none()
                            || scanned.parser_frames.len() != 1
                            || scanned.parser_frames[0].is_none()
                            || observed.cursor_bytes_yielded as int
                                != scanned.parser_frames[0].unwrap().bytes.len()
                            || observed.call_at != scanned.live.path_frames[0].unwrap().input.end
                        { None } else {
                            canonical_path_common_root_complete_macro_from_source(
                                before, scan_actions, source_snapshot, reducer,
                            )
                        }
                    },
                }
            },
            _ => None,
        }
    }
}

/// Source-event root Complete macro. EOF Value is computed after the loop
/// events from the retained parser, and the production summary and ordered
/// checked stats results must equal the computed value/counters.
pub open spec fn canonical_path_common_root_complete_macro_from_loop_source(
    before: LiveCanonicalPathCommonState,
    driver_step_index: usize,
    source_before: parser::ParserSourceSnapshot,
    loop_events: Seq<CanonicalPathLoopSourceEvent>,
    source_snapshot: parser::ParserSourceSnapshot,
    observed: terminal_driver::CanonicalPathTerminalDriverObservation,
) -> Option<terminal::LiveCanonicalPathTerminalOutcome> {
    if !live_canonical_path_common_at_driver_boundary(before)
        || before.live.path_frames.len() != 1
        || !canonical_path_source_boundary_matches(before, 0, source_before)
        || (loop_events.len() > 0 && loop_events[0].step_index != driver_step_index)
        || observed.stack_len_before != 1
        || observed.top_kind != 4
        || !observed.cursor_exhausted
        || !observed.waiting_absent
        || !observed.waiting_child_absent
        || observed.active_obligation
        || !observed.call_has_value
        || observed.stats_event != terminal_driver::canonical_path_terminal_checked_stats_event(
            observed,
        )
        || !match observed.stats_event {
            terminal_driver::CanonicalPathTerminalStatsEvent::Success { .. } => true,
            _ => false,
        }
        || observed.score_entries_before as int
            != before.live.base.source.product.bridge.transcript.entries.len()
        || observed.score_entries_after != observed.score_entries_before
    { None } else {
        match canonical_path_loop_source_fold(before, loop_events) {
            None => None,
            Some(scanned) => {
                if scanned.live != before.live
                    || scanned.parser_frames.len() != 1
                    || scanned.parser_frames[0].is_none()
                    || observed.cursor_bytes_yielded as int
                        != scanned.parser_frames[0].unwrap().bytes.len()
                    || observed.cursor_bytes_traversed
                        != scanned.parser_frames[0].unwrap().cursor_traversed
                    || scanned.live.path_frames[0].is_none()
                    || observed.call_at != scanned.live.path_frames[0].unwrap().input.end
                { None } else {
                    match observed.value_event {
                        terminal_driver::CanonicalPathValueCallEvent::ReturnedSummary {
                            summary,
                        } => {
                            let reducer = root_source::CanonicalPathReducerEofObservation {
                                has_value: observed.call_has_value,
                                at: observed.call_at,
                                active_obligation: observed.active_obligation,
                                value_result: Some(summary),
                            };
                            match canonical_path_internal_value_next(scanned, 0) {
                                None => None,
                                Some(selected) =>
                                    canonical_path_common_root_complete_from_source(
                                        selected, source_snapshot, reducer,
                                    ),
                            }
                        },
                        _ => None,
                    }
                }
            },
        }
    }
}

/// A single dispatcher over the retained parser and live admission product.
/// Complete is terminal because the no-parent driver pop has no live frame.
pub enum LiveCanonicalPathCommonAction {
    InternalEdge { index: int },
    InternalValue { index: int },
    PushMacro {
        driver_step_index: usize,
        source_before: parser::ParserSourceSnapshot,
        loop_events: Seq<CanonicalPathLoopSourceEvent>,
        source_snapshot: parser::ParserSourceSnapshot,
        source: admission::AdmissionCanonicalPathPushSource,
        child: admission::AdmissionFrameState,
        storage: admission::AdmissionCanonicalPathPushStorage,
        child_image: epm::IndexedRegionImage,
        child_reader: Option<wire::WireRuntimeCursorImage>,
    },
    ChildReturn { observed: path_live::LiveCanonicalPathReturnObservation },
    RootCompleteMacro {
        driver_step_index: usize,
        source_before: parser::ParserSourceSnapshot,
        loop_events: Seq<CanonicalPathLoopSourceEvent>,
        source_snapshot: parser::ParserSourceSnapshot,
        observed: terminal_driver::CanonicalPathTerminalDriverObservation,
    },
}

pub enum LiveCanonicalPathCommonOutcome {
    Active { state: LiveCanonicalPathCommonState },
    Terminal { returned: terminal::LiveCanonicalPathTerminalOutcome },
}

pub open spec fn live_canonical_path_common_dispatch(
    before: LiveCanonicalPathCommonState,
    action: LiveCanonicalPathCommonAction,
) -> Option<LiveCanonicalPathCommonOutcome> {
    match action {
        LiveCanonicalPathCommonAction::InternalEdge { index } =>
            match canonical_path_internal_edge_next(before, index) {
                None => None,
                Some(state) => Some(LiveCanonicalPathCommonOutcome::Active { state }),
            },
        LiveCanonicalPathCommonAction::InternalValue { index } =>
            match canonical_path_internal_value_next(before, index) {
                None => None,
                Some(state) => Some(LiveCanonicalPathCommonOutcome::Active { state }),
            },
        LiveCanonicalPathCommonAction::PushMacro {
            driver_step_index, source_before, loop_events, source_snapshot,
            source, child, storage, child_image, child_reader,
        } => match canonical_path_common_push_macro_from_loop_source(
            before, driver_step_index, source_before, loop_events, source_snapshot,
            source, child, storage, child_image, child_reader,
        ) {
            None => None,
            Some(state) => Some(LiveCanonicalPathCommonOutcome::Active { state }),
        },
        LiveCanonicalPathCommonAction::ChildReturn { observed } =>
            match canonical_path_common_return_from_source(before, observed) {
                None => None,
                Some(state) => Some(LiveCanonicalPathCommonOutcome::Active { state }),
            },
        LiveCanonicalPathCommonAction::RootCompleteMacro {
            driver_step_index, source_before, loop_events, source_snapshot, observed,
        } => match canonical_path_common_root_complete_macro_from_loop_source(
            before, driver_step_index, source_before, loop_events, source_snapshot, observed,
        ) {
            None => None,
            Some(returned) => Some(LiveCanonicalPathCommonOutcome::Terminal { returned }),
        },
    }
}

pub proof fn canonical_path_internal_edge_preserves_wf(
    before: LiveCanonicalPathCommonState,
    index: int,
)
    requires live_canonical_path_common_wf(before),
    ensures match canonical_path_internal_edge_next(before, index) {
        Some(after) => {
            &&& live_canonical_path_common_wf(after)
            &&& after.live == before.live
            &&& after.selected_values == before.selected_values
            &&& after.parser_frames[index].unwrap().bytes.len()
                == before.parser_frames[index].unwrap().bytes.len() + 1
        },
        None => true,
    },
{
    if let Some(after) = canonical_path_internal_edge_next(before, index) {
        assert(after.live == before.live);
        assert(after.selected_values == before.selected_values);
        assert forall|at: int| 0 <= at < after.parser_frames.len()
            implies parser_slot_matches(after, at) by {
            if at == index {
                let frame = before.live.path_frames[index].unwrap();
                let old = before.parser_frames[index].unwrap();
                let next = after.parser_frames[index].unwrap();
                assert(parser_slot_matches(before, index));
                assert(next.bytes == old.bytes.push(frame.input.bytes[old.bytes.len() as int]));
                assert(next.positions.len() == next.bytes.len());
                assert(old.cursor_traversed as int <= old.bytes.len());
                assert(next.cursor_traversed as int == old.cursor_traversed as int + 1);
                assert(next.cursor_traversed as int <= next.bytes.len());
                assert forall|byte_at: int| 0 <= byte_at < next.bytes.len()
                    implies next.bytes[byte_at] == frame.input.bytes[byte_at] by {
                    if byte_at < old.bytes.len() {
                        assert(old.bytes[byte_at] == frame.input.bytes[byte_at]);
                    }
                }
                assert forall|byte_at: int| 0 <= byte_at < next.positions.len()
                    implies next.positions[byte_at].source == frame.input.positions[byte_at].source
                        && next.positions[byte_at].offset == frame.input.positions[byte_at].offset by {
                    if byte_at < old.positions.len() {
                        assert(next.positions[byte_at] == old.positions[byte_at]);
                    }
                }
            } else {
                assert(parser_slot_matches(before, at));
                assert(after.parser_frames[at] == before.parser_frames[at]);
            }
        }
        assert forall|at: int| 0 <= at < after.selected_values.len()
            implies match #[trigger] after.selected_values[at] {
                None => true,
                Some(gate) => match (after.live.path_frames[at], after.parser_frames[at]) {
                    (Some(frame), Some(slot)) => {
                        &&& slot.bytes.len() == frame.input.bytes.len()
                        &&& slot.parser.owners.len() == 0
                        &&& parser::parser_valid_finished_path(slot.parser.focus)
                        &&& gate == parser::parser_root_gate(slot.parser)
                    },
                    _ => false,
                },
            } by {
            if at == index {
                assert(before.selected_values[index].is_none());
            } else {
                assert(parser_slot_matches(before, at));
                assert(after.parser_frames[at] == before.parser_frames[at]);
            }
        }
    }
}

pub proof fn canonical_path_internal_value_preserves_wf(
    before: LiveCanonicalPathCommonState,
    index: int,
)
    requires live_canonical_path_common_wf(before),
    ensures match canonical_path_internal_value_next(before, index) {
        Some(after) => {
            &&& live_canonical_path_common_wf(after)
            &&& after.live == before.live
            &&& after.parser_frames == before.parser_frames
            &&& after.selected_values[index]
                == Some(parser::parser_root_gate(before.parser_frames[index].unwrap().parser))
        },
        None => true,
    },
{
    if let Some(after) = canonical_path_internal_value_next(before, index) {
        assert(after.live == before.live);
        assert(after.parser_frames == before.parser_frames);
        assert forall|at: int| 0 <= at < after.parser_frames.len()
            implies parser_slot_matches(after, at) by {
            assert(parser_slot_matches(before, at));
            assert(after.parser_frames[at] == before.parser_frames[at]);
        }
        assert forall|at: int| 0 <= at < after.selected_values.len()
            implies match #[trigger] after.selected_values[at] {
                None => true,
                Some(gate) => match (after.live.path_frames[at], after.parser_frames[at]) {
                    (Some(frame), Some(slot)) => {
                        &&& slot.bytes.len() == frame.input.bytes.len()
                        &&& slot.parser.owners.len() == 0
                        &&& parser::parser_valid_finished_path(slot.parser.focus)
                        &&& gate == parser::parser_root_gate(slot.parser)
                    },
                    _ => false,
                },
            } by {
            if at != index {
                assert(before.selected_values[at] == after.selected_values[at]);
                assert(after.selected_values[at] == before.selected_values[at]);
            }
        }
    }
}

pub proof fn canonical_path_internal_trace_prefix_wf(
    states: Seq<LiveCanonicalPathCommonState>,
    actions: Seq<CanonicalPathInternalAction>,
    upto: int,
)
    requires
        canonical_path_internal_trace(states, actions),
        0 <= upto <= actions.len(),
    ensures live_canonical_path_common_wf(states[upto]),
    decreases upto,
{
    if upto > 0 {
        canonical_path_internal_trace_prefix_wf(states, actions, upto - 1);
        let at = upto - 1;
        match actions[at] {
            CanonicalPathInternalAction::Edge { index } =>
                canonical_path_internal_edge_preserves_wf(states[at], index),
            CanonicalPathInternalAction::Value { index } =>
                canonical_path_internal_value_preserves_wf(states[at], index),
        }
    }
}

pub open spec fn boolean_root_source() -> admission::AdmissionCanonicalPathRootSource {
    let start = admission::AdmissionSourceAnchor { source: 1, offset: 0 };
    let end = admission::AdmissionSourceAnchor { source: 1, offset: 1 };
    let bytes = seq![0x02u8];
    let positions = seq![start];
    admission::AdmissionCanonicalPathRootSource {
        indexed_input: admission::AdmissionIndexedSourceImage {
            bytes,
            positions,
            end,
        },
        body_bytes: bytes,
        body_positions: positions,
        body_start: start,
        body_end: end,
        observed_program_identity: None,
        request_kind: 4,
        observed_root_kind: 4,
        cursor_initial: 0,
        waiting_initial: None,
        score_arena_initial: Seq::empty(),
        stack_len_before: 0,
        stack_capacity_before: 1,
        stack_len_after: 1,
        stack_capacity_after: 1,
        frame_variant_bytes: 1,
        stats_frame_variant_bytes: 1,
        used_bytes: 1,
        reserved_bytes: 1,
    }
}

pub proof fn boolean_root_common_witness()
    ensures
        admission::admission_canonical_path_root_source_wf(boolean_root_source()),
        live_canonical_path_common_root_from_source(boolean_root_source()).is_some(),
        live_canonical_path_common_wf(
            live_canonical_path_common_root_from_source(boolean_root_source()).unwrap(),
        ),
        live_canonical_path_common_root_from_source(boolean_root_source())
            .unwrap().live.path_frames[0].unwrap().input.bytes == seq![0x02u8],
{
    assert(admission::admission_canonical_path_root_source_wf(boolean_root_source()));
    live_canonical_path_common_root_is_wf(boolean_root_source());
}

pub proof fn boolean_root_internal_edge_value_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let edge = canonical_path_internal_edge_next(root, 0);
        &&& edge.is_some()
        &&& canonical_path_internal_value_next(edge.unwrap(), 0).is_some()
        &&& canonical_path_internal_value_next(edge.unwrap(), 0)
            .unwrap().selected_values[0].unwrap().completed_segments == 1
        &&& canonical_path_internal_value_next(edge.unwrap(), 0)
            .unwrap().selected_values[0].unwrap().stable
    },
{
    boolean_root_common_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let source = parser::ParserSource { source: 1, offset: 0 };
    let step = parser::feed_parser_byte(parser::initial_parser(), 0x02, source, 1);
    assert(step.event is Continue);
    assert(!parser::parser_root_gate(step.image).dead);
    assert(step.image.stamp == 1);
    assert(parser::parser_valid_finished_path(step.image.focus));
    assert(canonical_path_internal_edge_next(root, 0).is_some());
    canonical_path_internal_edge_preserves_wf(root, 0);
    let edge = canonical_path_internal_edge_next(root, 0).unwrap();
    assert(edge.parser_frames[0].unwrap().parser == step.image);
    assert(canonical_path_internal_value_next(edge, 0).is_some());
}

pub open spec fn boolean_root_source_snapshot() -> parser::ParserSourceSnapshot {
    let anchor = parser::ParserSource { source: 1, offset: 0 };
    let path = parser::ParserSourcePath {
        status: parser::ParserStatus::Open,
        completed_segments: 1,
        first_tag: Some(parser::ParserLocated { byte: 0x02, source: anchor }),
        stable: true,
        escape_allowed: true,
        stack: None,
    };
    parser::ParserSourceSnapshot {
        next_obligation: 0,
        stamp: 1,
        path_bytes: seq![parser::ParserLocated { byte: 0x02, source: anchor }],
        focused: path,
        root: path,
        owner_chain: Seq::empty(),
        frames: Seq::empty(),
        fault: false,
        root_gate: parser::ParserGate {
            dead: false,
            stack_empty: true,
            split_complete: false,
            completed_segments: 1,
            first_is_list: false,
            stable: true,
            escape_allowed: true,
        },
    }
}

pub proof fn boolean_root_scan_fold_source_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let actions = seq![
            CanonicalPathInternalAction::Edge { index: 0 },
            CanonicalPathInternalAction::Value { index: 0 },
        ];
        let scanned = canonical_path_driver_scan_fold(root, actions);
        let edge = canonical_path_internal_edge_next(root, 0).unwrap();
        &&& scanned.is_some()
        &&& scanned == canonical_path_internal_value_next(edge, 0)
        &&& canonical_path_source_boundary_matches(
            scanned.unwrap(), 0, boolean_root_source_snapshot(),
        )
        &&& scanned.unwrap().selected_values[0]
            == Some(boolean_root_source_snapshot().root_gate)
        &&& scanned.unwrap().live == root.live
    },
{
    boolean_root_internal_edge_value_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let edge = canonical_path_internal_edge_next(root, 0).unwrap();
    canonical_path_internal_edge_preserves_wf(root, 0);
    canonical_path_internal_value_preserves_wf(edge, 0);
    let source = parser::ParserSource { source: 1, offset: 0 };
    let step = parser::feed_parser_byte(parser::initial_parser(), 0x02, source, 1);
    assert(edge.parser_frames[0].unwrap().parser == step.image);
    assert(parser::source_parser_matches(boolean_root_source_snapshot(), step.image));
    let value = canonical_path_internal_value_next(edge, 0).unwrap();
    assert(live_canonical_path_common_wf(value));
    assert(value.parser_frames[0].unwrap().parser == step.image);
    assert(value.parser_frames[0].unwrap().bytes == seq![0x02u8]);
    assert(value.parser_frames[0].unwrap().positions == seq![source]);
    assert(value.selected_values[0] == Some(parser::parser_root_gate(step.image)));
    assert(parser::parser_root_gate(step.image) == boolean_root_source_snapshot().root_gate);
    assert(canonical_path_source_boundary_matches(value, 0, boolean_root_source_snapshot()));
    let actions = seq![
        CanonicalPathInternalAction::Edge { index: 0 },
        CanonicalPathInternalAction::Value { index: 0 },
    ];
    assert(root.live.path_frames.len() == 1);
    assert(edge.live.path_frames.len() == 1);
    assert(actions[0] == CanonicalPathInternalAction::Edge { index: 0 });
    assert(canonical_path_internal_edge_next(root, 0) == Some(edge));
    let tail = actions.subrange(1, actions.len() as int);
    assert(tail == seq![CanonicalPathInternalAction::Value { index: 0 }]);
    assert(canonical_path_driver_scan_fold(root, actions)
        == canonical_path_driver_scan_fold(edge, tail));
    assert(tail.len() == 1);
    assert(tail[0] == CanonicalPathInternalAction::Value { index: 0 });
    assert(canonical_path_internal_value_next(edge, 0) == Some(value));
    let empty = tail.subrange(1, tail.len() as int);
    assert(empty == Seq::<CanonicalPathInternalAction>::empty());
    assert(canonical_path_driver_scan_fold(value, empty)
        == Some(value));
    assert(canonical_path_driver_scan_fold(edge, tail)
        == canonical_path_driver_scan_fold(value, empty));
    assert(canonical_path_driver_scan_fold(edge, tail) == Some(value));
    assert(value.live == edge.live);
    assert(edge.live == root.live);
    assert(canonical_path_driver_scan_fold(root, actions) == Some(value));
}

pub open spec fn boolean_root_reducer_eof() -> root_source::CanonicalPathReducerEofObservation {
    root_source::CanonicalPathReducerEofObservation {
        has_value: true,
        at: epm::SourceAnchor { source: 1, offset: 1 },
        active_obligation: false,
        value_result: Some(root_source::CanonicalPathSummaryImage {
            form: root_source::CanonicalPathFormImage::Bare,
            canonical_path_stable: true,
        }),
    }
}

/// A nonempty Boolean path reaches the same no-parent Complete selector used
/// by production. This exercises the internal scan and EOF cursor commit.
pub proof fn boolean_root_complete_macro_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let actions = seq![
            CanonicalPathInternalAction::Edge { index: 0 },
            CanonicalPathInternalAction::Value { index: 0 },
        ];
        canonical_path_common_root_complete_macro_from_source(
            root, actions, boolean_root_source_snapshot(), boolean_root_reducer_eof(),
        ).is_some()
    },
{
    boolean_root_scan_fold_source_witness();
    live_canonical_path_common_root_is_wf(boolean_root_source());
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let actions = seq![
        CanonicalPathInternalAction::Edge { index: 0 },
        CanonicalPathInternalAction::Value { index: 0 },
    ];
    let scanned = canonical_path_driver_scan_fold(root, actions).unwrap();
    assert(live_canonical_path_common_at_driver_boundary(root));
    assert(scanned.live == root.live);
    assert(canonical_path_commit_selected_root(scanned).is_some());
    let committed = canonical_path_commit_selected_root(scanned).unwrap();
    assert(path_live::live_canonical_path_wf(committed.live));
    assert(terminal::live_canonical_path_terminal_source_ready(
        committed.live, boolean_root_reducer_eof(),
    ));
    terminal::live_canonical_path_terminal_complete_is_exact(
        committed.live, boolean_root_reducer_eof(),
    );
}

pub open spec fn boolean_root_driver_observation()
    -> terminal_driver::CanonicalPathTerminalDriverObservation {
    terminal_driver::CanonicalPathTerminalDriverObservation {
        stack_len_before: 1,
        top_kind: 4,
        cursor_exhausted: true,
        cursor_bytes_yielded: 1,
        cursor_bytes_traversed: 1,
        waiting_absent: true,
        waiting_child_absent: true,
        active_obligation: false,
        call_has_value: true,
        call_at: epm::SourceAnchor { source: 1, offset: 1 },
        value_event: terminal_driver::CanonicalPathValueCallEvent::ReturnedSummary {
            summary: root_source::CanonicalPathSummaryImage {
                form: root_source::CanonicalPathFormImage::Bare,
                canonical_path_stable: true,
            },
        },
        stats_read_before: 0,
        stats_traversed_before: 0,
        stats_event: terminal_driver::CanonicalPathTerminalStatsEvent::Success {
            read_after: 1,
            traversed_after: 1,
        },
        score_entries_before: 0,
        score_entries_after: 0,
    }
}

pub open spec fn boolean_root_loop_event() -> CanonicalPathLoopSourceEvent {
    CanonicalPathLoopSourceEvent {
        step_index: 0,
        index: 0,
        byte: 0x02,
        source: parser::ParserSource { source: 1, offset: 0 },
        skipped: 0,
        yielded_after: 1,
        traversed_after: 1,
    }
}

pub proof fn boolean_root_loop_source_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let edge = canonical_path_loop_source_fold(root, seq![boolean_root_loop_event()]);
        &&& edge.is_some()
        &&& edge == canonical_path_internal_edge_next(root, 0)
        &&& edge.unwrap().parser_frames[0].unwrap().bytes == seq![0x02u8]
        &&& edge.unwrap().parser_frames[0].unwrap().cursor_traversed == 1
        &&& canonical_path_internal_value_next(edge.unwrap(), 0).is_some()
    },
{
    boolean_root_internal_edge_value_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let edge = canonical_path_internal_edge_next(root, 0).unwrap();
    let event = boolean_root_loop_event();
    let events = seq![event];
    assert(canonical_path_loop_event_next(root, event) == Some(edge));
    assert(root.parser_frames.len() == 1);
    assert(root.parser_frames.last().is_some());
    assert(!parser::parser_waiting_obligation(
        root.parser_frames.last().unwrap().parser.focus,
    ));
    assert(events.len() == 1);
    assert(events[0] == event);
    let tail = events.subrange(1, events.len() as int);
    assert(tail.len() == 0);
    assert(canonical_path_loop_source_fold(edge, tail) == Some(edge));
    assert(canonical_path_loop_source_fold(root, events) == Some(edge));
}

pub open spec fn opaque_escape_root_source() -> admission::AdmissionCanonicalPathRootSource {
    let bytes = seq![0x0fu8, 0x03u8, 0xaau8, 0xbbu8, 0xccu8];
    let positions = seq![
        admission::AdmissionSourceAnchor { source: 1, offset: 0 },
        admission::AdmissionSourceAnchor { source: 1, offset: 1 },
        admission::AdmissionSourceAnchor { source: 1, offset: 2 },
        admission::AdmissionSourceAnchor { source: 1, offset: 3 },
        admission::AdmissionSourceAnchor { source: 1, offset: 4 },
    ];
    let end = admission::AdmissionSourceAnchor { source: 1, offset: 5 };
    admission::AdmissionCanonicalPathRootSource {
        indexed_input: admission::AdmissionIndexedSourceImage { bytes, positions, end },
        body_bytes: bytes,
        body_positions: positions,
        body_end: end,
        ..boolean_root_source()
    }
}

pub open spec fn opaque_escape_loop_events() -> Seq<CanonicalPathLoopSourceEvent> {
    seq![
        CanonicalPathLoopSourceEvent {
            step_index: 0,
            index: 0, byte: 0x0f,
            source: parser::ParserSource { source: 1, offset: 0 },
            skipped: 0, yielded_after: 1, traversed_after: 1,
        },
        CanonicalPathLoopSourceEvent {
            step_index: 0,
            index: 1, byte: 0x03,
            source: parser::ParserSource { source: 1, offset: 1 },
            skipped: 2, yielded_after: 4, traversed_after: 2,
        },
        CanonicalPathLoopSourceEvent {
            step_index: 0,
            index: 4, byte: 0xcc,
            source: parser::ParserSource { source: 1, offset: 4 },
            skipped: 0, yielded_after: 5, traversed_after: 3,
        },
    ]
}

/// Concrete nonvacuity for the optimized production shape: two bytes of a
/// three-byte opaque RawPar payload are skipped after its length byte, and the
/// final payload byte completes the obligation on a later loop iteration.
pub proof fn opaque_escape_first_scalar_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let first = canonical_path_loop_event_next(root, opaque_escape_loop_events()[0]);
        let tag_step = parser::feed_parser_byte(
            parser::initial_parser(), 0x0f,
            parser::ParserSource { source: 1, offset: 0 }, 1,
        );
        &&& admission::admission_canonical_path_root_source_wf(
            opaque_escape_root_source(),
        )
        &&& first.is_some()
        &&& first == canonical_path_internal_edge_next(root, 0)
        &&& live_canonical_path_common_wf(first.unwrap())
        &&& first.unwrap().parser_frames[0].unwrap().parser == tag_step.image
        &&& !parser::parser_waiting_obligation(
            first.unwrap().parser_frames[0].unwrap().parser.focus,
        )
    },
{
    assert(admission::admission_canonical_path_root_source_wf(
        opaque_escape_root_source(),
    ));
    live_canonical_path_common_root_is_wf(opaque_escape_root_source());
    let root = live_canonical_path_common_root_from_source(
        opaque_escape_root_source(),
    ).unwrap();
    let step = parser::feed_parser_byte(
        parser::initial_parser(), 0x0f,
        parser::ParserSource { source: 1, offset: 0 }, 1,
    );
    assert(step.event is Continue);
    assert(!parser::parser_root_gate(step.image).dead);
    assert(step.image.stamp == 1);
    let edge_step = canonical_path_internal_edge_next(root, 0);
    assert(edge_step.is_some());
    canonical_path_internal_edge_preserves_wf(root, 0);
    let edge = edge_step.unwrap();
    assert(canonical_path_loop_event_next(root, opaque_escape_loop_events()[0]) == Some(edge));
    assert(edge.parser_frames[0].unwrap().parser == step.image);
    assert(!parser::parser_waiting_obligation(edge.parser_frames[0].unwrap().parser.focus));
}

pub proof fn opaque_escape_length_started_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let first = canonical_path_loop_event_next(root, opaque_escape_loop_events()[0]).unwrap();
        let scalar = canonical_path_internal_edge_next(first, 0);
        let length_step = parser::feed_parser_byte(
            first.parser_frames[0].unwrap().parser, 0x03,
            parser::ParserSource { source: 1, offset: 1 }, 2,
        );
        &&& scalar.is_some()
        &&& live_canonical_path_common_wf(scalar.unwrap())
        &&& scalar.unwrap().parser_frames[0].unwrap().parser == length_step.image
        &&& scalar.unwrap().parser_frames[0].unwrap().parser.owners.len() == 0
        &&& scalar.unwrap().parser_frames[0].unwrap().parser.focus.stack.last()
            == (parser::ParserFrame::Obligation {
                id: 0,
                kind: parser::ParserObligationKind::RawParEscape,
                payload_len: 3,
                tag_source: parser::ParserSource { source: 1, offset: 0 },
                payload_start: 2,
                status: parser::ParserObligationStatus::Payload,
            })
        &&& parser::parser_forward_capacity(
            scalar.unwrap().parser_frames[0].unwrap().parser, 2,
        )
        &&& canonical_path_root_opaque_skip_exact(
            scalar.unwrap().parser_frames[0].unwrap().parser, 3, 2,
        )
        &&& length_step.event is ObligationStarted
    },
{
    opaque_escape_first_scalar_witness();
    let root = live_canonical_path_common_root_from_source(
        opaque_escape_root_source(),
    ).unwrap();
    let first = canonical_path_loop_event_next(root, opaque_escape_loop_events()[0]).unwrap();
    let tag_step = parser::feed_parser_byte(
        parser::initial_parser(), 0x0f,
        parser::ParserSource { source: 1, offset: 0 }, 1,
    );
    assert(first.parser_frames[0].unwrap().parser == tag_step.image);
    assert(tag_step.image.focus.stack.len() == 1);
    assert(tag_step.image.focus.stack.last() == (parser::ParserFrame::Varint {
        role: parser::ParserVarintRole::ObligationLength {
            kind: parser::ParserObligationKind::RawParEscape,
            tag_source: parser::ParserSource { source: 1, offset: 0 },
        },
        state: parser::initial_varint(),
    }));
    parser::initial_single_byte_varint(0x03);
    assert(parser::feed_parser_varint(parser::initial_varint(), 0x03)
        == Some(parser::ParserVarintFeed::Complete { value: 3 }));
    let tag_image = tag_step.image;
    let length_source = parser::ParserSource { source: 1, offset: 1 };
    assert(tag_image.owners.len() == 0);
    assert(tag_image.next_obligation == 0);
    assert(!parser::parser_root_gate(tag_image).dead);
    assert(parser::reusable_parser_payload(tag_image, 2).is_none());
    assert(parser::parser_owner_has_room(tag_image));
    let selected = parser::consume_parser_owner_byte(tag_image, 0x03);
    assert(selected.focus == tag_image.focus);
    assert(selected.owners.len() == 0);
    assert(selected.next_obligation == tag_image.next_obligation);
    assert(selected.stamp == tag_image.stamp);
    assert(parser::prepare_parser_tag(selected.focus, 0x03, length_source)
        == selected.focus);
    let role = parser::ParserVarintRole::ObligationLength {
        kind: parser::ParserObligationKind::RawParEscape,
        tag_source: parser::ParserSource { source: 1, offset: 0 },
    };
    let finished = parser::finish_parser_varint(selected, role, 3, length_source, 2);
    assert(finished.event is ObligationStarted);
    let focused = parser::feed_parser_focus(selected, 0x03, length_source, 2);
    assert(focused.event == finished.event);
    assert(focused.event is ObligationStarted);
    assert(parser::settle_parser_owners(focused.image).focus == focused.image.focus);
    let length_step = parser::feed_parser_byte(
        first.parser_frames[0].unwrap().parser, 0x03,
        length_source, 2,
    );
    assert(length_step.event == focused.event);
    assert(length_step.event is ObligationStarted);
    assert(length_step.image.focus.stack.len() == 1);
    assert(length_step.image.focus.stack.last() == (parser::ParserFrame::Obligation {
        id: 0,
        kind: parser::ParserObligationKind::RawParEscape,
        payload_len: 3,
        tag_source: parser::ParserSource { source: 1, offset: 0 },
        payload_start: 2,
        status: parser::ParserObligationStatus::Payload,
    }));
    assert(length_step.image.focus.status == parser::ParserStatus::Open);
    assert(length_step.image.owners.len() == 0);
    assert(!parser::parser_root_gate(length_step.image).dead);
    assert(length_step.image.stamp == 2);
    assert(length_step.image.stamp as int + 2 <= usize::MAX as int);
    assert(parser::parser_forward_capacity(length_step.image, 2));
    let scalar_step = canonical_path_internal_edge_next(first, 0);
    assert(scalar_step.is_some());
    canonical_path_internal_edge_preserves_wf(first, 0);
    let scalar = scalar_step.unwrap();
    assert(scalar.parser_frames[0].unwrap().parser == length_step.image);
    assert(scalar.parser_frames[0].unwrap().parser.owners.len() == 0);
    assert(parser::parser_forward_capacity(
        scalar.parser_frames[0].unwrap().parser, 2,
    ));
    assert(canonical_path_root_opaque_skip_exact(
        scalar.parser_frames[0].unwrap().parser, 3, 2,
    ));
}

pub proof fn opaque_escape_bulk_prefix_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let events = opaque_escape_loop_events();
        let first = canonical_path_loop_event_next(root, events[0]);
        let bulk = canonical_path_loop_event_next(first.unwrap(), events[1]);
        &&& admission::admission_canonical_path_root_source_wf(
            opaque_escape_root_source(),
        )
        &&& first.is_some()
        &&& bulk.is_some()
        &&& live_canonical_path_common_wf(bulk.unwrap())
        &&& bulk.unwrap().parser_frames[0].unwrap().bytes.len() == 4
        &&& bulk.unwrap().parser_frames[0].unwrap().cursor_traversed == 2
        &&& bulk.unwrap().parser_frames[0].unwrap().parser.stamp == 4
        &&& bulk.unwrap().parser_frames[0].unwrap().parser.owners.len() == 0
        &&& bulk.unwrap().parser_frames[0].unwrap().parser.focus.status
            == parser::ParserStatus::Open
        &&& !parser::parser_root_gate(
            bulk.unwrap().parser_frames[0].unwrap().parser,
        ).dead
        &&& bulk.unwrap().parser_frames[0].unwrap().parser.focus.stack.len() > 0
        &&& bulk.unwrap().parser_frames[0].unwrap().parser.focus.stack.last()
            == (parser::ParserFrame::Obligation {
                id: 0,
                kind: parser::ParserObligationKind::RawParEscape,
                payload_len: 3,
                tag_source: parser::ParserSource { source: 1, offset: 0 },
                payload_start: 2,
                status: parser::ParserObligationStatus::Payload,
            })
        &&& !parser::parser_waiting_obligation(
            first.unwrap().parser_frames[0].unwrap().parser.focus,
        )
        &&& !parser::parser_waiting_obligation(
            bulk.unwrap().parser_frames[0].unwrap().parser.focus,
        )
    },
{
    opaque_escape_length_started_witness();
    let root = live_canonical_path_common_root_from_source(
        opaque_escape_root_source(),
    ).unwrap();
    let events = opaque_escape_loop_events();
    let first = canonical_path_loop_event_next(root, events[0]).unwrap();
    let scalar = canonical_path_internal_edge_next(first, 0).unwrap();
    let bulk_step = canonical_path_loop_event_next(first, events[1]);
    assert(bulk_step.is_some());
    let bulk = bulk_step.unwrap();
    assert(bulk.live == scalar.live);
    assert(bulk.selected_values == scalar.selected_values);
    assert(bulk.parser_frames.len() == 1);
    let slot = bulk.parser_frames[0].unwrap();
    let frame = bulk.live.path_frames[0].unwrap();
    assert(slot.bytes.len() == 4);
    assert(slot.positions.len() == 4);
    assert(slot.parser.stamp == 4);
    assert(slot.cursor_traversed == 2);
    assert(slot.parser.owners.len() == 0);
    assert(slot.parser.focus.status == parser::ParserStatus::Open);
    assert(!parser::parser_root_gate(slot.parser).dead);
    assert(slot.parser.focus.stack.len() > 0);
    assert forall|at: int| 0 <= at < slot.bytes.len()
        implies slot.bytes[at] == frame.input.bytes[at] by {
        assert(at == 0 || at == 1 || at == 2 || at == 3);
    }
    assert forall|at: int| 0 <= at < slot.positions.len()
        implies slot.positions[at].source == frame.input.positions[at].source
            && slot.positions[at].offset == frame.input.positions[at].offset by {
        assert(at == 0 || at == 1 || at == 2 || at == 3);
    }
    assert(parser_slot_matches(bulk, 0));
    assert forall|index: int| 0 <= index < bulk.parser_frames.len()
        implies parser_slot_matches(bulk, index) by {
        assert(index == 0);
    }
    assert(live_canonical_path_common_wf(bulk));
    assert(!parser::parser_waiting_obligation(
        first.parser_frames[0].unwrap().parser.focus,
    ));
    assert(slot.parser.focus == scalar.parser_frames[0].unwrap().parser.focus);
    assert(match slot.parser.focus.stack.last() {
        parser::ParserFrame::Obligation {
            status: parser::ParserObligationStatus::Payload, ..
        } => true,
        _ => false,
    });
    assert(!parser::parser_waiting_obligation(
        bulk.parser_frames[0].unwrap().parser.focus,
    ));
}

pub proof fn opaque_escape_bulk_parser_state_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let events = opaque_escape_loop_events();
        let first = canonical_path_loop_event_next(root, events[0]).unwrap();
        let bulk = canonical_path_loop_event_next(first, events[1]).unwrap();
        let image = bulk.parser_frames[0].unwrap().parser;
        &&& image.stamp == 4
        &&& image.owners.len() == 0
        &&& image.focus.status == parser::ParserStatus::Open
        &&& !parser::parser_root_gate(image).dead
        &&& image.focus.stack.len() > 0
        &&& image.focus.stack.last() == (parser::ParserFrame::Obligation {
            id: 0,
            kind: parser::ParserObligationKind::RawParEscape,
            payload_len: 3,
            tag_source: parser::ParserSource { source: 1, offset: 0 },
            payload_start: 2,
            status: parser::ParserObligationStatus::Payload,
        })
    },
{
    opaque_escape_bulk_prefix_witness();
}

pub proof fn opaque_escape_final_parser_byte_transition(image: parser::ParserImage)
    requires
        image.stamp == 4,
        image.owners.len() == 0,
        image.focus.status == parser::ParserStatus::Open,
        !parser::parser_root_gate(image).dead,
        image.focus.stack.len() > 0,
        image.focus.stack.last() == (parser::ParserFrame::Obligation {
            id: 0,
            kind: parser::ParserObligationKind::RawParEscape,
            payload_len: 3,
            tag_source: parser::ParserSource { source: 1, offset: 0 },
            payload_start: 2,
            status: parser::ParserObligationStatus::Payload,
        }),
    ensures {
        let source = parser::ParserSource { source: 1, offset: 4 };
        let step = parser::feed_parser_byte(image, 0xcc, source, 5);
        &&& step.event is ObligationByte
        &&& step.image.stamp == 5
        &&& step.image.focus.stack.last() == (parser::ParserFrame::Obligation {
            id: 0,
            kind: parser::ParserObligationKind::RawParEscape,
            payload_len: 3,
            tag_source: parser::ParserSource { source: 1, offset: 0 },
            payload_start: 2,
            status: parser::ParserObligationStatus::AwaitingResolution {
                payload_end: source,
            },
        })
        &&& parser::parser_waiting_obligation(step.image.focus)
    },
{
    let source = parser::ParserSource { source: 1, offset: 4 };
    assert(parser::reusable_parser_payload(image, 5).is_none());
    assert(parser::parser_owner_has_room(image));
    assert(parser::prepare_parser_tag(image.focus, 0xcc, source) == image.focus);
    let focused = parser::feed_parser_focus(image, 0xcc, source, 5);
    assert(focused.event is ObligationByte);
    assert(focused.image.focus.stack.last() == (parser::ParserFrame::Obligation {
        id: 0,
        kind: parser::ParserObligationKind::RawParEscape,
        payload_len: 3,
        tag_source: parser::ParserSource { source: 1, offset: 0 },
        payload_start: 2,
        status: parser::ParserObligationStatus::AwaitingResolution {
            payload_end: source,
        },
    }));
    assert(parser::settle_parser_owners(focused.image) == focused.image);
    let step = parser::feed_parser_byte(image, 0xcc, source, 5);
    assert(step.event == focused.event);
    assert(step.image.focus == focused.image.focus);
    assert(step.image.stamp == 5);
    assert(parser::parser_waiting_obligation(step.image.focus));
}

pub proof fn opaque_escape_final_loop_edge_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let events = opaque_escape_loop_events();
        let first = canonical_path_loop_event_next(root, events[0]).unwrap();
        let bulk = canonical_path_loop_event_next(first, events[1]).unwrap();
        let final_step = canonical_path_loop_event_next(bulk, events[2]);
        &&& final_step.is_some()
        &&& final_step.unwrap().parser_frames[0].unwrap().bytes.len() == 5
        &&& final_step.unwrap().parser_frames[0].unwrap().cursor_traversed == 3
        &&& parser::parser_waiting_obligation(
            final_step.unwrap().parser_frames[0].unwrap().parser.focus,
        )
    },
{
    opaque_escape_bulk_prefix_witness();
    opaque_escape_bulk_parser_state_witness();
    let root = live_canonical_path_common_root_from_source(
        opaque_escape_root_source(),
    ).unwrap();
    let events = opaque_escape_loop_events();
    let first = canonical_path_loop_event_next(root, events[0]).unwrap();
    let bulk = canonical_path_loop_event_next(first, events[1]).unwrap();
    let image = bulk.parser_frames[0].unwrap().parser;
    opaque_escape_final_parser_byte_transition(image);
    let source = parser::ParserSource { source: 1, offset: 4 };
    let parser_step = parser::feed_parser_byte(image, 0xcc, source, 5);
    let selected = canonical_path_internal_edge_next(bulk, 0);
    assert(selected.is_some());
    canonical_path_internal_edge_preserves_wf(bulk, 0);
    assert(selected.unwrap().parser_frames[0].unwrap().parser == parser_step.image);
    assert(selected.unwrap().parser_frames[0].unwrap().bytes.len() == 5);
    assert(selected.unwrap().parser_frames[0].unwrap().cursor_traversed == 3);
    assert(events[2].skipped == 0);
    assert(events[2].yielded_after == 5);
    assert(events[2].traversed_after == 3);
    let final_step = canonical_path_loop_event_next(bulk, events[2]);
    assert(final_step == selected);
    assert(final_step.is_some());
    assert(final_step.unwrap().parser_frames[0].unwrap().parser == parser_step.image);
}

pub proof fn opaque_escape_final_payload_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let events = opaque_escape_loop_events();
        let first = canonical_path_loop_event_next(root, events[0]).unwrap();
        let bulk = canonical_path_loop_event_next(first, events[1]).unwrap();
        let final_step = canonical_path_loop_event_next(bulk, events[2]);
        &&& final_step.is_some()
        &&& final_step.unwrap().parser_frames[0].unwrap().bytes.len() == 5
        &&& final_step.unwrap().parser_frames[0].unwrap().cursor_traversed == 3
        &&& parser::parser_waiting_obligation(
            final_step.unwrap().parser_frames[0].unwrap().parser.focus,
        )
    },
{
    opaque_escape_final_loop_edge_witness();
}

pub proof fn opaque_escape_bulk_loop_source_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(
            opaque_escape_root_source(),
        ).unwrap();
        let after = canonical_path_loop_source_fold(root, opaque_escape_loop_events());
        &&& admission::admission_canonical_path_root_source_wf(
            opaque_escape_root_source(),
        )
        &&& after.is_some()
        &&& after.unwrap().parser_frames[0].unwrap().bytes.len() == 5
        &&& after.unwrap().parser_frames[0].unwrap().cursor_traversed == 3
        &&& parser::parser_waiting_obligation(
            after.unwrap().parser_frames[0].unwrap().parser.focus,
        )
    },
{
    opaque_escape_bulk_prefix_witness();
    opaque_escape_final_payload_witness();
    let root = live_canonical_path_common_root_from_source(
        opaque_escape_root_source(),
    ).unwrap();
    let events = opaque_escape_loop_events();
    let first = canonical_path_loop_event_next(root, events[0]).unwrap();
    let bulk = canonical_path_loop_event_next(first, events[1]).unwrap();
    let final_state = canonical_path_loop_event_next(bulk, events[2]).unwrap();
    assert(parser::parser_waiting_obligation(
        final_state.parser_frames[0].unwrap().parser.focus,
    ));
    assert(events.len() == 3);
    assert(events[0].step_index == events[1].step_index);
    assert(events[1].step_index == events[2].step_index);
    let tail1 = events.subrange(1, events.len() as int);
    let tail2 = tail1.subrange(1, tail1.len() as int);
    let empty = tail2.subrange(1, tail2.len() as int);
    assert(tail1.len() == 2);
    assert(tail2.len() == 1);
    assert(empty.len() == 0);
    assert(tail1[0] == events[1]);
    assert(tail1[1] == events[2]);
    assert(tail1[0].step_index == tail1[1].step_index);
    assert(tail2[0] == events[2]);
    assert(canonical_path_loop_event_next(first, tail1[0]) == Some(bulk));
    assert(first.parser_frames.len() == 1);
    assert(first.parser_frames.last().is_some());
    assert(!parser::parser_waiting_obligation(
        first.parser_frames.last().unwrap().parser.focus,
    ));
    assert(!parser::parser_waiting_obligation(
        bulk.parser_frames.last().unwrap().parser.focus,
    ));
    assert(canonical_path_loop_source_fold(final_state, empty) == Some(final_state));
    assert(canonical_path_loop_source_fold(bulk, tail2) == Some(final_state));
    assert(canonical_path_loop_source_fold(first, tail1) == Some(final_state));
    assert(root.parser_frames.len() == 1);
    assert(root.parser_frames.last().is_some());
    assert(!parser::parser_waiting_obligation(
        root.parser_frames.last().unwrap().parser.focus,
    ));
    assert(canonical_path_loop_event_next(root, events[0]) == Some(first));
    assert(canonical_path_loop_source_fold(root, events) == Some(final_state));
}

pub proof fn boolean_root_loop_selected_value_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let events = seq![boolean_root_loop_event()];
        let edge = canonical_path_loop_source_fold(root, events);
        let selected = canonical_path_internal_value_next(edge.unwrap(), 0);
        let actions = seq![
            CanonicalPathInternalAction::Edge { index: 0 },
            CanonicalPathInternalAction::Value { index: 0 },
        ];
        &&& edge.is_some()
        &&& selected.is_some()
        &&& edge == canonical_path_internal_edge_next(root, 0)
        &&& canonical_path_driver_scan_fold(root, actions) == selected
        &&& selected.unwrap().live == root.live
        &&& canonical_path_source_boundary_matches(
            selected.unwrap(), 0, boolean_root_source_snapshot(),
        )
    },
{
    boolean_root_loop_source_witness();
    boolean_root_scan_fold_source_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let events = seq![boolean_root_loop_event()];
    let edge = canonical_path_loop_source_fold(root, events).unwrap();
    let selected = canonical_path_internal_value_next(edge, 0).unwrap();
    let actions = seq![
        CanonicalPathInternalAction::Edge { index: 0 },
        CanonicalPathInternalAction::Value { index: 0 },
    ];
    assert(edge == canonical_path_internal_edge_next(root, 0).unwrap());
    assert(canonical_path_driver_scan_fold(root, actions)
        == canonical_path_internal_value_next(edge, 0));
    assert(canonical_path_driver_scan_fold(root, actions) == Some(selected));
    assert(canonical_path_source_boundary_matches(
        selected, 0, boolean_root_source_snapshot(),
    ));
    assert(selected.live == root.live);
}

pub proof fn boolean_root_checked_eof_completion_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        canonical_path_common_root_complete_macro_from_loop_source(
            root, 0, parser::initial_parser_source_snapshot(),
            seq![boolean_root_loop_event()], boolean_root_source_snapshot(),
            boolean_root_driver_observation(),
        ).is_some()
    },
{
    boolean_root_complete_macro_witness();
    boolean_root_loop_selected_value_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let events = seq![boolean_root_loop_event()];
    let observed = boolean_root_driver_observation();
    let edge = canonical_path_loop_source_fold(root, events).unwrap();
    let selected = canonical_path_internal_value_next(edge, 0).unwrap();
    let actions = seq![
        CanonicalPathInternalAction::Edge { index: 0 },
        CanonicalPathInternalAction::Value { index: 0 },
    ];
    assert(canonical_path_internal_value_next(edge, 0) == Some(selected));
    assert(canonical_path_driver_scan_fold(root, actions)
        == canonical_path_internal_value_next(edge, 0));
    assert(canonical_path_driver_scan_fold(root, actions) == Some(selected));
    assert(canonical_path_common_root_complete_from_source(
        selected, boolean_root_source_snapshot(), boolean_root_reducer_eof(),
    ).is_some());
    assert(edge.parser_frames[0].unwrap().bytes.len() == 1);
    assert(edge.parser_frames[0].unwrap().cursor_traversed == 1);
    parser::initial_parser_source_snapshot_has_witness();
    assert(canonical_path_source_boundary_matches(
        root, 0, parser::initial_parser_source_snapshot(),
    ));
    assert(observed.stats_event
        == terminal_driver::canonical_path_terminal_checked_stats_event(observed));
    assert(root.live.base.source.product.bridge.transcript.entries.len() == 0);
    assert(observed.cursor_bytes_yielded == 1);
    assert(observed.cursor_bytes_traversed == 1);
    assert(observed.call_at == root.live.path_frames[0].unwrap().input.end);
    assert(observed.value_event == (terminal_driver::CanonicalPathValueCallEvent::ReturnedSummary {
        summary: boolean_root_reducer_eof().value_result.unwrap(),
    }));
    let reducer = root_source::CanonicalPathReducerEofObservation {
        has_value: observed.call_has_value,
        at: observed.call_at,
        active_obligation: observed.active_obligation,
        value_result: Some(boolean_root_reducer_eof().value_result.unwrap()),
    };
    assert(reducer == boolean_root_reducer_eof());
    assert(canonical_path_common_root_complete_macro_from_loop_source(
        root, 0, parser::initial_parser_source_snapshot(),
        events, boolean_root_source_snapshot(), observed,
    ).is_some());
}

pub proof fn boolean_root_common_dispatch_complete_witness()
    ensures {
        let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
        let events = seq![boolean_root_loop_event()];
        live_canonical_path_common_dispatch(root,
            LiveCanonicalPathCommonAction::RootCompleteMacro {
                driver_step_index: 0,
                source_before: parser::initial_parser_source_snapshot(),
                loop_events: events,
                source_snapshot: boolean_root_source_snapshot(),
                observed: boolean_root_driver_observation(),
            },
        ).is_some()
    },
{
    boolean_root_checked_eof_completion_witness();
    let root = live_canonical_path_common_root_from_source(boolean_root_source()).unwrap();
    let events = seq![boolean_root_loop_event()];
    assert(canonical_path_common_root_complete_macro_from_loop_source(
        root, 0, parser::initial_parser_source_snapshot(),
        events, boolean_root_source_snapshot(), boolean_root_driver_observation(),
    ).is_some());
}

}
