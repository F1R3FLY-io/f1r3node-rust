//! The EPathMap handoff over the live admission product.
//!
//! Production builds an EPathMap frame inside the push of its child region:
//! the EPathMap arm of `push_admission_frame_into` calls
//! `EPathMapFrame::prepare_epm1_cursor` and then
//! `EPathMapFrame::with_prepared_epm1_region`, whose closure pushes the
//! prepared frame. The product model splits that push in two. Every push
//! stores a `None` EPathMap entry for the new frame, and the product handoff
//! `product_epm_handoff_step` is the only relation that turns that entry into
//! the initial frame that `linked_map_epm_handoff` builds and validates. Every
//! live EPathMap step needs the entry (`product_linked_top`).
//!
//! `live_runtime_epm_handoff_step` lifts the product handoff to the live
//! runtime product. On a well-formed state whose top frame is a pushed
//! EPathMap child (kind 3) with a `None` entry, the product takes
//! `product_epm_handoff_step`, and the runtime, arena, wire and cursor images
//! are kept, as the bridge is (`bridge_same`). The backing and ACT plan images
//! are observations that `linked_map_epm_handoff` validates, not successors, so
//! the step is a function of the prestate and the observations. A top whose
//! entry is present takes no handoff. The step is opaque and only the lemmas
//! about it reveal it.
//!
//! The witness hands off the modeled-empty nested child right after its push:
//! the state of `live_epm_modeled_empty_nested_before` with the child's
//! admission cursor at offset 0 and its entry `None`.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_integrated_dispatcher as dispatcher, rpb3_map_frame_refinement as map,
    rpb3_runtime_frame_source_refinement as runtime, rpb3_score_admission_bridge as score_bridge,
    LiveRuntimeProductState, LiveSourceProductState, ProductScoreState,
};

verus! {

/// The EPathMap handoff of a pushed EPathMap child (kind 3): the product takes
/// `product_epm_handoff_step` with the observed backing and ACT plan images, so
/// the child's `None` entry becomes the initial frame that
/// `linked_map_epm_handoff` builds, and the runtime, arena, wire and cursor
/// images are kept. A top whose entry is present takes no handoff.
#[verifier::opaque]
pub open spec fn live_runtime_epm_handoff_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
) -> bool {
    if !integrated::live_runtime_product_wf(before)
        || before.source.product.bridge.admission.frames.len() == 0
        || before.source.product.bridge.admission.frames.last().frame_kind != 3
    {
        false
    } else {
        &&& match before.source.product.epm_frames.last() {
            None => integrated::product_epm_handoff_step(
                before.source.product, after.source.product, backing, plan, validated,
            ),
            Some(_) => false,
        }
        &&& after.source.runtime == before.source.runtime
        &&& after.source.arena == before.source.arena
        &&& after.source.wire == before.source.wire
        &&& after.cursors == before.cursors
    }
}

/// The handoff runs on a well-formed state whose top frame is the pushed
/// EPathMap child (kind 3) and whose top entry is still `None`.
pub proof fn live_runtime_epm_handoff_top_kind(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires live_runtime_epm_handoff_step(before, after, backing, plan, validated),
    ensures
        integrated::live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() > 0,
        before.source.product.bridge.admission.frames.last().frame_kind == 3,
        before.source.product.epm_frames.last().is_none(),
{
    reveal(live_runtime_epm_handoff_step);
}

/// The product handoff is a function of the prestate and the observations:
/// `linked_map_epm_handoff` is a function, `bridge_same` fixes the bridge, and
/// the carried receipts depend only on the frames and the indexed regions.
pub proof fn product_epm_handoff_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires
        integrated::product_epm_handoff_step(before, left, backing, plan, validated),
        integrated::product_epm_handoff_step(before, right, backing, plan, validated),
    ensures left == right,
{
    assert(left.bridge == right.bridge);
    assert(left.map == right.map);
    assert(left.indexed_regions == right.indexed_regions);
    assert(left.epm_frames == right.epm_frames);
    dispatcher::product_epm_receipts_carried_congruent(before, left, right);
    assert(left.accepted_epm_sources == right.accepted_epm_sources);
    assert(left == right);
}

/// The handoff selects at most one successor for fixed observations.
pub proof fn live_runtime_epm_handoff_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires
        live_runtime_epm_handoff_step(before, left, backing, plan, validated),
        live_runtime_epm_handoff_step(before, right, backing, plan, validated),
    ensures left == right,
{
    reveal(live_runtime_epm_handoff_step);
    product_epm_handoff_is_functional(
        before.source.product, left.source.product, right.source.product,
        backing, plan, validated,
    );
    assert(left.source == right.source);
    assert(left == right);
}

/// The handoff preserves the live runtime product's well-formedness: the
/// product stays well formed by the product handoff's exactness lemma, and the
/// kept bridge, runtime, arena, wire and cursor images keep every live clause.
pub proof fn live_runtime_epm_handoff_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    backing: epm::IndexedBackingImage,
    plan: Option<epm::ActPlanImage>,
    validated: Option<epm::ValidatedActPlanImage>,
)
    requires live_runtime_epm_handoff_step(before, after, backing, plan, validated),
    ensures integrated::live_runtime_product_wf(after),
{
    reveal(live_runtime_epm_handoff_step);
    integrated::product_epm_handoff_preserves_exact(
        before.source.product, after.source.product, backing, plan, validated,
    );
    assert(after.source.product.bridge == before.source.product.bridge);
    assert(integrated::live_source_product_wf(after.source));
}

/// The modeled-empty nested source right after the push of its EPathMap child:
/// the child's admission cursor is at offset 0, so nothing of the child has
/// been consumed.
pub open spec fn epm_handoff_witness_admission() -> admission::AdmissionState {
    let nested = integrated::product_epm_modeled_empty_nested_source().admission;
    admission::AdmissionState {
        frames: nested.frames.update(
            2, admission::AdmissionFrameState { cursor: 0, ..nested.frames[2] },
        ),
        ..nested
    }
}

/// The initial frame that `linked_map_epm_handoff` builds for the modeled-empty
/// child: the Empty-mode candidate of its snapshot, without an ACT plan.
pub open spec fn epm_handoff_witness_frame() -> epm::EpmFrame {
    let backing = integrated::product_epm_modeled_empty_backing();
    let snapshot = epm::indexed_backing_project(backing).unwrap();
    epm::initial_epm_candidate(
        backing, snapshot, epm::epm_prefix_from_bytes(snapshot.bytes).unwrap(), None, None,
    )
}

/// `live_epm_modeled_empty_nested_before` with the child as just pushed: its
/// admission cursor at offset 0 and its EPathMap entry `None`.
pub open spec fn epm_handoff_witness_before() -> LiveRuntimeProductState {
    let nested = integrated::live_epm_modeled_empty_nested_before();
    let product = nested.source.product;
    let pushed = epm_handoff_witness_admission();
    let bridge = score_bridge::ScoreAdmissionBridge {
        admission: pushed,
        transcript: product.bridge.transcript,
    };
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product: ProductScoreState {
                bridge,
                map: map::IntegratedAdmissionState { admission: pushed, ..product.map },
                epm_frames: seq![None, None, None],
                ..product
            },
            runtime: runtime::RuntimeFrameImage { score: bridge, ..nested.source.runtime },
            ..nested.source
        },
        ..nested
    }
}

/// The state after the handoff: the child's entry is the initial frame, and
/// everything else is kept.
pub open spec fn epm_handoff_witness_after() -> LiveRuntimeProductState {
    let before = epm_handoff_witness_before();
    LiveRuntimeProductState {
        source: LiveSourceProductState {
            product: ProductScoreState {
                epm_frames: seq![None, None, Some(epm_handoff_witness_frame())],
                ..before.source.product
            },
            ..before.source
        },
        ..before
    }
}

/// The pushed admission state is well formed: it replaces only the cursor of
/// the last frame of the modeled-empty nested source.
pub proof fn epm_handoff_witness_admission_is_wf()
    ensures admission::admission_state_wf(epm_handoff_witness_admission()),
{
    integrated::product_epm_modeled_empty_nested_source_accepts();
    let nested = integrated::product_epm_modeled_empty_nested_source().admission;
    let pushed = epm_handoff_witness_admission();
    assert(admission::admission_frame_wf(nested.frames[2]));
    assert(admission::admission_frame_wf(pushed.frames[2]));
    admission::admission_last_frame_replacement_preserves_wf(nested, pushed);
}

/// The pushed state is a well-formed live state. It differs from
/// `live_epm_modeled_empty_nested_before` only in the child's admission cursor
/// and in the child's `None` entry, and neither is constrained for a kind-3
/// frame without a wire reader.
pub proof fn epm_handoff_witness_before_is_wf()
    ensures integrated::live_runtime_product_wf(epm_handoff_witness_before()),
{
    integrated::live_epm_modeled_empty_nested_before_wf();
    epm_handoff_witness_admission_is_wf();
    let nested = integrated::live_epm_modeled_empty_nested_before();
    let before = epm_handoff_witness_before();
    let old_frames = nested.source.product.bridge.admission.frames;
    let new_frames = before.source.product.bridge.admission.frames;
    assert forall|i: int| 0 <= i < 3 implies {
        &&& new_frames[i].frame_kind == old_frames[i].frame_kind
        &&& new_frames[i].source_bytes == old_frames[i].source_bytes
        &&& new_frames[i].physical_start == old_frames[i].physical_start
        &&& new_frames[i].physical_source == old_frames[i].physical_source
        &&& new_frames[i].region_start == old_frames[i].region_start
        &&& new_frames[i].region_end == old_frames[i].region_end
        &&& (i < 2 ==> new_frames[i] == old_frames[i])
    } by {}
    assert(integrated::product_score_wf(before.source.product));
    assert(integrated::live_source_product_wf(before.source));
}

/// `linked_map_epm_handoff` accepts the pushed child with the modeled-empty
/// backing and no ACT plan, and links its initial frame to the pushed state.
pub proof fn epm_handoff_witness_links()
    ensures ({
        let product = epm_handoff_witness_before().source.product;
        epm::linked_map_epm_handoff(
            product.bridge.admission, product.indexed_regions,
            integrated::product_epm_modeled_empty_backing(), None, None,
        ) == Some(epm::LinkedEpmState {
            epm: epm_handoff_witness_frame(),
            admission: product.bridge.admission,
            indexed_regions: product.indexed_regions,
        })
    }),
{
    integrated::product_epm_modeled_empty_source_accepts();
    integrated::product_epm_modeled_empty_nested_source_accepts();
    epm_handoff_witness_admission_is_wf();
    let product = epm_handoff_witness_before().source.product;
    let pushed = product.bridge.admission;
    let regions = product.indexed_regions;
    let nested = integrated::product_epm_modeled_empty_nested_source();
    let source = integrated::product_epm_modeled_empty_source();
    let backing = integrated::product_epm_modeled_empty_backing();
    let bytes = integrated::product_epm_modeled_empty_snapshot_bytes();
    let snapshot = epm::indexed_backing_project(backing).unwrap();
    let frame = epm_handoff_witness_frame();
    assert(epm::indexed_backing_project(backing) == Some(snapshot));
    assert(snapshot.bytes =~= bytes);
    assert(source.epm.snapshot == snapshot);
    assert(epm::frame_layout_wf(source.epm));
    assert(epm::epm_prefix_from_bytes(snapshot.bytes).is_some());
    assert(frame.snapshot == snapshot);
    assert(frame.mode == epm::EpmMode::Empty);
    assert(epm::frame_layout_wf(frame));
    let child = pushed.frames.last();
    assert(child == pushed.frames[2]);
    assert(admission::admission_frame_wf(child));
    assert(epm::source_epm_initial_frame(child, backing, None, None) == Some(frame));
    assert(regions == nested.indexed_regions);
    assert(regions =~= seq![snapshot, snapshot, snapshot]);
    let span = epm::ByteSpan { start: 0, end: 8 };
    let view = epm::indexed_subview(snapshot, span);
    assert(view.bytes =~= snapshot.bytes);
    assert(view.positions =~= snapshot.positions);
    assert(view == snapshot);
    assert(epm::epm_handoff_parent_source(pushed, regions, snapshot));
    let linked = epm::LinkedEpmState { epm: frame, admission: pushed, indexed_regions: regions };
    assert forall|i: int| 0 <= i < 3 implies {
        &&& pushed.frames[i].source_bytes == nested.admission.frames[i].source_bytes
        &&& pushed.frames[i].physical_source == nested.admission.frames[i].physical_source
        &&& pushed.frames[i].physical_start == nested.admission.frames[i].physical_start
    } by {}
    assert(epm::linked_indexed_sources(linked));
}

/// The handoff is enabled on the pushed state: the witness pair is a step with
/// the modeled-empty backing and no ACT plan.
pub proof fn epm_handoff_witness_is_step()
    ensures live_runtime_epm_handoff_step(
        epm_handoff_witness_before(), epm_handoff_witness_after(),
        integrated::product_epm_modeled_empty_backing(), None, None,
    ),
{
    reveal(live_runtime_epm_handoff_step);
    epm_handoff_witness_before_is_wf();
    epm_handoff_witness_links();
    let before = epm_handoff_witness_before();
    let after = epm_handoff_witness_after();
    let product = before.source.product;
    let next = after.source.product;
    assert(next.epm_frames =~= product.epm_frames.update(2, Some(epm_handoff_witness_frame())));
    assert(integrated::product_epm_receipts_carried(product, next) =~= next.accepted_epm_sources);
    assert(integrated::product_epm_handoff_step(
        product, next, integrated::product_epm_modeled_empty_backing(), None, None,
    ));
}

/// The witness is an integrated `EpmHandoff` step between well-formed states.
pub proof fn epm_handoff_witness_is_integrated_step()
    ensures ({
        let before = dispatcher::IntegratedState::Active { product: epm_handoff_witness_before() };
        let after = dispatcher::IntegratedState::Active { product: epm_handoff_witness_after() };
        &&& dispatcher::integrated_wf(before)
        &&& dispatcher::integrated_step(
            before, after, dispatcher::IntegratedAction::EpmHandoff {
                backing: integrated::product_epm_modeled_empty_backing(),
                plan: None,
                validated: None,
            },
        )
        &&& dispatcher::integrated_wf(after)
    }),
{
    epm_handoff_witness_is_step();
    dispatcher::integrated_epm_handoff_is_integrated_step(
        epm_handoff_witness_before(), epm_handoff_witness_after(),
        integrated::product_epm_modeled_empty_backing(), None, None,
    );
}

}
