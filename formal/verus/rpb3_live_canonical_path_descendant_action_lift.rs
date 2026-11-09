//! A checked lift of steady live driver actions beneath a suspended path.
//!
//! This module is compiled with `rpb3_live_push`, where the integrated live
//! action dispatcher is available. It does not treat the descendant's product
//! poststate as a free observation: `live_runtime_product_step` must relate it
//! to the prestate. Push/pop and EPM macros need separate preservation lemmas.

use vstd::prelude::*;

use crate::{
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

/// These three actions keep stack length and all indexed images fixed. They
/// mutate only the active non-path top frame and its wire cursor. Scored,
/// pushing, popping, EPM, and terminal actions are excluded.
pub open spec fn canonical_steady_descendant_action(
    action: crate::LiveRuntimeProductAction,
) -> bool {
    match action {
        crate::LiveRuntimeProductAction::Continue
        | crate::LiveRuntimeProductAction::ContinueIndexed
        | crate::LiveRuntimeProductAction::MapKey => true,
        _ => false,
    }
}

pub open spec fn canonical_steady_descendant_step(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
) -> bool {
    &&& path_live::live_canonical_path_wf(before)
    &&& before.path_frames.len() > 1
    &&& before.path_frames[0].is_some()
    &&& before.base.source.product.bridge.admission.frames.last().frame_kind != 4
    &&& canonical_steady_descendant_action(action)
    &&& crate::live_runtime_product_step(before.base, after_base, action)
}

/// A real integrated step is the primary premise. Its steady action shape
/// supplies the exact lower-frame/indexed/wire prefix and the top child's
/// return header needed by the retained-path descendant guard.
pub proof fn canonical_steady_descendant_step_refines_retained_source(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_steady_descendant_step(before, after_base, action),
    ensures trace::canonical_descendant_source_ready(before, after_base),
{
    crate::live_runtime_product_step_preserves_wf(before.base, after_base, action);
    let old_len = before.base.source.product.bridge.admission.frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    match action {
        crate::LiveRuntimeProductAction::Continue => {
            assert(crate::live_runtime_product_continue_step(before.base, after_base));
        },
        crate::LiveRuntimeProductAction::ContinueIndexed => {
            assert(crate::live_runtime_product_continue_indexed_step(before.base, after_base));
        },
        crate::LiveRuntimeProductAction::MapKey => {
            assert(crate::live_runtime_product_map_key_step(before.base, after_base));
        },
        _ => assert(false),
    }
    assert(new_len == old_len);
    assert(after_base.source.product.indexed_regions
        == before.base.source.product.indexed_regions);
    assert forall|i: int| 0 <= i < old_len - 1 implies
        after_base.source.product.bridge.admission.frames[i]
            == before.base.source.product.bridge.admission.frames[i]
        && after_base.source.wire[i] == before.base.source.wire[i]
        && after_base.cursors[i] == before.base.cursors[i] by {}
    assert(after_base.source.product.bridge.admission.frames[old_len - 1].return_id
        == before.base.source.product.bridge.admission.frames[old_len - 1].return_id);
    assert(after_base.source.product.bridge.admission.frames[old_len - 1].frame_kind
        == before.base.source.product.bridge.admission.frames[old_len - 1].frame_kind);
    assert forall|i: int| 0 <= i < old_len && before.path_frames[i].is_some()
        implies i < new_len by {}
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
        if i < old_len - 1 {
            assert(after_base.source.product.bridge.admission.frames[i]
                == before.base.source.product.bridge.admission.frames[i]);
            assert(after_base.source.wire[i] == before.base.source.wire[i]);
            assert(after_base.cursors[i] == before.base.cursors[i]);
        } else {
            assert(i == old_len - 1);
            assert(before.path_frames[i].is_none());
        }
        assert(after_base.source.product.indexed_regions[i]
            == before.base.source.product.indexed_regions[i]);
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
        if i + 1 < old_len - 1 {
            assert(after_base.source.product.bridge.admission.frames[i + 1]
                == before.base.source.product.bridge.admission.frames[i + 1]);
        } else {
            assert(i + 1 == old_len - 1);
        }
        assert(after_base.source.product.indexed_regions[i + 1]
            == before.base.source.product.indexed_regions[i + 1]);
    }
    assert(trace::canonical_descendant_source_ready(before, after_base));
}

pub proof fn canonical_steady_descendant_step_preserves_trace_wf(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
    action: crate::LiveRuntimeProductAction,
)
    requires canonical_steady_descendant_step(before, after_base, action),
    ensures match trace::canonical_descendant_from_source(before, after_base) {
        Some(after) => path_live::live_canonical_path_wf(after)
            && after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0],
        None => false,
    },
{
    canonical_steady_descendant_step_refines_retained_source(before, after_base, action);
    trace::canonical_descendant_preserves_retained_path(before, after_base);
}

}
