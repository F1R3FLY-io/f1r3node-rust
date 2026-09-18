use super::*;
use crate::rust::rholang::protobuf_cursor::ProtobufRegion;

#[test]
fn incomplete_import_shares_body_and_preserves_canonical_image() {
    let child = Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(7)),
    }]);
    let input = Send {
        data: vec![Par::default(), child],
        locally_free: vec![5],
        connective_used: true,
        ..Send::default()
    };
    let shallow = Send {
        data: vec![Par::default(), Par::default()],
        ..input.clone()
    };
    let source = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&input));
    let pointer = source.as_bytes().as_ptr();
    let mut arena = ParConstruction::new();
    let prior = arena
        .import_par(&Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GInt(42)),
        }]))
        .unwrap();
    let prior_bytes = arena.finish(prior).unwrap();
    let component = arena
        .import_admitted_incomplete_component_region(&shallow, source.clone())
        .unwrap()
        .unwrap();
    let node = &arena.protobuf_nodes[arena.components[component.0].protobuf];
    let [ProtoPiece::SharedBytes(bytes)] = node.pieces.as_slice() else {
        panic!("one shared body leaf")
    };
    assert_eq!(bytes.as_ptr(), pointer);
    assert_eq!(bytes.as_ref().as_ref(), source.as_bytes());
    assert!(std::mem::size_of::<ProtoPiece>() <= std::mem::size_of::<(usize, usize, usize)>());
    drop(source);
    let count = Multiplicity::new(3).unwrap();
    let planned = arena.par([(component, count)], vec![5], true).unwrap();
    let actual = arena.finish(planned).unwrap();
    let mut expected = Par::default();
    expected
        .insert_component_many(&input, count, vec![5], true)
        .unwrap();
    assert_eq!(
        actual.components().compact_bytes(),
        expected.components().compact_bytes()
    );
    assert_eq!(
        protobuf_encoder::encode_to_vec(&actual),
        protobuf_encoder::encode_to_vec(&expected)
    );
    assert_eq!(arena.finish(prior).unwrap(), prior_bytes);
}

#[test]
fn complete_shell_declines_opaque_import_without_arena_mutation() {
    let input = Send {
        chan: Some(Par::default()),
        ..Send::default()
    };
    let region = ProtobufRegion::from(protobuf_encoder::encode_to_vec(&input));
    let mut arena = ParConstruction::new();
    let before = (
        arena.components.len(),
        arena.protobuf_nodes.len(),
        arena.scores.len(),
    );
    assert!(arena
        .import_admitted_incomplete_component_region(&input, region)
        .unwrap()
        .is_none());
    assert_eq!(
        (
            arena.components.len(),
            arena.protobuf_nodes.len(),
            arena.scores.len()
        ),
        before
    );
}
