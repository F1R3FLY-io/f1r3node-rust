//! P9 step 4: the LFS export and import of native space state (P9 design v2, section 9,
//! amendment A19).
//!
//! A node that joins a network loads the state of the last finalized block (LFS) from a peer.
//! The peer exports the history and the data of one root in pages, and the new node validates
//! each page and imports the pages into its own store. Spaces are consensus state in the one
//! RSpace, so the exporter and the importer of the history repository carry them with no Reified
//! code (`history_repository.rs`, `exporter` and `importer`). The campaign changes neither.
//!
//! | Test | Property |
//! |---|---|
//! | `space_lfs_export_import` | The source plays block 1 of `support::blocks` and the creations of the five stored families of P7 over one LMDB store. Its root R1 holds a durable space of each of the 26 families, the stored spaces with records and waiters, VectorDB records and an agent space. The test exports R1 in pages, validates each page, and imports the pages into a fresh LMDB store. Every space channel of block 1 holds the same records and waiters on both stores. Block 2 and the touches of the stored families then give the same record for each deploy on both stores, and native replay certifies each deploy of block 2 on the imported store. A page with one changed data byte fails the validation. |
//!
//! `temp`, `seq` and HashSet spaces never reach a root: each deploy sweeps them (QF-8), and the
//! sweep leaves no volatile channel after any deploy of the test (`play_recorded`).

#[path = "support/mod.rs"]
mod support;

use std::collections::BTreeSet;

use models::rhoapi::Par;
use rholang::rust::interpreter::rho_runtime::RhoHistoryRepository;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;
use rspace_plus_plus::rspace::state::rspace_importer::RSpaceImporterInstance;
use support::blocks::{blocks, Deploy};
use support::family_programs::STORED_FAMILIES;
use support::native::{
    committed_evidence, footprint_channels, is_space_channel, NativeChain, AMPLE_LIMIT,
};
use support::process::{
    assert_same_record, certify_record, play_recorded, run_directory, DeployRecord,
};

/// The number of history items in one page of the export. The root of block 1 needs more than
/// one page, so the test pages the export as a peer does (`export_import_tests.rs`).
const PAGE_SIZE: i32 = 64;

/// The items of an export: the history items, the data items and the number of pages.
struct Export {
    history: Vec<(Blake2b256Hash, Vec<u8>)>,
    data: Vec<(Blake2b256Hash, Vec<u8>)>,
    pages: usize,
}

/// Exports the root `root` of `history` in pages of [`PAGE_SIZE`] history items, and validates
/// each page with the importer of `history`, as a node validates the pages of a peer.
fn export_pages(history: &RhoHistoryRepository, root: &Blake2b256Hash) -> Export {
    let exporter = history.exporter();
    let importer = history.importer();
    let mut export = Export {
        history: Vec::new(),
        data: Vec::new(),
        pages: 0,
    };
    let mut start_path = vec![(root.clone(), None)];
    loop {
        let (history_page, data_page) = RSpaceExporterItems::get_history_and_data(
            exporter.clone(),
            start_path.clone(),
            0,
            PAGE_SIZE,
        );
        RSpaceImporterInstance::validate_state_items(
            history_page.items.clone(),
            data_page.items.clone(),
            start_path,
            PAGE_SIZE,
            0,
            importer.clone(),
        )
        .unwrap_or_else(|error| panic!("page {} validates: {error}", export.pages));
        export.pages += 1;
        let last = (history_page.items.len() as i32) < PAGE_SIZE;
        export.history.extend(history_page.items);
        export.data.extend(data_page.items);
        if last {
            return export;
        }
        start_path = history_page.last_path;
    }
}

/// The space channels that the operations of a play touched, from its committed journal.
fn space_channels(played: &support::native::Played) -> BTreeSet<Vec<u8>> {
    let (_, journal) = committed_evidence(played);
    journal
        .iter()
        .flat_map(footprint_channels)
        .filter(is_space_channel)
        .map(|channel| bincode::serialize(&channel).expect("encode a channel"))
        .collect()
}

/// Block 1 of the test: block 1 of `support::blocks` and the creations of the five stored
/// families.
fn first_block() -> Vec<Deploy> {
    let [first, _, _] = blocks();
    let mut deploys = first;
    deploys.extend(STORED_FAMILIES.iter().map(|family| {
        Deploy::new(
            format!("lfs stored {}", family.label),
            family.creation(),
            AMPLE_LIMIT,
        )
    }));
    deploys
}

/// Block 2 of the test: block 2 of `support::blocks` and the touches of the five stored families.
fn second_block() -> Vec<Deploy> {
    let [_, second, _] = blocks();
    let mut deploys = second;
    deploys.extend(STORED_FAMILIES.iter().map(|family| {
        Deploy::new(
            format!("lfs stored touch {}", family.label),
            family.touching(),
            AMPLE_LIMIT,
        )
    }));
    deploys
}

/// Plays `deploys` in order on `chain`, and returns the record of each deploy and the space
/// channels that the deploys touched.
async fn play_block(
    chain: &mut NativeChain,
    deploys: &[Deploy],
) -> (Vec<DeployRecord>, BTreeSet<Vec<u8>>) {
    let mut records = Vec::with_capacity(deploys.len());
    let mut channels = BTreeSet::new();
    for deploy in deploys {
        let (played, record) =
            play_recorded(chain, &deploy.term, &deploy.label, deploy.limit).await;
        channels.extend(space_channels(&played));
        records.push(record);
    }
    (records, channels)
}

/// The LFS export and import of space state (amendment A19).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn space_lfs_export_import() {
    let directory = run_directory("lfs");
    let mut source = NativeChain::open(&directory.join("source"), None).await;
    let (first, channels) = play_block(&mut source, &first_block()).await;
    let r1 = first.last().expect("block 1 has deploys").post();

    let export = export_pages(&source.history(), &r1);
    assert!(
        export.pages > 1,
        "the export of R1 needs more than one page"
    );
    let bytes: usize = export
        .history
        .iter()
        .chain(&export.data)
        .map(|(_, value)| value.len())
        .sum();
    eprintln!(
        "R7: {} pages of {PAGE_SIZE} history items, {} history items, {} data items, {bytes} \
         bytes, {} space channels",
        export.pages,
        export.history.len(),
        export.data.len(),
        channels.len(),
    );

    // Negative control: the whole export validates as one page, and the same page with one
    // changed byte in a data item fails the validation.
    let whole = |data: Vec<(Blake2b256Hash, Vec<u8>)>| {
        RSpaceImporterInstance::validate_state_items(
            export.history.clone(),
            data,
            vec![(r1.clone(), None)],
            i32::try_from(export.history.len() + 1).expect("the item count fits i32"),
            0,
            source.history().importer(),
        )
    };
    assert_eq!(
        whole(export.data.clone()),
        Ok(()),
        "the whole export validates"
    );
    let mut tampered = export.data.clone();
    let (_, value) = tampered.first_mut().expect("the export has data items");
    let last = value.len() - 1;
    value[last] ^= 0x01;
    assert!(
        whole(tampered).is_err(),
        "a changed data item fails the validation"
    );

    let mut target = NativeChain::open(&directory.join("target"), None).await;
    let importer = target.history().importer();
    importer.set_history_items(export.history.clone());
    importer.set_data_items(export.data.clone());
    importer.set_root(&r1);
    target.reset(&r1).await;

    // The test reads the imported state through the history reader, which finds a data item
    // under the raw key of the importer. It does not export again from the imported store: the
    // exporter reads data items only under the bincode key (verification.md, section 34.5).
    assert!(!channels.is_empty(), "block 1 touches space channels");
    for encoded in &channels {
        let channel: Par = bincode::deserialize(encoded).expect("decode a channel");
        assert_eq!(
            source.data(&channel).await,
            target.data(&channel).await,
            "the records of {channel:?} at R1"
        );
        assert_eq!(
            source.waiters(&channel).await,
            target.waiters(&channel).await,
            "the waiters of {channel:?} at R1"
        );
    }

    let block = second_block();
    let (on_source, _) = play_block(&mut source, &block).await;
    let (on_target, _) = play_block(&mut target, &block).await;
    for (own, imported) in on_source.iter().zip(&on_target) {
        assert_same_record(own, imported);
    }
    for (record, deploy) in on_target.iter().zip(&block) {
        certify_record(&mut target, record, deploy.limit).await;
    }
    std::fs::remove_dir_all(&directory).expect("remove the run directory");
}
