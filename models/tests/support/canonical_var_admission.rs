use super::{canonical_var, CanonicalVar};
use crate::rhoapi::var::{VarInstance, WildcardMsg};
use crate::rhoapi::Var;
use crate::rust::rholang::protobuf_encoder;

fn encoded(value: CanonicalVar) -> Vec<u8> {
    let var_instance = match value {
        CanonicalVar::Absent => None,
        CanonicalVar::Bound(value) => Some(VarInstance::BoundVar(value)),
        CanonicalVar::Free(value) => Some(VarInstance::FreeVar(value)),
        CanonicalVar::Wildcard => Some(VarInstance::Wildcard(WildcardMsg {})),
    };
    protobuf_encoder::encode_to_vec(&Var { var_instance })
}

proptest::proptest! {
    #![proptest_config(proptest::test_runner::Config {
        failure_persistence: Some(Box::new(
            proptest::test_runner::FileFailurePersistence::WithSource("regressions"),
        )),
        ..proptest::test_runner::Config::with_cases(1_024)
    })]

    #[test]
    fn canonical_var_proof_encoder_images_round_trip(value in proptest::prelude::any::<i32>()) {
        for expected in [
            CanonicalVar::Absent,
            CanonicalVar::Bound(value),
            CanonicalVar::Free(value),
            CanonicalVar::Wildcard,
        ] {
            let bytes = encoded(expected);
            proptest::prop_assert_eq!(canonical_var(&bytes), Some(expected));
            if matches!(expected, CanonicalVar::Bound(_) | CanonicalVar::Free(_)) {
                proptest::prop_assert!((2..=6).contains(&bytes.len()));
                proptest::prop_assert_ne!(bytes, encoded(CanonicalVar::Absent));
            }
        }
    }

    #[test]
    fn canonical_var_proof_accepted_bytes_have_the_exact_encoder_image(
        value in proptest::prelude::any::<i32>(),
        arbitrary in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..33),
    ) {
        let mut accepted = 0;
        for bytes in [
            arbitrary,
            encoded(CanonicalVar::Bound(value)),
            encoded(CanonicalVar::Free(value)),
            encoded(CanonicalVar::Absent),
            encoded(CanonicalVar::Wildcard),
        ] {
            if let Some(value) = canonical_var(&bytes) {
                accepted += 1;
                proptest::prop_assert_eq!(bytes, encoded(value));
            }
        }
        proptest::prop_assert!(accepted >= 4, "each case must exercise four admitted forms");
    }

    #[test]
    fn canonical_var_proof_mutations_refuse_noncanonical_scalar_images(
        value in proptest::prelude::any::<i32>(),
    ) {
        for expected in [CanonicalVar::Bound(value), CanonicalVar::Free(value)] {
            let bytes = encoded(expected);
            let mut padded_key = vec![bytes[0] | 0x80, 0];
            padded_key.extend_from_slice(&bytes[1..]);
            let mut padded_payload = bytes.clone();
            *padded_payload.last_mut().unwrap() |= 0x80;
            padded_payload.push(0);
            let mut duplicated = bytes.clone();
            duplicated.extend_from_slice(&bytes);
            let mut trailing = bytes.clone();
            trailing.extend_from_slice(&[0x1a, 0]);
            let mut unknown = bytes.clone();
            unknown[0] = 0x20;
            let truncated = bytes[..bytes.len() - 1].to_vec();
            let overflowing = vec![bytes[0], 0x80, 0x80, 0x80, 0x80, 0x10];
            for malformed in [
                padded_key, padded_payload, duplicated, trailing, unknown, truncated, overflowing,
            ] {
                proptest::prop_assert_eq!(canonical_var(&malformed), None, "{:02x?}", malformed);
            }
        }
    }
}
