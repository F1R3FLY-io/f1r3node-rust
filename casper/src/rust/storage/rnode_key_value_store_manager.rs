// See casper/src/main/scala/coop/rchain/casper/storage/RNodeKeyValueStoreManager.scala

use std::path::PathBuf;

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

// Database name to store instance name mapping (sub-folder for LMDB store)
// - keys with the same instance will be in one LMDB file (environment)
pub fn rnode_db_mapping(_legacy_rspace_paths: Option<bool>) -> Vec<(Db, LmdbEnvConfig)> {
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
            Db::new("deploy-index".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("last-finalized-block".to_string(), None),
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
        // (Signed<DeployData>) and it is bounded by `deployLifespan`.
        (
            Db::new("rejected_deploy_buffer".to_string(), None),
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

    // The live destination is canonical regardless of the compatibility flag.
    // Legacy v2 environments are read-only migration inputs, never live stores.
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

    mappings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rspace_layout(legacy: Option<bool>) -> Vec<(String, String)> {
        rnode_db_mapping(legacy)
            .into_iter()
            .filter(|(db, _)| matches!(db.id(), "rspace-history" | "rspace-roots" | "rspace-cold"))
            .map(|(db, env)| (db.id().to_string(), env.name))
            .collect()
    }

    #[test]
    fn legacy_flag_never_changes_the_canonical_rspace_destination() {
        let expected = rspace_layout(None);
        assert_eq!(rspace_layout(Some(false)), expected);
        assert_eq!(rspace_layout(Some(true)), expected);
        assert!(expected.contains(&("rspace-history".to_string(), "rspace/history".to_string())));
        assert!(expected.contains(&("rspace-roots".to_string(), "rspace/history".to_string())));
        assert!(expected.contains(&("rspace-cold".to_string(), "rspace/cold".to_string())));
    }
}
