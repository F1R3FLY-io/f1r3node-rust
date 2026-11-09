use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_map_push_scoped_identity_classes as scoped,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub struct CheckedMapPushFiniteObservation {
    pub classes: scoped::ScopedMapPushIdentityClasses,
    pub ghost selected_image: admission::AdmissionIndexedSourceImage,
    pub ghost requested_image: admission::AdmissionIndexedSourceImage,
    pub ghost retained_image: admission::AdmissionIndexedSourceImage,
}

pub open spec fn checked_map_push_finite_observation_matches_source(
    source: message_source::GeneratedMessageFieldSource,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: CheckedMapPushFiniteObservation,
) -> bool {
    &&& observed.classes.selected == 0
    &&& observed.classes.requested == 0
    &&& observed.classes.retained == 0
    &&& observed.selected_image == observed.requested_image
    &&& observed.requested_image == observed.retained_image
    &&& admission::admission_indexed_source_subview(
        source.indexed, receipt.field.payload_start,
        receipt.field.payload_end,
    ) == Some(observed.selected_image)
}

pub open spec fn checked_map_push_finite_symbolic_views(
    observed: CheckedMapPushFiniteObservation,
) -> map_push::CheckedMapPushViewTriple {
    map_push::CheckedMapPushViewTriple {
        selected: runtime_source::SourceSelectedFrameView {
            identity: observed.classes.selected,
            indexed: observed.selected_image,
        },
        requested: runtime_source::SourceSelectedFrameView {
            identity: observed.classes.requested,
            indexed: observed.requested_image,
        },
        retained: runtime_source::SourceSelectedFrameView {
            identity: observed.classes.retained,
            indexed: observed.retained_image,
        },
    }
}

pub proof fn checked_map_push_finite_observation_selects_symbolic_views(
    source: message_source::GeneratedMessageFieldSource,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: CheckedMapPushFiniteObservation,
)
    requires checked_map_push_finite_observation_matches_source(
        source, receipt, observed,
    ),
    ensures map_push::checked_map_push_view_triple_matches(
        source, receipt, checked_map_push_finite_symbolic_views(observed),
    ),
{}

pub proof fn checked_map_push_finite_observation_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    observed: CheckedMapPushFiniteObservation,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
        map_push::checked_map_push_request_is_fresh_child(live, receipt),
        checked_map_push_finite_observation_matches_source(
            source, receipt, observed,
        ),
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
    checked_map_push_finite_observation_selects_symbolic_views(
        source, receipt, observed,
    );
    map_push::checked_map_push_view_and_fresh_child_select_indexed_action(
        source, folded, live, receipt,
        checked_map_push_finite_symbolic_views(observed),
    );
}

pub proof fn equal_map_push_images_with_different_classes_are_rejected()
    ensures {
        let image = admission::AdmissionIndexedSourceImage {
            bytes: seq![0x08u8],
            positions: seq![admission::AdmissionSourceAnchor {
                source: 7, offset: 19,
            }],
            end: admission::AdmissionSourceAnchor { source: 7, offset: 20 },
        };
        let observed = CheckedMapPushFiniteObservation {
            classes: scoped::ScopedMapPushIdentityClasses {
                selected: 0, requested: 1, retained: 0,
            },
            selected_image: image,
            requested_image: image,
            retained_image: image,
        };
        let views = checked_map_push_finite_symbolic_views(observed);
        &&& admission::admission_indexed_source_wf(image)
        &&& observed.selected_image == observed.requested_image
        &&& observed.requested_image == observed.retained_image
        &&& !runtime_source::source_selected_frame_view_matches(
            views.selected, views.requested,
        )
    },
{}

}
