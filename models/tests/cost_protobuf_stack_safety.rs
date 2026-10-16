//! The cost-signature feedback cut must be as stack-safe as the existing Par
//! cut, including the public prost::Message API, not just the codec helpers.

use models::rhoapi::cost_signature::Value;
use models::rhoapi::{CostSignature, CostSignatureCompound, CostStack, Par};
use models::rust::rholang::{protobuf_decoder, protobuf_encoder};
use prost::Message;

mod rust {
    pub use models::rust::*;
}

#[allow(clippy::all, dead_code, unused_imports)]
mod recursive_oracle {
    include!(concat!(env!("OUT_DIR"), "/rhoapi_recursive_oracle.rs"));
}

const DEPTH: usize = 4_096;
const SMALL_STACK: usize = 256 * 1024;

fn compound_chain() -> CostSignature {
    let mut signature = CostSignature {
        value: Some(Value::Ground(vec![0, 1, 255])),
    };
    for _ in 0..DEPTH {
        signature = CostSignature {
            value: Some(Value::Compound(CostSignatureCompound {
                elements: vec![signature],
            })),
        };
    }
    signature
}

fn compound_depth(signature: &CostSignature) -> usize {
    let mut depth = 0;
    let mut current = signature;
    while let Some(Value::Compound(compound)) = current.value.as_ref() {
        assert_eq!(compound.elements.len(), 1);
        current = &compound.elements[0];
        depth += 1;
    }
    depth
}

#[test]
fn shallow_cost_message_bytes_and_errors_match_unmodified_prost() {
    let cases = [
        CostSignature::default(),
        CostSignature {
            value: Some(Value::Ground(vec![0, 255])),
        },
        CostSignature {
            value: Some(Value::Ground(Vec::new())),
        },
        CostSignature {
            value: Some(Value::BoundLevel(-7)),
        },
        CostSignature {
            value: Some(Value::BoundLevel(0)),
        },
        CostSignature {
            value: Some(Value::Quote(Par::default())),
        },
        CostSignature {
            value: Some(Value::Name(Par::default())),
        },
        CostSignature {
            value: Some(Value::Unit(true)),
        },
        CostSignature {
            value: Some(Value::Unit(false)),
        },
        CostSignature {
            value: Some(Value::Compound(CostSignatureCompound {
                elements: vec![
                    CostSignature {
                        value: Some(Value::BoundLevel(3)),
                    },
                    CostSignature {
                        value: Some(Value::Ground(vec![1, 2])),
                    },
                ],
            })),
        },
    ];
    for case in &cases {
        let bytes = case.encode_to_vec();
        let oracle = recursive_oracle::CostSignature::decode(bytes.as_slice())
            .expect("recursive Prost accepts the shallow cost fixture");
        assert_eq!(bytes, oracle.encode_to_vec());
        for cut in 0..=bytes.len() {
            let prefix = &bytes[..cut];
            let production = CostSignature::decode(prefix);
            let reference = recursive_oracle::CostSignature::decode(prefix);
            match (production, reference) {
                (Ok(actual), Ok(expected)) => assert_eq!(actual.encode_to_vec(), expected.encode_to_vec()),
                (Err(actual), Err(expected)) => assert_eq!(actual, expected),
                (actual, expected) => panic!(
                    "cost protobuf prefix {cut}/{} diverged: production {actual:?}, oracle {expected:?}",
                    bytes.len()
                ),
            }
        }
    }
}

#[test]
fn repeated_compound_oneof_fields_merge_like_prost() {
    let first = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: vec![CostSignature {
                value: Some(Value::BoundLevel(1)),
            }],
        })),
    };
    let second = CostSignature {
        value: Some(Value::Compound(CostSignatureCompound {
            elements: vec![CostSignature {
                value: Some(Value::Ground(vec![2])),
            }],
        })),
    };
    let mut bytes = first.encode_to_vec();
    bytes.extend(second.encode_to_vec());
    let actual = CostSignature::decode(bytes.as_slice()).expect("production compound merge");
    let expected = recursive_oracle::CostSignature::decode(bytes.as_slice())
        .expect("recursive Prost compound merge");
    assert_eq!(actual.encode_to_vec(), expected.encode_to_vec());
    let Some(Value::Compound(compound)) = actual.value.as_ref() else {
        panic!("repeated compound oneof must remain a compound");
    };
    assert_eq!(compound.elements.len(), 2);

    let mut merged = first;
    merged
        .merge(second.encode_to_vec().as_slice())
        .expect("merge into existing compound");
    assert_eq!(merged.encode_to_vec(), expected.encode_to_vec());
}

#[test]
fn standalone_cost_signature_message_round_trip_is_stack_safe() {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let mut signature = compound_chain();
            assert_eq!(compound_depth(&signature), DEPTH);
            let bytes = signature.encode_to_vec();
            assert_eq!(bytes, protobuf_encoder::encode_to_vec(&signature));
            assert_eq!(signature.encoded_len(), bytes.len());
            let decoded = CostSignature::decode(bytes.as_slice()).expect("prost Message decode");
            let framed = signature.encode_length_delimited_to_vec();
            let framed_decoded = CostSignature::decode_length_delimited(framed.as_slice())
                .expect("length-delimited Message decode");
            let machine = protobuf_decoder::decode_cost_signature(bytes.as_slice())
                .expect("generated decoder entry");
            assert_eq!(compound_depth(&decoded), DEPTH);
            assert_eq!(compound_depth(&framed_decoded), DEPTH);
            assert_eq!(compound_depth(&machine), DEPTH);
            assert_eq!(decoded.encode_to_vec(), bytes);
            assert_eq!(machine.encode_to_vec(), bytes);
            Message::clear(&mut signature);
            assert!(signature.value.is_none());
        })
        .expect("spawn small-stack signature roundtrip")
        .join()
        .expect("standalone cost signature Message must use explicit stacks");
}

#[test]
fn standalone_compound_message_and_alternating_quote_par_round_trip_are_stack_safe() {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let compound = CostSignatureCompound {
                elements: vec![compound_chain()],
            };
            let bytes = compound.encode_to_vec();
            let decoded =
                CostSignatureCompound::decode(bytes.as_slice()).expect("compound Message decode");
            let framed = compound.encode_length_delimited_to_vec();
            let framed_decoded = CostSignatureCompound::decode_length_delimited(framed.as_slice())
                .expect("length-delimited compound Message decode");
            assert_eq!(compound_depth(&decoded.elements[0]), DEPTH);
            assert_eq!(compound_depth(&framed_decoded.elements[0]), DEPTH);
            assert_eq!(decoded.encode_to_vec(), bytes);

            let mut signature = CostSignature {
                value: Some(Value::BoundLevel(7)),
            };
            for _ in 0..DEPTH {
                let mut quoted = Par::default();
                quoted.cost_stacks = vec![CostStack {
                    cells: vec![signature],
                    parallel_tail: Vec::new(),
                }];
                signature = CostSignature {
                    value: Some(Value::Quote(quoted)),
                };
            }
            let bytes = signature.encode_to_vec();
            let decoded = CostSignature::decode(bytes.as_slice())
                .expect("alternating Quote/Par/CostStack decode");
            assert_eq!(decoded.encode_to_vec(), bytes);
        })
        .expect("spawn small-stack compound/alternating roundtrip")
        .join()
        .expect("cost/Par cycles must use explicit protobuf stacks");
}

#[test]
fn shallow_parallel_stack_wire_matches_unmodified_prost() {
    let stack = CostStack {
        cells: vec![CostSignature {
            value: Some(Value::BoundLevel(3)),
        }],
        parallel_tail: vec![CostStack::default(), CostStack {
            cells: vec![CostSignature {
                value: Some(Value::Ground(vec![1, 2])),
            }],
            parallel_tail: Vec::new(),
        }],
    };
    let bytes = stack.encode_to_vec();
    let oracle = recursive_oracle::CostStack::decode(bytes.as_slice()).unwrap();
    assert_eq!(bytes, oracle.encode_to_vec());
    for cut in 0..=bytes.len() {
        match (
            CostStack::decode(&bytes[..cut]),
            recursive_oracle::CostStack::decode(&bytes[..cut]),
        ) {
            (Ok(actual), Ok(expected)) => {
                assert_eq!(actual.encode_to_vec(), expected.encode_to_vec());
            }
            (Err(actual), Err(expected)) => assert_eq!(actual, expected),
            (actual, expected) => {
                panic!("parallel stack protobuf prefix {cut} differs: {actual:?} vs {expected:?}")
            }
        }
    }
}

#[test]
fn standalone_parallel_stack_message_merge_and_clear_are_stack_safe() {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let mut stack = CostStack {
                cells: vec![CostSignature {
                    value: Some(Value::BoundLevel(7)),
                }],
                parallel_tail: Vec::new(),
            };
            for _ in 0..DEPTH {
                stack = CostStack {
                    cells: Vec::new(),
                    parallel_tail: vec![stack],
                };
            }
            let bytes = stack.encode_to_vec();
            assert_eq!(stack.encoded_len(), bytes.len());
            let mut decoded = CostStack::decode(bytes.as_slice()).expect("deep stack decode");
            let mut framed = CostStack::decode_length_delimited(
                stack.encode_length_delimited_to_vec().as_slice(),
            )
            .expect("deep framed stack decode");
            let mut merged = CostStack::default();
            merged.merge(bytes.as_slice()).expect("deep stack merge");
            assert_eq!(decoded.encode_to_vec(), bytes);
            assert_eq!(framed.encode_to_vec(), bytes);
            assert_eq!(merged.encode_to_vec(), bytes);
            let mut current = &decoded;
            for _ in 0..DEPTH {
                assert!(current.cells.is_empty());
                assert_eq!(current.parallel_tail.len(), 1);
                current = &current.parallel_tail[0];
            }
            assert_eq!(current.cells.len(), 1);
            for value in [&mut stack, &mut decoded, &mut framed, &mut merged] {
                Message::clear(value);
                assert!(value.cells.is_empty() && value.parallel_tail.is_empty());
            }
        })
        .expect("spawn small-stack parallel codec worker")
        .join()
        .expect("CostStack protobuf API must use explicit stacks");
}

#[test]
fn ordinary_parallel_stack_drop_is_stack_safe() {
    std::thread::Builder::new()
        .stack_size(SMALL_STACK)
        .spawn(|| {
            let mut stack = CostStack::default();
            for _ in 0..DEPTH {
                stack = CostStack {
                    cells: Vec::new(),
                    parallel_tail: vec![stack],
                };
            }
            drop(stack);
        })
        .expect("spawn small-stack drop worker")
        .join()
        .expect("ordinary CostStack drop must use an explicit worklist");
}
