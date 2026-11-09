use vstd::prelude::*;

use super::{rpb3_generic_message_eof_source_refinement as message_source, *};

verus! {

#[cfg(rpb3_live_push)]
pub open spec fn map_eof_live_snapshot(
    before: LiveRuntimeProductState,
) -> Option<map::MapEofReceiptModel> {
    if before.cursors.len() <= 1 || before.source.product.map.map_frames.len() == 0 {
        None
    } else {
        match (
            before.cursors.last(),
            before.source.product.map.map_frames.last(),
        ) {
            (Some(cursor), Some(frame)) if
                cursor.terminal_error.is_none()
                && wire_source::wire_runtime_derive_cursor(cursor).is_some()
                && wire_source::wire_runtime_next_field(cursor)
                    == Some(wire_source::WireConcreteTransition::End {
                        state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
                    }) => map::map_eof_snapshot(
                frame, cursor.terminal_error.is_none(),
            ),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn map_eof_live_snapshot_source_bound(
    before: LiveRuntimeProductState,
)
    ensures match map_eof_live_snapshot(before) {
        None => true,
        Some(receipt) => {
            let cursor = before.cursors.last().unwrap();
            let frame = before.source.product.map.map_frames.last().unwrap();
            &&& cursor.terminal_error.is_none()
            &&& map::map_finish(frame) == Some(receipt.summary)
            &&& map::map_summary_source_projection(frame, receipt.summary)
            &&& map::map_eof_checked_complete(
                frame, cursor.terminal_error.is_none(), receipt,
                map::MapEofReturnedModel::AcceptedMessage {
                    summary: receipt.summary,
                },
            )
        },
    },
{
    if let Some(receipt) = map_eof_live_snapshot(before) {
        let cursor = before.cursors.last().unwrap();
        let frame = before.source.product.map.map_frames.last().unwrap();
        map::map_finish_source_bound(frame);
        assert(map::map_eof_snapshot(
            frame, cursor.terminal_error.is_none(),
        ) == Some(receipt));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn map_eof_live_finish_has_checked_receipt(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_product_map_entry_finish_step(before, after),
    ensures
        before.cursors.last().unwrap().terminal_error.is_none(),
        map_eof_live_snapshot(before).is_some(),
        match map::integrated_admission_driver_step(
            before.source.product.map, map::IntegratedAdmissionAction::MapFinish,
        ) {
            Some(map::IntegratedAdmissionOutcome::MapEntryCompleted { summary, .. }) => {
                let frame = before.source.product.map.map_frames.last().unwrap();
                let receipt = map_eof_live_snapshot(before).unwrap();
                &&& receipt.summary == summary
                &&& map::map_eof_checked_complete(
                    frame, true, receipt,
                    map::MapEofReturnedModel::AcceptedMessage { summary },
                )
            },
            _ => false,
        },
{
    live_runtime_product_map_entry_finish_source_shape(before, after);
    reveal(live_runtime_product_map_entry_finish_step);
    let cursor = before.cursors.last().unwrap();
    assert(before.cursors.len() == before.source.wire.len());
    assert(before.cursors[before.cursors.len() - 1] == Some(cursor));
    assert(wire_source::wire_runtime_derive_cursor(cursor).is_some());
    assert(wire_source::wire_concrete_cursor_wf(
        wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
    ));
    assert(wire_source::wire_runtime_next_field(cursor)
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(cursor).unwrap(),
        }));
    wire_source::wire_runtime_end_is_exact(cursor);
    map::integrated_map_completion_source_bound(before.source.product.map);
    match map::integrated_admission_driver_step(
        before.source.product.map, map::IntegratedAdmissionAction::MapFinish,
    ) {
        Some(map::IntegratedAdmissionOutcome::MapEntryCompleted { summary, .. }) => {
            let frame = before.source.product.map.map_frames.last().unwrap();
            assert(map::map_finish(frame) == Some(summary));
            assert(map::map_eof_snapshot(frame, cursor.terminal_error.is_none()).is_some());
            assert(map_eof_live_snapshot(before).is_some());
            map_eof_live_snapshot_source_bound(before);
            let receipt = map_eof_live_snapshot(before).unwrap();
            assert(receipt.summary == summary);
        },
        _ => assert(false),
    }
}

#[cfg(rpb3_live_push)]
pub proof fn map_eof_live_snapshot_rejects_cached_error(
    before: LiveRuntimeProductState,
    error: wire_source::WirePhysicalError,
)
    requires before.cursors.len() > 1, before.cursors.last().is_some(),
    ensures {
        let cursor = before.cursors.last().unwrap();
        let corrupt = LiveRuntimeProductState {
            cursors: before.cursors.update(
                before.cursors.len() - 1,
                Some(wire_source::WireRuntimeCursorImage {
                    terminal_error: Some(error), ..cursor
                }),
            ),
            ..before
        };
        &&& corrupt.source == before.source
        &&& corrupt.cursors.len() == before.cursors.len()
        &&& corrupt.cursors.last().unwrap().terminal_error == Some(error)
        &&& map_eof_live_snapshot(corrupt).is_none()
    },
{
    let cursor = before.cursors.last().unwrap();
    let corrupt_cursor = wire_source::WireRuntimeCursorImage {
        terminal_error: Some(error), ..cursor
    };
    let corrupt = LiveRuntimeProductState {
        cursors: before.cursors.update(before.cursors.len() - 1, Some(corrupt_cursor)),
        ..before
    };
    assert(map_eof_live_snapshot(corrupt).is_none());
}

#[cfg(rpb3_live_push)]
pub struct MessageEofSendSourceObservation {
    pub program_type_id: u16,
    pub cursor_after: usize,
    pub slot_count: usize,
    pub ghost seen: Set<int>,
    pub ghost scalar_slots: Set<int>,
    pub ghost scalar_occurrences: Seq<admission::AdmissionScalarOccurrence>,
    pub ghost returns: Seq<admission::AdmissionReturnedSummary>,
    pub ghost score_entries_before: Seq<rpb3_score_transcript::ScoreEntry>,
    pub ghost appended_entries: Seq<rpb3_score_transcript::ScoreEntry>,
}

#[cfg(rpb3_live_push)]
pub struct MessageEofSendComponentImage {
    pub program_type_id: u16,
    pub family: admission::AdmissionFamily,
    pub complete: bool,
    pub canonical_path_stable: bool,
    pub score_handle: usize,
}

#[cfg(rpb3_live_push)]
pub struct MessageEofSendFinishImage {
    pub ghost score: score_bridge::ScoreAdmissionBridge,
    pub component: MessageEofSendComponentImage,
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_eof_send_source_matches_before(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofSendSourceObservation,
) -> bool {
    if !score_bridge::bridge_well_formed(before)
        || !admission::admission_state_wf(before.admission)
        || before.admission.frames.len() == 0
    {
        false
    } else {
        let top = before.admission.frames.last();
        &&& top.frame_kind == 1
        &&& top.family == admission::AdmissionFamily::Send
        &&& top.program_type_id == observed.program_type_id
        &&& top.program_type_id == 15
        &&& top.cursor == observed.cursor_after
        &&& top.cursor == top.region_end
        &&& top.waiting_id.is_none()
        &&& top.score_handle.is_none()
        &&& top.slot_count == observed.slot_count
        &&& top.seen == observed.seen
        &&& top.scalar_slots == observed.scalar_slots
        &&& top.scalar_occurrences == observed.scalar_occurrences
        &&& top.returns == observed.returns
        &&& before.transcript.entries == observed.score_entries_before
    }
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_eof_send_finish_from_source(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofSendSourceObservation,
) -> Option<MessageEofSendFinishImage> {
    if !message_eof_send_source_matches_before(before, observed) {
        None
    } else if admission::admission_family_complete(before.admission.frames.last()) {
        match score_bridge::bridge_send_step(before) {
            Some((next, _, _, _, score_handle))
                if next.transcript.entries
                    == observed.score_entries_before + observed.appended_entries =>
                Some(MessageEofSendFinishImage {
                    score: next,
                    component: MessageEofSendComponentImage {
                        program_type_id: observed.program_type_id,
                        family: admission::AdmissionFamily::Send,
                        complete: true,
                        canonical_path_stable: false,
                        score_handle,
                    },
                }),
            _ => None,
        }
    } else {
        match score_bridge::bridge_nonsemantic_step(before) {
            Some((next, _, score_handle))
                if next.transcript.entries
                    == observed.score_entries_before + observed.appended_entries =>
                Some(MessageEofSendFinishImage {
                    score: next,
                    component: MessageEofSendComponentImage {
                        program_type_id: observed.program_type_id,
                        family: admission::AdmissionFamily::Send,
                        complete: false,
                        canonical_path_stable: false,
                        score_handle,
                    },
                }),
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_send_finish_from_source_summary_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofSendSourceObservation,
)
    ensures match message_eof_send_finish_from_source(before, observed) {
        None => true,
        Some(result) => {
            let top = before.admission.frames.last();
            let after = result.score;
            let handle = result.component.score_handle;
            &&& result.component.program_type_id == top.program_type_id
            &&& result.component.family == admission::AdmissionFamily::Send
            &&& result.component.complete == admission::admission_family_complete(top)
            &&& !result.component.canonical_path_stable
            &&& after.admission.frames.last().score_handle == Some(handle)
            &&& after.admission.arena[handle as int].semantic
                == result.component.complete
            &&& after.transcript.entries
                == observed.score_entries_before + observed.appended_entries
            &&& score_bridge::bridge_score_finish_selected(before) == Some(after)
            &&& after.admission.frames.last().seen == observed.seen
            &&& after.admission.frames.last().scalar_slots == observed.scalar_slots
            &&& after.admission.frames.last().scalar_occurrences
                == observed.scalar_occurrences
            &&& after.admission.frames.last().returns == observed.returns
        },
    },
{
    reveal(message_eof_send_finish_from_source);
    reveal(message_eof_send_source_matches_before);
    if let Some(result) = message_eof_send_finish_from_source(before, observed) {
        let top = before.admission.frames.last();
        assert(score_bridge::bridge_primary_component_frame(before));
        score_bridge::bridge_primary_component_finish_selected_exact(before);
        if admission::admission_family_complete(top) {
            score_bridge::bridge_send_exact(before);
        } else {
            score_bridge::bridge_nonsemantic_exact(before);
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_unattached_append_count(
    before: score_bridge::ScoreAdmissionBridge,
    kind: rpb3_score_transcript::ScoreNode,
)
    ensures match score_bridge::bridge_append_unattached(before, kind) {
        None => true,
        Some((after, _)) => after.transcript.entries.len()
            == before.transcript.entries.len() + 1,
    },
{}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_constructor_append_count(
    before: score_bridge::ScoreAdmissionBridge,
    constructor: i32,
    children: Seq<usize>,
)
    ensures match score_bridge::bridge_constructor_step(
        before, constructor, children,
    ) {
        None => true,
        Some((after, _, _)) => after.transcript.entries.len()
            == before.transcript.entries.len() + 2,
    },
{
    if let Some((after, constructor_handle, _)) =
        score_bridge::bridge_constructor_step(before, constructor, children)
    {
        let first = score_bridge::bridge_append_unattached(
            before, rpb3_score_transcript::ScoreNode::Integer(constructor as i64),
        ).unwrap();
        message_eof_unattached_append_count(
            before, rpb3_score_transcript::ScoreNode::Integer(constructor as i64),
        );
        let node = rpb3_score_transcript::ScoreNode::Node(
            seq![constructor_handle] + children,
        );
        message_eof_unattached_append_count(first.0, node);
        assert(after.transcript.entries.len()
            == first.0.transcript.entries.len() + 1);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_send_finish_from_source_append_count_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofSendSourceObservation,
)
    ensures match message_eof_send_finish_from_source(before, observed) {
        None => true,
        Some(result) => observed.appended_entries.len()
            == if result.component.complete { 4int } else { 2int },
    },
{
    reveal(message_eof_send_finish_from_source);
    reveal(message_eof_send_source_matches_before);
    if let Some(result) = message_eof_send_finish_from_source(before, observed) {
        if result.component.complete {
            let (persistent, channel, data, connective_used, cost) =
                score_bridge::bridge_send_parts(
                    before.admission.frames.last(), before.admission.arena,
                ).unwrap();
            let persistent_kind = rpb3_score_transcript::ScoreNode::Integer(
                if persistent { 1i64 } else { 0i64 },
            );
            let connective_kind = rpb3_score_transcript::ScoreNode::Integer(
                if connective_used { 1i64 } else { 0i64 },
            );
            let (after_persistent, persistent_handle) =
                score_bridge::bridge_append_unattached(before, persistent_kind).unwrap();
            let (after_connective, connective_handle) =
                score_bridge::bridge_append_unattached(
                    after_persistent, connective_kind,
                ).unwrap();
            let children = seq![persistent_handle, channel]
                + data + seq![connective_handle] + cost;
            message_eof_unattached_append_count(before, persistent_kind);
            message_eof_unattached_append_count(after_persistent, connective_kind);
            message_eof_constructor_append_count(after_connective, 300i32, children);
            assert(result.score.transcript.entries.len()
                == before.transcript.entries.len() + 4);
        } else {
            message_eof_constructor_append_count(
                before, i32::MAX, Seq::empty(),
            );
            assert(result.score.transcript.entries.len()
                == before.transcript.entries.len() + 2);
        }
        assert(result.score.transcript.entries
            == observed.score_entries_before + observed.appended_entries);
        assert(observed.score_entries_before == before.transcript.entries);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_send_finish_from_source_is_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofSendSourceObservation,
)
    ensures match message_eof_send_finish_from_source(before, observed) {
        None => true,
        Some(result) => {
            &&& result.component.complete
                == admission::admission_family_complete(before.admission.frames.last())
            &&& result.score.admission.frames.last().score_handle
                == Some(result.component.score_handle)
            &&& result.score.transcript.entries
                == observed.score_entries_before + observed.appended_entries
            &&& observed.appended_entries.len()
                == if result.component.complete { 4int } else { 2int }
            &&& score_bridge::bridge_score_finish_selected(before)
                == Some(result.score)
        },
    },
{
    message_eof_send_finish_from_source_summary_exact(before, observed);
    message_eof_send_finish_from_source_append_count_exact(before, observed);
}

/// Bundle, If, and Match share the same pre-pop source boundary. The ghost
/// slot/return image must be projected from the borrowed, pre-EOF MessageFrame;
/// equality to this image is a separate Rust-to-formal obligation. In
/// particular, it must not be inferred from the reducer's returned summary.
/// The transcript prefix and suffix must likewise come from the borrowed
/// arena entries, and returned_component from the actual finish result.
#[cfg(rpb3_live_push)]
pub struct MessageEofPrimaryComponentSourceObservation {
    pub program_type_id: u16,
    pub cursor_after: usize,
    pub slot_count: usize,
    pub ghost seen: Set<int>,
    pub ghost scalar_slots: Set<int>,
    pub ghost scalar_occurrences: Seq<admission::AdmissionScalarOccurrence>,
    pub ghost returns: Seq<admission::AdmissionReturnedSummary>,
    pub ghost score_entries_before: Seq<rpb3_score_transcript::ScoreEntry>,
    pub ghost appended_entries: Seq<rpb3_score_transcript::ScoreEntry>,
    pub returned_component: MessageEofPrimaryComponentImage,
}

#[cfg(rpb3_live_push)]
pub struct MessageEofPrimaryComponentImage {
    pub program_type_id: u16,
    pub family: admission::AdmissionFamily,
    pub complete: bool,
    pub canonical_path_stable: bool,
    pub score_handle: usize,
}

#[cfg(rpb3_live_push)]
pub struct MessageEofPrimaryComponentFinishImage {
    pub ghost score: score_bridge::ScoreAdmissionBridge,
    pub component: MessageEofPrimaryComponentImage,
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_eof_primary_component_source_matches_before(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
) -> bool {
    if !score_bridge::bridge_well_formed(before)
        || !admission::admission_state_wf(before.admission)
        || before.admission.frames.len() == 0
    {
        false
    } else {
        let top = before.admission.frames.last();
        &&& top.frame_kind == 1
        &&& match top.family {
            admission::AdmissionFamily::Bundle => top.program_type_id == 8,
            admission::AdmissionFamily::If => top.program_type_id == 23,
            admission::AdmissionFamily::Match => top.program_type_id == 22,
            _ => false,
        }
        &&& top.program_type_id == observed.program_type_id
        &&& top.cursor == observed.cursor_after
        &&& top.cursor == top.region_end
        &&& top.waiting_id.is_none()
        &&& top.score_handle.is_none()
        &&& top.slot_count == observed.slot_count
        &&& top.seen == observed.seen
        &&& top.scalar_slots == observed.scalar_slots
        &&& top.scalar_occurrences == observed.scalar_occurrences
        &&& top.returns == observed.returns
        &&& before.transcript.entries == observed.score_entries_before
    }
}

#[cfg(rpb3_live_push)]
pub open spec fn message_eof_primary_component_result_from_source(
    next: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
    family: admission::AdmissionFamily,
    complete: bool,
    score_handle: usize,
) -> Option<MessageEofPrimaryComponentFinishImage> {
    let component = observed.returned_component;
    if next.transcript.entries
            != observed.score_entries_before + observed.appended_entries
        || component.program_type_id != observed.program_type_id
        || component.family != family
        || component.complete != complete
        || component.canonical_path_stable
        || component.score_handle != score_handle
    {
        None
    } else {
        Some(MessageEofPrimaryComponentFinishImage {
            score: next,
            component,
        })
    }
}

/// The successful Match branch is admissible only when all required cases
/// have fixed-point guards. A complete but non-fixed Match is an error, not
/// an incomplete component with a nonsemantic score.
#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_eof_primary_component_finish_from_source(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
) -> Option<MessageEofPrimaryComponentFinishImage> {
    if !message_eof_primary_component_source_matches_before(before, observed) {
        None
    } else {
        let top = before.admission.frames.last();
        if admission::admission_family_complete(top) {
            if !admission::admission_family_admissible(top) {
                None
            } else {
                match top.family {
                    admission::AdmissionFamily::Bundle =>
                        match score_bridge::bridge_bundle_step(before) {
                            Some((next, _, score_handle)) =>
                                message_eof_primary_component_result_from_source(
                                    next, observed, top.family, true, score_handle,
                                ),
                            _ => None,
                        },
                    admission::AdmissionFamily::If =>
                        match score_bridge::bridge_if_step(before) {
                            Some((next, _, _, score_handle)) =>
                                message_eof_primary_component_result_from_source(
                                    next, observed, top.family, true, score_handle,
                                ),
                            _ => None,
                        },
                    admission::AdmissionFamily::Match =>
                        match score_bridge::bridge_match_step(before) {
                            Some((next, _, _, score_handle)) =>
                                message_eof_primary_component_result_from_source(
                                    next, observed, top.family, true, score_handle,
                                ),
                            _ => None,
                        },
                    _ => None,
                }
            }
        } else {
            match score_bridge::bridge_nonsemantic_step(before) {
                Some((next, _, score_handle)) =>
                    message_eof_primary_component_result_from_source(
                        next, observed, top.family, false, score_handle,
                    ),
                _ => None,
            }
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_primary_component_finish_from_source_summary_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
)
    ensures match message_eof_primary_component_finish_from_source(before, observed) {
        None => true,
        Some(result) => {
            let top = before.admission.frames.last();
            let after = result.score;
            let handle = result.component.score_handle;
            &&& result.component == observed.returned_component
            &&& result.component.program_type_id == top.program_type_id
            &&& result.component.family == top.family
            &&& result.component.complete == admission::admission_family_complete(top)
            &&& !result.component.canonical_path_stable
            &&& after.admission.frames.last().score_handle == Some(handle)
            &&& after.admission.arena[handle as int].semantic
                == result.component.complete
            &&& after.transcript.entries
                == observed.score_entries_before + observed.appended_entries
            &&& score_bridge::bridge_score_finish_selected(before) == Some(after)
            &&& after.admission.frames.last().seen == observed.seen
            &&& after.admission.frames.last().scalar_slots == observed.scalar_slots
            &&& after.admission.frames.last().scalar_occurrences
                == observed.scalar_occurrences
            &&& after.admission.frames.last().returns == observed.returns
        },
    },
{
    reveal(message_eof_primary_component_finish_from_source);
    reveal(message_eof_primary_component_source_matches_before);
    if let Some(result) = message_eof_primary_component_finish_from_source(before, observed) {
        let top = before.admission.frames.last();
        assert(score_bridge::bridge_primary_component_frame(before));
        score_bridge::bridge_primary_component_finish_selected_exact(before);
        if admission::admission_family_complete(top) {
            match top.family {
                admission::AdmissionFamily::Bundle =>
                    score_bridge::bridge_bundle_exact(before),
                admission::AdmissionFamily::If =>
                    score_bridge::bridge_if_exact(before),
                admission::AdmissionFamily::Match =>
                    score_bridge::bridge_match_exact(before),
                _ => assert(false),
            }
        } else {
            score_bridge::bridge_nonsemantic_exact(before);
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_primary_component_finish_from_source_append_count_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
)
    ensures match message_eof_primary_component_finish_from_source(before, observed) {
        None => true,
        Some(result) => observed.appended_entries.len()
            == if result.component.complete
                && result.component.family != admission::AdmissionFamily::Bundle
            { 3int } else { 2int },
    },
{
    reveal(message_eof_primary_component_finish_from_source);
    reveal(message_eof_primary_component_source_matches_before);
    if let Some(result) = message_eof_primary_component_finish_from_source(before, observed) {
        let top = before.admission.frames.last();
        if result.component.complete {
            match top.family {
                admission::AdmissionFamily::Bundle => {
                    let (body, read, write) = score_bridge::bridge_bundle_parts(
                        top, before.admission.arena,
                    ).unwrap();
                    message_eof_constructor_append_count(
                        before, score_bridge::bridge_bundle_constructor(read, write),
                        seq![body],
                    );
                    assert(result.score.transcript.entries.len()
                        == before.transcript.entries.len() + 2);
                },
                admission::AdmissionFamily::If => {
                    let (condition, if_true, if_false, connective_used) =
                        score_bridge::bridge_if_parts(top, before.admission.arena).unwrap();
                    let flag = rpb3_score_transcript::ScoreNode::Integer(
                        if connective_used { 1i64 } else { 0i64 },
                    );
                    let (after_flag, flag_handle) =
                        score_bridge::bridge_append_unattached(before, flag).unwrap();
                    message_eof_unattached_append_count(before, flag);
                    message_eof_constructor_append_count(
                        after_flag, 309i32,
                        seq![condition, if_true, if_false, flag_handle],
                    );
                    assert(result.score.transcript.entries.len()
                        == before.transcript.entries.len() + 3);
                },
                admission::AdmissionFamily::Match => {
                    let (target, cases, connective_used) =
                        score_bridge::bridge_match_parts(top, before.admission.arena).unwrap();
                    let flag = rpb3_score_transcript::ScoreNode::Integer(
                        if connective_used { 1i64 } else { 0i64 },
                    );
                    let (after_flag, flag_handle) =
                        score_bridge::bridge_append_unattached(before, flag).unwrap();
                    message_eof_unattached_append_count(before, flag);
                    message_eof_constructor_append_count(
                        after_flag, 304i32, seq![target] + cases + seq![flag_handle],
                    );
                    assert(result.score.transcript.entries.len()
                        == before.transcript.entries.len() + 3);
                },
                _ => assert(false),
            }
        } else {
            message_eof_constructor_append_count(before, i32::MAX, Seq::empty());
            assert(result.score.transcript.entries.len()
                == before.transcript.entries.len() + 2);
        }
        assert(result.score.transcript.entries
            == observed.score_entries_before + observed.appended_entries);
        assert(observed.score_entries_before == before.transcript.entries);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_primary_component_finish_from_source_is_exact(
    before: score_bridge::ScoreAdmissionBridge,
    observed: MessageEofPrimaryComponentSourceObservation,
)
    ensures match message_eof_primary_component_finish_from_source(before, observed) {
        None => true,
        Some(result) => {
            &&& result.component == observed.returned_component
            &&& result.component.complete
                == admission::admission_family_complete(before.admission.frames.last())
            &&& result.score.admission.frames.last().score_handle
                == Some(result.component.score_handle)
            &&& result.score.transcript.entries
                == observed.score_entries_before + observed.appended_entries
            &&& observed.appended_entries.len()
                == if result.component.complete
                    && result.component.family != admission::AdmissionFamily::Bundle
                { 3int } else { 2int }
            &&& score_bridge::bridge_score_finish_selected(before)
                == Some(result.score)
        },
    },
{
    message_eof_primary_component_finish_from_source_summary_exact(before, observed);
    message_eof_primary_component_finish_from_source_append_count_exact(before, observed);
}

/// This adapter consumes the single indexed-field/child-return fold. The
/// producer still owes a projection of the actual checked driver events into
/// `source.events`; the EOF semantic collections are not separate inputs.
#[cfg(rpb3_live_push)]
pub open spec fn message_eof_send_observation_from_fold(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
) -> Option<MessageEofSendSourceObservation> {
    if source.score_entries_after.len() < source.score_entries_before.len() {
        None
    } else {
    let folded = message_source::generic_message_source_fold(
        source.events, source.program_type_id, source.family,
        source.slot_count, source.body_len, source.indexed,
    );
    let suffix = source.score_entries_after.subrange(
        source.score_entries_before.len() as int,
        source.score_entries_after.len() as int,
    );
    if source.family != admission::AdmissionFamily::Send
        || source.program_type_id != 15
        || !message_source::generic_message_eof_source_matches_before(before, source)
        || source.score_entries_after != source.score_entries_before + suffix
        || folded.is_none()
        || source.component.is_none()
    {
        None
    } else {
        let trace = folded.unwrap();
        Some(MessageEofSendSourceObservation {
            program_type_id: source.program_type_id,
            cursor_after: source.cursor_after,
            slot_count: source.slot_count,
            seen: trace.seen,
            scalar_slots: trace.scalar_slots,
            scalar_occurrences: trace.scalar_occurrences,
            returns: trace.returns,
            score_entries_before: source.score_entries_before,
            appended_entries: suffix,
        })
    }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_send_observation_from_fold_matches_before(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
)
    ensures match message_eof_send_observation_from_fold(before, source) {
        None => true,
        Some(observed) => {
            &&& message_eof_send_source_matches_before(before, observed)
            &&& observed.score_entries_before + observed.appended_entries
                == source.score_entries_after
        },
    },
{
    reveal(message_source::generic_message_eof_source_matches_before);
    reveal(message_eof_send_source_matches_before);
    if let Some(observed) = message_eof_send_observation_from_fold(before, source) {
        let trace = message_source::generic_message_source_fold(
            source.events, source.program_type_id, source.family,
            source.slot_count, source.body_len, source.indexed,
        ).unwrap();
        assert(before.admission.frames.last().seen == trace.seen);
        assert(before.admission.frames.last().scalar_slots == trace.scalar_slots);
        assert(before.admission.frames.last().scalar_occurrences
            == trace.scalar_occurrences);
        assert(before.admission.frames.last().returns == trace.returns);
    }
}

/// Send's older finish image does not carry the returned Rust component as
/// an input. Check it against the component retained by the generic EOF
/// source before publishing a selected result.
#[cfg(rpb3_live_push)]
pub open spec fn message_eof_send_finish_from_fold(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
) -> Option<MessageEofSendFinishImage> {
    match message_eof_send_observation_from_fold(before, source) {
        None => None,
        Some(observed) => match (
            message_eof_send_finish_from_source(before, observed), source.component,
        ) {
            (Some(result), Some(actual)) if
                actual.family == result.component.family
                && actual.complete == result.component.complete
                && actual.score_handle == result.component.score_handle
                && actual.canonical_path_stable
                    == result.component.canonical_path_stable => Some(result),
            _ => None,
        },
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_send_finish_from_fold_selected_exact(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
)
    ensures match message_eof_send_finish_from_fold(before, source) {
        None => true,
        Some(result) => {
            &&& score_bridge::bridge_score_finish_selected(before)
                == Some(result.score)
            &&& result.score.transcript.entries == source.score_entries_after
            &&& source.component.is_some()
            &&& source.component.unwrap().family == result.component.family
            &&& source.component.unwrap().complete == result.component.complete
            &&& source.component.unwrap().score_handle
                == result.component.score_handle
            &&& source.component.unwrap().canonical_path_stable
                == result.component.canonical_path_stable
        },
    },
{
    message_eof_send_observation_from_fold_matches_before(before, source);
    if let Some(observed) = message_eof_send_observation_from_fold(before, source) {
        message_eof_send_finish_from_source_is_exact(before, observed);
    }
}

/// Bundle, If, and Match use the same checked event fold as Send. The score
/// suffix is sliced from the actual after-image, and the returned component
/// comes from the generic EOF source, not from a reducer-produced summary.
#[cfg(rpb3_live_push)]
pub open spec fn message_eof_primary_component_observation_from_fold(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
) -> Option<MessageEofPrimaryComponentSourceObservation> {
    if source.score_entries_after.len() < source.score_entries_before.len() {
        None
    } else {
    let folded = message_source::generic_message_source_fold(
        source.events, source.program_type_id, source.family,
        source.slot_count, source.body_len, source.indexed,
    );
    let suffix = source.score_entries_after.subrange(
        source.score_entries_before.len() as int,
        source.score_entries_after.len() as int,
    );
    if !message_source::generic_message_eof_source_matches_before(before, source)
        || !(match source.family {
            admission::AdmissionFamily::Bundle => source.program_type_id == 8,
            admission::AdmissionFamily::If => source.program_type_id == 23,
            admission::AdmissionFamily::Match => source.program_type_id == 22,
            _ => false,
        })
        || source.score_entries_after != source.score_entries_before + suffix
        || folded.is_none()
        || source.component.is_none()
    {
        None
    } else {
        let trace = folded.unwrap();
        let actual = source.component.unwrap();
        Some(MessageEofPrimaryComponentSourceObservation {
            program_type_id: source.program_type_id,
            cursor_after: source.cursor_after,
            slot_count: source.slot_count,
            seen: trace.seen,
            scalar_slots: trace.scalar_slots,
            scalar_occurrences: trace.scalar_occurrences,
            returns: trace.returns,
            score_entries_before: source.score_entries_before,
            appended_entries: suffix,
            returned_component: MessageEofPrimaryComponentImage {
                program_type_id: source.program_type_id,
                family: actual.family,
                complete: actual.complete,
                canonical_path_stable: actual.canonical_path_stable,
                score_handle: actual.score_handle,
            },
        })
    }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_primary_component_observation_from_fold_matches_before(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
)
    ensures match message_eof_primary_component_observation_from_fold(before, source) {
        None => true,
        Some(observed) => {
            &&& message_eof_primary_component_source_matches_before(before, observed)
            &&& observed.score_entries_before + observed.appended_entries
                == source.score_entries_after
            &&& source.component.is_some()
            &&& observed.returned_component.family == source.component.unwrap().family
            &&& observed.returned_component.complete == source.component.unwrap().complete
            &&& observed.returned_component.score_handle
                == source.component.unwrap().score_handle
            &&& observed.returned_component.canonical_path_stable
                == source.component.unwrap().canonical_path_stable
        },
    },
{
    reveal(message_source::generic_message_eof_source_matches_before);
    reveal(message_eof_primary_component_source_matches_before);
    if let Some(observed) =
        message_eof_primary_component_observation_from_fold(before, source)
    {
        let trace = message_source::generic_message_source_fold(
            source.events, source.program_type_id, source.family,
            source.slot_count, source.body_len, source.indexed,
        ).unwrap();
        assert(before.admission.frames.last().seen == trace.seen);
        assert(before.admission.frames.last().scalar_slots == trace.scalar_slots);
        assert(before.admission.frames.last().scalar_occurrences
            == trace.scalar_occurrences);
        assert(before.admission.frames.last().returns == trace.returns);
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_eof_primary_component_finish_from_fold_selected_exact(
    before: score_bridge::ScoreAdmissionBridge,
    source: message_source::GenericMessageEofSourceObservation,
)
    ensures match message_eof_primary_component_observation_from_fold(before, source) {
        None => true,
        Some(observed) => match message_eof_primary_component_finish_from_source(
            before, observed,
        ) {
            None => true,
            Some(result) => {
                &&& score_bridge::bridge_score_finish_selected(before)
                    == Some(result.score)
                &&& result.score.transcript.entries == source.score_entries_after
                &&& result.component == observed.returned_component
                &&& result.component.complete
                    == admission::admission_family_complete(before.admission.frames.last())
                &&& observed.appended_entries.len()
                    == if result.component.complete
                        && result.component.family != admission::AdmissionFamily::Bundle
                    { 3int } else { 2int }
            },
        },
    },
{
    message_eof_primary_component_observation_from_fold_matches_before(before, source);
    if let Some(observed) =
        message_eof_primary_component_observation_from_fold(before, source)
    {
        message_eof_primary_component_finish_from_source_is_exact(before, observed);
    }
}

#[cfg(rpb3_live_push)]
pub struct MessageChildCompleteSourceObservation {
    pub ghost cursor: wire_source::WireRuntimeCursorImage,
    pub stack_len_before: usize,
    pub score_entries_after_finish: usize,
    pub child_program_type_id: u16,
    pub parent_program_type_id: u16,
    pub parent_waiting_id: Option<u64>,
    pub child_return_id: Option<u64>,
    pub parent_returns_before: usize,
    pub child_score_handle: Option<usize>,
    pub ghost returned: admission::AdmissionReturnedKind,
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_child_complete_source_cursor_matches(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
) -> bool {
    &&& live_runtime_product_wf(before)
    &&& before.cursors.len() > 1
    &&& before.cursors.last() == Some(observed.cursor)
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_child_complete_source_frame_matches(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
) -> bool {
    let state = before.source.product.bridge.admission;
    if state.frames.len() <= 1 {
        false
    } else {
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames.last();
        &&& observed.stack_len_before as int == state.frames.len()
        &&& observed.score_entries_after_finish as int
            == before.source.product.bridge.transcript.entries.len()
        &&& child.frame_kind == 1
        &&& parent.frame_kind == 1
    }
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_child_complete_source_semantics_match(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
) -> bool {
    let state = before.source.product.bridge.admission;
    if state.frames.len() <= 1 {
        false
    } else {
        let parent = state.frames[state.frames.len() - 2];
        let child = state.frames.last();
        &&& observed.child_program_type_id == child.program_type_id
        &&& observed.parent_program_type_id == parent.program_type_id
        &&& observed.parent_waiting_id == parent.waiting_id
        &&& observed.child_return_id == child.return_id
        &&& observed.parent_returns_before as int == parent.returns.len()
        &&& observed.child_score_handle == child.score_handle
        &&& child.waiting_id.is_none()
        &&& admission::concrete_derived_summary_with_arena(
            parent, child, state.arena,
        ) == Some(observed.returned)
    }
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_child_complete_source_wire_end(
    observed: MessageChildCompleteSourceObservation,
) -> bool {
    wire_source::wire_runtime_next_field(observed.cursor)
        == Some(wire_source::WireConcreteTransition::End {
            state: wire_source::wire_runtime_derive_cursor(
                observed.cursor,
            ).unwrap(),
        })
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn message_child_complete_source_matches(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
) -> bool {
    &&& message_child_complete_source_cursor_matches(before, observed)
    &&& message_child_complete_source_frame_matches(before, observed)
    &&& message_child_complete_source_semantics_match(before, observed)
    &&& message_child_complete_source_wire_end(observed)
}

#[cfg(rpb3_live_push)]
#[verifier::opaque]
pub open spec fn live_runtime_message_child_complete_from_source(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
) -> Option<LiveRuntimeProductState> {
    if !message_child_complete_source_matches(before, observed)
        || !product_map_pop_owned(before.source.product)
    {
        None
    } else {
        match map::integrated_admission_driver_step(
            before.source.product.map,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ) {
            Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                state: next_map,
            }) => {
                let prior = before.source.product;
                let partial = ProductScoreState {
                    bridge: score_bridge::ScoreAdmissionBridge {
                        admission: next_map.admission,
                        transcript: next_map.transcript,
                    },
                    map: next_map,
                    epm_frames: prior.epm_frames.drop_last(),
                    indexed_regions: prior.indexed_regions.drop_last(),
                    accepted_epm_sources: Seq::empty(),
                };
                let next_product = ProductScoreState {
                    accepted_epm_sources: product_epm_receipts_carried(prior, partial),
                    ..partial
                };
                Some(LiveRuntimeProductState {
                    source: LiveSourceProductState {
                        product: next_product,
                        runtime: runtime_source::runtime_after_admission_step(
                            before.source.runtime, next_product.bridge.admission,
                        ),
                        arena: before.source.arena,
                        wire: before.source.wire.drop_last(),
                    },
                    cursors: before.cursors.drop_last(),
                })
            },
            _ => None,
        }
    }
}

#[cfg(rpb3_live_push)]
pub proof fn message_child_complete_source_match_components(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    requires message_child_complete_source_matches(before, observed),
    ensures
        message_child_complete_source_cursor_matches(before, observed),
        message_child_complete_source_frame_matches(before, observed),
        message_child_complete_source_semantics_match(before, observed),
        message_child_complete_source_wire_end(observed),
{
    reveal(message_child_complete_source_matches);
}

#[cfg(rpb3_live_push)]
pub proof fn message_child_complete_source_cursor_facts(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    requires message_child_complete_source_cursor_matches(before, observed),
    ensures
        live_runtime_product_wf(before),
        before.cursors.len() > 1,
        before.cursors.last() == Some(observed.cursor),
{
    reveal(message_child_complete_source_cursor_matches);
}

#[cfg(rpb3_live_push)]
pub proof fn message_child_complete_source_frame_facts(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    requires message_child_complete_source_frame_matches(before, observed),
    ensures
        before.source.product.bridge.admission.frames.len()
            == observed.stack_len_before as int,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind == 1,
        before.source.product.bridge.transcript.entries.len()
            == observed.score_entries_after_finish as int,
{
    reveal(message_child_complete_source_frame_matches);
}

#[cfg(rpb3_live_push)]
pub proof fn message_child_complete_source_wire_end_facts(
    observed: MessageChildCompleteSourceObservation,
)
    requires message_child_complete_source_wire_end(observed),
    ensures
        wire_source::wire_runtime_next_field(observed.cursor)
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    observed.cursor,
                ).unwrap(),
            }),
{
    reveal(message_child_complete_source_wire_end);
}

#[cfg(rpb3_live_push)]
pub proof fn message_child_complete_source_facts(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    requires message_child_complete_source_matches(before, observed),
    ensures
        live_runtime_product_wf(before),
        before.cursors.len() > 1,
        before.cursors.last() == Some(observed.cursor),
        before.source.product.bridge.admission.frames.len()
            == observed.stack_len_before as int,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
        before.source.product.bridge.admission.frames[
            before.source.product.bridge.admission.frames.len() - 2
        ].frame_kind == 1,
        before.source.product.bridge.transcript.entries.len()
            == observed.score_entries_after_finish as int,
        wire_source::wire_runtime_next_field(observed.cursor)
            == Some(wire_source::WireConcreteTransition::End {
                state: wire_source::wire_runtime_derive_cursor(
                    observed.cursor,
                ).unwrap(),
            }),
{
    message_child_complete_source_match_components(before, observed);
    message_child_complete_source_cursor_facts(before, observed);
    message_child_complete_source_frame_facts(before, observed);
    message_child_complete_source_wire_end_facts(observed);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_dispatch(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => {
            &&& message_child_complete_source_matches(before, observed)
            &&& product_map_pop_owned(before.source.product)
            &&& map::integrated_admission_driver_step(
                before.source.product.map,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            ) == Some(map::IntegratedAdmissionOutcome::MessageParentAccepted {
                state: after.source.product.map,
            })
            &&& after.source.product.bridge.admission
                == after.source.product.map.admission
            &&& after.source.product.bridge.transcript
                == after.source.product.map.transcript
            &&& after.source.product.indexed_regions
                == before.source.product.indexed_regions.drop_last()
            &&& after.source.product.epm_frames
                == before.source.product.epm_frames.drop_last()
            &&& after.source.runtime == runtime_source::runtime_after_admission_step(
                before.source.runtime, after.source.product.bridge.admission,
            )
            &&& after.source.arena == before.source.arena
            &&& after.source.wire == before.source.wire.drop_last()
            &&& after.cursors == before.cursors.drop_last()
        },
    },
{
    reveal(live_runtime_message_child_complete_from_source);
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_receipts(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => after.source.product.accepted_epm_sources
            == product_epm_receipts_carried(
                before.source.product, after.source.product,
            ),
    },
{
    live_runtime_message_child_complete_source_dispatch(before, observed);
    reveal(live_runtime_message_child_complete_from_source);
    if let Some(after) = live_runtime_message_child_complete_from_source(before, observed) {
        let partial = ProductScoreState {
            accepted_epm_sources: Seq::empty(),
            ..after.source.product
        };
        assert(after.source.product.accepted_epm_sources
            == product_epm_receipts_carried(before.source.product, partial));
        assert(product_epm_receipts_carried(before.source.product, partial)
            == product_epm_receipts_carried(
                before.source.product, after.source.product,
            ));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_map_product(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Complete,
            },
        ),
    },
{
    live_runtime_message_child_complete_source_dispatch(before, observed);
    live_runtime_message_child_complete_source_receipts(before, observed);
    if let Some(after) = live_runtime_message_child_complete_from_source(before, observed) {
        let action = map::IntegratedAdmissionAction::Message {
            action: admission::ConcreteFrameActionModel::Complete,
        };
        let outcome = map::IntegratedAdmissionOutcome::MessageParentAccepted {
            state: after.source.product.map,
        };
        assert(product_map_regions(before.source.product, action, outcome)
            == Some(after.source.product.indexed_regions));
        assert(product_map_epm_frames(before.source.product, outcome)
            == Some(after.source.product.epm_frames));
        assert(product_map_step(
            before.source.product, after.source.product, action,
        ));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_wire_indexed(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(_) => {
            &&& product_wire_indexed_source_agrees(
                observed.cursor, before.source.product.indexed_regions.last(),
            )
            &&& before.source.wire.last()
                == wire_source::wire_runtime_cursor_project(observed.cursor)
        },
    },
{
    live_runtime_message_child_complete_source_dispatch(before, observed);
    if let Some(_) = live_runtime_message_child_complete_from_source(before, observed) {
        message_child_complete_source_facts(before, observed);
        let index = before.cursors.len() - 1;
        assert(before.cursors[index] == Some(observed.cursor));
        assert(product_wire_indexed_source_agrees(
            observed.cursor, before.source.product.indexed_regions[index],
        ));
        assert(before.source.wire[index]
            == wire_source::wire_runtime_cursor_project(observed.cursor));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_live_step(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => live_runtime_product_message_child_complete_step(before, after),
    },
{
    live_runtime_message_child_complete_source_dispatch(before, observed);
    live_runtime_message_child_complete_source_map_product(before, observed);
    if let Some(after) = live_runtime_message_child_complete_from_source(before, observed) {
        message_child_complete_source_facts(before, observed);
        assert(live_runtime_product_message_child_complete_step(before, after));
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_source_post_shape(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => {
            &&& after.source.product.indexed_regions
                == before.source.product.indexed_regions.drop_last()
            &&& after.source.product.bridge.transcript.entries.len()
                == observed.score_entries_after_finish as int
            &&& after.source.product.bridge.admission.frames.len() + 1
                == observed.stack_len_before as int
            &&& after.source.product.bridge.admission.completed as int
                == before.source.product.bridge.admission.completed as int + 1
            &&& after.source.product.bridge.admission.frames.last().waiting_id.is_none()
        },
    },
{
    live_runtime_message_child_complete_source_dispatch(before, observed);
    if let Some(after) = live_runtime_message_child_complete_from_source(before, observed) {
        message_child_complete_source_facts(before, observed);
        map::integrated_message_parent_accept_pop_shape(
            before.source.product.map, after.source.product.map,
        );
    }
}

#[cfg(rpb3_live_push)]
pub proof fn live_runtime_message_child_complete_from_source_is_exact(
    before: LiveRuntimeProductState,
    observed: MessageChildCompleteSourceObservation,
)
    ensures match live_runtime_message_child_complete_from_source(before, observed) {
        None => true,
        Some(after) => {
            &&& product_wire_indexed_source_agrees(
                observed.cursor, before.source.product.indexed_regions.last(),
            )
            &&& before.source.wire.last()
                == wire_source::wire_runtime_cursor_project(observed.cursor)
            &&& product_map_step(
                before.source.product, after.source.product,
                map::IntegratedAdmissionAction::Message {
                    action: admission::ConcreteFrameActionModel::Complete,
                },
            )
            &&& live_runtime_product_message_child_complete_step(before, after)
            &&& after.source.product.indexed_regions
                == before.source.product.indexed_regions.drop_last()
            &&& after.source.product.bridge.transcript.entries.len()
                == observed.score_entries_after_finish as int
            &&& after.source.product.bridge.admission.frames.len() + 1
                == observed.stack_len_before as int
            &&& after.source.product.bridge.admission.completed as int
                == before.source.product.bridge.admission.completed as int + 1
            &&& after.source.product.bridge.admission.frames.last().waiting_id.is_none()
        },
    },
{
    live_runtime_message_child_complete_source_wire_indexed(before, observed);
    live_runtime_message_child_complete_source_map_product(before, observed);
    live_runtime_message_child_complete_source_live_step(before, observed);
    live_runtime_message_child_complete_source_post_shape(before, observed);
}

}
