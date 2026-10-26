// D-D4 (DR-106) premise: the equality of `Par` is reflexive. Native replay
// records the position of each channel's entry when it builds the channel
// data. The recorded position equals the first-equal search in the finished
// entries only if every channel equals itself (`ChannelPositions.v`,
// `non_reflexive_equality_breaks_recorded_positions`). `Par` stores floats as
// raw bits and compares them as integers, so a NaN payload equals itself. The
// equality ignores `locally_free`, so equal values need not be identical, and
// reflexivity does not need that.

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{Expr, Par, Send};
use proptest::prelude::*;

fn equals<T: PartialEq>(left: &T, right: &T) -> bool { left == right }

fn leaf() -> impl Strategy<Value = Expr> {
    prop_oneof![
        any::<u64>().prop_map(ExprInstance::GDouble),
        prop_oneof![
            Just(f64::NAN.to_bits()),
            Just(0x7ff0_0000_0000_0001_u64),
            Just((-0.0_f64).to_bits()),
        ]
        .prop_map(ExprInstance::GDouble),
        any::<u32>().prop_map(ExprInstance::GFloat32),
        Just(f32::NAN.to_bits()).prop_map(ExprInstance::GFloat32),
        any::<i64>().prop_map(ExprInstance::GInt),
        ".{0,16}".prop_map(ExprInstance::GString),
        prop::collection::vec(any::<u8>(), 0..32).prop_map(ExprInstance::GByteArray),
    ]
    .prop_map(|instance| Expr {
        expr_instance: Some(instance),
    })
}

fn par() -> impl Strategy<Value = Par> {
    (
        prop::collection::vec(leaf(), 0..4),
        prop::collection::vec(any::<u8>(), 0..4),
    )
        .prop_map(|(exprs, locally_free)| Par {
            exprs,
            locally_free,
            ..Default::default()
        })
        .prop_recursive(3, 32, 4, |child| {
            (
                child.clone(),
                prop::collection::vec(child, 0..4),
                any::<bool>(),
            )
                .prop_map(|(chan, data, persistent)| Par {
                    sends: vec![Send {
                        chan: Some(chan),
                        data,
                        persistent,
                        ..Default::default()
                    }],
                    ..Default::default()
                })
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn par_equality_is_reflexive(value in par()) {
        prop_assert!(equals(&value, &value));
        let copy = value.clone();
        prop_assert!(equals(&copy, &value));
        prop_assert!(equals(&value, &copy));
    }
}

/// The premise needs the raw-bit encoding: IEEE 754 equality is not reflexive
/// on NaN.
#[test]
fn nan_payload_equals_itself_only_as_raw_bits() {
    let nan = Par {
        exprs: vec![Expr {
            expr_instance: Some(ExprInstance::GDouble(f64::NAN.to_bits())),
        }],
        ..Default::default()
    };
    assert!(equals(&nan, &nan));
    assert!(!equals(&f64::NAN, &f64::NAN));
}

/// The equality ignores `locally_free`, so equal channels need not be
/// identical.
#[test]
fn par_equality_ignores_locally_free() {
    let left = Par {
        locally_free: vec![1],
        ..Default::default()
    };
    let right = Par {
        locally_free: vec![2],
        ..Default::default()
    };
    assert!(equals(&left, &right));
    assert_ne!(left.locally_free, right.locally_free);
}
