//! A concrete raw-Par return into an EPathMap obligation over the live product.
//!
//! The admission stack has four frames: the outer Expr frame and the
//! program-31 frame of `product_epm_modeled_empty_nested_source`, the
//! EPathMap frame EP, and the raw-Par child C. EP is that source's modeled
//! empty EPathMap frame, now waiting (waiting_id 2, waiting_kind 1) on the
//! child that resolves a RawParEscape obligation. Its path holds the tag 0x0f
//! and the payload length 0x00; the pending obligation 0 has an empty payload
//! and awaits resolution, and the parser's only frame is that obligation. C is
//! a kind-1, program-1 frame without fields whose empty region ends at the
//! obligation's payload end, as `request_for_obligation` builds the child
//! body, and whose wire cursor is at End.
//!
//! `epm_raw_par_witness_unscored` is the live state before C is scored.
//! `epm_raw_par_witness_scored` scores C with `bridge_raw_par_step` and has
//! the post-state shape of the live MessageChildScore step: the runtime image
//! takes the new bridge, the arena is projected from the new transcript, and
//! the receipts are carried. `epm_raw_par_witness_returned` pops C: EP's
//! parser resolves the obligation with `RawPar { canonical_path_stable: false }`,
//! so its stack is empty, one segment is complete, the path is unstable and
//! alive, and EP no longer waits.
//!
//! The derived summary is `Par { canonical_path_stable: false }`: C has no
//! single returned component, so `admission_raw_par_path_stability` reports
//! an unstable path. `epm_raw_par_witness_is_step` shows that the scored and
//! the returned states are a `live_runtime_epm_raw_par_return_step`, and
//! `epm_raw_par_witness_is_integrated_step` lifts that step to an
//! `EpmRawParReturn` step of the integrated dispatcher.
//!
//! The score bridges, the products and the live states are opaque. Their
//! shape lemmas reveal them, and every other proof works from those shapes, so
//! that no proof unfolds more of the concrete state than it uses. The
//! well-formedness proofs go through lemmas over arbitrary states
//! (`epm_raw_par_witness_map_wf_from_parts`,
//! `epm_raw_par_witness_product_wf_from_parts`,
//! `epm_raw_par_witness_live_wf_from_parts`), which do the case analysis on
//! variables; the concrete proofs only supply their premises.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as nested,
    rpb3_epm_obligation_return_live as obligation_return,
    rpb3_epm_source_accept_refinement as source_accept, rpb3_integrated_dispatcher as dispatcher,
    rpb3_map_frame_refinement as map, rpb3_path_parser_refinement as parser,
    rpb3_runtime_frame_source_refinement as runtime, rpb3_score_admission_bridge as score_bridge,
    rpb3_score_arena_source_refinement as arena, rpb3_score_transcript as score,
    rpb3_wire_cursor_source_refinement as wire, LiveRuntimeProductState,
    LiveSourceProductState, ProductScoreState,
};

verus! {

/// The anchor of EP's path byte 0x0f, the RawParEscape tag.
pub open spec fn epm_raw_par_witness_tag_anchor() -> epm::SourceAnchor {
    epm::SourceAnchor { source: 0, offset: 7 }
}

/// The anchor of EP's path byte 0x00, the payload length. `start_obligation`
/// records it as the obligation's payload end.
pub open spec fn epm_raw_par_witness_end_anchor() -> epm::SourceAnchor {
    epm::SourceAnchor { source: 0, offset: 8 }
}

/// EP's parser before the return: an open root path whose first tag is 0x0f
/// and whose only frame is the RawParEscape obligation 0, which awaits
/// resolution with an empty payload.
pub open spec fn epm_raw_par_witness_parser() -> parser::ParserImage {
    parser::ParserImage {
        focus: parser::ParserPath {
            status: parser::ParserStatus::Open,
            completed_segments: 0,
            first_tag: Some((0x0fu8, parser::ParserSource { source: 0, offset: 7 })),
            stable: true,
            escape_allowed: true,
            stack: seq![parser::ParserFrame::Obligation {
                id: 0,
                kind: parser::ParserObligationKind::RawParEscape,
                payload_len: 0,
                tag_source: parser::ParserSource { source: 0, offset: 7 },
                payload_start: 2,
                status: parser::ParserObligationStatus::AwaitingResolution {
                    payload_end: parser::ParserSource { source: 0, offset: 8 },
                },
            }],
        },
        owners: Seq::empty(),
        next_obligation: 1,
        stamp: 2,
        fault: false,
    }
}

/// EP's parser after the obligation resolves with an unstable raw Par: the
/// stack is empty, one segment is complete, and the path is unstable.
pub open spec fn epm_raw_par_witness_resolved_parser() -> parser::ParserImage {
    let before = epm_raw_par_witness_parser();
    parser::ParserImage {
        focus: parser::ParserPath {
            completed_segments: 1,
            stable: false,
            stack: Seq::empty(),
            ..before.focus
        },
        ..before
    }
}

/// EP's path: the bytes [0x0f, 0x00] with the pending obligation 0 ready for
/// resolution, and the gate that is the image of the parser.
pub open spec fn epm_raw_par_witness_path() -> epm::PathTopology {
    epm::PathTopology {
        next_obligation: 1,
        pending: Some(epm::PathObligation {
            id: 0,
            kind: epm::PathObligationKind::RawParEscape,
            payload_len: 0,
            tag_source: epm_raw_par_witness_tag_anchor(),
            payload_start: 2,
            payload_end: epm_raw_par_witness_end_anchor(),
            awaiting_resolution: true,
        }),
        root_left: false,
        restore: Seq::empty(),
        path_bytes: seq![0x0fu8, 0x00u8],
        path_positions: seq![epm_raw_par_witness_tag_anchor(), epm_raw_par_witness_end_anchor()],
        gate: epm::PathGate {
            dead: false,
            stack_empty: false,
            split_complete: false,
            completed_segments: 0,
            first_is_list: false,
            stable: true,
            escape_allowed: true,
        },
        parser: epm_raw_par_witness_parser(),
    }
}

/// EP's path after the return: no pending obligation, the resolved parser,
/// and its gate.
pub open spec fn epm_raw_par_witness_resolved_path() -> epm::PathTopology {
    let before = epm_raw_par_witness_path();
    epm::PathTopology {
        pending: None,
        gate: epm::PathGate {
            stack_empty: true,
            completed_segments: 1,
            stable: false,
            ..before.gate
        },
        parser: epm_raw_par_witness_resolved_parser(),
        ..before
    }
}

/// EP's EPathMap frame: the modeled empty snapshot frame of
/// `product_epm_modeled_empty_source` with EP's path, waiting on the
/// obligation 0 for the child with return id 2.
pub open spec fn epm_raw_par_witness_parent_epm() -> epm::EpmFrame {
    epm::EpmFrame {
        path: epm_raw_par_witness_path(),
        wait: Some(epm::EpmWait::Obligation {
            id: 0,
            kind: epm::PathObligationKind::RawParEscape,
            return_id: 2,
        }),
        ..integrated::product_epm_modeled_empty_source().epm
    }
}

/// EP's EPathMap frame after the return: the resolved path and no wait.
pub open spec fn epm_raw_par_witness_resolved_epm() -> epm::EpmFrame {
    epm::EpmFrame {
        path: epm_raw_par_witness_resolved_path(),
        wait: None,
        ..epm_raw_par_witness_parent_epm()
    }
}

/// EP's admission frame: the EPathMap frame of the nested modeled source,
/// waiting on the kind-1 child with return id 2.
pub open spec fn epm_raw_par_witness_parent_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        waiting_id: Some(2),
        waiting_kind: 1,
        ..integrated::product_epm_modeled_empty_nested_source().admission.frames[2]
    }
}

/// The raw-Par child C at its End: kind 1, program 1, no fields, an empty
/// region that starts at the obligation's payload end, return id 2, and no
/// score yet.
pub open spec fn epm_raw_par_witness_child_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: 0,
        physical_start: 8,
        program_type_id: 1,
        region_start: 0,
        region_end: 0,
        source_bytes: Seq::empty(),
        cursor: 0,
        slot_count: 12,
        last_slot: None,
        seen: Set::empty(),
        required: Set::empty(),
        scalar_slots: Set::empty(),
        scalar_occurrences: Seq::empty(),
        returns: Seq::empty(),
        family: admission::AdmissionFamily::Other,
        waiting_id: None,
        waiting_kind: 0,
        waiting_slot: None,
        waiting_tag: None,
        return_id: Some(2),
        frame_kind: 1,
        body_id: 3,
        score_handle: None,
        nested_started: 0,
        nested_resolved: 0,
    }
}

/// C after `bridge_raw_par_step` attaches its score node at handle 2.
pub open spec fn epm_raw_par_witness_scored_child_frame() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        score_handle: Some(2),
        ..epm_raw_par_witness_child_frame()
    }
}

/// The unscored admission state [outer Expr, program 31, EP, C]: no score,
/// no completed frame, and the obligation ids 0 to 2 allocated.
pub open spec fn epm_raw_par_witness_admission() -> admission::AdmissionState {
    let frames = integrated::product_epm_modeled_empty_nested_source().admission.frames;
    admission::AdmissionState {
        frames: seq![
            frames[0],
            frames[1],
            epm_raw_par_witness_parent_frame(),
            epm_raw_par_witness_child_frame(),
        ],
        arena: Seq::empty(),
        root_end: 8,
        pushed: 4,
        completed: 0,
        next_obligation_id: 3,
    }
}

/// The unscored bridge: the unscored admission state and an empty transcript.
pub open spec fn epm_raw_par_witness_unscored_bridge() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::ScoreAdmissionBridge {
        admission: epm_raw_par_witness_admission(),
        transcript: score::empty_arena(),
    }
}

/// The bridge after `bridge_raw_par_step` appends C's connective flag
/// `Integer(0)`.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_flag_bridge() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::bridge_append_unattached(
        epm_raw_par_witness_unscored_bridge(),
        score::ScoreNode::Integer(0),
    ).unwrap().0
}

/// The bridge after the raw-Par constructor `Integer(999)` is appended.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_constructor_bridge() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::bridge_append_unattached(
        epm_raw_par_witness_flag_bridge(),
        score::ScoreNode::Integer(999i32 as i64),
    ).unwrap().0
}

/// The children of C's score node: the constructor handle 1, no component
/// handles, and the flag handle 0.
pub open spec fn epm_raw_par_witness_node_children() -> Seq<usize> {
    seq![1usize] + Seq::<usize>::empty().push(0usize)
}

/// The bridge after C's score node is appended and before it is attached.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_node_bridge() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::bridge_append_unattached(
        epm_raw_par_witness_constructor_bridge(),
        score::ScoreNode::Node(epm_raw_par_witness_node_children()),
    ).unwrap().0
}

/// The scored admission state: C holds the node handle 2, and the arena holds
/// the flag, the constructor and the node.
pub open spec fn epm_raw_par_witness_scored_admission() -> admission::AdmissionState {
    let frames = integrated::product_epm_modeled_empty_nested_source().admission.frames;
    admission::AdmissionState {
        frames: seq![
            frames[0],
            frames[1],
            epm_raw_par_witness_parent_frame(),
            epm_raw_par_witness_scored_child_frame(),
        ],
        ..epm_raw_par_witness_node_bridge().admission
    }
}

/// The scored bridge, which `bridge_raw_par_step` returns for the unscored
/// bridge.
pub open spec fn epm_raw_par_witness_scored_bridge() -> score_bridge::ScoreAdmissionBridge {
    score_bridge::ScoreAdmissionBridge {
        admission: epm_raw_par_witness_scored_admission(),
        transcript: epm_raw_par_witness_node_bridge().transcript,
    }
}

/// C's indexed region: no bytes, ending at the obligation's payload end.
pub open spec fn epm_raw_par_witness_child_region() -> epm::IndexedRegionImage {
    epm::IndexedRegionImage {
        bytes: Seq::empty(),
        positions: Seq::empty(),
        end: epm_raw_par_witness_end_anchor(),
    }
}

/// The indexed regions of the four frames.
pub open spec fn epm_raw_par_witness_regions() -> Seq<epm::IndexedRegionImage> {
    integrated::product_epm_modeled_empty_nested_source().indexed_regions.push(
        epm_raw_par_witness_child_region(),
    )
}

/// The unscored product.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_unscored_product() -> ProductScoreState {
    let bridge = epm_raw_par_witness_unscored_bridge();
    ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: bridge.admission,
            map_frames: seq![None, None, None, None],
            transcript: bridge.transcript,
        },
        epm_frames: seq![None, None, Some(epm_raw_par_witness_parent_epm()), None],
        indexed_regions: epm_raw_par_witness_regions(),
        accepted_epm_sources: seq![None, None, None, None],
    }
}

/// The scored product: the bridge of `bridge_raw_par_step`, the map following
/// it, the EPathMap frames and regions kept, and the receipts carried, as in
/// `product_message_child_score_step`.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_scored_product() -> ProductScoreState {
    let before = epm_raw_par_witness_unscored_product();
    let bridge = score_bridge::bridge_raw_par_step(before.bridge).unwrap().0;
    let shaped = ProductScoreState {
        bridge,
        map: map::IntegratedAdmissionState {
            admission: bridge.admission,
            map_frames: before.map.map_frames,
            transcript: bridge.transcript,
        },
        ..before
    };
    ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(before, shaped),
        ..shaped
    }
}

/// C's wire cursor: a fresh Message cursor over C's empty region.
pub open spec fn epm_raw_par_witness_child_cursor() -> wire::WireRuntimeCursorImage {
    wire::wire_runtime_message_pre_read(
        wire::WireRuntimeBackingImage::Borrowed {
            bytes: Seq::empty(),
            source: 0,
            absolute_start: 8,
            start: 0,
            len: 0,
            end_override: None,
        },
        0,
        0,
        0,
    )
}

/// The live state before C is scored. The outer and program-31 frames keep
/// the owner cursor of the nested live witness, EP has no wire reader, and C
/// has its own cursor.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_unscored() -> LiveRuntimeProductState {
    let product = epm_raw_par_witness_unscored_product();
    let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
    let reader = wire::wire_runtime_cursor_project(cursor).unwrap();
    let child_cursor = epm_raw_par_witness_child_cursor();
    let child_reader = wire::wire_runtime_cursor_project(child_cursor).unwrap();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::RuntimeFrameImage {
                score: product.bridge,
                capacity: 4,
                frame_bytes: 1,
                max_frames: 4,
                max_storage_bytes: 4,
                max_capacity_bytes: 4,
            },
            arena: arena::RuntimeScoreArena { entries: Seq::empty() },
            wire: seq![Some(reader), Some(reader), None, Some(child_reader)],
        },
        cursors: seq![Some(cursor), Some(cursor), None, Some(child_cursor)],
    }
}

/// The live state after C is scored, in the post-state shape of
/// `live_runtime_product_message_child_score_step`.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_scored() -> LiveRuntimeProductState {
    let before = epm_raw_par_witness_unscored();
    let product = epm_raw_par_witness_scored_product();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::RuntimeFrameImage { score: product.bridge, ..before.source.runtime },
            arena: integrated::product_runtime_arena_from_score(product.bridge.transcript),
            wire: before.source.wire,
        },
        cursors: before.cursors,
    }
}

/// The admission state after C returns: C is popped, EP becomes
/// `epm_parent_resolved`, and one more frame is completed.
pub open spec fn epm_raw_par_witness_returned_admission() -> admission::AdmissionState {
    let scored = epm_raw_par_witness_scored_admission();
    admission::AdmissionState {
        frames: scored.frames.subrange(0, 3).update(2, epm::epm_parent_resolved(scored.frames[2])),
        completed: (scored.completed as int + 1) as usize,
        ..scored
    }
}

/// The product after C returns into EP.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_returned_product() -> ProductScoreState {
    let before = epm_raw_par_witness_scored_product();
    let admission = epm_raw_par_witness_returned_admission();
    let provisional = ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge {
            admission,
            transcript: before.bridge.transcript,
        },
        map: map::IntegratedAdmissionState {
            admission,
            map_frames: before.map.map_frames.drop_last(),
            transcript: before.map.transcript,
        },
        epm_frames: before.epm_frames.drop_last().update(
            2,
            Some(epm_raw_par_witness_resolved_epm()),
        ),
        indexed_regions: before.indexed_regions.drop_last(),
        accepted_epm_sources: before.accepted_epm_sources,
    };
    ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(before, provisional),
        ..provisional
    }
}

/// The live state after C returns: the runtime follows the new admission
/// state, the arena is kept, and C's reader and cursor are dropped.
#[verifier::opaque]
pub open spec fn epm_raw_par_witness_returned() -> LiveRuntimeProductState {
    let before = epm_raw_par_witness_scored();
    let product = epm_raw_par_witness_returned_product();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product,
            runtime: runtime::runtime_after_admission_step(
                before.source.runtime,
                product.bridge.admission,
            ),
            arena: before.source.arena,
            wire: before.source.wire.drop_last(),
        },
        cursors: before.cursors.drop_last(),
    }
}

/// EP over the scored product, as `product_linked_parent` selects it.
pub open spec fn epm_raw_par_witness_prior() -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: epm_raw_par_witness_parent_epm(),
        admission: epm_raw_par_witness_scored_admission(),
        indexed_regions: epm_raw_par_witness_regions(),
    }
}

/// The resolved EP over the returned product, as `product_linked_top`
/// selects it.
pub open spec fn epm_raw_par_witness_next() -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: epm_raw_par_witness_resolved_epm(),
        admission: epm_raw_par_witness_returned_admission(),
        indexed_regions: epm_raw_par_witness_regions().drop_last(),
    }
}

/// EP over the unscored product.
pub open spec fn epm_raw_par_witness_unscored_parent() -> epm::LinkedEpmState {
    epm::LinkedEpmState {
        epm: epm_raw_par_witness_parent_epm(),
        admission: epm_raw_par_witness_admission(),
        indexed_regions: epm_raw_par_witness_regions(),
    }
}

/// The wire reader of a message frame is at the frame's cursor in the frame's
/// region, as `live_source_product_wf` requires of a present reader.
pub open spec fn epm_raw_par_witness_reader_at(
    reader: wire::WireSourceState,
    frame: admission::AdmissionFrameState,
) -> bool {
    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
    &&& wire::wire_cursor_shape(reader.cursor)
    &&& reader.cursor.region_start == frame.region_start
    &&& reader.cursor.region_end == frame.region_end
    &&& reader.cursor.next == frame.cursor
    &&& reader.cursor.source_bytes == frame.source_bytes
    &&& reader.terminal_error.is_none()
}

/// A runtime cursor derives a well-formed concrete cursor and projects to
/// `reader`, as `live_runtime_product_wf` requires of a present cursor.
pub open spec fn epm_raw_par_witness_cursor_ready(
    cursor: wire::WireRuntimeCursorImage,
    reader: wire::WireSourceState,
) -> bool {
    &&& wire::wire_runtime_derive_cursor(cursor).is_some()
    &&& wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap())
    &&& wire::wire_runtime_cursor_project(cursor) == Some(reader)
}

/// The facts of the nested modeled source that the witness reuses: its three
/// frames are well-formed, the outer Expr frame and the program-31 frame are
/// message frames, the third is an idle EPathMap frame of program 0, no frame
/// has returns, each frame is contained in its message parent, and the
/// indexed regions are linked to the frames.
pub proof fn epm_raw_par_witness_nested_frames()
    ensures
        ({
            let source = integrated::product_epm_modeled_empty_nested_source();
            let frames = source.admission.frames;
            &&& frames.len() == 3
            &&& source.indexed_regions.len() == 3
            &&& admission::admission_frame_wf(frames[0])
            &&& admission::admission_frame_wf(frames[1])
            &&& admission::admission_frame_wf(frames[2])
            &&& frames[0].return_id.is_none()
            &&& frames[0].region_end == 8
            &&& frames[0].frame_kind == 1
            &&& frames[1].frame_kind == 1
            &&& frames[2].frame_kind == 3
            &&& frames[2].program_type_id == 0
            &&& frames[2].waiting_id.is_none()
            &&& frames[0].returns.len() == 0
            &&& frames[1].returns.len() == 0
            &&& frames[2].returns.len() == 0
            &&& admission::admission_child_physical_contained(frames[0], frames[1])
            &&& admission::admission_child_physical_contained(frames[1], frames[2])
            &&& epm::linked_indexed_sources(source)
            &&& source.epm == integrated::product_epm_modeled_empty_source().epm
            &&& source.indexed_regions[2] == source.epm.snapshot
        }),
{
    integrated::product_epm_modeled_empty_nested_source_accepts();
    let source = integrated::product_epm_modeled_empty_nested_source();
    let frames = source.admission.frames;
    let bytes = integrated::product_epm_modeled_empty_snapshot_bytes();
    assert(admission::admission_frame_wf(frames[0]));
    assert(admission::admission_frame_wf(frames[1]));
    assert(admission::admission_frame_wf(frames[2]));
    assert(bytes.subrange(0, 8) =~= bytes);
    assert(admission::admission_child_physical_contained(frames[0], frames[1]));
    assert(admission::admission_child_physical_contained(frames[1], frames[2]));
}

/// EP's admission frame and its resolved form are well-formed and contained
/// in the program-31 frame, and EP has no nested EPathMap child in flight.
pub proof fn epm_raw_par_witness_parent_frame_wf()
    ensures
        ({
            let frames = integrated::product_epm_modeled_empty_nested_source().admission.frames;
            let parent = epm_raw_par_witness_parent_frame();
            &&& admission::admission_frame_wf(parent)
            &&& admission::admission_frame_wf(epm::epm_parent_resolved(parent))
            &&& admission::admission_child_physical_contained(frames[1], parent)
            &&& admission::admission_child_physical_contained(
                frames[1],
                epm::epm_parent_resolved(parent),
            )
            &&& parent.frame_kind == 3
            &&& parent.program_type_id == 0
            &&& parent.returns.len() == 0
            &&& parent.nested_started == parent.nested_resolved
        }),
{
    epm_raw_par_witness_nested_frames();
}

/// C is a well-formed and admissible frame before and after it is scored.
pub proof fn epm_raw_par_witness_child_frame_wf()
    ensures
        admission::admission_frame_wf(epm_raw_par_witness_child_frame()),
        admission::admission_frame_wf(epm_raw_par_witness_scored_child_frame()),
        admission::admission_family_admissible(epm_raw_par_witness_scored_child_frame()),
{
}

/// The unscored admission state is well-formed.
pub proof fn epm_raw_par_witness_frames_wf()
    ensures
        admission::admission_state_wf(epm_raw_par_witness_admission()),
{
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_child_frame_wf();
    let state = epm_raw_par_witness_admission();
    assert forall|index: int| 0 <= index < state.frames.len()
        implies admission::admission_frame_wf(state.frames[index]) by {
        assert(index == 0 || index == 1 || index == 2 || index == 3);
    }
    assert forall|index: int|
        1 <= index < state.frames.len()
            && (state.frames[index - 1].frame_kind == 1
                || state.frames[index - 1].frame_kind == 2)
        implies #[trigger] admission::admission_child_physical_contained(
            state.frames[index - 1],
            state.frames[index],
        ) by {
        assert(index == 1 || index == 2);
    }
    assert(admission::admission_state_wf(state));
}

/// The unscored bridge is well-formed: its arena and its transcript are empty.
pub proof fn epm_raw_par_witness_unscored_bridge_wf()
    ensures
        score_bridge::bridge_well_formed(epm_raw_par_witness_unscored_bridge()),
{
    score::empty_arena_well_formed();
}

/// An unattached append to a well-formed bridge succeeds when the kind is
/// valid and its transcript is nonempty and short. It pushes one arena
/// binding owned by the top frame and keeps the frames and the counters.
pub proof fn epm_raw_par_witness_append_entry(
    bridge: score_bridge::ScoreAdmissionBridge,
    kind: score::ScoreNode,
)
    requires
        score_bridge::bridge_well_formed(bridge),
        admission::admission_state_wf(bridge.admission),
        bridge.admission.arena.len() < 16,
        score::kind_valid(bridge.transcript.entries, kind),
        0 < score::kind_transcript(bridge.transcript.entries, kind).len() <= 4096,
    ensures
        match score_bridge::bridge_append_unattached(bridge, kind) {
            Some((next, handle)) => {
                &&& handle as int == bridge.admission.arena.len()
                &&& next.admission == (admission::AdmissionState {
                    arena: next.admission.arena,
                    ..bridge.admission
                })
                &&& next.admission.arena.len() == bridge.admission.arena.len() + 1
                &&& next.transcript.entries.len() == bridge.transcript.entries.len() + 1
                &&& next.transcript.entries.take(bridge.transcript.entries.len() as int)
                    == bridge.transcript.entries
                &&& next.transcript.entries[handle as int].encoded_len as int
                    == score::kind_transcript(bridge.transcript.entries, kind).len()
                &&& next.admission.arena[handle as int].encoded_len
                    == next.transcript.entries[handle as int].encoded_len
                &&& next.admission.arena[handle as int].transcript_bytes.len()
                    == next.admission.arena[handle as int].encoded_len
                &&& next.admission.arena[handle as int].body_id
                    == bridge.admission.frames.last().body_id
                &&& next.admission.arena[handle as int].semantic
                    == admission::admission_family_complete(bridge.admission.frames.last())
                &&& score_bridge::bridge_well_formed(next)
                &&& admission::admission_state_wf(next.admission)
            },
            None => false,
        },
{
    score::checked_append_exact(bridge.transcript, kind);
    score_bridge::bridge_append_unattached_exact(bridge, kind);
    score_bridge::bridge_append_unattached_preserves_admission_wf(bridge, kind);
    assert(bridge.admission.frames.len() > 0);
    assert(score::arena_well_formed(bridge.transcript));
    assert(score::checked_append(bridge.transcript, kind).is_some());
}

/// A frame without returns has no component handles for any list of tags.
pub proof fn epm_raw_par_witness_ordered_handles(tags: Seq<u32>)
    ensures
        score_bridge::bridge_raw_par_ordered_handles(
            Seq::<admission::AdmissionReturnedSummary>::empty(),
            tags,
        ) == Some(Seq::<usize>::empty()),
    decreases tags.len(),
{
    if tags.len() > 0 {
        epm_raw_par_witness_ordered_handles(tags.drop_first());
        assert(score_bridge::bridge_raw_par_handles_for_tag(
            Seq::<admission::AdmissionReturnedSummary>::empty(),
            tags[0],
        ) == Some(Seq::<usize>::empty()));
        assert(Seq::<usize>::empty() + Seq::<usize>::empty() =~= Seq::<usize>::empty());
    }
}

/// The three entries that `bridge_raw_par_step` appends for C: the flag
/// `Integer(0)` at handle 0, the constructor `Integer(999)` at handle 1, and
/// the node over [1, 0] at handle 2, a 22-byte transcript that C's body owns
/// semantically.
pub proof fn epm_raw_par_witness_score_entries()
    ensures
        ({
            let unscored = epm_raw_par_witness_unscored_bridge();
            let flag = epm_raw_par_witness_flag_bridge();
            let constructor = epm_raw_par_witness_constructor_bridge();
            let node = epm_raw_par_witness_node_bridge();
            &&& score_bridge::bridge_well_formed(unscored)
            &&& score_bridge::bridge_append_unattached(unscored, score::ScoreNode::Integer(0))
                == Some((flag, 0usize))
            &&& flag.admission.frames == unscored.admission.frames
            &&& flag.transcript.entries.len() == 1
            &&& score_bridge::bridge_append_unattached(
                flag,
                score::ScoreNode::Integer(999i32 as i64),
            ) == Some((constructor, 1usize))
            &&& score_bridge::bridge_append_unattached(
                constructor,
                score::ScoreNode::Node(epm_raw_par_witness_node_children()),
            ) == Some((node, 2usize))
            &&& node.admission == (admission::AdmissionState {
                arena: node.admission.arena,
                ..unscored.admission
            })
            &&& node.admission.arena.len() == 3
            &&& node.admission.arena[2].body_id == 3
            &&& node.admission.arena[2].semantic
            &&& node.admission.arena[2].encoded_len > 0
            &&& node.admission.arena[2].transcript_bytes.len()
                == node.admission.arena[2].encoded_len
            &&& score_bridge::bridge_well_formed(node)
            &&& admission::admission_state_wf(node.admission)
        }),
{
    reveal(epm_raw_par_witness_flag_bridge);
    reveal(epm_raw_par_witness_constructor_bridge);
    reveal(epm_raw_par_witness_node_bridge);
    epm_raw_par_witness_frames_wf();
    epm_raw_par_witness_unscored_bridge_wf();
    let unscored = epm_raw_par_witness_unscored_bridge();
    score::integer_transcript_exact(0);
    epm_raw_par_witness_append_entry(unscored, score::ScoreNode::Integer(0));
    let flag = epm_raw_par_witness_flag_bridge();
    score::integer_transcript_exact(999i32 as i64);
    epm_raw_par_witness_append_entry(flag, score::ScoreNode::Integer(999i32 as i64));
    let constructor = epm_raw_par_witness_constructor_bridge();
    let entries = constructor.transcript.entries;
    let children = epm_raw_par_witness_node_children();
    assert(children =~= seq![1usize, 0usize]);
    assert(score::child_handles_valid(entries, children));
    assert(entries.take(1)[0] == entries[0]);
    assert(entries[0] == flag.transcript.entries[0]);
    assert forall|i: int| 0 <= i < entries.len()
        implies entries[i].encoded_len as int == entries[i].transcript.len() by {
        assert(score::entry_well_formed(entries, i));
    }
    score::node_transcript_exact(entries, children);
    let tail = children.drop_first();
    assert(tail =~= seq![0usize]);
    assert(tail.drop_first() =~= Seq::<usize>::empty());
    assert(score::children_encoded_len(entries, tail.drop_first()) == 0);
    assert(score::children_encoded_len(entries, tail) == 10);
    assert(score::children_encoded_len(entries, children) == 20);
    epm_raw_par_witness_append_entry(
        constructor,
        score::ScoreNode::Node(children),
    );
}

/// `bridge_raw_par_step` scores C: with no returns and no connective it
/// appends the flag, the constructor and the node, and attaches the node to
/// C, which gives the scored bridge.
pub proof fn epm_raw_par_witness_score_step()
    ensures
        ({
            let scored = epm_raw_par_witness_scored_bridge();
            &&& score_bridge::bridge_raw_par_step(epm_raw_par_witness_unscored_bridge())
                == Some((scored, 0usize, 1usize, 2usize))
            &&& score_bridge::bridge_well_formed(scored)
            &&& admission::admission_state_wf(scored.admission)
            &&& scored.admission.arena.len() == 3
            &&& scored.admission.arena[2].body_id == 3
            &&& scored.admission.arena[2].semantic
            &&& scored.admission.arena[2].encoded_len > 0
            &&& scored.admission.arena[2].transcript_bytes.len()
                == scored.admission.arena[2].encoded_len
            &&& scored.admission.root_end == 8
            &&& scored.admission.pushed == 4
            &&& scored.admission.completed == 0
            &&& scored.admission.next_obligation_id == 3
        }),
{
    epm_raw_par_witness_frames_wf();
    epm_raw_par_witness_score_entries();
    epm_raw_par_witness_ordered_handles(score_bridge::bridge_raw_par_score_tags());
    let unscored = epm_raw_par_witness_unscored_bridge();
    let flag = epm_raw_par_witness_flag_bridge();
    let node = epm_raw_par_witness_node_bridge();
    let scored = epm_raw_par_witness_scored_bridge();
    let child = epm_raw_par_witness_child_frame();
    assert(unscored.admission.frames.last() == child);
    assert(score_bridge::bridge_raw_par_returns_source(child, unscored.admission.arena));
    assert(!admission::admission_raw_par_connective_used(child));
    assert(admission::admission_family_complete(child));
    assert(admission::admission_slots_complete(child));
    assert(admission::admission_family_admissible(child));
    assert(node.admission.frames[3] == child);
    assert(node.admission.frames.update(3, epm_raw_par_witness_scored_child_frame())
        =~= scored.admission.frames);
    assert(admission::admission_attach_score_step(node.admission, 2, 3)
        == Some(scored.admission));
    assert(score_bridge::bridge_constructor_step(
        flag,
        999i32,
        Seq::<usize>::empty().push(0usize),
    ) == Some((scored, 1usize, 2usize)));
    assert(score_bridge::bridge_raw_par_step(unscored)
        == Some((scored, 0usize, 1usize, 2usize)));
    score_bridge::bridge_raw_par_exact(unscored);
}

/// The scored bridge and its admission state are well-formed, the arena holds
/// C's three entries, and the counters are those of the unscored state.
pub proof fn epm_raw_par_witness_scored_bridge_wf()
    ensures
        ({
            let scored = epm_raw_par_witness_scored_bridge();
            &&& score_bridge::bridge_well_formed(scored)
            &&& admission::admission_state_wf(scored.admission)
            &&& scored.admission.frames.len() == 4
            &&& scored.admission.arena.len() == 3
            &&& scored.admission.arena[2].body_id == 3
            &&& scored.admission.arena[2].semantic
            &&& scored.admission.arena[2].encoded_len > 0
            &&& scored.admission.arena[2].transcript_bytes.len()
                == scored.admission.arena[2].encoded_len
            &&& scored.admission.root_end == 8
            &&& scored.admission.pushed == 4
            &&& scored.admission.completed == 0
            &&& scored.admission.next_obligation_id == 3
        }),
{
    epm_raw_par_witness_score_step();
}

/// The scored product in explicit form: the scored bridge, the map following
/// it, the EPathMap frames and regions of the unscored product, and no
/// receipts.
pub proof fn epm_raw_par_witness_scored_shape()
    ensures
        epm_raw_par_witness_scored_product() == (ProductScoreState {
            bridge: epm_raw_par_witness_scored_bridge(),
            map: map::IntegratedAdmissionState {
                admission: epm_raw_par_witness_scored_admission(),
                map_frames: seq![None, None, None, None],
                transcript: epm_raw_par_witness_scored_bridge().transcript,
            },
            epm_frames: seq![None, None, Some(epm_raw_par_witness_parent_epm()), None],
            indexed_regions: epm_raw_par_witness_regions(),
            accepted_epm_sources: seq![None, None, None, None],
        }),
{
    reveal(epm_raw_par_witness_scored_product);
    reveal(epm_raw_par_witness_unscored_product);
    epm_raw_par_witness_score_step();
    let after = epm_raw_par_witness_scored_product();
    assert(after.accepted_epm_sources =~= seq![None, None, None, None]);
}

/// The returned admission state is well-formed.
pub proof fn epm_raw_par_witness_popped_frames_wf()
    ensures
        admission::admission_state_wf(epm_raw_par_witness_returned_admission()),
{
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_scored_bridge_wf();
    let scored = epm_raw_par_witness_scored_admission();
    let state = epm_raw_par_witness_returned_admission();
    let frames = integrated::product_epm_modeled_empty_nested_source().admission.frames;
    let resolved = epm::epm_parent_resolved(epm_raw_par_witness_parent_frame());
    assert(state.frames =~= seq![frames[0], frames[1], resolved]);
    assert forall|index: int| 0 <= index < state.frames.len()
        implies admission::admission_frame_wf(state.frames[index]) by {
        assert(index == 0 || index == 1 || index == 2);
    }
    assert forall|index: int|
        1 <= index < state.frames.len()
            && (state.frames[index - 1].frame_kind == 1
                || state.frames[index - 1].frame_kind == 2)
        implies #[trigger] admission::admission_child_physical_contained(
            state.frames[index - 1],
            state.frames[index],
        ) by {
        assert(index == 1 || index == 2);
    }
    assert forall|handle: int| 0 <= handle < state.arena.len()
        implies state.arena[handle].encoded_len > 0
            && state.arena[handle].transcript_bytes.len() == state.arena[handle].encoded_len by {
        assert(state.arena[handle] == scored.arena[handle]);
    }
    assert(admission::admission_state_wf(state));
}

/// The admission states of the witness are well-formed: the unscored state
/// [outer Expr, program 31, EP, C], the scored state, and the state after C
/// returns into EP.
pub proof fn epm_raw_par_witness_admission_is_wf()
    ensures
        admission::admission_state_wf(epm_raw_par_witness_admission()),
        admission::admission_state_wf(epm_raw_par_witness_scored_admission()),
        admission::admission_state_wf(epm_raw_par_witness_returned_admission()),
{
    epm_raw_par_witness_frames_wf();
    epm_raw_par_witness_scored_bridge_wf();
    epm_raw_par_witness_popped_frames_wf();
}

/// EP's EPathMap frame, before and after the return, keeps the layout of the
/// modeled empty snapshot frame, and its snapshot is EP's indexed region.
pub proof fn epm_raw_par_witness_parent_layout()
    ensures
        epm::frame_layout_wf(epm_raw_par_witness_parent_epm()),
        epm::frame_layout_wf(epm_raw_par_witness_resolved_epm()),
        epm_raw_par_witness_parent_epm().snapshot == epm_raw_par_witness_regions()[2],
        epm_raw_par_witness_resolved_epm().snapshot == epm_raw_par_witness_regions()[2],
{
    integrated::product_epm_modeled_empty_source_accepts();
    epm_raw_par_witness_nested_frames();
}

/// The returned product in terms of the scored product, its carried
/// receipts, and the linked state of the resolved EP.
pub proof fn epm_raw_par_witness_returned_shape()
    ensures
        ({
            let scored = epm_raw_par_witness_scored_product();
            let returned = epm_raw_par_witness_returned_product();
            &&& returned == (ProductScoreState {
                bridge: score_bridge::ScoreAdmissionBridge {
                    admission: epm_raw_par_witness_returned_admission(),
                    transcript: scored.bridge.transcript,
                },
                map: map::IntegratedAdmissionState {
                    admission: epm_raw_par_witness_returned_admission(),
                    map_frames: scored.map.map_frames.drop_last(),
                    transcript: scored.map.transcript,
                },
                epm_frames: scored.epm_frames.drop_last().update(
                    2,
                    Some(epm_raw_par_witness_resolved_epm()),
                ),
                indexed_regions: scored.indexed_regions.drop_last(),
                accepted_epm_sources: seq![None, None, None],
            })
            &&& returned.epm_frames == seq![None, None, Some(epm_raw_par_witness_resolved_epm())]
            &&& integrated::product_epm_receipts_carried(scored, returned)
                == seq![None, None, None]
            &&& integrated::product_linked_top(returned) == Some(epm_raw_par_witness_next())
        }),
{
    reveal(epm_raw_par_witness_returned_product);
    epm_raw_par_witness_scored_shape();
    let scored = epm_raw_par_witness_scored_product();
    let returned = epm_raw_par_witness_returned_product();
    assert(scored.epm_frames.drop_last().update(2, Some(epm_raw_par_witness_resolved_epm()))
        =~= seq![None, None, Some(epm_raw_par_witness_resolved_epm())]);
    assert(returned.accepted_epm_sources =~= seq![None, None, None]);
    assert(integrated::product_epm_receipts_carried(scored, returned)
        =~= seq![None, None, None]);
}

/// EP's obligation resolves at the root path: `resolve_parser_obligation`
/// with `RawPar { canonical_path_stable: false }` pops the obligation and
/// bubbles one unstable segment, so EP's parser after the return has an empty
/// stack, one completed segment, an unstable path and no death. EP's gates
/// are the images of its parsers, and the returned product holds the
/// resolved frame.
pub proof fn epm_raw_par_witness_resolves_root()
    ensures
        ({
            let resolved = epm_raw_par_witness_resolved_parser();
            &&& parser::resolve_parser_obligation(
                epm_raw_par_witness_parser(),
                0,
                parser::ParserResolution::RawPar { canonical_path_stable: false },
            ) == resolved
            &&& resolved.focus.stack.len() == 0
            &&& resolved.focus.completed_segments == 1
            &&& !resolved.focus.stable
            &&& !parser::parser_root_gate(resolved).dead
            &&& epm::parser_gate_matches(
                epm_raw_par_witness_path().gate,
                epm_raw_par_witness_parser(),
            )
            &&& epm::parser_gate_matches(epm_raw_par_witness_resolved_path().gate, resolved)
            &&& epm_raw_par_witness_returned_product().epm_frames[2]
                == Some(epm_raw_par_witness_resolved_epm())
        }),
{
    epm_raw_par_witness_returned_shape();
    let before = epm_raw_par_witness_parser();
    assert(before.focus.stack.drop_last() =~= Seq::<parser::ParserFrame>::empty());
    assert(parser::resolve_parser_obligation(
        before,
        0,
        parser::ParserResolution::RawPar { canonical_path_stable: false },
    ) == epm_raw_par_witness_resolved_parser());
}

/// EP passes production's fatal pre-checks for its RawParEscape obligation,
/// accepts C's `Par { canonical_path_stable: false }` as the resolved frame,
/// and the source resolution of the return exists.
pub proof fn epm_raw_par_witness_parent_accepts()
    ensures
        obligation_return::epm_obligation_before_accept(
            epm_raw_par_witness_parent_epm(),
            epm::PathObligationKind::RawParEscape,
        ),
        epm::accept_child(
            epm_raw_par_witness_parent_epm(),
            epm_raw_par_witness_resolved_epm(),
            Some(2),
            epm::ChildSummary::Par { canonical_path_stable: false },
        ),
        nested::source_epm_resolution(
            epm_raw_par_witness_parent_epm().wait,
            epm::ChildSummary::Par { canonical_path_stable: false },
        ).is_some(),
{
    epm_raw_par_witness_resolves_root();
}

/// C's wire cursor derives, projects and is well-formed; it is at End; it
/// agrees with C's indexed region; and its reader is at C's cursor in C's
/// empty region, before and after C is scored.
pub proof fn epm_raw_par_witness_child_cursor_ends()
    ensures
        ({
            let cursor = epm_raw_par_witness_child_cursor();
            let reader = wire::wire_runtime_cursor_project(cursor).unwrap();
            &&& epm_raw_par_witness_cursor_ready(cursor, reader)
            &&& wire::wire_runtime_next_field(cursor) == Some(
                wire::WireConcreteTransition::End {
                    state: wire::wire_runtime_derive_cursor(cursor).unwrap(),
                },
            )
            &&& integrated::product_wire_indexed_source_agrees(
                cursor,
                epm_raw_par_witness_child_region(),
            )
            &&& epm_raw_par_witness_reader_at(reader, epm_raw_par_witness_child_frame())
            &&& epm_raw_par_witness_reader_at(reader, epm_raw_par_witness_scored_child_frame())
        }),
{
    let cursor = epm_raw_par_witness_child_cursor();
    assert(Seq::<u8>::empty().subrange(0, 0) =~= Seq::<u8>::empty());
    assert(wire::wire_runtime_derive_cursor(cursor).is_some());
    assert(wire::wire_runtime_cursor_project(cursor).is_some());
    assert(wire::wire_concrete_cursor_wf(wire::wire_runtime_derive_cursor(cursor).unwrap()));
    assert(integrated::product_wire_indexed_source_agrees(
        cursor,
        epm_raw_par_witness_child_region(),
    ));
}

/// The owner cursor of the outer and program-31 frames, the cursor of the
/// nested live witness, derives, projects to its reader and is well-formed,
/// and its reader is at both frames' cursor in their region.
pub proof fn epm_raw_par_witness_owner_ready()
    ensures
        ({
            let frames = integrated::product_epm_modeled_empty_nested_source().admission.frames;
            let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
            let reader = wire::wire_runtime_cursor_project(cursor).unwrap();
            &&& epm_raw_par_witness_cursor_ready(cursor, reader)
            &&& epm_raw_par_witness_reader_at(reader, frames[0])
            &&& epm_raw_par_witness_reader_at(reader, frames[1])
        }),
{
    integrated::live_epm_modeled_empty_nested_cursor_ready();
    let bytes = integrated::product_epm_modeled_empty_snapshot_bytes();
    assert(bytes.subrange(0, 8) =~= bytes);
}

/// The owner cursor agrees with the indexed regions of the outer and
/// program-31 frames: both are the modeled snapshot.
pub proof fn epm_raw_par_witness_owner_agrees()
    ensures
        ({
            let source = integrated::product_epm_modeled_empty_nested_source();
            let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
            &&& integrated::product_wire_indexed_source_agrees(cursor, source.indexed_regions[0])
            &&& integrated::product_wire_indexed_source_agrees(cursor, source.indexed_regions[1])
        }),
{
    let source = integrated::product_epm_modeled_empty_nested_source();
    let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
    let bytes = integrated::product_epm_modeled_empty_snapshot_bytes();
    assert(bytes.subrange(0, 8) =~= bytes);
    assert(integrated::product_wire_indexed_source_agrees(cursor, source.indexed_regions[0]));
    assert(integrated::product_wire_indexed_source_agrees(cursor, source.indexed_regions[1]));
}

/// The indexed regions are linked to the frames of the unscored, the scored
/// and the returned admission states.
pub proof fn epm_raw_par_witness_linked_sources()
    ensures
        epm::linked_indexed_sources(epm_raw_par_witness_unscored_parent()),
        epm::linked_indexed_sources(epm_raw_par_witness_prior()),
        epm::linked_indexed_sources(epm_raw_par_witness_next()),
{
    epm_raw_par_witness_nested_frames();
    let source = integrated::product_epm_modeled_empty_nested_source();
    let regions = epm_raw_par_witness_regions();
    let unscored = epm_raw_par_witness_unscored_parent();
    let prior = epm_raw_par_witness_prior();
    let next = epm_raw_par_witness_next();
    assert(regions.drop_last() =~= source.indexed_regions);
    assert(next.admission.frames =~= seq![
        source.admission.frames[0],
        source.admission.frames[1],
        epm::epm_parent_resolved(epm_raw_par_witness_parent_frame()),
    ]);
    assert forall|i: int| 0 <= i < 3 implies {
        &&& epm::indexed_image_wf(source.indexed_regions[i])
        &&& source.indexed_regions[i].bytes == source.admission.frames[i].source_bytes
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).source
            == source.admission.frames[i].physical_source
        &&& epm::indexed_position_at(source.indexed_regions[i], 0).offset
            == source.admission.frames[i].physical_start
    } by {}
    assert(epm::linked_indexed_sources(unscored));
    assert(epm::linked_indexed_sources(prior));
    assert(epm::linked_indexed_sources(next));
}

/// A map state over four frames without map frames is well-formed when its
/// admission state and its bridge are and no frame is a map frame.
pub proof fn epm_raw_par_witness_map_wf_from_parts(state: map::IntegratedAdmissionState)
    requires
        admission::admission_state_wf(state.admission),
        score_bridge::bridge_well_formed(score_bridge::ScoreAdmissionBridge {
            admission: state.admission,
            transcript: state.transcript,
        }),
        state.map_frames == seq![None, None, None, None],
        state.admission.frames.len() == 4,
        state.admission.frames[0].frame_kind != 2,
        state.admission.frames[1].frame_kind != 2,
        state.admission.frames[2].frame_kind != 2,
        state.admission.frames[3].frame_kind != 2,
    ensures
        map::integrated_admission_wf(state),
{
    assert forall|index: int| 0 <= index < state.map_frames.len()
        implies #[trigger] state.map_frames[index].is_none()
            && state.admission.frames[index].frame_kind != 2 by {
        assert(index == 0 || index == 1 || index == 2 || index == 3);
    }
    assert(map::integrated_admission_wf(state));
}

/// A product over four frames whose only EPathMap frame is the third and
/// which holds no receipt is well-formed when its bridge and its map are,
/// its regions are linked to its frames, and the EPathMap frame's layout is
/// well-formed over the third region.
pub proof fn epm_raw_par_witness_product_wf_from_parts(
    product: ProductScoreState,
    parent: epm::LinkedEpmState,
)
    requires
        score_bridge::bridge_well_formed(product.bridge),
        map::integrated_admission_wf(product.map),
        product.map.admission == product.bridge.admission,
        product.map.transcript == product.bridge.transcript,
        product.bridge.admission.frames.len() == 4,
        epm::linked_indexed_sources(parent),
        parent.admission == product.bridge.admission,
        parent.indexed_regions == product.indexed_regions,
        product.epm_frames == seq![None, None, Some(parent.epm), None],
        product.accepted_epm_sources == seq![None, None, None, None],
        product.bridge.admission.frames[2].frame_kind == 3,
        parent.epm.snapshot == product.indexed_regions[2],
        epm::frame_layout_wf(parent.epm),
    ensures
        integrated::product_score_wf(product),
{
    let frames = product.bridge.admission.frames;
    let regions = product.indexed_regions;
    assert forall|i: int| 0 <= i < product.accepted_epm_sources.len()
        implies match #[trigger] product.accepted_epm_sources[i] {
            None => true,
            Some(source) => integrated::product_epm_source_receipt_matches(
                frames[i],
                regions[i],
                source,
            ),
        } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert(integrated::product_epm_source_receipts_wf(product));
    assert forall|i: int| 0 <= i < regions.len() implies {
        &&& epm::indexed_image_wf(regions[i])
        &&& frames[i].source_bytes == regions[i].bytes
        &&& frames[i].physical_start == integrated::product_indexed_start_anchor(regions[i]).offset
        &&& frames[i].physical_source == integrated::product_indexed_start_anchor(regions[i]).source
    } by {
        assert(epm::indexed_position_at(regions[i], 0)
            == integrated::product_indexed_start_anchor(regions[i]));
    }
    assert(integrated::product_frame_source_anchors_match(product));
    assert forall|i: int| 0 <= i < product.epm_frames.len() implies {
        &&& epm::indexed_image_wf(regions[i])
        &&& regions[i].bytes == frames[i].source_bytes
        &&& match product.epm_frames[i] {
            None => true,
            Some(frame) => {
                &&& frames[i].frame_kind == 3
                &&& frame.snapshot == regions[i]
                &&& epm::frame_layout_wf(frame)
            },
        }
    } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert(integrated::product_score_wf(product));
}

/// The unscored product is well-formed.
pub proof fn epm_raw_par_witness_unscored_product_wf()
    ensures
        integrated::product_score_wf(epm_raw_par_witness_unscored_product()),
{
    reveal(epm_raw_par_witness_unscored_product);
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_frames_wf();
    epm_raw_par_witness_unscored_bridge_wf();
    epm_raw_par_witness_parent_layout();
    epm_raw_par_witness_linked_sources();
    let product = epm_raw_par_witness_unscored_product();
    epm_raw_par_witness_map_wf_from_parts(product.map);
    epm_raw_par_witness_product_wf_from_parts(product, epm_raw_par_witness_unscored_parent());
}

/// The scored product is well-formed.
pub proof fn epm_raw_par_witness_scored_product_wf()
    ensures
        integrated::product_score_wf(epm_raw_par_witness_scored_product()),
{
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_scored_bridge_wf();
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_parent_layout();
    epm_raw_par_witness_linked_sources();
    let product = epm_raw_par_witness_scored_product();
    epm_raw_par_witness_map_wf_from_parts(product.map);
    epm_raw_par_witness_product_wf_from_parts(product, epm_raw_par_witness_prior());
}

/// The unscored and the scored products are well-formed.
pub proof fn epm_raw_par_witness_product_is_wf()
    ensures
        integrated::product_score_wf(epm_raw_par_witness_unscored_product()),
        integrated::product_score_wf(epm_raw_par_witness_scored_product()),
{
    epm_raw_par_witness_unscored_product_wf();
    epm_raw_par_witness_scored_product_wf();
}

/// The unscored live state in explicit form.
pub proof fn epm_raw_par_witness_unscored_live_shape()
    ensures
        ({
            let state = epm_raw_par_witness_unscored();
            let product = epm_raw_par_witness_unscored_product();
            let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
            let reader = wire::wire_runtime_cursor_project(cursor).unwrap();
            let child_cursor = epm_raw_par_witness_child_cursor();
            let child_reader = wire::wire_runtime_cursor_project(child_cursor).unwrap();
            &&& state.source.product == product
            &&& state.source.runtime == runtime::RuntimeFrameImage {
                score: product.bridge,
                capacity: 4,
                frame_bytes: 1,
                max_frames: 4,
                max_storage_bytes: 4,
                max_capacity_bytes: 4,
            }
            &&& state.source.arena == arena::RuntimeScoreArena { entries: Seq::empty() }
            &&& state.source.wire == seq![Some(reader), Some(reader), None, Some(child_reader)]
            &&& state.cursors == seq![Some(cursor), Some(cursor), None, Some(child_cursor)]
        }),
{
    reveal(epm_raw_par_witness_unscored);
}

/// The scored live state in terms of the unscored one.
pub proof fn epm_raw_par_witness_scored_live_shape()
    ensures
        ({
            let state = epm_raw_par_witness_scored();
            let before = epm_raw_par_witness_unscored();
            let product = epm_raw_par_witness_scored_product();
            &&& state.source.product == product
            &&& state.source.runtime == runtime::RuntimeFrameImage {
                score: product.bridge,
                ..before.source.runtime
            }
            &&& state.source.arena == integrated::product_runtime_arena_from_score(
                product.bridge.transcript,
            )
            &&& state.source.wire == before.source.wire
            &&& state.cursors == before.cursors
        }),
{
    reveal(epm_raw_par_witness_scored);
}

/// The returned live state holds the returned product and is the pop of the
/// scored one.
pub proof fn epm_raw_par_witness_returned_pop()
    ensures
        epm_raw_par_witness_returned().source.product == epm_raw_par_witness_returned_product(),
        obligation_return::epm_obligation_return_pop(
            epm_raw_par_witness_scored(),
            epm_raw_par_witness_returned(),
        ),
{
    reveal(epm_raw_par_witness_returned);
}

/// A live state over four frames is well-formed when its product, its
/// runtime image and its arena are, the outer two frames share one owner
/// cursor and reader, the third frame is an EPathMap frame without either,
/// and the fourth has its own cursor and reader.
pub proof fn epm_raw_par_witness_live_wf_from_parts(
    state: LiveRuntimeProductState,
    cursor: wire::WireRuntimeCursorImage,
    reader: wire::WireSourceState,
    child_cursor: wire::WireRuntimeCursorImage,
    child_reader: wire::WireSourceState,
)
    requires
        integrated::product_score_wf(state.source.product),
        runtime::runtime_frame_image_wf(state.source.runtime),
        state.source.runtime.score == state.source.product.bridge,
        arena::runtime_projects_score(
            state.source.arena,
            state.source.product.bridge.transcript,
        ),
        state.source.product.bridge.admission.frames.len() == 4,
        state.source.wire == seq![Some(reader), Some(reader), None, Some(child_reader)],
        state.cursors == seq![Some(cursor), Some(cursor), None, Some(child_cursor)],
        epm_raw_par_witness_cursor_ready(cursor, reader),
        epm_raw_par_witness_cursor_ready(child_cursor, child_reader),
        integrated::product_wire_indexed_source_agrees(
            cursor,
            state.source.product.indexed_regions[0],
        ),
        integrated::product_wire_indexed_source_agrees(
            cursor,
            state.source.product.indexed_regions[1],
        ),
        integrated::product_wire_indexed_source_agrees(
            child_cursor,
            state.source.product.indexed_regions[3],
        ),
        epm_raw_par_witness_reader_at(reader, state.source.product.bridge.admission.frames[0]),
        epm_raw_par_witness_reader_at(reader, state.source.product.bridge.admission.frames[1]),
        state.source.product.bridge.admission.frames[2].frame_kind == 3,
        epm_raw_par_witness_reader_at(
            child_reader,
            state.source.product.bridge.admission.frames[3],
        ),
    ensures
        integrated::live_runtime_product_wf(state),
{
    let frames = state.source.product.bridge.admission.frames;
    assert forall|i: int| 0 <= i < state.source.wire.len() implies {
        let frame = frames[i];
        match #[trigger] state.source.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(present) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire::wire_cursor_shape(present.cursor)
                &&& present.cursor.region_start == frame.region_start
                &&& present.cursor.region_end == frame.region_end
                &&& present.cursor.next == frame.cursor
                &&& present.cursor.source_bytes == frame.source_bytes
                &&& present.terminal_error.is_none()
            },
        }
    } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert(integrated::live_source_product_wf(state.source));
    assert forall|i: int| 0 <= i < state.cursors.len()
        implies match #[trigger] state.cursors[i] {
            None => true,
            Some(present) => integrated::product_wire_indexed_source_agrees(
                present,
                state.source.product.indexed_regions[i],
            ),
        } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert forall|i: int| 0 <= i < state.cursors.len() implies {
        match (#[trigger] state.cursors[i], state.source.wire[i]) {
            (None, None) => true,
            (Some(present), Some(projected)) => {
                &&& wire::wire_runtime_derive_cursor(present).is_some()
                &&& wire::wire_concrete_cursor_wf(
                    wire::wire_runtime_derive_cursor(present).unwrap(),
                )
                &&& wire::wire_runtime_cursor_project(present) == Some(projected)
            },
            _ => false,
        }
    } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
    assert(integrated::live_runtime_product_wf(state));
}

/// The live state before C is scored is well-formed.
pub proof fn epm_raw_par_witness_unscored_state_wf()
    ensures
        integrated::live_runtime_product_wf(epm_raw_par_witness_unscored()),
{
    reveal(epm_raw_par_witness_unscored_product);
    epm_raw_par_witness_unscored_live_shape();
    epm_raw_par_witness_unscored_product_wf();
    epm_raw_par_witness_frames_wf();
    epm_raw_par_witness_unscored_bridge_wf();
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_owner_ready();
    epm_raw_par_witness_owner_agrees();
    epm_raw_par_witness_child_cursor_ends();
    score::empty_arena_well_formed();
    let state = epm_raw_par_witness_unscored();
    let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
    let child_cursor = epm_raw_par_witness_child_cursor();
    assert(runtime::runtime_frame_image_wf(state.source.runtime));
    assert(arena::runtime_projects_score(
        state.source.arena,
        state.source.product.bridge.transcript,
    ));
    epm_raw_par_witness_live_wf_from_parts(
        state,
        cursor,
        wire::wire_runtime_cursor_project(cursor).unwrap(),
        child_cursor,
        wire::wire_runtime_cursor_project(child_cursor).unwrap(),
    );
}

/// The live state after C is scored is well-formed.
pub proof fn epm_raw_par_witness_scored_state_wf()
    ensures
        integrated::live_runtime_product_wf(epm_raw_par_witness_scored()),
{
    epm_raw_par_witness_scored_live_shape();
    epm_raw_par_witness_unscored_live_shape();
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_scored_product_wf();
    epm_raw_par_witness_scored_bridge_wf();
    epm_raw_par_witness_nested_frames();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_owner_ready();
    epm_raw_par_witness_owner_agrees();
    epm_raw_par_witness_child_cursor_ends();
    let state = epm_raw_par_witness_scored();
    let cursor = integrated::live_epm_modeled_empty_runtime_cursor();
    let child_cursor = epm_raw_par_witness_child_cursor();
    integrated::product_runtime_arena_from_score_projects(
        epm_raw_par_witness_scored_bridge().transcript,
    );
    assert(runtime::runtime_frame_image_wf(state.source.runtime));
    epm_raw_par_witness_live_wf_from_parts(
        state,
        cursor,
        wire::wire_runtime_cursor_project(cursor).unwrap(),
        child_cursor,
        wire::wire_runtime_cursor_project(child_cursor).unwrap(),
    );
}

/// The live states before and after C is scored are well-formed.
pub proof fn epm_raw_par_witness_state_is_wf()
    ensures
        integrated::live_runtime_product_wf(epm_raw_par_witness_unscored()),
        integrated::live_runtime_product_wf(epm_raw_par_witness_scored()),
{
    epm_raw_par_witness_unscored_state_wf();
    epm_raw_par_witness_scored_state_wf();
}

/// The EPathMap frames of the scored product are a return stack over EP: the
/// regions are linked, EP sits under the top frame, and every EPathMap frame
/// is an admission frame of kind 3 with a well-formed layout over its region.
/// EP's kind comes from the open `epm_raw_par_witness_parent_frame`, not from
/// `epm_raw_par_witness_parent_frame_wf`: its containments of the nested
/// modeled source's frames equate the modeled bytes with a subrange of
/// themselves, and from that equation the solver can rewrite indexes into the
/// subrange without end.
pub proof fn epm_raw_par_witness_stack_source()
    ensures
        nested::epm_return_stack_source(
            epm_raw_par_witness_prior(),
            epm_raw_par_witness_scored_product().epm_frames,
        ),
{
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_linked_sources();
    epm_raw_par_witness_parent_layout();
    let prior = epm_raw_par_witness_prior();
    let frames = epm_raw_par_witness_scored_product().epm_frames;
    assert forall|i: int| 0 <= i < frames.len()
        implies match #[trigger] frames[i] {
            None => true,
            Some(frame) => prior.admission.frames[i].frame_kind == 3
                && frame.snapshot == prior.indexed_regions[i]
                && epm::frame_layout_wf(frame),
        } by {
        assert(i == 0 || i == 1 || i == 2 || i == 3);
    }
}

/// The admission facts of the scored C: its path is unstable because it has
/// no single returned component, its Par is the semantic default, its score
/// node is the semantic binding of its body, and the summary that
/// `concrete_derived_summary` derives under EP agrees with it.
pub proof fn epm_raw_par_witness_child_summary()
    ensures
        ({
            let child = epm_raw_par_witness_scored_child_frame();
            let arena = epm_raw_par_witness_scored_admission().arena;
            let summary = admission::AdmissionReturnedKind::Par {
                semantic_default: true,
                canonical_path_stable: false,
            };
            &&& admission::admission_raw_par_path_stability(child) == Some(false)
            &&& admission::admission_raw_par_semantic_default(child)
            &&& admission::admission_score_body_agrees(child, arena)
            &&& admission::admission_child_summary_agrees(child, summary, arena)
            &&& admission::concrete_derived_summary(epm_raw_par_witness_parent_frame(), child)
                == Some(summary)
        }),
{
    epm_raw_par_witness_scored_bridge_wf();
    epm_raw_par_witness_parent_frame_wf();
    let child = epm_raw_par_witness_scored_child_frame();
    let arena = epm_raw_par_witness_scored_admission().arena;
    assert(!admission::admission_raw_par_connective_used(child));
    assert(!admission::admission_raw_par_has_components(child));
    assert(admission::admission_family_complete(child));
    assert(admission::admission_score_body_agrees(child, arena));
}

/// The return derived from the scored product. C has no single returned
/// component, so `admission_raw_par_path_stability` reports an unstable path
/// and the summary is `Par { canonical_path_stable: false }`. C has no
/// components and uses no connective, so its admission summary is the
/// semantic default.
pub proof fn epm_raw_par_witness_derived_summary()
    ensures
        integrated::product_linked_parent(epm_raw_par_witness_scored_product()) == Some(
            epm_raw_par_witness_prior(),
        ),
        obligation_return::epm_obligation_derived(epm_raw_par_witness_scored_product()) == Some(
            nested::DerivedEpmReturn {
                summary: epm::ChildSummary::Par { canonical_path_stable: false },
                admission_summary: admission::AdmissionReturnedKind::Par {
                    semantic_default: true,
                    canonical_path_stable: false,
                },
                nested_before: None,
                nested_after: None,
            },
        ),
{
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_stack_source();
    epm_raw_par_witness_child_summary();
    let product = epm_raw_par_witness_scored_product();
    assert(integrated::product_linked_parent(product) == Some(epm_raw_par_witness_prior()));
}

/// EP is the linked parent of the scored product and passes the fatal
/// pre-checks for a RawParEscape return.
pub proof fn epm_raw_par_witness_parent_ready()
    ensures
        obligation_return::epm_obligation_parent_ready(
            epm_raw_par_witness_scored_product(),
            epm::PathObligationKind::RawParEscape,
        ),
{
    epm_raw_par_witness_derived_summary();
    epm_raw_par_witness_parent_accepts();
}

/// C returns into EP: the linked return of the event model holds for EP over
/// the scored product and the resolved EP over the returned product, with
/// the derived summaries.
pub proof fn epm_raw_par_witness_linked_return()
    ensures
        epm::linked_epm_child_return(
            epm_raw_par_witness_prior(),
            epm_raw_par_witness_next(),
            epm::ChildSummary::Par { canonical_path_stable: false },
            admission::AdmissionReturnedKind::Par {
                semantic_default: true,
                canonical_path_stable: false,
            },
            None,
            None,
        ),
{
    epm_raw_par_witness_scored_bridge_wf();
    epm_raw_par_witness_popped_frames_wf();
    epm_raw_par_witness_linked_sources();
    epm_raw_par_witness_parent_layout();
    epm_raw_par_witness_parent_accepts();
    epm_raw_par_witness_child_frame_wf();
    epm_raw_par_witness_parent_frame_wf();
    epm_raw_par_witness_child_summary();
}

/// The source return of C into EP: the derived return exists, EP's wait has
/// a resolution for its summary, and the linked return holds.
pub proof fn epm_raw_par_witness_source_return()
    ensures
        nested::source_linked_epm_child_return(
            epm_raw_par_witness_prior(),
            epm_raw_par_witness_next(),
            epm_raw_par_witness_scored_product().epm_frames,
        ),
{
    epm_raw_par_witness_derived_summary();
    epm_raw_par_witness_parent_accepts();
    epm_raw_par_witness_linked_return();
}

/// The scored and the returned products are a
/// `product_epm_child_return_step` with the derived summaries.
pub proof fn epm_raw_par_witness_product_return()
    ensures
        integrated::product_epm_child_return_step(
            epm_raw_par_witness_scored_product(),
            epm_raw_par_witness_returned_product(),
            epm::ChildSummary::Par { canonical_path_stable: false },
            admission::AdmissionReturnedKind::Par {
                semantic_default: true,
                canonical_path_stable: false,
            },
            None,
        ),
{
    reveal(integrated::product_epm_source_child_binding);
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_returned_shape();
    epm_raw_par_witness_derived_summary();
    epm_raw_par_witness_source_return();
    epm_raw_par_witness_linked_return();
}

/// The scored live state has C's cursor on top of four cursors.
pub proof fn epm_raw_par_witness_child_cursor_last()
    ensures
        epm_raw_par_witness_scored().cursors.len() == 4,
        epm_raw_par_witness_scored().cursors.last() == Some(epm_raw_par_witness_child_cursor()),
{
    epm_raw_par_witness_scored_live_shape();
    epm_raw_par_witness_unscored_live_shape();
}

/// The scored and the returned live states are a
/// `live_runtime_epm_raw_par_return_step`: C is a kind-1 child at its End on
/// a well-formed state, EP is ready for the RawParEscape obligation, the
/// product returns C into EP with the derived summaries, and the live images
/// are popped.
pub proof fn epm_raw_par_witness_is_step()
    ensures
        obligation_return::live_runtime_epm_raw_par_return_step(
            epm_raw_par_witness_scored(),
            epm_raw_par_witness_returned(),
        ),
{
    reveal(obligation_return::live_runtime_epm_raw_par_return_step);
    epm_raw_par_witness_scored_state_wf();
    epm_raw_par_witness_scored_live_shape();
    epm_raw_par_witness_child_cursor_last();
    epm_raw_par_witness_scored_shape();
    epm_raw_par_witness_parent_ready();
    epm_raw_par_witness_derived_summary();
    epm_raw_par_witness_child_cursor_ends();
    epm_raw_par_witness_product_return();
    epm_raw_par_witness_returned_pop();
}

/// The witness is an `EpmRawParReturn` step of the integrated dispatcher, by
/// the dispatcher's lift of `live_runtime_epm_raw_par_return_step`, and both
/// of its states are well-formed integrated states.
pub proof fn epm_raw_par_witness_is_integrated_step()
    ensures
        dispatcher::integrated_wf(
            dispatcher::IntegratedState::Active { product: epm_raw_par_witness_scored() },
        ),
        dispatcher::integrated_step(
            dispatcher::IntegratedState::Active { product: epm_raw_par_witness_scored() },
            dispatcher::IntegratedState::Active { product: epm_raw_par_witness_returned() },
            dispatcher::IntegratedAction::EpmRawParReturn,
        ),
        dispatcher::integrated_wf(
            dispatcher::IntegratedState::Active { product: epm_raw_par_witness_returned() },
        ),
{
    epm_raw_par_witness_is_step();
    dispatcher::integrated_epm_raw_par_return_is_integrated_step(
        epm_raw_par_witness_scored(),
        epm_raw_par_witness_returned(),
    );
}

}
