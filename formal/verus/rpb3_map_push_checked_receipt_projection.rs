use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_finite_observation as finite,
    rpb3_map_push_live_child_projection as child_projection,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_map_push_selected_subview_extraction as selected_source,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub open spec fn map_push_receipt_from_checked_child_observation(
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
    published_body_id: u64,
) -> message_source::GeneratedMessagePushReceipt {
    let sourced = checked_message_push_receipt_from_field_before(template, before);
    message_source::GeneratedMessagePushReceipt {
        body_id: published_body_id,
        request: runtime_source::SourceFrameRequestImage {
            child: child_projection::map_push_project_observed_child(observed),
            ..sourced.request
        },
        ..sourced
    }
}

pub open spec fn map_push_receipt_from_live_cursor(
    live: LiveRuntimeProductState,
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
) -> Option<message_source::GeneratedMessagePushReceipt> {
    if live.cursors.len() == 0
        || live.source.product.bridge.admission.next_obligation_id == u64::MAX
    {
        None
    } else {
        match live.cursors.last() {
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field { field, .. }) => {
                    let sourced = map_push_receipt_from_checked_child_observation(
                        template, before, observed,
                        (live.source.product.bridge.admission.next_obligation_id as int + 1) as u64,
                    );
                    if sourced.request.field == field {
                        Some(message_source::GeneratedMessagePushReceipt { field, ..sourced })
                    } else {
                        None
                    }
                },
                _ => None,
            },
            None => None,
        }
    }
}

pub proof fn checked_map_push_live_cursor_rejects_forged_field(
    live: LiveRuntimeProductState,
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
    cursor: wire_source::WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    concrete_after: wire_source::WireConcreteCursorImage,
    start: wire_source::WirePhysicalPosition,
)
    requires
        live.cursors.len() > 0,
        live.cursors.last() == Some(cursor),
        wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
        template.request.field != field,
    ensures map_push_receipt_from_live_cursor(
        live, template, before, observed,
    ).is_none(),
{}

pub proof fn checked_map_push_live_cursor_discards_forged_template_field(
    live: LiveRuntimeProductState,
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
    cursor: wire_source::WireRuntimeCursorImage,
    field: admission::IndexedCanonicalFieldModel,
    concrete_after: wire_source::WireConcreteCursorImage,
    start: wire_source::WirePhysicalPosition,
)
    requires
        live.cursors.len() > 0,
        live.cursors.last() == Some(cursor),
        wire_source::wire_runtime_next_field(cursor)
            == Some(wire_source::WireConcreteTransition::Field {
                state: concrete_after, field, start,
            }),
        template.request.field == field,
        template.field != field,
        live.source.product.bridge.admission.next_obligation_id != u64::MAX,
    ensures {
        let receipt = map_push_receipt_from_live_cursor(
            live, template, before, observed,
        ).unwrap();
        &&& map_push_receipt_from_live_cursor(
            live, template, before, observed,
        ).is_some()
        &&& receipt.field == field
        &&& receipt.field != template.field
    },
{}

pub proof fn checked_map_push_live_cursor_selects_indexed_field(
    live: LiveRuntimeProductState,
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
)
    requires map_push_receipt_from_live_cursor(
        live, template, before, observed,
    ).is_some(),
    ensures {
        let receipt = map_push_receipt_from_live_cursor(
            live, template, before, observed,
        ).unwrap();
        &&& receipt.field == receipt.request.field
        &&& receipt.request.field == template.request.field
        &&& receipt.request.child
            == child_projection::map_push_project_observed_child(observed)
        &&& receipt.cursor_before == before.cursor
        &&& receipt.last_slot_before == before.last_slot
        &&& receipt.seen_before == before.seen
        &&& receipt.body_id as int
            == live.source.product.bridge.admission.next_obligation_id as int + 1
        &&& match live.cursors.last() {
            Some(cursor) => match wire_source::wire_runtime_next_field(cursor) {
                Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
                    receipt.field == field,
                _ => false,
            },
            None => false,
        }
    },
{}

pub proof fn checked_map_push_receipt_child_is_observed_projection(
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
    published_body_id: u64,
)
    ensures {
        let receipt = map_push_receipt_from_checked_child_observation(
            template, before, observed, published_body_id,
        );
        &&& receipt.request.child
            == child_projection::map_push_project_observed_child(observed)
        &&& receipt.body_id == published_body_id
        &&& receipt.field == template.field
        &&& receipt.child_start == template.child_start
        &&& receipt.child_end == template.child_end
        &&& receipt.request.request_kind == template.request.request_kind
        &&& receipt.request.field == template.request.field
        &&& receipt.cursor_before == before.cursor
        &&& receipt.last_slot_before == before.last_slot
        &&& receipt.seen_before == before.seen
    },
{}

pub proof fn checked_map_push_constructed_receipt_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    operations: Seq<message_source::GeneratedMessageSourceOperation>,
    live: LiveRuntimeProductState,
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    selected: selected_source::CheckedMapPushSelectedSourceObservation,
    views: finite::CheckedMapPushFiniteObservation,
    observed: child_projection::MapPushObservedFreshChild,
)
    requires
        message_source::generated_message_replay_from_source_operations(
            source, operations,
        ).is_some(),
    {
        let replay = message_source::generated_message_replay_from_source_operations(
            source, operations,
        ).unwrap();
        let folded = replay.fields;
        let maybe_receipt = map_push_receipt_from_live_cursor(
            live, template, before, observed,
        );
        &&& maybe_receipt.is_some()
        &&& map_push::checked_map_push_source_receipt(
            source, folded, maybe_receipt.unwrap(),
        )
        &&& checked_message_push_snapshot_matches_live(
            checked_message_push_snapshot_from_field_before(before), live,
        )
        &&& checked_message_push_trace_core_matches_live(source, operations, live)
        &&& child_projection::checked_map_push_observed_child_matches_source(
            live, maybe_receipt.unwrap(), observed,
        )
        &&& selected_source::checked_map_push_selected_source_matches_image(
            source, maybe_receipt.unwrap(), selected,
        )
        &&& views.classes.selected == 0
        &&& views.classes.requested == 0
        &&& views.classes.retained == 0
        &&& views.selected_image == selected.selected_image
        &&& views.requested_image == views.selected_image
        &&& views.retained_image == views.requested_image
    },
    ensures {
        let receipt = map_push_receipt_from_live_cursor(
            live, template, before, observed,
        ).unwrap();
        let cursor = live.cursors.last().unwrap();
        let action = admission::ConcreteFrameActionModel::PushIndexed {
            field: receipt.field,
            child: receipt.request.child,
            parent_source: source.indexed,
            frame_bytes: live.source.runtime.frame_bytes,
        };
        &&& source.indexed == product_admission_source_image(
            live.source.product.indexed_regions.last(),
        )
        &&& product_wire_indexed_source_agrees(
            cursor, live.source.product.indexed_regions.last(),
        )
        &&& match wire_source::wire_runtime_next_field(cursor) {
            Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
                receipt.field == field,
            _ => false,
        }
        &&& map::integrated_source_message_action(
            live.source.product.map.admission, action,
        ) == Some(action)
    },
{
    let replay = message_source::generated_message_replay_from_source_operations(
        source, operations,
    ).unwrap();
    let folded = replay.fields;
    let receipt = map_push_receipt_from_live_cursor(
        live, template, before, observed,
    ).unwrap();
    checked_message_push_snapshot_and_core_establish_live_fold(
        source, operations, live, before, template,
    );
    checked_map_push_live_cursor_selects_indexed_field(
        live, template, before, observed,
    );
    assert(live_runtime_product_wf(live));
    assert(source.indexed == product_admission_source_image(
        live.source.product.indexed_regions.last(),
    ));
    assert(product_wire_indexed_source_agrees(
        live.cursors.last().unwrap(),
        live.source.product.indexed_regions.last(),
    ));
    child_projection::checked_map_push_observed_child_selects_indexed_action(
        source, folded, live, receipt, selected, views, observed,
    );
}

pub proof fn source_bound_receipt_discards_forged_template_body_id(
    template: message_source::GeneratedMessagePushReceipt,
    before: CheckedMessageFieldBeforeImage,
    observed: child_projection::MapPushObservedFreshChild,
    published_body_id: u64,
)
    requires template.request.child.body_id != 0,
    ensures {
        let receipt = map_push_receipt_from_checked_child_observation(
            template, before, observed, published_body_id,
        );
        &&& receipt.request.child.body_id == 0
        &&& receipt.body_id == published_body_id
        &&& receipt.request.child != template.request.child
        &&& receipt.field == template.field
    },
{}

/// A five-byte New body containing one generated Map field. The Map request
/// is fresh (body ID zero); its publication receives obligation zero and body
/// ID one. This witness prevents a conditional Push proof from being vacuous.
pub open spec fn concrete_new_map_push_bytes() -> Seq<u8> {
    seq![0x22u8, 0x03u8, 0x0au8, 0x01u8, 0x21u8]
}

pub open spec fn concrete_new_map_push_root() -> admission::AdmissionFrameState {
    admission::AdmissionFrameState {
        region_end: 5,
        source_bytes: concrete_new_map_push_bytes(),
        body_id: 0,
        score_handle: None,
        ..admission::admission_family_witness_frame(
            admission::AdmissionFamily::New, Set::empty(), Seq::empty(),
        )
    }
}

pub open spec fn concrete_new_map_push_field() -> admission::IndexedCanonicalFieldModel {
    admission::IndexedCanonicalFieldModel {
        number: 4,
        encoded_start: 0,
        encoded_end: 5,
        payload_start: 2,
        payload_end: 5,
        key_width: 1,
        payload_width: 1,
        payload_length: 3,
        wire: 2,
        decoded_varint: 0,
        payload_bytes: seq![0x0au8, 0x01u8, 0x21u8],
    }
}

pub open spec fn concrete_new_map_push_indexed_backing() -> epm::IndexedBackingImage {
    epm::IndexedBackingImage::Borrowed {
        bytes: concrete_new_map_push_bytes(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 5,
        end_override: None,
    }
}

pub open spec fn concrete_new_map_push_wire_backing()
    -> wire_source::WireRuntimeBackingImage {
    wire_source::WireRuntimeBackingImage::Borrowed {
        bytes: concrete_new_map_push_bytes(),
        source: 0,
        absolute_start: 0,
        start: 0,
        len: 5,
        end_override: None,
    }
}

#[verifier::opaque]
pub open spec fn concrete_new_map_push_live_before() -> Option<LiveRuntimeProductState> {
    match (
        product_root_initial(
            concrete_new_map_push_root(), concrete_new_map_push_indexed_backing(),
        ),
        wire_source::wire_runtime_initial(
            concrete_new_map_push_wire_backing(), false,
        ),
    ) {
        (Some(product), Some(cursor)) => match
            wire_source::wire_runtime_cursor_project(cursor)
        {
            Some(reader) => Some(LiveRuntimeProductState {
                source: LiveSourceProductState {
                    product,
                    runtime: runtime_source::RuntimeFrameImage {
                        score: product.bridge,
                        capacity: 1,
                        frame_bytes: 1,
                        max_frames: 2,
                        max_storage_bytes: 2,
                        max_capacity_bytes: 2,
                    },
                    arena: product_runtime_arena_from_score(
                        product.bridge.transcript,
                    ),
                    wire: seq![Some(reader)],
                },
                cursors: seq![Some(cursor)],
            }),
            None => None,
        },
        _ => None,
    }
}

pub open spec fn concrete_new_map_push_source(
    live: LiveRuntimeProductState,
) -> message_source::GeneratedMessageFieldSource {
    message_source::GeneratedMessageFieldSource {
        program_type_id: 20,
        family: admission::AdmissionFamily::New,
        slot_count: 5,
        body_id: 0,
        body_len: 5,
        indexed: product_admission_source_image(
            live.source.product.indexed_regions.last(),
        ),
    }
}

#[verifier::opaque]
pub open spec fn concrete_new_map_push_receipt()
    -> Option<message_source::GeneratedMessagePushReceipt> {
    let field = concrete_new_map_push_field();
    match map::map_fresh_child_from_field(concrete_new_map_push_root(), field) {
        Some(child) => Some(message_source::GeneratedMessagePushReceipt {
            field,
            slot_count: 5,
            slot: 3,
            repeated: true,
            oneof_arm: None,
            child_type_id: Some(1),
            child_start: admission::AdmissionSourceAnchor { source: 0, offset: 2 },
            child_end: admission::AdmissionSourceAnchor { source: 0, offset: 5 },
            body_id: 1,
            cursor_before: 0,
            cursor_after: 5,
            last_slot_before: None,
            last_slot_after: Some(3),
            seen_before: false,
            seen_after: true,
            request: runtime_source::SourceFrameRequestImage {
                request_kind: runtime_source::SourceFrameRequestKind::Map,
                field,
                child,
            },
            actual_top_kind: Some(runtime_source::SourceFrameRequestKind::Map),
            after_len: 2,
            after_capacity: 2,
            stats_frame_variant_bytes: 1,
        }),
        None => None,
    }
}

pub proof fn concrete_new_map_push_bytes_facts()
    ensures
        concrete_new_map_push_bytes().len() == 5,
        concrete_new_map_push_bytes()[0] == 0x22u8,
        concrete_new_map_push_bytes()[1] == 0x03u8,
        concrete_new_map_push_bytes()[2] == 0x0au8,
        concrete_new_map_push_bytes()[3] == 0x01u8,
        concrete_new_map_push_bytes()[4] == 0x21u8,
        concrete_new_map_push_bytes().subrange(0, 5) =~= concrete_new_map_push_bytes(),
        concrete_new_map_push_bytes().subrange(2, 5)
            =~= concrete_new_map_push_field().payload_bytes,
{
    let bytes = concrete_new_map_push_bytes();
    assert(bytes.len() == 5);
    assert(bytes[0] == 0x22u8);
    assert(bytes[1] == 0x03u8);
    assert(bytes[2] == 0x0au8);
    assert(bytes[3] == 0x01u8);
    assert(bytes[4] == 0x21u8);
    assert(bytes.subrange(0, 5) =~= bytes);
    let payload = concrete_new_map_push_field().payload_bytes;
    assert(payload.len() == 3);
    assert(payload[0] == 0x0au8);
    assert(payload[1] == 0x01u8);
    assert(payload[2] == 0x21u8);
    assert(bytes.subrange(2, 5) =~= payload);
}

pub proof fn concrete_new_map_push_varints()
    ensures
        wire_source::wire_varint_read(concrete_new_map_push_bytes(), 0, 5)
            == (wire_source::WireVarintResult::Value { value: 34, next: 1 }),
        wire_source::wire_varint_read(concrete_new_map_push_bytes(), 1, 5)
            == (wire_source::WireVarintResult::Value { value: 3, next: 2 }),
{
    concrete_new_map_push_bytes_facts();
    let bytes = concrete_new_map_push_bytes();
    assert(admission::varint_multiplier(0) == 1);
    assert(wire_source::wire_varint_loop(bytes, 0, 5, 0, 0)
        == wire_source::WireVarintResult::Value { value: 34, next: 1 });
    assert(wire_source::wire_varint_loop(bytes, 1, 5, 0, 0)
        == wire_source::WireVarintResult::Value { value: 3, next: 2 });
}

pub proof fn concrete_new_map_push_field_step()
    ensures
        wire_source::wire_field_step(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: 0,
                region_end: 5,
                next: 0,
                source_bytes: concrete_new_map_push_bytes(),
            },
            0, false, false,
        ) == (wire_source::WireFieldResult::Field {
            field: concrete_new_map_push_field(), next: 5, last_field: 4,
        }),
{
    concrete_new_map_push_bytes_facts();
    concrete_new_map_push_varints();
    let bytes = concrete_new_map_push_bytes();
    let cursor = admission::IndexedCanonicalFieldCursorModel {
        region_start: 0,
        region_end: 5,
        next: 0,
        source_bytes: bytes,
    };
    assert(wire_source::wire_cursor_shape(cursor));
    assert(wire_source::wire_select_varint_read(bytes, 0, 5, false)
        == wire_source::WireVarintResult::Value { value: 34, next: 1 });
    assert(wire_source::wire_select_varint_read(bytes, 1, 5, false)
        == wire_source::WireVarintResult::Value { value: 3, next: 2 });
    assert(wire_source::wire_field_step(cursor, 0, false, false)
        == wire_source::WireFieldResult::Field {
            field: concrete_new_map_push_field(), next: 5, last_field: 4,
        });
}

pub proof fn concrete_new_map_root_initial_exact()
    ensures {
        let root = concrete_new_map_push_root();
        let indexed_backing = concrete_new_map_push_indexed_backing();
        &&& admission::admission_root_initial(root)
        &&& map::integrated_admission_initial(root).is_some()
        &&& epm::indexed_backing_project(indexed_backing).is_some()
        &&& product_root_initial(root, indexed_backing).is_some()
        &&& product_score_wf(product_root_initial(root, indexed_backing).unwrap())
    },
{
    let root = concrete_new_map_push_root();
    let indexed_backing = concrete_new_map_push_indexed_backing();
    assert forall|tag: u32|
        #[trigger] admission::admission_descriptor_not_pathmap(20, tag) by {
    }
    assert forall|slot: int|
        #[trigger] admission::admission_no_required_descriptor_slot(20, slot) by {
        assert forall|tag: u32|
            !#[trigger] admission::admission_required_descriptor_slot(20, tag, slot) by {
            assert(admission::admission_descriptor_not_pathmap(20, tag));
        }
    }
    assert(admission::admission_root_initial(root));
    map::integrated_initial_is_well_formed(root);
    assert(map::integrated_admission_initial(root).is_some());
    assert(epm::indexed_backing_project(indexed_backing).is_some());
    let indexed = epm::indexed_backing_project(indexed_backing).unwrap();
    assert(epm::indexed_image_wf(indexed));
    assert(indexed.bytes =~= root.source_bytes);
    assert(indexed.positions[0] == epm::SourceAnchor { source: 0, offset: 0 });
    assert(product_indexed_start_anchor(indexed)
        == epm::SourceAnchor { source: 0, offset: 0 });
    assert(root.physical_source == product_indexed_start_anchor(indexed).source);
    assert(root.physical_start == product_indexed_start_anchor(indexed).offset);
    assert(product_root_initial(root, indexed_backing).is_some());
    product_root_initial_exact(root, indexed_backing);
}

pub proof fn concrete_new_map_wire_cursor_projects()
    ensures {
        let backing = concrete_new_map_push_wire_backing();
        let region = wire_source::wire_runtime_project_region(backing).unwrap();
        let cursor = wire_source::wire_runtime_initial(backing, false).unwrap();
        let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
        let reader = wire_source::wire_concrete_cursor_project(derived).unwrap();
        &&& wire_source::wire_runtime_project_region(backing).is_some()
        &&& region.bytes == concrete_new_map_push_bytes()
        &&& region.anchors.len() == 5
        &&& region.anchors[0] == wire_source::WireSourceAnchor { source: 0, offset: 0 }
        &&& wire_source::wire_runtime_initial(backing, false).is_some()
        &&& wire_source::wire_runtime_derive_cursor(cursor).is_some()
        &&& wire_source::wire_concrete_cursor_wf(derived)
        &&& wire_source::wire_concrete_cursor_project(derived).is_some()
        &&& wire_source::wire_runtime_cursor_project(cursor) == Some(reader)
        &&& reader.cursor == (admission::IndexedCanonicalFieldCursorModel {
            region_start: 0,
            region_end: 5,
            next: 0,
            source_bytes: concrete_new_map_push_bytes(),
        })
        &&& reader.last_field == 0
        &&& !reader.enforce_numeric_order
        &&& reader.terminal_error.is_none()
    },
{
    concrete_new_map_push_bytes_facts();
    let backing = concrete_new_map_push_wire_backing();
    assert(wire_source::wire_runtime_project_region(backing).is_some());
    assert(wire_source::wire_runtime_initial(backing, false).is_some());
    let cursor = wire_source::wire_runtime_initial(backing, false).unwrap();
    let region = wire_source::wire_runtime_project_region(backing).unwrap();
    assert(region.bytes =~= concrete_new_map_push_bytes());
    assert(region.anchors.len() == 5);
    assert(region.anchors[0]
        == wire_source::WireSourceAnchor { source: 0, offset: 0 });
    assert(wire_source::wire_region_position(region, 0).is_some());
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    assert(wire_source::wire_concrete_cursor_wf(derived));
    let reader = wire_source::wire_concrete_cursor_project(derived).unwrap();
    assert(reader.cursor.source_bytes =~= region.bytes);
    assert(reader.cursor.region_start == 0);
    assert(reader.cursor.region_end == 5);
    assert(reader.cursor.next == 0);
    assert(reader.last_field == 0);
    assert(!reader.enforce_numeric_order);
    assert(reader.terminal_error.is_none());
}

pub proof fn concrete_new_map_runtime_field_exact()
    ensures
        wire_source::wire_runtime_initial(
            concrete_new_map_push_wire_backing(), false,
        ).is_some(),
        match wire_source::wire_runtime_next_field(
            wire_source::wire_runtime_initial(
                concrete_new_map_push_wire_backing(), false,
            ).unwrap(),
        ) {
            Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
                field == concrete_new_map_push_field(),
            _ => false,
        },
{
    concrete_new_map_wire_cursor_projects();
    concrete_new_map_push_field_step();
    let backing = concrete_new_map_push_wire_backing();
    let cursor = wire_source::wire_runtime_initial(backing, false).unwrap();
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    let source_cursor = wire_source::wire_concrete_cursor_project(derived).unwrap();
    assert(wire_source::wire_field_step(
        source_cursor.cursor, source_cursor.last_field,
        source_cursor.enforce_numeric_order, false,
    ) == wire_source::WireFieldResult::Field {
        field: concrete_new_map_push_field(), next: 5, last_field: 4,
    });
    assert(match wire_source::wire_source_next_field(source_cursor) {
        wire_source::WireSourceTransition::Field { field, .. } =>
            field == concrete_new_map_push_field(),
        _ => false,
    });
    assert(match wire_source::wire_concrete_next_field(derived) {
        Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
            field == concrete_new_map_push_field(),
        _ => false,
    });
    wire_source::wire_concrete_next_field_or_matches_model(derived);
    assert(match wire_source::wire_runtime_next_field(cursor) {
        Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
            field == concrete_new_map_push_field(),
        _ => false,
    });
}

pub proof fn concrete_new_map_live_before_wf()
    ensures
        concrete_new_map_push_live_before().is_some(),
        live_runtime_product_wf(concrete_new_map_push_live_before().unwrap()),
        concrete_new_map_push_live_before().unwrap().cursors
            == seq![Some(wire_source::wire_runtime_initial(
                concrete_new_map_push_wire_backing(), false,
            ).unwrap())],
        concrete_new_map_push_live_before().unwrap().cursors.last()
            == Some(wire_source::wire_runtime_initial(
                concrete_new_map_push_wire_backing(), false,
            ).unwrap()),
{
    reveal(concrete_new_map_push_live_before);
    concrete_new_map_root_initial_exact();
    concrete_new_map_wire_cursor_projects();
    assert(concrete_new_map_push_live_before().is_some());
    let live = concrete_new_map_push_live_before().unwrap();
    assert(live.source.runtime.score == live.source.product.bridge);
    assert(runtime_source::runtime_frame_image_wf(live.source.runtime));
    assert(arena_source::runtime_projects_score(
        live.source.arena, live.source.product.bridge.transcript,
    ));
    assert(live_runtime_product_wf(live));
}

/// Checkpoint 1: the concrete New root and its borrowed wire cursor agree on
/// the same generated tag-4 Map field before any Push model is expanded.
pub proof fn concrete_new_map_root_and_wire_exact()
    ensures {
        &&& concrete_new_map_push_live_before().is_some()
        &&& live_runtime_product_wf(concrete_new_map_push_live_before().unwrap())
        &&& match wire_source::wire_runtime_next_field(
            concrete_new_map_push_live_before().unwrap().cursors.last().unwrap(),
        ) {
            Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
                field == concrete_new_map_push_field(),
            _ => false,
        }
    },
{
    concrete_new_map_root_initial_exact();
    concrete_new_map_wire_cursor_projects();
    concrete_new_map_push_field_step();
    let backing = concrete_new_map_push_wire_backing();
    let cursor = wire_source::wire_runtime_initial(backing, false).unwrap();
    let derived = wire_source::wire_runtime_derive_cursor(cursor).unwrap();
    let source_cursor = wire_source::wire_concrete_cursor_project(derived).unwrap();
    assert(wire_source::wire_field_step(
        source_cursor.cursor, source_cursor.last_field,
        source_cursor.enforce_numeric_order, false,
    ) == wire_source::WireFieldResult::Field {
        field: concrete_new_map_push_field(), next: 5, last_field: 4,
    });
    concrete_new_map_runtime_field_exact();
    concrete_new_map_live_before_wf();
    let live = concrete_new_map_push_live_before().unwrap();
    assert(live.cursors.last() == Some(cursor));
    assert(match wire_source::wire_runtime_next_field(live.cursors.last().unwrap()) {
        Some(wire_source::WireConcreteTransition::Field { field, .. }) =>
            field == concrete_new_map_push_field(),
        _ => false,
    });
}

}
