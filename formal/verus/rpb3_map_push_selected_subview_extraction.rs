use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_finite_observation as finite,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub struct CheckedMapPushSelectedSourceObservation {
    pub ghost parent_image: admission::AdmissionIndexedSourceImage,
    pub payload_start: usize,
    pub payload_end: usize,
    pub ghost selected_image: admission::AdmissionIndexedSourceImage,
}

pub open spec fn checked_map_push_selected_source_matches_image(
    source: message_source::GeneratedMessageFieldSource,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: CheckedMapPushSelectedSourceObservation,
) -> bool {
    &&& observed.parent_image == source.indexed
    &&& observed.payload_start == receipt.field.payload_start
    &&& observed.payload_end == receipt.field.payload_end
    &&& admission::admission_indexed_source_wf(observed.parent_image)
    &&& observed.payload_start <= observed.payload_end
    &&& observed.payload_end as int <= observed.parent_image.bytes.len()
    &&& observed.selected_image.bytes == observed.parent_image.bytes.subrange(
        observed.payload_start as int, observed.payload_end as int,
    )
    &&& observed.selected_image.positions == observed.parent_image.positions.subrange(
        observed.payload_start as int, observed.payload_end as int,
    )
    &&& observed.selected_image.end == admission::admission_indexed_source_position(
        observed.parent_image, observed.payload_end,
    ).unwrap()
}

pub proof fn checked_map_push_selected_source_reconstructs_subview(
    source: message_source::GeneratedMessageFieldSource,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: CheckedMapPushSelectedSourceObservation,
)
    requires checked_map_push_selected_source_matches_image(
        source, receipt, observed,
    ),
    ensures admission::admission_indexed_source_subview(
        source.indexed, receipt.field.payload_start,
        receipt.field.payload_end,
    ) == Some(observed.selected_image),
{
    let selected = admission::admission_indexed_source_subview(
        observed.parent_image, observed.payload_start,
        observed.payload_end,
    ).unwrap();
    assert(selected.bytes == observed.selected_image.bytes);
    assert(selected.positions == observed.selected_image.positions);
    assert(selected.end == observed.selected_image.end);
    assert(selected == observed.selected_image);
}

pub proof fn checked_map_push_selected_source_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: CheckedMapPushSelectedSourceObservation,
    observed: finite::CheckedMapPushFiniteObservation,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
        map_push::checked_map_push_request_is_fresh_child(live, receipt),
        checked_map_push_selected_source_matches_image(
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
    checked_map_push_selected_source_reconstructs_subview(
        source, receipt, selected,
    );
    assert(finite::checked_map_push_finite_observation_matches_source(
        source, receipt, observed,
    ));
    finite::checked_map_push_finite_observation_selects_indexed_action(
        source, folded, live, receipt, observed,
    );
}

pub proof fn changed_terminal_anchor_does_not_reconstruct_selected_subview()
    ensures {
        let parent = admission::AdmissionIndexedSourceImage {
            bytes: seq![0x08u8],
            positions: seq![admission::AdmissionSourceAnchor {
                source: 7, offset: 19,
            }],
            end: admission::AdmissionSourceAnchor { source: 7, offset: 20 },
        };
        let altered = admission::AdmissionIndexedSourceImage {
            bytes: parent.bytes,
            positions: parent.positions,
            end: admission::AdmissionSourceAnchor { source: 7, offset: 21 },
        };
        &&& admission::admission_indexed_source_wf(parent)
        &&& altered.bytes == parent.bytes
        &&& altered.positions == parent.positions
        &&& altered.end != parent.end
        &&& admission::admission_indexed_source_subview(parent, 0, 1)
            != Some(altered)
    },
{}

}
