//! Map-entry finish strictly below a suspended CanonicalPath frame.
//!
//! The integrated action completes a kind-2 map frame into its kind-1
//! message parent. It cannot be the retained kind-4 path child's own return.

use vstd::prelude::*;

use crate::{
    rpb3_live_canonical_path_source_refinement as path_live,
    rpb3_live_canonical_path_trace_refinement as trace,
};

verus! {

pub open spec fn canonical_map_finish_below_path(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
) -> bool {
    &&& path_live::live_canonical_path_wf(before)
    &&& before.path_frames.len() > 2
    &&& before.path_frames[0].is_some()
    &&& crate::live_runtime_product_step(
        before.base, after_base, crate::LiveRuntimeProductAction::MapEntryFinish,
    )
}

pub proof fn canonical_map_finish_refines_retained_source(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires canonical_map_finish_below_path(before, after_base),
    ensures trace::canonical_descendant_source_ready(before, after_base),
{
    assert(crate::live_runtime_product_map_entry_finish_step(before.base, after_base));
    crate::live_runtime_product_map_entry_finish_source_shape(before.base, after_base);
    crate::live_runtime_product_map_entry_finish_preserves_wf(before.base, after_base);
    let old_len = before.path_frames.len();
    let new_len = after_base.source.product.bridge.admission.frames.len();
    assert(before.base.source.product.bridge.admission.frames[old_len - 1].frame_kind == 2);
    assert(before.base.source.product.bridge.admission.frames[old_len - 2].frame_kind == 1);
    assert(new_len + 1 == old_len);
    assert(after_base.source.product.indexed_regions
        == before.base.source.product.indexed_regions.drop_last());
    assert(after_base.source.wire == before.base.source.wire.drop_last());
    assert(after_base.cursors == before.base.cursors.drop_last());
    assert forall|i: int| 0 <= i < new_len - 1 implies
        after_base.source.product.bridge.admission.frames[i]
            == before.base.source.product.bridge.admission.frames[i] by {}
    assert(after_base.source.product.bridge.admission.frames[new_len - 1].return_id
        == before.base.source.product.bridge.admission.frames[new_len - 1].return_id);
    assert(after_base.source.product.bridge.admission.frames[new_len - 1].frame_kind
        == before.base.source.product.bridge.admission.frames[new_len - 1].frame_kind);
    assert forall|i: int| 0 <= i < old_len && before.path_frames[i].is_some()
        implies i < new_len by {
        assert(path_live::path_frame_matches_live(before, i));
        if i == old_len - 1 {
            assert(before.base.source.product.bridge.admission.frames[i].frame_kind == 2);
        } else if i == old_len - 2 {
            assert(before.base.source.product.bridge.admission.frames[i].frame_kind == 1);
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

pub proof fn canonical_map_finish_preserves_trace_wf(
    before: path_live::LiveCanonicalPathState,
    after_base: crate::LiveRuntimeProductState,
)
    requires canonical_map_finish_below_path(before, after_base),
    ensures match trace::canonical_descendant_from_source(before, after_base) {
        Some(after) => path_live::live_canonical_path_wf(after)
            && after.base.source.product.indexed_regions[0]
                == before.base.source.product.indexed_regions[0],
        None => false,
    },
{
    canonical_map_finish_refines_retained_source(before, after_base);
    trace::canonical_descendant_preserves_retained_path(before, after_base);
}

}
