//! # The `Par`-typed record differential — `Datum` and `WaitingContinuation`
//!
//! `rspace++`'s own `mod tests` proves the composition claim on the test-double
//! instantiation (`Datum<String>`, `WaitingContinuation<String, String>`). This
//! file proves it on the **production** one —
//! `RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>` — which
//! `rspace++` cannot name, because `models` depends on `rspace_plus_plus` and a
//! reverse edge would be a cycle.
//!
//! ## The claim
//!
//! `decode_datum::<A>` is *not* a whole-struct read. It is
//!
//! ```text
//!     (a, extent) = A::cold_decode_prefix(bytes)        // the machine
//!     (persist, source) = bincode::deserialize(bytes[extent..])   // the tail
//! ```
//!
//! and the claim that this equals `bincode::deserialize::<DatumDe<A>>(bytes)`
//! rests on two facts about bincode 1.3.3:
//!
//! 1. a derived struct is `deserialize_struct` →
//!    `deserialize_tuple(fields.len())` (`src/de/mod.rs:402-412`), and
//! 2. a tuple is `visit_seq` over its elements with **no framing**
//!    (`:293-330`).
//!
//! So a struct's encoding is exactly the concatenation of its fields', and
//! splitting it at a field boundary is byte-identical — **provided the extent
//! is exact**. That proviso is the whole risk: an extent off by one silently
//! reads `persist` out of the last byte of `a`, and `source` out of the middle
//! of nothing, producing a *wrong value* rather than an error. Hence the
//! oracles below.
//!
//! The oracle structs are declared here rather than imported because the
//! library's own twins are `#[cfg(test)]` and therefore invisible to an
//! integration test. They are **compiler-generated from the same field list in
//! the same order**, which is the only property the oracle needs.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    BindPattern, Bundle, Connective, CostTokenStack, EList, Expr, GUnforgeable, If,
    ListParWithRandom, Match, New, Par, ParWithRandom, Receive, Send, TaggedContinuation, Var,
};
use models::rust::channel_image::ChannelImageV2;
use models::rust::rholang::bincode_schema_tables::{
    CONNECTIVE_INSTANCE_VARIANTS, EXPR_INSTANCE_VARIANTS, FORM_VARIANTS, TAGGED_CONT_VARIANTS,
    UNF_INSTANCE_VARIANTS, VAR_INSTANCE_VARIANTS,
};
use models::rust::rholang::match_schema_tables::{ParCursor, RHO_MATCH_REGISTRY};
use models::rust::rspace_path::rho_path_v2_continuation_key;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{
    hash, hash_from_slice, StableHashSerialize,
};
use rspace_plus_plus::rspace::history::cold_store::{
    ContinuationsLeaf, DataLeaf, JoinsLeaf, PersistedData, PersistedDataKind,
};
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::hot_store_action::{HotStoreAction, InsertAction};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::ordered_receive::{
    OrderedReceiveError, OrderedReceiveView, PreparedContinuationRecord, RestingReceiveVisit,
};
use rspace_plus_plus::rspace::rho_path::{ReceiveChannelProfile, ReceiveChannelQuery};
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_continuations, decode_datum, decode_datums, encode_continuations, encode_datum,
    encode_datums, encode_joins,
};
use rspace_plus_plus::rspace::trace::event::{Consume, Produce};
use serde::{Deserialize, Serialize};

mod par_corpus;
use par_corpus as corpus;

// ---------------------------------------------------------------------------
// The oracles — the pre-conversion derived layouts, field for field
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct DatumOracle<A> {
    a: A,
    persist: bool,
    source: Produce,
}

#[derive(Deserialize)]
struct WaitingContinuationOracle<P, K> {
    patterns: Vec<P>,
    continuation: K,
    persist: bool,
    peeks: BTreeSet<i32>,
    source: Consume,
}

#[derive(Serialize, Deserialize)]
struct LegacyListParWithRandom {
    pars: Vec<Par>,
    random_state: Vec<u8>,
}

impl From<&ListParWithRandom> for LegacyListParWithRandom {
    fn from(value: &ListParWithRandom) -> Self {
        Self {
            pars: value.pars.clone(),
            random_state: value.random_state.clone(),
        }
    }
}

impl From<LegacyListParWithRandom> for ListParWithRandom {
    fn from(value: LegacyListParWithRandom) -> Self {
        Self {
            pars: value.pars,
            random_state: value.random_state,
            cost_region: None,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct LegacyTaggedContinuation {
    guard: Option<Par>,
    tagged_cont: Option<TaggedCont>,
}

impl From<&TaggedContinuation> for LegacyTaggedContinuation {
    fn from(value: &TaggedContinuation) -> Self {
        Self {
            guard: value.guard.clone(),
            tagged_cont: value.tagged_cont.clone(),
        }
    }
}

impl From<LegacyTaggedContinuation> for TaggedContinuation {
    fn from(value: LegacyTaggedContinuation) -> Self {
        Self {
            guard: value.guard,
            tagged_cont: value.tagged_cont,
            cost_region: None,
            clause_regions: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fixtures — the production instantiation
// ---------------------------------------------------------------------------

fn datum_fixtures() -> Vec<(&'static str, Datum<ListParWithRandom>)> {
    vec![
        ("loaded", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::all_par_fields(), corpus::deep_mixed_par(24)],
                random_state: vec![0xF1, 0xF2, 0xF3, 0xF4],
                cost_region: None,
            }
            .into(),
            persist: false,
            source: Produce::new(
                Blake2b256Hash::new(&[1, 2, 3]),
                Blake2b256Hash::new(&[4, 5, 6]),
                false,
            ),
        }),
        ("empty", Datum {
            a: ListParWithRandom {
                pars: vec![],
                random_state: vec![],
                cost_region: None,
            }
            .into(),
            persist: true,
            source: Produce::new(Blake2b256Hash::new(&[7]), Blake2b256Hash::new(&[8]), true),
        }),
        ("deep_only", Datum {
            a: ListParWithRandom {
                pars: vec![corpus::deep_par(48)],
                random_state: vec![9; 32],
                cost_region: None,
            }
            .into(),
            persist: false,
            source: Produce::new(
                Blake2b256Hash::new(&[10]),
                Blake2b256Hash::new(&[11]),
                false,
            ),
        }),
    ]
}

fn continuation_fixtures() -> Vec<(
    &'static str,
    WaitingContinuation<BindPattern, TaggedContinuation>,
)> {
    vec![
        ("par_body", WaitingContinuation {
            patterns: vec![
                BindPattern {
                    patterns: vec![corpus::all_par_fields()],
                    remainder: Some(Var {
                        var_instance: Some(VarInstance::FreeVar(0)),
                    }),
                    free_count: 2,
                },
                BindPattern {
                    patterns: vec![],
                    remainder: None,
                    free_count: 0,
                },
            ]
            .into(),
            continuation: TaggedContinuation {
                guard: Some(corpus::gint(41)),
                tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
                    body: Some(corpus::deep_mixed_par(20)),
                    random_state: vec![1, 2],
                })),
                cost_region: None,
                clause_regions: Vec::new(),
            }
            .into(),
            persist: true,
            peeks: BTreeSet::from([0, 2, -5]),
            source: Consume {
                channel_hashes: vec![Blake2b256Hash::new(&[13])],
                hash: Blake2b256Hash::new(&[16]),
                persistent: true,
            },
        }),
        ("scala_ref_empty_patterns", WaitingContinuation {
            patterns: Vec::<BindPattern>::new().into(),
            continuation: TaggedContinuation {
                guard: None,
                tagged_cont: Some(TaggedCont::ScalaBodyRef(-99)),
                cost_region: None,
                clause_regions: Vec::new(),
            }
            .into(),
            persist: false,
            peeks: BTreeSet::new(),
            source: Consume {
                channel_hashes: vec![],
                hash: Blake2b256Hash::new(&[19]),
                persistent: false,
            },
        }),
    ]
}

// ---------------------------------------------------------------------------
// The differential
// ---------------------------------------------------------------------------

#[test]
fn par_typed_datum_decode_matches_the_derived_oracle() {
    use rspace_plus_plus::rspace::data_index::RSpaceDataIndex;
    use rspace_plus_plus::rspace::rho_path::{RhoPathEncode, RhoPathV1};

    for (label, datum) in datum_fixtures() {
        let bytes = encode_datum(&datum);
        let oracle: DatumOracle<LegacyListParWithRandom> =
            bincode::deserialize(&bytes).expect("oracle must decode");
        let machine = decode_datum::<ListParWithRandom>(&bytes)
            .unwrap_or_else(|e| panic!("{label}: machine rejected a well-formed datum: {e}"));

        assert!(
            *machine.a == oracle.a.into(),
            "{label}: the machine and the derived oracle disagree on `a`"
        );
        assert_eq!(
            machine.persist, oracle.persist,
            "{label}: `persist` — an extent off by one reads this from inside `a`"
        );
        assert_eq!(machine.source, oracle.source, "{label}: `source`");
        assert_eq!(
            encode_datum(&machine),
            bytes,
            "{label}: canonical identity fixed point"
        );
        let path = RSpaceDataIndex::datum_path(&Par::default(), &machine);
        let view = RhoPathV1::parse_datum(&path).unwrap();
        assert_eq!(view.datum_identity(), bytes, "{label}: identity suffix");
        assert_eq!(
            view.datum_structure(),
            machine.a.rho_path_structure(),
            "{label}: structure"
        );
        assert_eq!(
            &path[view.channel_prefix_len()..],
            RhoPathV1::relative_datum_key(&machine.a.rho_path_structure(), &bytes),
            "{label}: exact channel-relative address",
        );
    }
}

#[test]
fn par_typed_continuation_decode_matches_the_derived_oracle() {
    for (label, cont) in continuation_fixtures() {
        assert_continuation_round_trip(label, &cont);
    }
}

fn assert_continuation_round_trip(
    label: &str,
    cont: &WaitingContinuation<BindPattern, TaggedContinuation>,
) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    let leaf = encode_continuations(&vec![cont.clone()]);
    let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
    let bytes = &encoded[0];
    let legacy_continuation = LegacyTaggedContinuation::from(cont.continuation.as_ref());
    let frozen = bincode::serialize(&(
        cont.patterns.as_ref(),
        &legacy_continuation,
        cont.persist,
        &cont.peeks,
        &cont.source,
    ))
    .unwrap();
    assert_eq!(*bytes, frozen, "{label}: frozen field sequence");
    for pattern in cont.patterns.iter() {
        assert_eq!(
            pattern.stable_hash_bytes(),
            bincode::serialize(pattern).unwrap(),
            "{label}: generated pattern encoder versus frozen field codec",
        );
    }
    assert_eq!(
        cont.continuation.stable_hash_bytes(),
        bincode::serialize(&legacy_continuation).unwrap(),
        "{label}: generated continuation encoder versus frozen field codec",
    );

    let oracle: WaitingContinuationOracle<BindPattern, LegacyTaggedContinuation> =
        bincode::deserialize(bytes).expect("oracle must decode");
    let mut decoded =
        decode_continuations::<BindPattern, TaggedContinuation>(&leaf).expect("machine");
    assert_eq!(decoded.len(), 1);
    let machine = decoded.pop().unwrap();

    assert_eq!(
        *machine.patterns, oracle.patterns,
        "{label}: oracle patterns"
    );
    assert_eq!(
        *machine.continuation,
        oracle.continuation.into(),
        "{label}: oracle body"
    );
    assert_eq!(
        machine.persist, oracle.persist,
        "{label}: oracle persistence"
    );
    assert_eq!(machine.peeks, oracle.peeks, "{label}: oracle raw peeks");
    assert_eq!(machine.source, oracle.source, "{label}: oracle source");
    assert_eq!(
        *machine.patterns, *cont.patterns,
        "{label}: original patterns"
    );
    assert_eq!(
        *machine.continuation, *cont.continuation,
        "{label}: original body"
    );
    assert_eq!(
        machine.persist, cont.persist,
        "{label}: original persistence"
    );
    assert_eq!(machine.peeks, cont.peeks, "{label}: original raw peeks");
    assert_eq!(machine.source, cont.source, "{label}: original source");
    assert_eq!(
        bincode::serialize(machine.patterns.as_ref()).unwrap(),
        bincode::serialize(cont.patterns.as_ref()).unwrap(),
        "{label}: exact pattern fields beyond semantic equality",
    );
    assert_eq!(
        bincode::serialize(machine.continuation.as_ref()).unwrap(),
        bincode::serialize(cont.continuation.as_ref()).unwrap(),
        "{label}: exact body, guard and random-state fields",
    );
    machine
}

fn frozen_identity(cont: &WaitingContinuation<BindPattern, TaggedContinuation>) -> Vec<u8> {
    let legacy_continuation = LegacyTaggedContinuation::from(cont.continuation.as_ref());
    bincode::serialize(&(
        cont.patterns.as_ref(),
        &legacy_continuation,
        cont.persist,
        &cont.peeks,
    ))
    .unwrap()
}

fn framed_bytes(bytes: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for &byte in bytes {
        result.push(byte);
        if byte == 0 {
            result.push(255);
        }
    }
    result.extend_from_slice(&[0, 0]);
    result
}

fn positional_continuation(
    fields: &[(u8, i32, bool)],
    peeks: BTreeSet<i32>,
) -> WaitingContinuation<BindPattern, TaggedContinuation> {
    let mut cont = continuation_fixtures().remove(0).1;
    cont.patterns = fields
        .iter()
        .enumerate()
        .map(|(position, (_, free_count, remainder))| BindPattern {
            patterns: vec![corpus::gint(i64::try_from(position).unwrap())],
            remainder: remainder.then_some(Var {
                var_instance: Some(VarInstance::FreeVar(i32::try_from(position).unwrap())),
            }),
            free_count: *free_count,
        })
        .collect::<Vec<_>>()
        .into();
    cont.peeks = peeks;
    cont
}

struct EmptyHistory;

impl HistoryReaderBase<Par, BindPattern, ListParWithRandom, TaggedContinuation> for EmptyHistory {
    fn get_data_canonical(&self, _channel: &Par) -> Vec<Datum<ListParWithRandom>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _channels: &[Par],
    ) -> Vec<WaitingContinuation<BindPattern, TaggedContinuation>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _channel: &Par) -> Vec<Vec<Par>> { Vec::new() }
}

type ParHotStore = Box<dyn HotStore<Par, BindPattern, ListParWithRandom, TaggedContinuation>>;

fn empty_par_hot_store() -> ParHotStore {
    HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), Box::new(EmptyHistory))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ReceiveViewObservation {
    identity: Vec<u8>,
    source_suffix: Vec<u8>,
    patterns: Vec<Vec<u8>>,
    free_counts: Vec<Option<i32>>,
    body: Vec<u8>,
    persist: bool,
    raw_peeks: Vec<i32>,
    coarse_peek: bool,
    effective_peeks: Vec<bool>,
    query_positions: Vec<Vec<u64>>,
}

impl ReceiveViewObservation {
    fn from_view(view: &OrderedReceiveView<'_>, query_images: &[Vec<u8>]) -> Self {
        let record = view.record();
        let mut patterns = Vec::with_capacity(record.arity());
        let mut free_counts = Vec::with_capacity(record.arity());
        let mut effective_peeks = Vec::with_capacity(record.arity());
        for bind in view.binds() {
            patterns.push(bind.pattern().to_vec());
            free_counts.push(bind.free_count::<BindPattern>());
            effective_peeks.push(bind.effective_peek());
        }
        Self {
            identity: record.identity().to_vec(),
            source_suffix: record.source_suffix().to_vec(),
            patterns,
            free_counts,
            body: record.body().to_vec(),
            persist: record.persist(),
            raw_peeks: record.raw_peeks().collect(),
            coarse_peek: record.coarse_peek(),
            effective_peeks,
            query_positions: query_images
                .iter()
                .map(|image| profile_positions(view, image))
                .collect(),
        }
    }
}

fn positional_channels(fields: &[(u8, i32, bool)]) -> Vec<Par> {
    fields
        .iter()
        .map(|(channel, _, _)| corpus::gint(i64::from(*channel)))
        .collect()
}

fn legacy_identities(channels: &[Par]) -> Vec<Vec<u8>> {
    channels.iter().map(Par::stable_hash_bytes).collect()
}

fn rci2_images(channels: &[Par]) -> Vec<Vec<u8>> {
    let mut images = Vec::with_capacity(channels.len());
    for channel in channels {
        let image = ChannelImageV2::from_par(channel).expect("an RCI2 channel image");
        images.push(image.as_bytes().to_vec());
    }
    images
}

fn profile_query(profile: ReceiveChannelProfile, image: &[u8]) -> ReceiveChannelQuery<'_> {
    match profile {
        ReceiveChannelProfile::LegacyV1 => ReceiveChannelQuery::LegacyV1(image),
        ReceiveChannelProfile::Rci2V2 => ReceiveChannelQuery::Rci2V2(image),
    }
}

fn foreign_profile_query(profile: ReceiveChannelProfile, image: &[u8]) -> ReceiveChannelQuery<'_> {
    match profile {
        ReceiveChannelProfile::LegacyV1 => ReceiveChannelQuery::Rci2V2(image),
        ReceiveChannelProfile::Rci2V2 => ReceiveChannelQuery::LegacyV1(image),
    }
}

fn profile_positions(view: &OrderedReceiveView<'_>, image: &[u8]) -> Vec<u64> {
    view.positions(profile_query(view.profile(), image))
        .expect("a query in the view's own profile reads it")
        .map(|position| position.get())
        .collect()
}

fn matching_positions<T: PartialEq>(members: &[T], query: &T) -> Vec<u64> {
    members
        .iter()
        .enumerate()
        .filter(|(_, member)| *member == query)
        .map(|(position, _)| u64::try_from(position).expect("a position fits u64"))
        .collect()
}

fn nested_locally_free_list(locally_free: Vec<u8>) -> Par {
    Par::default().with_exprs(vec![Expr {
        expr_instance: Some(ExprInstance::EListBody(EList {
            ps: vec![corpus::gint(1)],
            locally_free,
            connective_used: false,
            remainder: None,
        })),
    }])
}

fn assert_prepared_record(
    label: &str,
    continuation: &WaitingContinuation<BindPattern, TaggedContinuation>,
    prepared: &PreparedContinuationRecord,
) {
    let leaf = encode_continuations(&vec![continuation.clone()]);
    let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("the leaf decodes");
    assert_eq!(encoded, [prepared.canonical()], "{label}: canonical");
    let record = prepared.record_view();
    let arity = continuation.patterns.len();
    let field_count = arity.checked_add(2).expect("a field count fits usize");
    let bounds = prepared.field_bounds().ends();
    assert_eq!(bounds.len(), field_count, "{label}: bounds");
    assert_eq!(record.arity(), arity, "{label}: arity");
    assert_eq!(record.pattern_at(arity), None, "{label}: past the arity");
    let body = continuation.continuation.stable_hash_bytes();
    assert_eq!(record.body(), body, "{label}: body");
    assert_eq!(record.persist(), continuation.persist, "{label}: persist");
    let raw_peeks: Vec<i32> = record.raw_peeks().collect();
    let expected_peeks: Vec<i32> = continuation.peeks.iter().copied().collect();
    assert_eq!(raw_peeks, expected_peeks, "{label}: raw peeks");
    assert_eq!(
        record.coarse_peek(),
        !continuation.peeks.is_empty(),
        "{label}: coarse peek"
    );
    let source = bincode::serialize(&continuation.source).expect("a source serializes");
    assert_eq!(record.source_suffix(), source, "{label}: source suffix");
}

fn assert_view_alignment(
    label: &str,
    view: &OrderedReceiveView<'_>,
    continuation: &WaitingContinuation<BindPattern, TaggedContinuation>,
    prepared: &PreparedContinuationRecord,
    channel_images: &[Vec<u8>],
) {
    let record = view.record();
    assert_eq!(record, prepared.record_view(), "{label}: record");
    assert_eq!(view.binds().len(), channel_images.len(), "{label}: arity");
    let binds = view
        .binds()
        .zip(continuation.patterns.iter())
        .zip(channel_images);
    for (index, ((bind, pattern), image)) in binds.enumerate() {
        let position = u64::try_from(index).expect("a bind index fits u64");
        let pattern_bytes = pattern.stable_hash_bytes();
        let peek = i32::try_from(index).is_ok_and(|key| continuation.peeks.contains(&key));
        assert_eq!(bind.position().get(), position, "{label}: bind position");
        assert_eq!(view.bind_at(bind.position()), Some(bind), "{label}: bind");
        assert_eq!(bind.channel_frame(), framed_bytes(image), "{label}: frame");
        assert!(bind.channel_matches(image), "{label}: channel image");
        assert_eq!(bind.pattern(), pattern_bytes, "{label}: pattern");
        assert_eq!(
            bind.free_count::<BindPattern>(),
            Some(pattern.free_count),
            "{label}: free count"
        );
        assert_eq!(bind.effective_peek(), peek, "{label}: effective peek");
    }
    for image in channel_images {
        let foreign = view.positions(foreign_profile_query(view.profile(), image));
        assert_eq!(
            foreign.err(),
            Some(OrderedReceiveError::ProfileMismatch),
            "{label}: foreign profile"
        );
    }
}

fn observe_legacy_store_view(
    label: &str,
    continuation: &WaitingContinuation<BindPattern, TaggedContinuation>,
    channels: &[Par],
    query_identities: &[Vec<u8>],
) -> ReceiveViewObservation {
    let prepared = PreparedContinuationRecord::encode_waiting(continuation);
    let identities = legacy_identities(channels);
    let hot_store = empty_par_hot_store();
    let inserted = hot_store.put_continuation(channels, continuation.clone());
    assert_eq!(inserted, Some(true), "{label}: fresh insertion");
    let mut observed = None;
    let visit = hot_store.try_visit_resting_receive_views(channels, &mut |orders, admission| {
        let view = admission.expect("a stored continuation admits its legacy view");
        assert_eq!(view.profile(), ReceiveChannelProfile::LegacyV1);
        assert_eq!(orders.len(), 1, "{label}: one historical order");
        assert_view_alignment(label, &view, continuation, &prepared, &identities);
        observed = Some(ReceiveViewObservation::from_view(&view, query_identities));
    });
    assert_eq!(visit, RestingReceiveVisit::Visited(1), "{label}: visit");
    observed.expect("the visitor observed the stored continuation")
}

fn observe_receive_views(
    label: &str,
    continuation: &WaitingContinuation<BindPattern, TaggedContinuation>,
    channels: &[Par],
    queries: &[Par],
) -> ReceiveViewObservation {
    let prepared = PreparedContinuationRecord::encode_waiting(continuation);
    assert_prepared_record(label, continuation, &prepared);
    let images = rci2_images(channels);
    let key = rho_path_v2_continuation_key(channels, prepared.record_view().identity())
        .expect("a non-empty Par channel group has an RCI2 continuation key");
    let view =
        OrderedReceiveView::admit_rci2_v2(&key, prepared.canonical(), prepared.field_bounds())
            .expect("a prepared record admits under its own RCI2 key");
    assert_eq!(view.profile(), ReceiveChannelProfile::Rci2V2);
    assert_view_alignment(label, &view, continuation, &prepared, &images);
    let rci2 = ReceiveViewObservation::from_view(&view, &rci2_images(queries));
    let query_identities = legacy_identities(queries);
    let legacy = observe_legacy_store_view(label, continuation, channels, &query_identities);
    assert_eq!(legacy, rci2, "{label}: legacy and RCI2 observations");
    rci2
}

fn assert_ordered_receive_correspondence(fields: &[(u8, i32, bool)], peeks: BTreeSet<i32>) {
    let original = positional_continuation(fields, peeks);
    let decoded = assert_continuation_round_trip("ordered receive", &original);
    let channels = positional_channels(fields);
    let queries: Vec<Par> = (0..=4_u8)
        .map(|query| corpus::gint(i64::from(query)))
        .collect();
    let observation = observe_receive_views("ordered receive", &original, &channels, &queries);
    assert_eq!(observation.identity, frozen_identity(&original));
    assert_eq!(observation.patterns.len(), fields.len());
    for (position, (_, free_count, _)) in fields.iter().enumerate() {
        let peek_key = i32::try_from(position).expect("a fixture position fits i32");
        assert_eq!(observation.free_counts[position], Some(*free_count));
        assert_eq!(decoded.patterns[position].free_count, *free_count);
        assert_eq!(
            observation.effective_peeks[position],
            decoded.peeks.contains(&peek_key)
        );
    }
    assert_eq!(observation.coarse_peek, !decoded.peeks.is_empty());
    assert_eq!(observation.query_positions.len(), queries.len());
    let channel_ids: Vec<u8> = fields.iter().map(|(channel, _, _)| *channel).collect();
    for (query, positions) in (0..=4_u8).zip(&observation.query_positions) {
        assert_eq!(*positions, matching_positions(&channel_ids, &query));
    }
}

#[test]
fn ordered_receive_signed_fields_survive_repeated_channel_permutations() {
    for channels in [[0, 1, 0], [0, 0, 1]] {
        let fields = [
            (channels[0], i32::MIN, true),
            (channels[1], 0, false),
            (channels[2], i32::MAX, true),
        ];
        assert_ordered_receive_correspondence(&fields, BTreeSet::from([-1, 2, 7]));
    }
}

proptest::proptest! {
    #[test]
    fn ordered_receive_codec_preserves_arbitrary_signed_fields(
        fields in proptest::collection::vec((0..4_u8, proptest::num::i32::ANY, proptest::bool::ANY), 1..65),
        peeks in proptest::collection::btree_set(proptest::num::i32::ANY, 0..17),
    ) {
        assert_ordered_receive_correspondence(&fields, peeks);
    }
}

#[test]
fn ordered_receive_raw_peeks_and_source_suffix_are_not_erased() {
    let fields = [(0, i32::MIN, true), (1, 0, false), (0, i32::MAX, true)];
    let channels = positional_channels(&fields);
    let queries = [corpus::gint(0), corpus::gint(1)];
    let original = positional_continuation(&fields, BTreeSet::from([-1, 7]));
    assert_continuation_round_trip("invalid-only raw peeks", &original);
    let observed = observe_receive_views("raw peeks", &original, &channels, &queries);
    assert_eq!(observed.raw_peeks, [-1, 7]);
    assert!(observed.coarse_peek);
    assert_eq!(observed.effective_peeks, [false, false, false]);
    assert_eq!(observed.query_positions, [vec![0_u64, 2], vec![1]]);

    let mut erased = original.clone();
    erased.peeks.clear();
    let observed_erased = observe_receive_views("erased peeks", &erased, &channels, &queries);
    assert!(observed_erased.raw_peeks.is_empty());
    assert!(!observed_erased.coarse_peek);
    assert_ne!(observed_erased.identity, observed.identity);

    let mut alias = original.clone();
    alias.source.hash = Blake2b256Hash::new(&[99]);
    let observed_alias = observe_receive_views("source alias", &alias, &channels, &queries);
    assert_eq!(observed_alias.identity, observed.identity);
    assert_ne!(observed_alias.source_suffix, observed.source_suffix);
    for (record, observation) in [(&original, &observed), (&alias, &observed_alias)] {
        let source = bincode::serialize(&record.source).expect("a consume source serializes");
        assert_eq!(observation.source_suffix, source);
        assert_eq!(observation.identity, frozen_identity(record));
        assert_continuation_round_trip("source alias", record);
    }
    assert_ne!(
        encode_continuations(&vec![original]),
        encode_continuations(&vec![alias]),
    );
}

#[test]
fn ordered_receive_fixture_detects_erased_and_shifted_fields() {
    let fields = [(0, i32::MIN, true), (1, 0, false), (0, i32::MAX, true)];
    let channels = positional_channels(&fields);
    let queries = [corpus::gint(0), corpus::gint(1)];
    let original = positional_continuation(&fields, BTreeSet::from([-1, 2, 7]));
    let baseline = observe_receive_views("baseline", &original, &channels, &queries);
    let free_counts = [Some(i32::MIN), Some(0), Some(i32::MAX)];
    assert_eq!(baseline.free_counts, free_counts);
    assert_eq!(baseline.raw_peeks, [-1, 2, 7]);
    assert_eq!(baseline.effective_peeks, [false, false, true]);
    for mutation in 0..7 {
        let mut mutant = original.clone();
        let mut expected = baseline.clone();
        match mutation {
            0 => {
                std::sync::Arc::make_mut(&mut mutant.patterns).swap(0, 2);
                expected.patterns.swap(0, 2);
                expected.free_counts.swap(0, 2);
            }
            1 => {
                std::sync::Arc::make_mut(&mut mutant.patterns)[0]
                    .patterns
                    .clear();
                expected.patterns[0] = mutant.patterns[0].stable_hash_bytes();
            }
            2 => {
                std::sync::Arc::make_mut(&mut mutant.patterns)[0].remainder = None;
                expected.patterns[0] = mutant.patterns[0].stable_hash_bytes();
            }
            3 => {
                std::sync::Arc::make_mut(&mut mutant.patterns)[0].free_count = 0;
                expected.patterns[0] = mutant.patterns[0].stable_hash_bytes();
                expected.free_counts[0] = Some(0);
            }
            4 => {
                mutant.peeks.remove(&2);
                mutant.peeks.insert(0);
                expected.raw_peeks = vec![-1, 0, 7];
                expected.effective_peeks = vec![true, false, false];
            }
            5 => {
                mutant.persist = false;
                expected.persist = false;
            }
            6 => {
                std::sync::Arc::make_mut(&mut mutant.continuation).guard = None;
                expected.body = mutant.continuation.stable_hash_bytes();
            }
            _ => unreachable!(),
        }
        expected.identity = frozen_identity(&mutant);
        let observed = observe_receive_views("field mutation", &mutant, &channels, &queries);
        assert_eq!(observed, expected, "mutation {mutation}");
        assert_ne!(observed.identity, baseline.identity, "mutation {mutation}");
        assert_continuation_round_trip("field mutation", &mutant);
    }
}

fn corpus_receive_patterns() -> Vec<(String, BindPattern)> {
    let corpus_patterns = corpus::bind_pattern_corpus();
    let fixtures = continuation_fixtures();
    let capacity = fixtures
        .iter()
        .map(|(_, continuation)| continuation.patterns.len())
        .try_fold(corpus_patterns.len(), usize::checked_add)
        .expect("the receive pattern count fits usize");
    let mut patterns = Vec::with_capacity(capacity);
    for (name, pattern) in corpus_patterns {
        patterns.push((format!("corpus::{name}"), pattern));
    }
    for (fixture, continuation) in &fixtures {
        for (index, pattern) in continuation.patterns.iter().enumerate() {
            patterns.push((format!("fixture::{fixture}::{index}"), pattern.clone()));
        }
    }
    patterns
}

fn corpus_receive_bodies() -> Vec<(String, TaggedContinuation)> {
    let corpus_bodies = corpus::tagged_continuation_corpus();
    let fixtures = continuation_fixtures();
    let capacity = corpus_bodies
        .len()
        .checked_add(1)
        .and_then(|count| count.checked_add(fixtures.len()))
        .expect("the receive body count fits usize");
    let mut bodies = Vec::with_capacity(capacity);
    for (name, body) in corpus_bodies {
        bodies.push((format!("corpus::{name}"), body));
    }
    let cost_annotated_name = String::from("corpus::cost_annotated_tagged_continuation");
    let cost_annotated = corpus::cost_annotated_tagged_continuation();
    bodies.push((cost_annotated_name, cost_annotated));
    for (fixture, continuation) in fixtures {
        let body = continuation.continuation.as_ref().clone();
        bodies.push((format!("fixture::{fixture}"), body));
    }
    bodies
}

#[test]
fn ordered_receive_view_admits_every_corpus_pattern_and_continuation() {
    const PATTERN_COUNT: usize = 8;
    const BODY_COUNT: usize = 7;
    const COVERED_NAME_COUNT: usize = 56;
    const LEGACY_ORACLE_NAME_COUNT: usize = 48;

    let patterns = corpus_receive_patterns();
    let bodies = corpus_receive_bodies();
    assert_eq!(patterns.len(), PATTERN_COUNT);
    assert_eq!(bodies.len(), BODY_COUNT);
    let product = patterns
        .len()
        .checked_mul(bodies.len())
        .expect("the corpus product fits usize");
    assert_eq!(product, COVERED_NAME_COUNT);
    let channels = vec![corpus::gint(1), corpus::gint(2), corpus::gint(1)];
    let queries = [corpus::gint(1), corpus::gint(2), corpus::gint(3)];
    let peeks = BTreeSet::from([-1, 1, 7]);
    let expected_positions = [vec![0_u64, 2], vec![1], vec![]];
    let mut covered = Vec::with_capacity(product);
    let mut legacy_oracle_names = Vec::with_capacity(product);
    for (pattern_name, pattern) in &patterns {
        let receive_patterns = vec![pattern.clone(); channels.len()];
        let free_counts = vec![Some(pattern.free_count); channels.len()];
        for (body_name, body) in &bodies {
            let name = format!("{pattern_name} with {body_name}");
            let continuation = WaitingContinuation::create(
                &channels,
                &receive_patterns,
                body,
                covered.len().is_multiple_of(2),
                peeks.clone(),
            );
            let observed = observe_receive_views(&name, &continuation, &channels, &queries);
            assert_eq!(observed.free_counts, free_counts, "{name}");
            assert_eq!(observed.effective_peeks, [false, true, false], "{name}");
            assert_eq!(observed.query_positions, expected_positions, "{name}");
            match (&body.cost_region, body.clause_regions.as_slice()) {
                (None, []) => {
                    assert_continuation_round_trip(&name, &continuation);
                    legacy_oracle_names.push(name.clone());
                }
                (Some(_), _) | (None, [_, ..]) => {
                    let leaf = encode_continuations(&vec![continuation.clone()]);
                    let decoded = decode_continuations::<BindPattern, TaggedContinuation>(&leaf)
                        .expect("a cost-annotated continuation decodes");
                    assert_eq!(decoded, [continuation], "{name}");
                }
            }
            println!("  covered {name}");
            covered.push(name);
        }
    }
    let distinct: BTreeSet<&String> = covered.iter().collect();
    assert_eq!(distinct.len(), covered.len(), "covered names are distinct");
    assert_eq!(covered.len(), COVERED_NAME_COUNT);
    assert_eq!(legacy_oracle_names.len(), LEGACY_ORACLE_NAME_COUNT);
    let covered_count = covered.len();
    let legacy_count = legacy_oracle_names.len();
    println!("  receive corpus: {covered_count} names, {legacy_count} legacy-oracle names");
}

#[test]
fn ordered_receive_legacy_store_views_match_typed_par_positions() {
    let five = corpus::gint(5);
    let mut rooted_five = five.clone();
    rooted_five.replace_locally_free(vec![9]);
    let nested_one = nested_locally_free_list(vec![1]);
    let nested_two = nested_locally_free_list(vec![2]);
    assert_eq!(rooted_five, five, "typed root locally_free");
    assert_eq!(
        rooted_five.stable_hash_bytes(),
        five.stable_hash_bytes(),
        "V1 root locally_free"
    );
    assert_ne!(nested_one, nested_two, "typed nested locally_free");
    assert_ne!(
        nested_one.stable_hash_bytes(),
        nested_two.stable_hash_bytes(),
        "V1 nested locally_free"
    );
    let channels = [
        five.clone(),
        corpus::tagged(3),
        rooted_five,
        nested_one.clone(),
        corpus::nil(),
        nested_two,
        five,
        nested_one,
    ];
    let absent_query = corpus::gint(6);
    let queries: Vec<Par> = channels.iter().chain([&absent_query]).cloned().collect();
    let fields = vec![(0_u8, 1, true); channels.len()];
    let continuation = positional_continuation(&fields, BTreeSet::from([1, 4]));
    let query_identities = legacy_identities(&queries);
    let label = "typed positions";
    let observed = observe_legacy_store_view(label, &continuation, &channels, &query_identities);
    assert_eq!(observed.query_positions.len(), queries.len());
    let identities = legacy_identities(&channels);
    let per_query = queries
        .iter()
        .zip(&query_identities)
        .zip(&observed.query_positions);
    for (index, ((query, query_identity), view_positions)) in per_query.enumerate() {
        let byte_positions = matching_positions(&identities, query_identity);
        let typed_positions = matching_positions(&channels, query);
        assert_eq!(*view_positions, byte_positions, "query {index}");
        assert_eq!(typed_positions, byte_positions, "query {index}");
        println!("  query {index}: view {view_positions:?}, typed {typed_positions:?}");
    }
}

/// The record-level rejection set. Truncating a `Datum<ListParWithRandom>` at
/// every offset walks the boundary between "the prefix consumed everything" and
/// "the tail ran out", which is exactly where a wrong extent hides.
#[test]
fn truncated_par_typed_records_are_rejected_by_both() {
    let mut checked = 0usize;
    let mut rejections = 0usize;

    for (label, datum) in datum_fixtures() {
        let bytes = encode_datum(&datum);
        for cut in 0..bytes.len() {
            let slice = &bytes[..cut];
            let oracle =
                bincode::deserialize::<DatumOracle<LegacyListParWithRandom>>(slice).is_ok();
            let machine = decode_datum::<ListParWithRandom>(slice).is_ok();
            assert_eq!(
                oracle, machine,
                "{label} truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
            if !machine {
                rejections += 1;
            }
        }
    }

    for (label, cont) in continuation_fixtures() {
        let leaf = encode_continuations(&vec![cont]);
        let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
        let bytes = &encoded[0];
        for cut in 0..bytes.len() {
            let slice = &bytes[..cut];
            let oracle = bincode::deserialize::<
                WaitingContinuationOracle<BindPattern, LegacyTaggedContinuation>,
            >(slice)
            .is_ok();
            // Re-frame the truncated record as a one-element leaf so it goes
            // through the same public entry point production uses.
            let framed = bincode::serialize(&vec![slice.to_vec()]).expect("frame");
            let machine = decode_continuations::<BindPattern, TaggedContinuation>(&framed).is_ok();
            assert_eq!(
                oracle, machine,
                "{label} truncated at {cut}: oracle ok={oracle}, machine ok={machine}"
            );
            checked += 1;
            if !machine {
                rejections += 1;
            }
        }
    }

    assert!(
        checked > 5_000,
        "ANTI-VACUITY: only {checked} truncations tried"
    );
    assert!(
        rejections > checked / 2,
        "ANTI-VACUITY: only {rejections} of {checked} truncated records were rejected"
    );
    println!("  record truncation: {checked} prefixes, {rejections} joint rejections");
}

/// A `Datum` whose payload is deeper than the derived decoder survives.
///
/// This is the point of the whole exercise stated as one assertion: the byte
/// string below is one that `rspace_importer` could commit to LMDB today, and
/// that the pre-conversion read path would abort on — permanently, on every
/// restart, on every peer that synced it.
#[test]
fn a_datum_too_deep_for_the_derived_decoder_still_reads_back() {
    // 4,096 levels. At the measured 28,362 B/level (debug) the derived decoder
    // would need ~110 MiB of native stack; this thread gets 256 KiB.
    let datum = Datum {
        a: ListParWithRandom {
            pars: vec![corpus::deep_par(4096)],
            random_state: vec![1, 2, 3],
            cost_region: None,
        }
        .into(),
        persist: true,
        source: Produce::new(Blake2b256Hash::new(&[1]), Blake2b256Hash::new(&[2]), true),
    };
    let bytes = encode_datum(&datum);
    models::rust::rholang::par_children::dismantle_all(
        std::sync::Arc::try_unwrap(datum.a)
            .map(|a| a.pars)
            .unwrap_or_default(),
    );

    std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let leaf = bincode::serialize(&vec![bytes]).expect("frame as a cold-store leaf");
            let datums = decode_datums::<ListParWithRandom>(&leaf)
                .expect("the cold-store read path must survive a depth-4096 datum");
            assert_eq!(datums.len(), 1);
            assert!(datums[0].persist, "the tail must still be read correctly");

            // The nesting really came back. Inspect the canonical Expr body in
            // one byte-native pass: decoding every successively shorter suffix
            // would make this oracle quadratic and says nothing more about the
            // cold-store reader.
            let mut depth = None;
            datums[0].a.pars[0].visit_encoded::<Expr>(|entry| {
                assert!(depth.is_none(), "fixture has exactly one Expr endpoint");
                assert_eq!(entry.multiplicity().get(), 1);
                depth = entry.canonical_unary_elist_depth();
            });
            assert_eq!(
                depth,
                Some(4096),
                "the DECODED datum must carry the nesting"
            );

            let datums = datums;
            for d in datums {
                if let Ok(a) = std::sync::Arc::try_unwrap(d.a) {
                    models::rust::rholang::par_children::dismantle_all(a.pars);
                }
            }
        })
        .expect("spawn decoder")
        .join()
        .expect("the cold-store read path overflowed a 256 KiB stack at depth 4096");
}

/// The Step-A goldens decode through the new path to the same values the
/// oracle produces — tying the pre-change byte pins to the post-change reader.
#[test]
fn step_a_golden_shapes_read_back_through_the_new_path() {
    let datums = vec![datum_fixtures().remove(0).1];
    let leaf = encode_datums(&datums);
    let decoded = decode_datums::<ListParWithRandom>(&leaf).expect("machine");
    assert_eq!(decoded.len(), 1);

    let encoded: Vec<Vec<u8>> = bincode::deserialize(&leaf).expect("leaf");
    let oracle: DatumOracle<LegacyListParWithRandom> =
        bincode::deserialize(&encoded[0]).expect("oracle");
    assert!(*decoded[0].a == oracle.a.into());

    // And the `Par` inside is the same one the oracle sees, field for field.
    let par: &Par = &decoded[0].a.pars[0];
    assert!(
        par.family_cardinality::<models::rhoapi::Send>() != 0
            && par.family_cardinality::<Expr>() != 0
            && par.family_cardinality::<models::rhoapi::If>() != 0
    );
    assert!(par.family_cardinality::<models::rhoapi::GUnforgeable>() != 0);
}

const PAYLOAD_UNREACHABLE_MESSAGES: [(&str, &str); 4] = [
    (
        "DeployId",
        "no field of Par, ListParWithRandom, BindPattern or TaggedContinuation, nor of a message \
         they reach, has this type; the unforgeable deploy-id name of a Par is the separate \
         message GDeployId",
    ),
    (
        "DeployerId",
        "no field of Par, ListParWithRandom, BindPattern or TaggedContinuation, nor of a message \
         they reach, has this type; the unforgeable deployer-id name of a Par is the separate \
         message GDeployerId",
    ),
    (
        "ListBindPatterns",
        "no field of Par, ListParWithRandom, BindPattern or TaggedContinuation, nor of a message \
         they reach, has this type; the hot store keeps the patterns of a continuation as a \
         Vec<BindPattern>, never as this list message",
    ),
    (
        "PCost",
        "no field of Par, ListParWithRandom, BindPattern or TaggedContinuation, nor of a message \
         they reach, has this type; it is the cost of performed operations, which no RSpace \
         payload holds",
    ),
];

#[derive(Debug)]
struct ExpectedCanonicalRecord {
    label: String,
    count: u64,
    encoded_persisted_data: Vec<u8>,
}

fn persisted_kind_label(kind: PersistedDataKind) -> &'static str {
    match kind {
        PersistedDataKind::Data => "data",
        PersistedDataKind::Continuations => "continuations",
        PersistedDataKind::Joins => "joins",
    }
}

fn stored_data_values() -> Vec<ListParWithRandom> {
    let wrapped_pars = corpus::par_corpus()
        .into_iter()
        .map(|(_, par)| par)
        .chain([corpus::all_par_fields(), corpus::cost_annotated_par()])
        .map(|par| ListParWithRandom {
            pars: vec![par],
            random_state: Vec::new(),
            cost_region: None,
        });
    corpus::list_par_with_random_corpus()
        .into_iter()
        .map(|(_, value)| value)
        .chain([corpus::cost_annotated_list_par_with_random()])
        .chain(wrapped_pars)
        .collect()
}

fn stored_continuation_values() -> Vec<TaggedContinuation> {
    corpus::tagged_continuation_corpus()
        .into_iter()
        .map(|(_, value)| value)
        .chain([corpus::cost_annotated_tagged_continuation()])
        .collect()
}

fn continuation_body(continuation: &TaggedContinuation) -> Option<&Par> {
    match &continuation.tagged_cont {
        Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(body), ..
        })) => Some(body),
        _ => None,
    }
}

fn record_components_of(
    par: &Par,
    types: &mut BTreeSet<&'static str>,
    variants: &mut BTreeSet<String>,
) {
    corpus::record_components::<Send>(par, types, variants);
    corpus::record_components::<Receive>(par, types, variants);
    corpus::record_components::<New>(par, types, variants);
    corpus::record_components::<Expr>(par, types, variants);
    corpus::record_components::<Match>(par, types, variants);
    corpus::record_components::<GUnforgeable>(par, types, variants);
    corpus::record_components::<Bundle>(par, types, variants);
    corpus::record_components::<Connective>(par, types, variants);
    corpus::record_components::<If>(par, types, variants);
    corpus::record_components::<CostTokenStack>(par, types, variants);
}

#[test]
fn checkpoint_egress_of_every_generated_payload_arm_equals_the_typed_encoders() {
    let data_values = stored_data_values();
    let continuation_values = stored_continuation_values();
    let patterns = corpus::bind_pattern_corpus()
        .into_iter()
        .map(|(_, pattern)| pattern)
        .collect::<Vec<_>>();
    let data_channel = corpus::gint(1);
    let group = (2_i64..)
        .map(corpus::gint)
        .take(patterns.len())
        .collect::<Vec<_>>();
    let datums = data_values
        .iter()
        .map(|value| Datum::create(&data_channel, value.clone(), false))
        .collect::<Vec<_>>();
    let continuations = continuation_values
        .iter()
        .map(|value| WaitingContinuation::create(&group, &patterns, value, false, BTreeSet::new()))
        .collect::<Vec<_>>();
    let group_joins = vec![group.clone()];

    let mut expected = BTreeMap::new();
    expected.insert(
        ("data", hash(&data_channel).bytes()),
        ExpectedCanonicalRecord {
            label: "the data channel".to_string(),
            count: u64::try_from(datums.len()).expect("a datum count fits u64"),
            encoded_persisted_data: bincode::serialize(&PersistedData::Data(DataLeaf {
                bytes: encode_datums(&datums),
            }))
            .expect("a typed data image"),
        },
    );
    expected.insert(
        ("continuations", hash_from_slice(&group).bytes()),
        ExpectedCanonicalRecord {
            label: "the continuation group".to_string(),
            count: u64::try_from(continuations.len()).expect("a continuation count fits u64"),
            encoded_persisted_data: bincode::serialize(&PersistedData::Continuations(
                ContinuationsLeaf {
                    bytes: encode_continuations(&continuations),
                },
            ))
            .expect("a typed continuation image"),
        },
    );
    let join_image = bincode::serialize(&PersistedData::Joins(JoinsLeaf {
        bytes: encode_joins(&group_joins),
    }))
    .expect("a typed join image");
    for (position, channel) in group.iter().enumerate() {
        expected.insert(("joins", hash(channel).bytes()), ExpectedCanonicalRecord {
            label: format!("the joins of group channel {position}"),
            count: 1,
            encoded_persisted_data: join_image.clone(),
        });
    }

    let hot_store = empty_par_hot_store();
    hot_store.set_state(HotStoreState {
        continuations: HashMap::from([(group.clone(), continuations)]),
        installed_continuations: HashMap::new(),
        data: HashMap::from([(data_channel, datums)]),
        joins: group
            .iter()
            .map(|channel| (channel.clone(), group_joins.clone()))
            .collect(),
        installed_joins: HashMap::new(),
    });
    let actions = hot_store
        .checkpoint_changes()
        .expect("the checkpoint preflight accepts the stored payloads");
    assert_eq!(
        actions.len(),
        expected.len(),
        "one canonical insert per stored channel and channel group"
    );
    for action in &actions {
        let (key, count, encoded_persisted_data) = match action {
            HotStoreAction::Insert(InsertAction::InsertCanonicalData(insert)) => (
                ("data", insert.hash().bytes()),
                insert.datum_count(),
                insert.encoded_persisted_data(),
            ),
            HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(insert)) => (
                (
                    persisted_kind_label(insert.persisted_kind()),
                    insert.hash().bytes(),
                ),
                insert.record_count(),
                insert.encoded_persisted_data(),
            ),
            other => {
                panic!("the checkpoint of stored payloads emits only inserts, found {other:?}")
            }
        };
        let record = expected
            .remove(&key)
            .unwrap_or_else(|| panic!("no stored {} record has the key {:?}", key.0, key.1));
        assert_eq!(count, record.count, "{}: record count", record.label);
        assert_eq!(
            encoded_persisted_data,
            record.encoded_persisted_data.as_slice(),
            "{}: the canonical bytes equal the typed encoder's",
            record.label
        );
    }
    assert!(
        expected.is_empty(),
        "stored records without a canonical insert: {:?}",
        expected
            .values()
            .map(|record| &record.label)
            .collect::<Vec<_>>()
    );

    let mut types = BTreeSet::new();
    let mut variants = BTreeSet::new();
    for value in &data_values {
        corpus::record(ParCursor::from(value), &mut types, &mut variants);
        for par in &value.pars {
            record_components_of(par, &mut types, &mut variants);
        }
    }
    for value in &continuation_values {
        corpus::record(ParCursor::from(value), &mut types, &mut variants);
        for par in value.guard.iter().chain(continuation_body(value)) {
            record_components_of(par, &mut types, &mut variants);
        }
    }
    for pattern in &patterns {
        corpus::record(ParCursor::from(pattern), &mut types, &mut variants);
        for par in &pattern.patterns {
            record_components_of(par, &mut types, &mut variants);
        }
    }

    let registry = RHO_MATCH_REGISTRY
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    let unreachable = PAYLOAD_UNREACHABLE_MESSAGES
        .iter()
        .map(|(name, _)| *name)
        .collect::<BTreeSet<_>>();
    assert!(
        unreachable.is_subset(&registry),
        "every message listed as unreachable is a registry message: {:?}",
        unreachable.difference(&registry).collect::<Vec<_>>()
    );
    let missing = registry
        .difference(&unreachable)
        .filter(|name| !types.contains(*name))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "the census of the stored payloads does not reach the messages {missing:?}"
    );
    let listed_but_reached = unreachable.intersection(&types).collect::<Vec<_>>();
    assert!(
        listed_but_reached.is_empty(),
        "messages listed as unreachable from an RSpace payload type are reached: \
         {listed_but_reached:?}"
    );

    let mut expected_variants = BTreeSet::new();
    for (oneof, programs) in [
        ("ExprInstance", EXPR_INSTANCE_VARIANTS),
        ("ConnectiveInstance", CONNECTIVE_INSTANCE_VARIANTS),
        ("VarInstance", VAR_INSTANCE_VARIANTS),
        ("UnfInstance", UNF_INSTANCE_VARIANTS),
        ("TaggedCont", TAGGED_CONT_VARIANTS),
        ("Form", FORM_VARIANTS),
    ] {
        for program in programs {
            expected_variants.insert(format!("{oneof}::{}", program.name));
        }
    }
    assert_eq!(
        variants, expected_variants,
        "the census of the stored payloads reaches every variant of the six oneofs"
    );
}
