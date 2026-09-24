use prost::bytes::{Buf, Bytes};
use prost::Message;

use crate::protobuf_decoder_oracle::{enot_bytes, legacy};
use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::var::VarInstance;
use crate::rhoapi::{self};

fn observe(mut input: impl Buf) -> (Result<Vec<u8>, prost::DecodeError>, usize) {
    let result = super::decode_par(&mut input).map(|value| value.encode_to_vec());
    (result, input.remaining())
}

fn compare(bytes: &[u8]) {
    for split in [0, bytes.len() / 2, bytes.len()] {
        let input = || {
            Bytes::copy_from_slice(&bytes[..split]).chain(Bytes::copy_from_slice(&bytes[split..]))
        };
        assert_eq!(legacy(|| observe(input())), observe(input()));
    }
}

#[test]
fn deep_general_decoder_materializes_once_and_retires_failed_frames_on_small_stack() {
    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            for depth in [1, 32] {
                compare(&enot_bytes(depth, &[]));
                compare(&enot_bytes(depth, &[0]));
            }
            let bytes = enot_bytes(8192, &[]);
            #[cfg(feature = "protobuf-projection-experiment")]
            let decoded = {
                use crate::rust::par_construction::substitution_cost::{capture, Metric};
                let (result, counters) =
                    capture(|| super::decode_par_with_stats(bytes[..1].chain(&bytes[1..])));
                assert_eq!(counters.get(Metric::FinishAttempts), 1);
                result
            };
            #[cfg(not(feature = "protobuf-projection-experiment"))]
            let decoded = super::decode_par_with_stats(bytes[..1].chain(&bytes[1..]));
            let (value, stats) = decoded.unwrap();
            assert!(stats.max_frames > 8192, "must exercise the general PDA");
            assert_eq!(value.encode_to_vec(), bytes);
            drop(value);
            let malformed = enot_bytes(8192, &[0]);
            let error = super::decode_par(malformed.as_slice()).unwrap_err();
            assert!(error.to_string().contains("invalid tag value: 0"));
            let mut machine = super::Machine::new(
                malformed.as_slice(),
                super::Node::Par {
                    value: Default::default(),
                    limit: 0,
                },
                true,
            );
            assert!(machine.run().is_err());
            assert!(!machine.frames.is_empty());
            machine.dismantle();
            assert!(machine.is_fully_drained());
        })
        .unwrap()
        .join()
        .unwrap();
}

#[test]
fn nested_snapshot_inputs_restore_parent_and_preserve_failure_contexts() {
    let mut value = rhoapi::Par::default();
    for n in 0..3 {
        let key = rhoapi::Par::default().with_exprs(vec![rhoapi::Expr {
            expr_instance: Some(ExprInstance::GInt(n)),
        }]);
        let map = rhoapi::EPathMap::new_map([(key, value)], vec![n as u8 + 1], true, None);
        value = rhoapi::Par::default().with_exprs(vec![rhoapi::Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }]);
    }
    let mut bytes = value.encode_to_vec();
    bytes.extend_from_slice(&[0x50, 1]);
    for end in 0..=bytes.len() {
        compare(&bytes[..end]);
    }
    let mut machine = super::Machine::new(
        bytes.as_slice(),
        super::Node::Par {
            value: Default::default(),
            limit: 0,
        },
        true,
    );
    machine.run().unwrap();
    assert_eq!(machine.epm_depth, 0);
    assert!(machine.input_stack.is_empty());
    assert_eq!(machine.buf.remaining(), 0);
    assert_eq!(machine.values.len(), 1);
}

#[test]
fn epm_remainder_reopens_all_native_var_variants_without_losing_mode() {
    let variants = [
        None,
        Some(VarInstance::BoundVar(7)),
        Some(VarInstance::FreeVar(11)),
        Some(VarInstance::Wildcard(Default::default())),
    ];
    for first in &variants {
        for second in &variants {
            let mut body = Vec::new();
            for variant in [first, second] {
                prost::encoding::bytes::encode(
                    5,
                    &rhoapi::Var {
                        var_instance: variant.clone(),
                    }
                    .encode_to_vec(),
                    &mut body,
                );
            }
            for end in 0..=body.len() {
                let observe = || {
                    let mut input = &body[..end];
                    let result =
                        super::decode_epath_map(&mut input).map(|value| value.encode_to_vec());
                    (result, input.remaining())
                };
                assert_eq!(legacy(observe), observe());
            }
        }
    }
}

#[test]
fn nested_error_locations_have_an_independent_nearest_first_witness() {
    let bytes = enot_bytes(1, &[0]);
    assert_eq!(bytes, [0x2a, 5, 0x2a, 3, 0x0a, 1, 0]);
    assert_eq!(super::decode_par(bytes.as_slice()).unwrap_err().to_string(),
        "failed to decode Protobuf message: ENot.p: Expr.expr_instance: Par.exprs: invalid tag value: 0");
}

fn pending_map_failure_bytes(depth: usize) -> Vec<u8> {
    use crate::rust::epathmap_trie_codec::{self, EPathMapRepr, OwnedResidentEPathMap};
    let mut owner = OwnedResidentEPathMap::default();
    *owner.as_repr_mut() = EPathMapRepr::Map(pathmap::PathMap::new());
    let EPathMapRepr::Map(map) = owner.as_repr_mut() else {
        unreachable!()
    };
    let mut key = Vec::with_capacity(depth + 1);
    for d in 1..=depth {
        key.push(1);
        key.push(0);
        if d >= depth - 1 {
            map.set_val_at(&key, rhoapi::Par::default());
        } else {
            map.create_path(&key);
        }
        key.pop();
    }
    let mut snapshot = epathmap_trie_codec::layout(owner.as_repr())
        .prefix()
        .to_vec();
    snapshot.push(0);
    let malformed = enot_bytes(1, &[0]);
    prost::encoding::encode_varint(malformed.len() as u64, &mut snapshot);
    snapshot.extend_from_slice(&malformed);
    let mut bytes = Vec::new();
    prost::encoding::bytes::encode(9, &snapshot, &mut bytes);
    bytes
}

#[test]
fn partial_map_failure_retires_unique_deep_pending_topology_before_arena() {
    std::thread::Builder::new().stack_size(256 * 1024).spawn(|| {
        let shallow = pending_map_failure_bytes(4);
        let observe = || super::decode_epath_map(shallow.as_slice()).unwrap_err();
        assert_eq!(legacy(observe), observe());
        let bytes = pending_map_failure_bytes(8192);
        let mut machine = super::Machine::new(bytes.as_slice(), super::Node::EPathMap {
            value: Default::default(), limit: 0,
        }, true);
        loop {
            let frame = machine.frames.pop().expect("first map value must be pending");
            let attached = matches!(&frame, super::Frame::Attach(super::Attach::EPathMapSnapshotValue { .. }));
            match frame {
                super::Frame::Node(node) => machine.step_node(node),
                super::Frame::Attach(attach) => machine.attach(attach),
                super::Frame::Epm(epm) => machine.step_epm_snapshot(epm),
            }.unwrap();
            if attached { break; }
        }
        assert!(matches!(machine.frames.last(), Some(super::Frame::Epm(_))));
        assert_eq!(machine.run().unwrap_err().to_string(),
            "failed to decode Protobuf message: ENot.p: Expr.expr_instance: Par.exprs: EPathMap.trie_snapshot: invalid tag value: 0");
        assert_eq!(machine.epm_depth, 1);
        assert_eq!(machine.input_stack.len(), 1);
        assert!(machine.frames.iter().any(|frame| matches!(frame,
            super::Frame::Attach(super::Attach::EPathMapSnapshotValue { .. }))));
        machine.dismantle();
        assert!(machine.is_fully_drained());
        drop(machine);
    }).unwrap().join().unwrap();
}
