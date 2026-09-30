use std::cell::Cell;

#[allow(dead_code, clippy::all)]
#[rustfmt::skip]
#[path = "protobuf_decoder_frozen.rs"]
pub(crate) mod frozen;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Routing {
    legacy: bool,
    poison: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Counts {
    frozen: usize,
    successor: usize,
}

thread_local! {
    static ROUTING: Cell<Routing> = const { Cell::new(Routing { legacy: false, poison: false }) };
    static COUNTS: Cell<Counts> = const { Cell::new(Counts { frozen: 0, successor: 0 }) };
}

struct Restore(Routing);

impl Drop for Restore {
    fn drop(&mut self) { ROUTING.set(self.0); }
}

fn scoped<T>(update: impl FnOnce(&mut Routing), run: impl FnOnce() -> T) -> T {
    let mut next = ROUTING.get();
    update(&mut next);
    let _restore = Restore(ROUTING.replace(next));
    run()
}

pub(crate) fn legacy<T>(run: impl FnOnce() -> T) -> T { scoped(|state| state.legacy = true, run) }

fn poisoned<T>(run: impl FnOnce() -> T) -> T { scoped(|state| state.poison = true, run) }

pub(crate) fn enter_decoder() -> bool {
    let routing = ROUTING.get();
    let mut counts = COUNTS.get();
    if routing.legacy {
        counts.frozen += 1;
    } else {
        counts.successor += 1;
    }
    COUNTS.set(counts);
    assert!(
        routing.legacy || !routing.poison,
        "successor decoder reached under oracle poison"
    );
    routing.legacy
}

pub(crate) fn assert_successor_body() {
    let state = ROUTING.get();
    assert!(
        !state.legacy && !state.poison,
        "successor body bypassed decoder routing"
    );
}

macro_rules! route {
    ($entry:ident, $buf:ident) => {
        if crate::protobuf_decoder_oracle::enter_decoder() {
            let mut erased = $buf;
            return crate::protobuf_decoder_oracle::frozen::$entry(
                &mut erased as &mut dyn prost::bytes::Buf,
            );
        }
    };
}

pub(crate) use route;

#[cfg(feature = "protobuf-projection-experiment")]
#[path = "protobuf_decoder_measurement.rs"]
mod measurement;

pub(crate) fn enot_bytes(depth: usize, terminal: &[u8]) -> Vec<u8> {
    let mut lengths = vec![terminal.len()];
    for _ in 0..depth {
        let child = *lengths.last().unwrap();
        let not = 1 + prost::encoding::encoded_len_varint(child as u64) + child;
        let expr = 1 + prost::encoding::encoded_len_varint(not as u64) + not;
        lengths.push(1 + prost::encoding::encoded_len_varint(expr as u64) + expr);
    }
    let mut bytes = Vec::with_capacity(*lengths.last().unwrap());
    for &child in lengths[..depth].iter().rev() {
        let not = 1 + prost::encoding::encoded_len_varint(child as u64) + child;
        let expr = 1 + prost::encoding::encoded_len_varint(not as u64) + not;
        for (tag, length) in [(0x2a, expr), (0x2a, not), (0x0a, child)] {
            bytes.push(tag);
            prost::encoding::encode_varint(length as u64, &mut bytes);
        }
    }
    bytes.extend_from_slice(terminal);
    bytes
}

#[path = "../par_corpus/mod.rs"]
mod corpus;

use proptest::prelude::*;
use prost::bytes::{Buf, Bytes};
use prost::{DecodeError, Message};

use crate::rhoapi::{EPathMap, Par};
use crate::rust::rholang::{protobuf_decoder as decoder, schema_test_adapters};

fn observed_root(mut buf: impl Buf) -> (Result<(Vec<u8>, Vec<u8>), DecodeError>, usize) {
    let decoded = decoder::decode_par(&mut buf)
        .map(|value| (value.encode_to_vec(), value.components().compact_bytes()));
    (decoded, buf.remaining())
}

fn observed_merge(
    mut value: Par,
    mut buf: impl Buf,
    delimited: bool,
) -> (Result<(), DecodeError>, usize, Vec<u8>, Vec<u8>) {
    let result = if delimited {
        value.merge_length_delimited(&mut buf)
    } else {
        value.merge(&mut buf)
    };
    (
        result,
        buf.remaining(),
        value.encode_to_vec(),
        value.components().compact_bytes(),
    )
}

fn assert_root_and_merge(bytes: &[u8]) {
    for split in [0, bytes.len() / 2, bytes.len()] {
        let source = || {
            Bytes::copy_from_slice(&bytes[..split]).chain(Bytes::copy_from_slice(&bytes[split..]))
        };
        assert_eq!(legacy(|| observed_root(source())), observed_root(source()));
        let message_root = || {
            let mut input = source();
            let result = Par::decode(&mut input)
                .map(|value| (value.encode_to_vec(), value.components().compact_bytes()));
            (result, input.remaining())
        };
        assert_eq!(legacy(message_root), message_root());
        for delimited in [false, true] {
            assert_eq!(
                legacy(|| observed_merge(corpus::gint(71), source(), delimited)),
                observed_merge(corpus::gint(71), source(), delimited)
            );
        }
    }
}

#[test]
fn routing_scope_restores_nested_state_and_unwind() {
    let initial = ROUTING.get();
    poisoned(|| {
        assert!(ROUTING.get().poison);
        legacy(|| {
            let outer = ROUTING.get();
            legacy(|| assert_eq!(ROUTING.get(), outer));
            assert_eq!(ROUTING.get(), outer);
        });
        assert!(!ROUTING.get().legacy);
    });
    assert_eq!(ROUTING.get(), initial);
    assert_decoder_oracle_unwind_child("routing", "scope unwind", b"inner:outer");
    assert_eq!(ROUTING.get(), initial);
    legacy(|| {
        std::thread::spawn(|| assert_eq!(ROUTING.get(), Routing::default()))
            .join()
            .unwrap()
    });
}

#[test]
fn successor_poison_is_live_and_legacy_route_bypasses_it() {
    COUNTS.set(Counts::default());
    let bytes = corpus::all_par_fields().encode_to_vec();
    poisoned(|| {
        legacy(|| {
            assert!(observed_root(bytes.as_slice()).0.is_ok());
            assert!(observed_merge(Par::default(), bytes.as_slice(), false)
                .0
                .is_ok());
        })
    });
    let counts = COUNTS.get();
    assert!(counts.frozen > 1);
    assert_eq!(counts.successor, 0);
    assert_decoder_oracle_unwind_child(
        "poison",
        "successor decoder reached under oracle poison",
        b"successor-restored",
    );
    COUNTS.set(Counts::default());
    assert!(observed_root(bytes.as_slice()).0.is_ok());
    assert_eq!(COUNTS.get().frozen, 0);
    assert!(COUNTS.get().successor > 0);
}

fn assert_decoder_oracle_unwind_child(case: &str, diagnostic: &str, expected_marker: &[u8]) {
    let scratch = shared::rust::test_scratch::acquire("decoder-oracle-unwind-");
    let marker = scratch.path().join("restored");
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "protobuf_decoder_oracle::decoder_oracle_unwind_child",
            "--nocapture",
        ])
        .env("DECODER_ORACLE_UNWIND_CASE", case)
        .env("DECODER_ORACLE_UNWIND_MARKER", &marker)
        .env_remove(shared::rust::test_process::ALLOW_EXPECTED_PROBE_CORE);
    let outcome = shared::rust::test_process::run_child_probe(
        &mut command,
        std::time::Duration::from_secs(30),
    )
    .expect("run decoder oracle unwind child");
    match outcome {
        shared::rust::test_process::ProbeOutcome::Failed(failure) => {
            assert_eq!(
                failure.kind,
                shared::rust::test_process::ProbeFailureKind::Panicked
            );
            assert!(failure.stderr.contains(diagnostic), "{failure:?}");
        }
        shared::rust::test_process::ProbeOutcome::Passed => {
            panic!("decoder oracle unwind child did not fail")
        }
    }
    assert_eq!(std::fs::read(marker).unwrap(), expected_marker);
}

#[test]
#[ignore = "isolated decoder oracle unwind child"]
fn decoder_oracle_unwind_child() {
    let Some(case) = std::env::var_os("DECODER_ORACLE_UNWIND_CASE") else {
        return;
    };
    let marker =
        std::path::PathBuf::from(std::env::var_os("DECODER_ORACLE_UNWIND_MARKER").unwrap());
    match case.to_str().unwrap() {
        "routing" => {
            struct VerifyOuter(std::path::PathBuf);
            impl Drop for VerifyOuter {
                fn drop(&mut self) {
                    assert_eq!(ROUTING.get(), Routing::default());
                    assert_eq!(std::fs::read(&self.0).unwrap(), b"inner");
                    std::fs::write(&self.0, b"inner:outer").unwrap();
                }
            }
            struct VerifyInner(std::path::PathBuf);
            impl Drop for VerifyInner {
                fn drop(&mut self) {
                    assert_eq!(ROUTING.get(), Routing {
                        legacy: true,
                        poison: false
                    });
                    std::fs::write(&self.0, b"inner").unwrap();
                }
            }
            let _outer = VerifyOuter(marker.clone());
            legacy(|| {
                let _inner = VerifyInner(marker);
                poisoned(|| panic!("scope unwind"));
            });
        }
        "poison" => {
            struct VerifyPoison(std::path::PathBuf);
            impl Drop for VerifyPoison {
                fn drop(&mut self) {
                    assert_eq!(ROUTING.get(), Routing::default());
                    assert_eq!(COUNTS.get().frozen, 0);
                    assert!(COUNTS.get().successor > 0);
                    std::fs::write(&self.0, b"successor-restored").unwrap();
                }
            }
            COUNTS.set(Counts::default());
            let bytes = corpus::all_par_fields().encode_to_vec();
            let _verify = VerifyPoison(marker);
            let _ = poisoned(|| decoder::decode_par(bytes.as_slice()));
        }
        other => panic!("unknown decoder oracle unwind case: {other}"),
    }
}

#[test]
fn descriptor_corpus_routes_all_generated_decoders_independently() {
    let fixtures = schema_test_adapters::schema_fixtures();
    assert!(fixtures.len() > 100);
    for fixture in fixtures {
        let bytes = fixture.bytes.as_slice();
        let old = poisoned(|| legacy(|| (fixture.machine_round_trip)(bytes)));
        let new = (fixture.machine_round_trip)(bytes);
        assert_eq!(old, new, "{}", fixture.fixture_name);
    }
}

#[test]
fn root_and_partial_merge_observations_match_frozen_reference() {
    for (_, value) in corpus::par_corpus() {
        let bytes = value.encode_to_vec();
        assert_root_and_merge(&bytes);
        let delimited = value.encode_length_delimited_to_vec();
        assert_root_and_merge(&delimited);
        for end in 0..bytes.len() {
            assert_eq!(
                legacy(|| observed_merge(corpus::gint(71), &bytes[..end], false)),
                observed_merge(corpus::gint(71), &bytes[..end], false)
            );
        }
    }
}

#[test]
fn late_failure_preserves_committed_fields_and_exact_error() {
    let mut bytes = corpus::gint(99).encode_to_vec();
    bytes.extend_from_slice(&[0x4a, 2, 1, 1, 0x52, 0xff]);
    let old = legacy(|| observed_merge(corpus::gint(71), bytes.as_slice(), false));
    let new = observed_merge(corpus::gint(71), bytes.as_slice(), false);
    assert_eq!(old, new);
    assert!(old.0.is_err());
    assert_ne!(old.2, corpus::gint(71).encode_to_vec());
}

#[test]
fn epathmap_adapter_and_indirect_value_decoders_are_isolated() {
    let nested = Par::default().with_sends(vec![crate::rhoapi::Send {
        chan: Some(corpus::gint(3)),
        data: vec![corpus::gint(4)],
        ..Default::default()
    }]);
    let map = EPathMap::new_map([(nested.clone(), nested.clone())], vec![], false, None);
    let mut fixtures = vec![
        corpus::ground_pathmap().encode_to_vec(),
        corpus::nonground_pathmap().encode_to_vec(),
        map.encode_to_vec(),
        Vec::new(),
    ];
    let mut legacy_entry = Vec::new();
    prost::encoding::bytes::encode(1, &nested.encode_to_vec(), &mut legacy_entry);
    let key = crate::rust::canonical_path::encode_trie_path(&nested);
    let mut paths = u32::try_from(key.len()).unwrap().to_le_bytes().to_vec();
    paths.extend_from_slice(&key);
    let mut legacy_paths = Vec::new();
    prost::encoding::bytes::encode(8, &paths, &mut legacy_paths);
    fixtures.push(legacy_entry.clone());
    fixtures.push(legacy_paths.clone());
    for legacy_field in [&legacy_entry, &legacy_paths] {
        let mut conflict = map.encode_to_vec();
        conflict.extend_from_slice(legacy_field);
        assert!(EPathMap::decode(conflict.as_slice()).is_err());
        fixtures.push(conflict);
    }
    for bytes in fixtures {
        for end in 0..=bytes.len() {
            let observe = || {
                let mut value = EPathMap::default();
                let mut input = &bytes[..end];
                let result = value.merge(&mut input);
                (result, input.remaining(), value.encode_to_vec())
            };
            assert_eq!(poisoned(|| legacy(observe)), observe());
            let generated = || {
                let mut input = &bytes[..end];
                let result =
                    decoder::decode_epath_map(&mut input).map(|value| value.encode_to_vec());
                (result, input.remaining())
            };
            assert_eq!(poisoned(|| legacy(generated)), generated());
        }
    }
}

#[test]
fn canonical_fast_import_retains_baseline_borrowed_input_observation() {
    let bytes = corpus::gint(5).encode_to_vec();
    let old = legacy(|| observed_root(bytes.as_slice()));
    assert!(old.0.is_ok());
    assert_eq!(old.1, bytes.len());
    assert_eq!(old, observed_root(bytes.as_slice()));
    let mut message_input = bytes.as_slice();
    assert!(legacy(|| Par::decode(&mut message_input)).is_ok());
    assert_eq!(message_input.remaining(), 0);
}

#[test]
fn fixed_numeric_root_stream_matches_fragmented_frozen_reference() {
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::Expr;

    let values = [
        ExprInstance::GBool(false),
        ExprInstance::GInt(i64::MIN),
        ExprInstance::GDouble(0x8000_0000_0000_0000),
        ExprInstance::GInt(0),
        ExprInstance::GBool(true),
        ExprInstance::GDouble(0x7ff8_0000_0000_0001),
        ExprInstance::GInt(i64::MAX),
        ExprInstance::GDouble(0),
        ExprInstance::GInt(-1),
    ];
    let mut bytes = Vec::new();
    for value in values {
        prost::encoding::bytes::encode(
            5,
            &Expr {
                expr_instance: Some(value),
            }
            .encode_to_vec(),
            &mut bytes,
        );
    }
    let duplicate = Expr {
        expr_instance: Some(ExprInstance::GInt(-1)),
    }
    .encode_to_vec();
    for _ in 0..258 {
        prost::encoding::bytes::encode(5, &duplicate, &mut bytes);
    }
    let mut malformed = bytes.clone();
    malformed.extend_from_slice(&[0x2a, 0x03, 0x10]);
    for input in [&bytes[..], &malformed[..], &bytes[..bytes.len() - 1]] {
        for split in [1, input.len() / 2, input.len() - 1] {
            let source = || {
                Bytes::copy_from_slice(&input[..split])
                    .chain(Bytes::copy_from_slice(&input[split..]))
            };
            assert_eq!(legacy(|| observed_root(source())), observed_root(source()));
        }
    }
}

proptest! {
    #[test]
    fn arbitrary_buffers_preserve_root_merge_and_consumption(bytes in prop::collection::vec(any::<u8>(), 0..128)) {
        assert_root_and_merge(&bytes);
    }
}

#[test]
fn repeated_fields_oneof_replacement_and_groups_preserve_observations() {
    use prost::encoding::{bytes, encode_key, WireType};

    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{Expr, New};

    let field = |tag, body: Vec<u8>| {
        let mut out = Vec::new();
        bytes::encode(tag, &body, &mut out);
        out
    };
    let first = Expr {
        expr_instance: Some(ExprInstance::GInt(17)),
    };
    let second = Expr {
        expr_instance: Some(ExprInstance::GString("replacement".into())),
    };
    let mut repeated_oneof = first.encode_to_vec();
    repeated_oneof.extend(second.encode_to_vec());
    let mut repeated_same_arm = first.encode_to_vec();
    repeated_same_arm.extend(first.encode_to_vec());
    let mut send = field(1, corpus::gint(1).encode_to_vec());
    send.extend(field(1, corpus::gint(2).encode_to_vec()));
    let first_new = New {
        injections: [("same".into(), corpus::gint(1))].into(),
        ..Default::default()
    };
    let second_new = New {
        injections: [("same".into(), corpus::gint(2))].into(),
        ..Default::default()
    };
    let mut repeated_map_key = first_new.encode_to_vec();
    repeated_map_key.extend(second_new.encode_to_vec());
    let mut metadata = field(9, vec![1, 2]);
    metadata.extend(field(9, Vec::new()));
    metadata.extend([0x50, 1, 0x50, 0]);
    let mut group = Vec::new();
    encode_key(99, WireType::StartGroup, &mut group);
    encode_key(100, WireType::StartGroup, &mut group);
    encode_key(100, WireType::EndGroup, &mut group);
    encode_key(99, WireType::EndGroup, &mut group);
    for body in [
        field(5, repeated_oneof),
        field(5, repeated_same_arm),
        field(1, send),
        field(4, repeated_map_key),
        metadata,
        group,
        field(1, vec![]),
        field(6, vec![]),
        field(12, vec![]),
    ] {
        for end in 0..=body.len() {
            assert_root_and_merge(&body[..end]);
        }
    }
}

fn attached_component<T: crate::rust::par_bag::ParBagComponent>(tag: u32, body: &[u8]) -> T {
    let mut bytes = Vec::new();
    prost::encoding::bytes::encode(tag, &body.to_vec(), &mut bytes);
    let old = poisoned(|| legacy(|| decoder::decode_par(bytes.as_slice()))).unwrap();
    let new = decoder::decode_par(bytes.as_slice()).unwrap();
    assert_eq!(old.encode_to_vec(), new.encode_to_vec());
    new.first::<T>().unwrap().unwrap().0
}

#[test]
fn public_new_keeps_uri_order_until_attachment_to_par() {
    use crate::rhoapi::New;

    let source = New {
        p: Some(Par::default()),
        uri: vec!["z".into(), "a".into(), "z".into()],
        ..Default::default()
    };
    let bytes = source.encode_to_vec();
    let raw = decoder::decode_new(bytes.as_slice()).unwrap();
    let reference = poisoned(|| legacy(|| decoder::decode_new(bytes.as_slice()))).unwrap();
    assert_eq!(raw.encode_to_vec(), reference.encode_to_vec());
    assert_eq!(raw.uri, source.uri);
    let canonical = attached_component::<New>(4, &bytes);
    assert_eq!(canonical.uri, ["a", "z", "z"]);
    assert_ne!(raw.encode_to_vec(), canonical.encode_to_vec());
}

#[test]
fn public_collection_expr_keeps_order_and_duplicates_until_attachment_to_par() {
    use crate::rhoapi::expr::ExprInstance;
    use crate::rhoapi::{EMap, ESet, Expr, KeyValuePair};

    let sources = [
        Expr {
            expr_instance: Some(ExprInstance::ESetBody(ESet {
                ps: vec![corpus::gint(2), corpus::gint(1), corpus::gint(2)],
                ..Default::default()
            })),
        },
        Expr {
            expr_instance: Some(ExprInstance::EMapBody(EMap {
                kvs: vec![
                    KeyValuePair {
                        key: Some(corpus::gint(2)),
                        value: Some(corpus::gint(20)),
                    },
                    KeyValuePair {
                        key: Some(corpus::gint(1)),
                        value: Some(corpus::gint(10)),
                    },
                ],
                ..Default::default()
            })),
        },
    ];
    for source in sources {
        let bytes = source.encode_to_vec();
        let raw = decoder::decode_expr(bytes.as_slice()).unwrap();
        let reference = poisoned(|| legacy(|| decoder::decode_expr(bytes.as_slice()))).unwrap();
        assert_eq!(raw.encode_to_vec(), reference.encode_to_vec());
        assert_eq!(raw.encode_to_vec(), bytes);
        let canonical = attached_component::<Expr>(5, &bytes);
        assert_ne!(raw.encode_to_vec(), canonical.encode_to_vec());
        match canonical.expr_instance.unwrap() {
            ExprInstance::ESetBody(set) => {
                assert_eq!(set.ps, [corpus::gint(1), corpus::gint(2)]);
            }
            ExprInstance::EMapBody(map) => {
                assert_eq!(map.kvs[0].key, Some(corpus::gint(1)));
                assert_eq!(map.kvs[1].key, Some(corpus::gint(2)));
            }
            _ => panic!("collection constructor changed"),
        }
    }
}

#[test]
fn public_receive_keeps_explicit_empty_condition_until_attachment_to_par() {
    use crate::rhoapi::Receive;

    let source = Receive {
        body: Some(Par::default()),
        condition: Some(Par::default()),
        ..Default::default()
    };
    let bytes = source.encode_to_vec();
    let raw = decoder::decode_receive(bytes.as_slice()).unwrap();
    let reference = poisoned(|| legacy(|| decoder::decode_receive(bytes.as_slice()))).unwrap();
    assert_eq!(raw.encode_to_vec(), reference.encode_to_vec());
    assert!(raw.condition.is_some());
    assert!(attached_component::<Receive>(2, &bytes).condition.is_none());
}

#[test]
fn public_match_keeps_explicit_empty_guard_until_attachment_to_par() {
    use crate::rhoapi::{Match, MatchCase};

    let source = Match {
        target: Some(corpus::gint(3)),
        cases: vec![MatchCase {
            pattern: Some(corpus::gint(3)),
            source: Some(corpus::gint(4)),
            guard: Some(Par::default()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let bytes = source.encode_to_vec();
    let raw = decoder::decode_match(bytes.as_slice()).unwrap();
    let reference = poisoned(|| legacy(|| decoder::decode_match(bytes.as_slice()))).unwrap();
    assert_eq!(raw.encode_to_vec(), reference.encode_to_vec());
    assert!(raw.cases[0].guard.is_some());
    assert!(attached_component::<Match>(6, &bytes).cases[0]
        .guard
        .is_none());
}
