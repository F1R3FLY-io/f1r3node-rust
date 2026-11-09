use vstd::prelude::*;

use super::*;
use super::rpb3_generic_message_eof_source_refinement as generic_message;
use super::rpb3_map_push_receipt_source_extraction as map_push;
use super::rpb3_message_driver_source_integration as message_source;

verus! {

pub enum ArenaRootKey {
    Empty,
    Span { node: usize, start: usize, len: usize },
}

pub enum ExactIndexedBackingKey {
    Borrowed {
        slice_data: usize,
        slice_len: usize,
        start: usize,
        len: usize,
        source: u32,
        absolute_start: usize,
        end_source: Option<u32>,
        end_offset: usize,
    },
    TestContiguous {
        bytes_data: usize,
        bytes_len: usize,
        source: u32,
        absolute_start: usize,
        end: admission::AdmissionSourceAnchor,
    },
    TestArena {
        arena_data: usize,
        root: ArenaRootKey,
        start: usize,
        len: usize,
        root_end: admission::AdmissionSourceAnchor,
    },
    Frozen {
        data_handle: usize,
        root_start: usize,
        root_len: usize,
        start: usize,
        len: usize,
        root_end: admission::AdmissionSourceAnchor,
    },
    Composed {
        root_node: Option<usize>,
        root_start: usize,
        root_len: usize,
        start: usize,
        len: usize,
        root_end: admission::AdmissionSourceAnchor,
    },
}

pub struct ExactIndexedViewObservation {
    pub key: ExactIndexedBackingKey,
    pub ghost image: admission::AdmissionIndexedSourceImage,
}

pub open spec fn exact_indexed_view_len(key: ExactIndexedBackingKey) -> usize {
    match key {
        ExactIndexedBackingKey::Borrowed { len, .. } => len,
        ExactIndexedBackingKey::TestContiguous { bytes_len, .. } => bytes_len,
        ExactIndexedBackingKey::TestArena { len, .. } => len,
        ExactIndexedBackingKey::Frozen { len, .. } => len,
        ExactIndexedBackingKey::Composed { len, .. } => len,
    }
}

pub open spec fn exact_indexed_view_observation_wf(
    observed: ExactIndexedViewObservation,
) -> bool {
    admission::admission_indexed_source_wf(observed.image)
        && observed.image.bytes.len() == exact_indexed_view_len(observed.key) as int
}

pub open spec fn exact_indexed_same_view(
    left: ExactIndexedBackingKey,
    right: ExactIndexedBackingKey,
) -> bool {
    match (left, right) {
        (
            ExactIndexedBackingKey::Borrowed {
                slice_data: a_data, slice_len: a_slice_len, start: a_start,
                len: a_len, source: a_source, absolute_start: a_base,
                end_source: a_end_source, end_offset: a_end_offset,
            },
            ExactIndexedBackingKey::Borrowed {
                slice_data: b_data, slice_len: b_slice_len, start: b_start,
                len: b_len, source: b_source, absolute_start: b_base,
                end_source: b_end_source, end_offset: b_end_offset,
            },
        ) => a_data == b_data && a_slice_len == b_slice_len
            && a_start == b_start && a_len == b_len && a_source == b_source
            && a_base == b_base && a_end_source == b_end_source
            && a_end_offset == b_end_offset,
        (
            ExactIndexedBackingKey::TestContiguous {
                bytes_data: a_data, bytes_len: a_len, source: a_source,
                absolute_start: a_base, end: a_end,
            },
            ExactIndexedBackingKey::TestContiguous {
                bytes_data: b_data, bytes_len: b_len, source: b_source,
                absolute_start: b_base, end: b_end,
            },
        ) => a_data == b_data && a_len == b_len && a_source == b_source
            && a_base == b_base && a_end == b_end,
        (
            ExactIndexedBackingKey::TestArena {
                arena_data: a_arena, root: a_root, start: a_start,
                len: a_len, root_end: a_end,
            },
            ExactIndexedBackingKey::TestArena {
                arena_data: b_arena, root: b_root, start: b_start,
                len: b_len, root_end: b_end,
            },
        ) => a_arena == b_arena && a_root == b_root
            && a_start == b_start && a_len == b_len && a_end == b_end,
        (
            ExactIndexedBackingKey::Frozen {
                data_handle: a_data, root_start: a_root_start,
                root_len: a_root_len, start: a_start, len: a_len,
                root_end: a_end,
            },
            ExactIndexedBackingKey::Frozen {
                data_handle: b_data, root_start: b_root_start,
                root_len: b_root_len, start: b_start, len: b_len,
                root_end: b_end,
            },
        ) => a_data == b_data && a_root_start == b_root_start
            && a_root_len == b_root_len && a_start == b_start
            && a_len == b_len && a_end == b_end,
        (
            ExactIndexedBackingKey::Composed {
                root_node: a_root, root_start: a_root_start,
                root_len: a_root_len, start: a_start, len: a_len,
                root_end: a_end,
            },
            ExactIndexedBackingKey::Composed {
                root_node: b_root, root_start: b_root_start,
                root_len: b_root_len, start: b_start, len: b_len,
                root_end: b_end,
            },
        ) => a_root == b_root && a_root_start == b_root_start
            && a_root_len == b_root_len && a_start == b_start
            && a_len == b_len && a_end == b_end,
        _ => false,
    }
}

pub proof fn exact_indexed_same_view_is_key_equality(
    left: ExactIndexedBackingKey,
    right: ExactIndexedBackingKey,
)
    ensures exact_indexed_same_view(left, right) == (left == right),
{
    match (left, right) {
        (ExactIndexedBackingKey::Borrowed { .. },
         ExactIndexedBackingKey::Borrowed { .. }) => {},
        (ExactIndexedBackingKey::TestContiguous { .. },
         ExactIndexedBackingKey::TestContiguous { .. }) => {},
        (ExactIndexedBackingKey::TestArena { .. },
         ExactIndexedBackingKey::TestArena { .. }) => {},
        (ExactIndexedBackingKey::Frozen { .. },
         ExactIndexedBackingKey::Frozen { .. }) => {},
        (ExactIndexedBackingKey::Composed { .. },
         ExactIndexedBackingKey::Composed { .. }) => {},
        _ => {},
    }
}

pub open spec fn local_map_push_symbolic_views(
    selected: ExactIndexedViewObservation,
    requested: ExactIndexedViewObservation,
    retained: ExactIndexedViewObservation,
    token: usize,
) -> map_push::CheckedMapPushViewTriple {
    map_push::CheckedMapPushViewTriple {
        selected: runtime_source::SourceSelectedFrameView {
            identity: token, indexed: selected.image,
        },
        requested: runtime_source::SourceSelectedFrameView {
            identity: token, indexed: requested.image,
        },
        retained: runtime_source::SourceSelectedFrameView {
            identity: token, indexed: retained.image,
        },
    }
}

pub proof fn checked_map_push_exact_views_project_symbolic_identity(
    source: message_source::GeneratedMessageFieldSource,
    folded: generic_message::GenericMessageSourceFold,
    receipt: message_source::GeneratedMessagePushReceipt,
    selected: ExactIndexedViewObservation,
    requested: ExactIndexedViewObservation,
    retained: ExactIndexedViewObservation,
    token: usize,
)
    requires
        map_push::checked_map_push_source_receipt(source, folded, receipt),
        exact_indexed_view_observation_wf(selected),
        exact_indexed_view_observation_wf(requested),
        exact_indexed_view_observation_wf(retained),
        exact_indexed_same_view(selected.key, requested.key),
        exact_indexed_same_view(requested.key, retained.key),
        selected.image == requested.image,
        requested.image == retained.image,
        admission::admission_indexed_source_subview(
            source.indexed, receipt.field.payload_start,
            receipt.field.payload_end,
        ) == Some(selected.image),
    ensures
        exact_indexed_same_view(selected.key, retained.key),
        map_push::checked_map_push_view_triple_matches(
            source, receipt,
            local_map_push_symbolic_views(selected, requested, retained, token),
        ),
{
    exact_indexed_same_view_is_key_equality(selected.key, requested.key);
    exact_indexed_same_view_is_key_equality(requested.key, retained.key);
    exact_indexed_same_view_is_key_equality(selected.key, retained.key);
    assert(selected.key == retained.key);
    map_push::checked_map_push_receipt_projects_body_and_anchors(
        source, folded, receipt,
    );
}

pub proof fn equal_image_distinct_borrowed_handles_fail_same_view()
    ensures {
        let image = admission::AdmissionIndexedSourceImage {
            bytes: seq![0x08u8],
            positions: seq![admission::AdmissionSourceAnchor {
                source: 7, offset: 19,
            }],
            end: admission::AdmissionSourceAnchor { source: 7, offset: 20 },
        };
        let selected = ExactIndexedViewObservation {
            key: ExactIndexedBackingKey::Borrowed {
                slice_data: 41, slice_len: 1, start: 0, len: 1,
                source: 7, absolute_start: 19,
                end_source: None, end_offset: 0,
            },
            image,
        };
        let retained = ExactIndexedViewObservation {
            key: ExactIndexedBackingKey::Borrowed {
                slice_data: 42, slice_len: 1, start: 0, len: 1,
                source: 7, absolute_start: 19,
                end_source: None, end_offset: 0,
            },
            image,
        };
        &&& exact_indexed_view_observation_wf(selected)
        &&& exact_indexed_view_observation_wf(retained)
        &&& selected.image == retained.image
        &&& !exact_indexed_same_view(selected.key, retained.key)
    },
{}

pub proof fn matching_root_with_changed_end_fails_same_view()
    ensures {
        let end = admission::AdmissionSourceAnchor { source: 7, offset: 20 };
        let changed = admission::AdmissionSourceAnchor { source: 7, offset: 21 };
        let frozen = ExactIndexedBackingKey::Frozen {
            data_handle: 41, root_start: 0, root_len: 1,
            start: 0, len: 1, root_end: end,
        };
        let frozen_changed = ExactIndexedBackingKey::Frozen {
            data_handle: 41, root_start: 0, root_len: 1,
            start: 0, len: 1, root_end: changed,
        };
        let composed = ExactIndexedBackingKey::Composed {
            root_node: Some(41), root_start: 0, root_len: 1,
            start: 0, len: 1, root_end: end,
        };
        let composed_changed = ExactIndexedBackingKey::Composed {
            root_node: Some(41), root_start: 0, root_len: 1,
            start: 0, len: 1, root_end: changed,
        };
        &&& !exact_indexed_same_view(frozen, frozen_changed)
        &&& !exact_indexed_same_view(composed, composed_changed)
    },
{}

pub proof fn canonical_empty_composed_roots_have_same_view()
    ensures {
        let end = admission::AdmissionSourceAnchor { source: 7, offset: 20 };
        let empty = ExactIndexedBackingKey::Composed {
            root_node: None, root_start: 0, root_len: 0,
            start: 0, len: 0, root_end: end,
        };
        let changed = ExactIndexedBackingKey::Composed {
            root_node: None, root_start: 0, root_len: 0,
            start: 0, len: 0,
            root_end: admission::AdmissionSourceAnchor { source: 7, offset: 21 },
        };
        &&& exact_indexed_same_view(empty, empty)
        &&& !exact_indexed_same_view(empty, changed)
    },
{}

}
