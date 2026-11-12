use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::durable_publication::directory_snapshot;
use super::durable_rspace_authority::Profile;
use super::support::trial_scratch;
use crate::rspace::errors::{AuthorityBackendMismatch, HistoryRepositoryError};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::history_repository::HistoryRepository;
use crate::rspace::hot_store::HotStore;
use crate::rspace::persistence::durable::{
    DurableAdmissionErrorV1, DurableIoError, DurableLimitConfig, DurableLimits,
    DurableStartupErrorV1,
};
use crate::rspace::rspace::{RSpace, RSpaceStore};
use crate::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use crate::rspace::shared::key_value_store_manager::{
    KeyValueStoreManager, RSpaceBackendSelection,
};
use crate::rspace::shared::lmdb_dir_store_manager::{
    DURABLE_RSPACE_DIRECTORY_NAME, Db, LmdbDirStoreManager, LmdbEnvConfig,
};
use crate::rspace::state::rspace_state_authority::{
    AuthorityRouteV1, DurableBackendErrorV1, DurableOwnerSlotV1, RSpaceStateAuthority,
};

const CATALOG_FILE_NAME: &str = "catalog.v1";
const ENVIRONMENT_BYTES: usize = 16 * 1024 * 1024;
const LEGACY_IS_FIXED: &str =
    "RSpace backend of prefix rspace is fixed as Legacy and cannot become Durable";
const DURABLE_IS_FIXED: &str =
    "RSpace backend of prefix rspace is fixed as Durable and cannot become Legacy";

type StringRepository =
    Box<dyn HistoryRepository<String, String, String, String> + Send + Sync + 'static>;
type StringHotStore = Box<dyn HotStore<String, String, String, String>>;

#[derive(Clone, Copy, Debug)]
enum FirstUse {
    RSpaceStores,
    StateAuthority,
    StoreName,
}

fn node_like_mapping() -> HashMap<Db, LmdbEnvConfig> {
    let prefixes = ["rspace", "eval", "scope"];
    let roles = [("history", "history"), ("roots", "history"), ("cold", "cold")];
    let mut mapping = HashMap::with_capacity(prefixes.len() * roles.len());
    for prefix in prefixes {
        for (role, environment) in roles {
            mapping.insert(
                Db::new(format!("{prefix}-{role}"), None),
                LmdbEnvConfig::new(format!("{prefix}/{environment}"), ENVIRONMENT_BYTES)
                    .with_max_dbs(32),
            );
        }
    }
    mapping
}

fn manager_over(directory: &Path) -> impl KeyValueStoreManager {
    LmdbDirStoreManager::new(directory.to_path_buf(), node_like_mapping())
}

fn open(store: RSpaceStore) -> Result<(StringRepository, StringHotStore), HistoryRepositoryError> {
    RSpace::<String, String, String, String>::create_history_repo(store)
}

fn is_durable(authority: &RSpaceStateAuthority) -> bool {
    matches!(authority.route(), AuthorityRouteV1::Durable(_))
}

fn owner_slot(authority: &RSpaceStateAuthority) -> Arc<DurableOwnerSlotV1<Profile>> {
    match authority.route() {
        AuthorityRouteV1::Durable(backend) => backend
            .owner_slot::<String, String, String, String>()
            .expect("the durable owner slot serves the string-channel profile"),
        AuthorityRouteV1::Legacy => panic!("the authority uses a legacy backend"),
    }
}

fn lmdb_environment_exists(directory: &Path, environment: &str) -> bool {
    directory.join(environment).join("data.mdb").is_file()
}

fn files_below(root: &Path) -> BTreeSet<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeSet::new();
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("list a directory of the trial") {
            let entry = entry.expect("read a directory entry of the trial");
            let entry_path = entry.path();
            if entry
                .file_type()
                .expect("read the type of a trial entry")
                .is_dir()
            {
                pending.push(entry_path);
            } else {
                files.insert(
                    entry_path
                        .strip_prefix(root)
                        .expect("a listed entry lies below the trial root")
                        .to_path_buf(),
                );
            }
        }
    }
    files
}

fn has_no_entry(directory: &Path) -> bool {
    std::fs::read_dir(directory)
        .expect("list the manager directory")
        .next()
        .is_none()
}

fn refusal_message<T>(outcome: Result<T, heed::Error>, attempt: &str) -> String {
    match outcome {
        Err(heed::Error::Io(error)) if error.kind() == std::io::ErrorKind::InvalidInput => {
            error.to_string()
        }
        Err(other) => panic!("{attempt} was refused outside the manager's own refusals: {other:?}"),
        Ok(_) => panic!("{attempt} was accepted"),
    }
}

#[tokio::test]
async fn a_durable_prefix_has_one_authority_and_one_owner_slot_per_manager() {
    let scratch = trial_scratch("durable-selection-shared-authority-");
    let mut manager = manager_over(scratch.path());
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("the durable backend is selected before the prefix is used");
    let first = manager
        .r_space_stores()
        .await
        .expect("the manager builds the first RSpace store");
    let second = manager
        .r_space_stores()
        .await
        .expect("the manager builds the second RSpace store");
    let first_authority = first.authority();
    let second_authority = second.authority();
    assert!(
        Arc::ptr_eq(&first_authority, &second_authority),
        "one manager built two authorities for one durable prefix"
    );
    assert!(is_durable(&first_authority), "the selected prefix does not use the durable backend");

    let (first_repository, _first_hot) =
        open(first).expect("the first RSpace store opens the durable store");
    let (second_repository, _second_hot) =
        open(second).expect("the second RSpace store shares the open durable store");
    let slot = owner_slot(&first_authority);
    assert!(
        Arc::ptr_eq(&slot, &owner_slot(&second_authority)),
        "the two RSpace stores of one manager opened two owner slots"
    );
    let published = Blake2b256Hash::from(slot.published_core().current_root());
    assert_eq!(first_repository.root(), published);
    assert_eq!(second_repository.root(), published);

    assert_eq!(DURABLE_RSPACE_DIRECTORY_NAME, "durable");
    let store_directory = Path::new("rspace").join("durable");
    assert!(
        directory_snapshot(&scratch.path().join(&store_directory)).contains_key(CATALOG_FILE_NAME),
        "the durable store's catalog is not in the durable directory of the selected prefix"
    );
    let files = files_below(scratch.path());
    assert!(
        files.iter().all(|file| file.starts_with(&store_directory)),
        "the manager wrote outside the durable store directory: {files:?}"
    );
}

#[tokio::test]
async fn the_backend_of_a_prefix_is_fixed_at_its_first_use() {
    for first_use in [FirstUse::RSpaceStores, FirstUse::StateAuthority, FirstUse::StoreName] {
        let scratch = trial_scratch("durable-selection-legacy-first-use-");
        let mut manager = manager_over(scratch.path());
        let used = match first_use {
            FirstUse::RSpaceStores => Some(
                manager
                    .r_space_stores()
                    .await
                    .expect("the unselected prefix opens through r_space_stores")
                    .authority(),
            ),
            FirstUse::StateAuthority => Some(
                manager
                    .state_authority("rspace")
                    .await
                    .expect("the unselected prefix opens through state_authority"),
            ),
            FirstUse::StoreName => {
                manager
                    .store("rspace-history".to_owned())
                    .await
                    .expect("the unselected prefix serves its history store name");
                None
            }
        };
        assert!(
            lmdb_environment_exists(scratch.path(), "rspace/history"),
            "the first use by {first_use:?} opened no LMDB environment"
        );
        let before = files_below(scratch.path());
        let refused = refusal_message(
            manager
                .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
                .await,
            &format!("a durable selection after the first use by {first_use:?}"),
        );
        assert_eq!(refused, LEGACY_IS_FIXED);
        assert_eq!(
            files_below(scratch.path()),
            before,
            "the refused selection after the first use by {first_use:?} changed the files"
        );
        assert!(
            !scratch.path().join("rspace/durable").exists(),
            "the refused selection after the first use by {first_use:?} created the durable \
             directory"
        );
        let cached = manager
            .state_authority("rspace")
            .await
            .expect("the used prefix still has its authority");
        assert!(
            !is_durable(&cached),
            "the refused selection after the first use by {first_use:?} changed the backend"
        );
        if let Some(used) = &used {
            assert!(
                Arc::ptr_eq(used, &cached),
                "the refused selection after the first use by {first_use:?} replaced the authority"
            );
        }

        manager
            .shutdown()
            .await
            .expect("the manager of the used prefix shuts down");
        let refused = refusal_message(
            manager
                .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
                .await,
            &format!("a durable selection after the shutdown that followed {first_use:?}"),
        );
        assert_eq!(refused, LEGACY_IS_FIXED);
        let reopened = manager
            .state_authority("rspace")
            .await
            .expect("the used prefix reopens after the shutdown");
        assert!(
            !is_durable(&reopened),
            "the prefix first used by {first_use:?} changed its backend across the shutdown"
        );
        assert!(
            !scratch.path().join("rspace/durable").exists(),
            "the prefix first used by {first_use:?} gained a durable directory after the shutdown"
        );
    }

    let scratch = trial_scratch("durable-selection-before-first-use-");
    let mut manager = manager_over(scratch.path());
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("a selection before the first use is accepted");
    let used = manager
        .r_space_stores()
        .await
        .expect("the selected prefix opens through r_space_stores")
        .authority();
    assert!(is_durable(&used), "a selection before the first use did not take effect");
    assert!(
        scratch.path().join("rspace/durable").is_dir(),
        "the first use of the durable prefix did not create its durable directory"
    );
    assert!(
        !lmdb_environment_exists(scratch.path(), "rspace/history"),
        "the first use of the durable prefix opened its LMDB environment"
    );
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
            .await,
        "a legacy selection after the first use of the durable prefix",
    );
    assert_eq!(refused, DURABLE_IS_FIXED);
    let cached = manager
        .state_authority("rspace")
        .await
        .expect("the durable prefix still has its authority");
    assert!(
        Arc::ptr_eq(&used, &cached),
        "the refused legacy selection replaced the authority of the durable prefix"
    );

    manager
        .shutdown()
        .await
        .expect("the manager of the durable prefix shuts down");
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
            .await,
        "a legacy selection after the shutdown of the durable prefix",
    );
    assert_eq!(refused, DURABLE_IS_FIXED);
    let reopened = manager
        .state_authority("rspace")
        .await
        .expect("the durable prefix reopens after the shutdown");
    assert!(is_durable(&reopened), "the durable prefix changed its backend across the shutdown");
    assert!(
        !lmdb_environment_exists(scratch.path(), "rspace/history"),
        "the durable prefix opened its LMDB environment after the shutdown"
    );
}

#[tokio::test]
async fn a_durable_selection_is_refused_for_eval_and_for_a_prefix_that_is_not_one_path_component() {
    let scratch = trial_scratch("durable-selection-refused-prefix-");
    let mut manager = manager_over(scratch.path());
    let absolute = scratch
        .path()
        .join("absolute")
        .to_str()
        .expect("the trial directory has a UTF-8 path")
        .to_owned();
    let refused = refusal_message(
        manager
            .select_rspace_backend("eval", RSpaceBackendSelection::Durable)
            .await,
        "a durable selection for the eval prefix",
    );
    assert_eq!(refused, "the durable RSpace backend cannot be selected for the eval prefix");
    let unsafe_prefixes = ["", ".", "..", "rspace/history", "rspace/", absolute.as_str()];
    for prefix in unsafe_prefixes {
        let refused = refusal_message(
            manager
                .select_rspace_backend(prefix, RSpaceBackendSelection::Durable)
                .await,
            &format!("a durable selection for the prefix {prefix:?}"),
        );
        assert_eq!(
            refused,
            format!("durable RSpace prefix {prefix:?} is not exactly one normal path component")
        );
    }
    assert!(
        has_no_entry(scratch.path()),
        "a refused durable selection created an entry in the manager directory"
    );

    for prefix in unsafe_prefixes {
        let refused = refusal_message(
            manager.state_authority(prefix).await,
            &format!("the first use of the unmapped prefix {prefix:?}"),
        );
        assert!(
            refused.starts_with("missing authority database mapping for "),
            "the first use of the prefix {prefix:?} was refused with: {refused}"
        );
    }
    assert!(
        has_no_entry(scratch.path()),
        "the first use of a refused prefix created an entry in the manager directory"
    );
    assert!(
        !scratch.path().join("..").join("durable").exists(),
        "the first use of a refused prefix created a directory beside the manager directory"
    );

    let authority = manager
        .eval_stores()
        .await
        .expect("the eval prefix opens after its refused durable selection")
        .authority();
    assert!(!is_durable(&authority), "the eval prefix uses the durable backend");
    assert!(
        authority.history_store().is_ok(),
        "the eval authority does not serve its legacy history facet"
    );
    assert!(
        lmdb_environment_exists(scratch.path(), "eval/history"),
        "the eval prefix opened no LMDB environment"
    );
    assert!(
        !scratch.path().join("eval/durable").exists(),
        "the eval prefix gained a durable directory"
    );
}

#[tokio::test]
async fn a_manager_without_a_durable_backend_accepts_only_the_legacy_selection() {
    let default_refusal =
        "this store manager has no durable RSpace backend to select for prefix rspace";
    let mut manager = InMemoryStoreManager::new();
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("the default accepts the legacy selection before the first use");
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
            .await,
        "a durable selection of the in-memory manager",
    );
    assert_eq!(refused, default_refusal);

    let used = manager
        .state_authority("rspace")
        .await
        .expect("the in-memory manager builds its authority");
    assert!(!is_durable(&used), "the in-memory manager built a durable authority");
    assert!(
        used.history_store().is_ok(),
        "the in-memory authority does not serve its legacy history facet"
    );
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("the default accepts the legacy selection after the first use");
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
            .await,
        "a durable selection of the used in-memory manager",
    );
    assert_eq!(refused, default_refusal);
    let cached = manager
        .state_authority("rspace")
        .await
        .expect("the in-memory manager still has its authority");
    assert!(
        Arc::ptr_eq(&used, &cached),
        "a selection replaced the authority of the in-memory manager"
    );
}

#[tokio::test]
async fn a_differing_selection_is_refused_and_an_equal_one_changes_nothing() {
    let durable_scratch = trial_scratch("durable-selection-durable-first-");
    let mut manager = manager_over(durable_scratch.path());
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("the first durable selection is accepted");
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("an equal durable selection is accepted before the first use");
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
            .await,
        "a legacy selection of a prefix selected as durable",
    );
    assert_eq!(refused, DURABLE_IS_FIXED);
    let used = manager
        .state_authority("rspace")
        .await
        .expect("the prefix selected as durable opens");
    assert!(is_durable(&used), "the refused legacy selection changed the selected backend");
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("an equal durable selection is accepted after the first use");
    let cached = manager
        .state_authority("rspace")
        .await
        .expect("the durable prefix still has its authority");
    assert!(
        Arc::ptr_eq(&used, &cached),
        "an equal durable selection replaced the authority of the prefix"
    );

    let legacy_scratch = trial_scratch("durable-selection-legacy-first-");
    let mut manager = manager_over(legacy_scratch.path());
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("the first legacy selection is accepted");
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("an equal legacy selection is accepted before the first use");
    let refused = refusal_message(
        manager
            .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
            .await,
        "a durable selection of a prefix selected as legacy",
    );
    assert_eq!(refused, LEGACY_IS_FIXED);
    assert!(
        has_no_entry(legacy_scratch.path()),
        "a selection before the first use created an entry in the manager directory"
    );
    let used = manager
        .state_authority("rspace")
        .await
        .expect("the prefix selected as legacy opens");
    assert!(!is_durable(&used), "the refused durable selection changed the selected backend");
    assert!(
        lmdb_environment_exists(legacy_scratch.path(), "rspace/history"),
        "the prefix selected as legacy opened no LMDB environment"
    );
    assert!(
        !legacy_scratch.path().join("rspace/durable").exists(),
        "the prefix selected as legacy gained a durable directory"
    );
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("an equal legacy selection is accepted after the first use");
    let cached = manager
        .state_authority("rspace")
        .await
        .expect("the legacy prefix still has its authority");
    assert!(
        Arc::ptr_eq(&used, &cached),
        "an equal legacy selection replaced the authority of the prefix"
    );

    let unselected_scratch = trial_scratch("durable-selection-unselected-");
    let mut manager = manager_over(unselected_scratch.path());
    let used = manager
        .state_authority("rspace")
        .await
        .expect("the unselected prefix opens");
    manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Legacy)
        .await
        .expect("the legacy selection equals the record of the first use");
    let cached = manager
        .state_authority("rspace")
        .await
        .expect("the unselected prefix still has its authority");
    assert!(
        Arc::ptr_eq(&used, &cached),
        "a legacy selection after an unselected first use replaced the authority"
    );
    assert!(!is_durable(&cached), "an unselected prefix uses the durable backend");
}

#[tokio::test]
async fn a_store_name_of_a_durable_prefix_is_refused_by_the_backend_facet() {
    let scratch = trial_scratch("durable-selection-store-names-");
    let mut manager = manager_over(scratch.path());
    let durable_prefixes = ["rspace", "scope"];
    for prefix in durable_prefixes {
        manager
            .select_rspace_backend(prefix, RSpaceBackendSelection::Durable)
            .await
            .expect("the durable backend is selected before the prefix is used");
    }
    for prefix in durable_prefixes {
        for (domain, facet) in [
            ("history", AuthorityBackendMismatch::History),
            ("roots", AuthorityBackendMismatch::Roots),
            ("cold", AuthorityBackendMismatch::Cold),
        ] {
            let name = format!("{prefix}-{domain}");
            match manager.store(name.clone()).await {
                Err(heed::Error::Io(error)) => assert_eq!(
                    error
                        .get_ref()
                        .and_then(|source| source.downcast_ref::<AuthorityBackendMismatch>()),
                    Some(&facet),
                    "the store name {name} was refused outside the backend's facet refusal: \
                     {error}"
                ),
                Err(other) => panic!("the store name {name} was refused with {other:?}"),
                Ok(_) => panic!("the store name {name} of a durable prefix was served"),
            }
        }
    }
    assert_eq!(
        files_below(scratch.path()),
        BTreeSet::new(),
        "a refused store name of a durable prefix created a file"
    );
    for prefix in durable_prefixes {
        assert!(
            scratch.path().join(prefix).join("durable").is_dir(),
            "the first use of the durable prefix {prefix} did not create its durable directory"
        );
        for environment in ["history", "cold"] {
            assert!(
                !scratch.path().join(prefix).join(environment).exists(),
                "a store name of the durable prefix {prefix} created its LMDB environment \
                 {environment}"
            );
        }
    }
}

#[tokio::test]
async fn shutdown_releases_the_writer_lease_of_a_durable_prefix() {
    let scratch = trial_scratch("durable-selection-shutdown-");
    let mut first_manager = manager_over(scratch.path());
    first_manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("the first manager selects the durable backend");
    let (first_repository, _first_hot) = open(
        first_manager
            .r_space_stores()
            .await
            .expect("the first manager builds its RSpace store"),
    )
    .expect("the first manager opens the durable store");

    let mut second_manager = manager_over(scratch.path());
    second_manager
        .select_rspace_backend("rspace", RSpaceBackendSelection::Durable)
        .await
        .expect("the second manager selects the durable backend");
    let contended = open(
        second_manager
            .r_space_stores()
            .await
            .expect("the second manager builds its RSpace store"),
    )
    .err()
    .expect("a second manager over a held durable store is refused");
    match &contended {
        HistoryRepositoryError::DurableBackend(error) => {
            match error.downcast_ref::<DurableBackendErrorV1>() {
                Some(DurableBackendErrorV1::Recovery(refusal)) => assert!(
                    matches!(
                        **refusal,
                        DurableAdmissionErrorV1::Startup(DurableStartupErrorV1::Io(
                            DurableIoError::WriterLockContended { .. }
                        ))
                    ),
                    "the second manager was refused with {refusal:?}"
                ),
                other => panic!("the second manager was refused with {other:?}"),
            }
        }
        other => panic!("the second manager was refused outside the durable backend: {other:?}"),
    }

    first_manager
        .shutdown()
        .await
        .expect("the first manager shuts down");
    let (second_repository, _second_hot) = open(
        second_manager
            .r_space_stores()
            .await
            .expect("the second manager builds its RSpace store again"),
    )
    .expect("the second manager opens the store after the first manager's shutdown");
    assert_eq!(second_repository.root(), first_repository.root());
}

#[test]
fn the_unbounded_durable_limits_are_the_limits_of_the_largest_configuration() {
    let largest = DurableLimitConfig {
        max_live_packs: u64::MAX,
        max_pack_bytes: u64::MAX,
        max_transactions: u64::MAX,
        max_object_records: u64::MAX,
        max_total_scan_bytes: u64::MAX,
        max_total_physical_read_bytes: u64::MAX,
        max_index_bytes: u64::MAX,
        max_index_records: u64::MAX,
        max_full_byte_comparison_bytes: u64::MAX,
        max_aggregate_full_byte_comparison_bytes: u64::MAX,
    };
    assert_eq!(DurableLimits::new(largest), Ok(DurableLimits::UNBOUNDED));
}
