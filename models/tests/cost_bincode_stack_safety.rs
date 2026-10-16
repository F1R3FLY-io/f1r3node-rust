//! The production cold-store path uses the generated explicit bincode encoder
//! and the hand-written decoder. The derived serde implementation is retained
//! only as a shallow byte oracle, not as a deep traversal.

use std::hash::{Hash, Hasher};

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostStack, Par};
use models::rust::rholang::bincode_encoder;
use models::rust::rholang::par_children::dismantle_all;
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;

const DEPTH: usize = 4_096;

#[test]
fn cold_store_cost_compound_round_trip_is_stack_safe() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut signature = CostSignature {
                value: Some(Value::BoundLevel(1)),
            };
            for _ in 0..DEPTH {
                signature = CostSignature {
                    value: Some(Value::Compound(CostSignatureCompound {
                        elements: vec![signature],
                    })),
                };
            }
            let mut par = Par::default();
            par.cost_stacks = vec![CostStack {
                cells: vec![signature],
                parallel_tail: Vec::new(),
            }];
            let bytes = bincode_encoder::encode(&par);
            let decoded = Par::cold_decode(&bytes).expect("iterative cold-store decode");
            let mut current = &decoded.cost_stacks[0].cells[0];
            for _ in 0..DEPTH {
                let Some(Value::Compound(compound)) = current.value.as_ref() else {
                    panic!("lost a compound level in cold storage");
                };
                assert_eq!(compound.elements.len(), 1);
                current = &compound.elements[0];
            }
            assert!(matches!(current.value.as_ref(), Some(Value::BoundLevel(1))));
            assert_eq!(bincode_encoder::encode(&decoded), bytes);
        })
        .expect("spawn small-stack cost cold-store roundtrip")
        .join()
        .expect("cost cold-store traversal must not grow the native stack");
}

#[test]
fn v6_linear_stack_bytes_are_unchanged_and_v7_tail_round_trips() {
    let linear = CostStack {
        cells: vec![CostSignature {
            value: Some(Value::Unit(true)),
        }],
        parallel_tail: Vec::new(),
    };
    // Pre-parallel-tail positional bincode: one cell, Some(Unit), enum arm 5,
    // and `true`. There was no trailing sequence length in CostV1/v6.
    let old_linear_bytes = [1, 0, 0, 0, 0, 0, 0, 0, 1, 5, 0, 0, 0, 1];
    assert_eq!(bincode_encoder::encode_cost_v1(&linear), old_linear_bytes);
    // The standalone Legacy emitter shares this one-field shape, although
    // authenticated Legacy events cannot contain a CostStack at all.
    assert_eq!(bincode_encoder::encode_legacy(&linear), old_linear_bytes);
    assert_eq!(
        bincode_encoder::encode(&linear),
        [old_linear_bytes.as_slice(), &[0; 8]].concat(),
    );

    let branched = CostStack {
        cells: linear.cells.clone(),
        parallel_tail: vec![linear.clone(), CostStack {
            cells: vec![CostSignature {
                value: Some(Value::BoundLevel(2)),
            }],
            parallel_tail: Vec::new(),
        }],
    };
    let mut par = Par::default();
    par.cost_stacks.push(branched.clone());
    let bytes = bincode_encoder::encode(&par);
    let decoded = Par::cold_decode(&bytes).expect("CostV2 shared-prefix stack");
    assert_eq!(decoded.cost_stacks, vec![branched]);
    assert_eq!(bincode_encoder::encode(&decoded), bytes);
}

#[test]
fn deeply_nested_parallel_tails_use_the_explicit_codec_and_drop_walks() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut stack = CostStack {
                cells: vec![CostSignature {
                    value: Some(Value::BoundLevel(0)),
                }],
                parallel_tail: Vec::new(),
            };
            for _ in 0..DEPTH {
                stack = CostStack {
                    cells: Vec::new(),
                    parallel_tail: vec![stack],
                };
            }
            let mut source = Par::default();
            source.cost_stacks.push(stack);
            let bytes = bincode_encoder::encode(&source);
            let decoded = Par::cold_decode(&bytes).expect("deep CostV2 shared-prefix stack");
            assert_eq!(bincode_encoder::encode(&decoded), bytes);
            assert_eq!(
                source, decoded,
                "generated Eq must walk CostStack iteratively"
            );
            assert_eq!(source.cmp(&decoded), std::cmp::Ordering::Equal);
            let mut left_hash = std::collections::hash_map::DefaultHasher::new();
            let mut right_hash = std::collections::hash_map::DefaultHasher::new();
            source.hash(&mut left_hash);
            decoded.hash(&mut right_hash);
            assert_eq!(left_hash.finish(), right_hash.finish());
            dismantle_all([source, decoded]);
        })
        .expect("spawn small-stack CostV1 codec worker")
        .join()
        .expect("parallel-tail codec and dismantle must not overflow");
}
