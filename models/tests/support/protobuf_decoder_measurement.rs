use std::hint::black_box;
use std::io::Write;
use std::time::Instant;

use crypto::rust::hash::blake2b256::Blake2b256;
use pathmap::PathMap;
use prost::bytes::Buf;
use prost::encoding::{self, DecodeContext, WireType};
use prost::{DecodeError, Message};

use crate::rhoapi::expr::ExprInstance;
use crate::rhoapi::{EList, ENot, EPathMap, Expr, Par, Send};
use crate::rust::canonical_path::encode_trie_path;
use crate::rust::epathmap_trie_codec::payload_mapping_tests::{
    allocation_checkpoint, capture_allocations, AllocationCounts,
};
use crate::rust::epathmap_trie_codec::EPathMapRepr;
use crate::rust::par_construction::substitution_cost::{capture, Metric};
use crate::rust::par_construction::{ParConstruction, ProtobufParBuilder};
use crate::rust::rholang::protobuf_decoder::{self as decoder, owned_slots as owned};
use crate::rust::utils::new_gint_par;

const SHAPES: &[&str] = &[
    "enot",
    "enot-alias",
    "enot-error",
    "incomplete",
    "list",
    "mixed",
    "mixed-metadata",
    "map",
    "wide-distinct",
    "wide-duplicate",
    "wide-late-duplicate",
    "wide-late-new",
];
const ENTRIES: &[&str] = &["par", "message", "merge", "delimited", "send"];

fn key() -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GBool(true)),
    }])
}

fn planned_fixture(shape: &str, depth: usize) -> Vec<u8> {
    let key = encode_trie_path(&key());
    let mut arena = ParConstruction::new();
    let mut value = ProtobufParBuilder::default();
    for level in 0..depth {
        let component = if shape == "map" || level % 3 == 2 {
            let child = arena.freeze_protobuf_par(value).unwrap();
            let mut map = PathMap::new();
            map.insert(&key, child);
            let repr = arena.own_decoded_epm_repr(EPathMapRepr::Map(map));
            owned::Expr {
                expr_instance: Some(owned::ExprExprInstance::EPathmapBody(owned::EPathMap {
                    repr,
                    ..Default::default()
                })),
            }
            .plan(&mut arena)
            .unwrap()
        } else if level % 3 == 0 {
            owned::Expr {
                expr_instance: Some(owned::ExprExprInstance::ENotBody(owned::ENot {
                    p: Some(value),
                })),
            }
            .plan(&mut arena)
            .unwrap()
        } else {
            owned::Send {
                chan: Some(value),
                ..Default::default()
            }
            .plan(&mut arena)
            .unwrap()
        };
        value = ProtobufParBuilder::default();
        arena
            .insert_protobuf_component(&mut value, component)
            .unwrap();
        if shape == "mixed-metadata" && level % 2 == 0 {
            value
                .merge_protobuf_locally_free(
                    WireType::LengthDelimited,
                    &mut &[1, 1][..],
                    DecodeContext::default(),
                )
                .unwrap();
            value
                .merge_protobuf_connective_used(
                    WireType::Varint,
                    &mut &[1][..],
                    DecodeContext::default(),
                )
                .unwrap();
        }
    }
    let root = arena.freeze_protobuf_par(value).unwrap();
    arena.protobuf_bytes(root)
}

fn fixture(shape: &str, size: usize) -> Vec<u8> {
    match shape {
        "enot" => super::enot_bytes(size, &[]),
        "enot-alias" => {
            let mut bytes = super::enot_bytes(size, &[]);
            bytes.extend_from_slice(&[0x50, 1, 0x50, 0]);
            bytes
        }
        "enot-error" => super::enot_bytes(size, &[0]),
        "incomplete" => super::enot_bytes(size, &[0x2a, 0]),
        "list" => Par::wrap_unary_elist(size, new_gint_par(0, vec![], false))
            .unwrap()
            .encode_to_vec(),
        "mixed" | "mixed-metadata" | "map" => planned_fixture(shape, size),
        "wide-distinct" | "wide-duplicate" | "wide-late-duplicate" | "wide-late-new" => {
            let mut bytes = Vec::new();
            for i in 0..size {
                let value = match shape {
                    "wide-duplicate" => 7,
                    "wide-late-duplicate" if i + 1 == size && size > 1 => 0,
                    "wide-late-new" if i + 1 == size && size > 1 => -1,
                    _ => i as i64,
                };
                let expr = Expr {
                    expr_instance: Some(ExprInstance::GInt(value)),
                };
                encoding::bytes::encode(5, &expr.encode_to_vec(), &mut bytes);
            }
            bytes
        }
        _ => panic!("unknown fixture shape"),
    }
}

fn resident_fixture(shape: &str, depth: usize) -> Par {
    let mut value = if shape == "list" {
        new_gint_par(0, vec![], false)
    } else {
        Par::default()
    };
    for level in 0..depth {
        value = if shape == "list" {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EListBody(EList {
                    ps: vec![value],
                    ..Default::default()
                })),
            }])
        } else if shape == "map" || (shape != "enot" && level % 3 == 2) {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::EPathmapBody(EPathMap::new_map(
                    [(key(), value)],
                    vec![],
                    false,
                    None,
                ))),
            }])
        } else if shape == "enot" || level % 3 == 0 {
            Par::default().with_exprs(vec![Expr {
                expr_instance: Some(ExprInstance::ENotBody(ENot { p: Some(value) })),
            }])
        } else {
            Par::default().with_sends(vec![Send {
                chan: Some(value),
                ..Default::default()
            }])
        };
        if shape == "mixed-metadata" && level % 2 == 0 {
            value.replace_locally_free(vec![1]);
            value.replace_connective_used(true);
        }
    }
    value
}

fn envelope(entry: &str, body: Vec<u8>) -> Vec<u8> {
    match entry {
        "send" => {
            let mut bytes = Vec::new();
            encoding::bytes::encode(1, &body, &mut bytes);
            bytes
        }
        "delimited" => {
            let mut bytes = Vec::new();
            encoding::encode_varint(body.len() as u64, &mut bytes);
            bytes.extend_from_slice(&body);
            bytes
        }
        _ => body,
    }
}

struct Outcome {
    value: Option<Par>,
    error: Option<DecodeError>,
}

fn decode(entry: &str, input: &mut dyn Buf, seed: Par) -> Outcome {
    let result = match entry {
        "par" => decoder::decode_par(input),
        "message" => Par::decode(input),
        "send" => decoder::decode_send(input).map(|send| send.chan.unwrap_or_default()),
        "merge" | "delimited" => {
            let mut value = seed;
            let result = if entry == "merge" {
                value.merge(input)
            } else {
                value.merge_length_delimited(input)
            };
            return Outcome {
                value: Some(value),
                error: result.err(),
            };
        }
        _ => panic!("unknown entry"),
    };
    match result {
        Ok(value) => Outcome {
            value: Some(value),
            error: None,
        },
        Err(error) => Outcome {
            value: None,
            error: Some(error),
        },
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Witness {
    protobuf: Vec<u8>,
    compact: Vec<u8>,
    bincode: Vec<u8>,
    locally_free: Vec<u8>,
    connective_used: bool,
    cardinality: u64,
    error: Option<String>,
    remaining: usize,
}

fn witness(entry: &str, bytes: &[u8], split: usize, frozen: bool) -> Witness {
    let mut input = bytes[..split].chain(&bytes[split..]);
    let seed = new_gint_par(71, vec![], false);
    let run = || decode(entry, &mut input, seed);
    let output = if frozen { super::legacy(run) } else { run() };
    let (protobuf, compact, bincode, locally_free, connective_used, cardinality) =
        match output.value {
            Some(value) => (
                value.encode_to_vec(),
                value.components().compact_bytes(),
                bincode::serialize(&value).unwrap(),
                value.locally_free().to_vec(),
                value.connective_used(),
                value.cardinality(),
            ),
            None => (vec![], vec![], vec![], vec![], false, 0),
        };
    Witness {
        protobuf,
        compact,
        bincode,
        locally_free,
        connective_used,
        cardinality,
        error: output.error.map(|error| error.to_string()),
        remaining: input.remaining(),
    }
}

fn digest(bytes: &[u8]) -> String { hex::encode(Blake2b256::hash_parts([bytes])) }

fn allocation_row(counts: AllocationCounts) -> serde_json::Value {
    serde_json::json!({ "allocations": counts.allocations, "requested_bytes": counts.allocated,
        "freed_bytes": counts.freed, "outstanding_bytes": counts.outstanding(),
        "peak_outstanding_bytes": counts.peak, "overflow": counts.overflow })
}

#[test]
fn measurement_fixtures_match_independent_resident_oracles() {
    for depth in [0, 1, 4, 16] {
        let alias = fixture("enot-alias", depth);
        let value = super::legacy(|| decoder::decode_par(alias.as_slice())).unwrap();
        assert_eq!(value.encode_to_vec(), fixture("enot", depth));
    }
    for shape in ["enot", "list", "mixed", "mixed-metadata", "map"] {
        for size in [0, 1, 4, 16] {
            let expected = resident_fixture(shape, size);
            assert_eq!(
                fixture(shape, size),
                expected.encode_to_vec(),
                "{shape}:{size}"
            );
        }
    }
    for shape in [
        "wide-distinct",
        "wide-duplicate",
        "wide-late-duplicate",
        "wide-late-new",
    ] {
        for size in [0, 1, 4, 16] {
            let input = fixture(shape, size);
            let expected = Par::default().with_exprs(
                (0..size)
                    .map(|i| Expr {
                        expr_instance: Some(ExprInstance::GInt(match shape {
                            "wide-duplicate" => 7,
                            "wide-late-duplicate" if i + 1 == size && size > 1 => 0,
                            "wide-late-new" if i + 1 == size && size > 1 => -1,
                            _ => i as i64,
                        })),
                    })
                    .collect(),
            );
            if matches!(shape, "wide-late-duplicate" | "wide-late-new") {
                let observed = super::legacy(|| decoder::decode_par(input.as_slice())).unwrap();
                assert_eq!(observed.encode_to_vec(), expected.encode_to_vec());
            } else {
                assert_eq!(input, expected.encode_to_vec());
            }
        }
    }
}

#[test]
fn measurement_entries_preserve_full_observations_and_arm_routing() {
    for &shape in SHAPES {
        for &entry in ENTRIES {
            let bytes = envelope(entry, fixture(shape, 4));
            for split in [1.min(bytes.len()), bytes.len()] {
                super::COUNTS.set(super::Counts::default());
                let old = witness(entry, &bytes, split, true);
                assert!(super::COUNTS.get().frozen > 0);
                assert_eq!(super::COUNTS.get().successor, 0);
                super::COUNTS.set(super::Counts::default());
                let new = witness(entry, &bytes, split, false);
                assert_eq!(super::COUNTS.get().frozen, 0);
                assert!(super::COUNTS.get().successor > 0);
                assert_eq!(old, new, "{shape}:{entry}:{split}");
            }
        }
    }
}

#[test]
#[ignore = "explicit resource cell, run under systemd memory and time limits"]
fn measure_decoder_cell() {
    let read = |name| std::env::var(name).unwrap_or_else(|_| panic!("missing {name}"));
    let arm = read("DECODER_MEASURE_ARM");
    assert!(["frozen", "planned"].contains(&arm.as_str()));
    let shape = read("DECODER_MEASURE_SHAPE");
    assert!(SHAPES.contains(&shape.as_str()));
    let size: usize = read("DECODER_MEASURE_SIZE").parse().unwrap();
    let entry = read("DECODER_MEASURE_ENTRY");
    assert!(ENTRIES.contains(&entry.as_str()));
    let form = read("DECODER_MEASURE_INPUT");
    assert!(["fragmented", "contiguous"].contains(&form.as_str()));
    let sample = read("DECODER_MEASURE_SAMPLE");
    let setup = Instant::now();
    let bytes = envelope(&entry, fixture(&shape, size));
    let setup_ns = setup.elapsed().as_nanos();
    let split = if form == "fragmented" {
        1.min(bytes.len())
    } else {
        bytes.len()
    };
    let mut input = bytes[..split].chain(&bytes[split..]);
    let owner = new_gint_par(71, vec![], false);
    let seed = owner.clone();
    super::COUNTS.set(super::Counts::default());
    let header = serde_json::json!({"arm":arm,"shape":shape,"size":size,"entry":entry,
        "input":form,"sample":sample,"input_bytes":bytes.len(),"input_digest":digest(&bytes),
        "setup_ns":setup_ns,"pid":std::process::id()});
    println!("DECODER_BEGIN\t{header}");
    std::io::stdout().flush().unwrap();
    let run = || {
        capture(|| {
            capture_allocations(|| {
                let begin = Instant::now();
                let output = black_box(decode(&entry, &mut input, seed));
                let decode_ns = begin.elapsed().as_nanos();
                let retained = allocation_checkpoint();
                let success = output.error.is_none();
                let begin = Instant::now();
                drop(output);
                let drop_ns = begin.elapsed().as_nanos();
                (decode_ns, drop_ns, retained, success)
            })
        })
    };
    let (((decode_ns, drop_ns, retained, success), lifecycle), counters) = if arm == "frozen" {
        super::legacy(run)
    } else {
        run()
    };
    let routes = super::COUNTS.get();
    assert!(!retained.overflow && !lifecycle.overflow);
    assert!(
        retained.outstanding() >= 0 && lifecycle.outstanding() >= 0,
        "negative scoped allocation balance; investigate external frees before interpretation"
    );
    if arm == "frozen" {
        assert!(routes.frozen > 0 && routes.successor == 0);
    } else {
        assert!(routes.successor > 0 && routes.frozen == 0);
    }
    if arm == "planned" && entry == "par" && form == "fragmented" && success {
        assert_eq!(counters.get(Metric::FinishAttempts), 1);
        assert_eq!(counters.get(Metric::FinishSuccesses), 1);
    }
    let remaining = input.remaining();
    let result = serde_json::json!({"cell":header,"decode_ns":decode_ns,"drop_ns":drop_ns,
        "decode":allocation_row(retained),"lifecycle":allocation_row(lifecycle),
        "success":success,"remaining":remaining,"frozen_calls":routes.frozen,"planned_calls":routes.successor,
        "arenas":counters.get(Metric::ArenaCount),"finish_attempts":counters.get(Metric::FinishAttempts),
        "finish_successes":counters.get(Metric::FinishSuccesses),"root_keys":counters.get(Metric::RootKeys),
        "root_key_bytes":counters.get(Metric::RootKeyBytes)});
    println!("DECODER_RESULT\t{result}");
    std::io::stdout().flush().unwrap();
    let output = witness(&entry, &bytes, split, arm == "frozen");
    assert_eq!(output.remaining, remaining);
    assert_eq!(output.error.is_none(), success);
    let witness = serde_json::json!({"cell":header,"protobuf_digest":digest(&output.protobuf),
        "compact_digest":digest(&output.compact),"bincode_digest":digest(&output.bincode),
        "locally_free":output.locally_free,"connective_used":output.connective_used,
        "cardinality":output.cardinality,"error":output.error,"remaining":output.remaining});
    println!("DECODER_WITNESS\t{witness}");
}
