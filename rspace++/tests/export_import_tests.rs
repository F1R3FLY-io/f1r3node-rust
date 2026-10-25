use std::collections::BTreeSet;
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
use rspace_plus_plus::rspace::state::rspace_exporter::RSpaceExporter;
use rspace_plus_plus::rspace::state::rspace_importer::{RSpaceImporter, RSpaceImporterInstance};
use serde::{Deserialize, Serialize};
use shared::rust::{Byte, ByteVector};

// See rspace/src/main/scala/coop/rchain/rspace/examples/StringExamples.scala
#[derive(Clone, Debug, Serialize, Deserialize, Default, PartialEq)]
enum Pattern {
    #[default]
    Wildcard,
    StringMatch(String),
}

#[derive(Clone)]
struct StringMatch;

impl Match<Pattern, String, String> for StringMatch {
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

// See rspace/src/test/scala/coop/rchain/rspace/ExportImportTests.scala
// TODO: Don't works for MergingHistory
#[tokio::test]
async fn export_and_import_of_one_page_should_works_correctly() {
    let (space1, exporter1, importer1, space2, _, importer2) = test_setup().await;

    let page_size = 1000;
    let data_size = 10;
    let start_skip = 0;
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

    // Export 1 page from space1
    let init_start_path: Vec<(Blake2b256Hash, Option<Byte>)> =
        vec![(init_point.root.clone(), None)];
    let export_data = RSpaceExporterItems::get_history_and_data(
        exporter1,
        init_start_path.clone(),
        start_skip,
        page_size,
    )
    .unwrap();
    let history_items = export_data.0.items;
    let data_items = export_data.1.items;

    // println!("\ndata_items_page: {:?}", data_items);

    // Validate exporting page
    RSpaceImporterInstance::validate_state_items(
        &history_items,
        &data_items,
        init_start_path,
        page_size,
        start_skip,
        importer1,
    )
    .unwrap();

    // Import page to space2
    importer2.set_history_items(history_items);
    importer2.set_data_items(data_items);
    importer2.set_root(&init_point.root);
    let _ = space2.reset(&init_point.root).await;

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
async fn multipage_export_should_work_correctly() {
    let (space1, exporter1, importer1, space2, _, importer2) = test_setup().await;

    let page_size = 10;
    let data_size = 1000;
    let start_skip = 0;
    let pattern = vec![Pattern::Wildcard];
    let continuation = "continuation".to_string();

    type Params = (
        Vec<(Blake2b256Hash, ByteVector)>,   // HistoryItems
        Vec<(Blake2b256Hash, ByteVector)>,   // DataItems
        Vec<(Blake2b256Hash, Option<Byte>)>, // StartPath
    );

    let multipage_export = |params: Params,
                            exporter1: Arc<dyn RSpaceExporter>,
                            importer1: Arc<dyn RSpaceImporter>|
     -> Result<Params, Params> {
        let (history_items, data_items, start_path) = params;
        // Export 1 page from space1
        let export_data = RSpaceExporterItems::get_history_and_data(
            exporter1,
            start_path.clone(),
            start_skip,
            page_size,
        )
        .unwrap();

        let history_items_page = export_data.0.items;
        let data_items_page = export_data.1.items;
        let last_path = export_data.0.last_path;

        // Validate exporting page
        RSpaceImporterInstance::validate_state_items(
            &history_items_page,
            &data_items_page,
            start_path,
            page_size,
            start_skip,
            importer1,
        )
        .unwrap();

        let r = (
            [history_items, history_items_page.clone()].concat(),
            [data_items, data_items_page].concat(),
            last_path,
        );

        if (history_items_page.len() as i32) < page_size {
            Err(r)
        } else {
            Ok(r)
        }
    };

    let init_history_items: Vec<(Blake2b256Hash, ByteVector)> = Vec::new();
    let init_data_items: Vec<(Blake2b256Hash, ByteVector)> = Vec::new();
    let init_child_num: Option<Byte> = None;

    // Generate init data in space1
    for i in 0..data_size {
        let res = space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await;
        assert!(res.is_ok());
    }

    let init_point = space1.create_checkpoint().await.unwrap();

    // Multipage export from space1
    let init_start_path: Vec<(Blake2b256Hash, Option<Byte>)> =
        vec![(init_point.root.clone(), init_child_num)];
    let init_export_data = (init_history_items, init_data_items, init_start_path);
    let mut export_data = init_export_data;
    loop {
        match multipage_export(export_data, exporter1.clone(), importer1.clone()) {
            Ok(data) => {
                export_data = data;
            }
            Err(final_data) => {
                export_data = final_data;
                break;
            }
        }
    }

    let history_items = export_data.0;
    let data_items = export_data.1;

    // Import page to space2
    importer2.set_history_items(history_items);
    importer2.set_data_items(data_items);
    importer2.set_root(&init_point.root);
    let _ = space2.reset(&init_point.root).await;

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
    let (space1, exporter1, importer1, space2, _, importer2) = test_setup().await;

    let page_size = 10;
    let data_size = 1000;
    let start_skip = 0;
    let pattern = vec![Pattern::Wildcard];
    let continuation = "continuation".to_string();

    type Params = (
        Vec<(Blake2b256Hash, ByteVector)>,   // HistoryItems
        Vec<(Blake2b256Hash, ByteVector)>,   // DataItems
        Vec<(Blake2b256Hash, Option<Byte>)>, // StartPath
        i32,                                 // Size of skip
    );

    let multipage_export_with_skip = |params: Params,
                                      exporter1: Arc<dyn RSpaceExporter>,
                                      importer1: Arc<dyn RSpaceImporter>|
     -> Result<Params, Params> {
        let (history_items, data_items, start_path, skip) = params;
        // Export 1 page from space1
        let export_data = RSpaceExporterItems::get_history_and_data(
            exporter1,
            start_path.clone(),
            skip,
            page_size,
        )
        .unwrap();

        let history_items_page = export_data.0.items;
        let data_items_page = export_data.1.items;

        // Validate exporting page
        RSpaceImporterInstance::validate_state_items(
            &history_items_page,
            &data_items_page,
            start_path.clone(),
            page_size,
            skip,
            importer1,
        )
        .unwrap();

        let r = (
            [history_items, history_items_page.clone()].concat(),
            [data_items, data_items_page].concat(),
            start_path,
            skip + page_size,
        );

        if (history_items_page.len() as i32) < page_size {
            Err(r)
        } else {
            Ok(r)
        }
    };

    let init_history_items: Vec<(Blake2b256Hash, ByteVector)> = Vec::new();
    let init_data_items: Vec<(Blake2b256Hash, ByteVector)> = Vec::new();
    let init_child_num: Option<Byte> = None;

    // Generate init data in space1
    for i in 0..data_size {
        let res = space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await;
        assert!(res.is_ok());
    }

    let init_point = space1.create_checkpoint().await.unwrap();

    // Multipage export with skip from space1
    let init_start_path: Vec<(Blake2b256Hash, Option<Byte>)> =
        vec![(init_point.root.clone(), init_child_num)];
    let init_export_data = (init_history_items, init_data_items, init_start_path, start_skip);
    let mut export_data = init_export_data;
    loop {
        match multipage_export_with_skip(export_data, exporter1.clone(), importer1.clone()) {
            Ok(data) => {
                export_data = data;
            }
            Err(final_data) => {
                export_data = final_data;
                break;
            }
        }
    }

    let history_items = export_data.0;
    let data_items = export_data.1;

    // Import page to space2
    importer2.set_history_items(history_items);
    importer2.set_data_items(data_items);
    importer2.set_root(&init_point.root);
    let _ = space2.reset(&init_point.root).await;

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

#[tokio::test]
async fn export_with_an_unknown_or_malformed_start_path_returns_an_error() {
    let (space1, exporter1, _, _, _, _) = test_setup().await;
    for i in 0..10 {
        space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await
            .unwrap();
    }
    let root = space1.create_checkpoint().await.unwrap().root;
    let filled = |b: u8| Blake2b256Hash::from_bytes(vec![b; 32]);

    let honest = RSpaceExporterItems::get_history_and_data(
        exporter1.clone(),
        vec![(root.clone(), None)],
        0,
        750,
    )
    .expect("an honest root request must export");
    assert!(!honest.1.items.is_empty());

    let mut prefix_size = vec![32u8];
    prefix_size.extend(vec![0u8; 31]);
    let malformed = [
        vec![],
        vec![(filled(0x42), None)],
        vec![(root.clone(), None), (filled(1), None), (filled(2), None)],
        vec![
            (root.clone(), None),
            (Blake2b256Hash::from_bytes(prefix_size), None),
            (filled(0xFF), None),
            (filled(0), None),
            (filled(0), None),
            (filled(0), None),
        ],
        vec![(Blake2b256Hash::from_bytes(vec![1, 2, 3]), None)],
        vec![
            (root.clone(), None),
            (Blake2b256Hash::from_bytes(vec![]), None),
            (filled(0), None),
            (filled(0), None),
            (filled(0), None),
            (filled(0), None),
        ],
    ];
    for start_path in malformed {
        let len = start_path.len();
        let result =
            RSpaceExporterItems::get_history_and_data(exporter1.clone(), start_path, 0, 750);
        assert!(
            result.is_err(),
            "a {len}-element bad start path must be an error, not a panic or an export"
        );
    }
}

// Issue #787: a node that restored its state through state sync must serve
// that state whole. The importer writes data items under the raw hash, and
// a checkpoint writes them under the bincode-serialized hash.
#[tokio::test]
async fn imported_state_exports_its_data_items() {
    let (space1, exporter1, _, _, exporter2, importer2) = test_setup().await;

    for i in 0..10 {
        space1
            .produce(format!("ch{}", i), format!("data{}", i), false)
            .await
            .unwrap();
    }
    let root = space1.create_checkpoint().await.unwrap().root;
    let start_path: Vec<(Blake2b256Hash, Option<Byte>)> = vec![(root.clone(), None)];

    let (history, data) =
        RSpaceExporterItems::get_history_and_data(exporter1, start_path.clone(), 0, 1000).unwrap();
    assert!(!data.items.is_empty(), "the source state has data items");
    importer2.set_history_items(history.items.clone());
    importer2.set_data_items(data.items.clone());
    importer2.set_root(&root);

    let (reexported_history, reexported_data) =
        RSpaceExporterItems::get_history_and_data(exporter2, start_path, 0, 1000).unwrap();
    let sorted = |mut items: Vec<(Blake2b256Hash, ByteVector)>| {
        items.sort_by_key(|item| item.0.bytes());
        items
    };
    assert_eq!(sorted(reexported_history.items), sorted(history.items));
    assert_eq!(
        sorted(reexported_data.items),
        sorted(data.items),
        "the imported state exports without the data items it was imported with"
    );
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

    let roots1 = kvm.store("roots1".to_string()).await.unwrap();
    let cold1 = kvm.store("cold1".to_string()).await.unwrap();
    let history1 = kvm.store("history1".to_string()).await.unwrap();

    let history_repository1 = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::lmdb_repository(
            roots1, cold1, history1,
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

    let roots2 = kvm.store("roots2".to_string()).await.unwrap();
    let cold2 = kvm.store("cold2".to_string()).await.unwrap();
    let history2 = kvm.store("history2".to_string()).await.unwrap();

    let history_repository2 = Arc::new(
        HistoryRepositoryInstances::<String, Pattern, String, String>::lmdb_repository(
            roots2, cold2, history2,
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
