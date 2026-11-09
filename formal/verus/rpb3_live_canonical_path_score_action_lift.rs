//! Source-checked score action under a suspended CanonicalPath root.
//!
//! MessageChildScore is an actual integrated live step whose score append
//! changes the top child and arena but keeps lower frames, every indexed
//! image, and the top return header. RootScore requires a lone kind-1 root
//! and therefore cannot run from a live kind-4 path root.

use vstd::prelude::*;

use crate::{
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
    rpb3_score_admission_bridge as bridge,
};

verus! {

pub open spec fn canonical_message_child_score_below_path(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
) -> bool {
    &&& path_live::live_canonical_path_wf(before)
    &&& before.path_frames.len() > 1
    &&& before.path_frames[0].is_some()
    &&& before.base.source.product.bridge.admission.frames.last().frame_kind != 4
    &&& crate::live_runtime_product_step(
        before.base, after_base, crate::LiveRuntimeProductAction::MessageChildScore,
    )
}

pub proof fn canonical_message_child_score_refines_retained_source(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires canonical_message_child_score_below_path(before, after_base),
    ensures trace::canonical_descendant_source_ready(before, after_base),
{
    crate::live_runtime_product_step_preserves_wf(
        before.base, after_base, crate::LiveRuntimeProductAction::MessageChildScore,
    );
    assert(crate::live_runtime_product_message_child_score_step(
        before.base, after_base,
    ));
    bridge::bridge_score_source_step_preserves_shape(
        before.base.source.product.bridge,
        after_base.source.product.bridge,
    );
    let old_len = before.base.source.product.bridge.admission.frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert(new_len == old_len);
    assert(after_base.source.product.indexed_regions
        == before.base.source.product.indexed_regions);
    assert(after_base.source.wire == before.base.source.wire);
    assert(after_base.cursors == before.base.cursors);
    assert forall|i: int| 0 <= i < old_len - 1 implies
        after_base.source.product.bridge.admission.frames[i]
            == before.base.source.product.bridge.admission.frames[i] by {}
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

pub proof fn canonical_message_child_score_preserves_trace_wf(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires canonical_message_child_score_below_path(before, after_base),
    ensures match trace::canonical_descendant_from_source(before, after_base) {
        Some(after) => path_live::live_canonical_path_wf(after)
            && after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0],
        None => false,
    },
{
    canonical_message_child_score_refines_retained_source(before, after_base);
    trace::canonical_descendant_preserves_retained_path(before, after_base);
}

pub proof fn canonical_root_score_excluded_by_path_root(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires path_live::live_canonical_path_wf(before),
        before.path_frames.len() > 0,
        before.path_frames[0].is_some(),
    ensures !crate::live_runtime_product_step(
        before.base, after_base, crate::LiveRuntimeProductAction::RootScore,
    ),
{
    if crate::live_runtime_product_step(
        before.base, after_base, crate::LiveRuntimeProductAction::RootScore,
    ) {
        assert(crate::live_runtime_product_root_score_step(before.base, after_base));
        assert(before.base.source.product.bridge.admission.frames.len() == 1);
        assert(before.base.source.product.bridge.admission.frames[0].frame_kind == 1);
        assert(path_live::path_frame_matches_live(before, 0));
        assert(before.base.source.product.bridge.admission.frames[0].frame_kind == 4);
        assert(false);
    }
}

}
