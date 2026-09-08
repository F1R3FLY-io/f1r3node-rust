// See casper/src/main/scala/coop/rchain/casper/storage/RNodeKeyValueStoreManager.scala

use std::path::PathBuf;

use block_storage::rust::finality::FinalizationLedger;
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
            Db::new("finalization-certificates".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("block-metadata".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("dag-admission-schema".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("equivocation-tracker-v5".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new("equivocation-evidence-v5".to_string(), None),
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
            Db::new("deploy-occurrence-index".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            // DD-7b-2 (a) Option 2: payload-hash → deploy-sig index built
            // during block processing.  Opened in
            // `BlockDagKeyValueStorage::new` alongside the other indices;
            // the mapping must be registered here so the LMDB store
            // manager can honor `kvm.store("payload-source-index")`.
            // Post-cost-accounted-merge (2026-09-03): the fileio side's
            // registration got dropped during the merge; this entry
            // reinstates it.
            Db::new("payload-source-index".to_string(), None),
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
            Db::new("genesis-hash".to_string(), None),
            dag_storage_env_config(),
        ),
        (
            Db::new(FinalizationLedger::STORE_NAME.to_string(), None),
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
        (
            Db::new("deploy_envelope_storage_v6".to_string(), None),
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
mod tests {
    use super::*;

    /// Post-cost-accounted-merge regression pin (2026-09-03).  The
    /// initial fileio → cost-accounted-rho merge dropped the
    /// `payload-source-index` DB registration silently — the
    /// `BlockDagKeyValueStorage::new` initializer expects this
    /// store to be openable via `kvm.store("payload-source-index")`,
    /// but a three-way merge on `rnode_db_mapping()` favored the
    /// cost-accounted side (which lacked the entry) and dropped
    /// the fileio side's addition.  Every genesis boot in an LMDB-
    /// backed test then failed with:
    ///
    ///   KvStoreError(IoError("LMDB_Dir_Store_Manager: Key
    ///   payload-source-index was not found"))
    ///
    /// only surfacing at multi-node E2E tests (in-memory rspace
    /// tests never hit the DAG-store init path).  This source-scan
    /// pin catches a re-drop before it hits runtime by asserting
    /// the mapping contains the DB name.  Enumerated here rather
    /// than at the DAG-store init site because a mapping-drop
    /// regression can happen either by omitting the `Db::new(...)`
    /// line here OR by refactoring the DAG store's `kvm.store(...)`
    /// call — but the failure mode is symmetric (store lookup
    /// fails at boot), so a single pin against the mapping table
    /// is sufficient.
    #[test]
    fn rnode_db_mapping_includes_payload_source_index() {
        let mapping = rnode_db_mapping(None);
        assert!(
            mapping
                .iter()
                .any(|(db, _)| db.id() == "payload-source-index"),
            "payload-source-index LMDB DB is missing from rnode_db_mapping().  \
             BlockDagKeyValueStorage::new opens it via kvm.store(\"payload-\
             source-index\"); a missing mapping produces KvStoreError at \
             every LMDB-backed genesis boot.  See docs/consensus-invariants \
             or the 2026-09-03 cost-accounted-rho merge for context — the \
             mapping was silently dropped during that merge."
        );
    }

    /// Companion pin: the payload-source-index mapping MUST use
    /// `dag_storage_env_config()` (not a fresh env), so it lives in
    /// the same LMDB environment as the other DAG-store DBs.  A
    /// regression that put it in its own env would compile cleanly
    /// AND satisfy the "id present" pin above, but would silently
    /// double the LMDB file footprint and split the DAG-store's
    /// atomicity domain.
    #[test]
    fn rnode_db_mapping_payload_source_index_uses_dag_storage_env() {
        let mapping = rnode_db_mapping(None);
        let (_, cfg) = mapping
            .iter()
            .find(|(db, _)| db.id() == "payload-source-index")
            .expect("payload-source-index entry present (covered by sister pin)");
        let dag_cfg = dag_storage_env_config();
        assert_eq!(
            cfg.name, dag_cfg.name,
            "payload-source-index must colocate with the dag-storage env \
             (`{}`); got `{}`.  Splitting env would break atomic writes \
             across the DAG store's related indices.",
            dag_cfg.name, cfg.name
        );
    }

    /// M-38 fix (2026-09-08, A8-F5): full-mapping enumeration pin.
    ///
    /// Only `payload-source-index` had both a presence pin + env-
    /// colocation pin (the T-3 canary above).  The other 30+
    /// mappings relied on the LMDB store failing to open at first
    /// use — a silent removal during merge (as happened to
    /// `payload-source-index` in the cost-accounted-rho merge) is
    /// caught only when the affected code path runs, and E2E tests
    /// often miss that.
    ///
    /// This pin enumerates the full DB-name set (excluding rspace-*
    /// which vary by `legacy_rspace_paths` config) and asserts each
    /// is present.  A merge that silently drops any entry surfaces
    /// HERE at CI time, not at a downstream E2E boot.
    ///
    /// Adding a new DB entry is a two-line change: add to
    /// `rnode_db_mapping()` + add to `EXPECTED_DB_NAMES` below.
    /// Same discipline as the WalOp tag pin (M-37).
    ///
    /// Companion `..._env_colocation` pin walks each DB name and
    /// asserts its env matches the expected one — the same class
    /// of silent-hazard as the payload-source-index env-colocation
    /// pin, generalized.
    #[test]
    fn rnode_db_mapping_pins_full_dag_and_related_stores() {
        let mapping = rnode_db_mapping(None);
        let names: Vec<&str> = mapping.iter().map(|(db, _)| db.id()).collect();

        // Non-rspace core DB set.  Order does not matter for the
        // pin (we do subset containment); order in
        // `rnode_db_mapping()` is documentation-only.
        let expected_core: &[&str] = &[
            // Block storage
            "blocks",
            "blocks-approved",
            "finalization-certificates",
            "block-metadata",
            "dag-admission-schema",
            "equivocation-tracker-v5",
            "equivocation-evidence-v5",
            "latest-messages",
            "invalid-blocks",
            "deploy-index",
            "deploy-occurrence-index",
            "payload-source-index",
            "floor-index",
            "frontier-index",
            "deploy-lifecycle-events",
            "carrier-index",
            "carrier-index-meta",
            "deploy-lifecycle-terminal",
            "last-finalized-block",
            "genesis-hash",
            FinalizationLedger::STORE_NAME,
            "mergeable-channel-cache",
            // Deploy storage
            "deploy_storage",
            "deploy_envelope_storage_v6",
            "rejected_deploy_buffer",
            // Reporting
            "reporting-cache",
            // CasperBuffer
            "parents-map",
            // Rholang evaluator
            "eval-history",
            "eval-roots",
            "eval-cold",
            // Transaction
            "transaction",
        ];

        for name in expected_core {
            assert!(
                names.contains(name),
                "M-38: rnode_db_mapping() is missing expected DB `{name}`.  \
                 Either it was silently dropped (as happened to `payload-\
                 source-index` in the cost-accounted-rho merge — a runtime \
                 KvStoreError at LMDB-backed genesis boot), or the pin is \
                 stale (name changed).  Fix at the mapping, not here."
            );
        }

        // The non-legacy rspace-* triple is always present alongside
        // the core set.
        for name in &["rspace-history", "rspace-roots", "rspace-cold"] {
            assert!(
                names.contains(name),
                "M-38: rnode_db_mapping(None) is missing `{name}` — the \
                 non-legacy branch dropped an rspace mapping.  Silent removal \
                 breaks rspace history opening at runtime."
            );
        }

        // Count pin: bump when adding a new mapping.  A silent
        // addition wouldn't fire the containment checks, but a
        // silent removal WOULD leave the count wrong — this is the
        // symmetric backstop for both directions.
        const EXPECTED_MAPPING_COUNT_NONLEGACY: usize = 34;
        assert_eq!(
            mapping.len(),
            EXPECTED_MAPPING_COUNT_NONLEGACY,
            "M-38: rnode_db_mapping(None) count drifted.  Either add the new \
             entry to `expected_core` above and bump this count, or find the \
             silent-removed entry.  Current count {}, expected {}.",
            mapping.len(),
            EXPECTED_MAPPING_COUNT_NONLEGACY
        );
    }

    /// M-38 companion (2026-09-08, A8-F5): legacy-rspace branch pin.
    /// Independent of the non-legacy pin because the two branches
    /// diverge in the rspace-* group (legacy has 4: history, roots,
    /// cold, channels; non-legacy has 3: history, roots, cold).  A
    /// silent removal of `rspace-channels` from the legacy branch
    /// would only fire on legacy-config genesis boot — this pin
    /// catches it at CI.
    #[test]
    fn rnode_db_mapping_pins_legacy_rspace_branch() {
        let mapping = rnode_db_mapping(Some(true));
        let names: Vec<&str> = mapping.iter().map(|(db, _)| db.id()).collect();
        for name in &[
            "rspace-history",
            "rspace-roots",
            "rspace-cold",
            "rspace-channels",
        ] {
            assert!(
                names.contains(name),
                "M-38: legacy rnode_db_mapping(Some(true)) is missing \
                 `{name}` — required for legacy-config genesis boot."
            );
        }
        const EXPECTED_MAPPING_COUNT_LEGACY: usize = 35;
        assert_eq!(
            mapping.len(),
            EXPECTED_MAPPING_COUNT_LEGACY,
            "M-38: rnode_db_mapping(Some(true)) count drifted from {}",
            EXPECTED_MAPPING_COUNT_LEGACY
        );
    }
}
