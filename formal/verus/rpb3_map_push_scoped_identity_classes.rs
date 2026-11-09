use vstd::prelude::*;

use super::{
    rpb3_generic_message_eof_source_refinement as generic_message,
    rpb3_map_push_backing_identity as backing, rpb3_map_push_receipt_source_extraction as map_push,
    rpb3_message_driver_source_integration as message_source, *,
};

verus! {

pub struct ScopedMapPushIdentityClasses {
    pub selected: usize,
    pub requested: usize,
    pub retained: usize,
}

pub open spec fn scoped_map_push_identity_classes(
    selected: backing::ExactIndexedBackingKey,
    requested: backing::ExactIndexedBackingKey,
    retained: backing::ExactIndexedBackingKey,
) -> ScopedMapPushIdentityClasses {
    let requested_class = if backing::exact_indexed_same_view(selected, requested) {
        0
    } else {
        1
    };
    let retained_class = if backing::exact_indexed_same_view(selected, retained) {
        0
    } else if requested_class == 1
        && backing::exact_indexed_same_view(requested, retained)
    {
        1
    } else {
        2
    };
    ScopedMapPushIdentityClasses {
        selected: 0,
        requested: requested_class,
        retained: retained_class,
    }
}

pub open spec fn scoped_map_push_identity_classes_match_observation(
    selected: backing::ExactIndexedViewObservation,
    requested: backing::ExactIndexedViewObservation,
    retained: backing::ExactIndexedViewObservation,
    observed: ScopedMapPushIdentityClasses,
) -> bool {
    observed == scoped_map_push_identity_classes(
        selected.key, requested.key, retained.key,
    )
}

pub open spec fn scoped_map_push_symbolic_views(
    selected: backing::ExactIndexedViewObservation,
    requested: backing::ExactIndexedViewObservation,
    retained: backing::ExactIndexedViewObservation,
    observed: ScopedMapPushIdentityClasses,
) -> map_push::CheckedMapPushViewTriple {
    map_push::CheckedMapPushViewTriple {
        selected: runtime_source::SourceSelectedFrameView {
            identity: observed.selected, indexed: selected.image,
        },
        requested: runtime_source::SourceSelectedFrameView {
            identity: observed.requested, indexed: requested.image,
        },
        retained: runtime_source::SourceSelectedFrameView {
            identity: observed.retained, indexed: retained.image,
        },
    }
}

pub proof fn scoped_map_push_identity_classes_are_injective(
    selected: backing::ExactIndexedBackingKey,
    requested: backing::ExactIndexedBackingKey,
    retained: backing::ExactIndexedBackingKey,
)
    ensures {
        let classes = scoped_map_push_identity_classes(
            selected, requested, retained,
        );
        &&& (classes.selected == classes.requested)
            == backing::exact_indexed_same_view(selected, requested)
        &&& (classes.selected == classes.retained)
            == backing::exact_indexed_same_view(selected, retained)
        &&& (classes.requested == classes.retained)
            == backing::exact_indexed_same_view(requested, retained)
        &&& classes.selected == 0
        &&& classes.requested <= 1
        &&& classes.retained <= 2
    },
{
    backing::exact_indexed_same_view_is_key_equality(selected, requested);
    backing::exact_indexed_same_view_is_key_equality(selected, retained);
    backing::exact_indexed_same_view_is_key_equality(requested, retained);
}

pub proof fn checked_map_push_scoped_identity_selects_symbolic_views(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: backing::ExactIndexedViewObservation,
    requested: backing::ExactIndexedViewObservation,
    retained: backing::ExactIndexedViewObservation,
    observed: ScopedMapPushIdentityClasses,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        backing::exact_indexed_view_observation_wf(selected),
        backing::exact_indexed_view_observation_wf(requested),
        backing::exact_indexed_view_observation_wf(retained),
        scoped_map_push_identity_classes_match_observation(
            selected, requested, retained, observed,
        ),
        observed.selected == observed.requested,
        observed.requested == observed.retained,
        selected.image == requested.image,
        requested.image == retained.image,
        admission::admission_indexed_source_subview(
            source.indexed, receipt.field.payload_start,
            receipt.field.payload_end,
        ) == Some(selected.image),
    ensures
        observed.selected == 0,
        observed.requested == 0,
        observed.retained == 0,
        backing::exact_indexed_same_view(selected.key, requested.key),
        backing::exact_indexed_same_view(requested.key, retained.key),
        map_push::checked_map_push_view_triple_matches(
            source, receipt,
            scoped_map_push_symbolic_views(
                selected, requested, retained, observed,
            ),
        ),
{
    scoped_map_push_identity_classes_are_injective(
        selected.key, requested.key, retained.key,
    );
    assert(observed.selected == 0);
    assert(observed.requested == 0);
    assert(observed.retained == 0);
    backing::checked_map_push_exact_views_project_symbolic_identity(
        source, folded, receipt, selected, requested, retained, 0,
    );
}

pub proof fn checked_map_push_scoped_identity_selects_indexed_action(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    live: LiveRuntimeProductState,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: backing::ExactIndexedViewObservation,
    requested: backing::ExactIndexedViewObservation,
    retained: backing::ExactIndexedViewObservation,
    observed: ScopedMapPushIdentityClasses,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        message_source::generated_message_live_frame_matches_fold(
            source, folded, live,
        ),
        live.source.product.bridge.admission.frames.last().region_start == 0,
        live.source.product.bridge.admission.frames.last().region_end == source.body_len,
        map_push::checked_map_push_request_is_fresh_child(live, receipt),
        backing::exact_indexed_view_observation_wf(selected),
        backing::exact_indexed_view_observation_wf(requested),
        backing::exact_indexed_view_observation_wf(retained),
        scoped_map_push_identity_classes_match_observation(
            selected, requested, retained, observed,
        ),
        observed.selected == observed.requested,
        observed.requested == observed.retained,
        selected.image == requested.image,
        requested.image == retained.image,
        admission::admission_indexed_source_subview(
            source.indexed, receipt.field.payload_start,
            receipt.field.payload_end,
        ) == Some(selected.image),
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
        &&& product_admission_source_image(child) == retained.image
        &&& map::integrated_source_message_action(
            live.source.product.map.admission, action,
        ) == Some(action)
    },
{
    checked_map_push_scoped_identity_selects_symbolic_views(
        source, folded, receipt, selected, requested, retained, observed,
    );
    map_push::checked_map_push_view_and_fresh_child_select_indexed_action(
        source, folded, live, receipt,
        scoped_map_push_symbolic_views(
            selected, requested, retained, observed,
        ),
    );
}

pub proof fn equal_image_distinct_backing_has_distinct_scoped_class()
    ensures {
        let image = admission::AdmissionIndexedSourceImage {
            bytes: seq![0x08u8],
            positions: seq![admission::AdmissionSourceAnchor {
                source: 7, offset: 19,
            }],
            end: admission::AdmissionSourceAnchor { source: 7, offset: 20 },
        };
        let selected = backing::ExactIndexedViewObservation {
            key: backing::ExactIndexedBackingKey::Borrowed {
                slice_data: 41, slice_len: 1, start: 0, len: 1,
                source: 7, absolute_start: 19,
                end_source: None, end_offset: 0,
            },
            image,
        };
        let other = backing::ExactIndexedViewObservation {
            key: backing::ExactIndexedBackingKey::Borrowed {
                slice_data: 42, slice_len: 1, start: 0, len: 1,
                source: 7, absolute_start: 19,
                end_source: None, end_offset: 0,
            },
            image,
        };
        let retained_selected = scoped_map_push_identity_classes(
            selected.key, other.key, selected.key,
        );
        let retained_requested = scoped_map_push_identity_classes(
            selected.key, other.key, other.key,
        );
        &&& backing::exact_indexed_view_observation_wf(selected)
        &&& backing::exact_indexed_view_observation_wf(other)
        &&& selected.image == other.image
        &&& !backing::exact_indexed_same_view(selected.key, other.key)
        &&& retained_selected.selected == 0
        &&& retained_selected.requested == 1
        &&& retained_selected.retained == 0
        &&& retained_requested.selected == 0
        &&& retained_requested.requested == 1
        &&& retained_requested.retained == 1
    },
{}

}
