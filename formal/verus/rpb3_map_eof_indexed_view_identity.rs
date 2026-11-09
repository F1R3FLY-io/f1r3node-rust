use vstd::prelude::*;

use super::*;

#[path = "rpb3_indexed_region_source_projection.rs"]
mod indexed_view;

verus! {

pub struct MapEofBackedReceiptModel {
    pub ghost view: indexed_view::IndexedRegionView,
    pub ghost receipt: map::MapEofReceiptModel,
}

pub open spec fn indexed_view_matches_admission_image(
    view: indexed_view::IndexedRegionView,
    image: admission::AdmissionIndexedSourceImage,
) -> bool {
    &&& indexed_view::view_wf(view)
    &&& admission::admission_indexed_source_wf(image)
    &&& indexed_view::project(view).bytes == image.bytes
    &&& indexed_view::project(view).positions.len() == image.positions.len()
    &&& forall|i: int| 0 <= i < image.positions.len() ==> {
        &&& indexed_view::project(view).positions[i].source
            == image.positions[i].source as int
        &&& indexed_view::project(view).positions[i].offset
            == image.positions[i].offset as int
    }
    &&& indexed_view::project(view).end.source == image.end.source as int
    &&& indexed_view::project(view).end.offset == image.end.offset as int
}

pub open spec fn backed_receipt_matches_image(
    backed: MapEofBackedReceiptModel,
) -> bool {
    match backed.receipt.source_image {
        None => false,
        Some(image) => backed.receipt.source_bytes == image.bytes
            && indexed_view_matches_admission_image(backed.view, image),
    }
}

pub open spec fn backed_receipts_match_source(
    before: MapEofBackedReceiptModel,
    produced: MapEofBackedReceiptModel,
) -> bool {
    &&& backed_receipt_matches_image(before)
    &&& backed_receipt_matches_image(produced)
    &&& indexed_view::model_same_view(before.view, produced.view)
    &&& map::map_eof_receipts_match(before.receipt, produced.receipt)
}

pub open spec fn eof_receipt_nonimage_fields_match(
    before: map::MapEofReceiptModel,
    produced: map::MapEofReceiptModel,
) -> bool {
    &&& before.owner_type_id == produced.owner_type_id
    &&& before.outer_field == produced.outer_field
    &&& before.value_type_id == produced.value_type_id
    &&& before.physical_start == produced.physical_start
    &&& before.physical_source == produced.physical_source
    &&& before.region_end == produced.region_end
    &&& before.cursor == produced.cursor
    &&& before.key_present == produced.key_present
    &&& before.key_position == produced.key_position
    &&& before.key_source == produced.key_source
    &&& before.saw_value == produced.saw_value
    &&& before.value_position == produced.value_position
    &&& before.value_source == produced.value_source
    &&& before.summary == produced.summary
}

pub proof fn symbolic_same_view_binds_eof_receipt_images(
    before: MapEofBackedReceiptModel,
    produced: MapEofBackedReceiptModel,
)
    requires
        backed_receipt_matches_image(before),
        backed_receipt_matches_image(produced),
        indexed_view::model_same_view(before.view, produced.view),
        eof_receipt_nonimage_fields_match(before.receipt, produced.receipt),
    ensures
        before.receipt.source_image == produced.receipt.source_image,
        before.receipt.source_bytes == produced.receipt.source_bytes,
        map::map_eof_receipts_match(before.receipt, produced.receipt),
        backed_receipts_match_source(before, produced),
{
    indexed_view::same_view_preserves_image(before.view, produced.view);
    let left = before.receipt.source_image.unwrap();
    let right = produced.receipt.source_image.unwrap();
    assert(left.bytes == right.bytes);
    assert(left.positions.len() == right.positions.len());
    assert forall|i: int| 0 <= i < left.positions.len()
        implies left.positions[i] == right.positions[i] by {
        assert(indexed_view::project(before.view).positions[i]
            == indexed_view::project(produced.view).positions[i]);
    };
    assert(left.positions =~= right.positions);
    assert(left.end == right.end);
    assert(left == right);
}

pub open spec fn indexed_view_matches_live_wire(
    view: indexed_view::IndexedRegionView,
    cursor: wire_source::WireRuntimeCursorImage,
) -> bool {
    match wire_source::wire_runtime_project_region(cursor.message) {
        None => false,
        Some(region) => {
            &&& indexed_view::view_wf(view)
            &&& indexed_view::project(view).bytes == region.bytes
            &&& indexed_view::project(view).positions.len() == region.anchors.len()
            &&& forall|i: int| 0 <= i < region.anchors.len() ==> {
                &&& indexed_view::project(view).positions[i].source
                    == region.anchors[i].source as int
                &&& indexed_view::project(view).positions[i].offset
                    == region.anchors[i].offset as int
            }
            &&& indexed_view::project(view).end.source == region.end.source as int
            &&& indexed_view::project(view).end.offset == region.end.offset as int
        },
    }
}

pub open spec fn live_map_eof_indexed_origin(
    before: LiveRuntimeProductState,
) -> bool {
    before.source.product.map.map_frames.len() > 0
        && match before.source.product.map.map_frames.last() {
            None => false,
            Some(frame) => frame.source_image == Some(product_admission_source_image(
                before.source.product.indexed_regions.last(),
            )),
        }
}

pub proof fn live_map_indexed_origin_binds_eof_receipt(
    before: LiveRuntimeProductState,
    receipt: map::MapEofReceiptModel,
)
    requires
        live_runtime_product_wf(before),
        before.cursors.len() > 1,
        rpb3_source_observation::map_eof_live_snapshot(before) == Some(receipt),
        live_map_eof_indexed_origin(before),
    ensures
        receipt.source_image == Some(product_admission_source_image(
            before.source.product.indexed_regions.last(),
        )),
        receipt.source_bytes == product_admission_source_image(
            before.source.product.indexed_regions.last(),
        ).bytes,
{
    let state = before.source.product;
    let frame = state.map.map_frames.last().unwrap();
    let cursor = before.cursors.last().unwrap();
    assert(map::map_eof_snapshot(frame, cursor.terminal_error.is_none())
        == Some(receipt));
    assert(receipt.source_image == frame.source_image);
    assert(receipt.source_bytes == frame.source_bytes);
    assert(state.map.map_frames.len() == state.bridge.admission.frames.len());
    assert(state.map.map_frames[state.map.map_frames.len() - 1] == Some(frame));
    assert(map::map_aux_projection(frame, state.bridge.admission.frames.last()));
    assert(frame.source_bytes == state.bridge.admission.frames.last().source_bytes);
    assert(state.indexed_regions.len() == state.bridge.admission.frames.len());
    assert(state.indexed_regions.last().bytes
        == state.bridge.admission.frames.last().source_bytes);
}

pub proof fn live_wire_indexed_view_binds_eof_receipt_image(
    before: LiveRuntimeProductState,
    backed: MapEofBackedReceiptModel,
)
    requires
        live_runtime_product_wf(before),
        before.cursors.len() > 1,
        before.cursors.last().is_some(),
        rpb3_source_observation::map_eof_live_snapshot(before)
            == Some(backed.receipt),
        live_map_eof_indexed_origin(before),
        indexed_view_matches_live_wire(backed.view, before.cursors.last().unwrap()),
    ensures backed_receipt_matches_image(backed),
{
    let cursor = before.cursors.last().unwrap();
    let indexed = before.source.product.indexed_regions.last();
    let admission_image = product_admission_source_image(indexed);
    live_map_indexed_origin_binds_eof_receipt(before, backed.receipt);
    assert(before.cursors[before.cursors.len() - 1] == Some(cursor));
    assert(product_wire_indexed_source_agrees(cursor, indexed));
    let region = wire_source::wire_runtime_project_region(cursor.message).unwrap();
    assert(indexed_view::project(backed.view).bytes == admission_image.bytes);
    assert(indexed_view::project(backed.view).positions.len()
        == admission_image.positions.len());
    assert forall|i: int| 0 <= i < admission_image.positions.len() implies {
        &&& indexed_view::project(backed.view).positions[i].source
            == admission_image.positions[i].source as int
        &&& indexed_view::project(backed.view).positions[i].offset
            == admission_image.positions[i].offset as int
    } by {
        assert(region.anchors[i].source as usize == indexed.positions[i].source);
        assert(region.anchors[i].offset == indexed.positions[i].offset);
    };
    assert(indexed_view::project(backed.view).end.source
        == admission_image.end.source as int);
    assert(indexed_view::project(backed.view).end.offset
        == admission_image.end.offset as int);
}

pub proof fn distinct_borrowed_handles_forge_equal_indexed_image(
    first_handle: usize,
    second_handle: usize,
    bytes: Seq<u8>,
    source: u32,
    base: usize,
    end: indexed_view::SourceAnchor,
    receipt: map::MapEofReceiptModel,
)
    requires
        first_handle != second_handle,
        base as int + bytes.len() <= usize::MAX as int,
        backed_receipt_matches_image(MapEofBackedReceiptModel {
            view: indexed_view::root_view(indexed_view::borrowed_backing(
                first_handle, bytes, source, base, end,
            )),
            receipt,
        }),
    ensures {
        let first = indexed_view::root_view(indexed_view::borrowed_backing(
            first_handle, bytes, source, base, end,
        ));
        let second = indexed_view::root_view(indexed_view::borrowed_backing(
            second_handle, bytes, source, base, end,
        ));
        let original = MapEofBackedReceiptModel { view: first, receipt };
        let forged = MapEofBackedReceiptModel { view: second, receipt };
        &&& indexed_view::project(first) == indexed_view::project(second)
        &&& backed_receipt_matches_image(forged)
        &&& map::map_eof_receipts_match(original.receipt, forged.receipt)
        &&& !indexed_view::model_same_view(first, second)
        &&& !backed_receipts_match_source(original, forged)
    },
{
    let first = indexed_view::root_view(indexed_view::borrowed_backing(
        first_handle, bytes, source, base, end,
    ));
    let second = indexed_view::root_view(indexed_view::borrowed_backing(
        second_handle, bytes, source, base, end,
    ));
    indexed_view::same_view_preserves_image(first, first);
    assert(indexed_view::project(first) == indexed_view::project(second));
    assert(indexed_view::model_same_view(first, second) == false);
}

pub proof fn indexed_map_push_establishes_eof_source_origin(
    before: ProductScoreState,
    after: ProductScoreState,
    field: admission::IndexedCanonicalFieldModel,
    child: admission::AdmissionFrameState,
    parent_source: admission::AdmissionIndexedSourceImage,
    frame_bytes: usize,
)
    requires
        product_score_wf(before),
        before.bridge.admission.frames.len() > 0,
        admission::indexed_field_at_cursor(
            admission::IndexedCanonicalFieldCursorModel {
                region_start: before.bridge.admission.frames.last().region_start,
                region_end: before.bridge.admission.frames.last().region_end,
                next: before.bridge.admission.frames.last().cursor,
                source_bytes: before.bridge.admission.frames.last().source_bytes,
            }, field,
        ),
        product_map_step(
            before, after,
            map::IntegratedAdmissionAction::Message {
                action: admission::ConcreteFrameActionModel::PushIndexed {
                    field, child, parent_source, frame_bytes,
                },
            },
        ),
        after.bridge.admission.frames.len() > 0,
        after.bridge.admission.frames.last().frame_kind == 2,
    ensures
        after.map.map_frames.last().is_some(),
        after.map.map_frames.last().unwrap().source_image
            == Some(product_admission_source_image(after.indexed_regions.last())),
{
    let parent = before.bridge.admission.frames.last();
    let indexed = before.indexed_regions.last();
    assert(parent.source_bytes == indexed.bytes);
    assert(parent_source == product_admission_source_image(indexed));
    product_indexed_child_source_anchor_exact(parent, indexed, field);
    let image = product_indexed_child(indexed, field).unwrap();
    assert(after.indexed_regions.last() == image);
    let action = map::IntegratedAdmissionAction::Message {
        action: admission::ConcreteFrameActionModel::PushIndexed {
            field, child, parent_source, frame_bytes,
        },
    };
    assert(map::integrated_admission_driver_step(before.map, action)
        == Some(map::IntegratedAdmissionOutcome::MessagePushed { state: after.map }));
    assert(after.map.map_frames.last().is_some());
}

pub open spec fn product_map_indexed_origin_at(
    state: ProductScoreState,
    depth: int,
) -> bool {
    &&& 0 <= depth < state.map.map_frames.len()
    &&& depth < state.indexed_regions.len()
    &&& match state.map.map_frames[depth] {
        None => false,
        Some(frame) => frame.source_image == Some(product_admission_source_image(
            state.indexed_regions[depth],
        )),
    }
}

pub proof fn product_map_step_preserves_indexed_origin_at(
    before: ProductScoreState,
    after: ProductScoreState,
    action: map::IntegratedAdmissionAction,
    depth: int,
)
    requires
        product_score_wf(before),
        product_map_indexed_origin_at(before, depth),
        product_map_step(before, after, action),
        depth < after.map.map_frames.len(),
        depth < after.indexed_regions.len(),
    ensures product_map_indexed_origin_at(after, depth),
{
    let outcome = map::integrated_admission_driver_step(before.map, action).unwrap();
    let next = map::integrated_nonterminal_state(outcome).unwrap();
    let regions = product_map_regions(before, action, outcome).unwrap();
    assert(after.map == next);
    assert(after.indexed_regions == regions);
    let original = before.map.map_frames[depth].unwrap();
    assert(original.source_image == Some(product_admission_source_image(
        before.indexed_regions[depth],
    )));
    match action {
        map::IntegratedAdmissionAction::Message { .. } => {
            assert(after.map.map_frames[depth] == Some(original));
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
        map::IntegratedAdmissionAction::MapKey { .. } => {
            if depth == before.map.map_frames.len() - 1 {
                assert(after.map.map_frames[depth].is_some());
                assert(after.map.map_frames[depth].unwrap().source_image
                    == original.source_image);
            } else {
                assert(after.map.map_frames[depth] == Some(original));
            }
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
        map::IntegratedAdmissionAction::MapValue { .. }
        | map::IntegratedAdmissionAction::MapValueIndexed { .. } => {
            if depth == before.map.map_frames.len() - 1 {
                assert(after.map.map_frames[depth].is_some());
                assert(after.map.map_frames[depth].unwrap().source_image
                    == original.source_image);
            } else {
                assert(after.map.map_frames[depth] == Some(original));
            }
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
        map::IntegratedAdmissionAction::MapParScore => {
            assert(after.map.map_frames[depth] == Some(original));
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
        map::IntegratedAdmissionAction::MapAccept => {
            if depth == before.map.map_frames.len() - 2 {
                assert(after.map.map_frames[depth].is_some());
                assert(after.map.map_frames[depth].unwrap().source_image
                    == original.source_image);
            } else {
                assert(after.map.map_frames[depth] == Some(original));
            }
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
        map::IntegratedAdmissionAction::MapFinish => {
            assert(after.map.map_frames[depth] == Some(original));
            assert(after.indexed_regions[depth] == before.indexed_regions[depth]);
        },
    }
}

pub proof fn valid_plain_map_eof_has_no_indexed_source_image()
    ensures {
        let frame = map::map_initial(Seq::<u8>::empty(), 0);
        let image = admission::AdmissionIndexedSourceImage {
            bytes: Seq::empty(),
            positions: Seq::empty(),
            end: admission::AdmissionSourceAnchor { source: 0, offset: 0 },
        };
        let receipt = map::map_eof_snapshot(frame, true).unwrap();
        &&& map::map_frame_wf(frame)
        &&& map::map_finish(frame).is_some()
        &&& map::map_eof_snapshot(frame, true).is_some()
        &&& admission::admission_indexed_source_wf(image)
        &&& receipt.source_bytes == image.bytes
        &&& receipt.source_image.is_none()
        &&& receipt.source_image != Some(image)
    },
{
    map::map_initial_wf(Seq::<u8>::empty(), 0);
    let frame = map::map_initial(Seq::<u8>::empty(), 0);
    assert(map::map_finish(frame).is_some());
    assert(map::map_eof_snapshot(frame, true).is_some());
}

}
