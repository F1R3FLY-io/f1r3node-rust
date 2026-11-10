//! Pop actions strictly below a suspended CanonicalPath frame.
//!
//! Both actions complete a message child and retain its non-path parent. They
//! cannot be used as the CanonicalPath child's own return: that is modeled by
//! the retained indexed PathChildReturn selector in the live wrapper.

use vstd::prelude::*;

use crate::{
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

pub open spec fn canonical_nested_pop_action(
    action: crate::LiveRuntimeProductAction,
) -> bool {
    match action {
        crate::LiveRuntimeProductAction::MessageChildComplete
        | crate::LiveRuntimeProductAction::MapValueAccept => true,
        _ => false,
    }
}

pub open spec fn canonical_nested_pop_step(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
) -> bool {
    &&& path_live::live_canonical_path_wf(before)
    &&& before.path_frames.len() > 2
    &&& before.path_frames[0].is_some()
    &&& canonical_nested_pop_action(action)
    &&& crate::live_runtime_product_step(before.base, after_base, action)
}

/// Frame, indexed-region, wire and cursor shape of one nested pop. The kind-1
/// top child is removed; every frame below its parent is unchanged; the parent
/// keeps its return header and its kind (1 for a message child accepted by a
/// message parent, 2 for a map value accepted by its map entry).
pub open spec fn canonical_nested_pop_shape(
    before: crate::LiveRuntimeProductState,
    after: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
) -> bool {
    let old_len = before.source.product.bridge.admission.frames.len();
    let new_len = after.source.product.bridge.admission.frames.len();
    &&& new_len + 1 == old_len
    &&& after.source.product.indexed_regions
        == before.source.product.indexed_regions.drop_last()
    &&& after.source.wire == before.source.wire.drop_last()
    &&& after.cursors == before.cursors.drop_last()
    &&& forall|i: int| 0 <= i < new_len - 1 ==>
        after.source.product.bridge.admission.frames[i]
            == before.source.product.bridge.admission.frames[i]
    &&& after.source.product.bridge.admission.frames[new_len - 1].return_id
        == before.source.product.bridge.admission.frames[new_len - 1].return_id
    &&& after.source.product.bridge.admission.frames[new_len - 1].frame_kind
        == before.source.product.bridge.admission.frames[new_len - 1].frame_kind
    &&& before.source.product.bridge.admission.frames.last().frame_kind == 1
    &&& match action {
        crate::LiveRuntimeProductAction::MessageChildComplete =>
            before.source.product.bridge.admission.frames[old_len - 2].frame_kind == 1,
        crate::LiveRuntimeProductAction::MapValueAccept =>
            before.source.product.bridge.admission.frames[old_len - 2].frame_kind == 2,
        _ => false,
    }
}

/// The live message-child completion step has the nested pop shape.
pub proof fn canonical_message_child_complete_pop_shape(
    before: crate::LiveRuntimeProductState,
    after: crate::LiveRuntimeProductState,
)
    requires crate::live_runtime_product_message_child_complete_step(before, after),
    ensures
        crate::live_runtime_product_wf(after),
        canonical_nested_pop_shape(
            before, after, crate::LiveRuntimeProductAction::MessageChildComplete,
        ),
{
    crate::live_runtime_product_message_child_complete_preserves_wf(before, after);
    let old_len = before.source.product.bridge.admission.frames.len();
    let new_len = after.source.product.bridge.admission.frames.len();
    assert(before.source.product.bridge.admission.frames[old_len - 2].frame_kind == 1);
    assert(new_len + 1 == old_len);
    assert(after.source.product.indexed_regions
        == before.source.product.indexed_regions.drop_last());
    assert(after.source.wire == before.source.wire.drop_last());
    assert(after.cursors == before.cursors.drop_last());
    assert forall|i: int| 0 <= i < new_len - 1 implies
        after.source.product.bridge.admission.frames[i]
            == before.source.product.bridge.admission.frames[i] by {}
    assert(after.source.product.bridge.admission.frames[new_len - 1].return_id
        == before.source.product.bridge.admission.frames[new_len - 1].return_id);
    assert(after.source.product.bridge.admission.frames[new_len - 1].frame_kind
        == before.source.product.bridge.admission.frames[new_len - 1].frame_kind);
}

/// The live map-value acceptance step has the nested pop shape.
pub proof fn canonical_map_value_accept_pop_shape(
    before: crate::LiveRuntimeProductState,
    after: crate::LiveRuntimeProductState,
)
    requires crate::live_runtime_product_map_value_accept_step(before, after),
    ensures
        crate::live_runtime_product_wf(after),
        canonical_nested_pop_shape(
            before, after, crate::LiveRuntimeProductAction::MapValueAccept,
        ),
{
    crate::live_runtime_product_map_value_accept_preserves_wf(before, after);
    let old_len = before.source.product.bridge.admission.frames.len();
    let new_len = after.source.product.bridge.admission.frames.len();
    assert(before.source.product.bridge.admission.frames[old_len - 2].frame_kind == 2);
    assert(new_len + 1 == old_len);
    assert(after.source.product.indexed_regions
        == before.source.product.indexed_regions.drop_last());
    assert(after.source.wire == before.source.wire.drop_last());
    assert(after.cursors == before.cursors.drop_last());
    assert forall|i: int| 0 <= i < new_len - 1 implies
        after.source.product.bridge.admission.frames[i]
            == before.source.product.bridge.admission.frames[i] by {}
    assert(after.source.product.bridge.admission.frames[new_len - 1].return_id
        == before.source.product.bridge.admission.frames[new_len - 1].return_id);
    assert(after.source.product.bridge.admission.frames[new_len - 1].frame_kind
        == before.source.product.bridge.admission.frames[new_len - 1].frame_kind);
}

/// The retained CanonicalPath frames survive any nested pop shape. Neither the
/// live product step nor a runtime step is a premise.
pub proof fn canonical_nested_pop_shape_source_ready(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires
        path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 2,
        before.path_frames[0].is_some(),
        crate::live_runtime_product_wf(after_base),
        canonical_nested_pop_shape(before.base, after_base, action),
    ensures trace::canonical_descendant_source_ready(before, after_base),
{
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert forall|i: int| 0 <= i < old_len && before.path_frames[i].is_some()
        implies i < new_len by {
        assert(path_live::path_frame_matches_live(before, i));
        if i == old_len - 1 {
            assert(before.base.source.product.bridge.admission.frames[i].frame_kind == 1);
        } else if i == old_len - 2 {
            assert(before.base.source.product.bridge.admission.frames[i].frame_kind == 1
                || before.base.source.product.bridge.admission.frames[i].frame_kind == 2);
        }
    }
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
        assert(i < old_len);
        assert(path_live::path_frame_matches_live(before, i));
        if i < new_len - 1 {
            assert(after_base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
        } else {
            assert(i == new_len - 1);
            assert(before.path_frames[i].is_none());
        }
        assert(after_base.source.product.indexed_regions[i]
            == before.base.source.product.indexed_regions[i]);
        assert(after_base.source.wire[i] == before.base.source.wire[i]);
        assert(after_base.cursors[i] == before.base.cursors[i]);
    }
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
        assert(i + 1 < new_len);
        if i + 1 < new_len - 1 {
            assert(after_base.source.product.bridge.admission.frames[i + 1]
                == before.base.source.product.bridge.admission.frames[i + 1]);
        } else {
            assert(i + 1 == new_len - 1);
        }
        assert(after_base.source.product.indexed_regions[i + 1]
            == before.base.source.product.indexed_regions[i + 1]);
    }
    assert(trace::canonical_descendant_source_ready(before, after_base));
}

pub proof fn canonical_nested_pop_refines_retained_source(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_nested_pop_step(before, after_base, action),
    ensures trace::canonical_descendant_source_ready(before, after_base),
{
    match action {
        crate::LiveRuntimeProductAction::MessageChildComplete => {
            assert(crate::live_runtime_product_message_child_complete_step(
                before.base, after_base,
            ));
            canonical_message_child_complete_pop_shape(before.base, after_base);
        },
        crate::LiveRuntimeProductAction::MapValueAccept => {
            assert(crate::live_runtime_product_map_value_accept_step(
                before.base, after_base,
            ));
            canonical_map_value_accept_pop_shape(before.base, after_base);
        },
        _ => assert(false),
    }
    canonical_nested_pop_shape_source_ready(before, after_base, action);
}

pub proof fn canonical_nested_pop_preserves_trace_wf(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_nested_pop_step(before, after_base, action),
    ensures match trace::canonical_descendant_from_source(before, after_base) {
        Some(after) => path_live::live_canonical_path_wf(after)
            && after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0],
        None => false,
    },
{
    canonical_nested_pop_refines_retained_source(before, after_base, action);
    trace::canonical_descendant_preserves_retained_path(before, after_base);
}

}
