use std::borrow::Cow;
use std::collections::{BTreeSet, HashSet};

use proptest::prelude::*;

use super::support::{TEST_SEMANTIC_PROFILE_ID, admit, encode, synthetic_ref};
use crate::rspace::data_index::{DataIndexError, RSpaceDataIndex, RSpaceDataStore};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::hashing::stable_hash_provider::StableHashSerialize;
use crate::rspace::internal::{Datum, WaitingContinuation};
use crate::rspace::persistence::semantic::{
    decode_continuation_endpoint_with_channels, prepare_semantic_delta,
};
use crate::rspace::persistence::{
    CanonicalBlob, CanonicalBlobKind, CanonicalFormatError, CanonicalObjectStore, ClosureError,
    EndpointValue, IncrementalSemanticLimits, MutationBatchViewV1, MutationError,
    RSpaceMutationBatchV1, SemanticNamespace, SemanticProfile, SemanticProfileId, StateVersionV1,
    semantic_profile_seal,
};
use crate::rspace::record_index::RSpaceJoinStore;
use crate::rspace::rho_path::{
    self, ReceiveChannelGroup, RhoPathChannelError, RhoPathEncode, RhoPathError,
    RhoPathStreamValidatorV1, RhoPathV1,
};
use crate::rspace::serializers::cold_store_decode::{ColdStoreDecode, ColdStoreDecodeError};
use crate::rspace::serializers::serializers::{
    encode_continuation_identity, encode_continuation_source_into, encode_join_record,
};

const KEY_MAGIC: [u8; 4] = *b"RHP\x01";
const CANONICAL_TAG: u8 = 0x01;
const ALIAS_TAG: u8 = 0x02;
const UNKNOWN_TAG: u8 = 0x7F;
const TRAILING_BYTE: u8 = 0xEE;
const STRUCTURE_SKEW: u8 = 0x53;

type StreamTrace = (Vec<Result<(), RhoPathError>>, Result<(), RhoPathError>);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct FreeFieldChannel {
    structure: Vec<u8>,
    identity: Vec<u8>,
}

impl StableHashSerialize for FreeFieldChannel {
    fn stable_hash_bytes(&self) -> Vec<u8> { self.identity.clone() }
}

impl RhoPathEncode for FreeFieldChannel {
    fn rho_path_structure(&self) -> Vec<u8> { self.structure.clone() }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
enum WrittenForm {
    Canonical,
    Alias,
    UnknownTag,
    Truncated,
    Trailing,
    SkewedStructure,
}

const WRITTEN_FORMS: [WrittenForm; 6] = [
    WrittenForm::Canonical,
    WrittenForm::Alias,
    WrittenForm::UnknownTag,
    WrittenForm::Truncated,
    WrittenForm::Trailing,
    WrittenForm::SkewedStructure,
];

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
struct ProbeChannel {
    form: WrittenForm,
    payload: Vec<u8>,
}

impl ProbeChannel {
    fn canonical(&self) -> Self {
        Self {
            form: WrittenForm::Canonical,
            payload: self.payload.clone(),
        }
    }
}

impl StableHashSerialize for ProbeChannel {
    fn stable_hash_bytes(&self) -> Vec<u8> {
        let (tag, declared_len, trailing): (u8, usize, &[u8]) = match self.form {
            WrittenForm::Canonical | WrittenForm::SkewedStructure => {
                (CANONICAL_TAG, self.payload.len(), &[])
            }
            WrittenForm::Alias => (ALIAS_TAG, self.payload.len(), &[]),
            WrittenForm::UnknownTag => (UNKNOWN_TAG, self.payload.len(), &[]),
            WrittenForm::Truncated => (CANONICAL_TAG, self.payload.len() + 1, &[]),
            WrittenForm::Trailing => (CANONICAL_TAG, self.payload.len(), &[TRAILING_BYTE]),
        };
        let mut output = Vec::with_capacity(2 + self.payload.len() + trailing.len());
        output.push(tag);
        output.push(u8::try_from(declared_len).expect("a probe payload is shorter than 255 bytes"));
        output.extend_from_slice(&self.payload);
        output.extend_from_slice(trailing);
        output
    }
}

impl RhoPathEncode for ProbeChannel {
    fn rho_path_structure(&self) -> Vec<u8> {
        let mut structure = self.payload.clone();
        if let WrittenForm::SkewedStructure = self.form {
            structure.push(STRUCTURE_SKEW);
        }
        structure
    }
}

impl ColdStoreDecode for ProbeChannel {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        match bytes {
            [CANONICAL_TAG | ALIAS_TAG, declared_len, rest @ ..] => {
                let declared_len = usize::from(*declared_len);
                match rest.get(..declared_len) {
                    Some(payload) => Ok((
                        Self {
                            form: WrittenForm::Canonical,
                            payload: payload.to_vec(),
                        },
                        2 + declared_len,
                    )),
                    None => Err(ColdStoreDecodeError::UnexpectedEof {
                        wanted: "probe channel payload",
                        needed: declared_len - rest.len(),
                        available: rest.len(),
                    }),
                }
            }
            [] | [CANONICAL_TAG | ALIAS_TAG] => Err(ColdStoreDecodeError::UnexpectedEof {
                wanted: "probe channel header",
                needed: 2 - bytes.len(),
                available: bytes.len(),
            }),
            [tag, ..] => Err(ColdStoreDecodeError::InvalidTagEncoding(usize::from(*tag))),
        }
    }
}

struct ProbeSemanticProfile;

impl semantic_profile_seal::Sealed for ProbeSemanticProfile {}

impl SemanticProfile for ProbeSemanticProfile {
    type Channel = ProbeChannel;
    type Continuation = String;
    type Datum = String;
    type Pattern = String;

    const ID: SemanticProfileId = TEST_SEMANTIC_PROFILE_ID;
}

fn grammar_magic<G: rho_path::RhoPathKeySyntax>() -> [u8; 4] { G::MAGIC }

fn grammar_datum_prefix<G, C>(channel: &C) -> Vec<u8>
where G: rho_path::RhoPathGrammar<C> {
    G::datum_prefix(channel)
}

fn grammar_continuation_prefix<G, C>(channels: &[C]) -> Vec<u8>
where G: rho_path::RhoPathGrammar<C> {
    G::continuation_prefix(channels)
}

fn grammar_join_prefix<G, C>(channel: &C) -> Vec<u8>
where G: rho_path::RhoPathGrammar<C> {
    G::join_prefix(channel)
}

fn grammar_channel_field<G, C>(channel: &C) -> Vec<u8>
where G: rho_path::RhoPathGrammar<C> {
    G::channel_field(channel)
}

fn grammar_parse_datum<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<G::DatumView<'_>, RhoPathError> {
    G::parse_datum(path)
}

fn grammar_datum_channel_prefix_len<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<usize, RhoPathError> {
    G::parse_datum(path).map(|view| G::datum_channel_prefix_len(&view))
}

fn grammar_parse_continuation<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<G::ContinuationView<'_>, RhoPathError> {
    G::parse_continuation(path)
}

fn grammar_continuation_group<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<ReceiveChannelGroup<'_>, RhoPathError> {
    G::parse_continuation(path).map(|view| G::continuation_group(&view))
}

fn grammar_parse_continuation_group<G: rho_path::RhoPathKeySyntax>(
    prefix: &[u8],
) -> Result<ReceiveChannelGroup<'_>, RhoPathError> {
    G::parse_continuation_group(prefix)
}

fn grammar_parse_join<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<G::JoinView<'_>, RhoPathError> {
    G::parse_join(path)
}

fn grammar_join_record<G: rho_path::RhoPathKeySyntax>(
    path: &[u8],
) -> Result<Cow<'_, [u8]>, RhoPathError> {
    G::parse_join(path).map(|view| G::join_record(&view))
}

fn grammar_join_record_path<G: rho_path::RhoPathKeySyntax>(
    join_prefix: &[u8],
    join_record: &[u8],
) -> Vec<u8> {
    G::join_record_path(join_prefix, join_record)
}

fn grammar_validate_key<G: rho_path::RhoPathKeySyntax>(
    namespace: SemanticNamespace,
    path: &[u8],
) -> Result<(), RhoPathError> {
    G::validate_key(namespace, path)
}

fn grammar_stream_trace<G: rho_path::RhoPathKeySyntax>(
    namespace: SemanticNamespace,
    chunks: &[&[u8]],
) -> StreamTrace {
    let mut validator = G::stream_validator(namespace);
    let mut pushes = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        pushes.push(rho_path::RhoPathStreamValidate::push(&mut validator, chunk));
    }
    (pushes, rho_path::RhoPathStreamValidate::finish(validator))
}

fn grammar_datum_channel<G, C>(path: &[u8]) -> Result<C, RhoPathChannelError>
where G: rho_path::RhoPathGrammarDecode<C> {
    let view = G::parse_datum(path).expect("the data key parses");
    G::decode_datum_channel(&view)
}

fn grammar_continuation_channels<G, C>(path: &[u8]) -> Result<Vec<C>, RhoPathChannelError>
where G: rho_path::RhoPathGrammarDecode<C> {
    let view = G::parse_continuation(path).expect("the continuation key parses");
    G::decode_continuation_channels(&view)
}

fn grammar_join_channel<G, C>(path: &[u8]) -> Result<C, RhoPathChannelError>
where G: rho_path::RhoPathGrammarDecode<C> {
    let view = G::parse_join(path).expect("the join key parses");
    G::decode_join_channel(&view)
}

fn grammar_prefix_channel<G, C>(datum_prefix: &[u8]) -> Result<C, RhoPathChannelError>
where G: rho_path::RhoPathGrammarDecode<C> {
    G::prefix_channel(datum_prefix)
}

fn grammar_prefix_legacy_identity<G, C>(
    datum_prefix: &[u8],
) -> Result<Vec<u8>, RhoPathChannelError>
where G: rho_path::RhoPathGrammarDecode<C> {
    G::prefix_legacy_identity(datum_prefix)
}

fn field_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![Just(0_u8), Just(1_u8), Just(0xFF_u8), any::<u8>()],
        0..12,
    )
}

fn record_bytes() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(
        prop_oneof![Just(0_u8), Just(1_u8), Just(0xFF_u8), any::<u8>()],
        1..12,
    )
}

fn free_field_channel() -> impl Strategy<Value = FreeFieldChannel> {
    (field_bytes(), field_bytes()).prop_map(|(structure, identity)| FreeFieldChannel {
        structure,
        identity,
    })
}

fn built_keys(
    channel: &FreeFieldChannel,
    other_channel: &FreeFieldChannel,
    datum_structure: &[u8],
    record: &[u8],
) -> [Vec<u8>; 3] {
    let continuation_prefix =
        RhoPathV1::continuation_prefix(&[channel.clone(), other_channel.clone()]);
    let mut join_key = RhoPathV1::join_prefix(channel);
    join_key.extend_from_slice(record);
    [
        RhoPathV1::datum_key(channel, datum_structure, record),
        RhoPathV1::collection_record_path(&continuation_prefix, record),
        join_key,
    ]
}

fn corrupted(bytes: &[u8], position: &proptest::sample::Index, mask: u8) -> Vec<u8> {
    let mut corrupted = bytes.to_vec();
    if let Some(byte) = corrupted.get_mut(position.index(bytes.len().max(1))) {
        *byte ^= mask;
    }
    corrupted
}

fn chunks_by_widths<'a>(bytes: &'a [u8], widths: &[usize]) -> Vec<&'a [u8]> {
    let mut chunks = Vec::with_capacity(widths.len() + 1);
    let mut rest = bytes;
    for width in widths {
        let (chunk, tail) = rest.split_at((*width).min(rest.len()));
        chunks.push(chunk);
        rest = tail;
    }
    chunks.push(rest);
    chunks
}

fn assert_builders_agree<C: Clone + RhoPathEncode>(channels: &[C]) {
    let magic = grammar_magic::<RhoPathV1>();
    assert_eq!(magic, KEY_MAGIC, "the V1 magic of the trait is RHP 01");
    let repeated: Vec<C> = channels.iter().chain(channels.iter()).cloned().collect();
    for group in [channels, repeated.as_slice()] {
        let continuation_prefix = grammar_continuation_prefix::<RhoPathV1, C>(group);
        assert_eq!(
            continuation_prefix,
            RhoPathV1::continuation_prefix(group),
            "continuation_prefix through the trait differs from RhoPathV1::continuation_prefix",
        );
        assert!(continuation_prefix.starts_with(&magic));
    }
    for channel in channels {
        let datum_prefix = grammar_datum_prefix::<RhoPathV1, C>(channel);
        assert_eq!(
            datum_prefix,
            RhoPathV1::datum_prefix(channel),
            "datum_prefix through the trait differs from RhoPathV1::datum_prefix",
        );
        assert!(datum_prefix.starts_with(&magic));
        let join_prefix = grammar_join_prefix::<RhoPathV1, C>(channel);
        assert_eq!(
            join_prefix,
            RhoPathV1::join_prefix(channel),
            "join_prefix through the trait differs from RhoPathV1::join_prefix",
        );
        assert!(join_prefix.starts_with(&magic));
        assert_eq!(
            grammar_channel_field::<RhoPathV1, C>(channel),
            channel.rho_path_identity(),
            "channel_field through the trait differs from rho_path_identity",
        );
    }
}

fn assert_parsers_agree(bytes: &[u8]) {
    let datum = RhoPathV1::parse_datum(bytes);
    assert_eq!(grammar_parse_datum::<RhoPathV1>(bytes), datum, "parse_datum differs");
    assert_eq!(
        grammar_datum_channel_prefix_len::<RhoPathV1>(bytes),
        datum.map(|view| view.channel_prefix_len()),
        "datum_channel_prefix_len differs",
    );

    let continuation = RhoPathV1::parse_continuation(bytes);
    assert_eq!(
        grammar_parse_continuation::<RhoPathV1>(bytes),
        continuation,
        "parse_continuation differs",
    );
    assert_eq!(
        grammar_continuation_group::<RhoPathV1>(bytes),
        continuation.map(|view| {
            let prefix_len = bytes.len() - view.encoded_identity_frame().len();
            RhoPathV1::parse_continuation_group(&bytes[..prefix_len])
                .expect("the prefix of a continuation key is its channel group")
        }),
        "continuation_group differs from the group of the key's prefix",
    );

    assert_eq!(
        grammar_parse_continuation_group::<RhoPathV1>(bytes),
        RhoPathV1::parse_continuation_group(bytes),
        "parse_continuation_group differs",
    );

    let join = RhoPathV1::parse_join(bytes);
    assert_eq!(grammar_parse_join::<RhoPathV1>(bytes), join, "parse_join differs");
    let join_record = grammar_join_record::<RhoPathV1>(bytes);
    assert_eq!(
        join_record,
        join.map(|view| Cow::Borrowed(view.join_identity())),
        "join_record differs from the raw suffix of the key",
    );
    assert!(
        !matches!(join_record, Ok(Cow::Owned(_))),
        "the V1 join record is borrowed from the key",
    );
}

fn mutation_codec_rule(namespace: SemanticNamespace, path: &[u8]) -> Result<(), RhoPathError> {
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(7, synthetic_ref(90)))
        .expect("the generation has a successor");
    let recorded = match namespace {
        SemanticNamespace::DataBag => batch.record_data(path, 0, 1),
        SemanticNamespace::ContinuationMap => {
            batch.record_continuation(path, None, Some(synthetic_ref(11)))
        }
        SemanticNamespace::JoinBag => batch.record_join(path, 0, 1),
    };
    match recorded {
        Ok(()) => Ok(()),
        Err(MutationError::InvalidKey {
            namespace: refused_namespace,
            source,
        }) => {
            assert_eq!(refused_namespace, namespace, "the codec names the namespace it validated");
            Err(source)
        }
        Err(other) => panic!("the mutation codec refused a key for another reason: {other:?}"),
    }
}

fn assert_validate_key_agrees(bytes: &[u8]) {
    for namespace in SemanticNamespace::ALL {
        assert_eq!(
            grammar_validate_key::<RhoPathV1>(namespace, bytes),
            mutation_codec_rule(namespace, bytes),
            "validate_key differs from the mutation codec for the namespace {}",
            namespace.name(),
        );
    }
}

fn inherent_stream_trace(namespace: SemanticNamespace, chunks: &[&[u8]]) -> StreamTrace {
    let mut validator = match namespace {
        SemanticNamespace::DataBag => RhoPathStreamValidatorV1::datum(),
        SemanticNamespace::ContinuationMap => RhoPathStreamValidatorV1::continuation(),
        SemanticNamespace::JoinBag => RhoPathStreamValidatorV1::join(),
    };
    let mut pushes = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        pushes.push(validator.push(chunk));
    }
    (pushes, validator.finish())
}

fn assert_stream_validator_agrees(bytes: &[u8], widths: &[usize]) {
    let schedules = [
        chunks_by_widths(bytes, widths),
        bytes.chunks(1).collect(),
        Vec::from([bytes]),
        Vec::from([&[][..], bytes, &[][..]]),
    ];
    for namespace in SemanticNamespace::ALL {
        let contiguous = grammar_validate_key::<RhoPathV1>(namespace, bytes);
        for schedule in &schedules {
            let through_trait = grammar_stream_trace::<RhoPathV1>(namespace, schedule);
            assert_eq!(
                through_trait,
                inherent_stream_trace(namespace, schedule),
                "the stream validator of the trait differs from the inherent validator for the \
                 namespace {}",
                namespace.name(),
            );
            assert_eq!(
                through_trait.1,
                contiguous,
                "the stream validator of the trait differs from validate_key for the namespace {}",
                namespace.name(),
            );
        }
    }
}

fn variant(error: &RhoPathChannelError) -> &'static str {
    match error {
        RhoPathChannelError::Path(_) => "Path",
        RhoPathChannelError::ColdDecode(_) => "ColdDecode",
        RhoPathChannelError::Detail(_) => "Detail",
        RhoPathChannelError::Mismatch(_) => "Mismatch",
    }
}

fn identity_refusal(form: WrittenForm) -> Option<&'static str> {
    match form {
        WrittenForm::Canonical | WrittenForm::SkewedStructure => None,
        WrittenForm::Alias => Some("Mismatch"),
        WrittenForm::UnknownTag | WrittenForm::Truncated | WrittenForm::Trailing => Some("Detail"),
    }
}

fn data_key_refusal(form: WrittenForm) -> Option<&'static str> {
    match form {
        WrittenForm::SkewedStructure => Some("Mismatch"),
        other => identity_refusal(other),
    }
}

fn egress_refusal(form: WrittenForm) -> Option<&'static str> {
    match form {
        WrittenForm::UnknownTag | WrittenForm::Truncated => Some("ColdDecode"),
        WrittenForm::Canonical |
        WrittenForm::Alias |
        WrittenForm::Trailing |
        WrittenForm::SkewedStructure => None,
    }
}

fn inline_egress_error(error: RhoPathChannelError) -> DataIndexError {
    match error {
        RhoPathChannelError::Path(source) => DataIndexError::from(source),
        RhoPathChannelError::ColdDecode(source) => DataIndexError::from(source),
        other => panic!("an egress decoder returned {other:?}, which the data index cannot return"),
    }
}

fn inline_delta_refusal(
    namespace: SemanticNamespace,
    record: impl FnOnce(&mut RSpaceMutationBatchV1) -> Result<(), MutationError>,
) -> Option<String> {
    let mut batch = RSpaceMutationBatchV1::new(StateVersionV1::new(1, synthetic_ref(90)))
        .expect("the generation has a successor");
    record(&mut batch).expect("the mutation codec accepts a key that parses");
    let encoded = encode(&batch.plan().expect("a batch with one entry plans"));
    let view = MutationBatchViewV1::parse(&encoded).expect("the encoded batch parses");
    let store = CanonicalObjectStore::new();
    match prepare_semantic_delta::<ProbeSemanticProfile, _>(
        &store,
        view,
        &HashSet::new(),
        IncrementalSemanticLimits::UNBOUNDED,
    ) {
        Ok(_) => None,
        Err(ClosureError::Format(CanonicalFormatError::SemanticKey {
            namespace: refused_namespace,
            detail,
        })) => {
            assert_eq!(refused_namespace, namespace.name());
            Some(detail)
        }
        Err(other) => panic!("the semantic delta was refused for another reason: {other:?}"),
    }
}

fn assert_data_key_decoder_agrees(written: &ProbeChannel) {
    let datum = Datum::create(written, "datum".to_owned(), false);
    let path = RSpaceDataIndex::datum_path(written, &datum);
    let inline =
        inline_delta_refusal(SemanticNamespace::DataBag, |batch| batch.record_data(&path, 0, 1));
    let through_trait = grammar_datum_channel::<RhoPathV1, ProbeChannel>(&path);
    assert_eq!(
        through_trait.as_ref().err().map(ToString::to_string),
        inline,
        "decode_datum_channel and validate_data_key refuse the written form {:?} differently",
        written.form,
    );
    assert_eq!(
        through_trait.as_ref().err().map(variant),
        data_key_refusal(written.form),
        "decode_datum_channel returned {through_trait:?} for the written form {:?}",
        written.form,
    );
    if let Ok(channel) = through_trait {
        assert_eq!(channel, written.canonical());
    }
}

fn assert_join_key_decoder_agrees(written: &ProbeChannel, other_payload: &[u8]) {
    let other_channel = ProbeChannel {
        form: WrittenForm::Canonical,
        payload: other_payload.to_vec(),
    };
    let mut path = RhoPathV1::join_prefix(written);
    path.extend_from_slice(&encode_join_record(&[written.canonical(), other_channel]));
    let inline =
        inline_delta_refusal(SemanticNamespace::JoinBag, |batch| batch.record_join(&path, 0, 1));
    let through_trait = grammar_join_channel::<RhoPathV1, ProbeChannel>(&path);
    assert_eq!(
        through_trait.as_ref().err().map(ToString::to_string),
        inline,
        "decode_join_channel and validate_join_key refuse the written form {:?} differently",
        written.form,
    );
    assert_eq!(
        through_trait.as_ref().err().map(variant),
        identity_refusal(written.form),
        "decode_join_channel returned {through_trait:?} for the written form {:?}",
        written.form,
    );
    if let Ok(channel) = through_trait {
        assert_eq!(channel, written.canonical());
    }
}

fn assert_continuation_decoder_agrees(written_group: &[ProbeChannel]) {
    let canonical_group: Vec<ProbeChannel> =
        written_group.iter().map(ProbeChannel::canonical).collect();
    let patterns: Vec<String> = (0..written_group.len())
        .map(|index| format!("pattern-{index}"))
        .collect();
    let waiting = WaitingContinuation::create(
        &canonical_group,
        &patterns,
        &"continuation".to_owned(),
        false,
        BTreeSet::new(),
    );
    let mut source_payload = Vec::new();
    encode_continuation_source_into(&waiting, &mut source_payload);
    let store = CanonicalObjectStore::new();
    let source = admit(
        &store,
        &CanonicalBlob::new(CanonicalBlobKind::ContinuationSourceV1, source_payload)
            .expect("the source blob is canonical"),
    );
    let path = RhoPathV1::collection_record_path(
        &RhoPathV1::continuation_prefix(written_group),
        &encode_continuation_identity(&waiting),
    );
    let decoded = decode_continuation_endpoint_with_channels::<ProbeChannel, String, String, _>(
        &store,
        &path,
        EndpointValue::Content(source),
        u64::MAX,
    );
    let inline = match decoded {
        Ok((channels, _)) => Ok(channels),
        Err(ClosureError::Format(CanonicalFormatError::SemanticKey { namespace, detail })) => {
            assert_eq!(namespace, SemanticNamespace::ContinuationMap.name());
            Err(detail)
        }
        Err(other) => panic!("the continuation record was refused for another reason: {other:?}"),
    };
    let through_trait = grammar_continuation_channels::<RhoPathV1, ProbeChannel>(&path);
    assert_eq!(
        through_trait.as_ref().err().map(variant),
        written_group
            .iter()
            .find_map(|channel| identity_refusal(channel.form)),
        "decode_continuation_channels returned {through_trait:?} for {written_group:?}",
    );
    assert_eq!(
        through_trait.map_err(|error| error.to_string()),
        inline,
        "decode_continuation_channels and validate_continuation_record differ for \
         {written_group:?}",
    );
    if let Ok(channels) = inline {
        assert_eq!(channels, canonical_group);
    }
}

fn assert_prefix_decoders_agree(written: &ProbeChannel) {
    let datum = Datum::create(written, "datum".to_owned(), false);
    let mut store = RSpaceDataStore::<ProbeChannel, String>::new();
    store
        .set_channel(written.clone(), [datum])
        .expect("the written channel loads");
    let datum_prefix = RhoPathV1::datum_prefix(written);

    let through_trait = grammar_prefix_channel::<RhoPathV1, ProbeChannel>(&datum_prefix);
    assert_eq!(
        through_trait.as_ref().err().map(variant),
        egress_refusal(written.form),
        "prefix_channel returned {through_trait:?} for the written form {:?}",
        written.form,
    );
    let expected_channels = through_trait
        .map(|channel| Vec::from([channel]))
        .map_err(inline_egress_error);
    assert_eq!(
        store.loaded_records().map(|records| {
            records
                .into_iter()
                .map(|(channel, _)| channel)
                .collect::<Vec<_>>()
        }),
        expected_channels,
        "prefix_channel and loaded_records differ for the written form {:?}",
        written.form,
    );
    let mut visited_channels = Vec::with_capacity(1);
    let visited = store.try_visit_loaded_records(|channel, _| {
        visited_channels.push(channel);
        Ok(())
    });
    assert_eq!(
        visited.map(|()| visited_channels),
        expected_channels,
        "prefix_channel and try_visit_loaded_records differ for the written form {:?}",
        written.form,
    );

    let legacy_identity = grammar_prefix_legacy_identity::<RhoPathV1, ProbeChannel>(&datum_prefix)
        .expect("the prefix of a loaded channel parses");
    assert_eq!(legacy_identity, written.rho_path_identity());
    let mut channel_hashes = Vec::with_capacity(1);
    store
        .try_visit_loaded_canonical_records(|record| {
            channel_hashes.push(record.channel_hash);
            Ok(())
        })
        .expect("the canonical records are visited");
    assert_eq!(
        channel_hashes,
        [Blake2b256Hash::new(&legacy_identity)],
        "prefix_legacy_identity and try_visit_loaded_canonical_records differ for the written \
         form {:?}",
        written.form,
    );
}

fn assert_malformed_prefixes_agree(written: &ProbeChannel) {
    let datum_prefix = RhoPathV1::datum_prefix(written);
    let mut trailing = datum_prefix.clone();
    trailing.push(7);
    let malformed = (0..datum_prefix.len())
        .map(|cut| datum_prefix[..cut].to_vec())
        .chain([
            trailing,
            RhoPathV1::join_prefix(written),
            RhoPathV1::continuation_prefix(std::slice::from_ref(written)),
        ]);
    for candidate in malformed {
        let refusal = DataIndexError::from(
            RhoPathV1::parse_datum_prefix(&candidate)
                .expect_err("a malformed datum prefix does not parse"),
        );
        assert_eq!(
            grammar_prefix_channel::<RhoPathV1, ProbeChannel>(&candidate)
                .map_err(inline_egress_error),
            Err(refusal.clone()),
            "prefix_channel differs from parse_datum_prefix for {candidate:?}",
        );
        assert_eq!(
            grammar_prefix_legacy_identity::<RhoPathV1, ProbeChannel>(&candidate)
                .map_err(inline_egress_error),
            Err(refusal),
            "prefix_legacy_identity differs from parse_datum_prefix for {candidate:?}",
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn v1_builders_equal_the_rhopathv1_functions_byte_for_byte(
        free_channels in proptest::collection::vec(free_field_channel(), 0..5),
        text_channels in proptest::collection::vec(".{0,12}", 0..5),
        integer_channels in proptest::collection::vec(any::<u64>(), 0..5),
        byte_channels in proptest::collection::vec(field_bytes(), 0..5),
    ) {
        assert_builders_agree(&free_channels);
        assert_builders_agree(&text_channels);
        assert_builders_agree(&integer_channels);
        assert_builders_agree(&byte_channels);
    }

    #[test]
    fn v1_parsers_return_the_same_views_and_errors_as_rhopathv1(
        arbitrary in proptest::collection::vec(any::<u8>(), 0..96),
        channel in free_field_channel(),
        other_channel in free_field_channel(),
        datum_structure in field_bytes(),
        record in record_bytes(),
        position in any::<proptest::sample::Index>(),
        mask in 1_u8..=255,
    ) {
        let keys = built_keys(&channel, &other_channel, &datum_structure, &record);
        prop_assert!(grammar_parse_datum::<RhoPathV1>(&keys[0]).is_ok());
        prop_assert_eq!(
            grammar_continuation_group::<RhoPathV1>(&keys[1]).map(|group| group.channel_count()),
            Ok(2)
        );
        prop_assert_eq!(
            grammar_join_record::<RhoPathV1>(&keys[2]),
            Ok(Cow::Borrowed(record.as_slice()))
        );
        assert_parsers_agree(&arbitrary);
        assert_parsers_agree(&RhoPathV1::continuation_prefix::<FreeFieldChannel>(&[]));
        for key in &keys {
            for cut in 0..=key.len() {
                assert_parsers_agree(&key[..cut]);
            }
            assert_parsers_agree(&corrupted(key, &position, mask));
        }
    }

    #[test]
    fn v1_stream_validator_through_the_trait_equals_the_inherent_validator_under_chunk_schedules(
        arbitrary in proptest::collection::vec(any::<u8>(), 0..96),
        widths in proptest::collection::vec(0_usize..24, 0..24),
        channel in free_field_channel(),
        other_channel in free_field_channel(),
        datum_structure in field_bytes(),
        record in record_bytes(),
        position in any::<proptest::sample::Index>(),
        mask in 1_u8..=255,
    ) {
        let keys = built_keys(&channel, &other_channel, &datum_structure, &record);
        for (namespace, key) in SemanticNamespace::ALL.into_iter().zip(&keys) {
            let whole = grammar_stream_trace::<RhoPathV1>(namespace, &[key.as_slice()]);
            prop_assert_eq!(whole, (Vec::from([Ok(())]), Ok(())));
        }
        assert_stream_validator_agrees(&arbitrary, &widths);
        for key in &keys {
            assert_stream_validator_agrees(key, &widths);
            assert_stream_validator_agrees(&key[..position.index(key.len())], &widths);
            assert_stream_validator_agrees(&corrupted(key, &position, mask), &widths);
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn v1_validate_key_equals_the_mutation_codec_rule_for_every_namespace(
        arbitrary in proptest::collection::vec(any::<u8>(), 0..96),
        channel in free_field_channel(),
        other_channel in free_field_channel(),
        datum_structure in field_bytes(),
        record in record_bytes(),
        position in any::<proptest::sample::Index>(),
        mask in 1_u8..=255,
    ) {
        let keys = built_keys(&channel, &other_channel, &datum_structure, &record);
        for (namespace, key) in SemanticNamespace::ALL.into_iter().zip(&keys) {
            prop_assert_eq!(grammar_validate_key::<RhoPathV1>(namespace, key), Ok(()));
        }
        assert_validate_key_agrees(&arbitrary);
        for key in &keys {
            for cut in 0..=key.len() {
                assert_validate_key_agrees(&key[..cut]);
            }
            assert_validate_key_agrees(&corrupted(key, &position, mask));
        }
    }

    #[test]
    fn v1_decoders_return_the_same_channels_and_error_texts_as_the_inline_code(
        payload in field_bytes(),
        other_payload in field_bytes(),
    ) {
        for form in WRITTEN_FORMS {
            let written = ProbeChannel {
                form,
                payload: payload.clone(),
            };
            assert_data_key_decoder_agrees(&written);
            assert_join_key_decoder_agrees(&written, &other_payload);
            assert_prefix_decoders_agree(&written);
            assert_malformed_prefixes_agree(&written);
            assert_continuation_decoder_agrees(std::slice::from_ref(&written));
            for other_form in WRITTEN_FORMS {
                let other_written = ProbeChannel {
                    form: other_form,
                    payload: other_payload.clone(),
                };
                assert_continuation_decoder_agrees(&[written.clone(), other_written]);
            }
        }
    }
}

#[test]
fn v1_join_record_path_is_the_raw_concatenation() {
    let channels = [
        FreeFieldChannel {
            structure: Vec::from(*b"shape"),
            identity: Vec::from(*b"channel"),
        },
        FreeFieldChannel {
            structure: Vec::new(),
            identity: Vec::from([0, 1, 0xFF, 0]),
        },
    ];
    let records: [&[u8]; 6] = [&[], b"r", b"join-record", &[0], &[0, 0, 1, 0xFF, 0], &[0xFF; 9]];
    for channel in &channels {
        let join_prefix = RhoPathV1::join_prefix(channel);
        for record in records {
            let path = grammar_join_record_path::<RhoPathV1>(&join_prefix, record);
            let mut concatenation = Vec::with_capacity(join_prefix.len() + record.len());
            concatenation.extend_from_slice(&join_prefix);
            concatenation.extend_from_slice(record);
            assert_eq!(
                path, concatenation,
                "the V1 join record path is the join prefix followed by the unframed join record",
            );
            match (record.is_empty(), RhoPathV1::parse_join(&path)) {
                (true, Err(RhoPathError::TruncatedFrame)) => {}
                (false, Ok(view)) => {
                    assert_eq!(view.join_identity(), record);
                    assert_eq!(view.indexed_channel_identity(), channel.rho_path_identity());
                }
                (_, parsed) => panic!("the join record path of {record:?} parsed as {parsed:?}"),
            }
        }
    }

    let indexed_channel = "indexed".to_owned();
    let join = [indexed_channel.clone(), "other".to_owned()];
    let mut join_store = RSpaceJoinStore::<String>::new();
    assert!(join_store.put(&indexed_channel, &join));
    let recorded_paths: Vec<Vec<u8>> = join_store
        .pending_delta()
        .additions()
        .iter()
        .map(|(path, _)| path)
        .collect();
    let path = grammar_join_record_path::<RhoPathV1>(
        &RhoPathV1::join_prefix(&indexed_channel),
        &encode_join_record(&join),
    );
    assert_eq!(
        recorded_paths,
        [path.clone()],
        "the V1 join record path is the path at which the record index stores a join",
    );
    assert_eq!(join_store.count_path(&path), 1);
}
