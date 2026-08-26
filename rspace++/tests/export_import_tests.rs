use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::history::history_repository::HistoryRepositoryInstances;
use rspace_plus_plus::rspace::history::instances::radix_history::RadixHistory;
use rspace_plus_plus::rspace::hot_store::{HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::ISpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::import_session::{
    StateImportError, StateImportPage, StateImportPublicationMode, StateImportSession,
    StateImportSpec,
};
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporter;
use serde::{Deserialize, Serialize};
use shared::rust::{Byte, ByteVector};

// See rspace/src/main/scala/coop/rchain/rspace/examples/StringExamples.scala
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}
// Default-body event-hash bytes for the test-local type.
impl rspace_plus_plus::rspace::hashing::stable_hash_provider::StableHashSerialize for Pattern {}

#[derive(Clone)]
struct StringMatch;

impl Match<Pattern, String, String> for StringMatch {
    // Borrowed inputs: clone only the matched (returned) value.
    fn get(&self, p: &Pattern, a: &String) -> Option<String> {
        match p {
            Pattern::Wildcard => Some(a.clone()),
            Pattern::StringMatch(value) => {
                if value == a {
                    Some(a.clone())
                } else {
                    None
                }
            }
        }
    }
}

type StateItem = (Blake2b256Hash, ByteVector);
type StatePath = Vec<(Blake2b256Hash, Option<Byte>)>;

#[derive(Debug, Default)]
struct ImportTrace {
    pages: usize,
    content_pages: usize,
    nonterminal_pages: usize,
    empty_terminal_pages: usize,
    nonempty_terminal_pages: usize,
    history_items: Vec<StateItem>,
    data_items: Vec<StateItem>,
}

fn activate_spec(root: &Blake2b256Hash) -> StateImportSpec {
    StateImportSpec::new([root.clone()], StateImportPublicationMode::Activate(root.clone()))
        .expect("one well-formed activation root is a valid import authority")
}

fn stage_export_page(
    exporter: Arc<dyn RSpaceExporter>,
    session: &mut dyn StateImportSession,
    start_path: StatePath,
    skip: i32,
    take: i32,
) -> (
    rspace_plus_plus::rspace::state::import_session::StateImportPageReceipt,
    Vec<StateItem>,
    Vec<StateItem>,
) {
    let exported =
        RSpaceExporterItems::get_history_and_data(exporter, start_path.clone(), skip, take)
            .expect("export authenticated state-import page");
    assert_eq!(
        exported.0.last_path, exported.1.last_path,
        "history and cold-leaf projections must advertise one cursor"
    );
    let advertised_next_path = exported.0.last_path;
    let history_items = exported.0.items;
    let data_items = exported.1.items;
    let receipt = session
        .validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path: advertised_next_path.clone(),
            skip,
            take,
            history_items: history_items.clone(),
            data_items: data_items.clone(),
        })
        .expect("validate and stage authenticated import page");
    assert_eq!(receipt.next_path, advertised_next_path);
    assert!(
        !receipt.next_path.is_empty(),
        "the affine API never exposes an empty terminal sentinel"
    );
    assert_eq!(
        receipt.terminal,
        receipt.next_path == start_path,
        "terminal status is derived from the authenticated cursor"
    );
    assert_eq!(receipt.history_objects_staged, history_items.len());
    assert_eq!(receipt.cold_leaves_staged, data_items.len());
    (receipt, history_items, data_items)
}

fn import_checkpoint_by_cursor(
    exporter: Arc<dyn RSpaceExporter>,
    importer: Arc<dyn RSpaceImporter>,
    root: &Blake2b256Hash,
    page_size: i32,
) -> ImportTrace {
    let mut session = importer
        .begin_import(activate_spec(root))
        .expect("begin affine state import");
    let mut start_path = vec![(root.clone(), None)];
    let mut seen = BTreeSet::new();
    let mut trace = ImportTrace::default();

    loop {
        assert!(seen.insert(start_path.clone()), "authenticated cursor traversal must not cycle");
        let (receipt, history_items, data_items) = stage_export_page(
            Arc::clone(&exporter),
            session.as_mut(),
            start_path.clone(),
            0,
            page_size,
        );
        let empty = history_items.is_empty() && data_items.is_empty();
        trace.pages += 1;
        trace.content_pages += usize::from(!empty);
        trace.history_items.extend(history_items);
        trace.data_items.extend(data_items);

        if receipt.terminal {
            trace.empty_terminal_pages += usize::from(empty);
            trace.nonempty_terminal_pages += usize::from(!empty);
            break;
        }

        assert!(!empty, "a nonterminal authenticated page must carry trie objects");
        assert_eq!(
            receipt.next_path.len(),
            6,
            "a nonterminal cursor is root plus five prefix components"
        );
        assert_eq!(receipt.next_path[0].0, *root);
        assert_ne!(receipt.next_path, start_path);
        trace.nonterminal_pages += 1;
        start_path = receipt.next_path;
    }

    assert_eq!(
        trace.empty_terminal_pages + trace.nonempty_terminal_pages,
        1,
        "every finite traversal has exactly one authenticated terminal page"
    );
    let publication = session
        .seal()
        .expect("seal complete state closure")
        .publish()
        .expect("atomically publish and activate verified checkpoint");
    assert_eq!(publication.verified_roots, vec![root.clone()]);
    let activation = publication
        .activation
        .expect("Activate mode returns its exact activation receipt");
    assert_eq!(&activation.root, root);
    trace
}

fn unique_projection(items: Vec<StateItem>) -> BTreeMap<Blake2b256Hash, ByteVector> {
    let mut unique = BTreeMap::new();
    for (hash, bytes) in items {
        if let Some(previous) = unique.insert(hash.clone(), bytes.clone()) {
            assert_eq!(previous, bytes, "one content address must never project conflicting bytes");
        }
    }
    unique
}

fn export_checkpoint_by_skip(
    exporter: Arc<dyn RSpaceExporter>,
    root: &Blake2b256Hash,
    page_size: i32,
) -> (Vec<StateItem>, Vec<StateItem>, usize) {
    let start_path = vec![(root.clone(), None)];
    let mut skip = 0_i32;
    let mut pages = 0_usize;
    let mut history_items = Vec::new();
    let mut data_items = Vec::new();

    loop {
        let exported = RSpaceExporterItems::get_history_and_data(
            Arc::clone(&exporter),
            start_path.clone(),
            skip,
            page_size,
        )
        .expect("export skip-addressed page");
        assert_eq!(exported.0.last_path, exported.1.last_path);
        assert!(
            !exported.0.last_path.is_empty(),
            "export pages never expose an empty terminal sentinel"
        );
        let terminal = exported.0.last_path == start_path;
        history_items.extend(exported.0.items);
        data_items.extend(exported.1.items);
        pages += 1;
        if terminal {
            break;
        }
        skip = skip
            .checked_add(page_size)
            .expect("test pagination remains within i32");
    }

    (history_items, data_items, pages)
}

// See rspace/src/test/scala/coop/rchain/rspace/ExportImportTests.scala
#[tokio::test]
async fn export_and_import_of_one_page_should_works_correctly() {
    let (space1, exporter1, _, space2, _, importer2) = test_setup().await;

    let page_size = 1000;
    let data_size = 10;
    let pattern = vec![Pattern::Wildcard];
    let continuation = "continuation".to_string();

    // Generate init data in space1
    for i in 0..data_size {
        let res = space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await;
        assert!(res.is_ok());
    }

    let init_point = space1.create_checkpoint().await.unwrap();

    let trace = import_checkpoint_by_cursor(exporter1, importer2, &init_point.root, page_size);
    assert_eq!(trace.content_pages, 1);
    assert_eq!(trace.nonterminal_pages, 0);
    assert_eq!(trace.nonempty_terminal_pages, 1);
    space2
        .reset(&init_point.root)
        .await
        .expect("activated imported checkpoint is resettable");

    // space2.store.print();

    // Testing data in space2 (match all installed channels)
    for i in 0..data_size {
        let res = space2
            .consume(
                vec![format!("ch{}", i)],
                pattern.clone(),
                continuation.clone(),
                false,
                BTreeSet::new(),
            )
            .await;
        assert!(res.is_ok());
    }

    // println!("\nspace2: {:?}", space2.to_map());

    let end_point = space2.create_checkpoint().await.unwrap();
    assert_eq!(end_point.root, RadixHistory::empty_root_node_hash())
}

#[tokio::test]
async fn empty_checkpoint_import_is_one_zero_object_terminal_page() {
    let (space1, exporter1, _, space2, _, importer2) = test_setup().await;
    let empty = space1.create_checkpoint().await.expect("empty checkpoint");

    let trace = import_checkpoint_by_cursor(exporter1, importer2, &empty.root, 64);
    assert_eq!(trace.pages, 1);
    assert_eq!(trace.content_pages, 0);
    assert_eq!(trace.empty_terminal_pages, 1);
    assert_eq!(trace.nonempty_terminal_pages, 0);
    space2
        .reset(&empty.root)
        .await
        .expect("verified empty checkpoint is resettable");
}

#[tokio::test]
async fn invalid_pages_leave_no_root_authority_and_the_same_session_can_retry() {
    let (space1, exporter1, _, space2, _, importer2) = test_setup().await;
    space1
        .produce("channel".to_string(), "datum".to_string(), false)
        .await
        .expect("produce source datum");
    let checkpoint = space1.create_checkpoint().await.expect("source checkpoint");
    let start_path = vec![(checkpoint.root.clone(), None)];
    let exported =
        RSpaceExporterItems::get_history_and_data(exporter1, start_path.clone(), 0, 1_000)
            .expect("export complete source checkpoint");
    assert_eq!(exported.0.last_path, exported.1.last_path);
    assert!(
        !exported.0.items.is_empty(),
        "a nonempty checkpoint has authenticated history objects"
    );
    let advertised_next_path = exported.0.last_path;
    assert_eq!(
        advertised_next_path, start_path,
        "a complete nonempty page uses the same normalized terminal cursor as an empty page"
    );
    let history_items = exported.0.items;
    let data_items = exported.1.items;
    let mut session = importer2
        .begin_import(activate_spec(&checkpoint.root))
        .expect("begin retryable affine import");

    let mut corrupt_history = history_items.clone();
    corrupt_history[0].1[0] ^= 1;
    let error = session
        .validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path: advertised_next_path.clone(),
            skip: 0,
            take: 1_000,
            history_items: corrupt_history,
            data_items: data_items.clone(),
        })
        .expect_err("digest-corrupt history must be rejected");
    assert!(matches!(error, StateImportError::HistoryDigestMismatch { .. }));

    let mut duplicate_history = history_items.clone();
    duplicate_history.push(history_items[0].clone());
    let error = session
        .validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path: advertised_next_path.clone(),
            skip: 0,
            take: 1_000,
            history_items: duplicate_history,
            data_items: data_items.clone(),
        })
        .expect_err("duplicate content addresses must be rejected before staging");
    assert!(matches!(error, StateImportError::DuplicateHistoryObject(_)));

    let error = session
        .validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path: Vec::new(),
            skip: 0,
            take: 1_000,
            history_items: history_items.clone(),
            data_items: data_items.clone(),
        })
        .expect_err("peer cursor must agree with authenticated traversal");
    assert!(matches!(error, StateImportError::AdvertisedCursorMismatch { .. }));

    for (hash, _) in &history_items {
        assert_eq!(
            importer2
                .get_history_item(hash.clone())
                .expect("read target staging store after rejected page"),
            None,
            "page validation is atomic: no history object is staged on failure"
        );
    }
    assert!(
        space2.reset(&checkpoint.root).await.is_err(),
        "a rejected page cannot publish or activate its claimed root"
    );

    let receipt = session
        .validate_and_stage_page(StateImportPage {
            start_path: start_path.clone(),
            advertised_next_path,
            skip: 0,
            take: 1_000,
            history_items,
            data_items,
        })
        .expect("the same affine capability accepts a corrected page");
    assert!(receipt.terminal);
    let publication = session
        .seal()
        .expect("corrected terminal page seals the complete closure")
        .publish()
        .expect("corrected closure publishes atomically");
    assert_eq!(
        publication
            .activation
            .expect("Activate mode returns its exact activation receipt")
            .root,
        checkpoint.root
    );
    space2
        .reset(&checkpoint.root)
        .await
        .expect("only the corrected closure makes the checkpoint resettable");
}

#[tokio::test]
async fn imported_state_can_be_reexported_and_mixed_with_a_new_checkpoint() {
    let (space_a, exporter_a, _, space_b, exporter_b, importer_b) = test_setup().await;
    let (_, _, _, space_c, _, importer_c) = test_setup().await;
    let page_size = 1_000;
    let source_channel = "source-channel".to_string();
    let added_channel = "added-channel".to_string();

    space_a
        .produce(source_channel.clone(), "source-data".to_string(), false)
        .await
        .expect("produce source datum");
    let source_checkpoint = space_a.create_checkpoint().await.expect("checkpoint A");
    let source_trace = import_checkpoint_by_cursor(
        exporter_a,
        Arc::clone(&importer_b),
        &source_checkpoint.root,
        page_size,
    );
    space_b
        .reset(&source_checkpoint.root)
        .await
        .expect("reset B to imported A root");

    let reexport_path = vec![(source_checkpoint.root.clone(), None)];
    let reexport = RSpaceExporterItems::get_history_and_data(
        exporter_b.clone(),
        reexport_path.clone(),
        0,
        page_size,
    )
    .expect("re-export imported checkpoint");
    assert_eq!(unique_projection(reexport.0.items), unique_projection(source_trace.history_items));
    assert_eq!(unique_projection(reexport.1.items), unique_projection(source_trace.data_items));

    space_b
        .produce(added_channel.clone(), "added-data".to_string(), false)
        .await
        .expect("produce a datum after import");
    let mixed_checkpoint = space_b.create_checkpoint().await.expect("checkpoint B");
    import_checkpoint_by_cursor(exporter_b, importer_c, &mixed_checkpoint.root, page_size);
    space_c
        .reset(&mixed_checkpoint.root)
        .await
        .expect("reset C to B's mixed imported/local root");

    for channel in [source_channel, added_channel] {
        space_c
            .consume(
                vec![channel],
                vec![Pattern::Wildcard],
                "continuation".to_string(),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("C can read every imported and locally added datum");
    }
}

#[tokio::test]
async fn multipage_export_should_work_correctly() {
    let (space1, exporter1, _, space2, _, importer2) = test_setup().await;

    let page_size = 10;
    let data_size = 1000;
    let pattern = vec![Pattern::Wildcard];
    let continuation = "continuation".to_string();

    // Generate init data in space1
    for i in 0..data_size {
        let res = space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await;
        assert!(res.is_ok());
    }

    let init_point = space1.create_checkpoint().await.unwrap();

    let trace = import_checkpoint_by_cursor(exporter1, importer2, &init_point.root, page_size);
    assert!(trace.pages > 1);
    assert!(trace.nonterminal_pages > 0);
    assert!(trace.content_pages > 1);
    space2
        .reset(&init_point.root)
        .await
        .expect("activated multipage checkpoint is resettable");

    // Testing data in space2 (match all installed channels)
    for i in 0..data_size {
        let res = space2
            .consume(
                vec![format!("ch{}", i)],
                pattern.clone(),
                continuation.clone(),
                false,
                BTreeSet::new(),
            )
            .await;
        assert!(res.is_ok());
    }
    let end_point = space2.create_checkpoint().await.unwrap();
    assert_eq!(end_point.root, RadixHistory::empty_root_node_hash())
}

// Attention! Skipped export is significantly slower than last path export.
// But on the other hand, this allows you to work simultaneously with several
// nodes.
#[tokio::test]
async fn multipage_export_with_skip_should_work_correctly() {
    let (space1, exporter1, _, space2, _, importer2) = test_setup().await;

    let page_size = 10;
    let data_size = 1000;
    let pattern = vec![Pattern::Wildcard];
    let continuation = "continuation".to_string();

    // Generate init data in space1
    for i in 0..data_size {
        let res = space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await;
        assert!(res.is_ok());
    }

    let init_point = space1.create_checkpoint().await.unwrap();

    let (skip_history, skip_data, skip_pages) =
        export_checkpoint_by_skip(Arc::clone(&exporter1), &init_point.root, page_size);
    assert!(skip_pages > 1);

    // Skip-addressed traversal remains an exporter capability. Import authority
    // follows authenticated continuation cursors so no caller-provided offset
    // can omit or repeat part of the state closure.
    let cursor_trace =
        import_checkpoint_by_cursor(exporter1, importer2, &init_point.root, page_size);
    assert_eq!(unique_projection(skip_history), unique_projection(cursor_trace.history_items));
    assert_eq!(unique_projection(skip_data), unique_projection(cursor_trace.data_items));
    space2
        .reset(&init_point.root)
        .await
        .expect("activated cursor-imported checkpoint is resettable");

    // Testing data in space2 (match all installed channels)
    for i in 0..data_size {
        let res = space2
            .consume(
                vec![format!("ch{}", i)],
                pattern.clone(),
                continuation.clone(),
                false,
                BTreeSet::new(),
            )
            .await;
        assert!(res.is_ok());
    }
    let end_point = space2.create_checkpoint().await.unwrap();
    assert_eq!(end_point.root, RadixHistory::empty_root_node_hash())
}

async fn test_setup() -> (
    RSpace<String, Pattern, String, String>,
    Arc<dyn RSpaceExporter>,
    Arc<dyn RSpaceImporter>,
    RSpace<String, Pattern, String, String>,
    Arc<dyn RSpaceExporter>,
    Arc<dyn RSpaceImporter>,
) {
    let mut kvm = InMemoryStoreManager::new();

    let history_repository1 = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::repository(
            kvm.state_authority("export-import-1").await.unwrap(),
        )
        .unwrap(),
    );

    let cache1: HotStoreState<String, Pattern, String, String> = HotStoreState::default();
    let history_reader = history_repository1
        .get_history_reader(&history_repository1.root())
        .unwrap();

    let store1 = {
        let hr = history_reader.base();
        HotStoreInstances::create_from_hs_and_hr(cache1, hr)
    };

    let exporter1 = history_repository1.exporter();
    let importer1 = history_repository1.importer();

    let space1 = RSpace::apply(history_repository1, store1, Arc::new(Box::new(StringMatch)));

    let history_repository2 = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::repository(
            kvm.state_authority("export-import-2").await.unwrap(),
        )
        .unwrap(),
    );

    let cache2: HotStoreState<String, Pattern, String, String> = HotStoreState::default();
    let history_reader = history_repository2
        .get_history_reader(&history_repository2.root())
        .unwrap();

    let store2 = {
        let hr = history_reader.base();
        HotStoreInstances::create_from_hs_and_hr(cache2, hr)
    };

    let exporter2 = history_repository2.exporter();
    let importer2 = history_repository2.importer();

    let space2 = RSpace::apply(history_repository2, store2, Arc::new(Box::new(StringMatch)));

    (space1, exporter1, importer1, space2, exporter2, importer2)
}

// ───────────────────────────────────────────────────────────────────────────
// The cold-store DECODE boundary for this file's test doubles.
//
// `ColdStoreDecode` deliberately has NO blanket impl over `DeserializeOwned`:
// Rust has no specialization, so a blanket would make `Par` un-overridable and
// the recursive path would silently stay in production while every call site
// LOOKED converted. The cost of that decision is exactly these one-line
// delegations. `legacy_prefix` is sound here because these doubles are
// bounded-depth by definition — none of them contains itself or a `Par`.
// ───────────────────────────────────────────────────────────────────────────

impl rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecode for Pattern {
    fn cold_decode_prefix(
        bytes: &[u8],
    ) -> Result<
        (Self, usize),
        rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecodeError,
    > {
        rspace_plus_plus::rspace::serializers::cold_store_decode::legacy_prefix(bytes)
    }
}
