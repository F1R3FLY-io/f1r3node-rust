//! PathMap-native EPathMap time/space experiment.
//!
//! This executable compares the production representation with an explicit
//! list projection used only as a benchmark baseline. It reports set and map
//! specializations separately over shared-prefix keys, including cold/warm
//! EPM1 encoding, indexed lookup, algebra, merkleization, and serialized size.
//! No projection is retained by production code.

use std::hint::black_box;
use std::time::{Duration, Instant};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{EList, EPathMap, Expr, Par};
use models::rust::canonical_path::encode_trie_path;
use models::rust::epathmap_trie_codec::{self, EPathMapRepr};

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

fn main() {
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

    let (set, map, encoded_keys) = fixtures(entries, value_bytes);
    let set_projection = set.entry_trie().entries_owned();
    let mut map_projection = Vec::<(Par, Par)>::with_capacity(entries);
    map.entry_trie()
        .for_each_map_entry(|key, value| map_projection.push((key.clone(), value.clone())))
        .unwrap();

    let set_epm1 = epathmap_trie_codec::encode(set.representation());
    let map_epm1 = epathmap_trie_codec::encode(map.representation());
    let set_projection_bytes = bincode::serialize(&set_projection).unwrap();
    let map_projection_bytes = bincode::serialize(&map_projection).unwrap();

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
    black_box(set.trie_snapshot());
    black_box(map.trie_snapshot());
    let set_warm_snapshot = measure(reps, || {
        black_box(set.trie_snapshot());
    });
    let map_warm_snapshot = measure(reps, || {
        black_box(map.trie_snapshot());
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
        "set,warm_snapshot_accessor_ns,{},{},-",
        set_warm_snapshot.as_nanos(),
        0
    );
    println!(
        "map,warm_snapshot_accessor_ns,{},{},-",
        map_warm_snapshot.as_nanos(),
        0
    );
    println!("set,native_join_ns,{},{},-", set_join.as_nanos(), 0);
    println!("map,native_join_ns,{},{},-", map_join.as_nanos(), 0);
    println!("set,merkleize_ns,{},{},-", set_merkle.as_nanos(), 0);
    println!("map,merkleize_ns,{},{},-", map_merkle.as_nanos(), 0);
}
