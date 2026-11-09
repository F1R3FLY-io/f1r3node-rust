use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_finite_observation as finite,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_map_push_selected_subview_extraction as selected_source,
    rpb3_message_driver_source_integration as message_source,
    rpb3_wire_cursor_source_refinement as wire_source, *,
};

verus! {

pub proof fn checked_map_push_live_parent_has_full_body_bounds(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
    ensures
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
{
    map_push::checked_map_push_receipt_projects_body_and_anchors(
        source, folded, receipt,
    );
    let frames = live.source.product.bridge.admission.frames;
    assert(frames.len() > 0);
    let i = frames.len() - 1;
    let frame = frames[i];
    assert(live_runtime_product_wf(live));
    assert(live_source_product_wf(live.source));
    assert(frame == frames.last());
    assert(frame.frame_kind == 1);
    assert(live.source.wire.len() == frames.len());
    assert(live.cursors.len() == live.source.wire.len());
    assert(match live.source.wire[i] {
        None => frame.frame_kind == 3 || frame.frame_kind == 4,
        Some(reader) => reader.cursor.region_start == frame.region_start
            && reader.cursor.region_end == frame.region_end
            && reader.cursor.source_bytes == frame.source_bytes,
    });
    assert(live.source.wire[i].is_some());
    let reader = live.source.wire[i].unwrap();
    assert(reader.cursor.region_start == frame.region_start);
    assert(reader.cursor.region_end == frame.region_end);
    assert(reader.cursor.source_bytes == frame.source_bytes);
    assert(match (live.cursors[i], live.source.wire[i]) {
        (None, None) => true,
        (Some(cursor), Some(projected)) =>
            wire_source::wire_runtime_cursor_project(cursor) == Some(projected),
        _ => false,
    });
    assert(live.cursors[i].is_some());
    let cursor = live.cursors[i].unwrap();
    assert(wire_source::wire_runtime_cursor_project(cursor) == Some(reader));
    assert(wire_source::wire_runtime_derive_cursor(cursor).is_some());
    let concrete = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_source::wire_concrete_cursor_project(concrete) == Some(reader));
    assert(wire_source::wire_concrete_cursor_wf(concrete));
    assert(wire_source::wire_backing_project(concrete.message).is_some());
    let body = wire_source::wire_backing_project(concrete.message).unwrap();
    assert(reader.cursor.region_start == 0);
    assert(reader.cursor.region_end as int == body.bytes.len());
    assert(reader.cursor.source_bytes == body.bytes);
    assert(frame.source_bytes == source.indexed.bytes);
    assert(source.body_len as int == source.indexed.bytes.len());
    assert(frame.region_start == 0);
    assert(frame.region_end == source.body_len);
}

pub proof fn checked_map_push_source_bounds_select_indexed_action(
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
        map_push::checked_map_push_request_is_fresh_child(live, receipt),
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
    checked_map_push_live_parent_has_full_body_bounds(
        source, folded, live, receipt,
    );
    selected_source::checked_map_push_selected_source_selects_indexed_action(
        source, folded, live, receipt, selected, observed,
    );
}

pub proof fn narrowed_message_window_disagrees_with_concrete_cursor()
    ensures {
        let backing = wire_source::WireBackingImage::Borrowed {
            bytes_root: seq![0x08u8, 0x01u8],
            source: 7,
            absolute_start: 19,
            start: 0,
            len: 2,
            end_override: None,
        };
        let concrete = wire_source::WireConcreteCursorImage {
            message: backing,
            reader_view: backing,
            reader_at: 0,
            cursor_yielded: 0,
            last_field: 0,
            enforce_numeric_order: false,
            fields_read: 0,
            terminal_error: None,
        };
        let projected = wire_source::wire_concrete_cursor_project(concrete);
        &&& projected.is_some()
        &&& projected.unwrap().cursor.region_start == 0
        &&& projected.unwrap().cursor.region_end == 2
        &&& projected.unwrap().cursor.region_start != 1
        &&& projected.unwrap().cursor.region_end != 1
    },
{}

}
