use std::collections::BTreeMap;
use std::sync::Arc;

use proptest::prelude::*;
use shared::rust::test_scratch;

use super::{AuthorityBackend, CURRENT_ROOT_KEY, RSpaceStateAuthority, verified_root_key};
use crate::rspace::errors::{
    AuthorityBackendMismatch, HistoryError, HistoryRepositoryError, RootError,
};
use crate::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use crate::rspace::history::cold_store::{
    ColdLeafAddress, DataLeaf, PersistedData, PersistedDataKind, PersistedDataView,
};
use crate::rspace::history::history::{History, HistoryInstances};
use crate::rspace::history::history_action::{HistoryAction, InsertAction};
use crate::rspace::history::instances::radix_history::PreparedHistoryCheckpoint;
use crate::rspace::history::roots_store::CurrentRoot;
use crate::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use crate::rspace::shared::lmdb_dir_store_manager::{Db, LmdbDirStoreManager, LmdbEnvConfig};
use crate::rspace::shared::trie_exporter::StateExportError;
use crate::rspace::state::closure_verifier::RecordValidator;
use crate::rspace::state::import_session::StateImportError;

#[derive(Debug, PartialEq, Eq)]
struct MemorySnapshot {
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    roots: BTreeMap<Vec<u8>, Vec<u8>>,
}

fn memory_snapshot(authority: &RSpaceStateAuthority) -> MemorySnapshot {
    let AuthorityBackend::Memory(state) = &authority.backend else {
        panic!("test requires the in-memory authority");
    };
    let state = state.lock().expect("authority state lock");
    MemorySnapshot {
        history: state.history.clone(),
        cold: state.cold.clone(),
        roots: state.roots.clone(),
    }
}

fn validator() -> RecordValidator { Arc::new(|_, _| Ok(())) }

fn scope_layout_configuration(
    prefix: &str,
    names: [&str; 3],
    cold_environment: &str,
) -> std::collections::HashMap<Db, LmdbEnvConfig> {
    ["history", "roots", "cold"]
        .into_iter()
        .zip(names)
        .map(|(role, name)| {
            let environment = if role == "cold" {
                cold_environment
            } else {
                "shared"
            };
            (
                Db::new(format!("{prefix}-{role}"), Some(name.to_owned())),
                LmdbEnvConfig::new(environment.to_owned(), 16 * 1024 * 1024).with_max_dbs(32),
            )
        })
        .collect()
}

#[tokio::test]
async fn scope_layout_preflight_refuses_before_open() {
    for case in 0..16 {
        for ordinary_first in [false, true] {
            let scratch = test_scratch::acquire("rspace-scope-refusal-");
            let target = scratch.path().join("unopened");
            let mut mapping =
                scope_layout_configuration("rspace", ["objects", "catalog", "payloads"], "split");
            let config = LmdbEnvConfig::new("shared".to_owned(), 16 * 1024 * 1024).with_max_dbs(32);
            mapping.insert(
                Db::new("application".to_owned(), None),
                LmdbEnvConfig::new("application".to_owned(), 16 * 1024 * 1024).with_max_dbs(32),
            );
            let expected = match case {
                0 => {
                    mapping.retain(|db, _| db.id() != "rspace-cold");
                    "missing authority"
                }
                1 => {
                    mapping.insert(
                        Db::new("rspace-history".to_owned(), Some("second-history".to_owned())),
                        config.clone(),
                    );
                    "duplicate logical"
                }
                2 | 3 => {
                    for (db, cfg) in &mut mapping {
                        if db.id() == "rspace-roots" {
                            if case == 2 {
                                cfg.name = "elsewhere".to_owned();
                            } else {
                                cfg.max_dbs += 1;
                            }
                        }
                    }
                    if case == 2 {
                        "must share"
                    } else {
                        "incompatible configurations"
                    }
                }
                4..=6 => {
                    let role = ["history", "roots", "cold"][case - 4];
                    mapping.retain(|db, _| db.id() != format!("rspace-{role}"));
                    mapping.insert(
                        Db::new(format!("rspace-{role}"), Some("bad\0name".to_owned())),
                        config.clone(),
                    );
                    "NUL"
                }
                7..=9 => {
                    let (role, name) =
                        [("roots", "objects"), ("cold", "objects"), ("cold", "catalog")][case - 7];
                    mapping.retain(|db, _| db.id() != format!("rspace-{role}"));
                    mapping.insert(
                        Db::new(format!("rspace-{role}"), Some(name.to_owned())),
                        config.clone(),
                    );
                    "distinct physical names"
                }
                10..=12 => {
                    mapping.retain(|db, _| db.id() != "application");
                    let name = if case == 10 { "objects" } else { "payloads" };
                    let mut alias_config = config.clone();
                    if case == 12 {
                        alias_config.name = "split".to_owned();
                    }
                    mapping.insert(
                        Db::new("application".to_owned(), Some(name.to_owned())),
                        alias_config,
                    );
                    "aliases an authority"
                }
                13 | 14 => {
                    mapping = scope_layout_configuration(
                        "rspace",
                        ["rspace-history", "rspace-roots", "rspace-cold"],
                        "split",
                    );
                    let mut alias_config = config.clone();
                    alias_config.name = if case == 13 {
                        "split"
                    } else {
                        "rspace/casper/v2/roots"
                    }
                    .to_owned();
                    mapping.insert(
                        Db::new("application".to_owned(), Some("db".to_owned())),
                        alias_config,
                    );
                    "aliases an authority"
                }
                15 => {
                    mapping.extend(scope_layout_configuration(
                        "eval",
                        ["objects", "eval-catalog", "eval-payloads"],
                        "split",
                    ));
                    "layouts overlap"
                }
                _ => unreachable!(),
            };
            let mut manager = LmdbDirStoreManager::new(target.clone(), mapping);
            let error = if ordinary_first {
                match manager.store("application".to_owned()).await {
                    Ok(_) => panic!("case {case} exposed an ordinary store"),
                    Err(error) => error,
                }
            } else {
                match manager.state_authority("rspace").await {
                    Ok(_) => panic!("case {case} opened an invalid authority"),
                    Err(error) => error,
                }
            };
            assert!(error.to_string().contains(expected), "case {case}: {error}");
            assert!(!target.exists(), "case {case} created a directory before rejection");
        }
    }
}

#[tokio::test]
async fn scope_layout_ordinary_literal_and_alias_routing() {
    let scratch = test_scratch::acquire("rspace-scope-literal-route-");
    let mut mapping =
        scope_layout_configuration("rspace", ["objects", "catalog", "payloads"], "shared");
    mapping.insert(
        Db::new("application".to_owned(), Some("rspace-history".to_owned())),
        LmdbEnvConfig::new("unrelated".to_owned(), 16 * 1024 * 1024).with_max_dbs(32),
    );
    let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
    let store = manager.store("application".to_owned()).await.unwrap();
    store.put(vec![(vec![1], vec![2])]).unwrap();
    let authority = manager.state_authority("rspace").await.unwrap();
    authority.initialize_for_records(&validator()).unwrap();
    assert_eq!(store.get_one(&vec![1]).unwrap(), Some(vec![2]));
    assert!(
        authority
            .history_store()
            .expect("legacy history facet")
            .to_map()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn scope_layout_custom_ownership_survives_shutdown() {
    for retain_authority in [false, true] {
        let scratch = test_scratch::acquire("rspace-scope-custom-owner-");
        let mapping =
            scope_layout_configuration("custom", ["objects", "catalog", "payloads"], "shared");
        let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
        let mut retained = Some(manager.state_authority("custom").await.unwrap());
        retained
            .as_ref()
            .unwrap()
            .initialize_for_records(&validator())
            .unwrap();
        manager.shutdown().await.unwrap();
        if !retain_authority {
            retained.take();
        }
        for role in ["history", "roots", "cold"] {
            let store = manager.store(format!("custom-{role}")).await.unwrap();
            assert!(store.put(vec![(vec![1], vec![2])]).is_err());
            assert!(store.delete(vec![vec![1]]).is_err());
            assert!(store.compare_and_swap_batch(&[], &[]).is_err());
        }
    }
    let scratch = test_scratch::acquire("rspace-scope-custom-ordinary-first-");
    let mapping =
        scope_layout_configuration("custom", ["objects", "catalog", "payloads"], "shared");
    let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
    let exposed = manager.store("custom-history".to_owned()).await.unwrap();
    assert!(manager.state_authority("custom").await.is_err());
    exposed.put(vec![(vec![1], vec![2])]).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn scope_layout_resolves_environment_aliases() {
    use crate::rspace::shared::lmdb_dir_store_manager::resolve_lmdb_environment;
    let scratch = test_scratch::acquire("rspace-scope-path-alias-");
    std::fs::create_dir(scratch.path().join("shared")).unwrap();
    std::os::unix::fs::symlink("shared", scratch.path().join("alias")).unwrap();
    let spelling = scratch.path().join("missing/../alias");
    assert_eq!(
        resolve_lmdb_environment(&spelling).unwrap(),
        std::fs::canonicalize(scratch.path().join("shared")).unwrap()
    );
    assert!(!scratch.path().join("missing").exists());
    let mut mapping =
        scope_layout_configuration("rspace", ["objects", "catalog", "payloads"], "shared");
    mapping.insert(
        Db::new("application".to_owned(), Some("payloads".to_owned())),
        LmdbEnvConfig::new("missing/../alias".to_owned(), 16 * 1024 * 1024).with_max_dbs(32),
    );
    let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
    assert!(manager.store("application".to_owned()).await.is_err());
    assert!(!scratch.path().join("shared/data.mdb").exists());
    std::os::unix::fs::symlink("absent", scratch.path().join("dangling")).unwrap();
    assert!(resolve_lmdb_environment(&scratch.path().join("dangling/child")).is_err());
    let mut mapping =
        scope_layout_configuration("rspace", ["objects", "catalog", "payloads"], "shared");
    for (db, cfg) in &mut mapping {
        if db.id() == "rspace-roots" {
            cfg.name = "alias".to_owned();
        }
    }
    let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
    manager
        .state_authority("rspace")
        .await
        .unwrap()
        .initialize_for_records(&validator())
        .unwrap();
}

fn scope_layout_database(
    path: &std::path::Path,
    name: &str,
    entries: BTreeMap<Vec<u8>, Vec<u8>>,
) -> (Arc<heed::Env>, super::BytesDb) {
    let path =
        crate::rspace::shared::lmdb_dir_store_manager::resolve_lmdb_environment(path).unwrap();
    let env =
        crate::rspace::shared::env_cache::get_or_open_env(&path, 16 * 1024 * 1024, 32).unwrap();
    let mut txn = env.write_txn().unwrap();
    let db = env.create_database(&mut txn, Some(name)).unwrap();
    for (key, value) in entries {
        db.put(&mut txn, &key, &value).unwrap();
    }
    txn.commit().unwrap();
    (env, db)
}

#[tokio::test]
async fn scope_layout_named_migration_and_marker_binding() {
    for default in [false, true] {
        let scratch = test_scratch::acquire("rspace-scope-migration-");
        let names = if default {
            ["rspace-history", "rspace-roots", "rspace-cold"]
        } else {
            ["objects", "catalog", "payloads"]
        };
        let source = RSpaceStateAuthority::in_memory();
        let history = history_at_current(&source);
        let (address, bytes) = data_leaf(b"scoped-legacy-source");
        let (_, prepared) = prepare_insert(history.as_ref(), 81, &address);
        let expected = source
            .commit_local_checkpoint(
                &prepared,
                vec![(address.clone(), bytes.clone())],
                &validator(),
            )
            .unwrap();
        let legacy_roots = BTreeMap::from([
            (expected.root().bytes(), b"tag".to_vec()),
            (b"current-root".to_vec(), expected.root().bytes()),
        ]);
        let (_history_env, _) = scope_layout_database(
            &scratch.path().join("shared"),
            names[0],
            source
                .history_store()
                .expect("legacy history facet")
                .to_map()
                .unwrap(),
        );
        let (_roots_env, _) =
            scope_layout_database(&scratch.path().join("shared"), names[1], legacy_roots);
        let source_name = if default { "db" } else { names[2] };
        let source_entries = source
            .cold_store()
            .expect("legacy cold facet")
            .to_map()
            .unwrap();
        let (cold_env, cold_db) = scope_layout_database(
            &scratch.path().join("split"),
            source_name,
            source_entries.clone(),
        );
        let ambient = if default {
            None
        } else {
            Some(scope_layout_database(
                &scratch.path().join("split"),
                "db",
                BTreeMap::from([(address.encode(), b"unrelated source".to_vec())]),
            ))
        };
        let mapping = scope_layout_configuration("rspace", names, "split");
        let mut manager = LmdbDirStoreManager::new(scratch.path().to_path_buf(), mapping);
        let authority = manager.state_authority("rspace").await.unwrap();
        authority.initialize_for_records(&validator()).unwrap();
        assert_eq!(authority.current_root().unwrap(), *expected.root());
        assert_eq!(
            authority
                .cold_store()
                .expect("legacy cold facet")
                .get_one(&address.encode())
                .unwrap(),
            Some(bytes)
        );
        let txn = cold_env.read_txn().unwrap();
        let after: BTreeMap<_, _> = cold_db
            .iter(&txn)
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(after, source_entries);
        drop(txn);
        if let Some((env, db)) = &ambient {
            let txn = env.read_txn().unwrap();
            assert_eq!(
                db.get(&txn, &address.encode()).unwrap(),
                Some(b"unrelated source".to_vec())
            );
        }
        let catalog = authority
            .roots_store_for_inspection()
            .expect("legacy roots facet")
            .to_map()
            .unwrap();
        let marker_key = b"rspace-state-authority/migration/legacy-state/v2/completed".to_vec();
        let marker = catalog.get(&marker_key).unwrap().clone();
        let encoded = marker
            .strip_prefix(b"rspace-legacy-state-migration\0\x02")
            .unwrap();
        let namespace_len = u32::from_be_bytes(encoded[..4].try_into().unwrap()) as usize;
        let namespace = &encoded[4..4 + namespace_len];
        let layout = super::LmdbAuthorityLayout::new("rspace", names.map(str::to_owned)).unwrap();
        assert_eq!(namespace, layout.namespace());
        if default {
            assert_eq!(namespace, b"rspace");
        }
        let before = authority.current_verified_root().unwrap();
        manager.shutdown().await.unwrap();
        let reopened = manager.state_authority("rspace").await.unwrap();
        reopened.initialize_for_records(&validator()).unwrap();
        assert_eq!(reopened.current_verified_root().unwrap(), before);
        let target_names = ["target-history", "target-roots", "target-cold"];
        let (_target_env, _) = scope_layout_database(
            &scratch.path().join("shared"),
            target_names[1],
            BTreeMap::from([(marker_key, marker)]),
        );
        let mut target = LmdbDirStoreManager::new(
            scratch.path().to_path_buf(),
            scope_layout_configuration("rspace", target_names, "split"),
        );
        let error = match target.state_authority("rspace").await {
            Ok(_) => panic!("foreign namespace marker was admitted"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("namespace"), "{error}");
        assert_eq!(authority.current_verified_root().unwrap(), before);
    }
}

#[tokio::test]
async fn scope_layout_scoped_migration_ignores_ambient_sources() {
    let scratch = test_scratch::acquire("rspace-scope-ambient-v2-");
    let (_env, _) = scope_layout_database(
        &scratch.path().join("rspace/casper/v2/roots"),
        "db",
        BTreeMap::from([(b"current-root".to_vec(), b"not a root".to_vec())]),
    );
    let mut manager = scope_layout_manager(scratch.path(), ["objects", "catalog", "payloads"]);
    let authority = manager.state_authority("rspace").await.unwrap();
    authority.initialize_for_records(&validator()).unwrap();
    assert_eq!(
        authority.current_root().unwrap(),
        crate::rspace::history::instances::radix_history::RadixHistory::empty_root_node_hash()
    );
    let layout =
        super::LmdbAuthorityLayout::new("rspace", ["h".to_owned(), "r".to_owned(), "c".to_owned()])
            .unwrap();
    let target = scratch.path().join("unopened");
    assert!(
        RSpaceStateAuthority::open_resolved_lmdb(
            &target,
            &layout,
            None,
            Some(&scratch.path().join("rspace/casper/v2")),
            16 * 1024 * 1024,
            32
        )
        .is_err()
    );
    assert!(!target.exists());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 32, failure_persistence: Some(Box::new(proptest::test_runner::FileFailurePersistence::WithSource("proptest-regressions"))), ..ProptestConfig::default() })]

    #[test]
    fn scope_layout_namespace_matches_framed_reference(prefix in "[a-zéλ_-]{0,24}", history in "[a-zéλ_-]{0,24}", roots in "[a-zéλ_-]{0,24}", cold in "[a-zéλ_-]{0,24}") {
        let fields = [prefix, format!("h:{history}"), format!("r:{roots}"), format!("c:{cold}")];
        let layout = super::LmdbAuthorityLayout::new(&fields[0], [fields[1].clone(), fields[2].clone(), fields[3].clone()]).unwrap();
        let mut rest = layout.namespace().strip_prefix(b"\0rspace-authority-layout\0\x01").unwrap();
        for field in &fields {
            let (length, remaining) = rest.split_at(8);
            let length = usize::try_from(u64::from_be_bytes(length.try_into().unwrap())).unwrap();
            let (value, tail) = remaining.split_at(length);
            prop_assert_eq!(value, field.as_bytes());
            rest = tail;
        }
        prop_assert!(rest.is_empty());
        for index in 0..4 {
            let mut changed = fields.clone();
            changed[index].push('!');
            let other = super::LmdbAuthorityLayout::new(&changed[0], [changed[1].clone(), changed[2].clone(), changed[3].clone()]).unwrap();
            prop_assert_ne!(layout.namespace(), other.namespace());
        }
        for prefix in ["rspace", "eval"] {
            let legacy = super::LmdbAuthorityLayout::for_prefix(prefix).unwrap();
            prop_assert_eq!(legacy.namespace(), prefix.as_bytes());
            prop_assert_ne!(legacy.namespace(), layout.namespace());
        }
    }

    #[test]
    fn scope_layout_generated_mutations_preserve_other_scopes(steps in prop::collection::vec((any::<bool>(), 1u8..128), 1..16)) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let scratch = test_scratch::acquire("rspace-scope-operations-");
            let mut first = scope_layout_manager(scratch.path(), ["a-history", "a-roots", "a-cold"]);
            let mut second = scope_layout_manager(scratch.path(), ["b-history", "b-roots", "b-cold"]);
            let first = first.state_authority("rspace").await.unwrap();
            let second = second.state_authority("rspace").await.unwrap();
            first.initialize_for_records(&validator()).unwrap();
            second.initialize_for_records(&validator()).unwrap();
            for (index, (choice, discriminator)) in steps.into_iter().enumerate() {
                let (target, other) = if choice { (&first, &second) } else { (&second, &first) };
                let before = (other.history_store().expect("legacy history facet").to_map().unwrap(), other.cold_store().expect("legacy cold facet").to_map().unwrap(), other.roots_store_for_inspection().expect("legacy roots facet").to_map().unwrap());
                let history = history_at_current(target);
                let (cold, bytes) = data_leaf(&[index as u8, discriminator]);
                let (_, plan) = prepare_insert(history.as_ref(), discriminator, &cold);
                target.commit_local_checkpoint(&plan, vec![(cold, bytes)], &validator()).unwrap();
                assert_eq!((other.history_store().expect("legacy history facet").to_map().unwrap(), other.cold_store().expect("legacy cold facet").to_map().unwrap(), other.roots_store_for_inspection().expect("legacy roots facet").to_map().unwrap()), before);
            }
        });
    }
}

fn scope_layout_manager(
    path: &std::path::Path,
    names: [&str; 3],
) -> impl crate::rspace::shared::key_value_store_manager::KeyValueStoreManager {
    use crate::rspace::shared::lmdb_dir_store_manager::{Db, LmdbDirStoreManager, LmdbEnvConfig};

    let config = LmdbEnvConfig::new("shared".to_owned(), 16 * 1024 * 1024).with_max_dbs(32);
    let mapping = ["history", "roots", "cold"]
        .into_iter()
        .zip(names)
        .map(|(role, name)| {
            (Db::new(format!("rspace-{role}"), Some(name.to_owned())), config.clone())
        })
        .collect();
    LmdbDirStoreManager::new(path.to_path_buf(), mapping)
}

#[tokio::test]
async fn scope_layout_disjoint_checkpoints_and_reopen() {
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    let scratch = test_scratch::acquire("rspace-authority-scopes-");
    let mut first_manager =
        scope_layout_manager(scratch.path(), ["a-history", "a-roots", "a-cold"]);
    let mut second_manager =
        scope_layout_manager(scratch.path(), ["objects", "catalog", "payloads"]);
    let first = first_manager.state_authority("rspace").await.unwrap();
    let second = second_manager.state_authority("rspace").await.unwrap();
    first.initialize_for_records(&validator()).unwrap();
    second.initialize_for_records(&validator()).unwrap();
    let first_history = history_at_current(&first);
    let second_history = history_at_current(&second);
    let (first_cold, first_bytes) = data_leaf(b"scope-first");
    let (second_cold, second_bytes) = data_leaf(b"scope-second");
    let (_, first_plan) = prepare_insert(first_history.as_ref(), 71, &first_cold);
    let (_, second_plan) = prepare_insert(second_history.as_ref(), 72, &second_cold);
    let second_before = second
        .roots_store_for_inspection()
        .expect("legacy roots facet")
        .to_map()
        .unwrap();
    let first_root = first
        .commit_local_checkpoint(&first_plan, vec![(first_cold.clone(), first_bytes)], &validator())
        .unwrap();
    assert_eq!(
        second
            .roots_store_for_inspection()
            .expect("legacy roots facet")
            .to_map()
            .unwrap(),
        second_before
    );
    let first_before = first
        .roots_store_for_inspection()
        .expect("legacy roots facet")
        .to_map()
        .unwrap();
    let second_root = second
        .commit_local_checkpoint(
            &second_plan,
            vec![(second_cold.clone(), second_bytes)],
            &validator(),
        )
        .unwrap();
    assert_eq!(
        first
            .roots_store_for_inspection()
            .expect("legacy roots facet")
            .to_map()
            .unwrap(),
        first_before
    );
    assert_ne!(first_root.root(), second_root.root());
    assert!(
        first
            .cold_store()
            .expect("legacy cold facet")
            .get_one(&second_cold.encode())
            .unwrap()
            .is_none()
    );
    assert!(
        second
            .cold_store()
            .expect("legacy cold facet")
            .get_one(&first_cold.encode())
            .unwrap()
            .is_none()
    );
    drop((first_plan, second_plan, first_history, second_history, first, second));
    first_manager.shutdown().await.unwrap();
    second_manager.shutdown().await.unwrap();
    let first = first_manager.state_authority("rspace").await.unwrap();
    let second = second_manager.state_authority("rspace").await.unwrap();
    first.initialize_for_records(&validator()).unwrap();
    second.initialize_for_records(&validator()).unwrap();
    assert_eq!(first.current_verified_root().unwrap(), Some(first_root));
    assert_eq!(second.current_verified_root().unwrap(), Some(second_root));
}

#[tokio::test]
async fn scope_layout_independent_names_and_reopen() {
    use crate::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    let scratch = test_scratch::acquire("rspace-authority-literal-names-");
    let names = ["objects", "catalog", "payloads"];
    let mut manager = scope_layout_manager(scratch.path(), names);
    let authority = manager.state_authority("rspace").await.unwrap();
    authority.initialize_for_records(&validator()).unwrap();
    assert!(Arc::ptr_eq(&authority, &manager.state_authority("rspace").await.unwrap()));
    let AuthorityBackend::Lmdb(state) = &authority.backend else {
        unreachable!()
    };
    let txn = state.env.read_txn().unwrap();
    for name in names {
        assert!(state.env.open_database::<heed::types::SerdeBincode<Vec<u8>>, heed::types::SerdeBincode<Vec<u8>>>(&txn, Some(name)).unwrap().is_some(), "missing literal database {name}");
    }
    for name in ["rspace-history", "rspace-roots", "rspace-cold"] {
        assert!(state.env.open_database::<heed::types::SerdeBincode<Vec<u8>>, heed::types::SerdeBincode<Vec<u8>>>(&txn, Some(name)).unwrap().is_none(), "unexpected unscoped database {name}");
    }
    drop(txn);
    for role in ["history", "roots", "cold"] {
        let store = manager.store(format!("rspace-{role}")).await.unwrap();
        assert!(
            store
                .put(vec![(b"not-authorized".to_vec(), vec![1])])
                .is_err()
        );
        assert!(store.delete(vec![b"not-authorized".to_vec()]).is_err());
        assert!(store.compare_and_swap_batch(&[], &[]).is_err());
    }
    let expected = authority.current_verified_root().unwrap();
    drop(authority);
    manager.shutdown().await.unwrap();
    let reopened = manager.state_authority("rspace").await.unwrap();
    reopened.initialize_for_records(&validator()).unwrap();
    assert_eq!(reopened.current_verified_root().unwrap(), expected);
}

fn data_leaf(label: &[u8]) -> (ColdLeafAddress, Vec<u8>) {
    let record = bincode::serialize(&label.to_vec()).expect("record");
    let payload = bincode::serialize(&vec![record]).expect("record sequence");
    let bytes = PersistedData::Data(DataLeaf { bytes: payload }).into_legacy_bincode();
    let view = PersistedDataView::parse_canonical(&bytes)
        .expect("canonical data envelope")
        .validate_records(PersistedDataKind::Data, Some(1))
        .expect("one canonical record");
    let digest = Blake2b256Hash::new(view.leaf_encoding());
    (ColdLeafAddress::new(PersistedDataKind::Data, digest).expect("typed address"), bytes)
}

fn history_key(discriminator: u8) -> Vec<u8> {
    let mut key = vec![0];
    key.extend(std::iter::repeat_n(discriminator, 32));
    key
}

fn history_at_current(authority: &Arc<RSpaceStateAuthority>) -> Box<dyn History> {
    Box::new(
        HistoryInstances::create_authorized(
            authority.current_root().expect("current root"),
            Arc::clone(authority),
        )
        .expect("authorized history"),
    )
}

fn prepare_insert(
    history: &dyn History,
    discriminator: u8,
    address: &ColdLeafAddress,
) -> (Box<dyn History>, PreparedHistoryCheckpoint) {
    history
        .prepare_checkpoint(vec![HistoryAction::Insert(InsertAction {
            key: history_key(discriminator),
            hash: address.digest().clone(),
        })])
        .expect("prepare checkpoint")
}

fn assert_root_objects_visible(
    authority: &Arc<RSpaceStateAuthority>,
    root: &CurrentRoot,
    prepared: &PreparedHistoryCheckpoint,
    cold: &ColdLeafAddress,
) {
    assert_eq!(authority.current_verified_root().unwrap(), Some(root.clone()));
    assert!(authority.contains_verified_root(root.root()).unwrap());
    assert_eq!(
        authority
            .history_store()
            .expect("legacy history facet")
            .get_one(&root.root().bytes())
            .unwrap()
            .as_deref(),
        prepared.root_encoding()
    );
    assert!(
        authority
            .cold_store()
            .expect("legacy cold facet")
            .get_one(&cold.encode())
            .unwrap()
            .is_some()
    );
}

#[test]
fn memory_checkpoint_publishes_all_namespaces_together() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"atomic-success");
    let (_next, prepared) = prepare_insert(history.as_ref(), 1, &cold);

    let current = authority
        .commit_local_checkpoint(&prepared, vec![(cold.clone(), bytes)], &validator())
        .expect("atomic checkpoint");

    assert_root_objects_visible(&authority, &current, &prepared, &cold);
}

#[test]
fn authority_facets_reject_deletion_and_authorized_history_rejects_detached_commit() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"immutable-authority");
    let (_next, prepared) = prepare_insert(history.as_ref(), 9, &cold);
    authority
        .commit_local_checkpoint(&prepared, vec![(cold.clone(), bytes)], &validator())
        .expect("seed atomic checkpoint");

    let deletion = authority
        .cold_store()
        .expect("legacy cold facet")
        .delete(vec![cold.encode()])
        .expect_err("authority-owned content cannot be deleted");
    assert!(deletion.to_string().contains("cannot be deleted"));

    let authorized = history_at_current(&authority);
    assert!(matches!(
        authorized.process(Vec::new()),
        Err(HistoryError::ActionError(message))
            if message.contains("atomic checkpoint authority")
    ));
}

#[test]
fn legacy_facet_mismatch_conversions_keep_their_typed_source() {
    for mismatch in [
        AuthorityBackendMismatch::History,
        AuthorityBackendMismatch::Cold,
        AuthorityBackendMismatch::Roots,
    ] {
        let message = mismatch.to_string();
        let history = HistoryError::from(mismatch);
        assert_eq!(history, HistoryError::BackendMismatch(mismatch));
        assert!(history.to_string().contains(&message));
        let repository = HistoryRepositoryError::from(mismatch);
        assert!(repository.to_string().contains(&message));
        assert!(matches!(
            repository,
            HistoryRepositoryError::HistoryError(HistoryError::BackendMismatch(source))
                if source == mismatch
        ));
        assert!(matches!(
            HistoryRepositoryError::from(history),
            HistoryRepositoryError::HistoryError(HistoryError::BackendMismatch(source))
                if source == mismatch
        ));
        let export = StateExportError::from(mismatch);
        assert_eq!(export, StateExportError::BackendMismatch(mismatch));
        assert!(export.to_string().contains(&message));
        let import = StateImportError::from(mismatch);
        assert_eq!(import, StateImportError::BackendMismatch(mismatch));
        assert!(import.to_string().contains(&message));
        let converted = heed::Error::from(mismatch);
        assert_eq!(converted.to_string(), message);
        let heed::Error::Io(io_error) = converted else {
            panic!("a backend mismatch must surface as heed::Error::Io");
        };
        assert_eq!(io_error.kind(), std::io::ErrorKind::Other);
        assert_eq!(
            io_error
                .get_ref()
                .and_then(|source| source.downcast_ref::<AuthorityBackendMismatch>()),
            Some(&mismatch)
        );
    }

    let scratch = test_scratch::acquire("rspace-authority-legacy-facets-");
    let lmdb =
        RSpaceStateAuthority::open_lmdb(scratch.path(), "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("open LMDB authority");
    lmdb.initialize_for_records(&validator())
        .expect("initialize LMDB authority");
    for authority in [RSpaceStateAuthority::in_memory(), lmdb] {
        let history = authority.history_store().expect("legacy history facet");
        let cold = authority.cold_store().expect("legacy cold facet");
        let roots = authority
            .roots_store_for_inspection()
            .expect("legacy roots facet");
        assert!(history.to_map().expect("history facet reads").is_empty());
        assert!(cold.to_map().expect("cold facet reads").is_empty());
        assert!(
            roots
                .get_one(&CURRENT_ROOT_KEY.to_vec())
                .expect("roots facet reads")
                .is_some()
        );
    }
}

#[test]
fn detached_history_rejects_a_missing_nonempty_root_without_panicking() {
    let missing = Blake2b256Hash::new(b"not-a-radix-root");
    let error = match HistoryInstances::create(missing, Arc::new(InMemoryKeyValueStore::new())) {
        Ok(_) => panic!("missing nonempty root was silently treated as empty"),
        Err(error) => error,
    };
    assert!(matches!(
        error,
        HistoryError::RadixTreeError(crate::rspace::errors::RadixTreeError::KeyNotFound(_))
    ));
}

#[test]
fn no_op_writes_nothing_and_does_not_advance_generation() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let before = memory_snapshot(&authority);
    let current_before = authority.current_verified_root().unwrap().unwrap();
    let (_next, prepared) = history.prepare_checkpoint(Vec::new()).expect("no-op plan");

    let observed = authority
        .commit_local_checkpoint(&prepared, Vec::new(), &validator())
        .expect("no-op checkpoint");

    assert_eq!(observed, current_before);
    assert_eq!(authority.current_verified_root().unwrap().unwrap(), current_before);
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn preparing_checkpoint_leaves_authority_byte_for_byte_unchanged() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, _bytes) = data_leaf(b"prepare-is-private");
    let before = memory_snapshot(&authority);
    let current_before = authority.current_verified_root().unwrap().unwrap();

    let (_next, _prepared) = prepare_insert(history.as_ref(), 14, &cold);

    assert_eq!(authority.current_verified_root().unwrap().unwrap(), current_before);
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn prepared_checkpoint_cannot_cross_an_authority_boundary() {
    let origin = RSpaceStateAuthority::in_memory();
    let foreign = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&origin);
    let (cold, bytes) = data_leaf(b"foreign-authority");
    let (_next, prepared) = prepare_insert(history.as_ref(), 12, &cold);
    let foreign_before = memory_snapshot(&foreign);

    let error = foreign
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect_err("a prepared checkpoint is authority-bound");

    assert!(matches!(
        error,
        HistoryError::ActionError(message) if message.contains("different RSpace authority")
    ));
    assert_eq!(memory_snapshot(&foreign), foreign_before);
}

#[test]
fn same_generation_plans_conflict_without_partial_visibility() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold_a, bytes_a) = data_leaf(b"winner");
    let (cold_b, bytes_b) = data_leaf(b"stale");
    let (_next_a, plan_a) = prepare_insert(history.as_ref(), 2, &cold_a);
    let (_next_b, plan_b) = prepare_insert(history.as_ref(), 3, &cold_b);
    authority
        .commit_local_checkpoint(&plan_a, vec![(cold_a, bytes_a)], &validator())
        .expect("winner commits");
    let before_stale = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(&plan_b, vec![(cold_b, bytes_b)], &validator())
        .expect_err("same-generation loser must conflict");

    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(memory_snapshot(&authority), before_stale);
}

#[test]
fn target_equal_replay_requires_reactivation_and_reuses_exact_object_closure() {
    let authority = RSpaceStateAuthority::in_memory();
    let initial = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"target-equal-record-replay");
    let (_play_history, play_plan) = prepare_insert(history.as_ref(), 15, &cold);
    let (_stale_replay_history, stale_replay_plan) = prepare_insert(history.as_ref(), 15, &cold);

    let play = authority
        .commit_local_checkpoint(&play_plan, vec![(cold.clone(), bytes.clone())], &validator())
        .expect("play checkpoint");
    assert_eq!(play_plan.target_root(), play.root());
    assert_eq!(stale_replay_plan.target_root(), play.root());
    let after_play = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(
            &stale_replay_plan,
            vec![(cold.clone(), bytes.clone())],
            &validator(),
        )
        .expect_err("target equality must not authorize a stale replay base");
    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(
        memory_snapshot(&authority),
        after_play,
        "RR-07: rejected skip-reset replay must preserve every authority namespace",
    );

    authority
        .activate_known(authority.known_verified_root(initial.root()).unwrap())
        .expect("reactivate the recorded base");
    let reset_snapshot = memory_snapshot(&authority);
    let fresh_history = history_at_current(&authority);
    let (_replay_history, replay_plan) = prepare_insert(fresh_history.as_ref(), 15, &cold);
    assert_eq!(
        memory_snapshot(&authority),
        reset_snapshot,
        "RR-10: fresh replay preparation must be byte-for-byte private",
    );
    let replay = authority
        .commit_local_checkpoint(&replay_plan, vec![(cold, bytes)], &validator())
        .expect("fresh replay checkpoint");
    assert_eq!(replay.root(), play.root());

    let mut after_replay = memory_snapshot(&authority);
    let mut play_closure = after_play;
    after_replay.roots.remove(CURRENT_ROOT_KEY);
    play_closure.roots.remove(CURRENT_ROOT_KEY);
    assert_eq!(
        after_replay, play_closure,
        "RR-12: replay must reuse the exact authenticated history/cold closure and verified-root \
         catalog",
    );
}

#[test]
fn aba_root_reactivation_does_not_revive_an_old_plan() {
    let authority = RSpaceStateAuthority::in_memory();
    let empty = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (stale_cold, stale_bytes) = data_leaf(b"stale-after-aba");
    let (_stale_next, stale_plan) = prepare_insert(history.as_ref(), 4, &stale_cold);
    let (winner_cold, winner_bytes) = data_leaf(b"aba-winner");
    let (_winner_next, winner_plan) = prepare_insert(history.as_ref(), 5, &winner_cold);
    authority
        .commit_local_checkpoint(&winner_plan, vec![(winner_cold, winner_bytes)], &validator())
        .expect("advance away from empty");
    authority
        .activate_known(authority.known_verified_root(empty.root()).unwrap())
        .expect("return to same root at newer generation");
    let before_stale = memory_snapshot(&authority);

    let error = authority
        .commit_local_checkpoint(&stale_plan, vec![(stale_cold, stale_bytes)], &validator())
        .expect_err("ABA must be rejected by generation");

    assert!(matches!(error, HistoryError::RootError(RootError::RootCatalogConflict { .. })));
    assert_eq!(memory_snapshot(&authority), before_stale);
}

#[test]
fn missing_or_malformed_cold_leaf_leaves_exact_pre_state() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, _bytes) = data_leaf(b"must-be-present");
    let (_next, prepared) = prepare_insert(history.as_ref(), 6, &cold);
    let before = memory_snapshot(&authority);

    authority
        .commit_local_checkpoint(&prepared, Vec::new(), &validator())
        .expect_err("missing cold closure must fail");
    assert_eq!(memory_snapshot(&authority), before);

    authority
        .commit_local_checkpoint(
            &prepared,
            vec![(cold, b"not-a-canonical-leaf".to_vec())],
            &validator(),
        )
        .expect_err("malformed cold object must fail");
    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn immutable_collision_leaves_exact_pre_state() {
    let authority = RSpaceStateAuthority::in_memory();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"collision");
    let (_next, prepared) = prepare_insert(history.as_ref(), 7, &cold);
    let AuthorityBackend::Memory(state) = &authority.backend else {
        unreachable!()
    };
    {
        let mut state = state.lock().unwrap();
        state
            .history
            .insert(prepared.target_root().bytes(), b"conflicting-object".to_vec());
    }
    let before = memory_snapshot(&authority);

    authority
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect_err("immutable history collision must fail");

    assert_eq!(memory_snapshot(&authority), before);
}

#[test]
fn same_key_same_value_is_idempotent_across_checkpoints() {
    let authority = RSpaceStateAuthority::in_memory();
    let first_history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"shared-object");
    let (second_history, first) = prepare_insert(first_history.as_ref(), 8, &cold);
    authority
        .commit_local_checkpoint(&first, vec![(cold.clone(), bytes.clone())], &validator())
        .expect("first checkpoint");
    let (_third_history, second) = prepare_insert(second_history.as_ref(), 9, &cold);

    authority
        .commit_local_checkpoint(&second, vec![(cold.clone(), bytes)], &validator())
        .expect("same-key/same-value cold object is idempotent");

    assert!(
        authority
            .cold_store()
            .expect("legacy cold facet")
            .get_one(&cold.encode())
            .unwrap()
            .is_some()
    );
}

#[test]
fn lmdb_success_and_stale_conflict_match_memory_atomicity() {
    let unique = format!("rspace-authority-test-{}", std::process::id());
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("create LMDB test directory");
    let authority =
        RSpaceStateAuthority::open_lmdb(&dir, "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("LMDB authority");
    authority
        .initialize_for_records(&validator())
        .expect("initialize LMDB authority");
    let history = history_at_current(&authority);
    let (winner_cold, winner_bytes) = data_leaf(b"lmdb-winner");
    let (loser_cold, loser_bytes) = data_leaf(b"lmdb-loser");
    let (_winner_next, winner) = prepare_insert(history.as_ref(), 10, &winner_cold);
    let (_loser_next, loser) = prepare_insert(history.as_ref(), 11, &loser_cold);
    let current = authority
        .commit_local_checkpoint(&winner, vec![(winner_cold.clone(), winner_bytes)], &validator())
        .expect("LMDB atomic checkpoint");
    assert_root_objects_visible(&authority, &current, &winner, &winner_cold);

    let history_before = authority
        .history_store()
        .expect("legacy history facet")
        .to_map()
        .unwrap();
    let cold_before = authority
        .cold_store()
        .expect("legacy cold facet")
        .to_map()
        .unwrap();
    let current_before = authority.current_verified_root().unwrap();
    authority
        .commit_local_checkpoint(&loser, vec![(loser_cold, loser_bytes)], &validator())
        .expect_err("LMDB stale plan conflicts");
    assert_eq!(
        authority
            .history_store()
            .expect("legacy history facet")
            .to_map()
            .unwrap(),
        history_before
    );
    assert_eq!(
        authority
            .cold_store()
            .expect("legacy cold facet")
            .to_map()
            .unwrap(),
        cold_before
    );
    assert_eq!(authority.current_verified_root().unwrap(), current_before);
}

#[test]
fn lmdb_reopen_preserves_exact_catalog_and_known_root_activation() {
    let scratch = test_scratch::acquire("rspace-authority-reopen-");
    let authority =
        RSpaceStateAuthority::open_lmdb(scratch.path(), "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("open first authority");
    authority
        .initialize_for_records(&validator())
        .expect("initialize first authority");
    let empty = authority.current_verified_root().unwrap().unwrap();
    let history = history_at_current(&authority);
    let (cold, bytes) = data_leaf(b"reopen-root");
    let (next_history, prepared) = prepare_insert(history.as_ref(), 13, &cold);
    let committed = authority
        .commit_local_checkpoint(&prepared, vec![(cold, bytes)], &validator())
        .expect("commit before reopen");
    drop(next_history);
    drop(prepared);
    drop(history);
    drop(authority);

    let reopened =
        RSpaceStateAuthority::open_lmdb(scratch.path(), "rspace", None, None, 16 * 1024 * 1024, 8)
            .expect("reopen authority");
    reopened
        .initialize_for_records(&validator())
        .expect("initialize reopened authority");
    assert_eq!(reopened.current_verified_root().unwrap(), Some(committed.clone()));
    assert!(reopened.contains_verified_root(committed.root()).unwrap());
    assert!(reopened.contains_verified_root(empty.root()).unwrap());

    let at_empty = reopened
        .activate_known(reopened.known_verified_root(empty.root()).unwrap())
        .expect("reset to the retained empty root");
    assert_eq!(at_empty.root(), empty.root());
    assert_eq!(at_empty.generation(), committed.generation() + 1);

    let restored = reopened
        .activate_known(reopened.known_verified_root(committed.root()).unwrap())
        .expect("reactivate the retained checkpoint root");
    assert_eq!(restored.root(), committed.root());
    assert_eq!(restored.generation(), at_empty.generation() + 1);
}

#[test]
fn lmdb_rejects_malformed_current_and_membership_records() {
    let current_scratch = test_scratch::acquire("rspace-authority-bad-current-");
    let current_authority = RSpaceStateAuthority::open_lmdb(
        current_scratch.path(),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .expect("open current-record authority");
    current_authority
        .initialize_for_records(&validator())
        .expect("initialize current-record authority");
    let AuthorityBackend::Lmdb(current_state) = &current_authority.backend else {
        unreachable!()
    };
    let mut txn = current_state
        .env
        .write_txn()
        .expect("corrupt current write");
    current_state
        .roots
        .put(&mut txn, &CURRENT_ROOT_KEY.to_vec(), &b"malformed-current".to_vec())
        .expect("write malformed current");
    txn.commit().expect("commit malformed current");
    assert!(current_authority.current_verified_root().is_err());

    let member_scratch = test_scratch::acquire("rspace-authority-bad-membership-");
    let member_authority = RSpaceStateAuthority::open_lmdb(
        member_scratch.path(),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .expect("open membership authority");
    member_authority
        .initialize_for_records(&validator())
        .expect("initialize membership authority");
    let current = member_authority.current_verified_root().unwrap().unwrap();
    let AuthorityBackend::Lmdb(member_state) = &member_authority.backend else {
        unreachable!()
    };
    let mut txn = member_state
        .env
        .write_txn()
        .expect("corrupt membership write");
    member_state
        .roots
        .put(&mut txn, &verified_root_key(current.root()), &b"malformed-membership".to_vec())
        .expect("write malformed membership");
    txn.commit().expect("commit malformed membership");
    assert!(
        member_authority
            .contains_verified_root(current.root())
            .is_err()
    );
}

type LegacyRepository = dyn crate::rspace::history::history_repository::HistoryRepository<String, String, String, String>
    + Send
    + Sync;
type LegacyAction = crate::rspace::hot_store_action::HotStoreAction<String, String, String, String>;
type LegacyPath = Vec<(Blake2b256Hash, Option<u8>)>;
type LegacyItems = Vec<(Blake2b256Hash, Vec<u8>)>;
type LegacyPage = (LegacyPath, LegacyPath, bool, LegacyItems, LegacyItems);

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
struct LegacyMigrationSnapshot {
    current: (Vec<u8>, u64),
    catalog: BTreeMap<Vec<u8>, Vec<u8>>,
    history: BTreeMap<Vec<u8>, Vec<u8>>,
    cold: BTreeMap<Vec<u8>, Vec<u8>>,
    pages: Vec<(Vec<u8>, i32, Vec<LegacyPage>)>,
}

fn legacy_fixture_actions() -> (Vec<LegacyAction>, Vec<LegacyAction>) {
    use crate::rspace::hot_store_action::{
        HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins,
    };
    use crate::rspace::internal::{Datum, WaitingContinuation};

    let channels = vec!["legacy-fixture-a".to_owned(), "legacy-fixture-b".to_owned()];
    let datum = Datum::create(&channels[0], "alpha".to_owned(), false);
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        true,
        std::collections::BTreeSet::from([0]),
    );
    let mut initial = vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: channels[0].clone(),
            data: vec![datum.clone(), datum],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: channels.clone(),
            continuations: vec![continuation],
        })),
    ];
    initial.extend(channels.iter().map(|channel| {
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channel.clone(),
            joins: vec![channels.clone()],
        }))
    }));
    let suffix = vec![HotStoreAction::Insert(InsertAction::InsertData(InsertData {
        channel: channels[1].clone(),
        data: vec![Datum::create(&channels[1], "beta".to_owned(), true)],
    }))];
    (initial, suffix)
}

fn legacy_fixture_repository(authority: Arc<RSpaceStateAuthority>) -> Box<LegacyRepository> {
    crate::rspace::history::history_repository::HistoryRepositoryInstances::<
        String,
        String,
        String,
        String,
    >::repository(authority)
    .unwrap()
}

fn seed_legacy_fixture(
    authority: Arc<RSpaceStateAuthority>,
) -> (Box<LegacyRepository>, [Blake2b256Hash; 3]) {
    let repository = legacy_fixture_repository(authority);
    let empty = repository.root();
    let (initial, suffix) = legacy_fixture_actions();
    let rollback = repository.checkpoint(initial).unwrap();
    let rollback_root = rollback.root();
    let latest = rollback.checkpoint(suffix).unwrap();
    let latest_root = latest.root();
    (latest, [empty, rollback_root, latest_root])
}

fn legacy_fixture_pages(
    repository: &LegacyRepository,
    root: &Blake2b256Hash,
    take: i32,
) -> Vec<LegacyPage> {
    use crate::rspace::state::exporters::rspace_exporter_items::RSpaceExporterItems;

    let exporter = repository.exporter();
    let mut path = vec![(root.clone(), None)];
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = Vec::new();
    loop {
        assert!(seen.insert(path.clone()), "legacy fixture cursor repeated before termination");
        let (history, cold) =
            RSpaceExporterItems::get_history_and_data(exporter.clone(), path.clone(), 0, take)
                .unwrap();
        assert_eq!(history.last_path, cold.last_path);
        let next = history.last_path;
        assert!(!next.is_empty());
        let terminal = next == path;
        pages.push((path, next.clone(), terminal, history.items, cold.items));
        if terminal {
            break;
        }
        assert_eq!(next.len(), 6);
        assert_eq!(next[0], (root.clone(), None));
        path = next;
    }
    pages
}

fn legacy_fixture_snapshot(
    authority: &Arc<RSpaceStateAuthority>,
    repository: &LegacyRepository,
    roots: &[Blake2b256Hash; 3],
) -> LegacyMigrationSnapshot {
    let current = authority.current_verified_root().unwrap().unwrap();
    let catalog: BTreeMap<_, _> = authority
        .roots_store_for_inspection()
        .expect("legacy roots facet")
        .to_map()
        .unwrap()
        .into_iter()
        .collect();
    let expected_members = roots
        .iter()
        .map(verified_root_key)
        .collect::<std::collections::BTreeSet<_>>();
    let actual_members = catalog
        .keys()
        .filter(|key| key.starts_with(super::VERIFIED_ROOT_KEY_PREFIX))
        .cloned()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual_members, expected_members);
    assert_eq!(catalog.len(), 4);
    for root in roots {
        assert!(authority.contains_verified_root(root).unwrap());
        assert_eq!(authority.known_verified_root(root).unwrap().root(), root);
    }
    assert_eq!(authority.current_verified_root().unwrap(), Some(current.clone()));
    let pages = roots
        .iter()
        .flat_map(|root| {
            [1, 2, 5].map(|take| (root.bytes(), take, legacy_fixture_pages(repository, root, take)))
        })
        .collect();
    LegacyMigrationSnapshot {
        current: (current.root().bytes(), current.generation()),
        catalog,
        history: authority
            .history_store()
            .expect("legacy history facet")
            .to_map()
            .unwrap()
            .into_iter()
            .collect(),
        cold: authority
            .cold_store()
            .expect("legacy cold facet")
            .to_map()
            .unwrap()
            .into_iter()
            .collect(),
        pages,
    }
}

fn legacy_fixture_digest(value: &impl serde::Serialize) -> String {
    hex::encode(Blake2b256Hash::new(&bincode::serialize(value).unwrap()).bytes())
}

fn assert_legacy_fixture_reads(repository: &LegacyRepository, roots: &[Blake2b256Hash; 3]) {
    use crate::rspace::internal::{Datum, WaitingContinuation};
    use crate::rspace::serializers::serializers::{
        encode_continuations, encode_datums, encode_joins,
    };

    let channels = vec!["legacy-fixture-a".to_owned(), "legacy-fixture-b".to_owned()];
    let datum = Datum::create(&channels[0], "alpha".to_owned(), false);
    let beta = Datum::create(&channels[1], "beta".to_owned(), true);
    let continuation = WaitingContinuation::create(
        &channels,
        &vec!["first".to_owned(), "second".to_owned()],
        &"body".to_owned(),
        true,
        std::collections::BTreeSet::from([0]),
    );
    for (index, root) in roots.iter().enumerate() {
        let reader = repository.get_history_reader(root).unwrap();
        let alpha = if index == 0 {
            vec![]
        } else {
            vec![datum.clone(), datum.clone()]
        };
        let beta = if index == 2 {
            vec![beta.clone()]
        } else {
            vec![]
        };
        let continuations = if index == 0 {
            vec![]
        } else {
            vec![continuation.clone()]
        };
        let joins = if index == 0 {
            vec![]
        } else {
            vec![channels.clone()]
        };
        assert_eq!(encode_datums(&reader.base().get_data(&channels[0])), encode_datums(&alpha));
        assert_eq!(encode_datums(&reader.base().get_data(&channels[1])), encode_datums(&beta));
        assert_eq!(
            encode_continuations(&reader.base().get_continuations(&channels)),
            encode_continuations(&continuations)
        );
        for channel in &channels {
            assert_eq!(encode_joins(&reader.base().get_joins(channel)), encode_joins(&joins));
        }
    }
}

#[test]
fn legacy_lmdb_retained_fixture_survives_process_exit_and_frozen_root_replay() {
    use crate::rspace::state::import_session::{
        StateImportPage, StateImportPublicationMode, StateImportPublicationWitness, StateImportSpec,
    };

    const CHILD_DIRECTORY: &str = "RSPACE_LEGACY_MIGRATION_FIXTURE_CHILD_DIRECTORY";
    const CHILD_FINISHED: i32 = 73;
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let authority = RSpaceStateAuthority::open_lmdb(
            std::path::Path::new(&directory),
            "rspace",
            None,
            None,
            16 * 1024 * 1024,
            8,
        )
        .unwrap();
        let (repository, roots) = seed_legacy_fixture(authority);
        assert_legacy_fixture_reads(repository.as_ref(), &roots);
        std::process::exit(CHILD_FINISHED);
    }

    let memory = RSpaceStateAuthority::in_memory();
    let (expected_repository, roots) = seed_legacy_fixture(memory.clone());
    assert_legacy_fixture_reads(expected_repository.as_ref(), &roots);
    let expected = legacy_fixture_snapshot(&memory, expected_repository.as_ref(), &roots);
    assert_eq!(expected.current, (roots[2].bytes(), 2));
    assert_ne!(roots[1], roots[2]);
    assert_eq!(
        roots
            .iter()
            .map(|root| hex::encode(root.bytes()))
            .collect::<Vec<_>>(),
        [
            "0e5751c026e543b2e8ab2eb06099daa1d1e5df47778f7787faab45cdf12fe3a8",
            "10842f8676716425a2fd9a60a4fd748b8225e4f6aa6c02578fc4fb79ded15c5d",
            "d430aa653eceb0c0df6e821ee8941bcb3f5e235584048a7279ebc48b49fe06b6",
        ]
    );
    for (observed, frozen) in [
        (
            legacy_fixture_digest(&expected.history),
            "96d2d98d20f9e5e7c77f0d826384360a6f7a8a02c03fed072106db7e9174eeb2",
        ),
        (
            legacy_fixture_digest(&expected.cold),
            "4cc917977696dcbc3c729117d3c85f69891e9244f2e0430614389bfdc6173c5c",
        ),
        (
            legacy_fixture_digest(&expected.catalog),
            "18957323e133547de68f8828cc9e7bc3c18612e6ef4f3795531e8d3152184097",
        ),
        (
            legacy_fixture_digest(&expected.pages),
            "c5bdca40adc5acc4d86117fb25309c16568265bef18441755189a536ddeaec12",
        ),
        (
            legacy_fixture_digest(&expected),
            "fafc11d0fb8accd3b1b3f3f2f26c07dc6aeb143aae9847dd502ef2f1a6f60afa",
        ),
    ] {
        assert_eq!(observed, frozen);
    }

    let scratch = test_scratch::acquire("rspace-legacy-migration-fixture-");
    let source = scratch.path().join("source");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("rspace::state::rspace_state_authority::atomic_state_authority_tests::legacy_lmdb_retained_fixture_survives_process_exit_and_frozen_root_replay")
        .arg("--nocapture")
        .env(CHILD_DIRECTORY, &source)
        .output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(CHILD_FINISHED),
        "writer did not finish its committed fixture: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let reopened =
        RSpaceStateAuthority::open_lmdb(&source, "rspace", None, None, 16 * 1024 * 1024, 8)
            .unwrap();
    let repository = legacy_fixture_repository(reopened.clone());
    assert_legacy_fixture_reads(repository.as_ref(), &roots);
    assert_eq!(legacy_fixture_snapshot(&reopened, repository.as_ref(), &roots), expected);

    let destination = RSpaceStateAuthority::open_lmdb(
        &scratch.path().join("rollback"),
        "rspace",
        None,
        None,
        16 * 1024 * 1024,
        8,
    )
    .unwrap();
    let rollback_repository = legacy_fixture_repository(destination.clone());
    let spec = StateImportSpec::new(
        [roots[1].clone()],
        StateImportPublicationMode::Activate(roots[1].clone()),
    )
    .unwrap();
    let mut import = rollback_repository.importer().begin_import(spec).unwrap();
    for (start, next, terminal, history_items, data_items) in
        legacy_fixture_pages(repository.as_ref(), &roots[1], 2)
    {
        let receipt = import
            .validate_and_stage_page(StateImportPage {
                start_path: start,
                advertised_next_path: next.clone(),
                skip: 0,
                take: 2,
                history_items,
                data_items,
            })
            .unwrap();
        assert_eq!(receipt.next_path, next);
        assert_eq!(receipt.terminal, terminal);
    }
    let publication = import
        .seal()
        .unwrap()
        .publish(&StateImportPublicationWitness::new())
        .unwrap();
    assert_eq!(publication.verified_roots, vec![roots[1].clone()]);
    assert_eq!(publication.activation.unwrap().root, roots[1]);
    assert!(!destination.contains_verified_root(&roots[2]).unwrap());
    let restored = legacy_fixture_repository(destination.clone());
    assert_eq!(restored.root(), roots[1]);
    let rollback_current = destination.current_verified_root().unwrap().unwrap();
    assert_eq!(rollback_current.root(), &roots[1]);
    assert!(
        restored
            .get_history_reader(&roots[1])
            .unwrap()
            .base()
            .get_data(&"legacy-fixture-b".to_owned())
            .is_empty()
    );
    let replayed = restored.checkpoint(legacy_fixture_actions().1).unwrap();
    assert_eq!(replayed.root(), roots[2]);
    let replayed_current = destination.current_verified_root().unwrap().unwrap();
    assert_eq!(replayed_current.root(), &roots[2]);
    assert_eq!(replayed_current.generation(), rollback_current.generation() + 1);
    assert_eq!(legacy_fixture_repository(destination.clone()).root(), roots[2]);
    assert_legacy_fixture_reads(replayed.as_ref(), &roots);
    for root in &roots {
        for take in [1, 2, 5] {
            assert_eq!(
                legacy_fixture_pages(replayed.as_ref(), root, take),
                legacy_fixture_pages(repository.as_ref(), root, take)
            );
        }
    }
    assert_eq!(legacy_fixture_snapshot(&reopened, repository.as_ref(), &roots), expected);
}
