use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use prost::Message;

use super::super::manifest::ChainManifest;

#[derive(Clone)]
pub struct ManifestGuard {
    directory: PathBuf,
    network: String,
    shard: String,
    existing: Option<ChainManifest>,
}

impl ManifestGuard {
    pub fn inspect(directory: &Path, network: &str, shard: &str) -> eyre::Result<Self> {
        let path = directory.join("consensus-manifest.json");
        let existing: Option<ChainManifest> = match std::fs::read(&path) {
            Ok(bytes) => Some(serde_json::from_slice(&bytes)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        if let Some(manifest) = &existing {
            eyre::ensure!(
                manifest.protocol == "cbc-casper",
                "Consensus protocol mismatch: {}",
                manifest.protocol
            );
            eyre::ensure!(
                manifest.protocol_version == 1 && manifest.schema_version == 1,
                "Unsupported consensus manifest version"
            );
            eyre::ensure!(
                manifest.network == network && manifest.shard == shard,
                "Consensus chain mismatch"
            );
            eyre::ensure!(
                !manifest.genesis.is_empty(),
                "Consensus manifest has no genesis identity"
            );
        } else if directory.exists() {
            for entry in std::fs::read_dir(directory)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    let known = matches!(
                        name.as_ref(),
                        "blockstorage"
                            | "dagstorage"
                            | "deploystorage"
                            | "casperbuffer"
                            | "reporting"
                            | "transaction"
                            | "rspace"
                            | "eval"
                    );
                    eyre::ensure!(
                        known || !contains_database(&entry.path())?,
                        "Unidentified store directory: {name}"
                    );
                } else if entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "mdb")
                {
                    eyre::bail!(
                        "Unidentified consensus database: {}",
                        entry.path().display()
                    );
                }
            }
        }
        let guard = Self {
            directory: directory.to_path_buf(),
            network: network.into(),
            shard: shard.into(),
            existing,
        };
        guard.inspect_native_store()?;
        Ok(guard)
    }

    fn inspect_native_store(&self) -> eyre::Result<()> {
        let directory = self.directory.join("dagstorage");
        if !directory.join("data.mdb").exists() {
            eyre::ensure!(
                self.existing.is_none(),
                "Manifest exists without a Casper store"
            );
            eyre::ensure!(
                !self.directory.join("blockstorage/data.mdb").exists(),
                "Incomplete Casper store identity"
            );
            return Ok(());
        }
        let env = open_readonly(&directory)?;
        let txn = env.read_txn()?;
        let approved: Option<
            heed::Database<heed::types::SerdeBincode<Vec<u8>>, heed::types::SerdeBincode<Vec<u8>>>,
        > = env.open_database(&txn, Some("blocks-approved"))?;
        let approved = approved
            .map(|db| db.get(&txn, &vec![42]))
            .transpose()?
            .flatten();
        match approved {
            Some(bytes) => {
                let approved =
                    models::rust::casper::protocol::casper_message::ApprovedBlock::from_proto(
                        models::casper::ApprovedBlockProto::decode(bytes.as_slice())?,
                    )
                    .map_err(|error| eyre::eyre!(error))?;
                let block = approved.candidate.block;
                eyre::ensure!(block.shard_id == self.shard, "Stored Casper shard mismatch");
                let genesis_db: Option<
                    heed::Database<
                        heed::types::SerdeBincode<Vec<u8>>,
                        heed::types::SerdeBincode<Vec<u8>>,
                    >,
                > = env.open_database(&txn, Some("genesis-hash"))?;
                let key = bincode::serialize("genesis")?;
                let genesis = genesis_db
                    .map(|db| db.get(&txn, &key))
                    .transpose()?
                    .flatten();
                if let Some(bytes) = genesis {
                    let hash: models::rust::block_hash::BlockHashSerde =
                        bincode::deserialize(&bytes)?;
                    self.verify_genesis(&hash.0)?;
                } else {
                    eyre::ensure!(
                        block.body.state.block_number == 0,
                        "Truncated Casper store has no genesis identity"
                    );
                    self.verify_genesis(&block.block_hash)?;
                }
            }
            None => {
                eyre::ensure!(
                    self.existing.is_none(),
                    "Manifest exists without an approved Casper block"
                );
                let blocks = self.directory.join("blockstorage");
                if blocks.join("data.mdb").exists() {
                    let blocks_env = open_readonly(&blocks)?;
                    let blocks_txn = blocks_env.read_txn()?;
                    let db: Option<
                        heed::Database<
                            heed::types::SerdeBincode<Vec<u8>>,
                            heed::types::SerdeBincode<Vec<u8>>,
                        >,
                    > = blocks_env.open_database(&blocks_txn, Some("blocks"))?;
                    eyre::ensure!(
                        db.map(|db| db.is_empty(&blocks_txn))
                            .transpose()?
                            .unwrap_or(true),
                        "Nonempty Casper store has no approved block identity"
                    );
                }
            }
        }
        Ok(())
    }

    pub fn verify_genesis(&self, genesis: &[u8]) -> eyre::Result<()> {
        if let Some(manifest) = &self.existing {
            eyre::ensure!(
                manifest.genesis == hex::encode(genesis),
                "Consensus genesis mismatch"
            );
        }
        Ok(())
    }

    pub fn record(&self, genesis: &[u8]) -> eyre::Result<()> {
        eyre::ensure!(!genesis.is_empty(), "Cannot record an unidentified genesis");
        self.verify_genesis(genesis)?;
        if self.existing.is_some() {
            return Ok(());
        }
        let manifest = ChainManifest {
            protocol: "cbc-casper".into(),
            protocol_version: 1,
            schema_version: 1,
            network: self.network.clone(),
            shard: self.shard.clone(),
            genesis: hex::encode(genesis),
        };
        let bytes = serde_json::to_vec_pretty(&manifest)?;
        let temporary = self
            .directory
            .join(format!(".consensus-manifest-{}.tmp", uuid::Uuid::new_v4()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        let _temporary_guard = TemporaryManifest(temporary.clone());
        file.write_all(&bytes)?;
        file.sync_all()?;
        let destination = self.directory.join("consensus-manifest.json");
        match std::fs::hard_link(&temporary, &destination) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let existing: ChainManifest =
                    serde_json::from_slice(&std::fs::read(&destination)?)?;
                eyre::ensure!(
                    existing == manifest,
                    "Concurrent consensus manifest mismatch"
                );
            }
            Err(error) => return Err(error.into()),
        }
        std::fs::remove_file(temporary)?;
        File::open(&self.directory)?.sync_all()?;
        Ok(())
    }
}

struct TemporaryManifest(PathBuf);

impl Drop for TemporaryManifest {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

fn open_readonly(path: &Path) -> eyre::Result<heed::Env> {
    let mut options = heed::EnvOpenOptions::new();
    options.max_dbs(128);
    unsafe {
        options.flags(heed::EnvFlags::READ_ONLY);
        Ok(options.open(path)?)
    }
}

fn contains_database(path: &Path) -> eyre::Result<bool> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_symlink() {
            continue;
        }
        if entry.file_type()?.is_dir() {
            if contains_database(&entry.path())? {
                return Ok(true);
            }
        } else if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "mdb")
        {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Barrier};

    use models::rust::casper::protocol::casper_message::{ApprovedBlock, ApprovedBlockCandidate};

    use super::*;

    fn publish_concurrently(genesis: [Vec<u8>; 2]) -> (tempfile::TempDir, Vec<eyre::Result<()>>) {
        let directory = tempfile::tempdir().unwrap();
        let guard = ManifestGuard::inspect(directory.path(), "test", "root").unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = genesis
                .into_iter()
                .map(|hash| {
                    let guard = guard.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        barrier.wait();
                        guard.record(&hash)
                    })
                })
                .collect();
            tasks.into_iter().map(|task| task.join().unwrap()).collect()
        });
        let entries: Vec<_> = std::fs::read_dir(directory.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(entries, vec![std::ffi::OsString::from(
            "consensus-manifest.json"
        )]);
        (directory, results)
    }

    #[test]
    fn concurrent_identical_publications_succeed() {
        let (directory, results) = publish_concurrently([vec![1; 32], vec![1; 32]]);
        assert!(results.iter().all(Result::is_ok));
        let manifest: ChainManifest = serde_json::from_slice(
            &std::fs::read(directory.path().join("consensus-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.genesis, hex::encode([1; 32]));
    }

    #[test]
    fn concurrent_conflicting_publications_preserve_the_winner() {
        let (directory, results) = publish_concurrently([vec![1; 32], vec![2; 32]]);
        assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
        let winner = results.iter().position(Result::is_ok).unwrap() as u8 + 1;
        let error = results.into_iter().find_map(Result::err).unwrap();
        assert!(error
            .to_string()
            .contains("Concurrent consensus manifest mismatch"));
        let manifest: ChainManifest = serde_json::from_slice(
            &std::fs::read(directory.path().join("consensus-manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.genesis, hex::encode([winner; 32]));
    }

    fn legacy_store(directory: &Path, height: i64, genesis: Option<&[u8]>) {
        let dag = directory.join("dagstorage");
        std::fs::create_dir_all(&dag).unwrap();
        let env = unsafe { heed::EnvOpenOptions::new().max_dbs(128).open(&dag).unwrap() };
        let mut txn = env.write_txn().unwrap();
        let approved_db: heed::Database<
            heed::types::SerdeBincode<Vec<u8>>,
            heed::types::SerdeBincode<Vec<u8>>,
        > = env
            .create_database(&mut txn, Some("blocks-approved"))
            .unwrap();
        let mut block = models::rust::block_implicits::get_random_block_default();
        block.body.state.block_number = height;
        block.shard_id = "root".into();
        block.block_hash = vec![2; 32].into();
        let approved = ApprovedBlock {
            candidate: ApprovedBlockCandidate {
                block,
                required_sigs: 0,
            },
            sigs: vec![],
            floor_seed: None,
        };
        approved_db
            .put(&mut txn, &vec![42], &approved.to_proto().encode_to_vec())
            .unwrap();
        if let Some(genesis) = genesis {
            let db: heed::Database<
                heed::types::SerdeBincode<Vec<u8>>,
                heed::types::SerdeBincode<Vec<u8>>,
            > = env.create_database(&mut txn, Some("genesis-hash")).unwrap();
            let hash = models::rust::block_hash::BlockHashSerde(genesis.to_vec().into());
            db.put(
                &mut txn,
                &bincode::serialize("genesis").unwrap(),
                &bincode::serialize(&hash).unwrap(),
            )
            .unwrap();
        }
        txn.commit().unwrap();
        env.force_sync().unwrap();
        env.prepare_for_closing().wait();
    }

    #[test]
    fn legacy_genesis_block_without_genesis_entry_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        legacy_store(directory.path(), 0, None);
        let before = std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap();
        ManifestGuard::inspect(directory.path(), "test", "root").unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap(),
            before
        );
    }

    #[test]
    fn legacy_checkpoint_without_genesis_identity_is_rejected_without_writes() {
        let directory = tempfile::tempdir().unwrap();
        legacy_store(directory.path(), 10, None);
        let before = std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap();
        let error = ManifestGuard::inspect(directory.path(), "test", "root")
            .err()
            .unwrap();
        assert!(error
            .to_string()
            .contains("Truncated Casper store has no genesis identity"));
        assert_eq!(
            std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap(),
            before
        );
        assert!(!directory.path().join("consensus-manifest.json").exists());
    }

    #[test]
    fn legacy_checkpoint_with_genesis_identity_is_accepted() {
        let directory = tempfile::tempdir().unwrap();
        legacy_store(directory.path(), 10, Some(&[1; 32]));
        let before = std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap();
        let guard = ManifestGuard::inspect(directory.path(), "test", "root").unwrap();
        guard.record(&[1; 32]).unwrap();
        ManifestGuard::inspect(directory.path(), "test", "root").unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("dagstorage/data.mdb")).unwrap(),
            before
        );
    }
}
