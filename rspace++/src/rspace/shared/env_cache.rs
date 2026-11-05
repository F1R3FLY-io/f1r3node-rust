// Process-global cache of opened LMDB environments, keyed by directory path.
//
// heed 0.22 rejects a second open of the same directory with EnvAlreadyOpened,
// so every LMDB consumer in the workspace (LmdbStoreManager,
// LmdbDirStoreManager, get_or_create_rspace_store) shares one Env per path
// through this cache.
//
// The cache keeps each environment open until the process exits. Read
// transactions use thread-local reader slots, and LMDB releases a slot from a
// destructor that runs when its thread exits. Closing an environment while a
// thread that once read from it is exiting lets that destructor write into the
// unmapped reader table. An environment that is never closed cannot race a
// thread exit.
//
// Map size and max_dbs are honored only on the first open of a path. heed does
// not resize the map after creation, so later calls return the cached Env
// regardless of the requested size.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};

use heed::{Env, EnvOpenOptions};

static ENV_CACHE: OnceLock<StdMutex<HashMap<PathBuf, Arc<Env>>>> = OnceLock::new();

/// Open the LMDB environment for `dir_path`, or return the cached handle. Map
/// size and max_dbs are honored only on the first open of a path.
pub fn get_or_open_env(
    dir_path: &Path,
    max_env_size: usize,
    max_dbs: u32,
) -> Result<Arc<Env>, heed::Error> {
    let cache = ENV_CACHE.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut cache_lock = cache.lock().expect("LMDB env cache mutex poisoned");

    if let Some(env) = cache_lock.get(dir_path) {
        return Ok(env.clone());
    }

    fs::create_dir_all(dir_path)?;
    let mut env_builder = EnvOpenOptions::new();
    env_builder.map_size(max_env_size);
    env_builder.max_dbs(max_dbs);
    env_builder.max_readers(2048);
    // SAFETY: heed::EnvOpenOptions::open is unsafe because LMDB mmaps the
    // data file. Callers must guarantee that:
    //   1. No other process modifies the file while this Env is alive.
    //   2. The path is on a local filesystem (LMDB does not support NFS or similar
    //      networked storage without explicit configuration).
    //   3. The same Env is not opened more than once in this process.
    // We uphold (3) via ENV_CACHE: this call only runs after a cache miss
    // under the cache mutex, and cached environments are never evicted.
    // (1) and (2) are responsibilities of the caller's deployment
    // configuration.
    let env = Arc::new(unsafe { env_builder.open(dir_path)? });
    cache_lock.insert(dir_path.to_path_buf(), env.clone());
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_stays_open_after_every_consumer_drops() {
        let dir = tempfile::tempdir().unwrap();
        let first = get_or_open_env(dir.path(), 1 << 20, 4).unwrap();
        let address = Arc::as_ptr(&first);
        drop(first);

        let second = get_or_open_env(dir.path(), 1 << 20, 4).unwrap();
        assert_eq!(Arc::as_ptr(&second), address);
        assert!(second.read_txn().is_ok());
    }
}
