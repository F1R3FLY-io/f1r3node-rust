use vstd::prelude::*;

use super::*;
use super::rpb3_generic_message_eof_source_refinement as generic_message;
use super::rpb3_message_driver_source_integration as message_source;

verus! {

pub open spec fn checked_map_push_source_receipt(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    receipt: message_source::GeneratedMessagePushReceipt,
) -> bool {
    &&& receipt.request.request_kind == runtime_source::SourceFrameRequestKind::Map
    &&& receipt.request.child.frame_kind == 2
    &&& message_source::generated_message_push_source_step(
        source, folded, receipt,
    ).is_some()
}

pub proof fn checked_map_push_receipt_projects_body_and_anchors(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    receipt: message_source::GeneratedMessagePushReceipt,
)
    requires checked_map_push_source_receipt(source, folded, receipt),
    ensures {
        let field = receipt.field;
        let body = admission::admission_indexed_source_subview(
            source.indexed, field.payload_start, field.payload_end,
        ).unwrap();
        &&& message_source::generated_message_push_receipt_matches(
            source, folded, receipt,
        )
        &&& admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: source.body_len,
                next: folded.cursor,
                source_bytes: source.indexed.bytes,
            }, field,
        )
        &&& admission::admission_indexed_source_subview(
            source.indexed, field.payload_start, field.payload_end,
        ).is_some()
        &&& body.bytes == field.payload_bytes
        &&& body.positions == source.indexed.positions.subrange(
            field.payload_start as int, field.payload_end as int,
        )
        &&& body.end == receipt.child_end
        &&& receipt.request.field == field
        &&& receipt.request.child.source_bytes == body.bytes
        &&& receipt.request.child.physical_source == receipt.child_start.source
        &&& receipt.request.child.physical_start == receipt.child_start.offset
        &&& admission::admission_indexed_source_position(
            source.indexed, field.payload_start,
        ) == Some(receipt.child_start)
        &&& admission::admission_indexed_source_position(
            source.indexed, field.payload_end,
        ) == Some(receipt.child_end)
    },
{
    let field = receipt.field;
    assert(message_source::generated_message_push_receipt_matches(
        source, folded, receipt,
    ));
    assert(generic_message::generic_message_source_event_step(
        folded,
        generic_message::GenericMessageSourceEvent::Push {
            field, body_id: receipt.body_id,
        },
        source.program_type_id, source.family, source.slot_count,
        source.body_len, source.indexed,
    ).is_some());
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: source.body_len,
            next: folded.cursor,
            source_bytes: source.indexed.bytes,
        }, field,
    ));
    assert(admission::admission_indexed_source_subview(
        source.indexed, field.payload_start, field.payload_end,
    ).is_some());
}

pub proof fn checked_map_push_receipt_reconciles_live_parent(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
)
    requires
        checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
    ensures {
        let parent = live.source.product.bridge.admission.frames.last();
        let parent_region = live.source.product.indexed_regions.last();
        let child = product_indexed_child(parent_region, receipt.field).unwrap();
        &&& source.indexed == product_admission_source_image(parent_region)
        &&& product_indexed_child(parent_region, receipt.field).is_some()
        &&& admission::admission_indexed_field_child_source(
            parent, source.indexed, receipt.field,
        ) == Some(product_admission_source_image(child))
        &&& product_admission_source_image(child).bytes
            == receipt.request.child.source_bytes
        &&& admission::admission_indexed_source_position(
            product_admission_source_image(child), 0,
        ) == Some(receipt.child_start)
        &&& product_admission_source_image(child).end == receipt.child_end
    },
{
    checked_map_push_receipt_projects_body_and_anchors(source, folded, receipt);
    let parent = live.source.product.bridge.admission.frames.last();
    let parent_region = live.source.product.indexed_regions.last();
    assert(admission::indexed_field_at_cursor(
        admission::IndexedCanonicalFieldCursorModel {
            region_start: parent.region_start,
            region_end: parent.region_end,
            next: parent.cursor,
            source_bytes: parent.source_bytes,
        }, receipt.field,
    ));
    assert(epm::indexed_image_wf(parent_region));
    product_indexed_child_source_anchor_exact(parent, parent_region, receipt.field);
}

pub struct CheckedMapPushViewTriple {
    pub ghost selected: runtime_source::SourceSelectedFrameView,
    pub ghost requested: runtime_source::SourceSelectedFrameView,
    pub ghost retained: runtime_source::SourceSelectedFrameView,
}

pub open spec fn checked_map_push_view_triple_matches(
    source: message_source::GeneratedMessageFieldSource,
    receipt: message_source::GeneratedMessagePushReceipt,
    views: CheckedMapPushViewTriple,
) -> bool {
    let child = admission::admission_indexed_source_subview(
        source.indexed, receipt.field.payload_start, receipt.field.payload_end,
    );
    &&& child.is_some()
    &&& views.selected.indexed == child.unwrap()
    &&& runtime_source::source_selected_frame_view_matches(
        views.selected, views.requested,
    )
    &&& runtime_source::source_selected_frame_view_matches(
        views.requested, views.retained,
    )
}

pub open spec fn checked_map_push_request_is_fresh_child(
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
) -> bool {
    let parent = live.source.product.map.admission.frames.last();
    let fresh = map::map_fresh_child_from_field(parent, receipt.field);
    &&& fresh.is_some()
    &&& receipt.request.child == admission::AdmissionFrameState {
        physical_source: receipt.child_start.source,
        physical_start: receipt.child_start.offset,
        ..fresh.unwrap()
    }
}

pub proof fn checked_map_push_view_and_fresh_child_select_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    views: CheckedMapPushViewTriple,
)
    requires
        checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
        checked_map_push_view_triple_matches(source, receipt, views),
        checked_map_push_request_is_fresh_child(live, receipt),
    ensures {
        let parent = live.source.product.map.admission.frames.last();
        let child = product_indexed_child(
            live.source.product.indexed_regions.last(), receipt.field,
        ).unwrap();
        let action = admission::ConcreteFrameActionModel::PushIndexed {
            field: receipt.field,
            child: receipt.request.child,
            parent_source: source.indexed,
            frame_bytes: live.source.runtime.frame_bytes,
        };
        &&& runtime_source::source_selected_frame_view_matches(
            views.selected, views.retained,
        )
        &&& views.retained.indexed == product_admission_source_image(child)
        &&& admission::admission_indexed_field_child_source(
            parent, source.indexed, receipt.field,
        ) == Some(views.retained.indexed)
        &&& map::integrated_source_message_action(
            live.source.product.map.admission, action,
        ) == Some(action)
    },
{
    checked_map_push_receipt_reconciles_live_parent(
        source, folded, live, receipt,
    );
    let child = product_indexed_child(
        live.source.product.indexed_regions.last(), receipt.field,
    ).unwrap();
    let image = product_admission_source_image(child);
    assert(admission::admission_indexed_source_subview(
        source.indexed, receipt.field.payload_start, receipt.field.payload_end,
    ) == Some(image));
    assert(views.selected.indexed == image);
    assert(views.requested.indexed == views.selected.indexed);
    assert(views.retained.indexed == views.requested.indexed);
    assert(views.selected.identity == views.requested.identity);
    assert(views.requested.identity == views.retained.identity);
    assert(runtime_source::source_selected_frame_view_matches(
        views.selected, views.retained,
    ));
    let parent = live.source.product.map.admission.frames.last();
    assert(map::map_fresh_child_from_field(parent, receipt.field).is_some());
    assert(admission::admission_indexed_source_position(
        source.indexed, receipt.field.payload_start,
    ) == Some(receipt.child_start));
}

pub proof fn equal_map_push_images_do_not_prove_same_view()
    ensures {
        let image = admission::AdmissionIndexedSourceImage {
            bytes: seq![0x08u8],
            positions: seq![admission::AdmissionSourceAnchor {
                source: 7, offset: 19,
            }],
            end: admission::AdmissionSourceAnchor { source: 7, offset: 20 },
        };
        let selected = runtime_source::SourceSelectedFrameView {
            identity: 41, indexed: image,
        };
        let retained = runtime_source::SourceSelectedFrameView {
            identity: 42, indexed: image,
        };
        &&& admission::admission_indexed_source_wf(image)
        &&& selected.indexed == retained.indexed
        &&& selected.indexed.bytes == retained.indexed.bytes
        &&& selected.indexed.positions == retained.indexed.positions
        &&& selected.indexed.end == retained.indexed.end
        &&& !runtime_source::source_selected_frame_view_matches(
            selected, retained,
        )
    },
{}

}
