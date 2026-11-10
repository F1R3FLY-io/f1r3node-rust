//! EPathMap obligation returns over the live admission product.
//!
//! Production returns a raw-Par child (kind 1, program 1) at its End into an
//! EPathMap parent that waits on a RawParEscape obligation in one driver
//! iteration. The child's `step_into` returns MessageComplete, and
//! `finish_message_complete` runs `process_admission_completion`: it pops the
//! child, checks the uncovered parent with `matches_popped` and
//! `before_accept`, and calls the obligation arm of the EPathMap frame's
//! `accept`. That arm takes the resolution from `PathObligationWait::resolution`
//! and calls `resolve_obligation`, whose `private_preflight` and
//! `resolve_leaf_obligation` fail fatally on a dead path or when the focused
//! top is not the awaited obligation.
//!
//! `live_runtime_epm_raw_par_return_step` is that iteration: the return derived
//! by `source_derived_epm_return`, the child cursor's End,
//! `product_epm_child_return_step`, and the pop of the runtime, wire and cursor
//! images. `epm_obligation_before_accept` adds production's fatal pre-checks as
//! guards, so the relation has no step where production returns an error. The
//! step is opaque and only the lemmas about it reveal it. The shared lemmas take
//! `nested_after` as a parameter, so they also hold for a nested EPathMap child,
//! whose End is part of its return.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_admission_cursor as admission, rpb3_epathmap_event_refinement as epm,
    rpb3_epm_nested_source_refinement as nested, rpb3_integrated_dispatcher as dispatcher,
    rpb3_path_parser_refinement as parser, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as score_bridge, rpb3_score_arena_source_refinement as arena,
    rpb3_wire_cursor_source_refinement as wire, LiveRuntimeProductState,
    LiveSourceProductState, ProductScoreState,
};

verus! {

/// Production's fatal checks before the reducer call. The obligation arm of
/// `before_accept` needs `active_obligation` to be Some with the waited id and
/// kind, awaiting resolution, and `active_obligation` is None on a dead path
/// (`dead_error`: a fault or a Dead root). `private_preflight` and the fatal
/// arms of `resolve_leaf_obligation` reject a focused top that is not the
/// awaited obligation.
pub open spec fn epm_obligation_before_accept(
    parent: epm::EpmFrame,
    kind: epm::PathObligationKind,
) -> bool {
    match parent.wait {
        Some(epm::EpmWait::Obligation { id, kind: waiting, .. }) => {
            &&& waiting == kind
            &&& !parser::parser_root_gate(parent.path.parser).dead
            &&& parent.path.parser.focus.stack.len() > 0
            &&& match parent.path.parser.focus.stack.last() {
                parser::ParserFrame::Obligation {
                    id: active,
                    kind: active_kind,
                    status: parser::ParserObligationStatus::AwaitingResolution { .. },
                    ..
                } => active == id && epm::parser_kind_matches(kind, active_kind),
                _ => false,
            }
        },
        _ => false,
    }
}

/// The EPathMap parent under the returning child exists and passes
/// `epm_obligation_before_accept` for `kind`.
pub open spec fn epm_obligation_parent_ready(
    product: ProductScoreState,
    kind: epm::PathObligationKind,
) -> bool {
    match integrated::product_linked_parent(product) {
        Some(prior) => epm_obligation_before_accept(prior.epm, kind),
        None => false,
    }
}

/// The return derived from the prestate by `source_derived_epm_return`. The
/// summary, the admission summary and the nested child frames are functions of
/// the parent and the child, so no action carries them.
pub open spec fn epm_obligation_derived(
    product: ProductScoreState,
) -> Option<nested::DerivedEpmReturn> {
    match integrated::product_linked_parent(product) {
        Some(prior) => nested::source_derived_epm_return(prior, product.epm_frames),
        None => None,
    }
}

/// The live images that the pop changes besides the product: the runtime
/// follows the new admission state, the arena is kept, and the child's wire
/// reader and cursor are dropped.
pub open spec fn epm_obligation_return_pop(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    &&& after.source.runtime == runtime::runtime_after_admission_step(
        before.source.runtime, after.source.product.bridge.admission,
    )
    &&& after.source.arena == before.source.arena
    &&& after.source.wire == before.source.wire.drop_last()
    &&& after.cursors == before.cursors.drop_last()
}

/// One frame is popped and the parent becomes `epm_parent_resolved`; the
/// transcript, the arena and the frames below the parent are kept, and the
/// resolved EPathMap parent neither waits nor holds a pending obligation.
pub open spec fn epm_obligation_return_effects(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    let product_before = before.source.product;
    let product_after = after.source.product;
    let count = product_before.bridge.admission.frames.len();
    &&& count > 1
    &&& product_after.bridge.admission.frames.len() + 1 == count
    &&& forall|i: int| 0 <= i < count - 2
        ==> product_after.bridge.admission.frames[i]
            == product_before.bridge.admission.frames[i]
    &&& product_after.bridge.admission.frames.last()
        == epm::epm_parent_resolved(product_before.bridge.admission.frames[count - 2])
    &&& product_after.bridge.admission.arena == product_before.bridge.admission.arena
    &&& product_after.bridge.admission.completed as int
        == product_before.bridge.admission.completed as int + 1
    &&& product_after.bridge.transcript == product_before.bridge.transcript
    &&& product_after.indexed_regions == product_before.indexed_regions.drop_last()
    &&& product_after.map.map_frames == product_before.map.map_frames.drop_last()
    &&& product_after.epm_frames == product_before.epm_frames.drop_last().update(
        count - 2, product_after.epm_frames.last(),
    )
    &&& match product_after.epm_frames.last() {
        Some(frame) => frame.wait.is_none() && frame.path.pending.is_none(),
        None => false,
    }
    &&& epm_obligation_return_pop(before, after)
}

/// A raw-Par child (kind 1, program 1) at its End returns into its EPathMap
/// parent waiting on a RawParEscape obligation: the iteration that
/// `finish_message_complete` runs through `process_admission_completion` and
/// the obligation arm of the EPathMap frame's `accept`. The summary is the
/// child's `Par { canonical_path_stable }`, as `PathObligationWait::resolution`
/// takes it.
#[verifier::opaque]
pub open spec fn live_runtime_epm_raw_par_return_step(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
) -> bool {
    if !integrated::live_runtime_product_wf(before) || before.cursors.len() <= 1
        || before.source.product.bridge.admission.frames.last().frame_kind != 1
        || !epm_obligation_parent_ready(
            before.source.product, epm::PathObligationKind::RawParEscape,
        )
    {
        false
    } else {
        match (before.cursors.last(), epm_obligation_derived(before.source.product)) {
            (Some(cursor), Some(derived)) => {
                &&& derived.nested_after.is_none()
                &&& wire::wire_runtime_next_field(cursor)
                    == Some(wire::WireConcreteTransition::End {
                        state: wire::wire_runtime_derive_cursor(cursor).unwrap(),
                    })
                &&& integrated::product_epm_child_return_step(
                    before.source.product, after.source.product,
                    derived.summary, derived.admission_summary, None,
                )
                &&& epm_obligation_return_pop(before, after)
            },
            _ => false,
        }
    }
}

/// The pop of `source_linked_epm_child_return` for any parent wait: one frame
/// fewer, the parent becomes `epm_parent_resolved`, the frames below it and the
/// admission counters other than `completed` are kept, and the parent's EPM
/// frame stops waiting with its cursor and mode unchanged.
pub proof fn epm_child_return_pop_shape(
    prior: epm::LinkedEpmState,
    next: epm::LinkedEpmState,
    epm_frames: Seq<Option<epm::EpmFrame>>,
)
    requires nested::source_linked_epm_child_return(prior, next, epm_frames),
    ensures
        prior.admission.frames.len() > 1,
        prior.admission.frames[prior.admission.frames.len() - 2].frame_kind == 3,
        next.admission.frames.len() + 1 == prior.admission.frames.len(),
        next.admission.completed as int == prior.admission.completed as int + 1,
        next.admission.pushed == prior.admission.pushed,
        next.admission.arena == prior.admission.arena,
        next.admission.root_end == prior.admission.root_end,
        next.admission.next_obligation_id == prior.admission.next_obligation_id,
        next.indexed_regions == prior.indexed_regions.drop_last(),
        forall|i: int| 0 <= i < next.admission.frames.len() - 1
            ==> next.admission.frames[i] == prior.admission.frames[i],
        next.admission.frames.last() == epm::epm_parent_resolved(
            prior.admission.frames[prior.admission.frames.len() - 2],
        ),
        next.epm.wait.is_none(),
        next.epm.cursor == prior.epm.cursor,
        next.epm.mode == prior.epm.mode,
{
    nested::source_epm_return_has_derived_summary(prior, next, epm_frames);
    let old_count = prior.admission.frames.len();
    assert(old_count > 1);
    let parent_index = old_count - 2;
    assert(next.admission.frames == prior.admission.frames.subrange(
        0, old_count - 1,
    ).update(
        parent_index,
        epm::epm_parent_resolved(prior.admission.frames[parent_index]),
    ));
    assert(next.admission.frames.len() == old_count - 1);
    assert forall|i: int| 0 <= i < next.admission.frames.len() - 1
        implies next.admission.frames[i] == prior.admission.frames[i] by {
        assert(i < parent_index);
    }
    assert(parent_index == next.admission.frames.len() - 1);
    let derived = nested::source_derived_epm_return(prior, epm_frames).unwrap();
    assert(epm::linked_epm_child_return(
        prior, next, derived.summary, derived.admission_summary,
        derived.nested_before, derived.nested_after,
    ));
    let returned_id = prior.admission.frames.last().return_id.unwrap();
    assert(epm::accept_child(prior.epm, next.epm, Some(returned_id), derived.summary));
}

/// The product return is a function of the prestate for any `nested_after`:
/// `accept_child` is functional and every other component is an equation.
pub proof fn product_epm_obligation_return_is_functional(
    before: ProductScoreState,
    left: ProductScoreState,
    right: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires
        integrated::product_epm_child_return_step(
            before, left, summary, admission_summary, nested_after,
        ),
        integrated::product_epm_child_return_step(
            before, right, summary, admission_summary, nested_after,
        ),
    ensures left == right,
{
    let prior = integrated::product_linked_parent(before).unwrap();
    let next_left = integrated::product_linked_top(left).unwrap();
    let next_right = integrated::product_linked_top(right).unwrap();
    let return_id = prior.admission.frames.last().return_id;
    dispatcher::accept_child_is_functional(
        prior.epm, next_left.epm, next_right.epm, return_id, summary,
    );
    assert(next_left.admission == next_right.admission);
    assert(next_left.indexed_regions == next_right.indexed_regions);
    assert(left.map == right.map);
    assert(left.bridge == right.bridge);
    assert(left.indexed_regions == right.indexed_regions);
    assert(left.epm_frames == right.epm_frames);
    dispatcher::product_epm_receipts_carried_congruent(before, left, right);
    assert(left == right);
}

/// The return keeps every product view strictly below the resolved parent.
pub proof fn product_epm_obligation_return_preserves_ancestor_prefix(
    before: ProductScoreState,
    after: ProductScoreState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires
        integrated::product_score_wf(before),
        integrated::product_epm_child_return_step(
            before, after, summary, admission_summary, nested_after,
        ),
    ensures
        after.bridge.admission.frames.len() + 1
            == before.bridge.admission.frames.len(),
        forall|i: int| 0 <= i < before.bridge.admission.frames.len() - 2 ==>
            integrated::product_ancestor_view_same(before, after, i),
{
    let prior = integrated::product_linked_parent(before).unwrap();
    let next = integrated::product_linked_top(after).unwrap();
    let limit = before.bridge.admission.frames.len() - 2;
    assert(epm::linked_epm_child_return(
        prior, next, summary, admission_summary,
        before.epm_frames.last(), nested_after,
    ));
    assert(prior.admission == before.bridge.admission);
    assert(after.bridge.admission == next.admission);
    assert(next.admission.frames == prior.admission.frames.subrange(
        0, prior.admission.frames.len() - 1,
    ).update(
        prior.admission.frames.len() - 2,
        epm::epm_parent_resolved(
            prior.admission.frames[prior.admission.frames.len() - 2],
        ),
    ));
    assert(after.indexed_regions == before.indexed_regions.drop_last());
    assert forall|i: int| 0 <= i < limit implies {
        &&& before.bridge.admission.frames[i] == after.bridge.admission.frames[i]
        &&& before.indexed_regions[i] == after.indexed_regions[i]
    } by {
        assert(after.bridge.admission.frames[i] == before.bridge.admission.frames[i]);
    }
    integrated::product_carried_receipts_preserve_prefix(before, after, limit);
    assert forall|i: int| 0 <= i < limit implies
        integrated::product_ancestor_view_same(before, after, i) by {
        assert(after.map.map_frames[i] == before.map.map_frames[i]);
        assert(after.epm_frames[i] == before.epm_frames[i]);
    }
}

/// The source images stay well formed across the return for any
/// `nested_after`: the runtime follows the popped admission state, the arena
/// is kept, and the child's wire reader is dropped.
pub proof fn live_source_epm_obligation_return_preserves_wf(
    before: LiveSourceProductState,
    after: LiveSourceProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires
        integrated::live_source_product_wf(before),
        integrated::product_score_wf(after.product),
        integrated::product_epm_child_return_step(
            before.product, after.product, summary, admission_summary, nested_after,
        ),
        after.runtime == runtime::runtime_after_admission_step(
            before.runtime, after.product.bridge.admission,
        ),
        after.arena == before.arena,
        after.wire == before.wire.drop_last(),
    ensures integrated::live_source_product_wf(after),
{
    integrated::product_epm_child_return_source_exact(
        before.product, after.product, summary, admission_summary, nested_after,
    );
    let prior = integrated::product_linked_parent(before.product).unwrap();
    let next = integrated::product_linked_top(after.product).unwrap();
    epm_child_return_pop_shape(prior, next, before.product.epm_frames);
    assert(after.product.bridge.admission == next.admission);
    assert(after.product.bridge.transcript == before.product.bridge.transcript);
    assert(after.runtime.score == after.product.bridge);
    assert(after.product.bridge.admission.frames.len() + 1
        == before.product.bridge.admission.frames.len());
    assert(admission::admission_state_wf(after.runtime.score.admission));
    assert(score_bridge::bridge_well_formed(after.runtime.score));
    assert(after.runtime.score.admission.frames.len() > 0);
    assert(after.runtime.score.admission.frames.len()
        <= before.runtime.score.admission.frames.len());
    assert(after.runtime.capacity == before.runtime.capacity);
    assert(after.runtime.max_frames == before.runtime.max_frames);
    assert(after.runtime.max_storage_bytes == before.runtime.max_storage_bytes);
    assert(after.runtime.frame_bytes == before.runtime.frame_bytes);
    assert(after.runtime.max_capacity_bytes == before.runtime.max_capacity_bytes);
    assert(after.runtime.frame_bytes > 0);
    assert(after.runtime.score.admission.frames.len() <= after.runtime.capacity as int);
    assert(after.runtime.capacity as int * after.runtime.frame_bytes as int
        <= usize::MAX as int);
    assert(after.runtime.max_frames as int
        >= after.runtime.score.admission.frames.len());
    let old_len = before.runtime.score.admission.frames.len();
    let new_len = after.runtime.score.admission.frames.len();
    let frame_bytes = after.runtime.frame_bytes as int;
    let max_storage_bytes = after.runtime.max_storage_bytes as int;
    assert(max_storage_bytes >= old_len * frame_bytes);
    assert(new_len <= old_len);
    assert(frame_bytes > 0);
    vstd::arithmetic::mul::lemma_mul_inequality(new_len as int, old_len as int, frame_bytes);
    assert(max_storage_bytes >= new_len * frame_bytes);
    assert(after.runtime.max_capacity_bytes as int
        >= after.runtime.capacity as int * after.runtime.frame_bytes as int);
    assert(runtime::runtime_frame_image_wf(after.runtime));
    assert(arena::runtime_projects_score(
        after.arena, after.product.bridge.transcript,
    ));
    assert(after.wire.len() == after.product.bridge.admission.frames.len());
    assert forall|i: int| 0 <= i < after.wire.len() implies {
        let frame = after.product.bridge.admission.frames[i];
        match #[trigger] after.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& wire::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        assert(after.wire[i] == before.wire[i]);
        if i < after.wire.len() - 1 {
            assert(after.product.bridge.admission.frames[i]
                == before.product.bridge.admission.frames[i]);
        } else {
            assert(i == after.product.bridge.admission.frames.len() - 1);
            assert(i == before.product.bridge.admission.frames.len() - 2);
            assert(before.product.bridge.admission.frames[i].frame_kind == 3);
            assert(before.wire[i].is_none());
            assert(after.product.bridge.admission.frames[i].frame_kind == 3);
        }
    }
}

/// The live runtime product stays well formed across the return for any
/// `nested_after`, with one frame fewer and one more completed frame.
pub proof fn live_runtime_epm_obligation_return_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires
        integrated::live_runtime_product_wf(before),
        integrated::product_epm_child_return_step(
            before.source.product, after.source.product,
            summary, admission_summary, nested_after,
        ),
        epm_obligation_return_pop(before, after),
    ensures
        integrated::live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    integrated::product_epm_child_return_preserves_exact(
        before.source.product, after.source.product,
        summary, admission_summary, nested_after,
    );
    live_source_epm_obligation_return_preserves_wf(
        before.source, after.source, summary, admission_summary, nested_after,
    );
    integrated::product_epm_child_return_source_exact(
        before.source.product, after.source.product,
        summary, admission_summary, nested_after,
    );
    let prior = integrated::product_linked_parent(before.source.product).unwrap();
    let next = integrated::product_linked_top(after.source.product).unwrap();
    epm_child_return_pop_shape(prior, next, before.source.product.epm_frames);
    integrated::live_runtime_wire_sources_pop(before, after);
    assert(after.cursors.len() == after.source.wire.len());
    assert forall|i: int| 0 <= i < after.cursors.len() implies {
        match (#[trigger] after.cursors[i], after.source.wire[i]) {
            (None, None) => true,
            (Some(cursor), Some(reader)) => {
                &&& wire::wire_runtime_derive_cursor(cursor).is_some()
                &&& wire::wire_concrete_cursor_wf(
                    wire::wire_runtime_derive_cursor(cursor).unwrap(),
                )
                &&& wire::wire_runtime_cursor_project(cursor) == Some(reader)
            },
            _ => false,
        }
    } by {
        assert(after.cursors[i] == before.cursors[i]);
        assert(after.source.wire[i] == before.source.wire[i]);
    }
}

/// A return into a parent waiting on an obligation has the effects of
/// `epm_obligation_return_effects`, for any `nested_after`. The premise on the
/// parent's wait excludes the map-value return, whose parent keeps its path.
pub proof fn live_epm_obligation_return_has_effects(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    summary: epm::ChildSummary,
    admission_summary: admission::AdmissionReturnedKind,
    nested_after: Option<epm::EpmFrame>,
)
    requires
        integrated::product_epm_child_return_step(
            before.source.product, after.source.product,
            summary, admission_summary, nested_after,
        ),
        epm_obligation_return_pop(before, after),
        match integrated::product_linked_parent(before.source.product) {
            Some(prior) => match prior.epm.wait {
                Some(epm::EpmWait::Obligation { .. }) => true,
                _ => false,
            },
            None => false,
        },
    ensures epm_obligation_return_effects(before, after),
{
    let product = before.source.product;
    integrated::product_epm_child_return_source_exact(
        product, after.source.product, summary, admission_summary, nested_after,
    );
    let prior = integrated::product_linked_parent(product).unwrap();
    let next = integrated::product_linked_top(after.source.product).unwrap();
    epm_child_return_pop_shape(prior, next, product.epm_frames);
    assert(nested::epm_return_stack_source(prior, product.epm_frames));
    assert(product.epm_frames.len() == product.bridge.admission.frames.len());
    assert(after.source.product.epm_frames.last() == Some(next.epm));
    assert(epm::linked_epm_child_return(
        prior, next, summary, admission_summary, product.epm_frames.last(), nested_after,
    ));
    let returned_id = prior.admission.frames.last().return_id.unwrap();
    assert(epm::accept_child(prior.epm, next.epm, Some(returned_id), summary));
    assert(next.epm.path.pending.is_none());
}

/// The raw-Par return runs on a well-formed state whose top frame is the
/// returning Message child (kind 1) above its parent.
pub proof fn live_runtime_epm_raw_par_return_top_kind(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures
        integrated::live_runtime_product_wf(before),
        before.source.product.bridge.admission.frames.len() > 1,
        before.source.product.bridge.admission.frames.last().frame_kind == 1,
{
    reveal(live_runtime_epm_raw_par_return_step);
}

/// The raw-Par return selects at most one successor.
pub proof fn live_runtime_epm_raw_par_return_is_functional(
    before: LiveRuntimeProductState,
    left: LiveRuntimeProductState,
    right: LiveRuntimeProductState,
)
    requires
        live_runtime_epm_raw_par_return_step(before, left),
        live_runtime_epm_raw_par_return_step(before, right),
    ensures left == right,
{
    reveal(live_runtime_epm_raw_par_return_step);
    let derived = epm_obligation_derived(before.source.product).unwrap();
    product_epm_obligation_return_is_functional(
        before.source.product, left.source.product, right.source.product,
        derived.summary, derived.admission_summary, None,
    );
    assert(left.source.runtime == right.source.runtime);
    assert(left.source.arena == right.source.arena);
    assert(left.source.wire == right.source.wire);
    assert(left.cursors == right.cursors);
    assert(left.source == right.source);
    assert(left == right);
}

/// The raw-Par return preserves the live runtime product's well-formedness,
/// with one frame fewer and one more completed frame.
pub proof fn live_runtime_epm_raw_par_return_preserves_wf(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures
        integrated::live_runtime_product_wf(after),
        after.source.product.bridge.admission.frames.len() + 1
            == before.source.product.bridge.admission.frames.len(),
        after.source.product.bridge.admission.completed as int
            == before.source.product.bridge.admission.completed as int + 1,
{
    reveal(live_runtime_epm_raw_par_return_step);
    let cursor = before.cursors.last().unwrap();
    wire::wire_runtime_end_is_exact(cursor);
    let derived = epm_obligation_derived(before.source.product).unwrap();
    live_runtime_epm_obligation_return_preserves_wf(
        before, after, derived.summary, derived.admission_summary, None,
    );
}

/// The raw-Par return has the effects of `epm_obligation_return_effects`.
pub proof fn live_runtime_epm_raw_par_return_has_effects(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures epm_obligation_return_effects(before, after),
{
    reveal(live_runtime_epm_raw_par_return_step);
    let derived = epm_obligation_derived(before.source.product).unwrap();
    live_epm_obligation_return_has_effects(
        before, after, derived.summary, derived.admission_summary, None,
    );
}

/// The raw-Par resolution, as in `resolve_leaf_obligation`: the derived summary
/// is the child's `Par { canonical_path_stable }`, the parent's parser is
/// resolved with `RawPar` of that stability, a stable escape kills the path
/// (EscapeOfStablePar), an unstable one leaves the gate unstable, and the
/// resolved parent keeps `entries_stable` and neither waits nor holds a pending
/// obligation.
pub proof fn epm_raw_par_return_resolution_shape(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures {
        let product = before.source.product;
        let child = product.bridge.admission.frames.last();
        let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
        let resolved = after.source.product.epm_frames.last().unwrap();
        let stable = admission::admission_raw_par_path_stability(child).unwrap();
        &&& admission::admission_raw_par_path_stability(child).is_some()
        &&& epm_obligation_derived(product).is_some()
        &&& epm_obligation_derived(product).unwrap().summary
            == epm::ChildSummary::Par { canonical_path_stable: stable }
        &&& after.source.product.epm_frames.last().is_some()
        &&& match parent.wait {
            Some(epm::EpmWait::Obligation { id, .. }) => resolved.path.parser
                == parser::resolve_parser_obligation(
                    parent.path.parser, id,
                    parser::ParserResolution::RawPar { canonical_path_stable: stable },
                ),
            _ => false,
        }
        &&& (stable ==> resolved.path.gate.dead)
        &&& (!stable ==> !resolved.path.gate.stable)
        &&& resolved.entries_stable == parent.entries_stable
        &&& resolved.path.pending.is_none()
        &&& resolved.wait.is_none()
    },
{
    reveal(live_runtime_epm_raw_par_return_step);
    let product = before.source.product;
    let derived = epm_obligation_derived(product).unwrap();
    integrated::product_epm_child_return_source_exact(
        product, after.source.product, derived.summary, derived.admission_summary, None,
    );
    let prior = integrated::product_linked_parent(product).unwrap();
    let next = integrated::product_linked_top(after.source.product).unwrap();
    nested::source_epm_return_has_derived_summary(prior, next, product.epm_frames);
    assert(prior.epm == product.epm_frames[product.epm_frames.len() - 2].unwrap());
    assert(next.epm == after.source.product.epm_frames.last().unwrap());
    assert(epm::linked_epm_child_return(
        prior, next, derived.summary, derived.admission_summary,
        product.epm_frames.last(), None,
    ));
    let returned_id = prior.admission.frames.last().return_id.unwrap();
    assert(epm::accept_child(prior.epm, next.epm, Some(returned_id), derived.summary));
}

/// Production's pre-checks hold at every raw-Par return, stated without
/// `epm_obligation_before_accept`: the parent's focused parser top is the
/// awaited RawParEscape obligation, awaiting resolution, and the path is not
/// dead.
pub proof fn epm_raw_par_return_preflight_agrees(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures {
        let product = before.source.product;
        let parent = product.epm_frames[product.epm_frames.len() - 2].unwrap();
        &&& product.epm_frames.len() > 1
        &&& product.epm_frames[product.epm_frames.len() - 2].is_some()
        &&& parent.path.parser.focus.stack.len() > 0
        &&& match (parent.wait, parent.path.parser.focus.stack.last()) {
            (
                Some(epm::EpmWait::Obligation {
                    id, kind: epm::PathObligationKind::RawParEscape, ..
                }),
                parser::ParserFrame::Obligation {
                    id: active,
                    kind: parser::ParserObligationKind::RawParEscape,
                    status: parser::ParserObligationStatus::AwaitingResolution { .. },
                    ..
                },
            ) => active == id,
            _ => false,
        }
        &&& !parser::parser_root_gate(parent.path.parser).dead
    },
{
    reveal(live_runtime_epm_raw_par_return_step);
}

/// A raw-Par return excludes the other Live actions on the same returning
/// child: MessageChildScore needs an unscored child, but a Par return carries a
/// score body (`admission_child_summary_agrees`); MessageChildComplete needs a
/// Message parent, but the parent is an EPathMap frame; EpmMapValueAccept needs
/// a MapValue wait, but the parent waits on an obligation.
pub proof fn epm_raw_par_return_excludes_child_steps(
    before: LiveRuntimeProductState,
    after: LiveRuntimeProductState,
    other: LiveRuntimeProductState,
)
    requires live_runtime_epm_raw_par_return_step(before, after),
    ensures
        !integrated::live_runtime_product_message_child_score_step(before, other),
        !integrated::live_runtime_product_message_child_complete_step(before, other),
        forall|summary: epm::ChildSummary, admission_summary: admission::AdmissionReturnedKind|
            !#[trigger] integrated::live_runtime_product_epm_map_value_accept_step(
                before, other, summary, admission_summary,
            ),
{
    reveal(live_runtime_epm_raw_par_return_step);
    let product = before.source.product;
    let derived = epm_obligation_derived(product).unwrap();
    let prior = integrated::product_linked_parent(product).unwrap();
    let next = integrated::product_linked_top(after.source.product).unwrap();
    assert(epm::linked_epm_child_return(
        prior, next, derived.summary, derived.admission_summary,
        product.epm_frames.last(), None,
    ));
    let child = product.bridge.admission.frames.last();
    assert(admission::admission_summary_has_score(derived.admission_summary));
    assert(admission::admission_score_body_agrees(child, product.bridge.admission.arena));
    assert(child.score_handle.is_some());
    assert(product.bridge.admission.frames[product.bridge.admission.frames.len() - 2]
        .frame_kind == 3);
    assert(!integrated::product_epm_map_value_owned(product));
}

}
