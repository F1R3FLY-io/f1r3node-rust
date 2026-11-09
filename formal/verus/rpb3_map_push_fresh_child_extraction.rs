use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_finite_observation as finite, rpb3_map_push_live_parent_bounds as parent_bounds,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_map_push_selected_subview_extraction as selected_source,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub open spec fn map_push_child_matches_observable_source_core(
    actual: admission::AdmissionFrameState,
    expected: admission::AdmissionFrameState,
) -> bool {
    &&& actual.physical_source == expected.physical_source
    &&& actual.physical_start == expected.physical_start
    &&& actual.program_type_id == expected.program_type_id
    &&& actual.region_start == expected.region_start
    &&& actual.region_end == expected.region_end
    &&& actual.source_bytes == expected.source_bytes
    &&& actual.cursor == expected.cursor
    &&& actual.slot_count == expected.slot_count
    &&& actual.last_slot == expected.last_slot
    &&& actual.seen == expected.seen
    &&& actual.scalar_slots == expected.scalar_slots
    &&& actual.scalar_occurrences == expected.scalar_occurrences
    &&& actual.returns == expected.returns
    &&& actual.waiting_id == expected.waiting_id
    &&& actual.waiting_kind == expected.waiting_kind
    &&& actual.waiting_slot == expected.waiting_slot
    &&& actual.waiting_tag == expected.waiting_tag
    &&& actual.return_id == expected.return_id
    &&& actual.frame_kind == expected.frame_kind
}

pub open spec fn map_push_child_has_model_defaults(
    child: admission::AdmissionFrameState,
) -> bool {
    &&& child.required == Set::empty()
    &&& child.family == admission::AdmissionFamily::Other
    &&& child.body_id == 0
    &&& child.score_handle.is_none()
    &&& child.nested_started == 0
    &&& child.nested_resolved == 0
}

pub open spec fn checked_map_push_child_matches_source_core(
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
) -> bool {
    if live.source.product.map.admission.frames.len() == 0 {
        false
    } else {
        let parent = live.source.product.map.admission.frames.last();
        match map::map_fresh_child_from_field(parent, receipt.field) {
            None => false,
            Some(fresh) => {
                let expected = admission::AdmissionFrameState {
                    physical_source: receipt.child_start.source,
                    physical_start: receipt.child_start.offset,
                    ..fresh
                };
                map_push_child_matches_observable_source_core(
                    receipt.request.child, expected,
                )
            },
        }
    }
}

pub proof fn checked_map_push_source_core_and_defaults_make_fresh_child(
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
)
    requires
        checked_map_push_child_matches_source_core(live, receipt),
        map_push_child_has_model_defaults(receipt.request.child),
    ensures map_push::checked_map_push_request_is_fresh_child(
        live, receipt,
    ),
{
    let parent = live.source.product.map.admission.frames.last();
    let fresh = map::map_fresh_child_from_field(parent, receipt.field).unwrap();
    let expected = admission::AdmissionFrameState {
        physical_source: receipt.child_start.source,
        physical_start: receipt.child_start.offset,
        ..fresh
    };
    assert(map_push_child_matches_observable_source_core(
        receipt.request.child, expected,
    ));
    assert(map_push_child_has_model_defaults(expected));
    assert(receipt.request.child == expected);
}

pub proof fn checked_map_push_source_core_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: selected_source::CheckedMapPushSelectedSourceObservation,
    observed: finite::CheckedMapPushFiniteObservation,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        checked_map_push_child_matches_source_core(live, receipt),
        map_push_child_has_model_defaults(receipt.request.child),
        selected_source::checked_map_push_selected_source_matches_image(
            source, receipt, selected,
        ),
        observed.classes.selected == 0,
        observed.classes.requested == 0,
        observed.classes.retained == 0,
        observed.selected_image == selected.selected_image,
        observed.requested_image == observed.selected_image,
        observed.retained_image == observed.requested_image,
    ensures {
        let child = product_indexed_child(
            live.source.product.indexed_regions.last(), receipt.field,
        ).unwrap();
        let action = admission::ConcreteFrameActionModel::PushIndexed {
            field: receipt.field,
            child: receipt.request.child,
            parent_source: source.indexed,
            frame_bytes: live.source.runtime.frame_bytes,
        };
        &&& product_admission_source_image(child) == observed.retained_image
        &&& map::integrated_source_message_action(
            live.source.product.map.admission, action,
        ) == Some(action)
    },
{
    checked_map_push_source_core_and_defaults_make_fresh_child(
        live, receipt,
    );
    parent_bounds::checked_map_push_source_bounds_select_indexed_action(
        source, folded, live, receipt, selected, observed,
    );
}

pub proof fn forged_body_id_preserves_source_core_but_not_freshness()
    ensures {
        let expected = admission::AdmissionFrameState {
            physical_source: 7,
            physical_start: 19,
            program_type_id: 1,
            region_start: 0,
            region_end: 1,
            source_bytes: seq![0x08u8],
            cursor: 0,
            slot_count: 2,
            last_slot: None,
            seen: Set::empty(),
            required: Set::empty(),
            scalar_slots: Set::empty(),
            scalar_occurrences: Seq::empty(),
            returns: Seq::empty(),
            family: admission::AdmissionFamily::Other,
            waiting_id: None,
            waiting_kind: 0,
            waiting_slot: None,
            waiting_tag: None,
            return_id: None,
            frame_kind: 2,
            body_id: 0,
            score_handle: None,
            nested_started: 0,
            nested_resolved: 0,
        };
        let forged = admission::AdmissionFrameState {
            body_id: 1,
            ..expected
        };
        &&& map_push_child_matches_observable_source_core(forged, expected)
        &&& map_push_child_has_model_defaults(expected)
        &&& !map_push_child_has_model_defaults(forged)
        &&& forged != expected
    },
{}

}
