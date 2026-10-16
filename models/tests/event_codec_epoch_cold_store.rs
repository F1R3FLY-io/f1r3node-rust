use std::collections::BTreeSet;
use std::io::Write;

use models::rhoapi::cost_signature::Value as CostValue;
use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{
    BindPattern, CostAuthority, CostSignature, CostStack, Expr, ListParWithRandom, Par,
    TaggedContinuation,
};
use models::rust::rholang::bincode_encoder;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::native_source;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{
    channel_hash_for_epoch, BincodeByteSink, EventCodecEpoch, StableHashSerialize,
    COST_CHANNEL_KEY_DOMAIN_V1,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_continuation_for_epoch, decode_datum_for_epoch, decode_joins_for_epoch,
    encode_continuations, encode_continuations_for_epoch, encode_datums_for_epoch, encode_joins,
    encode_joins_for_epoch,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};

fn cost_par() -> Par {
    let mut par = Par::default();
    par.cost_stacks.push(CostStack {
        cells: Vec::new(),
        parallel_tail: Vec::new(),
    });
    par
}

#[test]
fn pre_cost_legacy_channel_bytes_and_hash_remain_the_pinned_fixture() {
    let ordinary = Par::default();
    let old_bytes = vec![0; 81];
    assert_eq!(bincode_encoder::encode_legacy(&ordinary), old_bytes);
    assert_eq!(
        channel_hash_for_epoch(EventCodecEpoch::Legacy, &ordinary).unwrap(),
        Blake2b256Hash::new(&old_bytes),
    );
    assert_eq!(
        channel_hash_for_epoch(EventCodecEpoch::CostV1, &ordinary).unwrap(),
        Blake2b256Hash::new(&old_bytes),
    );
}

#[test]
fn protocol_v6_cost_stack_positional_bytes_and_channel_hash_remain_exact() {
    // Source 37730e1 RhoTypes.proto: CostStack had exactly `cells = 1`.
    // A zero-cell stack was therefore one bincode u64 count, never a second
    // count for the later `parallel_tail = 2` field.
    let stack = CostStack {
        cells: Vec::new(),
        parallel_tail: Vec::new(),
    };
    assert_eq!(bincode_encoder::encode_cost_v1(&stack), vec![0; 8]);
    assert_eq!(bincode_encoder::encode(&stack), vec![0; 16]);

    let channel = cost_par();
    let mut old_bytes = vec![0; 89]; // 81 legacy Par + empty signed-term count.
    old_bytes.extend(1_u64.to_le_bytes()); // one CostStack
    old_bytes.extend([0; 8]); // its empty cells count, protocol v6's last field
    assert_eq!(old_bytes.len(), 105);
    assert_eq!(bincode_encoder::encode_cost_v1(&channel), old_bytes);
    assert_eq!(bincode_encoder::encode(&channel).len(), 113);
    let old_key_preimage =
        bincode::serialize(&vec![COST_CHANNEL_KEY_DOMAIN_V1.to_vec(), old_bytes]).unwrap();
    assert_eq!(
        channel_hash_for_epoch(EventCodecEpoch::CostV1, &channel).unwrap(),
        Blake2b256Hash::new(&old_key_preimage),
    );
    assert_ne!(
        channel_hash_for_epoch(EventCodecEpoch::CostV1, &channel).unwrap(),
        channel_hash_for_epoch(EventCodecEpoch::CostV2, &channel).unwrap(),
    );
}

#[derive(Default)]
struct RecordingSink {
    bytes: Vec<u8>,
    chunks: Vec<usize>,
    reject_at: Option<usize>,
}

impl BincodeByteSink for RecordingSink {
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), RSpaceError> {
        if self.reject_at == Some(self.chunks.len()) {
            return Err(RSpaceError::HostWorkRejected);
        }
        self.chunks.push(bytes.len());
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}

#[test]
fn generated_metered_sink_matches_both_positional_layouts_and_refuses_before_write() {
    let ordinary = Par::default();
    let cost = cost_par();
    let datum = ListParWithRandom {
        pars: vec![cost.clone()],
        ..ListParWithRandom::default()
    };
    for (legacy, value, expected) in [
        (true, &ordinary, bincode_encoder::encode_legacy(&ordinary)),
        (false, &ordinary, bincode_encoder::encode(&ordinary)),
        (false, &cost, bincode_encoder::encode(&cost)),
    ] {
        let mut sink = RecordingSink::default();
        bincode_encoder::encode_metered_into(value, legacy, &mut sink).unwrap();
        assert_eq!(sink.bytes, expected);
        assert!(sink.chunks.len() > 1);
        for reject_at in 0..sink.chunks.len() {
            let mut rejected = RecordingSink {
                reject_at: Some(reject_at),
                ..RecordingSink::default()
            };
            assert!(matches!(
                bincode_encoder::encode_metered_into(value, legacy, &mut rejected),
                Err(RSpaceError::HostWorkRejected)
            ));
            assert_eq!(
                rejected.bytes.len(),
                sink.chunks[..reject_at].iter().sum::<usize>()
            );
        }
    }
    let mut sink = RecordingSink::default();
    bincode_encoder::encode_metered_into(&datum, false, &mut sink).unwrap();
    assert_eq!(sink.bytes, bincode_encoder::encode(&datum));

    let mut sink = RecordingSink::default();
    bincode_encoder::encode_cost_v1_metered_into(&datum, &mut sink).unwrap();
    assert_eq!(sink.bytes, bincode_encoder::encode_cost_v1(&datum));
}

#[test]
fn cost_v1_refuses_branch_topology_and_cost_v2_round_trips_it() {
    let mut branched = Par::default();
    branched.cost_stacks.push(CostStack {
        cells: Vec::new(),
        parallel_tail: vec![CostStack::default(), CostStack::default()],
    });
    let datum = ListParWithRandom {
        pars: vec![branched.clone()],
        ..ListParWithRandom::default()
    };
    let meter = |_: usize, _: usize, _: usize| Ok(());
    assert!(channel_hash_for_epoch(EventCodecEpoch::CostV1, &branched).is_err());
    assert!(native_source::encode_for_epoch(EventCodecEpoch::CostV1, &datum, &meter).is_err());
    assert!(Datum::create_for_epoch(
        EventCodecEpoch::CostV1,
        &Par::default(),
        datum.clone(),
        false,
    )
    .is_err());

    let (bytes, source) = encoded_datum(EventCodecEpoch::CostV2, &Par::default(), &datum);
    let decoded = decode_datum_for_epoch::<ListParWithRandom>(EventCodecEpoch::CostV2, &bytes)
        .expect("CostV2 branched datum");
    assert_eq!(decoded.source, source);
    assert_eq!(decoded.a.pars[0].cost_stacks[0].parallel_tail.len(), 2);
    assert_eq!(
        bincode_encoder::encode(decoded.a.as_ref()),
        bincode_encoder::encode(&datum)
    );
    // Positional bincode has no per-record epoch tag. The caller supplies the
    // authenticated replay/activation epoch; attempting the wrong decoder
    // need not fail merely from byte shape (and must never select the epoch).
}

fn quoted_stack_par(inner: Par) -> Par {
    let mut par = Par::default();
    par.cost_stacks = vec![CostStack {
        cells: vec![CostSignature {
            value: Some(CostValue::Quote(inner)),
        }],
        parallel_tail: Vec::new(),
    }];
    par
}

#[test]
fn cost_v1_validation_is_transitive_through_deep_signature_quotes() {
    let meter = |_: usize, _: usize, _: usize| Ok(());
    for depth in [0, 1, 16, 256, 4096] {
        let mut linear = Par::default();
        let mut branched = Par::default();
        branched.cost_stacks = vec![CostStack {
            cells: Vec::new(),
            parallel_tail: vec![CostStack::default()],
        }];
        for _ in 0..depth {
            linear = quoted_stack_par(linear);
            branched = quoted_stack_par(branched);
        }
        linear.stable_hash_validate_cost_v1().unwrap();
        assert!(branched.stable_hash_validate_cost_v1().is_err());
        assert!(branched
            .stable_hash_validate_cost_v1_metered(&meter)
            .is_err());

        // A datum's direct CostStack is outside its `pars` vector. Its cell
        // quotes must therefore be checked by the same transitive rule.
        let direct = ListParWithRandom {
            cost_stack: Some(CostStack {
                cells: vec![CostSignature {
                    value: Some(CostValue::Quote(branched)),
                }],
                parallel_tail: Vec::new(),
            }),
            ..ListParWithRandom::default()
        };
        assert!(direct.stable_hash_validate_cost_v1().is_err());
        assert!(direct.stable_hash_validate_cost_v1_metered(&meter).is_err());
    }
}

#[test]
fn native_source_uses_same_epoch_bytes_and_event_identity_for_model_roots() {
    let meter = |_: usize, _: usize, _: usize| Ok(());
    let ordinary = Par::default();
    let cost = cost_par();
    let datum = ListParWithRandom {
        pars: vec![cost.clone()],
        ..ListParWithRandom::default()
    };
    assert_eq!(
        native_source::encode_for_epoch(EventCodecEpoch::Legacy, &ordinary, &meter).unwrap(),
        bincode_encoder::encode_legacy(&ordinary)
    );
    assert_eq!(
        native_source::encode_for_epoch(EventCodecEpoch::CostV1, &datum, &meter).unwrap(),
        bincode_encoder::encode_cost_v1(&datum)
    );
    let native =
        native_source::produce_for_epoch(EventCodecEpoch::CostV1, &cost, &datum, false, &meter)
            .unwrap();
    let value = Produce::create_for_epoch(EventCodecEpoch::CostV1, &cost, &datum, false).unwrap();
    assert_eq!(native, value);
    assert!(native_source::produce_for_epoch(
        EventCodecEpoch::Legacy,
        &cost,
        &datum,
        false,
        &meter,
    )
    .is_err());
}

#[test]
fn ordinary_legacy_identity_and_current_meter_chunks_match_their_respective_oracles() {
    let channel = Par::default().with_exprs(vec![
        Expr {
            expr_instance: Some(ExprInstance::GByteArray(vec![1, 2, 3, 4, 5])),
        },
        Expr {
            expr_instance: Some(ExprInstance::GString(String::new())),
        },
    ]);
    let datum = ListParWithRandom {
        pars: vec![Par::default()],
        ..ListParWithRandom::default()
    };
    let meter = |_: usize, _: usize, _: usize| Ok(());
    let routed =
        native_source::produce_for_epoch(EventCodecEpoch::Legacy, &channel, &datum, false, &meter)
            .unwrap();
    assert_eq!(
        routed,
        Produce::create_for_epoch(EventCodecEpoch::Legacy, &channel, &datum, false).unwrap()
    );

    #[derive(Default)]
    struct SerdeChunks(Vec<usize>);
    impl Write for SerdeChunks {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.push(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    let mut serde = SerdeChunks::default();
    bincode::serialize_into(&mut serde, &channel).unwrap();
    let mut generated = RecordingSink::default();
    bincode_encoder::encode_metered_into(&channel, false, &mut generated).unwrap();
    assert_eq!(generated.chunks, serde.0);

    let mut serde = SerdeChunks::default();
    bincode::serialize_into(&mut serde, &datum).unwrap();
    let mut generated = RecordingSink::default();
    bincode_encoder::encode_metered_into(&datum, false, &mut generated).unwrap();
    assert_eq!(generated.chunks, serde.0);
}

fn encoded_datum(
    epoch: EventCodecEpoch,
    channel: &Par,
    datum: &ListParWithRandom,
) -> (Vec<u8>, Produce) {
    let source = Produce::create_for_epoch(epoch, channel, datum, false).unwrap();
    let mut bytes = match epoch {
        EventCodecEpoch::Legacy => bincode_encoder::encode_legacy(datum),
        EventCodecEpoch::CostV1 => bincode_encoder::encode_cost_v1(datum),
        EventCodecEpoch::CostV2 => bincode_encoder::encode(datum),
    };
    bytes.extend(bincode::serialize(&(false, &source)).unwrap());
    (bytes, source)
}

#[test]
fn legacy_datum_decoder_preserves_old_positional_bytes_and_rejects_truncation() {
    let channel = Par::default();
    let datum = ListParWithRandom {
        pars: vec![Par::default()],
        ..ListParWithRandom::default()
    };
    let (bytes, source) = encoded_datum(EventCodecEpoch::Legacy, &channel, &datum);
    let stored =
        Datum::create_for_epoch(EventCodecEpoch::Legacy, &channel, datum.clone(), false).unwrap();
    assert_eq!(
        encode_datums_for_epoch(EventCodecEpoch::Legacy, &[stored]).unwrap(),
        bincode::serialize(&vec![bytes.clone()]).unwrap(),
    );
    let decoded = decode_datum_for_epoch::<ListParWithRandom>(EventCodecEpoch::Legacy, &bytes)
        .expect("legacy datum");
    assert_eq!(decoded.source, source);
    assert_eq!(
        bincode_encoder::encode_legacy(decoded.a.as_ref()),
        bincode_encoder::encode_legacy(&datum)
    );
    for end in 0..bytes.len() {
        assert!(
            decode_datum_for_epoch::<ListParWithRandom>(EventCodecEpoch::Legacy, &bytes[..end])
                .is_err(),
            "truncated legacy datum at byte {end}"
        );
    }
}

#[test]
fn cost_v1_datum_decoder_retains_nested_cost_fields() {
    let channel = Par::default();
    let datum = ListParWithRandom {
        pars: vec![cost_par()],
        ..ListParWithRandom::default()
    };
    let (bytes, source) = encoded_datum(EventCodecEpoch::CostV1, &channel, &datum);
    let stored =
        Datum::create_for_epoch(EventCodecEpoch::CostV1, &channel, datum.clone(), false).unwrap();
    assert_eq!(
        encode_datums_for_epoch(EventCodecEpoch::CostV1, &[stored]).unwrap(),
        bincode::serialize(&vec![bytes.clone()]).unwrap(),
    );
    let decoded = decode_datum_for_epoch::<ListParWithRandom>(EventCodecEpoch::CostV1, &bytes)
        .expect("cost datum");
    assert_eq!(decoded.source, source);
    assert_eq!(
        bincode_encoder::encode_cost_v1(decoded.a.as_ref()),
        bincode_encoder::encode_cost_v1(&datum)
    );
}

#[test]
fn continuation_and_join_prefixes_select_the_authenticated_layout() {
    let channel = Par::default();
    let channels = vec![channel.clone()];
    let patterns = vec![BindPattern::default()];
    for (epoch, continuation) in [
        (EventCodecEpoch::Legacy, TaggedContinuation::default()),
        (EventCodecEpoch::CostV1, TaggedContinuation {
            cost_authority: Some(CostAuthority::default()),
            ..TaggedContinuation::default()
        }),
    ] {
        let source =
            Consume::create_for_epoch(epoch, &channels, &patterns, &continuation, false).unwrap();
        let mut bytes = (patterns.len() as u64).to_le_bytes().to_vec();
        for pattern in &patterns {
            bytes.extend(match epoch {
                EventCodecEpoch::Legacy => bincode_encoder::encode_legacy(pattern),
                EventCodecEpoch::CostV1 => bincode_encoder::encode_cost_v1(pattern),
                EventCodecEpoch::CostV2 => bincode_encoder::encode(pattern),
            });
        }
        bytes.extend(match epoch {
            EventCodecEpoch::Legacy => bincode_encoder::encode_legacy(&continuation),
            EventCodecEpoch::CostV1 => bincode_encoder::encode_cost_v1(&continuation),
            EventCodecEpoch::CostV2 => bincode_encoder::encode(&continuation),
        });
        bytes.extend(bincode::serialize(&(false, BTreeSet::<i32>::new(), &source)).unwrap());
        let stored = WaitingContinuation::create_for_epoch(
            epoch,
            &channels,
            &patterns,
            &continuation,
            false,
            BTreeSet::new(),
        )
        .unwrap();
        let leaf = encode_continuations_for_epoch(epoch, &[stored.clone()]).unwrap();
        assert_eq!(leaf, bincode::serialize(&vec![bytes.clone()]).unwrap());
        if epoch == EventCodecEpoch::CostV1 {
            assert_eq!(leaf, encode_continuations(&vec![stored]));
        }
        let decoded =
            decode_continuation_for_epoch::<BindPattern, TaggedContinuation>(epoch, &bytes)
                .unwrap();
        assert_eq!(decoded.source, source);
        assert_eq!(decoded.patterns.len(), patterns.len());
        let actual = match epoch {
            EventCodecEpoch::Legacy => {
                bincode_encoder::encode_legacy(decoded.continuation.as_ref())
            }
            EventCodecEpoch::CostV1 => {
                bincode_encoder::encode_cost_v1(decoded.continuation.as_ref())
            }
            EventCodecEpoch::CostV2 => bincode_encoder::encode(decoded.continuation.as_ref()),
        };
        let expected = match epoch {
            EventCodecEpoch::Legacy => bincode_encoder::encode_legacy(&continuation),
            EventCodecEpoch::CostV1 => bincode_encoder::encode_cost_v1(&continuation),
            EventCodecEpoch::CostV2 => bincode_encoder::encode(&continuation),
        };
        assert_eq!(actual, expected);
    }

    let mut encoded_join = 1_u64.to_le_bytes().to_vec();
    encoded_join.extend(bincode_encoder::encode_legacy(&channel));
    let outer = bincode::serialize(&vec![encoded_join]).unwrap();
    assert_eq!(
        encode_joins_for_epoch(EventCodecEpoch::Legacy, &[vec![channel.clone()]]).unwrap(),
        outer,
    );
    assert_eq!(
        encode_joins_for_epoch(EventCodecEpoch::CostV1, &[vec![channel.clone()]]).unwrap(),
        encode_joins(&vec![vec![channel.clone()]]),
    );
    let joins = decode_joins_for_epoch::<Par>(EventCodecEpoch::Legacy, &outer).unwrap();
    assert_eq!(joins.len(), 1);
    assert_eq!(joins[0].len(), 1);
    assert_eq!(
        bincode_encoder::encode_legacy(&joins[0][0]),
        bincode_encoder::encode_legacy(&channel)
    );
}
