use super::support::{encode, synthetic_ref};
use crate::rspace::persistence::{
    CanonicalEncode, CanonicalFormatError, CanonicalSink, MutationBatchViewV1,
    MutationDecodeLimits, MutationError, ObjectDigest, RSpaceMutationBatchV1, StateVersionV1,
};
use crate::rspace::rho_path::RhoPathV1;

fn state(generation: u64) -> StateVersionV1 { StateVersionV1::new(generation, synthetic_ref(90)) }

fn datum(identity: &[u8]) -> Vec<u8> { RhoPathV1::datum_key(&7_u64, b"shape", identity) }

fn continuation(identity: &[u8]) -> Vec<u8> {
    let prefix = RhoPathV1::continuation_prefix(&[7_u64, 8_u64]);
    RhoPathV1::collection_record_path(&prefix, identity)
}

fn join(record: &[u8]) -> Vec<u8> {
    let mut path = RhoPathV1::join_prefix(&7_u64);
    path.extend_from_slice(record);
    path
}

fn populated_batch() -> RSpaceMutationBatchV1 {
    let mut batch = RSpaceMutationBatchV1::new(state(41)).expect("generation has a successor");
    batch
        .record_data(&datum(b"alpha"), 0, 3)
        .expect("data key is valid");
    batch
        .record_continuation(&continuation(b"receive"), None, Some(synthetic_ref(11)))
        .expect("continuation key is valid");
    batch
        .record_join(&join(b"join-record"), 2, 1)
        .expect("join key is valid");
    batch
}

#[test]
fn normalized_batch_round_trips_all_three_semantic_sections() {
    let batch = populated_batch();
    let plan = batch.plan().expect("nonempty mutation batch plans");
    let bytes = encode(&plan);
    assert_eq!(bytes.len() as u64, plan.canonical_len());
    assert_eq!(&bytes[..16], b"RSpaceDAG.Mut\0\0\0");

    let parsed = RSpaceMutationBatchV1::parse(&bytes).expect("canonical mutation parses");
    assert_eq!(parsed.base(), state(41));
    assert_eq!(parsed.target_generation(), 42);
    assert_eq!(parsed.data_transition(&datum(b"alpha")).unwrap().before(), 0);
    assert_eq!(parsed.data_transition(&datum(b"alpha")).unwrap().after(), 3);
    assert_eq!(
        parsed
            .continuation_transition(&continuation(b"receive"))
            .unwrap()
            .after(),
        Some(synthetic_ref(11))
    );
    assert_eq!(
        parsed
            .join_transition(&join(b"join-record"))
            .unwrap()
            .before(),
        2
    );
    assert_eq!(encode(&parsed.plan().expect("parsed batch replans")), bytes);
}

#[test]
fn zipper_order_makes_insertion_order_irrelevant() {
    let keys = [datum(b"a"), datum(b"b"), datum(b"c")];
    let mut ascending = RSpaceMutationBatchV1::new(state(3)).unwrap();
    let mut descending = RSpaceMutationBatchV1::new(state(3)).unwrap();
    for key in &keys {
        ascending.record_data(key, 0, 1).unwrap();
    }
    for key in keys.iter().rev() {
        descending.record_data(key, 0, 1).unwrap();
    }
    assert_eq!(encode(&ascending.plan().unwrap()), encode(&descending.plan().unwrap()));
}

#[test]
fn sequential_changes_coalesce_to_exact_before_and_final_after() {
    let key = datum(b"coalesce");
    let mut batch = RSpaceMutationBatchV1::new(state(5)).unwrap();
    batch.record_data(&key, 2, 9).unwrap();
    batch.record_data(&key, 9, 4).unwrap();
    assert_eq!(batch.data_transition(&key).unwrap().before(), 2);
    assert_eq!(batch.data_transition(&key).unwrap().after(), 4);

    batch.record_data(&key, 4, 2).unwrap();
    assert!(batch.data_transition(&key).is_none());
    assert!(batch.is_empty());
    assert_eq!(batch.plan().err().unwrap(), MutationError::EmptyBatch);
}

#[test]
fn discontinuous_transitions_fail_without_changing_the_normalized_entry() {
    let key = datum(b"discontinuous");
    let mut batch = RSpaceMutationBatchV1::new(state(5)).unwrap();
    batch.record_data(&key, 2, 9).unwrap();
    assert_eq!(batch.record_data(&key, 8, 4).err().unwrap(), MutationError::DiscontinuousCount {
        expected: 9,
        actual: 8,
    });
    assert_eq!(batch.data_transition(&key).unwrap().after(), 9);

    assert_eq!(batch.plan().err().unwrap(), MutationError::PoisonedBatch);
    assert_eq!(
        batch.record_data(&datum(b"ignored"), 0, 1).err().unwrap(),
        MutationError::PoisonedBatch
    );

    let mut batch = RSpaceMutationBatchV1::new(state(5)).unwrap();
    let continuation = continuation(b"cas");
    batch
        .record_continuation(&continuation, None, Some(synthetic_ref(1)))
        .unwrap();
    assert!(matches!(
        batch.record_continuation(&continuation, Some(synthetic_ref(2)), Some(synthetic_ref(3))),
        Err(MutationError::DiscontinuousContent { .. })
    ));
    assert_eq!(
        batch
            .continuation_transition(&continuation)
            .unwrap()
            .after(),
        Some(synthetic_ref(1))
    );
}

#[test]
fn plan_is_an_immutable_cow_snapshot() {
    let mut batch = populated_batch();
    let plan = batch.plan().unwrap();
    let before = encode(&plan);
    batch.record_data(&datum(b"later"), 0, 10).unwrap();
    assert_eq!(encode(&plan), before);
    assert_ne!(encode(&batch.plan().unwrap()), before);
}

#[test]
fn borrowed_view_iterates_directly_over_the_canonical_payload() {
    let bytes = encode(&populated_batch().plan().unwrap());
    let view = MutationBatchViewV1::parse_with_limits(&bytes, MutationDecodeLimits {
        max_entries: 3,
        max_key_bytes: bytes.len() as u64,
    })
    .expect("bounded borrowed view validates");
    assert_eq!(view.base(), state(41));
    assert_eq!(view.target_generation(), 42);

    let data = view.data().next().expect("one data transition");
    let start = bytes.as_ptr() as usize;
    let end = start + bytes.len();
    let key = data.key.as_ptr() as usize;
    assert!((start..end).contains(&key), "the key must borrow the input image");
    assert_eq!(data.transition.after(), 3);
    assert_eq!(view.continuations().len(), 1);
    assert_eq!(view.joins().len(), 1);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InjectedSinkFailure;

struct FailingSink {
    remaining_calls: usize,
}

impl CanonicalSink for FailingSink {
    type Error = InjectedSinkFailure;

    fn write_all(&mut self, _bytes: &[u8]) -> Result<(), Self::Error> {
        if self.remaining_calls == 0 {
            return Err(InjectedSinkFailure);
        }
        self.remaining_calls -= 1;
        Ok(())
    }
}

#[test]
fn streaming_plan_propagates_sink_failure_and_has_a_payload_digest() {
    let plan = populated_batch().plan().unwrap();
    let bytes = encode(&plan);
    assert_eq!(plan.payload_digest(), ObjectDigest::of_canonical_bytes(&bytes));
    assert_eq!(
        plan.payload_digest().to_string(),
        "0ee6dad417d64eab84f0aa4443f536da162c8e9d775820f39991b1190631adb0"
    );
    let mut sink = FailingSink { remaining_calls: 5 };
    assert_eq!(plan.write_canonical(&mut sink), Err(InjectedSinkFailure));
}

#[test]
fn decoder_rejects_noncontiguous_generation_and_wrong_section_order() {
    let bytes = encode(&populated_batch().plan().unwrap());

    let mut generation = bytes.clone();
    generation[48..56].copy_from_slice(&43_u64.to_le_bytes());
    assert_eq!(
        RSpaceMutationBatchV1::parse(&generation).err().unwrap(),
        MutationError::NonContiguousGeneration {
            base: 41,
            target: 43,
        }
    );

    let mut section = bytes;
    section[104..112].copy_from_slice(&2_u64.to_le_bytes());
    assert_eq!(
        RSpaceMutationBatchV1::parse(&section).err().unwrap(),
        MutationError::SectionOrder {
            expected: 1,
            actual: 2,
        }
    );
}

#[test]
fn decoder_rejects_noop_unsorted_and_malformed_keys() {
    let mut batch = RSpaceMutationBatchV1::new(state(1)).unwrap();
    let first = datum(b"a");
    let second = datum(b"b");
    assert_eq!(first.len(), second.len());
    batch.record_data(&first, 3, 4).unwrap();
    batch.record_data(&second, 5, 6).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let first_key_at = 128;
    let first_before_at = first_key_at + first.len();
    let second_key_at = first_before_at + 16 + 8;

    let mut noop = bytes.clone();
    noop[first_before_at + 8..first_before_at + 16].copy_from_slice(&3_u64.to_le_bytes());
    assert_eq!(RSpaceMutationBatchV1::parse(&noop).err().unwrap(), MutationError::NoOpTransition);

    let mut duplicate = bytes.clone();
    duplicate.copy_within(first_key_at..first_key_at + first.len(), second_key_at);
    assert!(matches!(
        RSpaceMutationBatchV1::parse(&duplicate),
        Err(MutationError::KeysNotStrict { .. })
    ));

    let mut descending = bytes.clone();
    descending[second_key_at + second.len() - 1] = b'`';
    assert!(matches!(
        RSpaceMutationBatchV1::parse(&descending),
        Err(MutationError::KeysNotStrict { .. })
    ));

    let mut malformed = bytes;
    malformed[first_key_at] ^= 0xff;
    assert!(matches!(
        RSpaceMutationBatchV1::parse(&malformed),
        Err(MutationError::InvalidKey { .. })
    ));
}

#[test]
fn decoder_rejects_header_reference_and_presence_corruption() {
    let bytes = encode(&populated_batch().plan().unwrap());

    let mut schema = bytes.clone();
    schema[32..40].copy_from_slice(&2_u64.to_le_bytes());
    assert!(matches!(
        MutationBatchViewV1::parse(&schema),
        Err(MutationError::Canonical(CanonicalFormatError::UnsupportedVersion {
            domain: "RSpace mutation schema",
            version: 2,
        }))
    ));

    let mut base_ref = bytes.clone();
    base_ref[88..96].copy_from_slice(&1_u64.to_le_bytes());
    assert!(matches!(
        MutationBatchViewV1::parse(&base_ref),
        Err(MutationError::Canonical(CanonicalFormatError::ObjectLengthTooSmall(1)))
    ));

    let mut section_count = bytes.clone();
    section_count[96..104].copy_from_slice(&4_u64.to_le_bytes());
    assert_eq!(
        MutationBatchViewV1::parse(&section_count).err().unwrap(),
        MutationError::SectionCount(4)
    );

    let data_key_len = u64::from_le_bytes(bytes[120..128].try_into().unwrap()) as usize;
    let continuation_tag_at = 128 + data_key_len + 16;
    let continuation_key_len_at = continuation_tag_at + 16;
    let continuation_key_len = u64::from_le_bytes(
        bytes[continuation_key_len_at..continuation_key_len_at + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    let before_tag_at = continuation_key_len_at + 8 + continuation_key_len;
    let mut presence = bytes;
    presence[before_tag_at] = 2;
    assert_eq!(
        MutationBatchViewV1::parse(&presence).err().unwrap(),
        MutationError::InvalidPresenceTag(2)
    );
}

#[test]
fn count_transition_preserves_the_full_u64_multiplicity_domain() {
    let key = datum(b"maximum");
    let mut batch = RSpaceMutationBatchV1::new(state(99)).unwrap();
    batch.record_data(&key, 0, u64::MAX).unwrap();
    let bytes = encode(&batch.plan().unwrap());
    let parsed = MutationBatchViewV1::parse(&bytes).unwrap();
    let transition = parsed.data().next().unwrap().transition;
    assert_eq!(transition.before(), 0);
    assert_eq!(transition.after(), u64::MAX);
}

#[test]
fn decoder_enforces_total_resources_without_a_depth_limit() {
    let bytes = encode(&populated_batch().plan().unwrap());
    assert!(matches!(
        RSpaceMutationBatchV1::parse_with_limits(&bytes, MutationDecodeLimits {
            max_entries: 2,
            max_key_bytes: u64::MAX,
        }),
        Err(MutationError::Canonical(CanonicalFormatError::ResourceLimit {
            resource: "mutation entries",
            ..
        }))
    ));
    assert!(matches!(
        RSpaceMutationBatchV1::parse_with_limits(&bytes, MutationDecodeLimits {
            max_entries: u64::MAX,
            max_key_bytes: 1,
        }),
        Err(MutationError::Canonical(CanonicalFormatError::ResourceLimit {
            resource: "mutation key bytes",
            ..
        }))
    ));
}

#[test]
fn generation_overflow_and_trailing_bytes_fail_closed() {
    assert_eq!(
        RSpaceMutationBatchV1::new(state(u64::MAX)).err().unwrap(),
        MutationError::GenerationOverflow(u64::MAX)
    );

    let mut trailing = encode(&populated_batch().plan().unwrap());
    trailing.push(0);
    assert!(matches!(
        RSpaceMutationBatchV1::parse(&trailing),
        Err(MutationError::Canonical(CanonicalFormatError::TrailingBytes { .. }))
    ));
}

#[test]
fn hundred_thousand_byte_key_round_trips_on_a_small_native_stack() {
    let thread = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(|| {
            let key = datum(&vec![0x5a; 100_000]);
            let mut batch = RSpaceMutationBatchV1::new(state(8)).unwrap();
            batch.record_data(&key, 0, 1).unwrap();
            let bytes = encode(&batch.plan().unwrap());
            let parsed = RSpaceMutationBatchV1::parse(&bytes).unwrap();
            assert_eq!(parsed.data_transition(&key).unwrap().after(), 1);
        })
        .expect("small-stack thread starts");
    thread.join().expect("mutation codec remains stack-safe");
}
