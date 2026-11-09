//! Accepted batched Unwind as internal EPM product transitions. These modeled
//! transitions do not assert a Rust driver's final returned action.

use vstd::prelude::*;

use crate as integrated;
use crate::{
    rpb3_epathmap_event_refinement as epm, rpb3_epm_batch_cursor_composition as batch,
    rpb3_epm_restore_run_expansion as restore, rpb3_epm_unwind_event_lift as unwind,
    rpb3_map_frame_refinement as map, rpb3_runtime_frame_source_refinement as runtime,
    rpb3_score_admission_bridge as score_bridge,
};

verus! {

pub open spec fn unwind_product_source_guard(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& integrated::product_score_wf(initial)
    &&& integrated::product_linked_top(initial) == Some(source)
    &&& unwind::linked_unwind_source_guard(
        source, run, prefix, start, width, facts,
    )
}

pub open spec fn product_after_internal_epm_continue(
    prior: integrated::ProductScoreState,
    linked: epm::LinkedEpmState,
) -> integrated::ProductScoreState
    recommends prior.epm_frames.len() > 0,
{
    let candidate = integrated::ProductScoreState {
        bridge: score_bridge::ScoreAdmissionBridge {
            admission: linked.admission,
            transcript: prior.bridge.transcript,
        },
        map: map::IntegratedAdmissionState {
            admission: linked.admission,
            map_frames: prior.map.map_frames,
            transcript: prior.map.transcript,
        },
        epm_frames: prior.epm_frames.update(
            prior.epm_frames.len() - 1, Some(linked.epm),
        ),
        indexed_regions: prior.indexed_regions,
        accepted_epm_sources: Seq::empty(),
    };
    integrated::ProductScoreState {
        accepted_epm_sources: integrated::product_epm_receipts_carried(prior, candidate),
        ..candidate
    }
}

pub open spec fn unwind_product_virtual_state(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
) -> integrated::ProductScoreState
    recommends initial.epm_frames.len() > 0,
        source.admission.frames.len() > 0,
        batch::historical_run_wf(run),
        step as int <= batch::historical_run_width(run),
        source.epm.cursor.at as int + step as int <= usize::MAX as int,
    decreases step,
{
    if step == 0 {
        initial
    } else {
        let prior = unwind_product_virtual_state(
            initial, source, run, prefix, (step - 1) as usize,
        );
        let linked = unwind::linked_unwind_virtual_state(source, run, prefix, step);
        product_after_internal_epm_continue(prior, linked)
    }
}

pub proof fn unwind_product_virtual_matches_linked_source(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: usize,
)
    requires
        unwind_product_source_guard(initial, source, run, prefix, start, width, facts),
        step <= width,
    ensures {
        let view = unwind_product_virtual_state(initial, source, run, prefix, step);
        let linked = unwind::linked_unwind_virtual_state(source, run, prefix, step);
        &&& integrated::product_linked_top(view) == Some(linked)
        &&& view.epm_frames.len() == initial.epm_frames.len()
        &&& view.indexed_regions == initial.indexed_regions
        &&& view.bridge.transcript == initial.bridge.transcript
        &&& view.map.transcript == initial.map.transcript
        &&& view.map.map_frames == initial.map.map_frames
        &&& view.bridge.admission == linked.admission
        &&& view.map.admission == linked.admission
        &&& view.bridge.admission.arena == initial.bridge.admission.arena
    },
    decreases step,
{
    let view = unwind_product_virtual_state(initial, source, run, prefix, step);
    let linked = unwind::linked_unwind_virtual_state(source, run, prefix, step);
    if step == 0 {
        assert(view == initial);
        assert(linked == source);
    } else {
        unwind_product_virtual_matches_linked_source(
            initial, source, run, prefix, start, width, facts, (step - 1) as usize,
        );
        let prior = unwind_product_virtual_state(
            initial, source, run, prefix, (step - 1) as usize,
        );
        assert(prior.epm_frames.len() > 0);
        assert(view == product_after_internal_epm_continue(prior, linked));
        assert(view.epm_frames.last() == Some(linked.epm));
        assert(view.indexed_regions == linked.indexed_regions);
        assert(view.bridge.admission == linked.admission);
        assert(integrated::product_linked_top(view) == Some(linked));
    }
}

pub proof fn accepted_unwind_one_product_internal_continue(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        unwind_product_source_guard(initial, source, run, prefix, start, width, facts),
        0 <= step < width as int,
    ensures {
        let from = unwind_product_virtual_state(initial, source, run, prefix, step as usize);
        let to = unwind_product_virtual_state(
            initial, source, run, prefix, (step + 1) as usize,
        );
        integrated::product_epm_continue_step(
            from, to, unwind::unwind_event(start, width, step),
        )
    },
{
    unwind_product_virtual_matches_linked_source(
        initial, source, run, prefix, start, width, facts, step as usize,
    );
    unwind_product_virtual_matches_linked_source(
        initial, source, run, prefix, start, width, facts, (step + 1) as usize,
    );
    unwind::accepted_unwind_one_linked_continue(
        source, run, prefix, start, width, facts, step,
    );
    let from = unwind_product_virtual_state(initial, source, run, prefix, step as usize);
    let to = unwind_product_virtual_state(
        initial, source, run, prefix, (step + 1) as usize,
    );
    let linked_next = unwind::linked_unwind_virtual_state(
        source, run, prefix, (step + 1) as usize,
    );
    assert(to == product_after_internal_epm_continue(from, linked_next));
    assert(to.accepted_epm_sources == integrated::product_epm_receipts_carried(from, to));
    assert(integrated::product_epm_continue_step(
        from, to, unwind::unwind_event(start, width, step),
    ));
}

pub proof fn accepted_unwind_product_one_step_preserves_wf(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: usize,
)
    requires
        unwind_product_source_guard(initial, source, run, prefix, start, width, facts),
        step < width,
        integrated::product_score_wf(
            unwind_product_virtual_state(initial, source, run, prefix, step),
        ),
    ensures integrated::product_score_wf(
        unwind_product_virtual_state(
            initial, source, run, prefix, (step as int + 1) as usize,
        ),
    ),
{
    accepted_unwind_one_product_internal_continue(
        initial, source, run, prefix, start, width, facts, step as int,
    );
    let from = unwind_product_virtual_state(initial, source, run, prefix, step);
    let to = unwind_product_virtual_state(
        initial, source, run, prefix, (step as int + 1) as usize,
    );
    integrated::product_epm_continue_preserves_exact(
        from, to, unwind::unwind_event(start, width, step as int),
    );
}

pub proof fn accepted_unwind_product_virtual_wf(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: usize,
)
    requires
        unwind_product_source_guard(initial, source, run, prefix, start, width, facts),
        step <= width,
    ensures integrated::product_score_wf(
        unwind_product_virtual_state(initial, source, run, prefix, step),
    ),
    decreases step,
{
    if step == 0 {
        assert(unwind_product_virtual_state(initial, source, run, prefix, step) == initial);
    } else {
        let previous = (step - 1) as usize;
        accepted_unwind_product_virtual_wf(
            initial, source, run, prefix, start, width, facts, previous,
        );
        accepted_unwind_product_one_step_preserves_wf(
            initial, source, run, prefix, start, width, facts, previous,
        );
        assert((previous as int + 1) as usize == step);
    }
}

pub proof fn accepted_unwind_product_internal_sequence(
    initial: integrated::ProductScoreState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires unwind_product_source_guard(initial, source, run, prefix, start, width, facts),
    ensures
        unwind_product_virtual_state(initial, source, run, prefix, 0) == initial,
        integrated::product_score_wf(
            unwind_product_virtual_state(initial, source, run, prefix, width),
        ),
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] integrated::product_epm_continue_step(
                unwind_product_virtual_state(initial, source, run, prefix, step as usize),
                unwind_product_virtual_state(
                    initial, source, run, prefix, (step + 1) as usize,
                ),
                unwind::unwind_event(start, width, step),
            ),
{
    accepted_unwind_product_virtual_wf(
        initial, source, run, prefix, start, width, facts, width,
    );
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] integrated::product_epm_continue_step(
            unwind_product_virtual_state(initial, source, run, prefix, step as usize),
            unwind_product_virtual_state(
                initial, source, run, prefix, (step + 1) as usize,
            ),
            unwind::unwind_event(start, width, step),
        ) by {
        accepted_unwind_one_product_internal_continue(
            initial, source, run, prefix, start, width, facts, step,
        );
    }
}

/// Internal EPM events keep the wire cursor, arena, and runtime capacity fixed.
/// This relation deliberately says nothing about the eventual Rust return
/// action after the whole batch and following scalar events have completed.
pub open spec fn live_internal_epm_unwind_step(
    before: integrated::LiveSourceProductState,
    after: integrated::LiveSourceProductState,
    event: epm::EpmEvent,
) -> bool {
    &&& integrated::product_epm_continue_step(before.product, after.product, event)
    &&& after.runtime == runtime::runtime_after_admission_step(
        before.runtime, after.product.bridge.admission,
    )
    &&& after.arena == before.arena
    &&& after.wire == before.wire
}

pub open spec fn live_unwind_source_guard(
    initial: integrated::LiveSourceProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& integrated::live_source_product_wf(initial)
    &&& unwind_product_source_guard(
        initial.product, source, run, prefix, start, width, facts,
    )
}

pub open spec fn live_unwind_virtual_state(
    initial: integrated::LiveSourceProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
) -> integrated::LiveSourceProductState
    recommends initial.product.epm_frames.len() > 0,
        source.admission.frames.len() > 0,
        batch::historical_run_wf(run),
        step as int <= batch::historical_run_width(run),
        source.epm.cursor.at as int + step as int <= usize::MAX as int,
{
    if step == 0 {
        initial
    } else {
        let product = unwind_product_virtual_state(
            initial.product, source, run, prefix, step,
        );
        integrated::LiveSourceProductState {
            product,
            runtime: runtime::runtime_after_admission_step(
                initial.runtime, product.bridge.admission,
            ),
            arena: initial.arena,
            wire: initial.wire,
        }
    }
}

pub proof fn live_unwind_virtual_state_wf(
    initial: integrated::LiveSourceProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: usize,
)
    requires
        live_unwind_source_guard(initial, source, run, prefix, start, width, facts),
        step <= width,
    ensures {
        let view = live_unwind_virtual_state(initial, source, run, prefix, step);
        &&& integrated::live_source_product_wf(view)
        &&& view.arena == initial.arena
        &&& view.wire == initial.wire
        &&& view.runtime.capacity == initial.runtime.capacity
        &&& view.runtime.frame_bytes == initial.runtime.frame_bytes
        &&& view.runtime.max_frames == initial.runtime.max_frames
        &&& view.runtime.max_storage_bytes == initial.runtime.max_storage_bytes
        &&& view.runtime.max_capacity_bytes == initial.runtime.max_capacity_bytes
    },
{
    let view = live_unwind_virtual_state(initial, source, run, prefix, step);
    if step == 0 {
        assert(view == initial);
    } else {
        accepted_unwind_product_virtual_wf(
            initial.product, source, run, prefix, start, width, facts, step,
        );
        unwind_product_virtual_matches_linked_source(
            initial.product, source, run, prefix, start, width, facts, step,
        );
        let linked = unwind::linked_unwind_virtual_state(source, run, prefix, step);
        assert(initial.wire.len() > 0);
        let last = initial.wire.len() - 1;
        assert(initial.wire.last().is_none());
        assert(view.product.bridge.admission == linked.admission);
        assert(view.product.bridge.admission.frames.len()
            == initial.product.bridge.admission.frames.len());
        assert(view.product.bridge.transcript == initial.product.bridge.transcript);
        assert(view.runtime.score == view.product.bridge);
        assert(runtime::runtime_frame_image_wf(view.runtime));
        assert forall|i: int| 0 <= i < view.wire.len() implies {
            let frame = view.product.bridge.admission.frames[i];
            match #[trigger] view.wire[i] {
                None => frame.frame_kind == 3 || frame.frame_kind == 4,
                Some(reader) => {
                    &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                    &&& crate::rpb3_wire_cursor_source_refinement::wire_cursor_shape(reader.cursor)
                    &&& reader.cursor.region_start == frame.region_start
                    &&& reader.cursor.region_end == frame.region_end
                    &&& reader.cursor.next == frame.cursor
                    &&& reader.cursor.source_bytes == frame.source_bytes
                    &&& reader.terminal_error.is_none()
                },
            }
        } by {
            if i == last {
                assert(view.wire[i].is_none());
                assert(view.product.bridge.admission.frames[i].frame_kind == 3);
            } else {
                assert(view.product.bridge.admission.frames[i]
                    == initial.product.bridge.admission.frames[i]);
            }
        }
        assert(integrated::live_source_product_wf(view));
    }
}

pub proof fn accepted_unwind_one_live_internal_step(
    initial: integrated::LiveSourceProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: int,
)
    requires
        live_unwind_source_guard(initial, source, run, prefix, start, width, facts),
        0 <= step < width as int,
    ensures live_internal_epm_unwind_step(
        live_unwind_virtual_state(initial, source, run, prefix, step as usize),
        live_unwind_virtual_state(initial, source, run, prefix, (step + 1) as usize),
        unwind::unwind_event(start, width, step),
    ),
{
    accepted_unwind_one_product_internal_continue(
        initial.product, source, run, prefix, start, width, facts, step,
    );
    let from = live_unwind_virtual_state(
        initial, source, run, prefix, step as usize,
    );
    let to = live_unwind_virtual_state(
        initial, source, run, prefix, (step + 1) as usize,
    );
    assert(from.product == unwind_product_virtual_state(
        initial.product, source, run, prefix, step as usize,
    ));
    assert(to.product == unwind_product_virtual_state(
        initial.product, source, run, prefix, (step + 1) as usize,
    ));
    assert(to.runtime == runtime::runtime_after_admission_step(
        from.runtime, to.product.bridge.admission,
    ));
}

pub proof fn accepted_unwind_live_internal_sequence(
    initial: integrated::LiveSourceProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires live_unwind_source_guard(initial, source, run, prefix, start, width, facts),
    ensures
        live_unwind_virtual_state(initial, source, run, prefix, 0) == initial,
        integrated::live_source_product_wf(
            live_unwind_virtual_state(initial, source, run, prefix, width),
        ),
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] live_internal_epm_unwind_step(
                live_unwind_virtual_state(initial, source, run, prefix, step as usize),
                live_unwind_virtual_state(
                    initial, source, run, prefix, (step + 1) as usize,
                ),
                unwind::unwind_event(start, width, step),
            ),
{
    live_unwind_virtual_state_wf(
        initial, source, run, prefix, start, width, facts, width,
    );
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] live_internal_epm_unwind_step(
            live_unwind_virtual_state(initial, source, run, prefix, step as usize),
            live_unwind_virtual_state(
                initial, source, run, prefix, (step + 1) as usize,
            ),
            unwind::unwind_event(start, width, step),
        ) by {
        accepted_unwind_one_live_internal_step(
            initial, source, run, prefix, start, width, facts, step,
        );
    }
}

pub open spec fn live_runtime_internal_epm_unwind_step(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
) -> bool {
    &&& live_internal_epm_unwind_step(before.source, after.source, event)
    &&& after.cursors == before.cursors
}

pub proof fn live_runtime_internal_epm_continue_preserves_wf(
    before: integrated::LiveRuntimeProductState,
    after: integrated::LiveRuntimeProductState,
    event: epm::EpmEvent,
)
    requires
        integrated::live_runtime_product_wf(before),
        live_runtime_internal_epm_unwind_step(before, after, event),
    ensures integrated::live_runtime_product_wf(after),
{
    let old = before.source.product;
    let new = after.source.product;
    integrated::product_epm_continue_preserves_exact(old, new, event);
    let prior = integrated::product_linked_top(old).unwrap();
    let next = integrated::product_linked_top(new).unwrap();
    assert(epm::linked_epm_continue(prior, next, event));
    epm::linked_epm_continue_preserves_source_and_progress(prior, next, event);
    assert(new.bridge.transcript == old.bridge.transcript);
    assert(new.bridge.admission.frames.len() == old.bridge.admission.frames.len());
    assert(new.indexed_regions == old.indexed_regions);
    assert(after.source.runtime.score == new.bridge);
    assert(runtime::runtime_frame_image_wf(after.source.runtime));
    assert(after.source.wire.len() == new.bridge.admission.frames.len());
    assert(before.source.wire.len() > 0);
    let last = before.source.wire.len() - 1;
    assert(before.source.wire[last].is_none());
    assert forall|i: int| 0 <= i < after.source.wire.len() implies {
        let frame = new.bridge.admission.frames[i];
        match #[trigger] after.source.wire[i] {
            None => frame.frame_kind == 3 || frame.frame_kind == 4,
            Some(reader) => {
                &&& (frame.frame_kind == 1 || frame.frame_kind == 2)
                &&& crate::rpb3_wire_cursor_source_refinement::wire_cursor_shape(reader.cursor)
                &&& reader.cursor.region_start == frame.region_start
                &&& reader.cursor.region_end == frame.region_end
                &&& reader.cursor.next == frame.cursor
                &&& reader.cursor.source_bytes == frame.source_bytes
                &&& reader.terminal_error.is_none()
            },
        }
    } by {
        if i == last {
            assert(after.source.wire[i].is_none());
            assert(new.bridge.admission.frames[i].frame_kind == 3);
        } else {
            assert(new.bridge.admission.frames[i] == old.bridge.admission.frames[i]);
        }
    }
    assert(integrated::live_source_product_wf(after.source));
    assert(integrated::live_runtime_product_wf(after));
}

pub open spec fn live_runtime_unwind_source_guard(
    initial: integrated::LiveRuntimeProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
) -> bool {
    &&& integrated::live_runtime_product_wf(initial)
    &&& live_unwind_source_guard(
        initial.source, source, run, prefix, start, width, facts,
    )
}

pub open spec fn live_runtime_unwind_virtual_state(
    initial: integrated::LiveRuntimeProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    step: usize,
) -> integrated::LiveRuntimeProductState
    recommends initial.source.product.epm_frames.len() > 0,
        source.admission.frames.len() > 0,
        batch::historical_run_wf(run),
        step as int <= batch::historical_run_width(run),
        source.epm.cursor.at as int + step as int <= usize::MAX as int,
{
    integrated::LiveRuntimeProductState {
        source: live_unwind_virtual_state(
            initial.source, source, run, prefix, step,
        ),
        cursors: initial.cursors,
    }
}

pub proof fn live_runtime_unwind_virtual_state_wf(
    initial: integrated::LiveRuntimeProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
    step: usize,
)
    requires
        live_runtime_unwind_source_guard(
            initial, source, run, prefix, start, width, facts,
        ),
        step <= width,
    ensures integrated::live_runtime_product_wf(
        live_runtime_unwind_virtual_state(initial, source, run, prefix, step),
    ),
{
    live_unwind_virtual_state_wf(
        initial.source, source, run, prefix, start, width, facts, step,
    );
    let view = live_runtime_unwind_virtual_state(initial, source, run, prefix, step);
    assert(view.cursors == initial.cursors);
    assert(view.source.wire == initial.source.wire);
    assert(view.source.product.indexed_regions
        == initial.source.product.indexed_regions) by {
        unwind_product_virtual_matches_linked_source(
            initial.source.product, source, run, prefix, start, width, facts, step,
        );
    }
    assert(integrated::live_runtime_product_wf(view));
}

pub proof fn accepted_unwind_live_runtime_internal_sequence(
    initial: integrated::LiveRuntimeProductState,
    source: epm::LinkedEpmState,
    run: batch::HistoricalRestoreRunImage,
    prefix: Seq<epm::PathCheckpoint>,
    start: usize,
    width: usize,
    facts: restore::UnwindCapacityFacts,
)
    requires live_runtime_unwind_source_guard(
        initial, source, run, prefix, start, width, facts,
    ),
    ensures
        live_runtime_unwind_virtual_state(initial, source, run, prefix, 0) == initial,
        integrated::live_runtime_product_wf(
            live_runtime_unwind_virtual_state(initial, source, run, prefix, width),
        ),
        forall|step: int| 0 <= step < width as int ==>
            #[trigger] live_runtime_internal_epm_unwind_step(
                live_runtime_unwind_virtual_state(
                    initial, source, run, prefix, step as usize,
                ),
                live_runtime_unwind_virtual_state(
                    initial, source, run, prefix, (step + 1) as usize,
                ),
                unwind::unwind_event(start, width, step),
            ),
{
    live_runtime_unwind_virtual_state_wf(
        initial, source, run, prefix, start, width, facts, width,
    );
    assert forall|step: int| 0 <= step < width as int implies
        #[trigger] live_runtime_internal_epm_unwind_step(
            live_runtime_unwind_virtual_state(
                initial, source, run, prefix, step as usize,
            ),
            live_runtime_unwind_virtual_state(
                initial, source, run, prefix, (step + 1) as usize,
            ),
            unwind::unwind_event(start, width, step),
        ) by {
        accepted_unwind_one_live_internal_step(
            initial.source, source, run, prefix, start, width, facts, step,
        );
    }
}

}
