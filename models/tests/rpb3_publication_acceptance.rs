use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::{
    BindPattern, ListParWithRandom, Par, ParWithRandom, Send, TaggedContinuation, Var,
};
use models::rust::event_hash_bytes::{
    event_hash_bytes_bind_pattern, event_hash_bytes_list_par_with_random,
    event_hash_bytes_tagged_continuation,
};
use models::rust::par_bag::ParBag;
use models::rust::rholang::{bincode_encoder, protobuf_encoder};
use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
use rspace_plus_plus::rspace::history::cold_store::{
    ColdLeafAddress, DataLeaf, PersistedData, PersistedDataKind, PersistedDataView,
};
use rspace_plus_plus::rspace::history::history::{History, HistoryInstances};
use rspace_plus_plus::rspace::history::history_action::{
    HistoryAction, InsertAction as HistoryInsertAction,
};
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::history::CurrentRoot;
use rspace_plus_plus::rspace::hot_store_action::{
    HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode;
use rspace_plus_plus::rspace::serializers::serializers::{
    decode_datum, encode_continuations, encode_datum, encode_datums, encode_joins,
};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::rspace_store_manager::mk_rspace_store_manager;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportError, StateImportPage, StateImportPublicationMode, StateImportPublicationWitness,
    StateImportSpec,
};
use rspace_plus_plus::rspace::state::instances::rspace_importer_store::RSpaceImporterStore;
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporterInstance;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use rspace_plus_plus::rspace::state::rspace_state_authority::RSpaceStateAuthority;
use shared::rust::store::key_value_store::KeyValueStore;
use shared::rust::test_scratch;

#[path = "par_corpus/mod.rs"]
mod par_corpus;

type ParRepository = dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation>
    + std::marker::Send
    + Sync
    + 'static;
type AuthorityImage = [BTreeMap<Vec<u8>, Vec<u8>>; 3];
type ParRepositoryInstances =
    HistoryRepositoryInstances<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

const PUBLICATION_MODEL_CONFIGURATION: &str =
    "formal/tlaplus/stack_safe_pda/RSpaceRPB3CanonicalAdmissionPublication.cfg";
const MODEL_ONLY_INVARIANT: &str = "TypeInvariant";
const PUBLICATION_INVARIANT_BINDINGS: &[(&str, &str)] = &[
    (
        "MalformedInputCannotBeAdmitted",
        "generated_full_par_bincode_wire_matches_typed_oracle_and_refusal",
    ),
    (
        "MalformedRefusalPreservesEveryNamespace",
        "authenticated_malformed_compact_par_refuses_before_durable_publication",
    ),
    (
        "UnpublishedImportHasNoRootAuthority",
        "generated_lifecycle_keeps_unpublished_imports_without_root_authority",
    ),
    (
        "UnpublishedCheckpointPreservesEveryNamespace",
        "generated_lifecycle_keeps_unpublished_checkpoints_out_of_every_namespace",
    ),
    (
        "PublicationRequiresAdmissionAndSeal",
        "generated_authenticated_full_wire_par_import_seals_publishes_and_reopens",
    ),
    (
        "PublicationMatchesTypedOracle",
        "compact_admission_refusal_and_typed_publication_survive_lmdb_reopen",
    ),
    (
        "EveryKnownRootHasCompleteClosureAndExactObservation",
        "generated_lifecycle_keeps_every_known_root_closed_and_exactly_observable",
    ),
    (
        "CurrentRootIsKnown",
        "generated_lifecycle_keeps_the_current_root_known",
    ),
    (
        "GenerationAdvancesOnlyOnActivation",
        "generated_lifecycle_advances_the_generation_only_on_activation",
    ),
];
const RSPACE_ROOTS_NAMESPACE: usize = 2;

#[derive(Debug, PartialEq, Eq)]
struct PublicationObservation {
    protobuf: Vec<u8>,
    bincode: Vec<u8>,
    event_preimages: [Vec<u8>; 3],
    event_sources: [Vec<u8>; 2],
    records: [Vec<u8>; 3],
    root: Vec<u8>,
}

fn checkpoint_actions(
    par: &Par,
) -> Vec<HotStoreAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
    let datum = Datum::create(
        par,
        ListParWithRandom {
            pars: vec![par.clone(), par.clone()],
            random_state: vec![1, 7, 9],
            cost_region: None,
        },
        false,
    );
    let channels = vec![par.clone()];
    let pattern = BindPattern {
        patterns: vec![par.clone()],
        remainder: Some(Var::default()),
        free_count: -3,
    };
    let body = TaggedContinuation {
        guard: Some(par.clone()),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(par.clone()),
            random_state: vec![2, 8],
        })),
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let continuation = WaitingContinuation::create(
        &channels,
        &vec![pattern],
        &body,
        true,
        std::collections::BTreeSet::from([0]),
    );
    vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: par.clone(),
            data: vec![datum.clone(), datum],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: channels.clone(),
            continuations: vec![continuation],
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: par.clone(),
            joins: vec![channels],
        })),
    ]
}

fn checkpoint(repo: &ParRepository, par: &Par) -> Box<ParRepository> {
    repo.checkpoint(checkpoint_actions(par))
        .expect("typed checkpoint")
}

fn publish_compact(
    repo: &ParRepository,
    compact: &[u8],
    typed_metadata: &Par,
) -> Result<(Par, Box<ParRepository>), String> {
    let components = ParBag::from_compact_bytes(compact).map_err(|error| error.to_string())?;
    let admitted = Par::from_parts(
        components,
        typed_metadata.locally_free.clone(),
        typed_metadata.connective_used,
    );
    let next = repo
        .checkpoint(checkpoint_actions(&admitted))
        .map_err(|error| error.to_string())?;
    Ok((admitted, next))
}

fn observe(repo: &ParRepository, par: &Par) -> PublicationObservation {
    let datum_value = ListParWithRandom {
        pars: vec![par.clone(), par.clone()],
        random_state: vec![1, 7, 9],
        cost_region: None,
    };
    let pattern = BindPattern {
        patterns: vec![par.clone()],
        remainder: Some(Var::default()),
        free_count: -3,
    };
    let body = TaggedContinuation {
        guard: Some(par.clone()),
        tagged_cont: Some(TaggedCont::ParBody(ParWithRandom {
            body: Some(par.clone()),
            random_state: vec![2, 8],
        })),
        cost_region: None,
        clause_regions: Vec::new(),
    };
    let datum = Datum::create(par, datum_value.clone(), false);
    let continuation = WaitingContinuation::create(
        &vec![par.clone()],
        &vec![pattern.clone()],
        &body,
        true,
        std::collections::BTreeSet::from([0]),
    );
    let root = repo.root();
    let reader = repo.get_history_reader(&root).expect("published reader");
    PublicationObservation {
        protobuf: protobuf_encoder::encode_to_vec(par),
        bincode: bincode_encoder::encode(par),
        event_preimages: [
            event_hash_bytes_list_par_with_random(&datum_value),
            event_hash_bytes_bind_pattern(&pattern),
            event_hash_bytes_tagged_continuation(&body),
        ],
        event_sources: [
            bincode::serialize(&datum.source).expect("datum source serializes"),
            bincode::serialize(&continuation.source).expect("continuation source serializes"),
        ],
        records: [
            encode_datums(&reader.base().get_data(par)),
            encode_continuations(&reader.base().get_continuations(&vec![par.clone()])),
            encode_joins(&reader.base().get_joins(par)),
        ],
        root: root.bytes(),
    }
}

async fn authority_image(manager: &mut impl KeyValueStoreManager) -> AuthorityImage {
    let mut image = [BTreeMap::new(), BTreeMap::new(), BTreeMap::new()];
    for (index, name) in ["rspace-history", "rspace-cold", "rspace-roots"]
        .into_iter()
        .enumerate()
    {
        image[index] = manager
            .store(name.to_owned())
            .await
            .expect("authority namespace")
            .to_map()
            .expect("authority image");
    }
    image
}

fn full_par_wire(typed: &Par, locally_free: &[u8]) -> (Vec<u8>, usize) {
    let canonical = bincode_encoder::encode(typed);
    assert_eq!(
        canonical,
        bincode::serialize(typed).expect("typed Par serializes with bincode")
    );
    let compact = typed.compact_snapshot();
    assert_eq!(
        u64::from_le_bytes(
            canonical[..8]
                .try_into()
                .expect("canonical wire starts with an eight-byte compact length")
        ) as usize,
        compact.len()
    );
    assert_eq!(&canonical[8..8 + compact.len()], compact);
    let metadata_offset = 8 + compact.len();
    assert_eq!(&canonical[metadata_offset..metadata_offset + 8], &[0; 8]);
    assert_eq!(canonical.len(), metadata_offset + 9);
    assert_eq!(
        canonical[metadata_offset + 8],
        u8::from(typed.connective_used())
    );
    let mut wire = canonical[..metadata_offset].to_vec();
    wire.extend_from_slice(&(locally_free.len() as u64).to_le_bytes());
    wire.extend_from_slice(locally_free);
    wire.push(u8::from(typed.connective_used()));
    (wire, metadata_offset)
}

fn authenticated_data_page(channel: &Par, record: Vec<u8>) -> (Blake2b256Hash, StateImportPage) {
    let leaf_bytes = bincode::serialize(&PersistedData::Data(DataLeaf {
        bytes: bincode::serialize(&vec![record]).expect("datum record list serializes"),
    }))
    .expect("persisted data leaf serializes");
    let digest = Blake2b256Hash::new(
        PersistedDataView::parse_canonical(&leaf_bytes)
            .expect("serialized data leaf parses as canonical")
            .leaf_encoding(),
    );
    let address = ColdLeafAddress::new(PersistedDataKind::Data, digest.clone())
        .expect("data leaf digest forms a cold leaf address");
    let source_history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());
    let history = HistoryInstances::create(
        RadixHistory::empty_root_node_hash(),
        Arc::clone(&source_history),
    )
    .expect("source history opens at the empty root");
    let mut key = vec![0];
    key.extend(hash(channel).bytes());
    assert_eq!(key.len(), 33);
    let root = history
        .process(vec![HistoryAction::Insert(HistoryInsertAction {
            key,
            hash: digest.clone(),
        })])
        .expect("source history accepts the data insert")
        .root();
    let start_path = vec![(root.clone(), None)];
    let traversal = RSpaceExporterInstance::traverse_history_page(
        start_path.clone(),
        0,
        1024,
        Arc::new({
            let source_history = Arc::clone(&source_history);
            move |hash| source_history.get_one(hash).map_err(Into::into)
        }),
    )
    .expect("source history page traverses");
    let history_items = traversal
        .nodes
        .iter()
        .filter(|node| !node.is_leaf)
        .map(|node| {
            (
                node.hash.clone(),
                source_history
                    .get_one(&node.hash.0)
                    .expect("source history store reads the node")
                    .expect("traversed node is stored in the source history"),
            )
        })
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .collect();
    let leaves: Vec<_> = traversal.nodes.iter().filter(|node| node.is_leaf).collect();
    assert_eq!(leaves.len(), 1);
    assert_eq!(leaves[0].leaf_address.as_ref(), Some(&address));
    (root, StateImportPage {
        start_path,
        advertised_next_path: traversal.next_path,
        skip: 0,
        take: 1024,
        history_items,
        data_items: vec![(digest, leaf_bytes)],
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_full_par_bincode_wire_matches_typed_oracle_and_refusal(
        value in any::<i16>(),
        locally_free in proptest::collection::vec(any::<u8>(), 1..9),
        connective_used in any::<bool>(),
    ) {
        let mut typed = par_corpus::gint(i64::from(value));
        typed.replace_locally_free(locally_free.clone());
        typed.replace_connective_used(connective_used);
        let (wire, metadata_offset) = full_par_wire(&typed, &locally_free);
        let oracle: Par = bincode::deserialize(&wire).expect("full wire deserializes with bincode");
        let decoded = Par::cold_decode(&wire).expect("full wire cold-decodes");
        prop_assert_eq!(&decoded, &oracle);
        prop_assert_eq!(decoded.locally_free(), oracle.locally_free());
        prop_assert_eq!(decoded.connective_used(), oracle.connective_used());
        prop_assert_eq!(&oracle, &typed);
        prop_assert_eq!(oracle.locally_free(), locally_free.as_slice());
        prop_assert_eq!(oracle.connective_used(), connective_used);

        let mut changed_metadata = wire.clone();
        changed_metadata[metadata_offset + 8] ^= 1;
        let changed_oracle: Par = bincode::deserialize(&changed_metadata).expect("wire with changed metadata deserializes with bincode");
        prop_assert_eq!(changed_oracle.compact_snapshot(), oracle.compact_snapshot());
        prop_assert_ne!(changed_oracle.locally_free(), oracle.locally_free());
        let changed_decoded = Par::cold_decode(&changed_metadata).expect("wire with changed metadata cold-decodes");
        prop_assert_eq!(&changed_decoded, &changed_oracle);
        prop_assert_eq!(changed_decoded.locally_free(), changed_oracle.locally_free());

        let mut invalid_bool = wire.clone();
        *invalid_bool.last_mut().expect("full wire is not empty") = 2;
        prop_assert!(bincode::deserialize::<Par>(&invalid_bool).is_err());
        prop_assert!(Par::cold_decode(&invalid_bool).is_err());

        let truncated = &wire[..wire.len() - 1];
        prop_assert!(bincode::deserialize::<Par>(truncated).is_err());
        prop_assert!(Par::cold_decode(truncated).is_err());

        let mut overlong_metadata = wire.clone();
        overlong_metadata[metadata_offset..metadata_offset + 8]
            .copy_from_slice(&(locally_free.len() as u64 + 2).to_le_bytes());
        prop_assert!(bincode::deserialize::<Par>(&overlong_metadata).is_err());
        prop_assert!(Par::cold_decode(&overlong_metadata).is_err());
    }
}

#[tokio::test]
async fn compact_admission_refusal_and_typed_publication_survive_lmdb_reopen() {
    let scratch = test_scratch::acquire("rpb3-publication-acceptance-");
    let mut manager = mk_rspace_store_manager(scratch.path().to_path_buf(), 32 * 1024 * 1024);
    let authority = manager
        .state_authority("rspace")
        .await
        .expect("durable authority");
    let admitted_repo = HistoryRepositoryInstances::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >::repository(authority.clone())
    .expect("admitted repository");
    let oracle_authority = RSpaceStateAuthority::in_memory();
    let oracle_repo = HistoryRepositoryInstances::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >::repository(oracle_authority.clone())
    .expect("typed repository");
    let typed_first = par_corpus::gint(17);
    let mut typed_second = par_corpus::gint(23);
    typed_second.replace_sends(vec![Send {
        chan: Some(par_corpus::gint(4)),
        data: vec![par_corpus::gint(5)],
        ..Send::default()
    }]);
    typed_second.replace_locally_free(vec![0xE1, 0xE2]);
    typed_second.replace_connective_used(true);
    let first_compact = typed_first.compact_snapshot().to_vec();
    let second_compact = typed_second.compact_snapshot().to_vec();
    assert!(second_compact.len() > 1);

    let oracle_first = checkpoint(oracle_repo.as_ref(), &typed_first);
    let (admitted_first, actual_first) =
        publish_compact(admitted_repo.as_ref(), &first_compact, &typed_first)
            .expect("first compact admission");
    assert_eq!(
        observe(actual_first.as_ref(), &admitted_first),
        observe(oracle_first.as_ref(), &typed_first)
    );
    let first_current = authority
        .current_verified_root()
        .expect("current verified root reads")
        .expect("first publication activates a current root");
    assert_eq!(first_current.root(), &oracle_first.root());
    assert!(authority
        .contains_verified_root(first_current.root())
        .expect("verified-root membership reads"));

    let before_refusal = authority_image(&mut manager).await;
    let mut refused = 0;
    for length in 1..second_compact.len() {
        let prefix = &second_compact[..length];
        if ParBag::from_compact_bytes(prefix).is_err() {
            refused += 1;
            assert!(publish_compact(actual_first.as_ref(), prefix, &typed_second).is_err());
        }
    }
    assert!(refused > 0, "truncated compact images must be refused");
    assert_eq!(authority_image(&mut manager).await, before_refusal);
    assert_eq!(
        authority
            .current_verified_root()
            .expect("current verified root reads"),
        Some(first_current.clone())
    );

    let oracle_second = checkpoint(oracle_first.as_ref(), &typed_second);
    let (admitted_second, actual_second) =
        publish_compact(actual_first.as_ref(), &second_compact, &typed_second)
            .expect("second compact admission");
    assert_eq!(
        observe(actual_second.as_ref(), &admitted_second),
        observe(oracle_second.as_ref(), &typed_second)
    );
    let second_current = authority
        .current_verified_root()
        .expect("current verified root reads")
        .expect("second publication activates a current root");
    assert_eq!(second_current.root(), &oracle_second.root());
    assert_eq!(second_current.generation(), first_current.generation() + 1);
    assert!(authority
        .contains_verified_root(first_current.root())
        .expect("verified-root membership reads"));
    assert!(authority
        .contains_verified_root(second_current.root())
        .expect("verified-root membership reads"));

    let reset = authority
        .activate_known(
            authority
                .known_verified_root(first_current.root())
                .expect("first root is a known verified root"),
        )
        .expect("activate first known root");
    assert_eq!(reset.root(), first_current.root());
    assert_eq!(reset.generation(), second_current.generation() + 1);
    drop(actual_second);
    drop(actual_first);
    drop(admitted_repo);
    drop(authority);
    manager.shutdown().await.expect("close durable manager");
    drop(manager);

    let mut reopened_manager =
        mk_rspace_store_manager(scratch.path().to_path_buf(), 32 * 1024 * 1024);
    let reopened = reopened_manager
        .state_authority("rspace")
        .await
        .expect("reopened authority");
    assert_eq!(
        reopened
            .current_verified_root()
            .expect("current verified root reads after reopen"),
        Some(reset.clone())
    );
    assert!(reopened
        .contains_verified_root(first_current.root())
        .expect("verified-root membership reads after reopen"));
    assert!(reopened
        .contains_verified_root(second_current.root())
        .expect("verified-root membership reads after reopen"));
    let before_reactivation = authority_image(&mut reopened_manager).await;
    let restored = reopened
        .activate_known(
            reopened
                .known_verified_root(second_current.root())
                .expect("second root is a known verified root after reopen"),
        )
        .expect("activate second known root");
    assert_eq!(restored.root(), second_current.root());
    assert_eq!(restored.generation(), reset.generation() + 1);
    assert_ne!(
        authority_image(&mut reopened_manager).await,
        before_reactivation
    );
    let reopened_repo = HistoryRepositoryInstances::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >::repository(reopened.clone())
    .expect("reopened repository");
    assert_eq!(
        observe(reopened_repo.as_ref(), &admitted_second),
        observe(oracle_second.as_ref(), &typed_second)
    );
}

#[tokio::test]
async fn authenticated_malformed_compact_par_refuses_before_durable_publication() {
    let typed = par_corpus::gint(41);
    let compact = typed.compact_snapshot();
    let rejected_prefixes: Vec<_> = (1..compact.len())
        .filter(|length| ParBag::from_compact_bytes(&compact[..*length]).is_err())
        .take(4)
        .collect();
    assert!(rejected_prefixes.len() >= 2);

    let target_scratch = test_scratch::acquire("rpb3-import-refusal-target-");
    let mut target_manager =
        mk_rspace_store_manager(target_scratch.path().to_path_buf(), 32 * 1024 * 1024);
    let target = target_manager
        .state_authority("rspace")
        .await
        .expect("import target authority opens");
    let initial_repo = HistoryRepositoryInstances::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >::repository(target.clone())
    .expect("import target repository opens");
    let baseline_repo = checkpoint(initial_repo.as_ref(), &par_corpus::gint(7));
    let before_current = target
        .current_verified_root()
        .expect("current verified root reads")
        .expect("baseline checkpoint activates a current root");
    let before = authority_image(&mut target_manager).await;
    let importer =
        RSpaceImporterStore::create::<Par, BindPattern, ListParWithRandom, TaggedContinuation>(
            target.clone(),
        );

    let source_history: Arc<dyn KeyValueStore> = Arc::new(InMemoryKeyValueStore::new());

    for length in rejected_prefixes {
        let datum = Datum::create(
            &typed,
            ListParWithRandom {
                pars: vec![typed.clone()],
                random_state: vec![7],
                cost_region: None,
            },
            false,
        );
        let record = encode_datum(&datum);
        assert_eq!(
            u64::from_le_bytes(
                record[..8]
                    .try_into()
                    .expect("datum record starts with an eight-byte count")
            ),
            1
        );
        let original_len = usize::try_from(u64::from_le_bytes(
            record[8..16]
                .try_into()
                .expect("datum record holds an eight-byte Par length"),
        ))
        .expect("Par length fits in usize");
        assert_eq!(&record[16..16 + original_len], compact);
        let mut malformed_record = record[..8].to_vec();
        malformed_record.extend_from_slice(&(length as u64).to_le_bytes());
        malformed_record.extend_from_slice(&compact[..length]);
        malformed_record.extend_from_slice(&record[16 + original_len..]);
        assert!(decode_datum::<ListParWithRandom>(&malformed_record).is_err());

        let leaf_bytes = bincode::serialize(&PersistedData::Data(DataLeaf {
            bytes: bincode::serialize(&vec![malformed_record])
                .expect("malformed record list serializes"),
        }))
        .expect("persisted data leaf serializes");
        let digest = Blake2b256Hash::new(
            PersistedDataView::parse_canonical(&leaf_bytes)
                .expect("serialized data leaf parses as canonical")
                .leaf_encoding(),
        );
        let address = ColdLeafAddress::new(PersistedDataKind::Data, digest.clone())
            .expect("data leaf digest forms a cold leaf address");
        let history = HistoryInstances::create(
            RadixHistory::empty_root_node_hash(),
            Arc::clone(&source_history),
        )
        .expect("source history opens at the empty root");
        let mut key = vec![0];
        key.extend([41; 32]);
        let next = history
            .process(vec![HistoryAction::Insert(HistoryInsertAction {
                key,
                hash: digest.clone(),
            })])
            .expect("source history accepts the data insert");
        let root = next.root();
        let start_path = vec![(root.clone(), None)];
        let traversal = RSpaceExporterInstance::traverse_history_page(
            start_path.clone(),
            0,
            1024,
            Arc::new({
                let source_history = Arc::clone(&source_history);
                move |hash| source_history.get_one(hash).map_err(Into::into)
            }),
        )
        .expect("source history page traverses");
        let history_items = traversal
            .nodes
            .iter()
            .filter(|node| !node.is_leaf)
            .map(|node| {
                (
                    node.hash.clone(),
                    source_history
                        .get_one(&node.hash.0)
                        .expect("source history store reads the node")
                        .expect("traversed node is stored in the source history"),
                )
            })
            .collect::<BTreeMap<_, _>>()
            .into_iter()
            .collect();
        let leaves: Vec<_> = traversal.nodes.iter().filter(|node| node.is_leaf).collect();
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].leaf_address.as_ref(), Some(&address));
        let mut session = importer
            .begin_import(
                StateImportSpec::new(
                    [root.clone()],
                    StateImportPublicationMode::Activate(root.clone()),
                )
                .expect("import spec for the malformed root is valid"),
            )
            .expect("malformed import session begins");
        assert!(matches!(
            session.validate_and_stage_page(StateImportPage {
                start_path,
                advertised_next_path: traversal.next_path,
                skip: 0,
                take: 1024,
                history_items,
                data_items: vec![(digest, leaf_bytes)],
            }),
            Err(StateImportError::MalformedRecord { .. })
        ));
        assert!(matches!(
            session.seal(),
            Err(StateImportError::IncompletePagination(_))
        ));
        assert!(target.known_verified_root(&root).is_err());
        assert_eq!(
            target
                .current_verified_root()
                .expect("current verified root reads"),
            Some(before_current.clone())
        );
        assert_eq!(authority_image(&mut target_manager).await, before);
    }

    drop(importer);
    drop(baseline_repo);
    drop(initial_repo);
    drop(target);
    target_manager
        .shutdown()
        .await
        .expect("import target manager closes");
    drop(target_manager);
    let mut reopened_manager =
        mk_rspace_store_manager(target_scratch.path().to_path_buf(), 32 * 1024 * 1024);
    let reopened = reopened_manager
        .state_authority("rspace")
        .await
        .expect("import target authority reopens");
    assert_eq!(
        reopened
            .current_verified_root()
            .expect("current verified root reads after reopen"),
        Some(before_current)
    );
    assert_eq!(authority_image(&mut reopened_manager).await, before);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_authenticated_full_wire_par_import_seals_publishes_and_reopens(
        value in any::<i16>(),
        locally_free in proptest::collection::vec(any::<u8>(), 1..6),
        connective_used in any::<bool>(),
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime builds");
        runtime.block_on(async move {
            let mut typed = par_corpus::gint(i64::from(value));
            typed.replace_locally_free(locally_free.clone());
            typed.replace_connective_used(connective_used);
            let (wire, _) = full_par_wire(&typed, &locally_free);
            let oracle: Par = bincode::deserialize(&wire).expect("full wire deserializes with bincode");
            let decoded = Par::cold_decode(&wire).expect("full wire cold-decodes");
            assert_eq!(decoded, oracle);
            assert_eq!(decoded, typed);
            assert_eq!(decoded.locally_free(), typed.locally_free());
            assert_eq!(decoded.connective_used(), typed.connective_used());

            let channel = par_corpus::gint(i64::from(value) + 1);
            let payload = ListParWithRandom {
                pars: vec![typed.clone()],
                random_state: vec![7, locally_free[0]],
                cost_region: None,
            };
            let datum = Datum::create(&channel, payload.clone(), false);
            let encoded = encode_datum(&datum);
            assert_eq!(u64::from_le_bytes(encoded[..8].try_into().expect("datum record starts with an eight-byte count")), 1);
            let canonical_len =
                usize::try_from(u64::from_le_bytes(encoded[8..16].try_into().expect("datum record holds an eight-byte Par length"))).expect("Par length fits in usize");
            let canonical_par = bincode_encoder::encode(&typed);
            assert_eq!(canonical_len, typed.compact_snapshot().len());
            assert_eq!(&encoded[8..8 + canonical_par.len()], canonical_par.as_slice());
            let mut record = encoded[..8].to_vec();
            record.extend_from_slice(&wire);
            record.extend_from_slice(&encoded[8 + canonical_par.len()..]);
            let decoded_record = decode_datum::<ListParWithRandom>(&record).expect("full-wire datum record decodes");
            assert_eq!(decoded_record.a.as_ref(), &payload);
            assert_eq!(decoded_record.a.pars[0].locally_free(), typed.locally_free());
            assert_eq!(decoded_record.a.pars[0].connective_used(), typed.connective_used());
            assert_eq!(decoded_record.source, datum.source);

            let (root, valid_page) = authenticated_data_page(&channel, record);
            let mut malformed_wire = wire.clone();
            *malformed_wire.last_mut().expect("full wire is not empty") = 2;
            let mut malformed_record = encoded[..8].to_vec();
            malformed_record.extend_from_slice(&malformed_wire);
            malformed_record.extend_from_slice(&encoded[8 + canonical_par.len()..]);
            assert!(decode_datum::<ListParWithRandom>(&malformed_record).is_err());
            let (malformed_root, malformed_page) =
                authenticated_data_page(&channel, malformed_record);
            assert_ne!(malformed_root, root);

            for preserve_current in [false, true] {
            let scratch = test_scratch::acquire("rpb3-import-full-wire-");
            let mut manager = mk_rspace_store_manager(scratch.path().to_path_buf(), 32 * 1024 * 1024);
            let target = manager.state_authority("rspace").await.expect("import target authority opens");
            let initial_repo = HistoryRepositoryInstances::<
                Par,
                BindPattern,
                ListParWithRandom,
                TaggedContinuation,
            >::repository(target.clone())
            .expect("import target repository opens");
            let before_current = target.current_verified_root().expect("current verified root reads").expect("fresh authority has a current root");
            let before_image = authority_image(&mut manager).await;
            assert!(target.known_verified_root(&root).is_err());
            assert!(target.known_verified_root(&malformed_root).is_err());
            let importer = RSpaceImporterStore::create::<
                Par,
                BindPattern,
                ListParWithRandom,
                TaggedContinuation,
            >(target.clone());
            let truncated_compact = &typed.compact_snapshot()[..1];
            assert!(ParBag::from_compact_bytes(truncated_compact).is_err());
            assert!(publish_compact(initial_repo.as_ref(), truncated_compact, &typed).is_err());
            assert_eq!(target.current_verified_root().expect("current verified root reads"), Some(before_current.clone()));
            assert_eq!(authority_image(&mut manager).await, before_image);

            let mut malformed_session = importer
                .begin_import(
                    StateImportSpec::new(
                        [malformed_root.clone()],
                        StateImportPublicationMode::Activate(malformed_root.clone()),
                    )
                    .expect("import spec for the malformed root is valid"),
                )
                .expect("malformed import session begins");
            assert!(matches!(
                malformed_session.validate_and_stage_page(malformed_page.clone()),
                Err(StateImportError::MalformedRecord { .. })
            ));
            assert!(matches!(
                malformed_session.seal(),
                Err(StateImportError::IncompletePagination(_))
            ));
            assert!(target.known_verified_root(&malformed_root).is_err());
            assert_eq!(target.current_verified_root().expect("current verified root reads"), Some(before_current.clone()));
            assert_eq!(authority_image(&mut manager).await, before_image);

            let publication_mode = if preserve_current {
                StateImportPublicationMode::PreserveCurrent
            } else {
                StateImportPublicationMode::Activate(root.clone())
            };
            let unpaged = importer
                .begin_import(StateImportSpec::new([root.clone()], publication_mode.clone()).expect("import spec for the valid root is valid"))
                .expect("unpaged import session begins");
            assert!(matches!(unpaged.seal(), Err(StateImportError::IncompletePagination(_))));
            assert!(target.known_verified_root(&root).is_err());
            assert_eq!(target.current_verified_root().expect("current verified root reads"), Some(before_current.clone()));
            let mut session = importer
                .begin_import(
                    StateImportSpec::new([root.clone()], publication_mode).expect("import spec for the valid root is valid"),
                )
                .expect("import session begins");
            let receipt = session
                .validate_and_stage_page(valid_page.clone())
                .expect("valid page validates and stages");
            assert!(receipt.terminal);
            assert_eq!(receipt.next_path, valid_page.start_path);
            assert_eq!(receipt.history_objects_staged, valid_page.history_items.len());
            assert_eq!(receipt.cold_leaves_staged, 1);
            assert!(target.known_verified_root(&root).is_err());
            assert_eq!(target.current_verified_root().expect("current verified root reads"), Some(before_current.clone()));
            let witness = StateImportPublicationWitness::new();
            let sealed = session.seal().expect("fully paged session seals");
            assert!(target.known_verified_root(&root).is_err());
            assert_eq!(target.current_verified_root().expect("current verified root reads"), Some(before_current.clone()));
            let publication = sealed.publish(&witness).expect("sealed import publishes");
            assert!(witness.authoritative_publication_observed());
            assert_eq!(publication.verified_roots, vec![root.clone()]);
            assert!(target.known_verified_root(&root).is_ok());
            let published_current = target.current_verified_root().expect("current verified root reads").expect("authority keeps a current root after publication");
            if preserve_current {
                assert!(publication.activation.is_none());
                assert_eq!(published_current, before_current);
            } else {
                assert_eq!(published_current.root(), &root);
                assert_eq!(published_current.generation(), before_current.generation() + 1);
                assert_eq!(publication.activation.expect("activating publication reports its activation").generation, published_current.generation());
            }
            assert!(target.known_verified_root(published_current.root()).is_ok());

            let published_repo = HistoryRepositoryInstances::<
                Par,
                BindPattern,
                ListParWithRandom,
                TaggedContinuation,
            >::repository(target.clone())
            .expect("published repository opens");
            let published_reader = published_repo.get_history_reader(&root).expect("published root has a history reader");
            let observed = published_reader.base().get_data(&channel);
            assert_eq!(observed.len(), 1);
            assert_eq!(observed[0].a.as_ref(), &payload);
            assert_eq!(observed[0].a.pars[0].locally_free(), typed.locally_free());
            assert_eq!(observed[0].a.pars[0].connective_used(), typed.connective_used());
            assert_eq!(observed[0].source, datum.source);
            drop(published_reader);
            assert!(target.known_verified_root(before_current.root()).is_ok());
            let base_reader = published_repo
                .get_history_reader(before_current.root())
                .expect("previous root keeps a history reader");
            assert!(base_reader.base().get_data(&channel).is_empty());
            drop(base_reader);
            drop(published_repo);
            let current = if preserve_current {
                target.activate_known(target.known_verified_root(&root).expect("imported root is a known verified root")).expect("known imported root activates")
            } else {
                published_current
            };
            assert_eq!(current.root(), &root);
            assert_eq!(current.generation(), before_current.generation() + 1);
            assert!(target.known_verified_root(current.root()).is_ok());
            let published_image = authority_image(&mut manager).await;
            assert_ne!(published_image, before_image);
            drop(importer);
            drop(initial_repo);
            drop(target);
            manager.shutdown().await.expect("import target manager closes");
            drop(manager);

            let mut reopened_manager =
                mk_rspace_store_manager(scratch.path().to_path_buf(), 32 * 1024 * 1024);
            let reopened = reopened_manager.state_authority("rspace").await.expect("import target authority reopens");
            assert_eq!(reopened.current_verified_root().expect("current verified root reads after reopen"), Some(current));
            assert_eq!(authority_image(&mut reopened_manager).await, published_image);
            let reopened_repo = HistoryRepositoryInstances::<
                Par,
                BindPattern,
                ListParWithRandom,
                TaggedContinuation,
            >::repository(reopened.clone())
            .expect("reopened repository opens");
            let reopened_reader = reopened_repo.get_history_reader(&root).expect("reopened repository has a reader for the imported root");
            let reopened_data = reopened_reader.base().get_data(&channel);
            assert_eq!(reopened_data.len(), 1);
            assert_eq!(reopened_data[0].a.as_ref(), &payload);
            assert_eq!(reopened_data[0].a.pars[0].locally_free(), typed.locally_free());
            assert_eq!(reopened_data[0].a.pars[0].connective_used(), typed.connective_used());
            assert_eq!(reopened_data[0].source, datum.source);
            }
        });
    }
}

fn test_definition_count(source: &str, test: &str) -> usize {
    let mut carries_test_attribute = false;
    let mut definitions = 0;
    for line in source.lines().map(str::trim) {
        match line {
            "#[test]" | "#[tokio::test]" => carries_test_attribute = true,
            attribute if attribute.starts_with("#[") => {}
            definition => {
                let parameters = definition
                    .strip_prefix("async fn ")
                    .or_else(|| definition.strip_prefix("fn "))
                    .and_then(|signature| signature.strip_prefix(test));
                definitions += match (carries_test_attribute, parameters) {
                    (true, Some(parameters)) if parameters.starts_with('(') => 1,
                    _ => 0,
                };
                carries_test_attribute = false;
            }
        }
    }
    definitions
}

fn binding_defects(configuration: &str, source: &str, bindings: &[(&str, &str)]) -> Vec<String> {
    let mut defects = Vec::new();
    let mut configured = Vec::with_capacity(bindings.len() + 1);
    for statement in configuration.lines() {
        let mut tokens = statement.split_whitespace();
        match (tokens.next(), tokens.next(), tokens.next()) {
            (Some("INVARIANT"), Some(invariant), None) => configured.push(invariant),
            (Some("INVARIANT" | "INVARIANTS"), _, _) => defects.push(format!(
                "configuration statement `{}` does not name exactly one invariant",
                statement.trim()
            )),
            _ => {}
        }
    }
    defects.extend(
        configured
            .iter()
            .filter(|invariant| **invariant != MODEL_ONLY_INVARIANT)
            .filter_map(|invariant| {
                match bindings
                    .iter()
                    .filter(|(bound, _)| bound == invariant)
                    .count()
                {
                    1 => None,
                    0 => Some(format!(
                        "configured invariant {invariant} has no bound test"
                    )),
                    rows => Some(format!(
                        "configured invariant {invariant} has {rows} bound tests"
                    )),
                }
            }),
    );
    defects.extend(
        bindings
            .iter()
            .filter(|(invariant, _)| !configured.contains(invariant))
            .map(|(invariant, _)| {
                format!("bound invariant {invariant} is not checked by the configuration")
            }),
    );
    defects.extend(bindings.iter().filter_map(|(_, test)| {
        match test_definition_count(source, test) {
            1 => None,
            definitions => Some(format!(
                "bound test {test} is defined {definitions} times as a test function"
            )),
        }
    }));
    defects
}

#[test]
fn publication_invariant_bindings_cover_the_model_configuration() {
    let configuration = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("models crate sits inside the repository root")
            .join(PUBLICATION_MODEL_CONFIGURATION),
    )
    .expect("RPB3 publication model configuration reads");
    let source = include_str!("rpb3_publication_acceptance.rs");
    let bindings = PUBLICATION_INVARIANT_BINDINGS;
    let no_defects = Vec::<String>::new();
    assert_eq!(
        binding_defects(&configuration, source, bindings),
        no_defects,
        "the binding table, the model configuration and the bound tests are out of step"
    );

    for (removed, (invariant, _)) in bindings.iter().enumerate() {
        let remaining = [&bindings[..removed], &bindings[removed + 1..]].concat();
        assert_eq!(binding_defects(&configuration, source, &remaining), vec![
            format!("configured invariant {invariant} has no bound test")
        ]);
    }

    let extended = format!("{configuration}\nINVARIANT SomeNewInvariant\n");
    assert_eq!(binding_defects(&extended, source, bindings), vec![
        "configured invariant SomeNewInvariant has no bound test".to_owned()
    ]);

    let mut misbound = bindings.to_vec();
    misbound[0].1 = "lifecycle_test_that_is_not_defined";
    assert_eq!(binding_defects(&configuration, source, &misbound), vec![
        "bound test lifecycle_test_that_is_not_defined is defined 0 times as a test function"
            .to_owned()
    ]);

    let (first_invariant, first_test) = bindings[0];
    let doubled = [bindings, &bindings[..1]].concat();
    assert_eq!(binding_defects(&configuration, source, &doubled), vec![
        format!("configured invariant {first_invariant} has 2 bound tests")
    ]);

    let retired = [bindings, &[("RetiredInvariant", first_test)]].concat();
    assert_eq!(binding_defects(&configuration, source, &retired), vec![
        "bound invariant RetiredInvariant is not checked by the configuration".to_owned()
    ]);

    let redefined = format!("{source}\n#[test]\nfn {first_test}() {{}}\n");
    assert_eq!(binding_defects(&configuration, &redefined, bindings), vec![
        format!("bound test {first_test} is defined 2 times as a test function")
    ]);

    let solo = [("Solo", "solo_test")];
    assert_eq!(
        binding_defects("INVARIANT Solo", "#[test]\nfn solo_test() {}", &solo),
        no_defects
    );
    assert_eq!(
        binding_defects(
            "INVARIANT Solo",
            "#[tokio::test]\nasync fn solo_test() {}",
            &solo
        ),
        no_defects
    );
    assert_eq!(
        binding_defects("INVARIANT Solo", "fn solo_test() {}", &solo),
        vec!["bound test solo_test is defined 0 times as a test function".to_owned()]
    );
    assert_eq!(
        binding_defects("INVARIANTS Solo Other", "#[test]\nfn solo_test() {}", &solo),
        vec![
            "configuration statement `INVARIANTS Solo Other` does not name exactly one invariant"
                .to_owned(),
            "bound invariant Solo is not checked by the configuration".to_owned(),
        ]
    );
}

#[derive(Clone, Copy, Debug)]
enum LifecycleStep {
    Checkpoint,
    CheckpointUnchanged,
    RefusedCheckpoint,
    ImportActivate,
    ImportPreserve,
    MalformedImport,
    ActivateKnown(u8),
}

#[derive(Clone, Copy, Debug)]
enum LifecyclePhase {
    StepStart,
    CheckpointAccepted,
    CheckpointWithoutActionsReturned,
    CheckpointWithoutEffectReturned,
    ImportStaged,
    ImportSealed,
    Rejected,
    Published,
    StepEnd,
}

#[derive(Clone, Copy)]
enum ImportRecordForm {
    FullWire,
    MalformedMetadata,
}

struct ImportCandidateObservation {
    root: Blake2b256Hash,
    is_verified_member: bool,
}

struct KnownRootObservation {
    root: Blake2b256Hash,
    is_verified_member: bool,
    reader_opens: bool,
    observed_data: Result<Vec<u8>, String>,
    expected_data: Vec<u8>,
}

struct LifecycleObservation {
    step_index: usize,
    step: LifecycleStep,
    phase: LifecyclePhase,
    image: AuthorityImage,
    step_start_image: AuthorityImage,
    current: CurrentRoot,
    step_start_current: CurrentRoot,
    import_candidate: Option<ImportCandidateObservation>,
    unchanged_checkpoint_root: Option<Blake2b256Hash>,
    activations: u64,
    initial_generation: u64,
    current_is_verified_member: bool,
    current_is_accepted_as_known: bool,
    known_roots: Vec<KnownRootObservation>,
}

struct GhostRoot {
    root: Blake2b256Hash,
    channel: Par,
    expected_data: Vec<u8>,
}

struct LifecycleGhost {
    initial_generation: u64,
    activations: u64,
    known_roots: Vec<GhostRoot>,
}

struct StepFrame {
    index: usize,
    step: LifecycleStep,
    start_image: AuthorityImage,
    start_current: CurrentRoot,
    import_candidate: Option<Blake2b256Hash>,
    unchanged_checkpoint_root: Option<Blake2b256Hash>,
}

struct LifecycleImport {
    channel: Par,
    datum: Datum<ListParWithRandom>,
    root: Blake2b256Hash,
    page: StateImportPage,
}

struct Lifecycle<M> {
    base: i16,
    manager: M,
    authority: Arc<RSpaceStateAuthority>,
    reader_repository: Box<ParRepository>,
    importer: Box<dyn RSpaceImporter>,
    ghost: LifecycleGhost,
}

fn lifecycle_step_tag(step_index: usize) -> u8 {
    u8::try_from(step_index).expect("lifecycle step index fits in a byte")
}

fn lifecycle_value(base: i16, step_index: usize) -> i64 {
    i64::from(base) + i64::from(lifecycle_step_tag(step_index))
}

fn lifecycle_par(base: i16, step_index: usize) -> Par {
    let mut typed = par_corpus::gint(lifecycle_value(base, step_index));
    typed.replace_locally_free(vec![0xE0, lifecycle_step_tag(step_index)]);
    typed.replace_connective_used(step_index % 2 == 1);
    typed
}

fn lifecycle_import(base: i16, step_index: usize, form: ImportRecordForm) -> LifecycleImport {
    let typed = lifecycle_par(base, step_index);
    let channel = par_corpus::gint(lifecycle_value(base, step_index));
    let datum = Datum::create(
        &channel,
        ListParWithRandom {
            pars: vec![typed.clone()],
            random_state: vec![7, lifecycle_step_tag(step_index)],
            cost_region: None,
        },
        false,
    );
    let (mut wire, _) = full_par_wire(&typed, typed.locally_free());
    *wire.last_mut().expect("full wire is not empty") = match form {
        ImportRecordForm::FullWire => u8::from(typed.connective_used()),
        ImportRecordForm::MalformedMetadata => 2,
    };
    let encoded = encode_datum(&datum);
    let canonical_len = bincode_encoder::encode(&typed).len();
    let mut record = Vec::with_capacity(encoded.len() - canonical_len + wire.len());
    record.extend_from_slice(&encoded[..8]);
    record.extend_from_slice(&wire);
    record.extend_from_slice(&encoded[8 + canonical_len..]);
    let (root, page) = authenticated_data_page(&channel, record);
    LifecycleImport {
        channel,
        datum,
        root,
        page,
    }
}

fn inserted_data_encoding(
    actions: &[HotStoreAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>],
) -> Vec<u8> {
    actions
        .iter()
        .find_map(|action| match action {
            HotStoreAction::Insert(InsertAction::InsertData(insert)) => {
                Some(encode_datums(&insert.data))
            }
            _ => None,
        })
        .expect("checkpoint actions insert data")
}

fn absent_record_deletes(
    channel: &Par,
) -> Vec<HotStoreAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
    use rspace_plus_plus::rspace::hot_store_action::{
        DeleteAction, DeleteContinuations, DeleteData, DeleteJoins,
    };
    vec![
        HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
            channel: channel.clone(),
        })),
        HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
            channels: vec![channel.clone()],
        })),
        HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
            channel: channel.clone(),
        })),
    ]
}

async fn open_lifecycle(base: i16, step_count: usize) -> Lifecycle<impl KeyValueStoreManager> {
    let scratch = test_scratch::acquire("rpb3-publication-lifecycle-");
    let mut manager = mk_rspace_store_manager(scratch.path().to_path_buf(), 32 * 1024 * 1024);
    let authority = manager
        .state_authority("rspace")
        .await
        .expect("lifecycle authority opens");
    let reader_repository =
        ParRepositoryInstances::repository(authority.clone()).expect("lifecycle repository opens");
    let importer = Box::new(RSpaceImporterStore::create::<
        Par,
        BindPattern,
        ListParWithRandom,
        TaggedContinuation,
    >(authority.clone()));
    let initial = authority
        .current_verified_root()
        .expect("current verified root reads")
        .expect("fresh authority has a current root");
    let mut known_roots = Vec::with_capacity(step_count + 1);
    known_roots.push(GhostRoot {
        root: initial.root().clone(),
        channel: par_corpus::gint(lifecycle_value(base, 0)),
        expected_data: encode_datums(&Vec::<Datum<ListParWithRandom>>::new()),
    });
    Lifecycle {
        base,
        manager,
        authority,
        reader_repository,
        importer,
        ghost: LifecycleGhost {
            initial_generation: initial.generation(),
            activations: 0,
            known_roots,
        },
    }
}

impl<M: KeyValueStoreManager> Lifecycle<M> {
    fn current(&self) -> CurrentRoot {
        self.authority
            .current_verified_root()
            .expect("current verified root reads")
            .expect("lifecycle authority keeps a current root")
    }

    fn current_repository(&self) -> Box<ParRepository> {
        ParRepositoryInstances::repository(self.authority.clone())
            .expect("repository opens at the current root")
    }

    async fn report(
        &mut self,
        frame: &StepFrame,
        phase: LifecyclePhase,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let current = self.current();
        let mut known_roots = Vec::with_capacity(self.ghost.known_roots.len());
        for known in &self.ghost.known_roots {
            let (reader_opens, observed_data) =
                match self.reader_repository.get_history_reader(&known.root) {
                    Ok(reader) => (
                        true,
                        reader
                            .get_data(&hash(&known.channel))
                            .map(|datums| encode_datums(&datums))
                            .map_err(|error| error.to_string()),
                    ),
                    Err(error) => (false, Err(error.to_string())),
                };
            known_roots.push(KnownRootObservation {
                root: known.root.clone(),
                is_verified_member: self
                    .authority
                    .contains_verified_root(&known.root)
                    .expect("verified-root membership reads"),
                reader_opens,
                observed_data,
                expected_data: known.expected_data.clone(),
            });
        }
        let observation = LifecycleObservation {
            step_index: frame.index,
            step: frame.step,
            phase,
            image: authority_image(&mut self.manager).await,
            step_start_image: frame.start_image.clone(),
            step_start_current: frame.start_current.clone(),
            import_candidate: frame.import_candidate.as_ref().map(|candidate| {
                ImportCandidateObservation {
                    root: candidate.clone(),
                    is_verified_member: self
                        .authority
                        .contains_verified_root(candidate)
                        .expect("verified-root membership reads"),
                }
            }),
            unchanged_checkpoint_root: frame.unchanged_checkpoint_root.clone(),
            activations: self.ghost.activations,
            initial_generation: self.ghost.initial_generation,
            current_is_verified_member: self
                .authority
                .contains_verified_root(current.root())
                .expect("verified-root membership reads"),
            current_is_accepted_as_known: self
                .authority
                .known_verified_root(current.root())
                .is_ok(),
            current,
            known_roots,
        };
        check(&observation);
    }

    async fn begin_step(
        &mut self,
        index: usize,
        step: LifecycleStep,
        import_candidate: Option<Blake2b256Hash>,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) -> StepFrame {
        let frame = StepFrame {
            index,
            step,
            start_image: authority_image(&mut self.manager).await,
            start_current: self.current(),
            import_candidate,
            unchanged_checkpoint_root: None,
        };
        self.report(&frame, LifecyclePhase::StepStart, check).await;
        frame
    }

    async fn checkpoint(
        &mut self,
        index: usize,
        step: LifecycleStep,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let typed = lifecycle_par(self.base, index);
        let frame = self.begin_step(index, step, None, check).await;
        let components = ParBag::from_compact_bytes(typed.compact_snapshot())
            .expect("canonical compact image is admitted");
        let admitted = Par::from_parts(
            components,
            typed.locally_free.clone(),
            typed.connective_used,
        );
        self.report(&frame, LifecyclePhase::CheckpointAccepted, check)
            .await;
        let actions = checkpoint_actions(&admitted);
        let expected_data = inserted_data_encoding(&actions);
        let published_root = self
            .current_repository()
            .checkpoint(actions)
            .expect("admitted checkpoint publishes")
            .root();
        self.ghost.activations += 1;
        self.ghost.known_roots.push(GhostRoot {
            root: published_root,
            channel: admitted,
            expected_data,
        });
        self.report(&frame, LifecyclePhase::Published, check).await;
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn checkpoint_unchanged(
        &mut self,
        index: usize,
        step: LifecycleStep,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let mut frame = self.begin_step(index, step, None, check).await;
        for (phase, actions) in [
            (LifecyclePhase::CheckpointWithoutActionsReturned, Vec::new()),
            (
                LifecyclePhase::CheckpointWithoutEffectReturned,
                absent_record_deletes(&lifecycle_par(self.base, index)),
            ),
        ] {
            frame.unchanged_checkpoint_root = Some(
                self.current_repository()
                    .checkpoint(actions)
                    .expect("a checkpoint that changes nothing is accepted")
                    .root(),
            );
            self.report(&frame, phase, check).await;
        }
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn refuse_checkpoint(
        &mut self,
        index: usize,
        step: LifecycleStep,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let typed = lifecycle_par(self.base, index);
        let compact = typed.compact_snapshot();
        let refused_length = (1..compact.len())
            .find(|length| ParBag::from_compact_bytes(&compact[..*length]).is_err())
            .expect("a truncated compact image is refused by admission");
        let frame = self.begin_step(index, step, None, check).await;
        assert!(
            publish_compact(
                self.current_repository().as_ref(),
                &compact[..refused_length],
                &typed
            )
            .is_err(),
            "a refused compact image must not reach a checkpoint"
        );
        self.report(&frame, LifecyclePhase::Rejected, check).await;
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn import(
        &mut self,
        index: usize,
        step: LifecycleStep,
        activates: bool,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let import = lifecycle_import(self.base, index, ImportRecordForm::FullWire);
        let frame = self
            .begin_step(index, step, Some(import.root.clone()), check)
            .await;
        let (publication_mode, activations) = match activates {
            true => (StateImportPublicationMode::Activate(import.root.clone()), 1),
            false => (StateImportPublicationMode::PreserveCurrent, 0),
        };
        let mut session = self
            .importer
            .begin_import(
                StateImportSpec::new([import.root.clone()], publication_mode)
                    .expect("import spec for the candidate root is valid"),
            )
            .expect("import session begins");
        session
            .validate_and_stage_page(import.page)
            .expect("authenticated page validates and stages");
        self.report(&frame, LifecyclePhase::ImportStaged, check)
            .await;
        let sealed = session.seal().expect("fully paged session seals");
        self.report(&frame, LifecyclePhase::ImportSealed, check)
            .await;
        sealed
            .publish(&StateImportPublicationWitness::new())
            .expect("sealed import publishes");
        self.ghost.activations += activations;
        self.ghost.known_roots.push(GhostRoot {
            root: import.root,
            channel: import.channel,
            expected_data: encode_datums(&vec![import.datum]),
        });
        self.report(&frame, LifecyclePhase::Published, check).await;
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn refuse_import(
        &mut self,
        index: usize,
        step: LifecycleStep,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let import = lifecycle_import(self.base, index, ImportRecordForm::MalformedMetadata);
        let frame = self
            .begin_step(index, step, Some(import.root.clone()), check)
            .await;
        let mut session = self
            .importer
            .begin_import(
                StateImportSpec::new(
                    [import.root.clone()],
                    StateImportPublicationMode::Activate(import.root),
                )
                .expect("import spec for the malformed root is valid"),
            )
            .expect("malformed import session begins");
        assert!(
            matches!(
                session.validate_and_stage_page(import.page),
                Err(StateImportError::MalformedRecord { .. })
            ),
            "a malformed record must refuse the import page"
        );
        self.report(&frame, LifecyclePhase::Rejected, check).await;
        assert!(
            matches!(
                session.seal(),
                Err(StateImportError::IncompletePagination(_))
            ),
            "a refused import session must not seal"
        );
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn activate_known(
        &mut self,
        index: usize,
        step: LifecycleStep,
        selector: u8,
        check: &mut dyn FnMut(&LifecycleObservation),
    ) {
        let frame = self.begin_step(index, step, None, check).await;
        let selected = usize::from(selector) % self.ghost.known_roots.len();
        let known = self
            .authority
            .known_verified_root(&self.ghost.known_roots[selected].root)
            .expect("ghost-known root is a known verified root");
        self.authority
            .activate_known(known)
            .expect("known root activates");
        self.ghost.activations += 1;
        self.report(&frame, LifecyclePhase::StepEnd, check).await;
    }

    async fn close(mut self) {
        drop(self.importer);
        drop(self.reader_repository);
        drop(self.authority);
        self.manager
            .shutdown()
            .await
            .expect("lifecycle manager closes");
    }
}

async fn drive_lifecycle(
    base: i16,
    steps: &[LifecycleStep],
    check: &mut dyn FnMut(&LifecycleObservation),
) {
    let mut lifecycle = open_lifecycle(base, steps.len()).await;
    for (index, step) in steps.iter().copied().enumerate() {
        match step {
            LifecycleStep::Checkpoint => lifecycle.checkpoint(index, step, check).await,
            LifecycleStep::CheckpointUnchanged => {
                lifecycle.checkpoint_unchanged(index, step, check).await
            }
            LifecycleStep::RefusedCheckpoint => {
                lifecycle.refuse_checkpoint(index, step, check).await
            }
            LifecycleStep::ImportActivate => lifecycle.import(index, step, true, check).await,
            LifecycleStep::ImportPreserve => lifecycle.import(index, step, false, check).await,
            LifecycleStep::MalformedImport => lifecycle.refuse_import(index, step, check).await,
            LifecycleStep::ActivateKnown(selector) => {
                lifecycle.activate_known(index, step, selector, check).await
            }
        }
    }
    lifecycle.close().await;
}

fn checked_lifecycle_points<const ASSERTIONS: usize>(
    base: i16,
    steps: &[LifecycleStep],
    assertions: [fn(&LifecycleObservation) -> usize; ASSERTIONS],
) -> [usize; ASSERTIONS] {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime builds");
    let mut checked = [0; ASSERTIONS];
    runtime.block_on(drive_lifecycle(base, steps, &mut |observation| {
        for (points, assertion) in checked.iter_mut().zip(assertions) {
            *points += assertion(observation);
        }
    }));
    checked
}

fn lifecycle_steps(
    required: &'static [&'static [LifecycleStep]],
) -> impl Strategy<Value = Vec<LifecycleStep>> {
    let generated = prop_oneof![
        Just(LifecycleStep::Checkpoint),
        Just(LifecycleStep::CheckpointUnchanged),
        Just(LifecycleStep::RefusedCheckpoint),
        Just(LifecycleStep::ImportActivate),
        Just(LifecycleStep::ImportPreserve),
        Just(LifecycleStep::MalformedImport),
        any::<u8>().prop_map(LifecycleStep::ActivateKnown),
    ];
    let forced: Vec<_> = required
        .iter()
        .map(|alternatives| {
            (
                proptest::sample::select(*alternatives),
                any::<proptest::sample::Index>(),
            )
        })
        .collect();
    (proptest::collection::vec(generated, 0..=5), forced).prop_map(|(mut steps, forced)| {
        for (forced_step, position) in forced {
            steps.insert(position.index(steps.len() + 1), forced_step);
        }
        steps
    })
}

fn observation_point(observation: &LifecycleObservation) -> String {
    format!(
        "step {} {:?} at {:?}",
        observation.step_index, observation.step, observation.phase
    )
}

fn unpublished_import_has_no_root_authority(observation: &LifecycleObservation) -> usize {
    match observation.phase {
        LifecyclePhase::ImportStaged | LifecyclePhase::ImportSealed => {
            let candidate = observation
                .import_candidate
                .as_ref()
                .expect("an unpublished import observation carries its candidate root");
            assert!(
                !candidate.is_verified_member,
                "{}: unpublished import candidate {} is a verified root",
                observation_point(observation),
                candidate.root
            );
            assert_eq!(
                observation.current,
                observation.step_start_current,
                "{}: an unpublished import moved the current root or its generation",
                observation_point(observation)
            );
            assert!(
                observation.image[RSPACE_ROOTS_NAMESPACE]
                    == observation.step_start_image[RSPACE_ROOTS_NAMESPACE],
                "{}: an unpublished import changed rspace-roots from {} to {} entries",
                observation_point(observation),
                observation.step_start_image[RSPACE_ROOTS_NAMESPACE].len(),
                observation.image[RSPACE_ROOTS_NAMESPACE].len()
            );
            1
        }
        _ => 0,
    }
}

fn unpublished_checkpoint_preserves_every_namespace(observation: &LifecycleObservation) -> usize {
    match observation.phase {
        LifecyclePhase::CheckpointAccepted => {
            assert!(
                observation.image == observation.step_start_image,
                "{}: an unpublished checkpoint changed entries per namespace from {:?} to {:?}",
                observation_point(observation),
                observation.step_start_image.each_ref().map(BTreeMap::len),
                observation.image.each_ref().map(BTreeMap::len)
            );
            assert_eq!(
                observation.current,
                observation.step_start_current,
                "{}: an unpublished checkpoint moved the current root or its generation",
                observation_point(observation)
            );
            1
        }
        _ => 0,
    }
}

fn unchanged_checkpoint_preserves_every_namespace(observation: &LifecycleObservation) -> usize {
    match observation.phase {
        LifecyclePhase::CheckpointWithoutActionsReturned
        | LifecyclePhase::CheckpointWithoutEffectReturned => {
            for (namespace, name) in ["rspace-history", "rspace-cold", "rspace-roots"]
                .into_iter()
                .enumerate()
            {
                assert!(
                    observation.image[namespace] == observation.step_start_image[namespace],
                    "{}: an unchanged checkpoint changed {} from {} to {} entries",
                    observation_point(observation),
                    name,
                    observation.step_start_image[namespace].len(),
                    observation.image[namespace].len()
                );
            }
            1
        }
        _ => 0,
    }
}

fn every_known_root_is_closed_and_exactly_observable(observation: &LifecycleObservation) -> usize {
    for known in &observation.known_roots {
        assert!(
            known.is_verified_member,
            "{}: published root {} is not a verified root",
            observation_point(observation),
            known.root
        );
        assert!(
            known.reader_opens,
            "{}: no history reader opens for known root {}: {:?}",
            observation_point(observation),
            known.root,
            known.observed_data
        );
        assert!(
            known.observed_data.as_ref() == Ok(&known.expected_data),
            "{}: known root {} returns {:?} bytes of data, not the {} bytes published there",
            observation_point(observation),
            known.root,
            known.observed_data.as_ref().map(Vec::len),
            known.expected_data.len()
        );
    }
    observation.known_roots.len()
}

fn current_root_is_known(observation: &LifecycleObservation) -> usize {
    assert!(
        observation.current_is_verified_member,
        "{}: current root {} is not a verified root",
        observation_point(observation),
        observation.current.root()
    );
    assert!(
        observation.current_is_accepted_as_known,
        "{}: known_verified_root refuses current root {}",
        observation_point(observation),
        observation.current.root()
    );
    1
}

fn unchanged_checkpoint_keeps_the_current_and_known_roots(
    observation: &LifecycleObservation,
) -> usize {
    match observation.phase {
        LifecyclePhase::CheckpointWithoutActionsReturned
        | LifecyclePhase::CheckpointWithoutEffectReturned => {
            assert_eq!(
                observation.unchanged_checkpoint_root.as_ref(),
                Some(observation.current.root()),
                "{}: an unchanged checkpoint returned a root other than the current root",
                observation_point(observation)
            );
            assert_eq!(
                observation.current.root(),
                observation.step_start_current.root(),
                "{}: an unchanged checkpoint moved the current root",
                observation_point(observation)
            );
            assert!(
                observation.image[RSPACE_ROOTS_NAMESPACE]
                    == observation.step_start_image[RSPACE_ROOTS_NAMESPACE],
                "{}: an unchanged checkpoint changed rspace-roots, which records the known \
                 roots, from {} to {} entries",
                observation_point(observation),
                observation.step_start_image[RSPACE_ROOTS_NAMESPACE].len(),
                observation.image[RSPACE_ROOTS_NAMESPACE].len()
            );
            1
        }
        _ => 0,
    }
}

fn generation_advances_only_on_activation(observation: &LifecycleObservation) -> usize {
    assert_eq!(
        observation.current.generation(),
        observation.initial_generation + observation.activations,
        "{}: generation is not the initial generation {} plus {} activations",
        observation_point(observation),
        observation.initial_generation,
        observation.activations
    );
    1
}

fn unchanged_checkpoint_keeps_the_generation(observation: &LifecycleObservation) -> usize {
    match observation.phase {
        LifecyclePhase::CheckpointWithoutActionsReturned
        | LifecyclePhase::CheckpointWithoutEffectReturned => {
            assert_eq!(
                observation.current.generation(),
                observation.step_start_current.generation(),
                "{}: an unchanged checkpoint advanced the generation",
                observation_point(observation)
            );
            1
        }
        _ => 0,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn generated_lifecycle_keeps_unpublished_imports_without_root_authority(
        base in any::<i16>(),
        steps in lifecycle_steps(&[&[
            LifecycleStep::ImportActivate,
            LifecycleStep::ImportPreserve,
        ]]),
    ) {
        let checked =
            checked_lifecycle_points(base, &steps, [unpublished_import_has_no_root_authority]);
        prop_assert!(!checked.contains(&0), "an assertion never ran: {checked:?}");
    }

    #[test]
    fn generated_lifecycle_keeps_unpublished_checkpoints_out_of_every_namespace(
        base in any::<i16>(),
        steps in lifecycle_steps(&[
            &[LifecycleStep::Checkpoint],
            &[LifecycleStep::CheckpointUnchanged],
        ]),
    ) {
        let checked = checked_lifecycle_points(base, &steps, [
            unpublished_checkpoint_preserves_every_namespace,
            unchanged_checkpoint_preserves_every_namespace,
        ]);
        prop_assert!(!checked.contains(&0), "an assertion never ran: {checked:?}");
    }

    #[test]
    fn generated_lifecycle_keeps_every_known_root_closed_and_exactly_observable(
        base in any::<i16>(),
        steps in lifecycle_steps(&[&[
            LifecycleStep::Checkpoint,
            LifecycleStep::ImportActivate,
            LifecycleStep::ImportPreserve,
        ]]),
    ) {
        let checked = checked_lifecycle_points(base, &steps, [
            every_known_root_is_closed_and_exactly_observable,
        ]);
        prop_assert!(!checked.contains(&0), "an assertion never ran: {checked:?}");
    }

    #[test]
    fn generated_lifecycle_keeps_the_current_root_known(
        base in any::<i16>(),
        steps in lifecycle_steps(&[
            &[
                LifecycleStep::Checkpoint,
                LifecycleStep::ImportActivate,
                LifecycleStep::ActivateKnown(0),
            ],
            &[LifecycleStep::CheckpointUnchanged],
        ]),
    ) {
        let checked = checked_lifecycle_points(base, &steps, [
            current_root_is_known,
            unchanged_checkpoint_keeps_the_current_and_known_roots,
        ]);
        prop_assert!(!checked.contains(&0), "an assertion never ran: {checked:?}");
    }

    #[test]
    fn generated_lifecycle_advances_the_generation_only_on_activation(
        base in any::<i16>(),
        steps in lifecycle_steps(&[
            &[
                LifecycleStep::Checkpoint,
                LifecycleStep::ImportActivate,
                LifecycleStep::ImportPreserve,
                LifecycleStep::ActivateKnown(0),
            ],
            &[LifecycleStep::CheckpointUnchanged],
        ]),
    ) {
        let checked = checked_lifecycle_points(base, &steps, [
            generation_advances_only_on_activation,
            unchanged_checkpoint_keeps_the_generation,
        ]);
        prop_assert!(!checked.contains(&0), "an assertion never ran: {checked:?}");
    }
}
