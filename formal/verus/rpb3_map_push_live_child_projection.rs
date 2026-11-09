use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_finite_observation as finite, rpb3_map_push_live_parent_bounds as parent_bounds,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_map_push_selected_subview_extraction as selected_source,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub struct MapPushObservedFreshChild {
    pub owner_type_id: u16,
    pub outer_field: u32,
    pub value_type_id: u16,
    pub physical_source: usize,
    pub physical_start: usize,
    pub body_len: usize,
    pub ghost body_bytes: Seq<u8>,
    pub cursor_at: usize,
    pub key_present: bool,
    pub key_position_present: bool,
    pub saw_value: bool,
    pub value_position_present: bool,
    pub value_present: bool,
    pub waiting_value: bool,
}

pub open spec fn map_push_observed_child_is_initial(
    observed: MapPushObservedFreshChild,
) -> bool {
    &&& observed.cursor_at == 0
    &&& !observed.key_present
    &&& !observed.key_position_present
    &&& !observed.saw_value
    &&& !observed.value_position_present
    &&& !observed.value_present
    &&& !observed.waiting_value
}

pub open spec fn map_push_project_observed_child(
    observed: MapPushObservedFreshChild,
) -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        physical_source: observed.physical_source,
        physical_start: observed.physical_start,
        program_type_id: observed.value_type_id,
        region_start: 0,
        region_end: observed.body_len,
        source_bytes: observed.body_bytes,
        cursor: observed.cursor_at,
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
    }
}

pub open spec fn checked_map_push_observed_child_matches_source(
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: MapPushObservedFreshChild,
) -> bool {
    if live.source.product.map.admission.frames.len() == 0 {
        false
    } else {
        let parent = live.source.product.map.admission.frames.last();
        match map::map_fresh_child_from_field(parent, receipt.field) {
            None => false,
            Some(fresh) => {
                &&& observed.owner_type_id == parent.program_type_id
                &&& observed.outer_field == receipt.field.number
                &&& observed.value_type_id == fresh.program_type_id
                &&& observed.physical_source == receipt.child_start.source
                &&& observed.physical_start == receipt.child_start.offset
                &&& observed.body_len == fresh.region_end
                &&& observed.body_bytes == receipt.field.payload_bytes
                &&& map_push_observed_child_is_initial(observed)
            },
        }
    }
}

pub proof fn checked_map_push_observed_child_projects_full_fresh_state(
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: MapPushObservedFreshChild,
)
    requires checked_map_push_observed_child_matches_source(
        live, receipt, observed,
    ),
    ensures {
        let parent = live.source.product.map.admission.frames.last();
        let fresh = map::map_fresh_child_from_field(parent, receipt.field).unwrap();
        let anchored = admission::AdmissionFrameState {
            physical_source: receipt.child_start.source,
            physical_start: receipt.child_start.offset,
            ..fresh
        };
        &&& map_push_observed_child_is_initial(observed)
        &&& map_push_project_observed_child(observed) == anchored
    },
{
    let parent = live.source.product.map.admission.frames.last();
    let fresh = map::map_fresh_child_from_field(parent, receipt.field).unwrap();
    assert(fresh.region_start == 0);
    assert(fresh.source_bytes == receipt.field.payload_bytes);
    assert(fresh.cursor == 0);
    assert(fresh.slot_count == 2);
    assert(fresh.frame_kind == 2);
}

pub proof fn checked_map_push_observed_child_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: selected_source::CheckedMapPushSelectedSourceObservation,
    views: finite::CheckedMapPushFiniteObservation,
    observed: MapPushObservedFreshChild,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        checked_map_push_observed_child_matches_source(live, receipt, observed),
        receipt.request.child == map_push_project_observed_child(observed),
        selected_source::checked_map_push_selected_source_matches_image(
            source, receipt, selected,
        ),
        views.classes.selected == 0,
        views.classes.requested == 0,
        views.classes.retained == 0,
        views.selected_image == selected.selected_image,
        views.requested_image == views.selected_image,
        views.retained_image == views.requested_image,
    ensures {
        let action = admission::ConcreteFrameActionModel::PushIndexed {
            field: receipt.field,
            child: receipt.request.child,
            parent_source: source.indexed,
            frame_bytes: live.source.runtime.frame_bytes,
        };
        map::integrated_source_message_action(
            live.source.product.map.admission, action,
        ) == Some(action)
    },
{
    checked_map_push_observed_child_projects_full_fresh_state(
        live, receipt, observed,
    );
    assert(map_push::checked_map_push_request_is_fresh_child(live, receipt));
    parent_bounds::checked_map_push_source_bounds_select_indexed_action(
        source, folded, live, receipt, selected, views,
    );
}

pub proof fn pending_value_forgery_preserves_bytes_but_is_not_initial()
    ensures {
        let initial = MapPushObservedFreshChild {
            owner_type_id: 20,
            outer_field: 4,
            value_type_id: 1,
            physical_source: 7,
            physical_start: 19,
            body_len: 1,
            body_bytes: seq![0x08u8],
            cursor_at: 0,
            key_present: false,
            key_position_present: false,
            saw_value: false,
            value_position_present: false,
            value_present: false,
            waiting_value: false,
        };
        let forged = MapPushObservedFreshChild {
            waiting_value: true,
            ..initial
        };
        &&& map_push_observed_child_is_initial(initial)
        &&& initial.body_bytes == forged.body_bytes
        &&& initial.physical_source == forged.physical_source
        &&& initial.physical_start == forged.physical_start
        &&& !map_push_observed_child_is_initial(forged)
    },
{}

}
