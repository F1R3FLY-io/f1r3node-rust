//! CanonicalPath driver actions over one retained parser and runtime product.
//!
//! A CanonicalPath `step` call can scan many bytes before returning Push or
//! Complete. The common selector folds those observed scalar loop events from
//! the retained indexed input. A child Complete is observed after the child's
//! own work, at the pre-pop boundary; this module does not invent that work or
//! claim to derive its result from an arbitrary Rust execution.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission, rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_terminal_driver_observation as terminal_driver,
    rpb3_epathmap_event_refinement as epm, rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_terminal_refinement as terminal,
    rpb3_path_parser_refinement as parser, rpb3_wire_cursor_source_refinement as wire,
};

verus! {

/// The explicit product is the same object retained inside the common parser
/// image. Its equality is the synchronization invariant, not a projection
/// premise supplied separately at each transition.
pub struct CanonicalPathDriverProductState {
    pub ghost common: common::LiveCanonicalPathCommonState,
    pub ghost product: crate::LiveRuntimeProductState,
    pub canonical_events: nat,
    pub last_driver_step: Option<usize>,
}

pub open spec fn canonical_path_driver_product_sync(
    state: CanonicalPathDriverProductState,
) -> bool {
    state.product == state.common.live.base
}

pub open spec fn canonical_path_driver_product_wf(
    state: CanonicalPathDriverProductState,
) -> bool {
    &&& canonical_path_driver_product_sync(state)
    &&& common::live_canonical_path_common_at_driver_boundary(state.common)
}

/// Scalar facts at the source transition boundary. Push and root Complete use
/// the driver-call boundary. ChildReturn uses the completed child's pre-pop
/// score and frame image, after its own `step` has produced the summary; its
/// pre-call score may differ. The selected poststate is never a field here.
pub struct CanonicalPathDriverStorageObservation {
    pub step_index: usize,
    pub before_len: usize,
    pub after_len: usize,
    pub before_top_kind: u8,
    pub after_top_kind: Option<u8>,
    pub score_entries_before: usize,
    pub score_entries_after: usize,
}

pub open spec fn driver_storage_before_matches(
    state: CanonicalPathDriverProductState,
    observed: CanonicalPathDriverStorageObservation,
) -> bool {
    let admission = state.product.source.product.bridge.admission;
    &&& admission.frames.len() > 0
    &&& observed.before_len as int == admission.frames.len()
    &&& observed.before_top_kind == admission.frames.last().frame_kind
    &&& observed.score_entries_before as int
        == state.product.source.product.bridge.transcript.entries.len()
    &&& match state.last_driver_step {
        None => true,
        Some(previous) => previous < observed.step_index,
    }
}

pub open spec fn driver_storage_active_after_matches(
    product: crate::LiveRuntimeProductState,
    observed: CanonicalPathDriverStorageObservation,
) -> bool {
    let admission = product.source.product.bridge.admission;
    &&& admission.frames.len() > 0
    &&& observed.after_len as int == admission.frames.len()
    &&& observed.after_top_kind == Some(admission.frames.last().frame_kind)
    &&& observed.score_entries_after as int
        == product.source.product.bridge.transcript.entries.len()
}

/// These three variants are emitted by different source boundaries: the
/// CanonicalPath frame's Push, a completed child's driver pop/parent accept,
/// and the root CanonicalPath frame's terminal Complete. In ChildReturn the
/// `before` product is the completed child at its pre-pop boundary.
pub enum CanonicalPathDriverSourceAction {
    Push {
        storage_observed: CanonicalPathDriverStorageObservation,
        source_before: parser::ParserSourceSnapshot,
        loop_events: Seq<common::CanonicalPathLoopSourceEvent>,
        source_after_scan: parser::ParserSourceSnapshot,
        source: admission::AdmissionCanonicalPathPushSource,
        child: admission::AdmissionFrameState,
        storage: admission::AdmissionCanonicalPathPushStorage,
        child_image: epm::IndexedRegionImage,
        child_reader: Option<wire::WireRuntimeCursorImage>,
    },
    ChildReturn {
        storage_observed: CanonicalPathDriverStorageObservation,
        source_before_accept: parser::ParserSourceSnapshot,
        source_after_accept: parser::ParserSourceSnapshot,
        observed: path_live::LiveCanonicalPathReturnObservation,
    },
    RootComplete {
        storage_observed: CanonicalPathDriverStorageObservation,
        source_before: parser::ParserSourceSnapshot,
        loop_events: Seq<common::CanonicalPathLoopSourceEvent>,
        source_after_scan: parser::ParserSourceSnapshot,
        observed: terminal_driver::CanonicalPathTerminalDriverObservation,
    },
}

pub enum CanonicalPathDriverProductOutcome {
    Active { state: CanonicalPathDriverProductState },
    Terminal { returned: terminal::LiveCanonicalPathTerminalOutcome },
}

pub open spec fn canonical_path_driver_action_storage(
    action: CanonicalPathDriverSourceAction,
) -> CanonicalPathDriverStorageObservation {
    match action {
        CanonicalPathDriverSourceAction::Push { storage_observed, .. }
        | CanonicalPathDriverSourceAction::ChildReturn { storage_observed, .. }
        | CanonicalPathDriverSourceAction::RootComplete { storage_observed, .. } =>
            storage_observed,
    }
}

pub open spec fn driver_selected_active(
    before: CanonicalPathDriverProductState,
    selected: common::LiveCanonicalPathCommonState,
    observed: CanonicalPathDriverStorageObservation,
) -> Option<CanonicalPathDriverProductOutcome> {
    if !driver_storage_active_after_matches(selected.live.base, observed) {
        None
    } else {
        Some(CanonicalPathDriverProductOutcome::Active {
            state: CanonicalPathDriverProductState {
                common: selected,
                product: selected.live.base,
                canonical_events: before.canonical_events + 1,
                last_driver_step: Some(observed.step_index),
            },
        })
    }
}

/// The source-shaped action selects one of the already deterministic common
/// transitions. Neither an active post-product nor a terminal result is
/// caller-supplied. The remaining Rust extraction obligation is to show that
/// the real successful driver observation has these fields and that its child
/// pre-pop image matches the retained product.
pub open spec fn canonical_path_driver_product_step(
    before: CanonicalPathDriverProductState,
    action: CanonicalPathDriverSourceAction,
) -> Option<CanonicalPathDriverProductOutcome> {
    if !canonical_path_driver_product_wf(before) {
        None
    } else {
        match action {
            CanonicalPathDriverSourceAction::Push {
                storage_observed, source_before, loop_events,
                source_after_scan, source, child, storage, child_image, child_reader,
            } => {
                if !driver_storage_before_matches(before, storage_observed)
                    || storage_observed.before_top_kind != 4
                    || storage_observed.before_len as int + 1
                        != storage_observed.after_len as int
                { None } else {
                    match common::canonical_path_common_push_macro_from_loop_source(
                        before.common, storage_observed.step_index, source_before,
                        loop_events, source_after_scan, source, child, storage,
                        child_image, child_reader,
                    ) {
                        None => None,
                        Some(selected) => driver_selected_active(
                            before, selected, storage_observed,
                        ),
                    }
                }
            },
            CanonicalPathDriverSourceAction::ChildReturn {
                storage_observed, source_before_accept, source_after_accept,
                observed,
            } => {
                if !driver_storage_before_matches(before, storage_observed)
                    || storage_observed.before_len as int
                        != storage_observed.after_len as int + 1
                    || storage_observed.after_top_kind != Some(4)
                    || before.common.live.path_frames.len() < 2
                    || !common::canonical_path_source_boundary_matches(
                        before.common,
                        before.common.live.path_frames.len() - 2,
                        source_before_accept,
                    )
                { None } else {
                    match common::canonical_path_common_return_from_source(
                        before.common, observed,
                    ) {
                        None => None,
                        Some(selected) => {
                            let parent = selected.live.path_frames.len() - 1;
                            if !common::canonical_path_source_boundary_matches(
                                selected, parent, source_after_accept,
                            ) { None } else {
                                driver_selected_active(before, selected, storage_observed)
                            }
                        },
                    }
                }
            },
            CanonicalPathDriverSourceAction::RootComplete {
                storage_observed, source_before, loop_events,
                source_after_scan, observed,
            } => {
                if !driver_storage_before_matches(before, storage_observed)
                    || storage_observed.before_len != 1
                    || storage_observed.before_top_kind != 4
                    || storage_observed.after_len != 0
                    || storage_observed.after_top_kind.is_some()
                    || storage_observed.score_entries_after
                        != storage_observed.score_entries_before
                { None } else {
                    match common::canonical_path_common_root_complete_macro_from_loop_source(
                        before.common, storage_observed.step_index, source_before,
                        loop_events, source_after_scan, observed,
                    ) {
                        None => None,
                        Some(returned) => Some(
                            CanonicalPathDriverProductOutcome::Terminal { returned },
                        ),
                    }
                }
            },
        }
    }
}

/// A successful source-selected Push carries the checked live indexed push.
/// The appended child has no path parser; every old parser slot stays attached
/// to its original indexed region. The top path cursor is committed to the
/// observed scan length by the live selector.
pub proof fn common_push_selected_preserves_wf(
    scanned: common::LiveCanonicalPathCommonState,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
)
    requires common::live_canonical_path_common_wf(scanned),
    ensures match common::canonical_path_common_push_from_source(
        scanned, source, child, storage, child_image, child_reader,
    ) {
        Some(after) => {
            let n = scanned.parser_frames.len();
            &&& common::live_canonical_path_common_wf(after)
            &&& after.live.base.source.product.bridge.admission.frames.len()
                == scanned.live.base.source.product.bridge.admission.frames.len() + 1
            &&& after.live.base.source.product.bridge.transcript
                == scanned.live.base.source.product.bridge.transcript
            &&& after.live.base.source.product.indexed_regions[n - 1]
                == scanned.live.base.source.product.indexed_regions[n - 1]
            &&& after.live.path_frames[n - 1].unwrap().cursor as int
                == after.parser_frames[n - 1].unwrap().bytes.len()
        },
        None => true,
    },
{
    if let Some(after) = common::canonical_path_common_push_from_source(
        scanned, source, child, storage, child_image, child_reader,
    ) {
        let n = scanned.parser_frames.len();
        assert(n > 0);
        assert(path_live::live_canonical_path_push_source_ready(
            scanned.live, source, child, storage, child_image, child_reader,
        ));
        path_live::live_canonical_path_push_preserves_wf(
            scanned.live, source, child, storage, child_image, child_reader,
        );
        path_live::live_canonical_path_push_preserves_ancestors(
            scanned.live, source, child, storage, child_image, child_reader,
        );
        assert(path_live::live_canonical_path_wf(after.live));
        assert(after.parser_frames == scanned.parser_frames.push(None));
        assert(after.selected_values == scanned.selected_values.push(None));
        assert(after.live.path_frames[n - 1].unwrap().input
            == scanned.live.path_frames[n - 1].unwrap().input);
        assert(after.live.path_frames[n - 1].unwrap().cursor
            == source.cursor_after_step);
        assert(scanned.parser_frames[n - 1].unwrap().bytes.len()
            == source.cursor_after_step as int);
        assert forall|i: int| 0 <= i < after.parser_frames.len()
            implies common::parser_slot_matches(after, i) by {
            if i < n - 1 {
                assert(after.live.path_frames[i] == scanned.live.path_frames[i]);
                assert(after.parser_frames[i] == scanned.parser_frames[i]);
                assert(common::parser_slot_matches(scanned, i));
            } else if i == n - 1 {
                assert(after.parser_frames[i] == scanned.parser_frames[i]);
                assert(common::parser_slot_matches(scanned, i));
            } else {
                assert(i == n);
                assert(after.parser_frames[i].is_none());
                assert(after.live.path_frames[i].is_none());
            }
        }
        assert forall|i: int| 0 <= i < after.selected_values.len()
            implies match #[trigger] after.selected_values[i] {
                None => true,
                Some(gate) => match (after.live.path_frames[i], after.parser_frames[i]) {
                    (Some(frame), Some(slot)) => {
                        &&& slot.bytes.len() == frame.input.bytes.len()
                        &&& slot.parser.owners.len() == 0
                        &&& parser::parser_valid_finished_path(slot.parser.focus)
                        &&& gate == parser::parser_root_gate(slot.parser)
                    },
                    _ => false,
                },
            } by {
            if i < n - 1 {
                assert(after.live.path_frames[i] == scanned.live.path_frames[i]);
                assert(after.parser_frames[i] == scanned.parser_frames[i]);
                assert(after.selected_values[i] == scanned.selected_values[i]);
            } else {
                assert(after.selected_values[i].is_none());
            }
        }
        assert(common::live_canonical_path_common_wf(after));
    }
}

/// Each scalar/bulk loop event mutates only the active parser slot. This
/// structural fact is enough to carry ancestor driver boundaries through a
/// macro-step; it does not assume the parser's final state.
pub proof fn common_loop_fold_retains_ancestors(
    before: common::LiveCanonicalPathCommonState,
    events: Seq<common::CanonicalPathLoopSourceEvent>,
)
    ensures match common::canonical_path_loop_source_fold(before, events) {
        Some(after) => {
            &&& after.live == before.live
            &&& after.selected_values == before.selected_values
            &&& after.parser_frames.len() == before.parser_frames.len()
            &&& forall|i: int| 0 <= i < before.parser_frames.len() - 1
                ==> after.parser_frames[i] == before.parser_frames[i]
        },
        None => true,
    },
    decreases events.len(),
{
    if events.len() > 0 {
        if let Some(after) = common::canonical_path_loop_source_fold(before, events) {
            let first = common::canonical_path_loop_event_next(before, events[0]).unwrap();
            let tail = events.subrange(1, events.len() as int);
            assert(first.live == before.live);
            assert(first.selected_values == before.selected_values);
            assert(first.parser_frames.len() == before.parser_frames.len());
            assert forall|i: int| 0 <= i < before.parser_frames.len() - 1
                implies first.parser_frames[i] == before.parser_frames[i] by {}
            common_loop_fold_retains_ancestors(first, tail);
            assert(common::canonical_path_loop_source_fold(first, tail) == Some(after));
            assert forall|i: int| 0 <= i < before.parser_frames.len() - 1
                implies after.parser_frames[i] == before.parser_frames[i] by {}
        }
    }
}

/// Settling completed Region owners rewrites focus and owner residuals, but
/// never advances the indexed path cursor. This induction includes arbitrary
/// owner depth, including nested EPathMap payloads.
pub proof fn parser_settle_retains_stamp(image: parser::ParserImage)
    ensures parser::settle_parser_owners(image).stamp == image.stamp,
    decreases image.owners.len(),
{
    if image.owners.len() > 0 {
        let owner = image.owners.last();
        if image.focus.status != parser::ParserStatus::Dead
            && owner.path_remaining == 0
            && !parser::parser_waiting_obligation(image.focus)
        {
            let parent = if image.focus.status == parser::ParserStatus::Dead
                || !parser::parser_valid_finished_path(image.focus)
                || !image.focus.stable
                || owner.child_bytes.len() != owner.declared_len as int
                || match owner.previous {
                    Some(previous) => !parser::parser_bytes_less(previous, owner.child_bytes),
                    None => false,
                } {
                parser::parser_dead(owner.parent)
            } else if owner.remaining == 0 {
                parser::bubble_parser(parser::pop_parser_top(owner.parent), true)
            } else {
                parser::replace_parser_top(
                    owner.parent,
                    parser::ParserFrame::RegionLength {
                        remaining: owner.remaining,
                        previous: Some(owner.child_bytes),
                        state: parser::initial_varint(),
                    },
                )
            };
            let next = parser::ParserImage {
                focus: parent,
                owners: image.owners.drop_last(),
                ..image
            };
            parser_settle_retains_stamp(next);
            assert(next.stamp == image.stamp);
            assert(parser::settle_parser_owners(image)
                == parser::settle_parser_owners(next));
        }
    }
}

pub proof fn parser_resolution_retains_stamp(
    image: parser::ParserImage,
    id: u64,
    resolution: parser::ParserResolution,
)
    requires image.focus.stack.len() > 0,
    ensures parser::resolve_parser_obligation(image, id, resolution).stamp == image.stamp,
{
    let path = image.focus;
    let focus = match path.stack.last() {
        parser::ParserFrame::Obligation {
            id: active_id,
            kind,
            status: parser::ParserObligationStatus::AwaitingResolution { .. },
            ..
        } if active_id == id => match (kind, resolution) {
            (
                parser::ParserObligationKind::RawParEscape,
                parser::ParserResolution::RawPar { canonical_path_stable: false },
            ) => parser::bubble_parser(parser::pop_parser_top(path), false),
            (
                parser::ParserObligationKind::NestedEpm1,
                parser::ParserResolution::NestedEpm1 {
                    is_map: true,
                    entries_stable: true,
                },
            ) => parser::bubble_parser(parser::pop_parser_top(path), true),
            _ => parser::parser_dead(path),
        },
        _ => parser::parser_dead(path),
    };
    let settling = parser::ParserImage { focus, ..image };
    parser_settle_retains_stamp(settling);
    let selected = parser::settle_parser_owners(settling);
    assert(selected.stamp == image.stamp);
    if selected.focus.status == parser::ParserStatus::Dead
        && image.owners.len() > 0
    {
        assert(parser::resolve_parser_obligation(image, id, resolution).stamp
            == image.stamp);
    } else {
        assert(parser::resolve_parser_obligation(image, id, resolution) == selected);
    }
}

/// Resolution changes the parent parser focus and gate, but never consumes a
/// new byte or changes its indexed source. The live return selector removes
/// exactly the completed child while retaining that parent's cursor/anchors.
pub proof fn common_return_selected_preserves_boundary(
    before: common::LiveCanonicalPathCommonState,
    observed: path_live::LiveCanonicalPathReturnObservation,
)
    requires common::live_canonical_path_common_at_driver_boundary(before),
    ensures match common::canonical_path_common_return_from_source(before, observed) {
        Some(after) => {
            let parent = before.parser_frames.len() - 2;
            &&& common::live_canonical_path_common_at_driver_boundary(after)
            &&& after.live.base.source.product.bridge.admission.frames.len() + 1
                == before.live.base.source.product.bridge.admission.frames.len()
            &&& after.live.base.source.product.bridge.transcript
                == before.live.base.source.product.bridge.transcript
            &&& after.live.base.source.product.indexed_regions[parent]
                == before.live.base.source.product.indexed_regions[parent]
            &&& after.parser_frames[parent].unwrap().bytes
                == before.parser_frames[parent].unwrap().bytes
            &&& after.parser_frames[parent].unwrap().positions
                == before.parser_frames[parent].unwrap().positions
        },
        None => true,
    },
{
    if let Some(after) = common::canonical_path_common_return_from_source(before, observed) {
        let n = before.parser_frames.len();
        let parent = n - 2;
        assert(path_live::live_canonical_path_return_source_ready(before.live, observed));
        path_live::live_canonical_path_return_preserves_wf(before.live, observed);
        path_live::live_canonical_path_return_preserves_ancestors(before.live, observed);
        assert(path_live::live_canonical_path_wf(after.live));
        let old_slot = before.parser_frames[parent].unwrap();
        let next_slot = after.parser_frames[parent].unwrap();
        assert(old_slot.parser.focus.stack.len() > 0);
        parser_resolution_retains_stamp(
            old_slot.parser, observed.reducer_waiting_id,
            common::parser_resolution_from_return(observed).unwrap(),
        );
        assert(next_slot.bytes == old_slot.bytes);
        assert(next_slot.positions == old_slot.positions);
        assert(next_slot.cursor_traversed == old_slot.cursor_traversed);
        assert(next_slot.parser.stamp == old_slot.parser.stamp);
        assert(after.selected_values == before.selected_values.drop_last());
        assert(before.selected_values[parent].is_none());
        assert forall|i: int| 0 <= i < after.parser_frames.len()
            implies common::parser_slot_matches(after, i) by {
            if i < parent {
                assert(after.live.path_frames[i] == before.live.path_frames[i]);
                assert(after.parser_frames[i] == before.parser_frames[i]);
                assert(common::parser_slot_matches(before, i));
            } else {
                assert(i == parent);
                assert(after.live.path_frames[i].unwrap().input
                    == before.live.path_frames[i].unwrap().input);
                assert(after.live.path_frames[i].unwrap().cursor
                    == before.live.path_frames[i].unwrap().cursor);
                assert(common::parser_slot_matches(before, i));
            }
        }
        assert forall|i: int| 0 <= i < after.selected_values.len()
            implies match #[trigger] after.selected_values[i] {
                None => true,
                Some(gate) => match (after.live.path_frames[i], after.parser_frames[i]) {
                    (Some(frame), Some(slot)) => {
                        &&& slot.bytes.len() == frame.input.bytes.len()
                        &&& slot.parser.owners.len() == 0
                        &&& parser::parser_valid_finished_path(slot.parser.focus)
                        &&& gate == parser::parser_root_gate(slot.parser)
                    },
                    _ => false,
                },
            } by {
            if i < parent {
                assert(after.live.path_frames[i] == before.live.path_frames[i]);
                assert(after.parser_frames[i] == before.parser_frames[i]);
                assert(after.selected_values[i] == before.selected_values[i]);
            } else {
                assert(after.selected_values[i].is_none());
            }
        }
        assert(common::live_canonical_path_common_wf(after));
        assert forall|i: int| 0 <= i < after.parser_frames.len()
            implies match (#[trigger] after.parser_frames[i], after.live.path_frames[i]) {
                (Some(slot), Some(frame)) => frame.cursor as int == slot.bytes.len(),
                _ => true,
            } by {
            if i < parent {
                assert(before.parser_frames[i] == after.parser_frames[i]);
                assert(before.live.path_frames[i] == after.live.path_frames[i]);
            } else {
                assert(i == parent);
                assert(before.live.path_frames[i].unwrap().cursor
                    == after.live.path_frames[i].unwrap().cursor);
                assert(before.parser_frames[i].unwrap().bytes
                    == after.parser_frames[i].unwrap().bytes);
            }
        }
        assert(common::live_canonical_path_common_at_driver_boundary(after));
    }
}

pub proof fn common_push_macro_preserves_boundary(
    before: common::LiveCanonicalPathCommonState,
    driver_step_index: usize,
    source_before: parser::ParserSourceSnapshot,
    loop_events: Seq<common::CanonicalPathLoopSourceEvent>,
    source_after_scan: parser::ParserSourceSnapshot,
    source: admission::AdmissionCanonicalPathPushSource,
    child: admission::AdmissionFrameState,
    storage: admission::AdmissionCanonicalPathPushStorage,
    child_image: epm::IndexedRegionImage,
    child_reader: Option<wire::WireRuntimeCursorImage>,
)
    requires common::live_canonical_path_common_at_driver_boundary(before),
    ensures match common::canonical_path_common_push_macro_from_loop_source(
        before, driver_step_index, source_before, loop_events, source_after_scan,
        source, child, storage, child_image, child_reader,
    ) {
        Some(after) => {
            &&& common::live_canonical_path_common_at_driver_boundary(after)
            &&& after.live.base.source.product.bridge.admission.frames.len()
                == before.live.base.source.product.bridge.admission.frames.len() + 1
            &&& after.live.base.source.product.bridge.transcript
                == before.live.base.source.product.bridge.transcript
            &&& after.live.base.source.product.indexed_regions[
                before.live.path_frames.len() - 1]
                == before.live.base.source.product.indexed_regions[
                    before.live.path_frames.len() - 1]
        },
        None => true,
    },
{
    if let Some(after) = common::canonical_path_common_push_macro_from_loop_source(
        before, driver_step_index, source_before, loop_events, source_after_scan,
        source, child, storage, child_image, child_reader,
    ) {
        let scanned = common::canonical_path_loop_source_fold(before, loop_events).unwrap();
        let n = before.parser_frames.len();
        assert(scanned.live == before.live);
        assert(common::canonical_path_source_boundary_matches(
            scanned, n - 1, source_after_scan,
        ));
        assert(common::live_canonical_path_common_wf(scanned));
        common_loop_fold_retains_ancestors(before, loop_events);
        common_push_selected_preserves_wf(
            scanned, source, child, storage, child_image, child_reader,
        );
        assert(common::canonical_path_common_push_from_source(
            scanned, source, child, storage, child_image, child_reader,
        ) == Some(after));
        assert(common::live_canonical_path_common_wf(after));
        assert forall|i: int| 0 <= i < after.parser_frames.len()
            implies match (#[trigger] after.parser_frames[i], after.live.path_frames[i]) {
                (Some(slot), Some(frame)) => frame.cursor as int == slot.bytes.len(),
                _ => true,
            } by {
            if i < n - 1 {
                assert(scanned.parser_frames[i] == before.parser_frames[i]);
                assert(scanned.live.path_frames[i] == before.live.path_frames[i]);
                assert(after.parser_frames[i] == scanned.parser_frames[i]);
                assert(after.live.path_frames[i] == scanned.live.path_frames[i]);
            } else if i == n - 1 {
                assert(after.live.path_frames[i].unwrap().cursor as int
                    == after.parser_frames[i].unwrap().bytes.len());
            } else {
                assert(i == n);
                assert(after.parser_frames[i].is_none());
                assert(after.live.path_frames[i].is_none());
            }
        }
        assert(common::live_canonical_path_common_at_driver_boundary(after));
    }
}

/// The selected common state is the only active poststate. Both action arms
/// preserve the full parser/live invariant, not merely equality of two ghost
/// product fields. The terminal arm remains a no-parent outcome.
pub proof fn canonical_path_driver_product_active_preserves_wf(
    before: CanonicalPathDriverProductState,
    action: CanonicalPathDriverSourceAction,
)
    requires canonical_path_driver_product_wf(before),
    ensures match canonical_path_driver_product_step(before, action) {
        Some(CanonicalPathDriverProductOutcome::Active { state }) =>
            canonical_path_driver_product_wf(state),
        _ => true,
    },
{
    match action {
        CanonicalPathDriverSourceAction::Push {
            storage_observed, source_before, loop_events, source_after_scan,
            source, child, storage, child_image, child_reader,
        } => {
            common_push_macro_preserves_boundary(
                before.common, storage_observed.step_index, source_before,
                loop_events, source_after_scan, source, child, storage,
                child_image, child_reader,
            );
        },
        CanonicalPathDriverSourceAction::ChildReturn {
            observed, ..
        } => {
            common_return_selected_preserves_boundary(before.common, observed);
        },
        CanonicalPathDriverSourceAction::RootComplete { .. } => {},
    }
}

pub proof fn canonical_path_driver_product_active_sync(
    before: CanonicalPathDriverProductState,
    action: CanonicalPathDriverSourceAction,
)
    ensures match canonical_path_driver_product_step(before, action) {
        Some(CanonicalPathDriverProductOutcome::Active { state }) => {
            &&& canonical_path_driver_product_sync(state)
            &&& state.canonical_events == before.canonical_events + 1
            &&& state.last_driver_step
                == Some(canonical_path_driver_action_storage(action).step_index)
            &&& driver_storage_before_matches(
                before, canonical_path_driver_action_storage(action),
            )
            &&& driver_storage_active_after_matches(
                state.product, canonical_path_driver_action_storage(action),
            )
        },
        Some(CanonicalPathDriverProductOutcome::Terminal { .. }) => {
            let storage = canonical_path_driver_action_storage(action);
            &&& driver_storage_before_matches(before, storage)
            &&& storage.before_len == 1
            &&& storage.before_top_kind == 4
            &&& storage.after_len == 0
            &&& storage.after_top_kind.is_none()
            &&& storage.score_entries_after == storage.score_entries_before
        },
        None => true,
    },
{
}

/// The synchronized dispatcher has a concrete successful terminal path. The
/// common proof supplies the byte-loop fold and EOF value; this witness only
/// adds the source-observed driver storage boundary and product equality.
pub proof fn boolean_root_driver_product_witness()
    ensures {
        let common_root = common::live_canonical_path_common_root_from_source(
            common::boolean_root_source(),
        ).unwrap();
        let before = CanonicalPathDriverProductState {
            common: common_root,
            product: common_root.live.base,
            canonical_events: 0,
            last_driver_step: None,
        };
        let storage = CanonicalPathDriverStorageObservation {
            step_index: 0,
            before_len: 1,
            after_len: 0,
            before_top_kind: 4,
            after_top_kind: None,
            score_entries_before: 0,
            score_entries_after: 0,
        };
        let action = CanonicalPathDriverSourceAction::RootComplete {
            storage_observed: storage,
            source_before: parser::initial_parser_source_snapshot(),
            loop_events: seq![common::boolean_root_loop_event()],
            source_after_scan: common::boolean_root_source_snapshot(),
            observed: common::boolean_root_driver_observation(),
        };
        &&& canonical_path_driver_product_wf(before)
        &&& canonical_path_driver_product_step(before, action).is_some()
    },
{
    common::boolean_root_common_witness();
    common::boolean_root_common_dispatch_complete_witness();
    let common_root = common::live_canonical_path_common_root_from_source(
        common::boolean_root_source(),
    ).unwrap();
    let before = CanonicalPathDriverProductState {
        common: common_root,
        product: common_root.live.base,
        canonical_events: 0,
        last_driver_step: None,
    };
    let storage = CanonicalPathDriverStorageObservation {
        step_index: 0,
        before_len: 1,
        after_len: 0,
        before_top_kind: 4,
        after_top_kind: None,
        score_entries_before: 0,
        score_entries_after: 0,
    };
    assert(common::live_canonical_path_common_at_driver_boundary(common_root));
    assert(driver_storage_before_matches(before, storage));
    assert(common::canonical_path_common_root_complete_macro_from_loop_source(
        common_root, 0, parser::initial_parser_source_snapshot(),
        seq![common::boolean_root_loop_event()],
        common::boolean_root_source_snapshot(),
        common::boolean_root_driver_observation(),
    ).is_some());
}

}
