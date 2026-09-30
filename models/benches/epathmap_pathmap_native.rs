//! PathMap-native EPathMap time/space experiment.
//!
//! This executable compares the production representation with an explicit
//! list projection used only as a benchmark baseline. It reports set and map
//! specializations separately over shared-prefix keys, including cold/warm
//! EPM1 encoding, indexed lookup, algebra, merkleization, and serialized size.
//! No projection is retained by production code.

use std::collections::HashMap;
use std::hint::black_box;
use std::time::{Duration, Instant};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::epathmap_trie_codec::{self, EPathMapRepr};
use models::rust::rholang::{bincode_encoder, protobuf_decoder, protobuf_encoder};
use pathmap::arena_compact::ArenaCompactTree;
use pathmap::zipper::ZipperReadOnlyIteration;
use prost::bytes::Bytes;
use prost::encoding::encode_varint;

fn gint(value: i64) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GInt(value)),
    }])
}

fn gstring(value: String) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::GString(value)),
    }])
}

fn key(index: usize) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![gint(7), gint(11), gint(13), gint(index as i64)],
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn fixtures(entries: usize, value_bytes: usize) -> (EPathMap, EPathMap, Vec<Vec<u8>>) {
    let keys = (0..entries).map(key).collect::<Vec<_>>();
    let encoded = keys.iter().map(encode_trie_path).collect::<Vec<_>>();
    let set = EPathMap::new(keys.clone(), Vec::new(), false, None);
    let map = EPathMap::new_map(
        keys.into_iter().enumerate().map(|(index, key)| {
            (
                key,
                gstring(format!("{index:08}-{}", "v".repeat(value_bytes))),
            )
        }),
        Vec::new(),
        false,
        None,
    );
    (set, map, encoded)
}

fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

fn measure(reps: usize, mut operation: impl FnMut()) -> Duration {
    let mut samples = Vec::with_capacity(reps);
    for _ in 0..reps {
        let started = Instant::now();
        operation();
        samples.push(started.elapsed());
    }
    median(samples)
}

fn ns_per(duration: Duration, operations: usize) -> f64 {
    duration.as_nanos() as f64 / operations.max(1) as f64
}

fn compact_import_focus(setup_only: bool) {
    use models::rust::par_bag::ParBag;

    fn parameter(name: &str, default: usize) -> usize {
        std::env::var(name).map_or(default, |value| {
            value.parse().expect("integer benchmark parameter")
        })
    }
    fn carrier(map: EPathMap) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EPathmapBody(map)),
        }])
    }

    let depth = parameter("EPATHMAP_IMPORT_DEPTH", 32);
    let endpoints = parameter("EPATHMAP_IMPORT_ENDPOINTS", 256);
    let width = parameter("EPATHMAP_IMPORT_WIDTH", 1_024);
    let repetitions = parameter("EPATHMAP_IMPORT_REPS", 51);
    let warmups = parameter("EPATHMAP_IMPORT_WARMUPS", 3);
    let shape = std::env::var("EPATHMAP_IMPORT_SHAPE").unwrap_or_else(|_| "region".to_string());
    assert!(width > 0 && (1..=256).contains(&endpoints));
    assert!(shape == "region" || shape == "opaque");
    let mut entries = Vec::with_capacity(endpoints);
    for endpoint in 0..endpoints {
        let mut payload = vec![0; width];
        *payload.last_mut().unwrap() = u8::try_from(endpoint).unwrap();
        let mut key = Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::GByteArray(payload)),
        }]);
        for _ in 0..depth {
            key = carrier(if shape == "region" {
                EPathMap::from_set_iter([key], Vec::new(), false, None)
            } else {
                EPathMap::new_map([(key, gint(7))], Vec::new(), false, None)
            });
        }
        entries.push((key, gint(7)));
    }
    let expected = carrier(EPathMap::new_map(entries, Vec::new(), false, None));
    let compact = expected.components().compact_bytes();
    let verified = ParBag::from_compact_bytes(&compact).expect("fixture compact import");
    assert_eq!(&verified, expected.components());
    assert_eq!(verified.compact_snapshot(), compact);
    drop(verified);
    println!("compact-import fixture shape={shape} depth={depth} endpoints={endpoints} width={width} compact_bytes={} mode={}", compact.len(), if setup_only { "setup_only" } else { "import" });
    if setup_only {
        return;
    }
    let mut times = Vec::with_capacity(repetitions);
    for sample in 0..warmups.checked_add(repetitions).unwrap() {
        let started = Instant::now();
        let imported =
            ParBag::from_compact_bytes(black_box(&compact)).expect("measured compact import");
        black_box(&imported);
        drop(imported);
        let elapsed = started.elapsed();
        if sample >= warmups {
            times.push(elapsed.as_nanos());
        }
    }
    for (sample, elapsed) in times.into_iter().enumerate() {
        println!("compact-import sample shape={shape} depth={depth} endpoints={endpoints} width={width} compact_bytes={} sample={sample} elapsed_ns={elapsed}", compact.len());
    }
}

fn legacy_address_ordinal_map_snapshot(repr: &EPathMapRepr<Par>) -> Vec<u8> {
    let EPathMapRepr::Map(map) = repr else {
        panic!("legacy ordinal control requires map mode")
    };
    let mut ordinal_by_address = HashMap::new();
    let mut zipper = map.read_zipper();
    let mut value_count = 0usize;
    while let Some(value) = zipper.to_next_get_val() {
        ordinal_by_address.insert(value as *const Par as usize, value_count as u64);
        value_count += 1;
    }
    let arena = ArenaCompactTree::from_zipper(map.read_zipper(), |value| {
        ordinal_by_address[&(value as *const Par as usize)]
    });

    let mut snapshot = Vec::new();
    snapshot.extend_from_slice(b"EPM1");
    snapshot.extend_from_slice(&[1, 2]);
    encode_varint(arena.get_data().len() as u64, &mut snapshot);
    snapshot.extend_from_slice(arena.get_data());
    encode_varint(value_count as u64, &mut snapshot);
    let mut zipper = map.read_zipper();
    while let Some(value) = zipper.to_next_get_val() {
        let body = protobuf_encoder::encode_to_vec(value);
        encode_varint(body.len() as u64, &mut snapshot);
        snapshot.extend_from_slice(&body);
    }
    snapshot
}

fn main() {
    match std::env::var("EPATHMAP_BENCH_FOCUS").as_deref() {
        Ok("compact_import") => return compact_import_focus(false),
        Ok("compact_import_setup_only") => return compact_import_focus(true),
        _ => {}
    }
    let entries = std::env::var("EPATHMAP_BENCH_ENTRIES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(2_048usize);
    let reps = std::env::var("EPATHMAP_BENCH_REPS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(11usize)
        .max(3);
    let value_bytes = std::env::var("EPATHMAP_BENCH_VALUE_BYTES")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(64usize);
    let focus = std::env::var("EPATHMAP_BENCH_FOCUS").ok();

    let (set, map, encoded_keys) = fixtures(entries, value_bytes);
    let set_projection = set.entry_trie().entries_owned();
    let mut map_projection = Vec::<(Par, Par)>::with_capacity(entries);
    map.entry_trie()
        .for_each_map_entry(|key, value| map_projection.push((key.clone(), value.clone())))
        .unwrap();

    let set_epm1 = epathmap_trie_codec::encode(set.representation());
    let map_epm1 = epathmap_trie_codec::encode(map.representation());
    let legacy_map_epm1 = legacy_address_ordinal_map_snapshot(map.representation());
    let set_protobuf = Bytes::from(protobuf_encoder::encode_to_vec(&set));
    let map_protobuf = Bytes::from(protobuf_encoder::encode_to_vec(&map));
    let set_bincode = bincode_encoder::encode(&set);
    let map_bincode = bincode_encoder::encode(&map);
    let set_projection_bytes = bincode::serialize(&set_projection).unwrap();
    let map_projection_bytes = bincode::serialize(&map_projection).unwrap();

    // A profiling-only route keeps sampling tools on one production operation
    // instead of diluting their call graph across this benchmark's full matrix.
    // It deliberately calls the public decoder and does not introduce a
    // benchmark-only implementation of EPM1 admission.
    if focus.as_deref() == Some("map_epm1_decode") {
        let iterations = std::env::var("EPATHMAP_BENCH_FOCUS_ITERATIONS")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10_000usize);
        let started = Instant::now();
        for _ in 0..iterations {
            black_box(epathmap_trie_codec::decode(black_box(&map_epm1)).unwrap());
        }
        let elapsed = started.elapsed();
        println!(
            "map,focused_epm1_decode_ns_per_iteration,{:.2},iterations={iterations}",
            ns_per(elapsed, iterations)
        );
        return;
    }

    let set_native_lookup = measure(reps, || {
        let EPathMapRepr::Set(trie) = set.representation() else {
            unreachable!()
        };
        for encoded in &encoded_keys {
            black_box(trie.contains(encoded));
        }
    });
    let map_native_lookup = measure(reps, || {
        for encoded in &encoded_keys {
            black_box(
                map.entry_trie()
                    .get_map_value_by_encoded_key(encoded)
                    .unwrap(),
            );
        }
    });
    let set_projection_lookup = measure(reps, || {
        for sought in &set_projection {
            black_box(set_projection.iter().any(|candidate| candidate == sought));
        }
    });
    let map_projection_lookup = measure(reps, || {
        for (sought, _) in &map_projection {
            black_box(
                map_projection
                    .iter()
                    .find(|(candidate, _)| candidate == sought),
            );
        }
    });

    let set_cold_encode = measure(reps, || {
        black_box(epathmap_trie_codec::encode(set.representation()));
    });
    let map_cold_encode = measure(reps, || {
        black_box(epathmap_trie_codec::encode(map.representation()));
    });
    let legacy_map_cold_encode = measure(reps, || {
        black_box(legacy_address_ordinal_map_snapshot(map.representation()));
    });
    black_box(set.trie_snapshot());
    black_box(map.trie_snapshot());
    let set_warm_snapshot = measure(reps, || {
        black_box(set.trie_snapshot());
    });
    let map_warm_snapshot = measure(reps, || {
        black_box(map.trie_snapshot());
    });

    let set_epm1_decode = measure(reps, || {
        black_box(epathmap_trie_codec::decode(black_box(&set_epm1)).unwrap());
    });
    let map_epm1_decode = measure(reps, || {
        black_box(epathmap_trie_codec::decode(black_box(&map_epm1)).unwrap());
    });
    let set_protobuf_decode = measure(reps, || {
        black_box(protobuf_decoder::decode_epath_map(black_box(set_protobuf.clone())).unwrap());
    });
    let map_protobuf_decode = measure(reps, || {
        black_box(protobuf_decoder::decode_epath_map(black_box(map_protobuf.clone())).unwrap());
    });
    let set_bincode_decode = measure(reps, || {
        black_box(bincode::deserialize::<EPathMap>(black_box(&set_bincode)).unwrap());
    });
    let map_bincode_decode = measure(reps, || {
        black_box(bincode::deserialize::<EPathMap>(black_box(&map_bincode)).unwrap());
    });

    let split = entries / 2;
    let set_other = EPathMap::new(
        (split..entries + split).map(key).collect::<Vec<_>>(),
        Vec::new(),
        false,
        None,
    );
    let map_other = EPathMap::new_map(
        (split..entries + split).map(|index| {
            (
                key(index),
                gstring(format!("{index:08}-{}", "v".repeat(value_bytes))),
            )
        }),
        Vec::new(),
        false,
        None,
    );
    let set_join = measure(reps, || {
        black_box(set.entry_trie().try_join(set_other.entry_trie()).unwrap());
    });
    let map_join = measure(reps, || {
        black_box(map.entry_trie().try_join(map_other.entry_trie()).unwrap());
    });

    let set_merkle = measure(reps, || {
        let EPathMapRepr::Set(trie) = set.representation() else {
            unreachable!()
        };
        let mut trie = trie.clone();
        black_box(trie.merkleize());
    });
    let map_merkle = measure(reps, || {
        let EPathMapRepr::Map(trie) = map.representation() else {
            unreachable!()
        };
        let mut trie = trie.clone();
        black_box(trie.merkleize());
    });

    println!(
        "epathmap_pathmap_native entries={entries} shared_prefix_segments=3 value_bytes={value_bytes} reps={reps}"
    );
    println!("mode,metric,native,projection,projection/native");
    println!(
        "set,serialized_bytes,{},{},{:.3}",
        set_epm1.len(),
        set_projection_bytes.len(),
        set_projection_bytes.len() as f64 / set_epm1.len() as f64
    );
    println!(
        "map,serialized_bytes,{},{},{:.3}",
        map_epm1.len(),
        map_projection_bytes.len(),
        map_projection_bytes.len() as f64 / map_epm1.len() as f64
    );
    println!(
        "map,positional_vs_legacy_serialized_bytes,{},{},{:.3}",
        map_epm1.len(),
        legacy_map_epm1.len(),
        legacy_map_epm1.len() as f64 / map_epm1.len() as f64
    );
    println!(
        "set,lookup_ns_per_key,{:.2},{:.2},{:.3}",
        ns_per(set_native_lookup, entries),
        ns_per(set_projection_lookup, entries),
        ns_per(set_projection_lookup, entries) / ns_per(set_native_lookup, entries)
    );
    println!(
        "map,lookup_ns_per_key,{:.2},{:.2},{:.3}",
        ns_per(map_native_lookup, entries),
        ns_per(map_projection_lookup, entries),
        ns_per(map_projection_lookup, entries) / ns_per(map_native_lookup, entries)
    );
    println!("set,cold_epm1_ns,{},{},-", set_cold_encode.as_nanos(), 0);
    println!("map,cold_epm1_ns,{},{},-", map_cold_encode.as_nanos(), 0);
    println!(
        "map,positional_vs_legacy_cold_epm1_ns,{},{},{:.3}",
        map_cold_encode.as_nanos(),
        legacy_map_cold_encode.as_nanos(),
        legacy_map_cold_encode.as_nanos() as f64 / map_cold_encode.as_nanos() as f64
    );
    println!(
        "set,warm_snapshot_accessor_ns,{},{},-",
        set_warm_snapshot.as_nanos(),
        0
    );
    println!(
        "map,warm_snapshot_accessor_ns,{},{},-",
        map_warm_snapshot.as_nanos(),
        0
    );
    println!("set,epm1_decode_ns,{},{},-", set_epm1_decode.as_nanos(), 0);
    println!("map,epm1_decode_ns,{},{},-", map_epm1_decode.as_nanos(), 0);
    println!(
        "set,protobuf_decode_ns,{},{},-",
        set_protobuf_decode.as_nanos(),
        0
    );
    println!(
        "map,protobuf_decode_ns,{},{},-",
        map_protobuf_decode.as_nanos(),
        0
    );
    println!(
        "set,bincode_decode_ns,{},{},-",
        set_bincode_decode.as_nanos(),
        0
    );
    println!(
        "map,bincode_decode_ns,{},{},-",
        map_bincode_decode.as_nanos(),
        0
    );
    println!("set,native_join_ns,{},{},-", set_join.as_nanos(), 0);
    println!("map,native_join_ns,{},{},-", map_join.as_nanos(), 0);
    println!("set,merkleize_ns,{},{},-", set_merkle.as_nanos(), 0);
    println!("map,merkleize_ns,{},{},-", map_merkle.as_nanos(), 0);
}
