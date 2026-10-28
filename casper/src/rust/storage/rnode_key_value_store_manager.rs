// See casper/src/main/scala/coop/rchain/casper/storage/RNodeKeyValueStoreManager.scala

use std::path::PathBuf;

use block_storage::rust::deploy::key_value_deploy_storage::{
    DEPLOY_STORAGE_QUARANTINE_DB, REJECTED_DEPLOY_BUFFER_QUARANTINE_DB,
};
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::shared::lmdb_dir_store_manager::{
    Db, LmdbDirStoreManager, LmdbEnvConfig, GB, TB,
};

pub fn new_key_value_store_manager(
    dir_path: PathBuf,
    legacy_rspace_paths: Option<bool>,
) -> impl KeyValueStoreManager {
    LmdbDirStoreManager::new(
        dir_path,
        rnode_db_mapping(legacy_rspace_paths).into_iter().collect(),
    )
}

// Config name is used as a sub-folder for LMDB files

// RSpace
fn rspace_history_env_config() -> LmdbEnvConfig {
    LmdbEnvConfig::new("rspace/history".to_string(), TB)
}

fn rspace_cold_env_config() -> LmdbEnvConfig { LmdbEnvConfig::new("rspace/cold".to_string(), TB) }

// RSpace evaluator
fn eval_history_env_config() -> LmdbEnvConfig { LmdbEnvConfig::new("eval/history".to_string(), TB) }

fn eval_cold_env_config() -> LmdbEnvConfig { LmdbEnvConfig::new("eval/cold".to_string(), TB) }

// Blocks
fn block_storage_env_config() -> LmdbEnvConfig {
    LmdbEnvConfig::new("blockstorage".to_string(), TB)
}

fn dag_storage_env_config() -> LmdbEnvConfig {
    LmdbEnvConfig::new("dagstorage".to_string(), 100 * GB)
}

fn deploy_storage_env_config() -> LmdbEnvConfig {
    LmdbEnvConfig::new("deploystorage".to_string(), GB)
}

// Temporary storage / cache
fn casper_buffer_env_config() -> LmdbEnvConfig {
    LmdbEnvConfig::new("casperbuffer".to_string(), GB)
}

fn reporting_env_config() -> LmdbEnvConfig { LmdbEnvConfig::new("reporting".to_string(), 10 * TB) }

fn transaction_env_config() -> LmdbEnvConfig { LmdbEnvConfig::new("transaction".to_string(), GB) }

// Legacy RSpace paths
fn legacy_env_config(dir: &str) -> LmdbEnvConfig {
    LmdbEnvConfig::new(format!("{}/{}", "rspace/casper/v2", dir), TB)
}

// Database name to store instance name mapping (sub-folder for LMDB store)
// - keys with the same instance will be in one LMDB file (environment)
pub fn rnode_db_mapping(legacy_rspace_paths: Option<bool>) -> Vec<(Db, LmdbEnvConfig)> {
    let legacy_rspace_paths = legacy_rspace_paths.unwrap_or(false);

    let mut mappings = vec![
        // Block storage
        (
            Db::new("blocks".to_string(), None),
            block_storage_env_config(),
        ),
        // Block metadata storage
        (
            Db::new("blocks-approved".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("block-metadata".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("block-metadata-schema".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("equivocation-tracker".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("latest-messages".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("invalid-blocks".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("floor-index".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // Per-block finalized frontier F(X) (the warm up-walk pivot). Must be
            // registered here so the production LMDB store manager can open it in
            // BlockDagKeyValueStorage::new (mirrors "floor-index").
            Db::new("frontier-index".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // Deploy-lifecycle event rows (per-sig body projection, bounded to
            // open sigs — pruned at the terminal write). Opened in
            // BlockDagKeyValueStorage::new like the indices above.
            Db::new("deploy-lifecycle-events".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // Repeat-deploy carrier index: per-sig carrier records over valid,
            // invalid, and settled blocks, in a dedicated store (no shared
            // keyspace with wire-keyed rows). Opened in
            // BlockDagKeyValueStorage::new (mirrors "floor-index").
            Db::new("carrier-index".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // Carrier-index metadata: the write-once engagement watermark and
            // the prune stride cursor.
            Db::new("carrier-index-meta".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // WRITE-ONCE terminal deploy verdicts (Finalized/Expired/Failed),
            // written by the finality layer's lifecycle register.
            Db::new("deploy-lifecycle-terminal".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("last-finalized-block".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // Single-slot register for the shard's genesis hash (learned during
            // a truncated restore). Opened in BlockDagKeyValueStorage::new like
            // the indices above.
            Db::new("genesis-hash".to_string(), None),
            dag_storage_env_config(),
        ),
        // Runtime mergeable store (cache of mergeable channels for block-merge)
        (
            Db::new("mergeable-channel-cache".to_string(), None),
            dag_storage_env_config(),
        ),
        // Deploy storage
        (
            Db::new("deploy_storage".to_string(), None),
            deploy_storage_env_config(),
        ),
        // Buffer of deploys rejected during multi-parent merge; shares sizing
        // with deploy_storage since its entries are the same value type
        // (Signed<DeployData>) and it is bounded by `deploy_lifespan`.
        (
            Db::new("rejected_deploy_buffer".to_string(), None),
            deploy_storage_env_config(),
        ),
        // Undecodable records moved out of deploy_storage and
        // rejected_deploy_buffer by a scan, kept byte-for-byte for diagnosis.
        (
            Db::new(DEPLOY_STORAGE_QUARANTINE_DB.to_string(), None),
            deploy_storage_env_config(),
        ),
        (
            Db::new(REJECTED_DEPLOY_BUFFER_QUARANTINE_DB.to_string(), None),
            deploy_storage_env_config(),
        ),
        // Reporting (trace) cache
        (
            Db::new("reporting-cache".to_string(), None),
            reporting_env_config(),
        ),
        // CasperBuffer
        (
            Db::new("parents-map".to_string(), None),
            casper_buffer_env_config(),
        ),
        // Rholang evaluator store
        (
            Db::new("eval-history".to_string(), None),
            eval_history_env_config(),
        ),
        (
            Db::new("eval-roots".to_string(), None),
            eval_history_env_config(),
        ),
        (
            Db::new("eval-cold".to_string(), None),
            eval_cold_env_config(),
        ),
        // Transaction store
        (
            Db::new("transaction".to_string(), None),
            transaction_env_config(),
        ),
    ];

    // RSpace
    if !legacy_rspace_paths {
        // History and roots maps are part of the same LMDB file (environment)
        mappings.push((
            Db::new("rspace-history".to_string(), None),
            rspace_history_env_config(),
        ));
        mappings.push((
            Db::new("rspace-roots".to_string(), None),
            rspace_history_env_config(),
        ));
        mappings.push((
            Db::new("rspace-cold".to_string(), None),
            rspace_cold_env_config(),
        ));
    } else {
        // Legacy config has the same database name for all maps
        mappings.push((
            Db::new("rspace-history".to_string(), Some("db".to_string())),
            legacy_env_config("history"),
        ));
        mappings.push((
            Db::new("rspace-roots".to_string(), Some("db".to_string())),
            legacy_env_config("roots"),
        ));
        mappings.push((
            Db::new("rspace-cold".to_string(), Some("db".to_string())),
            legacy_env_config("cold"),
        ));
        mappings.push((
            Db::new("rspace-channels".to_string(), Some("db".to_string())),
            legacy_env_config("channels"),
        ));
    }

    mappings
}

#[cfg(test)]
mod bigint_metadata_tests {
    use block_storage::rust::dag::block_dag_key_value_storage::{
        BlockDagKeyValueStorage, InsertMode,
    };
    use models::rust::block_implicits::get_random_block_default;

    use super::*;

    #[tokio::test]
    async fn production_schema_checks_every_registered_store_before_stamping() {
        for (db, _) in rnode_db_mapping(None) {
            let directory = tempfile::tempdir().unwrap();
            let mut manager = new_key_value_store_manager(directory.path().to_path_buf(), None);
            let name = db.id().to_string();
            manager
                .store(name.clone())
                .await
                .unwrap()
                .put_one(b"legacy-key".to_vec(), b"legacy-value".to_vec())
                .unwrap();
            manager.shutdown().await.unwrap();
            drop(manager);
            let mut manager = new_key_value_store_manager(directory.path().to_path_buf(), None);
            let result = BlockDagKeyValueStorage::ensure_metadata_schema(&mut manager).await;
            assert!(
                matches!(result,
                Err(shared::rust::store::key_value_store::KvStoreError::InvalidArgument(message))
                    if message.contains("Use a fresh data directory")),
                "{name}"
            );
            assert!(
                manager
                    .store("block-metadata-schema".to_string())
                    .await
                    .unwrap()
                    .get_one(&b"version".to_vec())
                    .unwrap()
                    .is_none(),
                "{name}"
            );
            assert_eq!(
                manager
                    .store(name.clone())
                    .await
                    .unwrap()
                    .get_one(&b"legacy-key".to_vec())
                    .unwrap(),
                Some(b"legacy-value".to_vec()),
                "{name}"
            );
        }
    }

    #[tokio::test]
    async fn production_lmdb_mapping_supports_bigint_restart_and_legacy_refusal() {
        let directory = tempfile::tempdir().unwrap();
        let mut manager = new_key_value_store_manager(directory.path().to_path_buf(), None);
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        let mut block = get_random_block_default();
        block.header.parents_hash_list.clear();
        block.body.state.block_number = 0;
        block.body.state.bonds[0].stake = "100000000000000000000".parse().unwrap();
        dag.insert(&block, InsertMode::Approved).unwrap();
        drop(dag);
        manager.shutdown().await.unwrap();
        drop(manager);
        let mut manager = new_key_value_store_manager(directory.path().to_path_buf(), None);
        let dag = BlockDagKeyValueStorage::new(&mut manager).await.unwrap();
        assert_eq!(
            dag.get_representation()
                .unwrap()
                .lookup(&block.block_hash)
                .unwrap()
                .unwrap()
                .weight_map[&block.body.state.bonds[0].validator],
            block.body.state.bonds[0].stake
        );
        drop(dag);
        let metadata = manager.store("block-metadata".to_string()).await.unwrap();
        let before = metadata.to_map().unwrap();
        manager
            .store("block-metadata-schema".to_string())
            .await
            .unwrap()
            .delete(vec![b"version".to_vec()])
            .unwrap();
        assert!(
            BlockDagKeyValueStorage::ensure_metadata_schema(&mut manager)
                .await
                .is_err()
        );
        assert_eq!(metadata.to_map().unwrap(), before);
    }
}
