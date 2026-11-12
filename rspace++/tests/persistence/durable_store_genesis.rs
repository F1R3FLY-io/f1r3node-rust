use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

use super::durable_publication::directory_snapshot;
use super::durable_recovery::{catalog_image, limits};
use super::durable_rspace_authority::Profile;
use super::support::{empty_roots, encode, trial_scratch};
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableAdmissionLimitsV1, DurableIoError, DurableStartupErrorV1,
    DurableStoreGenesisErrorV1, DurableStoreGenesisStepV1, IndexBindingDispositionV1,
    arm_catalog_slot_write_prefix, arm_durable_store_genesis_failpoint, create_empty_store_v1,
    index_file_name, pack_file_name, recover_read_authority, start_durable_store_genesis_trace,
    take_durable_store_genesis_trace,
};
use crate::rspace::persistence::retention::v2_format::{
    RetentionAnchorV2, empty_retention_radix_ref,
};
use crate::rspace::persistence::{
    CATALOG_SLOT_LEN, CanonicalDagNode, DurableAuthorityViewV1, DurableFormatError,
    IndexGenerationV1, LogPackDecodeLimits, ObjectDigest, ObjectRefV1, PackId, RetainedStateV1,
    SemanticProfile, SemanticStateManifestV1, StateVersionV1,
};

const CATALOG_FILE_NAME: &str = "catalog.v1";

struct EmptyStateOracle {
    empty_root: ObjectRefV1,
    state: StateVersionV1,
    retention_anchor: ObjectRefV1,
}

fn empty_state_oracle() -> EmptyStateOracle {
    let empty_root = CanonicalDagNode::empty().object_ref();
    let manifest_bytes = encode(
        &SemanticStateManifestV1::new(1, Profile::ID, empty_roots(empty_root), None)
            .expect("the schema-one empty manifest is valid"),
    );
    let manifest = ObjectRefV1::new(
        ObjectDigest::of_canonical_bytes(&manifest_bytes),
        manifest_bytes.len() as u64,
    )
    .expect("the empty manifest has a valid object reference");
    let state = StateVersionV1::new(1, manifest);
    let retained = RetainedStateV1::new(state, empty_root.encoded_len() + manifest.encoded_len())
        .expect("the empty state retains its manifest and its empty root");
    let retention_anchor = RetentionAnchorV2::new(retained, 0, empty_retention_radix_ref())
        .expect("an anchor without history names the empty radix")
        .object_ref();
    EmptyStateOracle {
        empty_root,
        state,
        retention_anchor,
    }
}

fn write_owner_only(path: &Path, bytes: &[u8]) {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .expect("create a pre-existing trial file");
    file.write_all(bytes)
        .expect("write a pre-existing trial file");
    file.sync_all().expect("sync a pre-existing trial file");
    #[cfg(unix)]
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .expect("restrict a pre-existing trial file to its owner");
}

fn reopen(root: &Path) -> Result<DurableAuthorityViewV1<Profile>, Box<DurableAdmissionErrorV1>> {
    recover_read_authority::<Profile>(
        root,
        limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .map_err(Box::new)
}

fn assert_empty_state(view: &DurableAuthorityViewV1<Profile>, oracle: &EmptyStateOracle) {
    let catalog = view.catalog();
    assert_eq!(view.state(), oracle.state);
    assert_eq!(view.state().generation(), 1);
    assert_eq!(view.manifest(), oracle.state.manifest());
    assert_eq!(catalog.epoch(), 1);
    assert_eq!(catalog.profile_id(), Profile::ID);
    assert_eq!(catalog.pack().first_live_pack(), PackId::GENESIS.get());
    assert_eq!(catalog.pack().active_pack(), PackId::GENESIS.get());
    assert_eq!(catalog.pack().index_generation(), IndexGenerationV1::FIRST.get());
    assert_eq!(catalog.pack().retention_manifest(), oracle.retention_anchor);
    assert_eq!(
        view.semantic_roots()
            .expect("the empty manifest names its namespace roots"),
        empty_roots(oracle.empty_root)
    );
    assert!(
        view.data_entries(u64::MAX, u64::MAX)
            .expect("the data namespace enumerates")
            .is_empty()
    );
    assert!(
        view.continuation_entries(u64::MAX, u64::MAX, u64::MAX)
            .expect("the continuation namespace enumerates")
            .is_empty()
    );
    assert!(
        view.join_entries(u64::MAX, u64::MAX)
            .expect("the join namespace enumerates")
            .is_empty()
    );
    let report = view.report();
    assert_eq!(report.startup_disposition, IndexBindingDispositionV1::VerifiedExisting);
    assert_eq!(report.retained_states, 1);
    assert_eq!(report.current_structural.distinct_objects, 2);
    assert_eq!(report.retention_metadata_objects, 2);
}

fn require_second_genesis_refused(root: &Path) {
    let before = directory_snapshot(root);
    let listed = before.keys().cloned().collect::<Vec<_>>();
    match create_empty_store_v1::<Profile>(root, limits()) {
        Err(DurableStoreGenesisErrorV1::DirectoryHoldsStoreFiles { directory, files }) => {
            assert_eq!(directory.as_path(), root);
            assert_eq!(files.into_vec(), listed);
        }
        other => panic!("a genesis over the store files {listed:?} returned {other:?}"),
    }
    assert_eq!(directory_snapshot(root), before, "a refused genesis changed the directory");
}

fn require_unopened_crash_state(
    error: &DurableAdmissionErrorV1,
    crashed: &BTreeMap<String, Vec<u8>>,
    point: DurableStoreGenesisStepV1,
) {
    match (error, crashed.get(CATALOG_FILE_NAME)) {
        (
            DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(DurableIoError::Io {
                source,
                ..
            })),
            None,
        ) if source.kind() == std::io::ErrorKind::NotFound => {}
        (
            DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::NoCatalogAuthority),
            Some(catalog),
        ) if catalog.iter().all(|byte| *byte == 0) => {}
        (error, catalog) => panic!(
            "the crash at {point:?} left a store whose recovery reported {error:?} (catalog \
             present: {})",
            catalog.is_some()
        ),
    }
}

fn traced_genesis_points() -> Vec<DurableStoreGenesisStepV1> {
    let scratch = trial_scratch("store-genesis-trace-");
    start_durable_store_genesis_trace();
    let created = create_empty_store_v1::<Profile>(scratch.path(), limits());
    let points = take_durable_store_genesis_trace();
    created.expect("the traced genesis creates the empty store");
    points
}

#[test]
fn an_empty_store_reopens_at_generation_one_with_empty_namespaces() {
    let oracle = empty_state_oracle();
    let scratch = trial_scratch("store-genesis-");
    let slot = create_empty_store_v1::<Profile>(scratch.path(), limits())
        .expect("an empty directory receives the empty store");
    let view = reopen(scratch.path()).expect("the empty store reopens through cold admission");
    assert_eq!(view.catalog(), slot);
    assert_empty_state(&view, &oracle);
    drop(view);
    let files = directory_snapshot(scratch.path());
    assert_eq!(
        files.keys().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([
            CATALOG_FILE_NAME.to_owned(),
            pack_file_name(PackId::GENESIS),
            index_file_name(IndexGenerationV1::FIRST),
        ])
    );
    assert_eq!(files[CATALOG_FILE_NAME], catalog_image(slot));
}

#[test]
fn independently_created_empty_stores_share_the_empty_state_manifest_digest() {
    let oracle = empty_state_oracle();
    let left = trial_scratch("store-genesis-left-");
    let right = trial_scratch("store-genesis-right-");
    let left_slot = create_empty_store_v1::<Profile>(left.path(), limits())
        .expect("the left directory receives an empty store");
    let right_slot = create_empty_store_v1::<Profile>(right.path(), limits())
        .expect("the right directory receives an empty store");
    assert_ne!(left_slot.store_id(), right_slot.store_id());
    assert_eq!(left_slot.state().manifest().digest(), right_slot.state().manifest().digest());
    assert_eq!(left_slot.state().manifest(), oracle.state.manifest());
    let left_view = reopen(left.path()).expect("the left empty store reopens");
    let right_view = reopen(right.path()).expect("the right empty store reopens");
    assert_eq!(left_view.manifest().digest(), right_view.manifest().digest());
    assert_eq!(left_view.manifest(), oracle.state.manifest());
    assert_eq!(left_view.state(), right_view.state());
    assert_empty_state(&left_view, &oracle);
    assert_empty_state(&right_view, &oracle);
}

#[test]
fn a_crash_at_any_genesis_write_or_sync_point_never_opens_a_different_state() {
    let oracle = empty_state_oracle();
    let points = traced_genesis_points();
    assert!(!points.is_empty(), "genesis reaches at least one write or sync point");
    for (position, point) in points.iter().enumerate() {
        assert!(!points[..position].contains(point), "genesis reaches {point:?} twice");
    }
    let slot_write = points
        .iter()
        .position(|point| *point == DurableStoreGenesisStepV1::WriteCatalogSlot)
        .expect("genesis reaches the catalog slot write");
    let mut exercised = Vec::with_capacity(points.len());
    let mut opened = Vec::with_capacity(points.len());
    let mut unopened = Vec::with_capacity(points.len());
    for (position, point) in points.iter().copied().enumerate() {
        let scratch = trial_scratch("store-genesis-crash-");
        let root = scratch.path();
        arm_durable_store_genesis_failpoint(point);
        start_durable_store_genesis_trace();
        let failed = create_empty_store_v1::<Profile>(root, limits());
        let reached = take_durable_store_genesis_trace();
        assert_eq!(reached, points[..=position], "the crash at {point:?} stops genesis there");
        let injected = format!("injected durable store genesis failure at {point:?}");
        match (failed, position < slot_write) {
            (Err(DurableStoreGenesisErrorV1::Io(DurableIoError::Io { source, .. })), true) |
            (
                Err(DurableStoreGenesisErrorV1::CatalogIndeterminate(DurableIoError::Io {
                    source,
                    ..
                })),
                false,
            ) => assert_eq!(source.to_string(), injected),
            (other, _) => panic!("the crash at {point:?} returned {other:?}"),
        }
        let crashed = directory_snapshot(root);
        assert_eq!(
            crashed.is_empty(),
            position == 0,
            "only a crash before the first create leaves no store file ({point:?})"
        );
        match reopen(root) {
            Ok(view) => {
                assert!(
                    position > slot_write,
                    "the crash at {point:?} left a store that opens before its catalog slot was \
                     written"
                );
                assert_empty_state(&view, &oracle);
                opened.push(point);
            }
            Err(error) => {
                assert!(
                    position <= slot_write,
                    "the crash at {point:?} left a written catalog slot that does not open: \
                     {error:?}"
                );
                require_unopened_crash_state(&error, &crashed, point);
                unopened.push(point);
            }
        }
        assert_eq!(directory_snapshot(root), crashed, "recovery changed the crash at {point:?}");
        if crashed.is_empty() {
            let slot = create_empty_store_v1::<Profile>(root, limits())
                .expect("a directory without store files receives the empty store");
            let view = reopen(root).expect("the store created after the crash reopens");
            assert_eq!(view.catalog(), slot);
            assert_empty_state(&view, &oracle);
        } else {
            require_second_genesis_refused(root);
        }
        exercised.push(point);
    }
    assert_eq!(exercised, points);
    assert!(!opened.is_empty(), "some crash point lies after the catalog slot write");
    assert!(!unopened.is_empty(), "some crash point lies before the catalog slot write");

    let mut torn_opened = 0_usize;
    let mut torn_unopened = 0_usize;
    for prefix in 0..CATALOG_SLOT_LEN {
        let scratch = trial_scratch("store-genesis-torn-");
        let root = scratch.path();
        arm_catalog_slot_write_prefix(prefix);
        match create_empty_store_v1::<Profile>(root, limits()) {
            Err(DurableStoreGenesisErrorV1::CatalogIndeterminate(DurableIoError::Io {
                source,
                ..
            })) => assert_eq!(
                source.to_string(),
                format!("injected torn catalog-slot write after {prefix} bytes")
            ),
            other => panic!("a {prefix}-byte torn catalog slot returned {other:?}"),
        }
        let crashed = directory_snapshot(root);
        match reopen(root) {
            Ok(view) => {
                assert!(
                    view.catalog().encode()[prefix..]
                        .iter()
                        .all(|byte| *byte == 0),
                    "a {prefix}-byte torn catalog slot opened although its unwritten suffix is \
                     not zero"
                );
                assert_eq!(crashed[CATALOG_FILE_NAME], catalog_image(view.catalog()));
                assert_empty_state(&view, &oracle);
                torn_opened += 1;
            }
            Err(error) => match *error {
                DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::NoCatalogAuthority) => {
                    assert_eq!(prefix, 0, "only an unwritten catalog slot leaves no authority");
                    torn_unopened += 1;
                }
                DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Catalog(
                    DurableFormatError::NoValidCatalog { .. },
                )) => {
                    assert_ne!(prefix, 0, "an unwritten catalog slot is not a torn slot");
                    torn_unopened += 1;
                }
                error => panic!("a {prefix}-byte torn catalog slot reported {error:?}"),
            },
        }
        assert_eq!(
            directory_snapshot(root),
            crashed,
            "recovery changed a {prefix}-byte torn catalog slot"
        );
        require_second_genesis_refused(root);
    }
    assert!(torn_opened > 0, "some torn catalog prefix already holds the whole slot");
    assert!(torn_unopened > 0, "some torn catalog prefix leaves no authority");
}

#[test]
fn genesis_refuses_a_directory_holding_any_store_file_and_leaves_it_unchanged() {
    let existing = trial_scratch("store-genesis-existing-");
    create_empty_store_v1::<Profile>(existing.path(), limits())
        .expect("the first genesis creates the empty store");
    require_second_genesis_refused(existing.path());

    let store_files = [
        CATALOG_FILE_NAME.to_owned(),
        pack_file_name(PackId::GENESIS),
        pack_file_name(PackId::new(5).expect("pack five is a valid pack")),
        index_file_name(IndexGenerationV1::FIRST),
        format!("{}.rebuild", index_file_name(IndexGenerationV1::FIRST)),
        "reclaim.v1".to_owned(),
        "reclaim.v1.tmp".to_owned(),
    ];
    for name in store_files {
        let scratch = trial_scratch("store-genesis-refusal-");
        write_owner_only(&scratch.path().join(&name), b"pre-existing durable store bytes");
        assert_eq!(
            directory_snapshot(scratch.path())
                .into_keys()
                .collect::<Vec<_>>(),
            [name.clone()]
        );
        require_second_genesis_refused(scratch.path());
    }
}

#[test]
fn genesis_ignores_names_that_no_durable_store_creates() {
    let oracle = empty_state_oracle();
    let scratch = trial_scratch("store-genesis-foreign-");
    for name in ["operator-notes.txt", "pack-1.rsp", "index-1.rsi"] {
        write_owner_only(&scratch.path().join(name), b"not a durable store file");
    }
    let slot = create_empty_store_v1::<Profile>(scratch.path(), limits())
        .expect("foreign names do not block the empty store");
    let view = reopen(scratch.path()).expect("the empty store beside foreign names reopens");
    assert_eq!(view.catalog(), slot);
    assert_empty_state(&view, &oracle);
}
