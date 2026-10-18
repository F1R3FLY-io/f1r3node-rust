use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{EList, EPathMap, Expr};

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn append_delimited(out: &mut Vec<u8>, tag: u32, payload: &[u8]) {
    prost::encoding::encode_key(tag, WireType::LengthDelimited, out);
    prost::encoding::encode_varint(payload.len() as u64, out);
    out.extend_from_slice(payload);
}

fn oracle(payload: &[u8]) -> Result<Send, prost::DecodeError> {
    crate::rust::rholang::protobuf_decoder::decode_send_deferred_epathmap_validation(payload)
}

#[test]
fn projection_preserves_presence_order_unknowns_and_metadata() {
    let value = Send {
        chan: Some(Par::default()),
        data: vec![gint(3), Par::default(), gint(5)],
        persistent: true,
        locally_free: vec![1, 4, 9],
        connective_used: true,
        cost_region: None,
    };
    let mut canonical = protobuf_encoder::encode_to_vec(&value);
    prost::encoding::encode_key(8, WireType::Varint, &mut canonical);
    prost::encoding::encode_varint(17, &mut canonical);

    let projected = decode_resident_send_projection(&canonical)
        .expect("the generated Send schema admits direct Par cuts")
        .expect("the projected Send decodes");
    let expected = oracle(&canonical).expect("the complete generated Send decoder accepts it");
    assert_eq!(projected, expected);
    assert_eq!(projected.chan, Some(Par::default()));
    assert_eq!(projected.data, value.data);
    assert!(projected.persistent);
    assert_eq!(projected.locally_free, vec![1, 4, 9]);
    assert!(projected.connective_used);

    let absent = protobuf_encoder::encode_to_vec(&Send {
        data: vec![gint(8)],
        ..Send::default()
    });
    let projected = decode_resident_send_projection(&absent)
        .expect("a missing optional channel is still projectable")
        .expect("the projected channel-free Send decodes");
    assert_eq!(projected, oracle(&absent).expect("the oracle decodes"));
    assert!(projected.chan.is_none());
}

#[test]
fn noncanonical_field_order_and_duplicate_singular_fields_use_the_complete_decoder() {
    let (chan_tag, data_tag) = resident_send_par_field_tags().expect("generated Send Par fields");
    let first = protobuf_encoder::encode_to_vec(&gint(1));
    let second = protobuf_encoder::encode_to_vec(&gint(2));

    let mut decreasing = Vec::new();
    append_delimited(&mut decreasing, data_tag, &second);
    append_delimited(&mut decreasing, chan_tag, &first);
    assert!(decode_resident_send_projection(&decreasing).is_none());
    assert_eq!(
        decode_resident_send(&decreasing).expect("fallback decodes decreasing fields"),
        oracle(&decreasing).expect("oracle decodes decreasing fields")
    );

    let mut duplicate = Vec::new();
    append_delimited(&mut duplicate, chan_tag, &first);
    append_delimited(&mut duplicate, chan_tag, &second);
    assert!(decode_resident_send_projection(&duplicate).is_none());
    assert_eq!(
        decode_resident_send(&duplicate).expect("fallback decodes duplicate channels"),
        oracle(&duplicate).expect("oracle decodes duplicate channels")
    );
}

#[test]
fn projected_children_retain_deferred_epathmap_validation() {
    let map = EPathMap::new_map([(gint(1), gint(2))], vec![2], true, None);
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EPathmapBody(map)),
    }]);
    let value = Send {
        chan: Some(gint(0)),
        data: vec![child],
        ..Send::default()
    };
    let canonical = protobuf_encoder::encode_to_vec(&value);
    let projected = decode_resident_send_projection(&canonical)
        .expect("the EPathMap child is retained as one Par cut")
        .expect("the valid ACT snapshot decodes");
    assert_eq!(projected, oracle(&canonical).expect("the oracle decodes"));

    let snapshot = canonical
        .windows(4)
        .position(|window| window == b"EPM1")
        .expect("the canonical Send contains the EPathMap snapshot");
    let mut malformed = canonical;
    malformed[snapshot] = b'X';
    let projected_error = decode_resident_send_projection(&malformed)
        .expect("the outer Send still projects its opaque malformed child")
        .expect_err("the projected child validates EPathMap framing");
    let oracle_error = oracle(&malformed).expect_err("the oracle validates EPathMap framing");
    assert_eq!(projected_error.to_string(), oracle_error.to_string());
}

#[test]
fn deep_unary_data_decodes_through_the_resident_endpoint_on_a_small_stack() {
    const DEPTH: usize = 4_096;

    std::thread::Builder::new()
        .name("resident-send-projection-depth".to_string())
        .stack_size(256 * 1024)
        .spawn(|| {
            let nested = Par::wrap_unary_elist(DEPTH, gint(7))
                .expect("the unary-list depth is representable");
            let original = Send {
                chan: Some(gint(0)),
                data: vec![nested],
                persistent: true,
                locally_free: vec![1, 2, 3],
                connective_used: true,
                cost_region: None,
            };
            let resident = Par::default().with_sends(vec![original.clone()]);
            let mut endpoint = None;
            resident.visit_encoded::<Send>(|entry| {
                assert!(endpoint.is_none());
                endpoint = Some(entry.to_owned_canonical());
            });
            let endpoint = endpoint.expect("the resident ParBag contains one Send endpoint");
            let plan = cut_at_par(
                ProtobufRegion::copy_from_slice(endpoint.payload()),
                &SEND_RHO_MATCH_PROGRAM,
            )
            .expect("the authoritative Send endpoint cuts");
            let (_, data_tag) = resident_send_par_field_tags().expect("generated Send Par fields");
            let data_cut = (0..plan.cuts().len())
                .find(|&index| {
                    plan.cut_step_from_leaf(index, 0)
                        .is_some_and(|step| step.field_tag == data_tag)
                })
                .expect("the authoritative Send endpoint contains its data cut");
            assert!(
                import_canonical_parbag_fast_path(plan.cuts()[data_cut].region().as_bytes())
                    .is_some()
            );
            assert!(
                decode_resident_send_projection(endpoint.payload()).is_some(),
                "the depth gate must exercise the projection rather than its fallback"
            );
            let decoded = endpoint
                .decode()
                .expect("the resident Send endpoint decodes");
            assert_eq!(decoded, original);
            assert_eq!(
                decoded.data[0].canonical_unary_elist_gint_parts(),
                Some((DEPTH, 7))
            );
        })
        .expect("spawn small-stack resident Send worker")
        .join()
        .expect("resident Send worker completes");
}

#[test]
fn non_fast_child_shapes_remain_differentially_equivalent() {
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![gint(1), gint(2)],
            locally_free: vec![3],
            connective_used: true,
            ..EList::default()
        })),
    }]);
    let value = Send {
        chan: Some(child.clone()),
        data: vec![child, gint(9)],
        ..Send::default()
    };
    let canonical = protobuf_encoder::encode_to_vec(&value);
    let projected = decode_resident_send_projection(&canonical)
        .expect("the outer Send shape remains projectable")
        .expect("the generated child decoder accepts the general shape");
    assert_eq!(projected, oracle(&canonical).expect("the oracle decodes"));
}
