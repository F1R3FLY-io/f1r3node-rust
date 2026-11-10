//! Generated negative controls for the live protobuf oneof seen bit.
//!
//! The wire tags and shared slot come from the generated descriptor, while
//! admission and its field trace come from the production RPB3 driver. Each
//! arm is independently valid; only the second occurrence makes a pair bad.

use proptest::prelude::*;

use super::*;
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::Expr;
use crate::rust::rholang::protobuf_encoder;

fn scalar_expr(kind: u8, integer: i16, boolean: bool, text: &str, bytes: &[u8]) -> Expr {
    let instance = match kind {
        0 => ExprInstance::GBool(boolean),
        1 => ExprInstance::GInt(i64::from(integer)),
        2 => ExprInstance::GString(text.to_owned()),
        3 => ExprInstance::GByteArray(bytes.to_vec()),
        4 => ExprInstance::GDouble(integer as u64),
        _ => unreachable!("generated arm selector has five values"),
    };
    Expr {
        expr_instance: Some(instance),
    }
}

fn wire_tag(bytes: &[u8]) -> u32 {
    let mut key = 0u64;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        key |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            assert_ne!(key >> 3, 0);
            return u32::try_from(key >> 3).expect("generated field number fits u32");
        }
    }
    panic!("generated Expr field has no terminating key byte");
}

fn oneof_slot_and_arm(tag: u32) -> (usize, &'static str) {
    EXPR_RHO_MATCH_PROGRAM
        .fields
        .iter()
        .enumerate()
        .find_map(|(slot, field)| match field.kind {
            RhoMatchFieldKind::Oneof(arms) => arms
                .iter()
                .find(|arm| arm.tag == tag)
                .map(|arm| (slot, arm.name)),
            _ => None,
        })
        .expect("generated scalar Expr tag belongs to a oneof slot")
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(40))]

    #[test]
    fn generated_oneof_pair_rejects_second_arm_after_exactly_one_seen_transition(
        first_kind in 0u8..5,
        second_kind in 0u8..5,
        first_integer in any::<i16>(),
        second_integer in any::<i16>(),
        first_boolean in any::<bool>(),
        second_boolean in any::<bool>(),
        first_text in prop::collection::vec(33u8..127, 1..9),
        second_text in prop::collection::vec(33u8..127, 1..9),
        first_bytes in prop::collection::vec(any::<u8>(), 1..9),
        second_bytes in prop::collection::vec(any::<u8>(), 1..9),
    ) {
        let first_text = String::from_utf8(first_text).unwrap();
        let second_text = String::from_utf8(second_text).unwrap();
        let first = protobuf_encoder::encode_to_vec(&scalar_expr(
            first_kind, first_integer, first_boolean, &first_text, &first_bytes,
        ));
        let second = protobuf_encoder::encode_to_vec(&scalar_expr(
            second_kind, second_integer, second_boolean, &second_text, &second_bytes,
        ));
        let first_tag = wire_tag(&first);
        let second_tag = wire_tag(&second);
        let (first_slot, first_arm) = oneof_slot_and_arm(first_tag);
        let (second_slot, _) = oneof_slot_and_arm(second_tag);
        prop_assert_eq!(first_slot, second_slot);

        // These controls distinguish the duplicate-slot rule from a malformed
        // scalar payload, an omitted oneof default, or an incomplete Expr.
        for arm in [&first, &second] {
            let accepted = validate_canonical_protobuf(
                ProtobufRegion::copy_from_slice(arm), &EXPR_RHO_MATCH_PROGRAM,
            );
            prop_assert!(accepted.is_ok(), "single generated arm rejected: {accepted:?}");
        }

        let mut pair = first.clone();
        pair.extend_from_slice(&second);
        rpb3_root_finish_trace::start();
        let rejected = validate_canonical_protobuf(
            ProtobufRegion::copy_from_slice(&pair), &EXPR_RHO_MATCH_PROGRAM,
        );
        let trace = rpb3_root_finish_trace::take();
        let duplicate_rejected = matches!(
            &rejected,
            Err(CanonicalProtobufAdmissionError::Schema {
                field,
                detail: "canonical singular or oneof field is duplicated",
                ..
            }) if *field == second_tag
        );
        prop_assert!(duplicate_rejected, "unexpected duplicate verdict: {rejected:?}");
        prop_assert!(trace.returned.is_none());
        prop_assert_eq!(trace.message_fields.len(), 1);
        prop_assert_eq!(trace.driver_steps.len(), 1);

        let accepted_first = &trace.message_fields[0];
        prop_assert_eq!(accepted_first.tag, first_tag);
        prop_assert_eq!(accepted_first.slot, first_slot);
        prop_assert_eq!(accepted_first.encoded_range.start, 0);
        prop_assert_eq!(accepted_first.encoded_range.end, first.len());
        prop_assert_eq!(accepted_first.frame_after.last_slot, Some(first_slot));
        prop_assert_eq!(accepted_first.frame_after.seen_slots.as_slice(), &[first_slot]);
        prop_assert_eq!(accepted_first.frame_after.semantic_slots[first_slot].len(), 1);
        prop_assert_eq!(
            accepted_first.frame_after.semantic_slots[first_slot][0].variant,
            Some(first_arm),
        );
        prop_assert_eq!(accepted_first.frame_after.cursor_fields_read, 1);
        prop_assert_eq!(accepted_first.frame_after.cursor_bytes_read, first.len());
        prop_assert!(matches!(
            trace.driver_steps[0].kind,
            rpb3_root_finish_trace::DriverStepKind::Continue,
        ));
    }
}
