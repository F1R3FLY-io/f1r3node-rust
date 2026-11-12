//! Actual message and map-value pushes below a suspended CanonicalPath.
//!
//! The child image is derived by `product_indexed_child` from the selected
//! wire field in the integrated step. A push cannot create another path frame:
//! the message child has kind 1 or 2, and the map value child has kind 1.
//! This is a model/source-observation handoff, not a universal Rust extraction.

use vstd::prelude::*;

use crate::{
    rpb3_admission_cursor as admission,
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
    rpb3_map_frame_refinement as map,
    rpb3_runtime_frame_source_refinement as runtime_source,
    rpb3_wire_cursor_source_refinement as wire_source,
};

verus! {

pub open spec fn canonical_nested_push_action(
    action: crate::LiveRuntimeProductAction,
) -> bool {
    match action {
        crate::LiveRuntimeProductAction::MessagePush { .. }
        | crate::LiveRuntimeProductAction::MapValuePush { .. } => true,
        _ => false,
    }
}

pub open spec fn canonical_nested_push_step(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
) -> bool {
    &&& path_live::live_canonical_path_wf(before)
    &&& before.path_frames.len() > 1
    &&& before.path_frames[0].is_some()
    &&& canonical_nested_push_action(action)
    &&& crate::live_runtime_product_step(before.base, after_base, action)
}

/// The field, child kind, and entire indexed image are selected by the actual
/// integrated push step. No caller-provided after-image is accepted.
pub open spec fn canonical_nested_push_child_exact(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
) -> bool {
    match action {
        crate::LiveRuntimeProductAction::MessagePush { request, .. } => {
            &&& (request.child.frame_kind == 1 || request.child.frame_kind == 2)
            &&& after_base.source.product.bridge.admission.frames.last().frame_kind
                == request.child.frame_kind
            &&& after_base.source.product.bridge.admission.frames.last().source_bytes
                == request.field.payload_bytes
            &&& crate::product_indexed_child(
                before.base.source.product.indexed_regions.last(), request.field,
            ).is_some()
            &&& after_base.source.product.indexed_regions.last()
                == crate::product_indexed_child(
                    before.base.source.product.indexed_regions.last(), request.field,
                ).unwrap()
        },
        crate::LiveRuntimeProductAction::MapValuePush { request, .. } => {
            &&& request.child.frame_kind == 1
            &&& after_base.source.product.bridge.admission.frames.last().frame_kind == 1
            &&& after_base.source.product.bridge.admission.frames.last().source_bytes
                == request.field.payload_bytes
            &&& crate::product_indexed_child(
                before.base.source.product.indexed_regions.last(), request.field,
            ).is_some()
            &&& after_base.source.product.indexed_regions.last()
                == crate::product_indexed_child(
                    before.base.source.product.indexed_regions.last(), request.field,
                ).unwrap()
        },
        _ => false,
    }
}

/// Concrete push shape shared by both observed actions. The old top may
/// advance and wait, but its return header remains the path child's header.
pub open spec fn canonical_nested_push_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
) -> bool {
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    &&& new_len == old_len + 1
    &&& before.base.source.product.bridge.admission.frames.last().frame_kind != 4
    &&& after_base.source.product.indexed_regions.len() == new_len
    &&& after_base.source.product.indexed_regions.drop_last()
        == before.base.source.product.indexed_regions
    &&& after_base.source.wire.len() == new_len
    &&& after_base.cursors.len() == new_len
    &&& after_base.source.wire.drop_last().drop_last()
        == before.base.source.wire.drop_last()
    &&& after_base.cursors.drop_last().drop_last()
        == before.base.cursors.drop_last()
    &&& after_base.source.product.bridge.admission.frames.drop_last().drop_last()
        == before.base.source.product.bridge.admission.frames.drop_last()
    &&& after_base.source.product.bridge.admission.frames[old_len - 1].return_id
        == before.base.source.product.bridge.admission.frames[old_len - 1].return_id
    &&& after_base.source.product.bridge.admission.frames[old_len - 1].frame_kind
        == before.base.source.product.bridge.admission.frames[old_len - 1].frame_kind
    &&& after_base.source.product.bridge.admission.frames.last().frame_kind != 4
}

/// The stack lengths that `live_canonical_path_wf` fixes: the path frames, the
/// cursors, the wire readers and the indexed regions have one entry per
/// admission frame, and the map sidecar shares the bridge's admission state.
/// They are conjuncts of the well-formedness predicates, unfolded at the
/// requires.
pub proof fn canonical_path_wf_stack_lengths(state: path_live::LiveCanonicalPathState)
    requires path_live::live_canonical_path_wf(state),
    ensures
        state.path_frames.len() == state.base.source.product.bridge.admission.frames.len(),
        state.base.cursors.len() == state.base.source.wire.len(),
        state.base.source.wire.len()
            == state.base.source.product.bridge.admission.frames.len(),
        state.base.source.product.indexed_regions.len()
            == state.base.source.product.bridge.admission.frames.len(),
        state.base.source.product.map.admission
            == state.base.source.product.bridge.admission,
{
}

/// What a plain message push step fixes for the frame shape: the product step
/// and the unchanged source action at the request's field and child, the
/// child's kind, and the wire and cursor stacks as the old stacks with the top
/// entry replaced and one entry pushed. These are conjuncts of the step,
/// unfolded at the requires. The replaced and the pushed entries are left as
/// witnesses, so no sequence fact is derived here.
pub proof fn canonical_message_push_plain_step_parts(
    before: crate::LiveRuntimeProductState,
    after: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires crate::live_runtime_product_message_push_step(before, after, request, storage),
    ensures
        crate::product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Push {
                    field: request.field,
                    child: request.child,
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            },
        ),
        map::integrated_source_message_action(
            before.source.product.map.admission,
            admission::ConcreteFrameActionModel::Push {
                field: request.field,
                child: request.child,
                frame_bytes: before.source.runtime.frame_bytes,
            },
        ) == Some(admission::ConcreteFrameActionModel::Push {
            field: request.field,
            child: request.child,
            frame_bytes: before.source.runtime.frame_bytes,
        }),
        request.child.frame_kind == 1 || request.child.frame_kind == 2,
        exists|parent_reader: Option<wire_source::WireSourceState>,
            child_reader: Option<wire_source::WireSourceState>|
            after.source.wire == #[trigger] before.source.wire.update(
                before.source.wire.len() - 1, parent_reader,
            ).push(child_reader),
        exists|parent_cursor: Option<wire_source::WireRuntimeCursorImage>,
            child_cursor: Option<wire_source::WireRuntimeCursorImage>|
            after.cursors == #[trigger] before.cursors.update(
                before.cursors.len() - 1, parent_cursor,
            ).push(child_cursor),
{
}

/// The same for an indexed message push step, whose action carries the admission
/// source image of the parent's indexed region.
pub proof fn canonical_message_push_indexed_step_parts(
    before: crate::LiveRuntimeProductState,
    after: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires
        crate::live_runtime_product_message_push_indexed_step(before, after, request, storage),
    ensures
        crate::product_map_step(
            before.source.product, after.source.product,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed {
                    field: request.field,
                    child: request.child,
                    parent_source: crate::product_admission_source_image(
                        before.source.product.indexed_regions.last(),
                    ),
                    frame_bytes: before.source.runtime.frame_bytes,
                },
            },
        ),
        map::integrated_source_message_action(
            before.source.product.map.admission,
            admission::ConcreteFrameActionModel::PushIndexed {
                field: request.field,
                child: request.child,
                parent_source: crate::product_admission_source_image(
                    before.source.product.indexed_regions.last(),
                ),
                frame_bytes: before.source.runtime.frame_bytes,
            },
        ) == Some(admission::ConcreteFrameActionModel::PushIndexed {
            field: request.field,
            child: request.child,
            parent_source: crate::product_admission_source_image(
                before.source.product.indexed_regions.last(),
            ),
            frame_bytes: before.source.runtime.frame_bytes,
        }),
        request.child.frame_kind == 1 || request.child.frame_kind == 2,
        exists|parent_reader: Option<wire_source::WireSourceState>,
            child_reader: Option<wire_source::WireSourceState>|
            after.source.wire == #[trigger] before.source.wire.update(
                before.source.wire.len() - 1, parent_reader,
            ).push(child_reader),
        exists|parent_cursor: Option<wire_source::WireRuntimeCursorImage>,
            child_cursor: Option<wire_source::WireRuntimeCursorImage>|
            after.cursors == #[trigger] before.cursors.update(
                before.cursors.len() - 1, parent_cursor,
            ).push(child_cursor),
{
}

/// A concrete message push leaves the old top's return header in place and gives
/// the new top the pushed child's kind; the old top is a message frame. The
/// driver step is unfolded at the push constructors, as in the root's
/// `product_map_push_concrete_frames`.
pub proof fn canonical_concrete_push_return_header(
    state: admission::AdmissionState,
    next: admission::AdmissionState,
    action: admission::ConcreteFrameActionModel,
)
    requires
        match action {
            admission::ConcreteFrameActionModel::Push { .. }
            | admission::ConcreteFrameActionModel::PushIndexed { .. } => true,
            _ => false,
        },
        admission::concrete_message_driver_step(state, action)
            == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: next }),
    ensures
        state.frames.last().frame_kind == 1,
        next.frames[state.frames.len() - 1].return_id == state.frames.last().return_id,
        match action {
            admission::ConcreteFrameActionModel::Push { child, .. } =>
                next.frames.last().frame_kind == child.frame_kind,
            admission::ConcreteFrameActionModel::PushIndexed { child, .. } =>
                next.frames.last().frame_kind == child.frame_kind,
            _ => true,
        },
{
    let view = admission::concrete_message_view(state).unwrap();
    match action {
        admission::ConcreteFrameActionModel::Push { field, child, frame_bytes } => {
            assert(admission::concrete_message_push(view, field, child, frame_bytes)
                == Some(next));
        },
        admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        } => {
            assert(admission::concrete_message_push_indexed(
                view, field, child, parent_source, frame_bytes,
            ) == Some(next));
        },
        _ => {},
    }
}

/// The return header facts of a plain message push at the product level, from
/// the product step and the unchanged source action: the old top is a message
/// frame and keeps its `return_id`, and the new top has the child's kind.
pub proof fn canonical_message_push_plain_return_header(
    before: crate::ProductScoreState,
    after: crate::ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    frame_bytes: usize,
)
    requires
        before.map.admission == before.bridge.admission,
        map::integrated_source_message_action(
            before.map.admission,
            admission::ConcreteFrameActionModel::Push { field, child, frame_bytes },
        ) == Some(admission::ConcreteFrameActionModel::Push { field, child, frame_bytes }),
        crate::product_map_step(
            before, after,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::Push { field, child, frame_bytes },
            },
        ),
    ensures
        before.bridge.admission.frames.last().frame_kind == 1,
        after.bridge.admission.frames[before.bridge.admission.frames.len() - 1].return_id
            == before.bridge.admission.frames.last().return_id,
        after.bridge.admission.frames.last().frame_kind == child.frame_kind,
{
    let push = admission::ConcreteFrameActionModel::Push { field, child, frame_bytes };
    let action = map::IntegratedAdmissionAction::Message { action: push };
    assert(map::integrated_admission_driver_step(before.map, action)
        == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(admission::concrete_message_driver_step(before.map.admission, push)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: after.map.admission }));
    canonical_concrete_push_return_header(
        before.bridge.admission, after.bridge.admission, push,
    );
}

/// The same return header facts for an indexed message push.
pub proof fn canonical_message_push_indexed_return_header(
    before: crate::ProductScoreState,
    after: crate::ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires
        before.map.admission == before.bridge.admission,
        map::integrated_source_message_action(
            before.map.admission,
            admission::ConcreteFrameActionModel::PushIndexed {
                field, child, parent_source, frame_bytes,
            },
        ) == Some(admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        }),
        crate::product_map_step(
            before, after,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed {
                    field, child, parent_source, frame_bytes,
                },
            },
        ),
    ensures
        before.bridge.admission.frames.last().frame_kind == 1,
        after.bridge.admission.frames[before.bridge.admission.frames.len() - 1].return_id
            == before.bridge.admission.frames.last().return_id,
        after.bridge.admission.frames.last().frame_kind == child.frame_kind,
{
    let push = admission::ConcreteFrameActionModel::PushIndexed {
        field, child, parent_source, frame_bytes,
    };
    let action = map::IntegratedAdmissionAction::Message { action: push };
    assert(map::integrated_admission_driver_step(before.map, action)
        == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(admission::concrete_message_driver_step(before.map.admission, push)
        == Some(admission::ConcreteFrameOutcomeModel::Pushed { state: after.map.admission }));
    canonical_concrete_push_return_header(
        before.bridge.admission, after.bridge.admission, push,
    );
}

/// The frame shape from its parts: the stack lengths of the old state, the wire
/// and cursor stacks as the old stacks with the top entry replaced and one entry
/// pushed, the product push facts of the root's shape lemmas, and the return
/// header facts. Only sequence and arithmetic reasoning is done here; the facts
/// arrive as lemma conclusions, so no relation besides the shape is unfolded.
pub proof fn canonical_nested_push_shape_from_parts(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    child: admission::AdmissionFrameState,
)
    requires
        before.path_frames.len() == before.base.source.product.bridge.admission.frames.len(),
        before.base.cursors.len() == before.base.source.wire.len(),
        before.base.source.wire.len()
            == before.base.source.product.bridge.admission.frames.len(),
        before.base.source.product.indexed_regions.len()
            == before.base.source.product.bridge.admission.frames.len(),
        exists|parent_reader: Option<wire_source::WireSourceState>,
            child_reader: Option<wire_source::WireSourceState>|
            after_base.source.wire == #[trigger] before.base.source.wire.update(
                before.base.source.wire.len() - 1, parent_reader,
            ).push(child_reader),
        exists|parent_cursor: Option<wire_source::WireRuntimeCursorImage>,
            child_cursor: Option<wire_source::WireRuntimeCursorImage>|
            after_base.cursors == #[trigger] before.base.cursors.update(
                before.base.cursors.len() - 1, parent_cursor,
            ).push(child_cursor),
        child.frame_kind == 1 || child.frame_kind == 2,
        before.base.source.product.bridge.admission.frames.len() > 0,
        after_base.source.product.bridge.admission.frames.len()
            == before.base.source.product.bridge.admission.frames.len() + 1,
        after_base.source.product.indexed_regions.len()
            == before.base.source.product.indexed_regions.len() + 1,
        after_base.source.product.indexed_regions.drop_last()
            == before.base.source.product.indexed_regions,
        forall|i: int| 0 <= i < before.base.source.product.bridge.admission.frames.len() - 1
            ==> after_base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i],
        after_base.source.product.bridge.admission.frames[
            before.base.source.product.bridge.admission.frames.len() - 1
        ].frame_kind == before.base.source.product.bridge.admission.frames.last().frame_kind,
        before.base.source.product.bridge.admission.frames.last().frame_kind == 1,
        after_base.source.product.bridge.admission.frames[
            before.base.source.product.bridge.admission.frames.len() - 1
        ].return_id == before.base.source.product.bridge.admission.frames.last().return_id,
        after_base.source.product.bridge.admission.frames.last().frame_kind == child.frame_kind,
    ensures canonical_nested_push_shape(before, after_base),
{
    assert(after_base.source.wire.drop_last().drop_last()
        =~= before.base.source.wire.drop_last());
    assert(after_base.cursors.drop_last().drop_last()
        =~= before.base.cursors.drop_last());
    assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()
        =~= before.base.source.product.bridge.admission.frames.drop_last());
}

/// The stack lengths, the push step's parts, the root's product push shape and
/// the return header facts are separate lemmas, and
/// `canonical_nested_push_shape_from_parts` assembles the shape from them. Each
/// obligation here is a conclusion of an earlier lemma, so this proof only
/// chains lemma contracts.
pub proof fn canonical_message_push_plain_frame_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_message_push_step(
            before.base, after_base, request, storage,
        ),
    ensures canonical_nested_push_shape(before, after_base),
{
    canonical_path_wf_stack_lengths(before);
    canonical_message_push_plain_step_parts(before.base, after_base, request, storage);
    crate::product_map_push_message_shape(
        before.base.source.product, after_base.source.product,
        request.field, request.child, before.base.source.runtime.frame_bytes,
    );
    canonical_message_push_plain_return_header(
        before.base.source.product, after_base.source.product,
        request.field, request.child, before.base.source.runtime.frame_bytes,
    );
    canonical_nested_push_shape_from_parts(before, after_base, request.child);
}

/// The indexed push follows the same chain with the indexed step's parts, the
/// root's indexed product push shape and the indexed return header facts.
pub proof fn canonical_message_push_indexed_frame_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    storage: runtime_source::SourceFramePushStorageImage,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_message_push_indexed_step(
            before.base, after_base, request, storage,
        ),
    ensures canonical_nested_push_shape(before, after_base),
{
    canonical_path_wf_stack_lengths(before);
    canonical_message_push_indexed_step_parts(before.base, after_base, request, storage);
    crate::product_map_push_message_indexed_shape(
        before.base.source.product, after_base.source.product,
        request.field, request.child,
        crate::product_admission_source_image(
            before.base.source.product.indexed_regions.last(),
        ),
        before.base.source.runtime.frame_bytes,
    );
    canonical_message_push_indexed_return_header(
        before.base.source.product, after_base.source.product,
        request.field, request.child,
        crate::product_admission_source_image(
            before.base.source.product.indexed_regions.last(),
        ),
        before.base.source.runtime.frame_bytes,
    );
    canonical_nested_push_shape_from_parts(before, after_base, request.child);
}

pub proof fn canonical_message_push_observed_frame_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_message_push_observed_step(
            before.base, after_base, request, actual_top_kind,
            after_len, after_capacity, stats_frame_variant_bytes,
        ),
    ensures canonical_nested_push_shape(before, after_base),
{
    match runtime_source::source_frame_push_observe_after(
        request.request_kind, actual_top_kind,
        before.base.source.product.bridge.admission.frames.len() as usize,
        before.base.source.runtime.capacity, after_len, after_capacity,
        before.base.source.runtime.frame_bytes, stats_frame_variant_bytes,
    ) {
        runtime_source::SourceFramePushObservationResult::Success { storage } => {
            if crate::live_runtime_product_message_push_step(
                before.base, after_base, request, storage,
            ) {
                canonical_message_push_plain_frame_shape(before, after_base, request, storage);
            } else {
                canonical_message_push_indexed_frame_shape(before, after_base, request, storage);
            }
        },
        _ => assert(false),
    }
}

pub proof fn canonical_message_push_observed_child_exact(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_message_push_observed_step(
            before.base, after_base, request, actual_top_kind,
            after_len, after_capacity, stats_frame_variant_bytes,
        ),
    ensures canonical_nested_push_child_exact(
        before, after_base,
        crate::LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ),
{
    assert(canonical_nested_push_child_exact(
        before, after_base,
        crate::LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ));
}

pub proof fn canonical_map_value_push_observed_frame_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_map_value_push_observed_step(
            before.base, after_base, request, actual_top_kind,
            after_len, after_capacity, stats_frame_variant_bytes,
        ),
    ensures
        canonical_nested_push_shape(before, after_base),
{
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert(new_len == old_len + 1);
    assert(after_base.source.product.indexed_regions.drop_last()
        == before.base.source.product.indexed_regions);
    assert(after_base.source.product.indexed_regions.len() == new_len);
    assert(after_base.source.wire.drop_last().drop_last()
        == before.base.source.wire.drop_last());
    assert(after_base.cursors.drop_last().drop_last()
        == before.base.cursors.drop_last());
    assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()
        == before.base.source.product.bridge.admission.frames.drop_last());
    assert(after_base.source.product.bridge.admission.frames[old_len - 1].return_id
        == before.base.source.product.bridge.admission.frames[old_len - 1].return_id);
    assert(after_base.source.product.bridge.admission.frames[old_len - 1].frame_kind
        == before.base.source.product.bridge.admission.frames[old_len - 1].frame_kind);
    assert(canonical_nested_push_shape(before, after_base));
}

pub proof fn canonical_map_value_push_observed_child_exact(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    request: runtime_source::SourceFrameRequestImage,
    actual_top_kind: Option<runtime_source::SourceFrameRequestKind>,
    after_len: usize,
    after_capacity: usize,
    stats_frame_variant_bytes: usize,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        crate::live_runtime_product_map_value_push_observed_step(
            before.base, after_base, request, actual_top_kind,
            after_len, after_capacity, stats_frame_variant_bytes,
        ),
    ensures canonical_nested_push_child_exact(
        before, after_base,
        crate::LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ),
{
    assert(canonical_nested_push_child_exact(
        before, after_base,
        crate::LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        },
    ));
}

pub proof fn canonical_nested_push_observed_shape(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_nested_push_step(before, after_base, action),
    ensures
        canonical_nested_push_child_exact(before, after_base, action),
        canonical_nested_push_shape(before, after_base),
{
    match action {
        crate::LiveRuntimeProductAction::MessagePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            assert(crate::live_runtime_product_message_push_observed_step(
                before.base, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            ));
            canonical_message_push_observed_frame_shape(
                before, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            );
            canonical_message_push_observed_child_exact(
                before, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            );
        },
        crate::LiveRuntimeProductAction::MapValuePush {
            request, actual_top_kind, after_len, after_capacity,
            stats_frame_variant_bytes,
        } => {
            assert(crate::live_runtime_product_map_value_push_observed_step(
                before.base, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            ));
            canonical_map_value_push_observed_frame_shape(
                before, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            );
            canonical_map_value_push_observed_child_exact(
                before, after_base, request, actual_top_kind,
                after_len, after_capacity, stats_frame_variant_bytes,
            );
        },
        _ => assert(false),
    }
    assert(canonical_nested_push_child_exact(before, after_base, action));
    assert(canonical_nested_push_shape(before, after_base));
}

pub proof fn canonical_nested_push_retained_frames(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        canonical_nested_push_shape(before, after_base),
    ensures forall|i: int| 0 <= i < after_base.source.product.bridge.admission.frames.len()
        ==> {
            if i < before.path_frames.len() {
                match before.path_frames[i] {
                    None => after_base.source.product.bridge.admission.frames[i].frame_kind != 4,
                    Some(_) => {
                        &&& after_base.source.product.bridge.admission.frames[i]
                            == before.base.source.product.bridge.admission.frames[i]
                        &&& after_base.source.product.indexed_regions[i]
                            == before.base.source.product.indexed_regions[i]
                        &&& after_base.source.wire[i] == before.base.source.wire[i]
                        &&& after_base.cursors[i] == before.base.cursors[i]
                    },
                }
            } else {
                after_base.source.product.bridge.admission.frames[i].frame_kind != 4
            }
        },
{
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert(path_live::path_frame_matches_live(before, old_len - 1));
    assert(before.path_frames[old_len - 1].is_none());
    assert forall|i: int| 0 <= i < new_len implies {
        if i < old_len {
            match before.path_frames[i] {
                None => after_base.source.product.bridge.admission.frames[i].frame_kind != 4,
                Some(_) => {
                    &&& after_base.source.product.bridge.admission.frames[i]
                        == before.base.source.product.bridge.admission.frames[i]
                    &&& after_base.source.product.indexed_regions[i]
                        == before.base.source.product.indexed_regions[i]
                    &&& after_base.source.wire[i] == before.base.source.wire[i]
                    &&& after_base.cursors[i] == before.base.cursors[i]
                },
            }
        } else {
            after_base.source.product.bridge.admission.frames[i].frame_kind != 4
        }
    } by {
        if i < old_len - 1 {
            assert(path_live::path_frame_matches_live(before, i));
            assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()[i]
                == before.base.source.product.bridge.admission.frames.drop_last()[i]);
            assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()[i]
                == after_base.source.product.bridge.admission.frames[i]);
            assert(before.base.source.product.bridge.admission.frames.drop_last()[i]
                == before.base.source.product.bridge.admission.frames[i]);
            assert(after_base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
            assert(after_base.source.product.indexed_regions.drop_last()[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after_base.source.product.indexed_regions.drop_last()[i]
                == after_base.source.product.indexed_regions[i]);
            assert(after_base.source.product.indexed_regions[i]
                == before.base.source.product.indexed_regions[i]);
            assert(after_base.source.wire.drop_last().drop_last()[i]
                == before.base.source.wire.drop_last()[i]);
            assert(after_base.source.wire.drop_last().drop_last()[i]
                == after_base.source.wire[i]);
            assert(before.base.source.wire.drop_last()[i] == before.base.source.wire[i]);
            assert(after_base.source.wire[i] == before.base.source.wire[i]);
            assert(after_base.cursors.drop_last().drop_last()[i]
                == before.base.cursors.drop_last()[i]);
            assert(after_base.cursors.drop_last().drop_last()[i]
                == after_base.cursors[i]);
            assert(before.base.cursors.drop_last()[i] == before.base.cursors[i]);
            assert(after_base.cursors[i] == before.base.cursors[i]);
        } else if i == old_len - 1 {
            assert(path_live::path_frame_matches_live(before, i));
            assert(before.path_frames[i].is_none());
        } else {
            assert(i == old_len);
        }
    }
}

pub proof fn canonical_nested_push_retained_return_headers(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 1,
        canonical_nested_push_shape(before, after_base),
    ensures forall|i: int| 0 <= i < before.path_frames.len()
        && before.outstanding[i].is_some() ==> {
            &&& i + 1 < after_base.source.product.bridge.admission.frames.len()
            &&& after_base.source.product.bridge.admission.frames[i + 1].return_id
                == before.base.source.product.bridge.admission.frames[i + 1].return_id
            &&& after_base.source.product.bridge.admission.frames[i + 1].frame_kind
                == before.base.source.product.bridge.admission.frames[i + 1].frame_kind
            &&& after_base.source.product.indexed_regions[i + 1]
                == before.base.source.product.indexed_regions[i + 1]
        },
{
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()
        == before.base.source.product.bridge.admission.frames.drop_last());
    assert forall|i: int| 0 <= i < old_len && before.outstanding[i].is_some()
        implies {
            &&& i + 1 < new_len
            &&& after_base.source.product.bridge.admission.frames[i + 1].return_id
                == before.base.source.product.bridge.admission.frames[i + 1].return_id
            &&& after_base.source.product.bridge.admission.frames[i + 1].frame_kind
                == before.base.source.product.bridge.admission.frames[i + 1].frame_kind
            &&& after_base.source.product.indexed_regions[i + 1]
                == before.base.source.product.indexed_regions[i + 1]
        } by {
        assert(path_live::path_frame_matches_live(before, i));
        assert(i + 1 < old_len);
        if i + 1 < old_len - 1 {
            assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()[i + 1]
                == before.base.source.product.bridge.admission.frames.drop_last()[i + 1]);
            assert(after_base.source.product.bridge.admission.frames.drop_last().drop_last()[i + 1]
                == after_base.source.product.bridge.admission.frames[i + 1]);
            assert(before.base.source.product.bridge.admission.frames.drop_last()[i + 1]
                == before.base.source.product.bridge.admission.frames[i + 1]);
            assert(after_base.source.product.bridge.admission.frames[i + 1]
                == before.base.source.product.bridge.admission.frames[i + 1]);
        } else {
            assert(i + 1 == old_len - 1);
        }
        assert(after_base.source.product.indexed_regions.drop_last()[i + 1]
            == before.base.source.product.indexed_regions[i + 1]);
        assert(after_base.source.product.indexed_regions.drop_last()[i + 1]
            == after_base.source.product.indexed_regions[i + 1]);
    }
}

pub proof fn canonical_nested_push_refines_retained_source(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_nested_push_step(before, after_base, action),
    ensures
        canonical_nested_push_child_exact(before, after_base, action),
        trace::canonical_descendant_source_ready(before, after_base),
{
    canonical_nested_push_observed_shape(before, after_base, action);
    crate::live_runtime_product_step_preserves_wf(before.base, after_base, action);
    canonical_nested_push_retained_frames(before, after_base);
    canonical_nested_push_retained_return_headers(before, after_base);
    assert(trace::canonical_descendant_source_ready(before, after_base));
}

pub proof fn canonical_nested_push_preserves_trace_wf(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_nested_push_step(before, after_base, action),
    ensures match trace::canonical_descendant_from_source(before, after_base) {
        Some(after) => path_live::live_canonical_path_wf(after)
            && after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0],
        None => false,
    },
{
    canonical_nested_push_refines_retained_source(before, after_base, action);
    trace::canonical_descendant_preserves_retained_path(before, after_base);
}

}
