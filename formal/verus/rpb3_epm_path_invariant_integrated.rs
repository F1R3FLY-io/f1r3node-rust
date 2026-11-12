//! The EPathMap path invariant over the integrated live admission product.
//!
//! `product_epm_paths_wf` states that every `Some` EPathMap frame of the live
//! product's `epm_frames` satisfies `epm_frame_path_wf`
//! (`rpb3_epm_frame_path_invariant`), and `integrated_epm_paths_wf` lifts it to
//! the integrated state the way `integrated_wf` lifts product well-formedness:
//! an accepted state names the product that produced it.
//!
//! This file proves that the CanonicalPath driver actions and the ACT Forward
//! and Unwind batches of `integrated_step` preserve the invariant:
//! - CanonicalPath driver actions never rewrite an EPathMap frame. Their
//!   parsers live in the common state's own parser slots. A Push appends a
//!   child slot without an EPathMap frame, a ChildReturn drops the completed
//!   child's slot, and a root Complete accepts the unchanged product.
//! - An ACT Forward batch replaces the active frame by `forward_batch_after`.
//!   `forward_parser_payload` changes only the stamp and the Region owners'
//!   residual counters and child bytes, so the focused path, the fault flag and
//!   every owner's parent stay fixed, and with them every parser fact the
//!   invariant reads. The new pending obligation and each published checkpoint
//!   repeat the focused Payload obligation, which does not await resolution.
//!   The path grows by `width` bytes while the cursor advances by `2 * width`
//!   events.
//! - An ACT Unwind batch replaces the active frame by `unwind_batch_after`. The
//!   checkpoint it selects is the restore entry at the new path length, so the
//!   checkpoint invariant of the prestate supplies the restored parser, pending
//!   obligation and gate. The parser is restored with the saved focus and
//!   owners (`restore_parser_checkpoint`); without that restore the Unwind case
//!   fails. The path shrinks by `width` bytes while the cursor advances by
//!   `width` events.
//!
//! The sequence lemmas (`epm_frames_paths_wf_push_none`,
//! `epm_frames_paths_wf_prefix`, `epm_frames_paths_wf_update`) cover the three
//! ways an integrated action edits `epm_frames`: a pushed child slot, popped
//! child slots, and one replaced frame.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_act_source_product_integration as act,
    rpb3_canonical_path_common_product as common,
    rpb3_canonical_path_driver_product_integration as path_driver,
    rpb3_epathmap_event_refinement as epm,
    rpb3_epm_active_line_work_source as active,
    rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_batch_cursor_source_extraction as act_source,
    rpb3_epm_frame_path_invariant as frame_path,
    rpb3_epm_parser_escape_invariant as escape,
    rpb3_epm_restore_run_expansion as restore,
    rpb3_epm_unwind_event_lift as unwind,
    rpb3_epm_unwind_product_lift as unwind_product,
    rpb3_integrated_dispatcher as dispatcher,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_path_parser_refinement as parser,
    LiveRuntimeProductState, ProductScoreState,
};

verus! {

/// One EPathMap slot of the product: no frame, or a frame that satisfies the
/// path invariant.
pub open spec fn epm_slot_path_wf(slot: Option<epm::EpmFrame>) -> bool {
    match slot {
        Some(frame) => frame_path::epm_frame_path_wf(frame),
        None => true,
    }
}

/// Every slot of an `epm_frames` sequence satisfies `epm_slot_path_wf`.
pub open spec fn epm_frames_paths_wf(frames: Seq<Option<epm::EpmFrame>>) -> bool {
    forall|i: int| 0 <= i < frames.len() ==> epm_slot_path_wf(#[trigger] frames[i])
}

/// Every `Some` EPathMap frame of the live product satisfies
/// `epm_frame_path_wf`.
pub open spec fn product_epm_paths_wf(state: LiveRuntimeProductState) -> bool {
    epm_frames_paths_wf(state.source.product.epm_frames)
}

/// `product_epm_paths_wf` of the integrated state's product. An accepted state
/// names the product that produced it, as in `integrated_accepted_wf`.
pub open spec fn integrated_epm_paths_wf(state: dispatcher::IntegratedState) -> bool {
    match state {
        dispatcher::IntegratedState::Active { product } => product_epm_paths_wf(product),
        dispatcher::IntegratedState::Accepted { accepted } => match accepted {
            dispatcher::IntegratedAccepted::LiveRoot { post_root_score, .. } =>
                product_epm_paths_wf(post_root_score),
            dispatcher::IntegratedAccepted::LiveNoScoreRoot { pre_complete, .. } =>
                product_epm_paths_wf(pre_complete),
            dispatcher::IntegratedAccepted::CanonicalPathRoot { pre_complete, .. } =>
                product_epm_paths_wf(pre_complete),
        },
    }
}

/// Two parser images with the same focused path, fault flag, Region owner
/// count and owner parents. Every parser fact of the path invariant reads only
/// these fields.
pub open spec fn parser_same_path_shape(
    left: parser::ParserImage,
    right: parser::ParserImage,
) -> bool {
    &&& left.focus == right.focus
    &&& left.fault == right.fault
    &&& left.owners.len() == right.owners.len()
    &&& forall|k: int| 0 <= k < left.owners.len()
        ==> (#[trigger] left.owners[k]).parent == right.owners[k].parent
}

/// Appending a slot without a frame keeps the invariant.
pub proof fn epm_frames_paths_wf_push_none(frames: Seq<Option<epm::EpmFrame>>)
    requires
        epm_frames_paths_wf(frames),
    ensures
        epm_frames_paths_wf(frames.push(None)),
{
    let next = frames.push(None);
    assert forall|i: int| 0 <= i < next.len() implies epm_slot_path_wf(#[trigger] next[i]) by {
        if i < frames.len() {
            assert(next[i] == frames[i]);
        }
    }
}

/// A prefix keeps the invariant.
pub proof fn epm_frames_paths_wf_prefix(frames: Seq<Option<epm::EpmFrame>>, len: int)
    requires
        epm_frames_paths_wf(frames),
        0 <= len <= frames.len(),
    ensures
        epm_frames_paths_wf(frames.subrange(0, len)),
{
    let next = frames.subrange(0, len);
    assert forall|i: int| 0 <= i < next.len() implies epm_slot_path_wf(#[trigger] next[i]) by {
        assert(next[i] == frames[i]);
    }
}

/// Replacing one slot by a frame that satisfies the path invariant keeps the
/// invariant.
pub proof fn epm_frames_paths_wf_update(
    frames: Seq<Option<epm::EpmFrame>>,
    index: int,
    frame: epm::EpmFrame,
)
    requires
        epm_frames_paths_wf(frames),
        0 <= index < frames.len(),
        frame_path::epm_frame_path_wf(frame),
    ensures
        epm_frames_paths_wf(frames.update(index, Some(frame))),
{
    let next = frames.update(index, Some(frame));
    assert forall|i: int| 0 <= i < next.len() implies epm_slot_path_wf(#[trigger] next[i]) by {
        if i != index {
            assert(next[i] == frames[i]);
        }
    }
}

/// Parser images of the same path shape agree on the root, the root gate and
/// the root potential, and every parser fact of the path invariant carries
/// from `left` to `right`: the escape invariant, the obligation layout, clause
/// (b) for `pending`, and the gate agreement for `gate`.
pub proof fn parser_same_path_shape_keeps_facts(
    left: parser::ParserImage,
    right: parser::ParserImage,
    pending: Option<epm::PathObligation>,
    gate: epm::PathGate,
)
    requires
        parser_same_path_shape(left, right),
    ensures
        parser::parser_root(right) == parser::parser_root(left),
        parser::parser_root_gate(right) == parser::parser_root_gate(left),
        frame_path::parser_root_potential(right) == frame_path::parser_root_potential(left),
        escape::parser_image_escape_wf(left) ==> escape::parser_image_escape_wf(right),
        frame_path::parser_image_obligation_wf(left)
            ==> frame_path::parser_image_obligation_wf(right),
        frame_path::epm_pending_matches_focus(pending, left)
            ==> frame_path::epm_pending_matches_focus(pending, right),
        epm::parser_gate_matches(gate, left) ==> epm::parser_gate_matches(gate, right),
{
    if left.owners.len() > 0 {
        assert(left.owners[0].parent == right.owners[0].parent);
    }
    assert(parser::parser_root(right) == parser::parser_root(left));
    assert(parser::parser_root_gate(right) == parser::parser_root_gate(left));
    if escape::parser_image_escape_wf(left) {
        assert forall|k: int| 0 <= k < right.owners.len()
            implies escape::parser_path_escape_wf(#[trigger] right.owners[k].parent, k == 0) by {
            assert(left.owners[k].parent == right.owners[k].parent);
        }
        assert forall|k: int| 0 <= k < right.owners.len()
            implies (#[trigger] right.owners[k].parent).stack.len() > 0 by {
            assert(left.owners[k].parent == right.owners[k].parent);
        }
    }
    if frame_path::parser_image_obligation_wf(left) {
        assert forall|k: int| 0 <= k < right.owners.len()
            implies frame_path::parser_owner_parent_wf(#[trigger] right.owners[k]) by {
            assert(left.owners[k].parent == right.owners[k].parent);
            assert(frame_path::parser_owner_parent_wf(left.owners[k]));
        }
    }
}

/// A bulk Forward payload step keeps the path shape: only the stamp and the
/// owners' residual counters and child bytes change.
pub proof fn parser_forward_payload_same_shape(image: parser::ParserImage, bytes: Seq<u8>)
    ensures
        parser_same_path_shape(image, parser::forward_parser_payload(image, bytes)),
{
    let forwarded = parser::forward_parser_payload(image, bytes);
    assert forall|k: int| 0 <= k < image.owners.len()
        implies (#[trigger] image.owners[k]).parent == forwarded.owners[k].parent by {}
}

/// Restoring a checkpoint keeps the saved image's path shape: only the global
/// obligation counter comes from the current image.
pub proof fn parser_restore_checkpoint_same_shape(
    saved: parser::ParserImage,
    current: parser::ParserImage,
)
    ensures
        parser_same_path_shape(saved, parser::restore_parser_checkpoint(saved, current)),
{
    let restored = parser::restore_parser_checkpoint(saved, current);
    assert(restored.owners == saved.owners);
    assert forall|k: int| 0 <= k < saved.owners.len()
        implies (#[trigger] saved.owners[k]).parent == restored.owners[k].parent by {}
}

/// A well-formed driver image is synchronized with its product, has one path
/// slot per admission frame, and the product has one EPathMap slot per
/// admission frame.
pub proof fn canonical_path_driver_epm_lengths(
    state: path_driver::CanonicalPathDriverProductState,
)
    requires
        path_driver::canonical_path_driver_product_wf(state),
    ensures
        state.product == state.common.live.base,
        state.common.live.path_frames.len()
            == state.product.source.product.bridge.admission.frames.len(),
        state.product.source.product.epm_frames.len()
            == state.product.source.product.bridge.admission.frames.len(),
{
    assert(common::live_canonical_path_common_wf(state.common));
    assert(path_live::live_canonical_path_wf(state.common.live));
    assert(integrated::live_runtime_product_wf(state.product));
    assert(integrated::live_source_product_wf(state.product.source));
    assert(integrated::product_score_wf(state.product.source.product));
}

/// A successful CanonicalPath Push appends one slot without an EPathMap frame:
/// the loop fold keeps the live product, and the indexed push appends `None`.
pub proof fn canonical_path_push_epm_frames(
    driver_before: path_driver::CanonicalPathDriverProductState,
    driver_action: path_driver::CanonicalPathDriverSourceAction,
)
    requires
        driver_action is Push,
    ensures
        match path_driver::canonical_path_driver_product_step(driver_before, driver_action) {
            Some(path_driver::CanonicalPathDriverProductOutcome::Active { state }) =>
                state.product.source.product.epm_frames
                    == driver_before.product.source.product.epm_frames.push(None),
            Some(path_driver::CanonicalPathDriverProductOutcome::Terminal { .. }) => false,
            None => true,
        },
{
    hide(path_driver::driver_storage_before_matches);
    hide(path_driver::driver_storage_active_after_matches);
    hide(common::live_canonical_path_common_at_driver_boundary);
    hide(common::live_canonical_path_common_wf);
    hide(common::canonical_path_source_boundary_matches);
    hide(common::canonical_path_loop_source_fold);
    hide(common::parser_obligation_matches_push);
    hide(common::canonical_path_common_return_from_source);
    hide(common::canonical_path_common_root_complete_macro_from_loop_source);
    hide(path_live::live_canonical_path_push_source_ready);
    match driver_action {
        path_driver::CanonicalPathDriverSourceAction::Push {
            storage_observed, source_before, loop_events, source_after_scan,
            source, child, storage, child_image, child_reader,
        } => {
            let common_before = driver_before.common;
            if path_driver::canonical_path_driver_product_step(driver_before, driver_action)
                .is_some() {
                assert(path_driver::canonical_path_driver_product_wf(driver_before));
                assert(driver_before.product == common_before.live.base);
                let selected = common::canonical_path_common_push_macro_from_loop_source(
                    common_before, storage_observed.step_index, source_before,
                    loop_events, source_after_scan, source, child, storage,
                    child_image, child_reader,
                );
                assert(selected.is_some());
                let scanned = common::canonical_path_loop_source_fold(
                    common_before, loop_events,
                ).unwrap();
                assert(scanned.live == common_before.live);
                let pushed = path_live::live_canonical_path_push_from_source(
                    scanned.live, source, child, storage, child_image, child_reader,
                );
                assert(pushed.is_some());
                assert(selected.unwrap().live == pushed.unwrap());
                assert(pushed.unwrap().base.source.product.epm_frames
                    == scanned.live.base.source.product.epm_frames.push(None));
            }
        },
        _ => {},
    }
}

/// A successful CanonicalPath ChildReturn drops the completed child's slot:
/// the product keeps the strict prefix of its EPathMap slots up to the parent.
pub proof fn canonical_path_child_return_epm_frames(
    driver_before: path_driver::CanonicalPathDriverProductState,
    driver_action: path_driver::CanonicalPathDriverSourceAction,
)
    requires
        driver_action is ChildReturn,
    ensures
        match path_driver::canonical_path_driver_product_step(driver_before, driver_action) {
            Some(path_driver::CanonicalPathDriverProductOutcome::Active { state }) => {
                let frames = driver_before.product.source.product.epm_frames;
                &&& frames.len() >= 2
                &&& state.product.source.product.epm_frames
                    == frames.subrange(0, frames.len() - 1)
            },
            Some(path_driver::CanonicalPathDriverProductOutcome::Terminal { .. }) => false,
            None => true,
        },
{
    hide(path_driver::canonical_path_driver_product_wf);
    hide(path_driver::driver_storage_before_matches);
    hide(path_driver::driver_storage_active_after_matches);
    hide(common::live_canonical_path_common_at_driver_boundary);
    hide(common::live_canonical_path_common_wf);
    hide(common::canonical_path_source_boundary_matches);
    hide(common::canonical_path_loop_source_fold);
    hide(common::parser_obligation_matches_push);
    hide(common::parser_resolution_from_return);
    hide(common::canonical_path_common_push_macro_from_loop_source);
    hide(common::canonical_path_common_root_complete_macro_from_loop_source);
    hide(parser::resolve_parser_obligation);
    hide(parser::parser_root_gate);
    hide(path_live::live_canonical_path_return_source_ready);
    match driver_action {
        path_driver::CanonicalPathDriverSourceAction::ChildReturn { observed, .. } => {
            let common_before = driver_before.common;
            if path_driver::canonical_path_driver_product_step(driver_before, driver_action)
                .is_some() {
                assert(path_driver::canonical_path_driver_product_wf(driver_before));
                canonical_path_driver_epm_lengths(driver_before);
                let live_before = common_before.live;
                let frames = driver_before.product.source.product.epm_frames;
                let admission_len = live_before.base.source.product.bridge.admission.frames.len();
                assert(live_before.path_frames.len() >= 2);
                assert(frames.len() == admission_len);
                assert(frames.len() >= 2);
                let selected = common::canonical_path_common_return_from_source(
                    common_before, observed,
                );
                assert(selected.is_some());
                let returned = path_live::live_canonical_path_return_from_source(
                    live_before, observed,
                );
                assert(returned.is_some());
                assert(selected.unwrap().live == returned.unwrap().state);
                assert(returned.unwrap().state.base.source.product.epm_frames
                    == live_before.base.source.product.epm_frames.subrange(
                        0, admission_len - 2 + 1,
                    ));
            }
        },
        _ => {},
    }
}

/// CanonicalPath Push case of `integrated_step`.
pub proof fn integrated_canonical_path_push_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        match action {
            dispatcher::IntegratedAction::CanonicalPath { driver_action, .. } =>
                driver_action is Push,
            _ => false,
        },
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_kind_guard);
    hide(dispatcher::integrated_live_case);
    hide(dispatcher::integrated_act_forward_guard);
    hide(dispatcher::integrated_act_forward_after);
    hide(dispatcher::integrated_act_unwind_guard);
    hide(dispatcher::integrated_act_unwind_after);
    hide(path_driver::canonical_path_driver_product_wf);
    hide(path_driver::canonical_path_driver_product_step);
    match (before, action) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::CanonicalPath { driver_before, driver_action },
        ) => {
            canonical_path_push_epm_frames(driver_before, driver_action);
            assert(dispatcher::integrated_path_case(product, after, driver_before, driver_action));
            assert(driver_before.product == product);
            match path_driver::canonical_path_driver_product_step(driver_before, driver_action) {
                Some(path_driver::CanonicalPathDriverProductOutcome::Active { state }) => {
                    assert(after == dispatcher::IntegratedState::Active { product: state.product });
                    epm_frames_paths_wf_push_none(product.source.product.epm_frames);
                },
                _ => {},
            }
        },
        _ => {},
    }
}

/// CanonicalPath ChildReturn case of `integrated_step`.
pub proof fn integrated_canonical_path_child_return_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        match action {
            dispatcher::IntegratedAction::CanonicalPath { driver_action, .. } =>
                driver_action is ChildReturn,
            _ => false,
        },
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_kind_guard);
    hide(dispatcher::integrated_live_case);
    hide(dispatcher::integrated_act_forward_guard);
    hide(dispatcher::integrated_act_forward_after);
    hide(dispatcher::integrated_act_unwind_guard);
    hide(dispatcher::integrated_act_unwind_after);
    hide(path_driver::canonical_path_driver_product_wf);
    hide(path_driver::canonical_path_driver_product_step);
    match (before, action) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::CanonicalPath { driver_before, driver_action },
        ) => {
            canonical_path_child_return_epm_frames(driver_before, driver_action);
            assert(dispatcher::integrated_path_case(product, after, driver_before, driver_action));
            assert(driver_before.product == product);
            match path_driver::canonical_path_driver_product_step(driver_before, driver_action) {
                Some(path_driver::CanonicalPathDriverProductOutcome::Active { state }) => {
                    assert(after == dispatcher::IntegratedState::Active { product: state.product });
                    let frames = product.source.product.epm_frames;
                    epm_frames_paths_wf_prefix(frames, frames.len() - 1);
                },
                _ => {},
            }
        },
        _ => {},
    }
}

/// CanonicalPath root Complete case of `integrated_step`: the accepted state
/// names the unchanged prestate product.
pub proof fn integrated_canonical_path_root_complete_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        match action {
            dispatcher::IntegratedAction::CanonicalPath { driver_action, .. } =>
                driver_action is RootComplete,
            _ => false,
        },
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_kind_guard);
    hide(dispatcher::integrated_live_case);
    hide(dispatcher::integrated_act_forward_guard);
    hide(dispatcher::integrated_act_forward_after);
    hide(dispatcher::integrated_act_unwind_guard);
    hide(dispatcher::integrated_act_unwind_after);
    hide(path_driver::canonical_path_driver_product_wf);
    hide(path_driver::driver_storage_before_matches);
    hide(path_driver::driver_storage_active_after_matches);
    hide(common::canonical_path_source_boundary_matches);
    hide(common::canonical_path_common_push_macro_from_loop_source);
    hide(common::canonical_path_common_return_from_source);
    hide(common::canonical_path_common_root_complete_macro_from_loop_source);
    match (before, action) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::CanonicalPath { driver_before, driver_action },
        ) => {
            assert(dispatcher::integrated_path_case(product, after, driver_before, driver_action));
            match path_driver::canonical_path_driver_product_step(driver_before, driver_action) {
                Some(path_driver::CanonicalPathDriverProductOutcome::Terminal { returned }) => {
                    assert(after == dispatcher::IntegratedState::Accepted {
                        accepted: dispatcher::IntegratedAccepted::CanonicalPathRoot {
                            pre_complete: product,
                            path_returned: returned,
                        },
                    });
                },
                _ => {},
            }
        },
        _ => {},
    }
}

/// Every CanonicalPath driver action of `integrated_step` keeps the invariant.
pub proof fn integrated_canonical_path_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        action is CanonicalPath,
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_step);
    hide(integrated_epm_paths_wf);
    match action {
        dispatcher::IntegratedAction::CanonicalPath { driver_action, .. } => {
            match driver_action {
                path_driver::CanonicalPathDriverSourceAction::Push { .. } => {
                    integrated_canonical_path_push_keeps_epm_paths_wf(before, after, action);
                },
                path_driver::CanonicalPathDriverSourceAction::ChildReturn { .. } => {
                    integrated_canonical_path_child_return_keeps_epm_paths_wf(
                        before, after, action,
                    );
                },
                path_driver::CanonicalPathDriverSourceAction::RootComplete { .. } => {
                    integrated_canonical_path_root_complete_keeps_epm_paths_wf(
                        before, after, action,
                    );
                },
            }
        },
        _ => {},
    }
}

/// An ACT Forward step runs on an active EPathMap frame that meets the batch
/// guard, and replaces that frame by `forward_batch_after`.
pub proof fn act_forward_after_epm_frames(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_step(before, after, action),
        action is ActForward,
    ensures
        match (before, action) {
            (
                dispatcher::IntegratedState::Active { product },
                dispatcher::IntegratedAction::ActForward {
                    width, commit, pending, state_id, latest_restore_state_id, forward_facts, ..
                },
            ) => {
                let frames = product.source.product.epm_frames;
                &&& frames.len() > 0
                &&& frames.last().is_some()
                &&& batch::forward_batch_source_guard(
                    frames.last().unwrap(), commit.start, width, pending, state_id,
                    latest_restore_state_id, forward_facts,
                )
                &&& match after {
                    dispatcher::IntegratedState::Active { product: next } =>
                        next.source.product.epm_frames == frames.update(
                            frames.len() - 1,
                            Some(batch::forward_batch_after(
                                frames.last().unwrap(), commit.start, width, pending, state_id,
                            )),
                        ),
                    _ => false,
                }
            },
            _ => false,
        },
{
    hide(dispatcher::integrated_kind_guard);
    hide(dispatcher::integrated_live_case);
    hide(dispatcher::integrated_path_case);
    hide(dispatcher::integrated_act_unwind_guard);
    hide(dispatcher::integrated_act_unwind_after);
    hide(integrated::live_runtime_product_wf);
    hide(integrated::product_score_wf);
    hide(act_source::accepted_act_structure_projects_frame);
    hide(act_source::accepted_indexed_line_source);
    hide(active::active_line_work_source_shape);
    hide(active::prepared_commit_source_projection);
    hide(act::derived_line_placement);
    hide(batch::batch_cursor_source_aligned);
    hide(restore::forward_capacity_facts_match);
    hide(epm::parser_gate_matches);
    hide(epm::frame_layout_wf);
    hide(parser::parser_forward_capacity);
    hide(batch::forward_batch_after);
    match (before, action) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::ActForward {
                plan_source, work, width, commit, continuation, fuel, pending,
                state_id, latest_restore_state_id, forward_facts,
            },
        ) => {
            let prior = product.source.product;
            let frames = prior.epm_frames;
            assert(act::accepted_product_forward_source(
                prior, plan_source, work, width, commit, continuation, fuel,
                pending, state_id, latest_restore_state_id, forward_facts,
            ));
            let frame = act::active_product_epm(prior).unwrap();
            assert(frames.len() > 0);
            assert(frames.last() == Some(frame));
            act::prepared_forward_batch_aligns_act_source(
                frame, work, width, commit, continuation, fuel,
            );
            assert(batch::forward_batch_source_guard(
                frame, commit.start, width, pending, state_id,
                latest_restore_state_id, forward_facts,
            ));
            let next_frame = dispatcher::integrated_act_forward_frame(
                prior, width, commit, pending, state_id,
            );
            assert(next_frame == batch::forward_batch_after(
                frame, commit.start, width, pending, state_id,
            ));
            let next = dispatcher::integrated_act_forward_after(
                product, width, commit, pending, state_id,
            );
            assert(after == dispatcher::IntegratedState::Active { product: next });
            assert(next.source.product == dispatcher::integrated_act_forward_product(
                prior, next_frame,
            ));
            assert(next.source.product.epm_frames
                == frames.update(frames.len() - 1, Some(next_frame)));
        },
        _ => {},
    }
}

/// ACT Forward case of `integrated_step`. The forwarded parser has the path
/// shape of the prestate parser, the new pending obligation and every
/// published checkpoint repeat the focused Payload obligation without awaiting
/// resolution, checkpoint `n + j` is taken at path length `n + j` with root
/// potential at most `n`, and the cursor advances two events per path byte.
pub proof fn integrated_act_forward_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        action is ActForward,
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_step);
    hide(batch::batch_cursor_source_aligned);
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::indexed_image_wf);
    hide(epm::indexed_subview);
    hide(epm::trie_position);
    hide(parser::parser_forward_capacity);
    hide(parser::forward_parser_payload);
    act_forward_after_epm_frames(before, after, action);
    match (before, action, after) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::ActForward {
                width, commit, pending, state_id, latest_restore_state_id, forward_facts, ..
            },
            dispatcher::IntegratedState::Active { product: next },
        ) => {
            let frames = product.source.product.epm_frames;
            let last = frames.len() - 1;
            let before_frame = frames.last().unwrap();
            assert(frames[last] == Some(before_frame));
            assert(epm_slot_path_wf(frames[last]));
            assert(frame_path::epm_frame_path_wf(before_frame));
            let start = commit.start;
            batch::accepted_forward_batch_composes_cursor_and_restore_run(
                before_frame, start, width, pending, state_id,
                latest_restore_state_id, forward_facts,
            );
            let image = before_frame.path.parser;
            let gate = before_frame.path.gate;
            let base = batch::path_source(before_frame);
            let source = batch::trie_source(before_frame);
            restore::accepted_forward_publishes_exact_virtual_checkpoints(
                base, pending, gate, image, state_id, source, start, width, forward_facts,
            );
            let run = restore::publish_forward_run(
                base, pending, gate, image, state_id, source, start, width,
            );
            let after_frame = batch::forward_batch_after(
                before_frame, start, width, pending, state_id,
            );
            let count = before_frame.path.path_bytes.len() as int;
            assert(before_frame.path.restore.len() == count);
            assert(run.base_parser == image);
            assert(run.first_depth as int == count);
            assert(run.base_pending == pending);
            assert(before_frame.path.pending == Some(pending));
            assert(!pending.awaiting_resolution);
            let tail = run.current.bytes.subrange(
                run.first_depth as int, run.current.bytes.len() as int,
            );
            assert(after_frame.path.parser == parser::forward_parser_payload(image, tail));
            parser_forward_payload_same_shape(image, tail);
            assert(after_frame.path.pending == Some(restore::run_current_pending(run)));
            assert(frame_path::epm_pending_matches_focus(after_frame.path.pending, image));
            parser_same_path_shape_keeps_facts(
                image, after_frame.path.parser, after_frame.path.pending, gate,
            );
            assert(after_frame.path.gate == gate);
            assert(after_frame.wait == before_frame.wait);
            assert(after_frame.path.restore
                == before_frame.path.restore + restore::expanded_run(run));
            assert forall|i: int| 0 <= i < after_frame.path.restore.len() implies {
                &&& escape::parser_image_escape_wf(
                    (#[trigger] after_frame.path.restore[i]).parser,
                )
                &&& frame_path::epm_checkpoint_wf(after_frame.path.restore[i], i)
            } by {
                if i < count {
                    assert(after_frame.path.restore[i] == before_frame.path.restore[i]);
                } else {
                    let offset = i - count;
                    assert(after_frame.path.restore[i] == restore::expanded_run(run)[offset]);
                    let checkpoint = restore::run_checkpoint(run, offset);
                    assert(restore::expanded_run(run)[offset] == checkpoint);
                    let prefix_bytes = run.current.bytes.subrange(
                        run.first_depth as int, run.first_depth as int + offset,
                    );
                    assert(checkpoint.parser == parser::forward_parser_payload(
                        image, prefix_bytes,
                    ));
                    parser_forward_payload_same_shape(image, prefix_bytes);
                    assert(frame_path::epm_pending_matches_focus(checkpoint.pending, image));
                    parser_same_path_shape_keeps_facts(
                        image, checkpoint.parser, checkpoint.pending, gate,
                    );
                    assert(checkpoint.path_len as int == i);
                }
            }
            assert(frame_path::epm_path_escape_wf(after_frame.path));
            assert(frame_path::epm_restore_wf(after_frame.path));
            assert(frame_path::epm_path_length_bounds(after_frame));
            assert(frame_path::epm_frame_path_wf(after_frame));
            assert(next.source.product.epm_frames == frames.update(last, Some(after_frame)));
            epm_frames_paths_wf_update(frames, last, after_frame);
        },
        _ => {},
    }
}

/// Every internal step of an accepted Unwind replaces only the top EPathMap
/// slot, by the virtual frame of that step.
pub proof fn unwind_product_virtual_epm_frames(
    initial: ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
)
    requires
        initial.epm_frames.len() > 0,
    ensures
        ({
            let view = unwind_product::unwind_product_virtual_state(
                initial, source, run, prefix, step,
            );
            &&& view.epm_frames.len() == initial.epm_frames.len()
            &&& forall|i: int| 0 <= i < initial.epm_frames.len() - 1
                ==> #[trigger] view.epm_frames[i] == initial.epm_frames[i]
            &&& step > 0 ==> view.epm_frames.last()
                == Some(batch::unwind_virtual_frame(source.epm, run, prefix, step))
        }),
    decreases step,
{
    hide(batch::unwind_virtual_frame);
    hide(unwind::linked_unwind_virtual_admission);
    hide(integrated::product_epm_receipts_carried);
    if step > 0 {
        let earlier = (step - 1) as usize;
        unwind_product_virtual_epm_frames(initial, source, run, prefix, earlier);
        let prior = unwind_product::unwind_product_virtual_state(
            initial, source, run, prefix, earlier,
        );
        let linked = unwind::linked_unwind_virtual_state(source, run, prefix, step);
        let view = unwind_product::unwind_product_virtual_state(
            initial, source, run, prefix, step,
        );
        assert(view == unwind_product::product_after_internal_epm_continue(prior, linked));
        assert(view.epm_frames
            == prior.epm_frames.update(prior.epm_frames.len() - 1, Some(linked.epm)));
        assert(linked.epm == batch::unwind_virtual_frame(source.epm, run, prefix, step));
        assert forall|i: int| 0 <= i < initial.epm_frames.len() - 1
            implies #[trigger] view.epm_frames[i] == initial.epm_frames[i] by {
            assert(view.epm_frames[i] == prior.epm_frames[i]);
        }
    }
}

/// An ACT Unwind step runs on an active EPathMap frame that meets the batch
/// guard with a positive width, and replaces that frame by
/// `unwind_batch_after`.
pub proof fn act_unwind_after_epm_frames(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        dispatcher::integrated_step(before, after, action),
        action is ActUnwind,
    ensures
        match (before, action) {
            (
                dispatcher::IntegratedState::Active { product },
                dispatcher::IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts },
            ) => {
                let frames = product.source.product.epm_frames;
                &&& frames.len() > 0
                &&& frames.last().is_some()
                &&& width > 0
                &&& batch::unwind_batch_source_guard(
                    frames.last().unwrap(), run, prefix, start, width, unwind_facts,
                )
                &&& match after {
                    dispatcher::IntegratedState::Active { product: next } =>
                        next.source.product.epm_frames == frames.update(
                            frames.len() - 1,
                            Some(batch::unwind_batch_after(
                                frames.last().unwrap(), run, prefix, width,
                            )),
                        ),
                    _ => false,
                }
            },
            _ => false,
        },
{
    hide(dispatcher::integrated_kind_guard);
    hide(dispatcher::integrated_live_case);
    hide(dispatcher::integrated_path_case);
    hide(dispatcher::integrated_act_forward_guard);
    hide(dispatcher::integrated_act_forward_after);
    hide(integrated::live_runtime_product_wf);
    hide(integrated::live_source_product_wf);
    hide(integrated::product_score_wf);
    hide(batch::batch_cursor_source_aligned);
    hide(batch::historical_run_wf);
    hide(batch::unwind_batch_after);
    hide(epm::parser_gate_matches);
    hide(epm::trie_position);
    hide(epm::linked_indexed_sources);
    hide(unwind_product::unwind_product_virtual_state);
    match (before, action) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts },
        ) => {
            let prior = product.source.product;
            let frames = prior.epm_frames;
            let source = dispatcher::integrated_act_unwind_source(product);
            assert(integrated::product_linked_top(prior).is_some());
            assert(frames.len() > 0);
            assert(frames.last().is_some());
            assert(source.epm == frames.last().unwrap());
            assert(batch::unwind_batch_source_guard(
                source.epm, run, prefix, start, width, unwind_facts,
            ));
            assert(batch::historical_unwind_accepted(run, width, unwind_facts));
            assert(width > 0);
            unwind_product_virtual_epm_frames(prior, source, run, prefix, width);
            let next = dispatcher::integrated_act_unwind_after(product, run, prefix, width);
            assert(after == dispatcher::IntegratedState::Active { product: next });
            let view = unwind_product::unwind_product_virtual_state(
                prior, source, run, prefix, width,
            );
            assert(next.source.product == view);
            assert(batch::unwind_virtual_frame(source.epm, run, prefix, width)
                == batch::unwind_batch_after(source.epm, run, prefix, width));
            assert(view.epm_frames =~= frames.update(
                frames.len() - 1,
                Some(batch::unwind_batch_after(source.epm, run, prefix, width)),
            ));
        },
        _ => {},
    }
}

/// The checkpoint an accepted Unwind selects is the restore entry at the new
/// path length `depth`: it satisfies the checkpoint invariant at `depth` and
/// the escape invariant, its gate matches its parser, and the remaining restore
/// entries are the prefix of length `depth`.
pub proof fn act_unwind_selected_checkpoint(
    before: epm::EpmFrame,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires
        frame_path::epm_frame_path_wf(before),
        batch::unwind_batch_source_guard(before, run, prefix, start, width, facts),
    ensures
        ({
            let total = before.path.path_bytes.len() as int;
            let depth = total - width as int;
            let selected = batch::selected_historical_checkpoint(run, width).checkpoint;
            &&& 0 <= depth < total
            &&& before.wait.is_none()
            &&& run.current.bytes.len() == total
            &&& selected == before.path.restore[depth]
            &&& frame_path::epm_checkpoint_wf(selected, depth)
            &&& escape::parser_image_escape_wf(selected.parser)
            &&& epm::parser_gate_matches(selected.gate, selected.parser)
            &&& prefix + run.checkpoints.subrange(
                0, batch::historical_run_width(run) - width as int,
            ) == before.path.restore.subrange(0, depth)
        }),
{
    hide(batch::batch_cursor_source_aligned);
    hide(epm::trie_position);
    hide(epm::parser_gate_matches);
    hide(frame_path::epm_checkpoint_wf);
    hide(escape::parser_image_escape_wf);
    let total = before.path.path_bytes.len() as int;
    let count = batch::historical_run_width(run);
    let depth = total - width as int;
    let selected_index = count - width as int;
    assert(batch::historical_run_wf(run));
    assert(batch::historical_unwind_accepted(run, width, facts));
    assert(run.checkpoints.len() == count);
    assert(0 < width as int <= count);
    assert(before.path.restore == prefix + run.checkpoints);
    assert(before.path.path_bytes == run.current.bytes);
    assert(frame_path::epm_restore_wf(before.path));
    assert(before.path.restore.len() == total);
    assert(prefix.len() + count == total);
    assert(depth == prefix.len() + selected_index);
    assert(before.path.restore[depth] == run.checkpoints[selected_index]);
    assert(frame_path::epm_checkpoint_wf(before.path.restore[depth], depth));
    assert(frame_path::epm_path_escape_wf(before.path));
    assert(escape::parser_image_escape_wf(before.path.restore[depth].parser));
    assert(epm::parser_gate_matches(
        run.checkpoints[selected_index].gate, run.checkpoints[selected_index].parser,
    ));
    assert(prefix + run.checkpoints.subrange(0, selected_index)
        =~= before.path.restore.subrange(0, depth));
}

/// ACT Unwind case of `integrated_step`. The selected checkpoint is the
/// restore entry at the new path length; the restored parser has its path
/// shape, so its layout, clause (b), root potential, escape invariant and gate
/// agreement carry over, and the cursor advances one event per removed byte.
pub proof fn integrated_act_unwind_keeps_epm_paths_wf(
    before: dispatcher::IntegratedState,
    after: dispatcher::IntegratedState,
    action: dispatcher::IntegratedAction,
)
    requires
        integrated_epm_paths_wf(before),
        dispatcher::integrated_step(before, after, action),
        action is ActUnwind,
    ensures
        integrated_epm_paths_wf(after),
{
    hide(dispatcher::integrated_step);
    hide(batch::unwind_batch_source_guard);
    hide(epm::frame_layout_wf);
    hide(epm::cursor_event_count);
    hide(epm::cursor_event_at);
    hide(epm::trie_position);
    hide(parser::restore_parser_checkpoint);
    act_unwind_after_epm_frames(before, after, action);
    match (before, action, after) {
        (
            dispatcher::IntegratedState::Active { product },
            dispatcher::IntegratedAction::ActUnwind { run, prefix, start, width, unwind_facts },
            dispatcher::IntegratedState::Active { product: next },
        ) => {
            let frames = product.source.product.epm_frames;
            let last = frames.len() - 1;
            let before_frame = frames.last().unwrap();
            assert(frames[last] == Some(before_frame));
            assert(epm_slot_path_wf(frames[last]));
            assert(frame_path::epm_frame_path_wf(before_frame));
            act_unwind_selected_checkpoint(before_frame, run, prefix, start, width, unwind_facts);
            batch::accepted_unwind_batch_composes_cursor_and_restore_run(
                before_frame, run, prefix, start, width, unwind_facts,
            );
            let after_frame = batch::unwind_batch_after(before_frame, run, prefix, width);
            let total = before_frame.path.path_bytes.len() as int;
            let depth = total - width as int;
            let selected = batch::selected_historical_checkpoint(run, width).checkpoint;
            parser_restore_checkpoint_same_shape(selected.parser, before_frame.path.parser);
            parser_same_path_shape_keeps_facts(
                selected.parser, after_frame.path.parser, selected.pending, selected.gate,
            );
            assert(after_frame.path.pending == selected.pending);
            assert(after_frame.path.gate == selected.gate);
            assert(after_frame.wait == before_frame.wait);
            assert(after_frame.path.restore == before_frame.path.restore.subrange(0, depth));
            assert(after_frame.path.path_bytes.len() == depth);
            assert forall|i: int| 0 <= i < after_frame.path.restore.len()
                implies #[trigger] after_frame.path.restore[i]
                    == before_frame.path.restore[i] by {}
            assert(frame_path::epm_path_escape_wf(after_frame.path));
            assert(frame_path::epm_restore_wf(after_frame.path));
            assert(frame_path::epm_path_length_bounds(after_frame));
            assert(frame_path::epm_frame_path_wf(after_frame));
            assert(next.source.product.epm_frames == frames.update(last, Some(after_frame)));
            epm_frames_paths_wf_update(frames, last, after_frame);
        },
        _ => {},
    }
}

}
