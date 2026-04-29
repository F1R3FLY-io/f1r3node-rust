// Process-global cache of opened LMDB environments, keyed by directory path.
//
// Why this exists: heed 0.22 rejects duplicate env opens of the same directory
// with EnvAlreadyOpened. Any code that opens the same path more than once must
// share a single Env handle. The cache mediates this for all LMDB consumers in
// the workspace (LmdbStoreManager, LmdbDirStoreManager,
// get_or_create_rspace_store).
//
// Lifecycle (Weak-ref semantics): the cache stores Weak<Env> entries. Consumers
// receive Arc<Env> clones. Once the last Arc<Env> for a given path drops, the
// inner heed::Env releases its file handles and the cached Weak becomes dead.
// The next lookup detects the dead Weak, evicts it, and opens a fresh env. This
// gives bounded growth (one live entry per active path) without ever risking
// EnvAlreadyOpened on outstanding clones.
//
// Map size is locked at first open per path. heed does not allow resizing the
// mmap after env creation, so subsequent get_or_open_env calls for the same
// path return the cached Env regardless of the requested map_size.
//
// Locking: a single StdMutex guards the HashMap. The critical section on the
// hot path is HashMap::get + Weak::upgrade (an Arc bump if alive). The slow
// path (env_builder.open) only runs on cache miss — once per unique path until
// all consumers drop, then again. Per-path lock granularity would add
// complexity without measurable benefit.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};

use heed::{Env, EnvOpenOptions};

static ENV_CACHE: OnceLock<StdMutex<HashMap<PathBuf, Weak<Env>>>> = OnceLock::new();

/// Open an LMDB environment for `dir_path`, or return a clone of the cached
/// handle if one is already alive. Map size and max_dbs are honored only on
/// the first open per path.
pub fn get_or_open_env(
    dir_path: &Path,
    max_env_size: usize,
    max_dbs: u32,
) -> Result<Arc<Env>, heed::Error> {
    let cache = ENV_CACHE.get_or_init(|| StdMutex::new(HashMap::new()));
    let mut cache_lock = cache.lock().expect("LMDB env cache mutex poisoned");

    if let Some(weak) = cache_lock.get(dir_path) {
        if let Some(arc) = weak.upgrade() {
            return Ok(arc);
        }
        // Last consumer dropped — evict the dead Weak and fall through to reopen.
        cache_lock.remove(dir_path);
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
    // under the cache mutex, so no concurrent open of the same path is
    // possible. (1) and (2) are responsibilities of the caller's deployment
    // configuration.
    let env = unsafe { env_builder.open(dir_path)? };
    let arc = Arc::new(env);
    cache_lock.insert(dir_path.to_path_buf(), Arc::downgrade(&arc));
    Ok(arc)
}
