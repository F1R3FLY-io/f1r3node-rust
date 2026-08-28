use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::convert::Infallible;

use proptest::prelude::*;

use super::support::{encode, synthetic_ref};
use crate::rspace::persistence::mutation::{
    MUTATION_STREAM_MAX_SCRATCH_BYTES_V1, MutationBatchReadAtV1, MutationReadClassV1,
    MutationStreamErrorV1, StreamedMutationBatchV1, stream_mutation_batch_v1,
};
use crate::rspace::persistence::{
    MutationBatchViewV1, MutationDecodeLimits, MutationError, ObjectDigest, RSpaceMutationBatchV1,
    StateVersionV1,
};
use crate::rspace::rho_path::RhoPathV1;

struct SliceReader {
    bytes: RefCell<Vec<u8>>,
    reads: RefCell<Vec<(MutationReadClassV1, u64, usize)>>,
    mutate_on_read: Cell<Option<(usize, u8)>>,
}

impl SliceReader {
    fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes: RefCell::new(bytes),
            reads: RefCell::new(Vec::new()),
            mutate_on_read: Cell::new(None),
        }
    }
}

impl MutationBatchReadAtV1 for SliceReader {
    type Error = Infallible;

    fn read_exact_at(
        &mut self,
        class: MutationReadClassV1,
        offset: u64,
        destination: &mut [u8],
    ) -> Result<(), Self::Error> {
        if let Some((read_index, replacement)) = self.mutate_on_read.get() {
            if self.reads.borrow().len() == read_index {
                let index = usize::try_from(offset).unwrap();
                self.bytes.borrow_mut()[index] = replacement;
                self.mutate_on_read.set(None);
            }
        }
        let start = usize::try_from(offset).unwrap();
        destination.copy_from_slice(&self.bytes.borrow()[start..start + destination.len()]);
        self.reads
            .borrow_mut()
            .push((class, offset, destination.len()));
        Ok(())
    }
}

fn batch_bytes(identities: &[u8]) -> Vec<u8> {
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(41, synthetic_ref(90))).unwrap();
    for &identity in identities {
        batch
            .record_data(
                &RhoPathV1::datum_key(&7_u64, b"shape", &[identity]),
                0,
                u64::from(identity) + 1,
            )
            .unwrap();
        batch
            .record_continuation(
                &RhoPathV1::collection_record_path(
                    &RhoPathV1::continuation_prefix(&[7_u64, 8_u64]),
                    &[identity],
                ),
                None,
                Some(synthetic_ref(identity)),
            )
            .unwrap();
        let mut join = RhoPathV1::join_prefix(&7_u64);
        join.push(identity);
        batch
            .record_join(&join, 2, u64::from(identity) + 3)
            .unwrap();
    }
    encode(&batch.plan().unwrap())
}

fn batch_bytes_for_variable_identities(identities: &[&[u8]]) -> Vec<u8> {
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(41, synthetic_ref(90))).unwrap();
    for (ordinal, identity) in identities.iter().enumerate() {
        batch
            .record_data(&RhoPathV1::datum_key(&7_u64, b"shape", identity), 0, ordinal as u64 + 1)
            .unwrap();
        batch
            .record_continuation(
                &RhoPathV1::collection_record_path(
                    &RhoPathV1::continuation_prefix(&[7_u64, 8_u64]),
                    identity,
                ),
                None,
                Some(synthetic_ref(ordinal as u8 + 1)),
            )
            .unwrap();
        let mut join = RhoPathV1::join_prefix(&7_u64);
        join.extend_from_slice(identity);
        batch.record_join(&join, 0, ordinal as u64 + 1).unwrap();
    }
    encode(&batch.plan().unwrap())
}

type MutationIdentity = (StateVersionV1, u64, u64, u64, ObjectDigest);

fn oracle_metrics_with_limits(
    bytes: &[u8],
    limits: MutationDecodeLimits,
) -> Result<MutationIdentity, MutationError> {
    let view = MutationBatchViewV1::parse_with_limits(bytes, limits)?;
    let data = view.data().collect::<Vec<_>>();
    let continuations = view.continuations().collect::<Vec<_>>();
    let joins = view.joins().collect::<Vec<_>>();
    let entry_count = (data.len() + continuations.len() + joins.len()) as u64;
    let key_bytes = data
        .iter()
        .map(|entry| entry.key.len() as u64)
        .chain(continuations.iter().map(|entry| entry.key.len() as u64))
        .chain(joins.iter().map(|entry| entry.key.len() as u64))
        .sum();
    Ok((view.base(), view.target_generation(), entry_count, key_bytes, view.payload_digest()))
}

fn oracle_metrics(bytes: &[u8]) -> MutationIdentity {
    oracle_metrics_with_limits(bytes, MutationDecodeLimits::UNBOUNDED).unwrap()
}

fn streamed_metrics_with_limits(
    bytes: &[u8],
    limits: MutationDecodeLimits,
    scratch_len: usize,
) -> Result<MutationIdentity, MutationError> {
    let mut reader = SliceReader::new(bytes.to_vec());
    let mut scratch = vec![0; scratch_len];
    match stream_mutation_batch_v1(
        &mut reader,
        0,
        bytes.len() as u64,
        limits,
        u64::MAX,
        &mut scratch,
        |_| {},
    ) {
        Ok(streamed) => Ok((
            streamed.base(),
            streamed.target_generation(),
            streamed.entry_count(),
            streamed.key_bytes(),
            streamed.payload_digest(),
        )),
        Err(failure) => match failure.into_error() {
            MutationStreamErrorV1::Mutation(error) => Err(error),
            MutationStreamErrorV1::Read(never) => match never {},
            other => panic!("unexpected non-semantic stream failure: {other:?}"),
        },
    }
}

fn assert_stream_matches_oracle(bytes: &[u8], limits: MutationDecodeLimits) {
    let expected = oracle_metrics_with_limits(bytes, limits);
    for scratch_len in [2, 3, 16, 127, 128, 4096] {
        assert_eq!(
            streamed_metrics_with_limits(bytes, limits, scratch_len),
            expected,
            "scratch length {scratch_len}",
        );
    }
}

#[derive(Debug)]
struct MutationEntryLayout {
    key_len_offset: usize,
    key_start: usize,
    key_end: usize,
    transition_start: usize,
    end: usize,
}

#[derive(Debug)]
struct MutationSectionLayout {
    tag_offset: usize,
    count_offset: usize,
    entries: Vec<MutationEntryLayout>,
}

fn read_u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

fn mutation_layout(bytes: &[u8]) -> [MutationSectionLayout; 3] {
    let mut cursor = 104usize;
    std::array::from_fn(|section_index| {
        let tag_offset = cursor;
        cursor += 8;
        let count_offset = cursor;
        let count = usize::try_from(read_u64_at(bytes, cursor)).unwrap();
        cursor += 8;
        let entries = (0..count)
            .map(|_| {
                let key_len_offset = cursor;
                let key_len = usize::try_from(read_u64_at(bytes, cursor)).unwrap();
                cursor += 8;
                let key_start = cursor;
                cursor += key_len;
                let key_end = cursor;
                let transition_start = cursor;
                if section_index == 1 {
                    for _ in 0..2 {
                        let present = bytes[cursor];
                        cursor += 1;
                        if present == 1 {
                            cursor += 40;
                        }
                    }
                } else {
                    cursor += 16;
                }
                MutationEntryLayout {
                    key_len_offset,
                    key_start,
                    key_end,
                    transition_start,
                    end: cursor,
                }
            })
            .collect();
        MutationSectionLayout {
            tag_offset,
            count_offset,
            entries,
        }
    })
}

fn empty_batch_bytes() -> Vec<u8> {
    let source = batch_bytes(&[1]);
    let layout = mutation_layout(&source);
    let mut bytes = source[..104].to_vec();
    for section in &layout {
        bytes.extend_from_slice(&source[section.tag_offset..section.tag_offset + 8]);
        bytes.extend_from_slice(&0_u64.to_le_bytes());
    }
    let body_len = u64::try_from(bytes.len() - 32).unwrap();
    bytes[24..32].copy_from_slice(&body_len.to_le_bytes());
    bytes
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn streamed_mutation_identity_and_digest_match_the_contiguous_oracle_and_observe_primary_bytes_once(
        mut identities in proptest::collection::vec(any::<u8>(), 1..32),
        scratch_len in 2usize..=4096,
        source_prefix in proptest::collection::vec(any::<u8>(), 0..64),
    ) {
        identities.sort_unstable();
        identities.dedup();
        prop_assume!(!identities.is_empty());
        let bytes = batch_bytes(&identities);
        let expected = oracle_metrics(&bytes);
        let source_offset = source_prefix.len() as u64;
        let mut source = source_prefix;
        source.extend_from_slice(&bytes);
        let mut reader = SliceReader::new(source);
        let mut observed = Vec::new();
        let mut scratch = vec![0; scratch_len];
        let streamed = stream_mutation_batch_v1(
            &mut reader,
            source_offset,
            bytes.len() as u64,
            MutationDecodeLimits::UNBOUNDED,
            u64::MAX,
            &mut scratch,
            |chunk| observed.extend_from_slice(chunk),
        ).unwrap();
        prop_assert_eq!(observed.as_slice(), bytes.as_slice());
        prop_assert_eq!(streamed.base(), expected.0);
        prop_assert_eq!(streamed.target_generation(), expected.1);
        prop_assert_eq!(streamed.entry_count(), expected.2);
        prop_assert_eq!(streamed.key_bytes(), expected.3);
        prop_assert_eq!(streamed.payload_digest(), expected.4);
        prop_assert_eq!(streamed.primary_read_bytes(), bytes.len() as u64);
        prop_assert_eq!(
            streamed.physical_read_bytes(),
            streamed.primary_read_bytes() + streamed.prior_key_reread_bytes(),
        );
        prop_assert!(streamed.physical_read_bytes() >= bytes.len() as u64);
    }

    #[test]
    fn every_single_byte_substitution_preserves_exact_mutation_oracle_identity_or_error(
        selected in any::<usize>(),
        mask in any::<u8>(),
        scratch_len in 2usize..=4096,
    ) {
        let mut bytes = batch_bytes(&[1, 2, 3]);
        let index = selected % bytes.len();
        bytes[index] ^= mask | 1;
        prop_assert_eq!(
            streamed_metrics_with_limits(
                &bytes,
                MutationDecodeLimits::UNBOUNDED,
                scratch_len,
            ),
            oracle_metrics_with_limits(&bytes, MutationDecodeLimits::UNBOUNDED),
        );
    }
}

#[test]
fn streamed_mutation_envelope_generation_and_section_order_match_contiguous_oracle() {
    let valid = batch_bytes(&[1, 2]);
    assert_stream_matches_oracle(&valid, MutationDecodeLimits::UNBOUNDED);
    let layout = mutation_layout(&valid);
    let mut faults = Vec::new();

    let mut invalid_domain = valid.clone();
    invalid_domain[0] ^= 0x80;
    faults.push(invalid_domain);

    let mut invalid_envelope_version = valid.clone();
    invalid_envelope_version[16..24].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_envelope_version);

    let mut truncated_extent = valid.clone();
    let body_len = read_u64_at(&truncated_extent, 24);
    truncated_extent[24..32].copy_from_slice(&(body_len + 1).to_le_bytes());
    faults.push(truncated_extent);

    let mut trailing_extent = valid.clone();
    trailing_extent[24..32].copy_from_slice(&(body_len - 1).to_le_bytes());
    faults.push(trailing_extent);

    let mut invalid_schema = valid.clone();
    invalid_schema[32..40].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_schema);

    let mut noncontiguous_target = valid.clone();
    let base = read_u64_at(&noncontiguous_target, 40);
    noncontiguous_target[48..56].copy_from_slice(&(base + 2).to_le_bytes());
    faults.push(noncontiguous_target);

    let mut overflow_base = valid.clone();
    overflow_base[40..48].copy_from_slice(&u64::MAX.to_le_bytes());
    overflow_base[48..56].copy_from_slice(&0_u64.to_le_bytes());
    faults.push(overflow_base);

    let mut invalid_section_count = valid.clone();
    invalid_section_count[96..104].copy_from_slice(&2_u64.to_le_bytes());
    faults.push(invalid_section_count);

    for section_index in 0..3 {
        let mut wrong_order = valid.clone();
        let wrong_tag = read_u64_at(&valid, layout[(section_index + 1) % 3].tag_offset);
        wrong_order[layout[section_index].tag_offset..layout[section_index].tag_offset + 8]
            .copy_from_slice(&wrong_tag.to_le_bytes());
        faults.push(wrong_order);
    }
    let section_tags: [u64; 3] =
        std::array::from_fn(|index| read_u64_at(&valid, layout[index].tag_offset));
    for permutation in [[0, 2, 1], [1, 0, 2], [1, 2, 0], [2, 0, 1], [2, 1, 0]] {
        let mut permuted = valid.clone();
        for (section_index, source_index) in permutation.into_iter().enumerate() {
            permuted[layout[section_index].tag_offset..layout[section_index].tag_offset + 8]
                .copy_from_slice(&section_tags[source_index].to_le_bytes());
        }
        faults.push(permuted);
    }

    for fault in faults {
        assert!(MutationBatchViewV1::parse(&fault).is_err());
        assert_stream_matches_oracle(&fault, MutationDecodeLimits::UNBOUNDED);
    }
}

#[test]
fn streamed_mutation_aggregate_counts_and_key_bytes_are_exactly_admitted() {
    let bytes = batch_bytes(&[1, 2, 3, 4]);
    let identity = oracle_metrics(&bytes);
    let layout = mutation_layout(&bytes);
    let independent_entries: u64 = layout
        .iter()
        .map(|section| section.entries.len() as u64)
        .sum();
    let independent_key_bytes: u64 = layout
        .iter()
        .flat_map(|section| &section.entries)
        .map(|entry| (entry.key_end - entry.key_start) as u64)
        .sum();
    assert_eq!(identity.2, independent_entries);
    assert_eq!(identity.3, independent_key_bytes);
    let exact = MutationDecodeLimits {
        max_entries: identity.2,
        max_key_bytes: identity.3,
    };
    assert_stream_matches_oracle(&bytes, exact);

    for limits in [
        MutationDecodeLimits {
            max_entries: identity.2 - 1,
            max_key_bytes: identity.3,
        },
        MutationDecodeLimits {
            max_entries: identity.2,
            max_key_bytes: identity.3 - 1,
        },
    ] {
        assert!(oracle_metrics_with_limits(&bytes, limits).is_err());
        assert_stream_matches_oracle(&bytes, limits);
    }

    for section_index in 0..3 {
        for delta in [-1_i64, 1] {
            let mut malformed = bytes.clone();
            let count = read_u64_at(&malformed, layout[section_index].count_offset);
            let changed = if delta < 0 { count - 1 } else { count + 1 };
            malformed[layout[section_index].count_offset..layout[section_index].count_offset + 8]
                .copy_from_slice(&changed.to_le_bytes());
            assert_stream_matches_oracle(&malformed, MutationDecodeLimits::UNBOUNDED);
        }
    }
}

#[test]
fn streamed_prior_key_reread_proves_full_lexicographic_order_or_rejects_source_change() {
    let bytes = batch_bytes(&[1, 2, 3, 4]);
    let mut stable_reader = SliceReader::new(bytes.clone());
    let mut scratch = [0u8; 32];
    let stable = stream_mutation_batch_v1(
        &mut stable_reader,
        0,
        bytes.len() as u64,
        MutationDecodeLimits::UNBOUNDED,
        u64::MAX,
        &mut scratch,
        |_| {},
    )
    .unwrap();
    let reads = stable_reader.reads.into_inner();
    let layout = mutation_layout(&bytes);
    let expected_reread_bytes: u64 = layout
        .iter()
        .flat_map(|section| section.entries.iter().rev().skip(1))
        .map(|entry| (entry.key_end - entry.key_start) as u64)
        .sum();
    assert_eq!(stable.primary_read_bytes(), bytes.len() as u64);
    assert_eq!(stable.prior_key_reread_bytes(), expected_reread_bytes);
    assert_eq!(stable.physical_read_bytes(), bytes.len() as u64 + expected_reread_bytes,);
    assert_eq!(
        reads
            .iter()
            .filter(|(class, _, _)| *class == MutationReadClassV1::Primary)
            .map(|(_, _, len)| *len as u64)
            .sum::<u64>(),
        bytes.len() as u64,
    );
    assert_eq!(
        reads
            .iter()
            .filter(|(class, _, _)| *class == MutationReadClassV1::PriorKeyReread)
            .map(|(_, _, len)| *len as u64)
            .sum::<u64>(),
        expected_reread_bytes,
    );
    let mut primary_seen = BTreeSet::new();
    let mut reread_index = None;
    for (index, (class, offset, len)) in reads.iter().copied().enumerate() {
        let range = offset..offset + len as u64;
        match class {
            MutationReadClassV1::Primary => primary_seen.extend(range),
            MutationReadClassV1::PriorKeyReread => {
                assert!(
                    range.clone().all(|byte| primary_seen.contains(&byte)),
                    "prior-key validation may reread only bytes already authenticated by the \
                     primary stream",
                );
                reread_index.get_or_insert(index);
            }
        }
    }
    let reread_index = reread_index.expect("multiple keys require a prior-key reread");
    assert_eq!(primary_seen.len(), bytes.len());
    let reread_offset = usize::try_from(reads[reread_index].1).unwrap();
    assert!(stable.physical_read_bytes() > bytes.len() as u64);

    let mut unstable_reader = SliceReader::new(bytes.clone());
    unstable_reader
        .mutate_on_read
        .set(Some((reread_index, bytes[reread_offset] ^ 0x80)));
    let mut scratch = [0u8; 32];
    let failure = stream_mutation_batch_v1(
        &mut unstable_reader,
        0,
        bytes.len() as u64,
        MutationDecodeLimits::UNBOUNDED,
        u64::MAX,
        &mut scratch,
        |_| {},
    )
    .unwrap_err();
    assert!(matches!(failure.error(), MutationStreamErrorV1::SourceChanged { .. }));
    assert_eq!(failure.source_cursor(), bytes.len() as u64);
    assert_eq!(failure.primary_read_bytes(), bytes.len() as u64);
    assert!(failure.prior_key_reread_bytes() > 0);
    assert_eq!(
        failure.physical_read_bytes(),
        failure.primary_read_bytes() + failure.prior_key_reread_bytes(),
    );

    let mut duplicate = bytes.clone();
    let layout = mutation_layout(&duplicate);
    let first = &layout[0].entries[0];
    let second = &layout[0].entries[1];
    assert_eq!(first.key_end - first.key_start, second.key_end - second.key_start);
    let first_key = duplicate[first.key_start..first.key_end].to_vec();
    duplicate[second.key_start..second.key_end].copy_from_slice(&first_key);
    assert!(matches!(
        MutationBatchViewV1::parse(&duplicate),
        Err(MutationError::KeysNotStrict { .. })
    ));
    assert_stream_matches_oracle(&duplicate, MutationDecodeLimits::UNBOUNDED);

    let mut swapped = bytes.clone();
    let layout = mutation_layout(&swapped);
    let first = &layout[2].entries[0];
    let second = &layout[2].entries[1];
    assert_eq!(first.end - first.key_len_offset, second.end - second.key_len_offset);
    let first_entry = swapped[first.key_len_offset..first.end].to_vec();
    let second_entry = swapped[second.key_len_offset..second.end].to_vec();
    swapped[first.key_len_offset..first.end].copy_from_slice(&second_entry);
    swapped[second.key_len_offset..second.end].copy_from_slice(&first_entry);
    assert_stream_matches_oracle(&swapped, MutationDecodeLimits::UNBOUNDED);

    let prefix_related = batch_bytes_for_variable_identities(&[b"a", b"ab"]);
    assert!(MutationBatchViewV1::parse(&prefix_related).is_ok());
    assert_stream_matches_oracle(&prefix_related, MutationDecodeLimits::UNBOUNDED);

    let layout = mutation_layout(&prefix_related);
    let first = &layout[0].entries[0];
    let second = &layout[0].entries[1];
    let first_entry = prefix_related[first.key_len_offset..first.end].to_vec();
    let second_entry = prefix_related[second.key_len_offset..second.end].to_vec();
    let mut reversed_prefix = prefix_related.clone();
    let mut replacement = second_entry;
    replacement.extend_from_slice(&first_entry);
    reversed_prefix.splice(first.key_len_offset..second.end, replacement);
    assert!(matches!(
        MutationBatchViewV1::parse(&reversed_prefix),
        Err(MutationError::KeysNotStrict { .. }),
    ));
    assert_stream_matches_oracle(&reversed_prefix, MutationDecodeLimits::UNBOUNDED);
}

fn continuation_transition_bytes(
    before: Option<ObjectDigest>,
    after: Option<ObjectDigest>,
) -> Result<Vec<u8>, MutationError> {
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(41, synthetic_ref(90)))?;
    let reference =
        |digest: ObjectDigest| crate::rspace::persistence::ObjectRefV1::new(digest, 50).unwrap();
    batch.record_continuation(
        &RhoPathV1::collection_record_path(
            &RhoPathV1::continuation_prefix(&[7_u64, 8]),
            b"identity",
        ),
        before.map(reference),
        after.map(reference),
    )?;
    Ok(encode(&batch.plan()?))
}

#[test]
fn streamed_mutation_transition_presence_and_empty_rules_match_oracle() {
    let a = ObjectDigest::from_array([0xa1; 32]);
    let b = ObjectDigest::from_array([0xb2; 32]);
    for bytes in [
        continuation_transition_bytes(None, Some(a)).unwrap(),
        continuation_transition_bytes(Some(a), None).unwrap(),
        continuation_transition_bytes(Some(a), Some(b)).unwrap(),
    ] {
        assert_stream_matches_oracle(&bytes, MutationDecodeLimits::UNBOUNDED);
    }
    assert_eq!(continuation_transition_bytes(None, None), Err(MutationError::EmptyBatch),);
    assert_eq!(continuation_transition_bytes(Some(a), Some(a)), Err(MutationError::EmptyBatch),);

    let none_to_some = continuation_transition_bytes(None, Some(a)).unwrap();
    let none_to_some_layout = mutation_layout(&none_to_some);
    let content = &none_to_some_layout[1].entries[0];
    let mut invalid_tag = none_to_some.clone();
    invalid_tag[content.transition_start] = 2;
    assert!(matches!(
        MutationBatchViewV1::parse(&invalid_tag),
        Err(MutationError::InvalidPresenceTag(2)),
    ));
    assert_stream_matches_oracle(&invalid_tag, MutationDecodeLimits::UNBOUNDED);

    let mut none_to_none = none_to_some.clone();
    none_to_none[content.transition_start + 1] = 0;
    assert!(matches!(
        MutationBatchViewV1::parse(&none_to_none),
        Err(MutationError::NoOpTransition),
    ));
    assert_stream_matches_oracle(&none_to_none, MutationDecodeLimits::UNBOUNDED);

    let some_to_some = continuation_transition_bytes(Some(a), Some(b)).unwrap();
    let some_to_some_layout = mutation_layout(&some_to_some);
    let content = &some_to_some_layout[1].entries[0];
    let mut same_reference = some_to_some.clone();
    let before_reference =
        same_reference[content.transition_start + 1..content.transition_start + 41].to_vec();
    same_reference[content.transition_start + 42..content.transition_start + 82]
        .copy_from_slice(&before_reference);
    assert!(matches!(
        MutationBatchViewV1::parse(&same_reference),
        Err(MutationError::NoOpTransition),
    ));
    assert_stream_matches_oracle(&same_reference, MutationDecodeLimits::UNBOUNDED);

    let empty = empty_batch_bytes();
    assert_eq!(MutationBatchViewV1::parse(&empty).unwrap_err(), MutationError::EmptyBatch);
    assert_stream_matches_oracle(&empty, MutationDecodeLimits::UNBOUNDED);
}

#[test]
fn streamed_mutation_consumes_exact_declared_extent_and_preserves_precedence() {
    let valid = batch_bytes(&[1, 2]);
    for prefix_len in 0..valid.len() {
        assert_stream_matches_oracle(&valid[..prefix_len], MutationDecodeLimits::UNBOUNDED);
    }

    for trailing_len in [1, 2, 7, 8, 31] {
        let mut trailing = valid.clone();
        trailing.extend(std::iter::repeat_n(0xa5, trailing_len));
        assert_stream_matches_oracle(&trailing, MutationDecodeLimits::UNBOUNDED);
    }

    let mut malformed_key_and_noop = valid.clone();
    let layout = mutation_layout(&malformed_key_and_noop);
    let first = &layout[0].entries[0];
    malformed_key_and_noop[first.key_start] ^= 0x80;
    let before =
        malformed_key_and_noop[first.transition_start..first.transition_start + 8].to_vec();
    malformed_key_and_noop[first.transition_start + 8..first.transition_start + 16]
        .copy_from_slice(&before);
    assert!(matches!(
        MutationBatchViewV1::parse(&malformed_key_and_noop),
        Err(MutationError::InvalidKey { .. }),
    ));
    assert_stream_matches_oracle(&malformed_key_and_noop, MutationDecodeLimits::UNBOUNDED);

    let mut duplicate_key_and_noop = valid.clone();
    let layout = mutation_layout(&duplicate_key_and_noop);
    let first = &layout[0].entries[0];
    let second = &layout[0].entries[1];
    let first_key = duplicate_key_and_noop[first.key_start..first.key_end].to_vec();
    duplicate_key_and_noop[second.key_start..second.key_end].copy_from_slice(&first_key);
    let before =
        duplicate_key_and_noop[second.transition_start..second.transition_start + 8].to_vec();
    duplicate_key_and_noop[second.transition_start + 8..second.transition_start + 16]
        .copy_from_slice(&before);
    assert!(matches!(
        MutationBatchViewV1::parse(&duplicate_key_and_noop),
        Err(MutationError::KeysNotStrict { .. }),
    ));
    assert_stream_matches_oracle(&duplicate_key_and_noop, MutationDecodeLimits::UNBOUNDED);

    let mut trailing_empty = empty_batch_bytes();
    trailing_empty.push(0);
    assert!(matches!(
        MutationBatchViewV1::parse(&trailing_empty),
        Err(MutationError::Canonical(
            crate::rspace::persistence::CanonicalFormatError::TrailingBytes { .. }
        )),
    ));
    assert_stream_matches_oracle(&trailing_empty, MutationDecodeLimits::UNBOUNDED);

    let mut envelope_before_schema = valid.clone();
    envelope_before_schema[16..24].copy_from_slice(&2_u64.to_le_bytes());
    envelope_before_schema[32..40].copy_from_slice(&2_u64.to_le_bytes());
    assert!(matches!(
        MutationBatchViewV1::parse(&envelope_before_schema),
        Err(MutationError::Canonical(
            crate::rspace::persistence::CanonicalFormatError::UnsupportedVersion {
                domain: "RSpace mutation batch",
                ..
            }
        )),
    ));
    assert_stream_matches_oracle(&envelope_before_schema, MutationDecodeLimits::UNBOUNDED);
}

#[test]
fn streamed_mutation_exact_physical_budget_accepts_and_minus_one_rejects_before_overread() {
    let bytes = batch_bytes(&[1, 2, 3, 4]);
    let run = |limit| {
        let mut reader = SliceReader::new(bytes.clone());
        let mut scratch = [0u8; 128];
        let result = stream_mutation_batch_v1(
            &mut reader,
            0,
            bytes.len() as u64,
            MutationDecodeLimits::UNBOUNDED,
            limit,
            &mut scratch,
            |_| {},
        );
        (result, reader.reads.into_inner())
    };
    let (unbounded, _) = run(u64::MAX);
    let exact = unbounded.unwrap().physical_read_bytes();
    assert!(run(exact).0.is_ok());
    let (under, reads) = run(exact - 1);
    assert!(matches!(
        under.unwrap_err().error(),
        MutationStreamErrorV1::PhysicalReadLimit { limit, attempted } if *limit == exact - 1 && *attempted == exact
    ));
    assert!(reads.iter().map(|(_, _, len)| *len as u64).sum::<u64>() < exact);
}

#[test]
fn streamed_mutation_rejects_invalid_scratch_without_reading() {
    let bytes = batch_bytes(&[1]);
    for actual in [0, 1, MUTATION_STREAM_MAX_SCRATCH_BYTES_V1 + 1] {
        let mut reader = SliceReader::new(bytes.clone());
        let mut scratch = vec![0; actual];
        let failure = stream_mutation_batch_v1(
            &mut reader,
            0,
            bytes.len() as u64,
            MutationDecodeLimits::UNBOUNDED,
            u64::MAX,
            &mut scratch,
            |_| {},
        )
        .unwrap_err();
        assert!(
            matches!(failure.error(), MutationStreamErrorV1::ScratchBytes { actual: got, .. } if *got == actual)
        );
        assert!(reader.reads.borrow().is_empty());
    }
}

#[test]
fn streamed_mutation_semantic_failure_raw_drains_the_declared_extent() {
    let mut bytes = batch_bytes(&[1, 2]);
    bytes[32..40].copy_from_slice(&2_u64.to_le_bytes());
    let mut reader = SliceReader::new(bytes.clone());
    let mut scratch = [0u8; 128];
    let mut observed = Vec::new();
    let failure = stream_mutation_batch_v1(
        &mut reader,
        0,
        bytes.len() as u64,
        MutationDecodeLimits::UNBOUNDED,
        u64::MAX,
        &mut scratch,
        |chunk| observed.extend_from_slice(chunk),
    )
    .unwrap_err();
    assert!(matches!(failure.error(), MutationStreamErrorV1::Mutation(_)));
    assert_eq!(failure.source_cursor(), bytes.len() as u64);
    assert_eq!(failure.primary_read_bytes(), bytes.len() as u64);
    assert_eq!(failure.prior_key_reread_bytes(), 0);
    assert_eq!(failure.physical_read_bytes(), bytes.len() as u64);
    assert!(
        reader
            .reads
            .borrow()
            .iter()
            .all(|(class, _, _)| *class == MutationReadClassV1::Primary)
    );
    assert_eq!(observed, bytes);
}

#[test]
fn streamed_mutation_retained_memory_is_independent_of_key_and_batch_extent() {
    assert!(std::mem::size_of::<StreamedMutationBatchV1>() <= 128);
    let long_identity = vec![0x5a; 65_537];
    let many_identities = (0u8..128).map(|index| [index, !index]).collect::<Vec<_>>();
    let many_identity_slices = many_identities
        .iter()
        .map(|identity| identity.as_slice())
        .collect::<Vec<_>>();
    for bytes in [
        batch_bytes_for_variable_identities(&[long_identity.as_slice()]),
        batch_bytes_for_variable_identities(&many_identity_slices),
    ] {
        let mut reader = SliceReader::new(bytes.clone());
        let mut scratch = [0u8; 4096];
        assert!(
            stream_mutation_batch_v1(
                &mut reader,
                0,
                bytes.len() as u64,
                MutationDecodeLimits::UNBOUNDED,
                u64::MAX,
                &mut scratch,
                |_| {},
            )
            .is_ok()
        );
    }
}
