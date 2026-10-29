use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::ordered_receive::{
    ContinuationFieldBounds, ContinuationRecordView, OrderedReceiveError, OrderedReceiveView,
    PreparedContinuationRecord, ReceivePatternLayout, RestingReceiveVisit,
};
use rspace_plus_plus::rspace::rho_path::{
    ReceiveChannelProfile, ReceiveChannelQuery, RhoPathEncode, RhoPathError, RhoPathV2,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError,
};
use rspace_plus_plus::rspace::serializers::serializers::{
    CandidateOrderingBytes, encode_continuations,
};
use serde::Serialize;

const IMAGES: [&[u8]; 5] = [&[], &[0x00], &[0x01], &[0x01, 0x00], &[0x01, 0x00, 0xFF]];
const ABSENT_IMAGE: &[u8] = &[0x01, 0x01];

#[derive(Clone, Debug, Serialize)]
struct LayoutPattern {
    image: Vec<u8>,
    free_count: i32,
}

impl StableHashSerialize for LayoutPattern {}

struct TrailingFreeCount;

impl ReceivePatternLayout for TrailingFreeCount {
    fn trailing_free_count(pattern: &[u8]) -> Option<i32> {
        pattern
            .last_chunk::<4>()
            .map(|word| i32::from_le_bytes(*word))
    }
}

fn string_continuation(
    arity: usize,
    peeks: BTreeSet<i32>,
    persist: bool,
) -> WaitingContinuation<String, String> {
    let channels: Vec<Vec<u8>> = (0..arity)
        .map(|index| index.to_le_bytes().to_vec())
        .collect();
    let patterns: Vec<String> = (0..arity).map(|index| format!("pattern-{index}")).collect();
    WaitingContinuation::create(
        &channels,
        &patterns,
        &"continuation body".to_string(),
        persist,
        peeks,
    )
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize)]
struct RawChannel(Vec<u8>);

impl StableHashSerialize for RawChannel {}

impl RhoPathEncode for RawChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.0.clone() }

    fn rho_path_identity(&self) -> Vec<u8> { self.0.clone() }
}

impl ColdStoreDecode for RawChannel {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        let (raw, consumed) = Vec::<u8>::cold_decode_prefix(bytes)?;
        Ok((RawChannel(raw), consumed))
    }
}

type StringContinuation = WaitingContinuation<String, String>;

#[derive(Clone, Default)]
struct StubHistory {
    continuations: Arc<Mutex<HashMap<Vec<RawChannel>, Vec<StringContinuation>>>>,
    continuation_reads: Arc<AtomicUsize>,
}

impl StubHistory {
    fn put_continuations(&self, channels: Vec<RawChannel>, continuations: Vec<StringContinuation>) {
        self.continuations
            .lock()
            .expect("stub history lock")
            .insert(channels, continuations);
    }

    fn continuation_reads(&self) -> usize { self.continuation_reads.load(Ordering::SeqCst) }
}

impl HistoryReaderBase<RawChannel, String, String, String> for StubHistory {
    fn get_data_canonical(&self, _channel: &RawChannel) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(&self, channels: &[RawChannel]) -> Vec<StringContinuation> {
        self.continuation_reads.fetch_add(1, Ordering::SeqCst);
        self.continuations
            .lock()
            .expect("stub history lock")
            .get(channels)
            .cloned()
            .unwrap_or_default()
    }

    fn get_joins_canonical(&self, _channel: &RawChannel) -> Vec<Vec<RawChannel>> { Vec::new() }
}

fn raw_hot_store() -> (StubHistory, Box<dyn HotStore<RawChannel, String, String, String>>) {
    let history = StubHistory::default();
    let hot_store = HotStoreInstances::create_from_hs_and_hr(
        HotStoreState::default(),
        Box::new(history.clone()),
    );
    (history, hot_store)
}

fn raw_channels(images: &[&[u8]]) -> Vec<RawChannel> {
    images
        .iter()
        .map(|image| RawChannel(image.to_vec()))
        .collect()
}

fn numbered_continuation(arity: usize, number: usize) -> StringContinuation {
    let channels: Vec<Vec<u8>> = (0..arity)
        .map(|index| index.to_le_bytes().to_vec())
        .collect();
    let patterns: Vec<String> = (0..arity).map(|index| format!("pattern-{index}")).collect();
    WaitingContinuation::create(
        &channels,
        &patterns,
        &format!("continuation body {number}"),
        false,
        BTreeSet::new(),
    )
}

fn prepared_identity(continuation: &StringContinuation) -> Vec<u8> {
    PreparedContinuationRecord::encode_waiting(continuation)
        .record_view()
        .identity()
        .to_vec()
}

fn frame_into(output: &mut Vec<u8>, raw: &[u8]) {
    for &byte in raw {
        match byte {
            0x00 => output.extend_from_slice(&[0x00, 0xFF]),
            other => output.push(other),
        }
    }
    output.extend_from_slice(&[0x00, 0x00]);
}

fn legacy_prefix(channels: &[&[u8]]) -> Vec<u8> {
    let mut prefix = b"RHP\x01\x02\x01".to_vec();
    for channel in channels {
        frame_into(&mut prefix, channel);
    }
    prefix.extend_from_slice(&[0x00, 0x01]);
    prefix
}

fn legacy_record_path(prefix: &[u8], canonical: &[u8]) -> Vec<u8> {
    let mut path = Vec::with_capacity(prefix.len() + canonical.len());
    path.extend_from_slice(prefix);
    path.extend_from_slice(canonical);
    path
}

fn empty_rci2_key(identity: &[u8]) -> Vec<u8> {
    let mut key = b"RHP\x02\x02\x01".to_vec();
    key.extend_from_slice(&0u64.to_be_bytes());
    key.extend_from_slice(&[0x00, 0x01]);
    frame_into(&mut key, identity);
    key
}

fn oracle_positions(channels: &[&[u8]], query: &[u8]) -> Vec<u64> {
    channels
        .iter()
        .enumerate()
        .filter(|(_, channel)| **channel == query)
        .map(|(index, _)| u64::try_from(index).expect("a channel index fits u64"))
        .collect()
}

fn admit_with_ends(record: &[u8], ends: &[usize]) -> Result<(), OrderedReceiveError> {
    let bounds = ContinuationFieldBounds::from_ends(ends.to_vec().into_boxed_slice());
    ContinuationRecordView::admit_record(record, &bounds).map(|_| ())
}

fn tail_record(body: &[u8], peek_count: u64) -> (Vec<u8>, [usize; 2]) {
    let mut record = Vec::with_capacity(body.len() + 17);
    record.extend_from_slice(&0u64.to_le_bytes());
    record.extend_from_slice(body);
    record.push(0);
    record.extend_from_slice(&peek_count.to_le_bytes());
    let body_end = body.len() + 8;
    (record, [body_end, body_end + 9])
}

#[test]
fn rci2_views_return_exact_positions_over_explicit_images() {
    for arity in [65usize, 257, 4097] {
        let channels: Vec<&[u8]> = (0..arity)
            .map(|index| IMAGES[(index * 7 + index / 5) % IMAGES.len()])
            .collect();
        let prepared = PreparedContinuationRecord::encode_waiting(&string_continuation(
            arity,
            BTreeSet::new(),
            false,
        ));
        let key = RhoPathV2::continuation_key(&channels, prepared.record_view().identity())
            .expect("a non-empty channel group has an RCI2 key");
        let view =
            OrderedReceiveView::admit_rci2_v2(&key, prepared.canonical(), prepared.field_bounds())
                .expect("an encoded record under its own key admits");
        assert_eq!(view.profile(), ReceiveChannelProfile::Rci2V2);
        assert_eq!(view.record().arity(), arity);
        assert_eq!(view.binds().len(), arity);
        for query in IMAGES.iter().copied().chain([ABSENT_IMAGE]) {
            let positions: Vec<u64> = view
                .positions(ReceiveChannelQuery::Rci2V2(query))
                .expect("an RCI2 query reads an RCI2 view")
                .map(|position| position.get())
                .collect();
            assert_eq!(
                positions,
                oracle_positions(&channels, query),
                "arity {arity}, query {query:?}"
            );
            assert!(matches!(
                view.positions(ReceiveChannelQuery::LegacyV1(query)),
                Err(OrderedReceiveError::ProfileMismatch)
            ));
        }
        for (bind, channel) in view.binds().zip(&channels) {
            assert!(bind.channel_matches(channel));
        }
    }
}

#[test]
fn view_binds_align_channel_pattern_and_effective_peek_by_position() {
    let repeated: &[u8] = &[0x01, 0x00];
    let distinct: &[u8] = &[0x00];
    let channels = [repeated, distinct, repeated];
    let channel_values: Vec<Vec<u8>> = channels.iter().map(|channel| channel.to_vec()).collect();
    let patterns = vec![
        LayoutPattern {
            image: vec![0x00, 0x01],
            free_count: -3,
        },
        LayoutPattern {
            image: Vec::new(),
            free_count: 0,
        },
        LayoutPattern {
            image: vec![0xFF; 5],
            free_count: i32::MAX,
        },
    ];
    let body = "body".to_string();
    let continuation = WaitingContinuation::create(
        &channel_values,
        &patterns,
        &body,
        true,
        BTreeSet::from([-1, 2, 7]),
    );
    let prepared = PreparedContinuationRecord::encode_waiting(&continuation);
    let record = prepared.record_view();
    let key = RhoPathV2::continuation_key(&channels, record.identity())
        .expect("a non-empty channel group has an RCI2 key");
    let view =
        OrderedReceiveView::admit_rci2_v2(&key, prepared.canonical(), prepared.field_bounds())
            .expect("an encoded record under its own key admits");
    assert_eq!(view.record(), record);
    assert_eq!(record.arity(), 3);
    assert_eq!(record.body(), body.stable_hash_bytes().as_slice());
    assert!(record.persist());
    assert!(record.coarse_peek());
    assert_eq!(record.raw_peeks().collect::<Vec<_>>(), vec![-1, 2, 7]);
    assert_eq!(record.pattern_at(3), None);
    let binds: Vec<_> = view.binds().collect();
    assert_eq!(binds.len(), 3);
    for (index, bind) in binds.iter().enumerate() {
        assert_eq!(bind.position().get(), u64::try_from(index).expect("index fits u64"));
        assert_eq!(bind.pattern(), patterns[index].stable_hash_bytes().as_slice());
        assert_eq!(record.pattern_at(index), Some(bind.pattern()));
        assert!(bind.channel_matches(channels[index]));
        assert_eq!(bind.free_count::<TrailingFreeCount>(), Some(patterns[index].free_count));
        assert_eq!(bind.effective_peek(), index == 2);
        assert_eq!(view.bind_at(bind.position()), Some(*bind));
    }
    assert!(!binds[0].channel_matches(distinct));
    assert!(!binds[0].channel_matches(&[0x01]));
    assert!(!binds[1].channel_matches(repeated));

    let two_patterns = WaitingContinuation::create(
        &channel_values[..2].to_vec(),
        &patterns[..2].to_vec(),
        &body,
        false,
        BTreeSet::new(),
    );
    let prepared_two = PreparedContinuationRecord::encode_waiting(&two_patterns);
    let wider_key = RhoPathV2::continuation_key(&channels, prepared_two.record_view().identity())
        .expect("a non-empty channel group has an RCI2 key");
    assert_eq!(
        OrderedReceiveView::admit_rci2_v2(
            &wider_key,
            prepared_two.canonical(),
            prepared_two.field_bounds()
        ),
        Err(OrderedReceiveError::ArityMismatch {
            channels: 3,
            patterns: 2,
        })
    );
    assert_eq!(
        OrderedReceiveView::admit_rci2_v2(
            &key,
            prepared_two.canonical(),
            prepared_two.field_bounds()
        ),
        Err(OrderedReceiveError::IdentityMismatch)
    );
}

#[test]
fn malformed_framing_is_refused() {
    let prepared =
        PreparedContinuationRecord::encode_waiting(&string_continuation(1, BTreeSet::new(), false));
    let canonical = prepared.canonical();
    let bounds = prepared.field_bounds();
    let channel: &[u8] = &[0x41, 0x00];
    let prefix = legacy_prefix(&[channel]);
    let path = legacy_record_path(&prefix, canonical);
    assert!(OrderedReceiveView::admit_legacy_v1(&path, prefix.len(), bounds).is_ok());
    assert_eq!(
        OrderedReceiveView::admit_legacy_v1(&path, path.len() + 1, bounds),
        Err(OrderedReceiveError::TruncatedRecord)
    );
    let legacy_cases: [(&[u8], RhoPathError); 10] = [
        (b"", RhoPathError::InvalidMagic),
        (b"RHP\x01", RhoPathError::TruncatedFrame),
        (b"RHP\x02\x02\x01\x41\x00\x00\x00\x01", RhoPathError::InvalidMagic),
        (b"RHP\x01\x01\x01\x41\x00\x00\x00\x01", RhoPathError::InvalidKind(1)),
        (b"RHP\x01\x02\x02\x41\x00\x00\x00\x01", RhoPathError::InvalidCollectionVersion(2)),
        (b"RHP\x01\x02\x01\x41\x42", RhoPathError::TruncatedFrame),
        (b"RHP\x01\x02\x01\x41\x00", RhoPathError::TruncatedFrame),
        (b"RHP\x01\x02\x01\x41\x00\x07\x00\x00\x00\x01", RhoPathError::InvalidEscape(0x07)),
        (b"RHP\x01\x02\x01\x41\x00\x00", RhoPathError::TruncatedFrame),
        (b"RHP\x01\x02\x01\x41\x00\x00\x00\x01\x7A", RhoPathError::TrailingBytes),
    ];
    for (malformed, expected) in legacy_cases {
        let path = legacy_record_path(malformed, canonical);
        assert_eq!(
            OrderedReceiveView::admit_legacy_v1(&path, malformed.len(), bounds),
            Err(OrderedReceiveError::Path(expected)),
            "prefix {malformed:?}"
        );
    }

    let identity = prepared.record_view().identity();
    let key = RhoPathV2::continuation_key(&[channel], identity)
        .expect("a non-empty channel group has an RCI2 key");
    assert!(OrderedReceiveView::admit_rci2_v2(&key, canonical, bounds).is_ok());
    let mut over_declared = key.clone();
    over_declared[6..14].copy_from_slice(&2u64.to_be_bytes());
    let mut under_declared = RhoPathV2::continuation_key(&[channel, channel], identity)
        .expect("a non-empty channel group has an RCI2 key");
    under_declared[6..14].copy_from_slice(&1u64.to_be_bytes());
    let mut trailing = key.clone();
    trailing.push(0x7A);
    let rci2_cases: [(&[u8], RhoPathError); 5] = [
        (b"RHP\x02\x02\x01", RhoPathError::InvalidMagic),
        (&over_declared, RhoPathError::InvalidEscape(0x01)),
        (&under_declared, RhoPathError::TruncatedFrame),
        (&key[..key.len() - 1], RhoPathError::TruncatedFrame),
        (&trailing, RhoPathError::TrailingBytes),
    ];
    for (malformed, expected) in rci2_cases {
        assert_eq!(
            OrderedReceiveView::admit_rci2_v2(malformed, canonical, bounds),
            Err(OrderedReceiveError::Path(expected)),
            "key {malformed:?}"
        );
    }
}

#[test]
fn malformed_field_bounds_and_peek_tails_are_refused() {
    let prepared = PreparedContinuationRecord::encode_waiting(&string_continuation(
        2,
        BTreeSet::from([1, 5]),
        true,
    ));
    let canonical = prepared.canonical();
    let ends = prepared.field_bounds().ends().to_vec();
    assert_eq!(ends.len(), 4);
    assert_eq!(admit_with_ends(canonical, &ends), Ok(()));
    let body_end = ends[2];
    let identity_end = ends[3];
    let peeks_start = body_end + 9;

    let bounds_cases: [(Vec<usize>, OrderedReceiveError); 9] = [
        (Vec::new(), OrderedReceiveError::MalformedFieldBounds),
        (vec![identity_end], OrderedReceiveError::MalformedFieldBounds),
        (ends[1..].to_vec(), OrderedReceiveError::MalformedFieldBounds),
        ([ends.as_slice(), &[identity_end]].concat(), OrderedReceiveError::MalformedFieldBounds),
        (vec![ends[1], ends[0], body_end, identity_end], OrderedReceiveError::MalformedFieldBounds),
        (vec![7, ends[1], body_end, identity_end], OrderedReceiveError::MalformedFieldBounds),
        (vec![ends[0], ends[1], identity_end, body_end], OrderedReceiveError::MalformedFieldBounds),
        (vec![ends[0], ends[1], body_end, identity_end + 4], OrderedReceiveError::PeekTailMismatch),
        (vec![ends[0], ends[1], body_end, identity_end - 4], OrderedReceiveError::PeekTailMismatch),
    ];
    for (malformed, expected) in bounds_cases {
        assert_eq!(admit_with_ends(canonical, &malformed), Err(expected), "ends {malformed:?}");
    }

    let rewritten = |offset: usize, bytes: &[u8]| {
        let mut record = canonical.to_vec();
        record[offset..offset + bytes.len()].copy_from_slice(bytes);
        record
    };
    let signed_words =
        |first: i32, second: i32| [first.to_le_bytes(), second.to_le_bytes()].concat();
    let record_cases: [(Vec<u8>, Result<(), OrderedReceiveError>); 12] = [
        (rewritten(0, &3u64.to_le_bytes()), Err(OrderedReceiveError::MalformedFieldBounds)),
        (rewritten(body_end, &[2]), Err(OrderedReceiveError::InvalidPersist(2))),
        (rewritten(body_end, &[0xFF]), Err(OrderedReceiveError::InvalidPersist(0xFF))),
        (rewritten(body_end + 1, &3u64.to_le_bytes()), Err(OrderedReceiveError::PeekTailMismatch)),
        (rewritten(peeks_start, &signed_words(5, 1)), Err(OrderedReceiveError::NonCanonicalPeeks)),
        (rewritten(peeks_start, &signed_words(3, 3)), Err(OrderedReceiveError::NonCanonicalPeeks)),
        (rewritten(peeks_start, &signed_words(1, -1)), Err(OrderedReceiveError::NonCanonicalPeeks)),
        (rewritten(peeks_start, &signed_words(-1, 1)), Ok(())),
        (canonical[..identity_end].to_vec(), Ok(())),
        (canonical[..identity_end - 1].to_vec(), Err(OrderedReceiveError::TruncatedRecord)),
        (canonical[..body_end + 5].to_vec(), Err(OrderedReceiveError::TruncatedRecord)),
        (canonical[..body_end].to_vec(), Err(OrderedReceiveError::TruncatedRecord)),
    ];
    for (record, expected) in record_cases {
        assert_eq!(admit_with_ends(&record, &ends), expected, "record {record:?}");
    }
    assert_eq!(admit_with_ends(&canonical[..7], &ends), Err(OrderedReceiveError::TruncatedRecord));
}

#[test]
fn peek_count_overflow_is_checked_without_wrap() {
    for body_len in [0usize, 4096] {
        let body = vec![0xA5; body_len];
        let (record, ends) = tail_record(&body, 0);
        assert_eq!(admit_with_ends(&record, &ends), Ok(()));
        let peeks_start = ends[1];
        let largest_fitting =
            u64::try_from((usize::MAX - peeks_start) / 4).expect("a peek count bound fits u64");
        let cases: [(u64, OrderedReceiveError); 7] = [
            (u64::MAX, OrderedReceiveError::PeekCountOverflow),
            (1u64 << 62, OrderedReceiveError::PeekCountOverflow),
            ((1u64 << 62) - 1, OrderedReceiveError::PeekCountOverflow),
            (largest_fitting + 1, OrderedReceiveError::PeekCountOverflow),
            (largest_fitting, OrderedReceiveError::PeekTailMismatch),
            (1u64 << 61, OrderedReceiveError::PeekTailMismatch),
            (1, OrderedReceiveError::PeekTailMismatch),
        ];
        for (count, expected) in cases {
            let (record, ends) = tail_record(&body, count);
            assert_eq!(
                admit_with_ends(&record, &ends),
                Err(expected),
                "body {body_len}, peek count {count}"
            );
        }
    }
}

#[test]
fn empty_aligned_view_has_no_bind_and_v2_empty_group_stays_refused() {
    let prepared = PreparedContinuationRecord::encode_waiting(&string_continuation(
        0,
        BTreeSet::from([0]),
        false,
    ));
    let prefix = legacy_prefix(&[]);
    let path = legacy_record_path(&prefix, prepared.canonical());
    let view = OrderedReceiveView::admit_legacy_v1(&path, prefix.len(), prepared.field_bounds())
        .expect("an empty group aligns with an empty pattern list");
    assert_eq!(view.record().arity(), 0);
    assert!(view.record().coarse_peek());
    assert_eq!(view.record().pattern_at(0), None);
    assert_eq!(view.binds().len(), 0);
    for query in IMAGES {
        assert_eq!(
            view.positions(ReceiveChannelQuery::LegacyV1(query))
                .expect("a legacy query reads a legacy view")
                .count(),
            0
        );
    }

    let single = legacy_prefix(&[IMAGES[1]]);
    let single_path = legacy_record_path(&single, prepared.canonical());
    assert_eq!(
        OrderedReceiveView::admit_legacy_v1(&single_path, single.len(), prepared.field_bounds()),
        Err(OrderedReceiveError::ArityMismatch {
            channels: 1,
            patterns: 0,
        })
    );

    let identity = prepared.record_view().identity();
    assert_eq!(RhoPathV2::continuation_key(&[], identity), Err(RhoPathError::EmptyChannelGroup));
    let key = empty_rci2_key(identity);
    assert_eq!(
        OrderedReceiveView::admit_rci2_v2(&key, prepared.canonical(), prepared.field_bounds()),
        Err(OrderedReceiveError::Path(RhoPathError::EmptyChannelGroup))
    );
}

#[test]
fn profile_queries_are_explicit() {
    let channels: [&[u8]; 5] = [IMAGES[3], IMAGES[2], IMAGES[3], IMAGES[0], IMAGES[4]];
    let prepared = PreparedContinuationRecord::encode_waiting(&string_continuation(
        channels.len(),
        BTreeSet::new(),
        false,
    ));
    let prefix = legacy_prefix(&channels);
    let path = legacy_record_path(&prefix, prepared.canonical());
    let legacy = OrderedReceiveView::admit_legacy_v1(&path, prefix.len(), prepared.field_bounds())
        .expect("an encoded record under its own legacy prefix admits");
    let key = RhoPathV2::continuation_key(&channels, prepared.record_view().identity())
        .expect("a non-empty channel group has an RCI2 key");
    let rci2 =
        OrderedReceiveView::admit_rci2_v2(&key, prepared.canonical(), prepared.field_bounds())
            .expect("an encoded record under its own key admits");
    assert_eq!(legacy.profile(), ReceiveChannelProfile::LegacyV1);
    assert_eq!(rci2.profile(), ReceiveChannelProfile::Rci2V2);
    assert_eq!(legacy.record(), rci2.record());
    for query in IMAGES.iter().copied().chain([ABSENT_IMAGE]) {
        let expected = oracle_positions(&channels, query);
        let legacy_positions: Vec<u64> = legacy
            .positions(ReceiveChannelQuery::LegacyV1(query))
            .expect("a legacy query reads a legacy view")
            .map(|position| position.get())
            .collect();
        let rci2_positions: Vec<u64> = rci2
            .positions(ReceiveChannelQuery::Rci2V2(query))
            .expect("an RCI2 query reads an RCI2 view")
            .map(|position| position.get())
            .collect();
        assert_eq!(legacy_positions, expected, "legacy query {query:?}");
        assert_eq!(rci2_positions, expected, "RCI2 query {query:?}");
        assert!(matches!(
            legacy.positions(ReceiveChannelQuery::Rci2V2(query)),
            Err(OrderedReceiveError::ProfileMismatch)
        ));
        assert!(matches!(
            rci2.positions(ReceiveChannelQuery::LegacyV1(query)),
            Err(OrderedReceiveError::ProfileMismatch)
        ));
    }
}

#[test]
fn legacy_store_views_return_exact_ordered_positions_for_repeated_and_prefix_related_channels() {
    let (_, hot_store) = raw_hot_store();
    let mut groups = 0usize;
    for (first_index, first) in IMAGES.iter().copied().enumerate() {
        for second in IMAGES.iter().copied().skip(first_index + 1) {
            for group in [[first, second, first], [first, first, second]] {
                let channels = raw_channels(&group);
                let continuation = numbered_continuation(3, groups);
                assert_eq!(hot_store.put_continuation(&channels, continuation), Some(true));
                let mut admitted = 0usize;
                let visit = hot_store.try_visit_resting_receive_views(
                    &channels,
                    &mut |orders, admission| {
                        let view = admission.expect("a stored record admits its legacy view");
                        assert_eq!(view.profile(), ReceiveChannelProfile::LegacyV1);
                        assert_eq!(orders.len(), 1);
                        assert_eq!(view.binds().len(), 3);
                        for query in IMAGES.iter().copied().chain([ABSENT_IMAGE]) {
                            let positions: Vec<u64> = view
                                .positions(ReceiveChannelQuery::LegacyV1(query))
                                .expect("a legacy query reads a legacy view")
                                .map(|position| position.get())
                                .collect();
                            assert_eq!(
                                positions,
                                oracle_positions(&group, query),
                                "group {group:?} query {query:?}"
                            );
                        }
                        let rci2 = view.positions(ReceiveChannelQuery::Rci2V2(first));
                        assert_eq!(rci2.err(), Some(OrderedReceiveError::ProfileMismatch));
                        admitted += 1;
                    },
                );
                assert_eq!(visit, RestingReceiveVisit::Visited(1));
                assert_eq!(admitted, 1);
                groups += 1;
            }
        }
    }
    assert_eq!(groups, 20);
}

#[test]
fn admission_refuses_mismatched_arity_before_any_position() {
    let (history, hot_store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x02]]);
    let wide = string_continuation(3, BTreeSet::new(), false);
    assert_eq!(hot_store.put_continuation(&channels, wide), Some(true));
    let mut put_admissions = Vec::new();
    let visit = hot_store.try_visit_resting_receive_views(&channels, &mut |_, admission| {
        put_admissions.push(admission.map(|_| ()));
    });
    assert_eq!(visit, RestingReceiveVisit::Visited(1));
    assert_eq!(put_admissions, vec![Err(OrderedReceiveError::ArityMismatch {
        channels: 2,
        patterns: 3,
    })]);

    let hydrated_channels = raw_channels(&[&[0x03]]);
    history.put_continuations(hydrated_channels.clone(), vec![string_continuation(
        2,
        BTreeSet::new(),
        true,
    )]);
    assert_eq!(hot_store.get_continuations(&hydrated_channels).len(), 1);
    let mut hydrated_admissions = Vec::new();
    let visit =
        hot_store.try_visit_resting_receive_views(&hydrated_channels, &mut |_, admission| {
            hydrated_admissions.push(admission.map(|_| ()));
        });
    assert_eq!(visit, RestingReceiveVisit::Visited(1));
    assert_eq!(hydrated_admissions, vec![Err(OrderedReceiveError::ArityMismatch {
        channels: 1,
        patterns: 2,
    })]);
}

#[test]
fn first_representative_and_exact_source_suffix_are_retained() {
    let (_, hot_store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x01, 0x00]]);
    let first = string_continuation(2, BTreeSet::from([1]), false);
    let mut alias = first.clone();
    alias.source.hash = Blake2b256Hash::from_bytes(vec![0x5A; 32]);
    let first_prepared = PreparedContinuationRecord::encode_waiting(&first);
    let alias_prepared = PreparedContinuationRecord::encode_waiting(&alias);
    assert_eq!(first_prepared.record_view().identity(), alias_prepared.record_view().identity());
    assert_ne!(
        first_prepared.record_view().source_suffix(),
        alias_prepared.record_view().source_suffix()
    );

    assert_eq!(hot_store.put_continuation(&channels, first.clone()), Some(true));
    assert_eq!(hot_store.put_continuation(&channels, alias), Some(false));
    let mut retained = Vec::new();
    let visit = hot_store.try_visit_resting_receive_views(&channels, &mut |_, admission| {
        let view = admission.expect("a stored record admits its legacy view");
        retained.push((view.record().identity().to_vec(), view.record().source_suffix().to_vec()));
    });
    assert_eq!(visit, RestingReceiveVisit::Visited(1));
    let frozen_source = bincode::serialize(&first.source).expect("a consume source serializes");
    assert_eq!(retained, vec![(first_prepared.record_view().identity().to_vec(), frozen_source)]);
    assert_eq!(hot_store.get_continuations(&channels), vec![first]);
}

#[test]
fn installed_continuations_are_not_visited_as_resting_records() {
    let (_, hot_store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01], &[0x02], &[0x01]]);
    let record = string_continuation(3, BTreeSet::from([-1, 2]), true);
    assert_eq!(hot_store.install_continuation(&channels, record.clone()), Some(()));
    let unloaded = hot_store.try_visit_resting_receive_views(&channels, &mut |_, _| {
        panic!("an installed continuation is not a resting record")
    });
    assert_eq!(unloaded, RestingReceiveVisit::Unloaded);

    assert_eq!(hot_store.get_continuations(&channels), vec![record.clone()]);
    let empty = hot_store.try_visit_resting_receive_views(&channels, &mut |_, _| {
        panic!("an installed continuation is not a resting record")
    });
    assert_eq!(empty, RestingReceiveVisit::Visited(0));

    assert_eq!(hot_store.put_continuation(&channels, record.clone()), Some(true));
    let mut identities = Vec::new();
    let visit = hot_store.try_visit_resting_receive_views(&channels, &mut |_, admission| {
        identities.push(
            admission
                .expect("a stored record admits its legacy view")
                .record()
                .identity()
                .to_vec(),
        );
    });
    assert_eq!(visit, RestingReceiveVisit::Visited(1));
    assert_eq!(identities, vec![prepared_identity(&record)]);
    assert_eq!(hot_store.get_continuations(&channels), vec![record.clone(), record]);
}

#[test]
fn visit_order_reconstructs_historical_order_and_unloaded_groups_are_not_hydrated() {
    let (history, hot_store) = raw_hot_store();
    let channels = raw_channels(&[&[0x01, 0x00], &[0x01]]);
    let continuations: Vec<StringContinuation> = (0..12)
        .map(|number| numbered_continuation(2, number))
        .collect();
    for continuation in &continuations {
        assert_eq!(hot_store.put_continuation(&channels, continuation.clone()), Some(true));
    }
    let mut visited: Vec<(u64, Vec<u8>, Vec<u8>)> = Vec::with_capacity(continuations.len());
    let visit = hot_store.try_visit_resting_receive_views(&channels, &mut |orders, admission| {
        let view = admission.expect("a stored record admits its legacy view");
        let [order] = orders else {
            panic!("a continuation record carries exactly one order, got {orders:?}");
        };
        let mut record = view.record().identity().to_vec();
        record.extend_from_slice(view.record().source_suffix());
        visited.push((*order, view.record().identity().to_vec(), record));
    });
    assert_eq!(visit, RestingReceiveVisit::Visited(continuations.len()));
    assert!(
        visited.windows(2).all(|pair| pair[0].2 < pair[1].2),
        "the visit follows strictly ascending canonical record bytes"
    );
    visited.sort_by_key(|(order, _, _)| std::cmp::Reverse(*order));
    let historical: Vec<Vec<u8>> = visited
        .into_iter()
        .map(|(_, identity, _)| identity)
        .collect();
    let expected: Vec<Vec<u8>> = hot_store
        .get_continuations(&channels)
        .iter()
        .map(prepared_identity)
        .collect();
    assert_eq!(historical, expected);
    let newest_first: Vec<Vec<u8>> = continuations.iter().rev().map(prepared_identity).collect();
    assert_eq!(expected, newest_first);

    let reads_before = history.continuation_reads();
    let unloaded_channels = raw_channels(&[&[0x07]]);
    history.put_continuations(unloaded_channels.clone(), vec![
        numbered_continuation(1, 0),
        numbered_continuation(1, 1),
    ]);
    let unloaded = hot_store.try_visit_resting_receive_views(&unloaded_channels, &mut |_, _| {
        panic!("an unloaded group is never hydrated by a visit")
    });
    assert_eq!(unloaded, RestingReceiveVisit::Unloaded);
    assert_eq!(history.continuation_reads(), reads_before);
    assert!(
        !hot_store
            .snapshot()
            .continuations
            .contains_key(&unloaded_channels)
    );

    assert_eq!(hot_store.get_continuations(&unloaded_channels).len(), 2);
    assert_eq!(history.continuation_reads(), reads_before + 1);
    let mut hydrated = 0usize;
    let visit =
        hot_store.try_visit_resting_receive_views(&unloaded_channels, &mut |_, admission| {
            admission.expect("a hydrated record admits its legacy view");
            hydrated += 1;
        });
    assert_eq!(visit, RestingReceiveVisit::Visited(2));
    assert_eq!(hydrated, 2);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn prepared_record_bytes_equal_the_frozen_encoders(
        patterns in proptest::collection::vec("[a-z\\x00]{0,12}", 0..6),
        body in "[a-z\\x00]{0,24}",
        persist in any::<bool>(),
        peeks in proptest::collection::btree_set(any::<i32>(), 0..6),
    ) {
        let channels: Vec<Vec<u8>> = (0..patterns.len())
            .map(|index| index.to_le_bytes().to_vec())
            .collect();
        let continuation =
            WaitingContinuation::create(&channels, &patterns, &body, persist, peeks.clone());
        let prepared = PreparedContinuationRecord::encode_waiting(&continuation);
        let canonical = prepared.canonical();
        let candidate_ordering = continuation.candidate_ordering_bytes();
        prop_assert_eq!(canonical, candidate_ordering.as_slice());
        let frozen = encode_continuations(&vec![continuation.clone()]);
        let record_length = u64::try_from(canonical.len()).expect("a record length fits u64");
        let frozen_header = [1u64.to_le_bytes(), record_length.to_le_bytes()].concat();
        prop_assert_eq!(&frozen[..16], frozen_header.as_slice());
        prop_assert_eq!(&frozen[16..], canonical);

        let view = prepared.record_view();
        prop_assert_eq!(prepared.field_bounds().ends().len(), patterns.len() + 2);
        prop_assert_eq!(view.arity(), patterns.len());
        for (index, pattern) in patterns.iter().enumerate() {
            let pattern_bytes = pattern.stable_hash_bytes();
            prop_assert_eq!(view.pattern_at(index), Some(pattern_bytes.as_slice()));
        }
        let body_bytes = body.stable_hash_bytes();
        prop_assert_eq!(view.body(), body_bytes.as_slice());
        prop_assert_eq!(view.persist(), persist);
        prop_assert_eq!(view.coarse_peek(), !peeks.is_empty());
        prop_assert_eq!(
            view.raw_peeks().collect::<Vec<_>>(),
            peeks.iter().copied().collect::<Vec<_>>()
        );
        let source = bincode::serialize(&continuation.source).expect("a consume source serializes");
        prop_assert_eq!(view.source_suffix(), source.as_slice());
        prop_assert_eq!(view.identity().len() + source.len(), canonical.len());
        prop_assert_eq!(view.identity(), &canonical[..view.identity().len()]);
    }
}
