use proptest::prelude::*;
use prost::bytes::Buf;
use prost::encoding::{DecodeContext, WireType};
use prost::Message;

use super::*;
use crate::protobuf_decoder_oracle::legacy;

fn integer(value: i64) -> Expr {
    Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }
}

fn assert_observation(arena: &mut ParConstruction, state: &ProtobufParBuilder, reference: &Par) {
    let observed = ProtobufParBuilder {
        occurrence_root: state.occurrence_root,
        locally_free: state.locally_free.clone(),
        connective_used: state.connective_used,
    };
    let plan = arena.freeze_protobuf_par(observed).unwrap();
    let actual = arena.finish(plan).unwrap();
    assert_eq!(actual.encode_to_vec(), reference.encode_to_vec());
    assert_eq!(
        actual.components().compact_bytes(),
        reference.components().compact_bytes()
    );
    assert_eq!(actual.locally_free(), reference.locally_free());
    assert_eq!(actual.connective_used(), reference.connective_used());
}

fn scalar(
    state: &mut ProtobufParBuilder,
    reference: &mut Par,
    tag: u32,
    wire_type: WireType,
    bytes: &[u8],
    split: usize,
) -> bool {
    let mut original = bytes[..split].chain(&bytes[split..]);
    let mut planned = bytes[..split].chain(&bytes[split..]);
    let expected =
        legacy(|| reference.merge_field(tag, wire_type, &mut original, DecodeContext::default()));
    let actual = match tag {
        9 => state.merge_protobuf_locally_free(wire_type, &mut planned, DecodeContext::default()),
        10 => {
            state.merge_protobuf_connective_used(wire_type, &mut planned, DecodeContext::default())
        }
        _ => unreachable!(),
    };
    assert_eq!(actual, expected);
    assert_eq!(planned.remaining(), original.remaining());
    assert_eq!(state.locally_free.as_slice(), reference.locally_free());
    assert_eq!(state.connective_used, reference.connective_used());
    actual.is_ok()
}

fn append_integer(
    arena: &mut ParConstruction,
    state: &mut ProtobufParBuilder,
    reference: &mut Par,
    value: i64,
) {
    let expr = integer(value);
    let component = arena.import_component(&expr).unwrap();
    arena.insert_protobuf_component(state, component).unwrap();
    legacy(|| reference.merge_protobuf_component(&expr)).unwrap();
}

#[test]
fn all_component_families_preserve_explicit_root_metadata() {
    let mut arena = ParConstruction::new();
    let mut state = ProtobufParBuilder::default();
    let mut reference = Par::default();
    assert!(scalar(
        &mut state,
        &mut reference,
        9,
        WireType::LengthDelimited,
        &[2, 7, 9],
        1
    ));
    assert!(scalar(
        &mut state,
        &mut reference,
        10,
        WireType::Varint,
        &[1],
        0
    ));
    macro_rules! append {
        ($value:expr) => {{
            let value = $value;
            let component = arena.import_component(&value).unwrap();
            arena
                .insert_protobuf_component(&mut state, component)
                .unwrap();
            legacy(|| reference.merge_protobuf_component(&value)).unwrap();
            assert_observation(&mut arena, &state, &reference);
        }};
    }
    append!(Send {
        connective_used: true,
        ..Default::default()
    });
    append!(Receive {
        connective_used: true,
        ..Default::default()
    });
    append!(New::default());
    append!(integer(42));
    append!(Match::default());
    append!(GUnforgeable::default());
    append!(Connective::default());
    append!(Bundle::default());
    append!(If::default());
    assert!(scalar(
        &mut state,
        &mut reference,
        9,
        WireType::LengthDelimited,
        &[0],
        1
    ));
    assert!(scalar(
        &mut state,
        &mut reference,
        10,
        WireType::Varint,
        &[0],
        0
    ));
    append!(Send {
        connective_used: true,
        locally_free: vec![255],
        ..Default::default()
    });
    assert!(state.locally_free.is_empty());
    assert!(!state.connective_used);
}

#[test]
fn malformed_scalar_updates_keep_exact_state_error_and_consumption() {
    for tag in [9, 10] {
        for wire_type in [
            WireType::LengthDelimited,
            WireType::Varint,
            WireType::SixtyFourBit,
            WireType::ThirtyTwoBit,
            WireType::StartGroup,
            WireType::EndGroup,
        ] {
            for bytes in [vec![], vec![0], vec![1], vec![3, 1, 2], vec![255; 12]] {
                for split in 0..=bytes.len() {
                    let mut arena = ParConstruction::new();
                    let mut state = ProtobufParBuilder::default();
                    let mut reference = Par::default();
                    append_integer(&mut arena, &mut state, &mut reference, 9);
                    scalar(
                        &mut state,
                        &mut reference,
                        9,
                        WireType::LengthDelimited,
                        &[2, 4, 5],
                        1,
                    );
                    scalar(&mut state, &mut reference, 10, WireType::Varint, &[1], 0);
                    let old_root = state.occurrence_root;
                    scalar(&mut state, &mut reference, tag, wire_type, &bytes, split);
                    assert_eq!(state.occurrence_root, old_root);
                    assert_observation(&mut arena, &state, &reference);
                }
            }
        }
    }
}

#[test]
fn failed_occurrence_insert_preserves_builder_and_prior_roots() {
    let mut arena = ParConstruction::new();
    let mut state = ProtobufParBuilder::default();
    let mut reference = Par::default();
    append_integer(&mut arena, &mut state, &mut reference, 1);
    assert!(scalar(
        &mut state,
        &mut reference,
        9,
        WireType::LengthDelimited,
        &[2, 4, 5],
        1
    ));
    assert!(scalar(
        &mut state,
        &mut reference,
        10,
        WireType::Varint,
        &[1],
        0
    ));
    let root = state.occurrence_root;
    let prior = arena
        .par_from_occurrence_root(root, Vec::new(), false)
        .unwrap();
    let before = arena.protobuf_bytes(prior);
    for partial_path_copy in [false, true] {
        let invalid = arena.import_component(&integer(2)).unwrap();
        let proto = arena.components[invalid.0].protobuf;
        let tag = u32::from(arena.components[invalid.0].tag.byte());
        let prefix = key_len(tag) + encoded_len_varint(usize::MAX as u64);
        let body_len = if partial_path_copy {
            usize::MAX - prefix
        } else {
            usize::MAX
        };
        if partial_path_copy {
            assert_eq!(
                body_len.checked_add(key_len(tag) + encoded_len_varint(body_len as u64)),
                Some(usize::MAX)
            );
        }
        arena.protobuf_nodes[proto].encoded_len = body_len;
        let occurrence_nodes = arena.occurrence_nodes.len();
        assert!(matches!(
            arena.insert_protobuf_component(&mut state, invalid),
            Err(ParConstructionError::LengthOverflow(_))
        ));
        assert_eq!(
            arena.occurrence_nodes.len(),
            occurrence_nodes + usize::from(partial_path_copy)
        );
        assert_eq!(state.occurrence_root, root);
        assert_eq!(arena.protobuf_bytes(prior), before);
        assert_observation(&mut arena, &state, &reference);
    }
    append_integer(&mut arena, &mut state, &mut reference, 3);
    assert_eq!(arena.protobuf_bytes(prior), before);
    assert_observation(&mut arena, &state, &reference);
}

#[test]
fn move_reentry_and_insert_do_not_copy_metadata_or_finalize_pars() {
    let mut arena = ParConstruction::new();
    let mut state = ProtobufParBuilder::default();
    let mut bytes = Vec::new();
    prost::encoding::encode_varint(65536, &mut bytes);
    bytes.resize(bytes.len() + 65536, 7);
    state
        .merge_protobuf_locally_free(
            WireType::LengthDelimited,
            &mut bytes.as_slice(),
            DecodeContext::default(),
        )
        .unwrap();
    let pointer = state.locally_free.as_ptr();
    let capacity = state.locally_free.capacity();
    let component = arena.import_component(&integer(5)).unwrap();
    let score_nodes = arena.scores.len();
    let proto_nodes = arena.protobuf_nodes.len();
    let mut slot = Some(state);
    for _ in 0..4096 {
        let mut reopened = slot.take().unwrap();
        arena
            .insert_protobuf_component(&mut reopened, component)
            .unwrap();
        assert_eq!(reopened.locally_free.as_ptr(), pointer);
        assert_eq!(reopened.locally_free.capacity(), capacity);
        assert!(arena.pars.is_empty());
        assert_eq!(arena.scores.len(), score_nodes);
        assert_eq!(arena.protobuf_nodes.len(), proto_nodes);
        slot = Some(reopened);
    }
    let plan = arena.freeze_protobuf_par(slot.unwrap()).unwrap();
    assert_eq!(arena.pars.len(), 1);
    assert_eq!(arena.pars[plan.0].locally_free.as_ptr(), pointer);
    assert_eq!(arena.family_cardinality::<Expr>(plan), 4096);
}

fn mixed_depth(arena: &mut ParConstruction, depth: usize) -> PlannedPar {
    let mut child = arena
        .freeze_protobuf_par(ProtobufParBuilder::default())
        .unwrap();
    for level in 0..depth {
        let component = match level % 3 {
            0 => arena
                .expr_component(
                    &Expr {
                        expr_instance: Some(ExprInstance::ENotBody(crate::rhoapi::ENot {
                            p: Some(Par::default()),
                        })),
                    },
                    &[child],
                    false,
                )
                .unwrap(),
            1 => arena
                .send_component(
                    &Send {
                        chan: Some(Par::default()),
                        ..Default::default()
                    },
                    &[child],
                )
                .unwrap(),
            _ => arena
                .incomplete_component(
                    &If {
                        condition: Some(Par::default()),
                        ..Default::default()
                    },
                    &[child],
                )
                .unwrap()
                .unwrap(),
        };
        let mut state = ProtobufParBuilder::default();
        arena
            .insert_protobuf_component(&mut state, component)
            .unwrap();
        child = arena.freeze_protobuf_par(state).unwrap();
    }
    child
}

#[test]
fn mixed_nested_accumulation_is_stack_safe_and_finalizes_each_par_once() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for depth in [256, 1024, 4096, 8192] {
                let mut arena = ParConstruction::new();
                let plan = mixed_depth(&mut arena, depth);
                assert_eq!(arena.pars.len(), depth + 1);
                assert_eq!(arena.components.len(), depth);
                assert_eq!(arena.occurrence_nodes.len(), depth);
                assert!(arena.protobuf_nodes.len() <= 4 * depth + 1);
                let bytes = arena.protobuf_bytes(plan);
                assert_eq!(arena.finish(plan).unwrap().encode_to_vec(), bytes);
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

proptest! {
    #[test]
    fn ordered_field_events_match_frozen_committed_observations(
        events in prop::collection::vec((0u8..3, any::<i16>(), prop::collection::vec(any::<u8>(), 0..40)), 0..80)
    ) {
        let mut arena = ParConstruction::new();
        let mut state = ProtobufParBuilder::default();
        let mut reference = Par::default();
        for (kind, value, bytes) in events {
            match kind {
                0 => append_integer(&mut arena, &mut state, &mut reference, i64::from(value)),
                1 => {
                    let mut payload = Vec::new();
                    prost::encoding::encode_varint(bytes.len() as u64, &mut payload);
                    payload.extend(bytes);
                    assert!(scalar(&mut state, &mut reference, 9, WireType::LengthDelimited, &payload, payload.len() / 2));
                }
                _ => { assert!(scalar(&mut state, &mut reference, 10, WireType::Varint, &[u8::from(value > 0)], 0)); }
            }
            assert_observation(&mut arena, &state, &reference);
        }
    }
}

#[cfg(feature = "protobuf-projection-experiment")]
#[test]
fn accumulation_emits_no_resident_root_until_requested() {
    use substitution_cost::{capture, Metric};
    let mut arena = ParConstruction::new();
    let (root, planning) = capture(|| mixed_depth(&mut arena, 4096));
    for metric in [
        Metric::FinishAttempts,
        Metric::FinishSuccesses,
        Metric::ResidentBytes,
        Metric::RootKeys,
        Metric::RootKeyBytes,
    ] {
        assert_eq!(planning.get(metric), 0);
    }
    let (_, emitted) = capture(|| arena.finish(root).unwrap());
    assert_eq!(emitted.get(Metric::FinishAttempts), 1);
    assert_eq!(emitted.get(Metric::FinishSuccesses), 1);
    assert_eq!(emitted.get(Metric::RootKeys), 1);
}
