//! The production cold-store path uses the generated explicit bincode encoder
//! and the hand-written decoder. The derived serde implementation is retained
//! only as a shallow byte oracle, not as a deep traversal.

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostStack, Par};
use models::rust::rholang::bincode_encoder;
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
            let par = Par {
                cost_stacks: vec![CostStack {
                    cells: vec![signature],
                }],
                ..Par::default()
            };
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
