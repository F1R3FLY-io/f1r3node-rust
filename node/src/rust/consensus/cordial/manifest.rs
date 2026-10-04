use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cordial_consensus::{Chain, ChainSpec};
use cordial_rholang::GenesisSettings;
use fs2::FileExt;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    pub chain: ChainSpec,
    pub genesis: GenesisSettings,
}

impl ChainConfig {
    pub fn read(path: &Path) -> eyre::Result<Self> {
        let bytes = bounded_read(path)?;
        let mut config: Self = serde_json::from_slice(&bytes)?;
        config.chain = Chain::new(config.chain)?.spec().clone();
        eyre::ensure!(
            config.chain.execution_genesis != [0; 32],
            "Pin the Cordial execution genesis before node startup"
        );
        Ok(config)
    }
}

#[derive(Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    protocol: String,
    protocol_version: u32,
    schema_version: u32,
    config: ChainConfig,
}

pub struct DirectoryGuard {
    _lease: File,
}

impl DirectoryGuard {
    pub fn open(path: &Path, config: &ChainConfig) -> eyre::Result<Self> {
        eyre::ensure!(
            config.chain.execution_genesis != [0; 32],
            "Unpinned Cordial genesis"
        );
        let canonical = Chain::new(config.chain.clone())?;
        eyre::ensure!(
            canonical.spec() == &config.chain,
            "Cordial committee must be canonical"
        );
        if path.exists() {
            eyre::ensure!(
                !std::fs::symlink_metadata(path)?.file_type().is_symlink(),
                "Cordial data directory cannot be a symlink"
            );
        }
        std::fs::create_dir_all(path)?;
        let lock_path = path.join(".cordial-node-lock");
        if lock_path.exists() {
            eyre::ensure!(
                !std::fs::symlink_metadata(&lock_path)?
                    .file_type()
                    .is_symlink(),
                "Cordial lock cannot be a symlink"
            );
        }
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lease
            .try_lock_exclusive()
            .map_err(|_| eyre::eyre!("Cordial data directory is already in use"))?;
        let destination = path.join("consensus-manifest.json");
        let expected = Manifest {
            protocol: "cordial-miners".into(),
            protocol_version: 2,
            schema_version: 2,
            config: config.clone(),
        };
        if destination.exists() {
            eyre::ensure!(
                !std::fs::symlink_metadata(&destination)?
                    .file_type()
                    .is_symlink(),
                "Consensus manifest cannot be a symlink"
            );
            let existing: Manifest = serde_json::from_slice(&bounded_read(&destination)?)?;
            eyre::ensure!(
                existing == expected,
                "Cordial protocol, chain, genesis, or storage version mismatch"
            );
        } else {
            reject_existing_stores(path)?;
            let temporary = path.join(format!(".cordial-manifest-{}.tmp", uuid::Uuid::new_v4()));
            let cleanup = Temporary(temporary.clone());
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&serde_json::to_vec_pretty(&expected)?)?;
            file.sync_all()?;
            std::fs::hard_link(&temporary, &destination)?;
            File::open(path)?.sync_all()?;
            drop(cleanup);
        }
        for name in ["cordial-consensus", "cordial-execution"] {
            let store = path.join(name);
            if store.exists() {
                reject_symlinks(&store)?;
            }
        }
        Ok(Self { _lease: lease })
    }
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

fn bounded_read(path: &Path) -> eyre::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    eyre::ensure!(
        bytes.len() <= 1024 * 1024,
        "Cordial configuration exceeds size limit"
    );
    Ok(bytes)
}

fn reject_symlinks(path: &Path) -> eyre::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    eyre::ensure!(
        !metadata.file_type().is_symlink(),
        "Cordial store cannot contain symlinks"
    );
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path)? {
            reject_symlinks(&entry?.path())?;
        }
    }
    Ok(())
}

fn reject_existing_stores(path: &Path) -> eyre::Result<()> {
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        eyre::ensure!(
            !entry.file_type()?.is_symlink(),
            "Unidentified data directory contains a symlink"
        );
        let name = entry.file_name();
        let name = name.to_string_lossy();
        eyre::ensure!(
            !matches!(
                name.as_ref(),
                "cordial-consensus"
                    | "cordial-execution"
                    | "blockstorage"
                    | "dagstorage"
                    | "deploystorage"
                    | "casperbuffer"
                    | "rspace"
            ),
            "Existing store has no Cordial manifest: {name}"
        );
        eyre::ensure!(
            entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "mdb"),
            "Unidentified database in Cordial data directory"
        );
        if entry.file_type()?.is_dir() {
            reject_existing_stores(&entry.path())?;
        }
    }
    Ok(())
}
