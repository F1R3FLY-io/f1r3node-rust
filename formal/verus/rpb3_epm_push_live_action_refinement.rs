use vstd::prelude::*;

use crate::{
    rpb3_epathmap_event_refinement as epm, rpb3_epm_push_source_action_refinement as source,
    rpb3_runtime_frame_source_refinement as runtime, rpb3_wire_cursor_source_refinement as wire,
};

verus! {

pub open spec fn epm_push_live_child_reader(
    observed: source::EpmPushLiveObservation,
) -> Option<wire::WireSourceState> {
    match observed.child_cursor {
        Some(cursor) => wire::wire_runtime_cursor_project(cursor),
        None => None,
    }
}

pub open spec fn epm_push_live_child_ready(
    before: crate::LiveRuntimeProductState,
    observed: source::EpmPushLiveObservation,
) -> bool {
    let product = before.source.product;
    let prior = crate::product_linked_top(product).unwrap();
    let request = source::epm_push_source_request(prior, observed.push).unwrap();
    let next = source::epm_push_source_product_after(product, observed.push, request);
    let child = next.bridge.admission.frames.last();
    match source::epm_push_source_request_kind(request) {
        runtime::SourceFrameRequestKind::Message => match observed.child_cursor {
            Some(cursor) => match wire::wire_runtime_cursor_project(cursor) {
                Some(reader) => {
                    &&& wire::wire_runtime_initial(cursor.message, false) == Some(cursor)
                    &&& wire::wire_runtime_derive_cursor(cursor).is_some()
                    &&& wire::wire_concrete_cursor_wf(
                        wire::wire_runtime_derive_cursor(cursor).unwrap(),
                    )
                    &&& crate::product_wire_indexed_source_agrees(
                        cursor, epm::request_body(request),
                    )
                    &&& reader.cursor.region_start == child.region_start
                    &&& reader.cursor.region_end == child.region_end
                    &&& reader.cursor.next == child.cursor
                    &&& reader.cursor.source_bytes == child.source_bytes
                    &&& reader.terminal_error.is_none()
                },
                None => false,
            },
            None => false,
        },
        runtime::SourceFrameRequestKind::EPathMap => observed.child_cursor.is_none(),
        _ => false,
    }
}

pub open spec fn epm_push_live_ready(
    before: crate::LiveRuntimeProductState,
    observed: source::EpmPushLiveObservation,
) -> bool {
    if !source::epm_push_source_ready(before, observed.push) {
        false
    } else {
        let prior = crate::product_linked_top(before.source.product).unwrap();
        let request = source::epm_push_source_request(prior, observed.push).unwrap();
        &&& before.source.wire.last().is_none()
        &&& before.cursors.last().is_none()
        &&& epm_push_live_child_ready(before, observed)
        &&& match runtime::source_frame_push_observe_after(
            source::epm_push_source_request_kind(request),
            observed.push.actual_top_kind,
            prior.admission.frames.len() as usize,
            before.source.runtime.capacity,
            observed.push.after_len,
            observed.push.after_capacity,
            before.source.runtime.frame_bytes,
            observed.push.stats_frame_variant_bytes,
        ) {
            runtime::SourceFramePushObservationResult::Success { storage } =>
                runtime::source_successful_push_storage(before.source.runtime, storage),
            _ => false,
        }
    }
}

pub open spec fn epm_push_live_runtime_after(
    before: crate::LiveRuntimeProductState,
    product: crate::ProductScoreState,
    storage: runtime::SourceFramePushStorageImage,
) -> runtime::RuntimeFrameImage {
    let old = before.source.runtime;
    runtime::RuntimeFrameImage {
        score: product.bridge,
        capacity: storage.after_capacity,
        frame_bytes: old.frame_bytes,
        max_frames: if old.max_frames >= storage.after_len {
            old.max_frames
        } else {
            storage.after_len
        },
        max_storage_bytes: if old.max_storage_bytes >= storage.used_bytes {
            old.max_storage_bytes
        } else {
            storage.used_bytes
        },
        max_capacity_bytes: if old.max_capacity_bytes >= storage.reserved_bytes {
            old.max_capacity_bytes
        } else {
            storage.reserved_bytes
        },
    }
}

pub open spec fn epm_push_live_after(
    before: crate::LiveRuntimeProductState,
    observed: source::EpmPushLiveObservation,
) -> Option<crate::LiveRuntimeProductState> {
    if !epm_push_live_ready(before, observed) {
        None
    } else {
        let prior = crate::product_linked_top(before.source.product).unwrap();
        let request = source::epm_push_source_request(prior, observed.push).unwrap();
        match runtime::source_frame_push_observe_after(
            source::epm_push_source_request_kind(request),
            observed.push.actual_top_kind,
            prior.admission.frames.len() as usize,
            before.source.runtime.capacity,
            observed.push.after_len,
            observed.push.after_capacity,
            before.source.runtime.frame_bytes,
            observed.push.stats_frame_variant_bytes,
        ) {
            runtime::SourceFramePushObservationResult::Success { storage } => {
                let product = source::epm_push_source_product_after(
                    before.source.product, observed.push, request,
                );
                Some(crate::LiveRuntimeProductState {
                    source: crate::LiveSourceProductState {
                        product,
                        runtime: epm_push_live_runtime_after(before, product, storage),
                        arena: before.source.arena,
                        wire: before.source.wire.push(epm_push_live_child_reader(observed)),
                    },
                    cursors: before.cursors.push(observed.child_cursor),
                })
            },
            _ => None,
        }
    }
}

pub proof fn epm_push_live_action_is_checked(
    before: crate::LiveRuntimeProductState,
    observed: source::EpmPushLiveObservation,
)
    requires epm_push_live_ready(before, observed),
    ensures match epm_push_live_after(before, observed) {
        Some(after) => {
            let prior = crate::product_linked_top(before.source.product).unwrap();
            let request = source::epm_push_source_request(prior, observed.push).unwrap();
            &&& crate::live_runtime_product_wf(after)
            &&& crate::product_source_step(
                before.source.product, after.source.product,
                crate::ProductSourceAction::EpmPush {
                    event: observed.push.terminal_event,
                    action: epm::EpmAction::Push(request),
                    child: observed.push.child,
                },
            )
            &&& after.source.runtime.score == after.source.product.bridge
            &&& after.source.runtime.capacity == observed.push.after_capacity
            &&& after.source.runtime.frame_bytes == before.source.runtime.frame_bytes
            &&& after.source.product.indexed_regions.last() == epm::request_body(request)
            &&& after.source.product.bridge.transcript
                == before.source.product.bridge.transcript
            &&& after.source.arena == before.source.arena
            &&& after.source.wire.drop_last() == before.source.wire
            &&& after.cursors.drop_last() == before.cursors
            &&& after.source.wire.last() == epm_push_live_child_reader(observed)
            &&& after.cursors.last() == observed.child_cursor
        },
        None => false,
    },
{
    source::epm_push_source_enables_integrated_action(before, observed.push);
    let prior = crate::product_linked_top(before.source.product).unwrap();
    let request = source::epm_push_source_request(prior, observed.push).unwrap();
    let product = source::epm_push_source_product_after(
        before.source.product, observed.push, request,
    );
    let result = runtime::source_frame_push_observe_after(
        source::epm_push_source_request_kind(request),
        observed.push.actual_top_kind,
        prior.admission.frames.len() as usize,
        before.source.runtime.capacity,
        observed.push.after_len,
        observed.push.after_capacity,
        before.source.runtime.frame_bytes,
        observed.push.stats_frame_variant_bytes,
    );
    runtime::source_frame_push_observe_after_exact(
        source::epm_push_source_request_kind(request),
        observed.push.actual_top_kind,
        prior.admission.frames.len() as usize,
        before.source.runtime.capacity,
        observed.push.after_len,
        observed.push.after_capacity,
        before.source.runtime.frame_bytes,
        observed.push.stats_frame_variant_bytes,
    );
    if let runtime::SourceFramePushObservationResult::Success { storage } = result {
        let after = epm_push_live_after(before, observed).unwrap();
        assert(crate::product_epm_push_step(
            before.source.product, product, observed.push.terminal_event,
            epm::EpmAction::Push(request), observed.push.child,
        ));
        crate::product_epm_push_preserves_exact(
            before.source.product, product, observed.push.terminal_event,
            epm::EpmAction::Push(request), observed.push.child,
        );
        assert(runtime::runtime_frame_image_wf(after.source.runtime));
        assert(crate::live_source_product_wf(after.source));
        assert(crate::live_runtime_product_wf(after));
    }
}

pub proof fn epm_push_live_action_dispatches(
    before: crate::LiveRuntimeProductState,
    observed: source::EpmPushLiveObservation,
)
    requires epm_push_live_ready(before, observed),
    ensures match epm_push_live_after(before, observed) {
        Some(after) => {
            &&& crate::live_runtime_product_step(
                before, after, crate::LiveRuntimeProductAction::EpmPush { observed },
            )
            &&& crate::live_runtime_product_wf(after)
        },
        None => false,
    },
{
    epm_push_live_action_is_checked(before, observed);
    let after = epm_push_live_after(before, observed).unwrap();
    assert(crate::live_runtime_product_step(
        before, after, crate::LiveRuntimeProductAction::EpmPush { observed },
    ));
}

}
