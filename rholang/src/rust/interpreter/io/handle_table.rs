//! Open-file-handle table for the native-primitive layer.
//!
//! When `nativeOpen(path, mode)` succeeds it stores the `tokio::fs::File`
//! in this table under a freshly-issued `i64` fd and returns the fd to
//! Rholang. Subsequent positional / read / write / close calls take the
//! fd and look up the entry here.
//!
//! Fds are monotonically increasing per-runtime; they never wrap, and
//! closed slots are removed rather than reused. This keeps replay
//! deterministic (a follower node's replay of a captured
//! non-deterministic sequence gets the same fds as the lead node) at
//! the cost of unbounded `next_fd` growth. In practice the runtime
//! lifetime is a single deployment, so overflow is not a concern.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

/// One entry in the handle table.
///
/// The `File` is wrapped in a `tokio::sync::Mutex` because
/// `tokio::fs::File`'s read/write methods take `&mut self` (they
/// track the file position), but we need to hand a `FileHandle` out
/// through an `Arc<RwLock<HashMap<...>>>` where each handler grabs a
/// clone of the `Arc` and takes the mutex for the duration of one
/// call.
pub struct FileHandle {
    pub file: tokio::sync::Mutex<tokio::fs::File>,
    /// The canonicalized absolute path the file was opened under.
    /// Retained so error messages and later per-file operations can
    /// echo it back, and so a future path-quarantine layer has
    /// something to check against.
    pub canonical_path: std::path::PathBuf,
    /// The `fopen`-style mode string the file was opened under
    /// (`"r"`, `"w"`, `"r+"`, etc.). Kept for later use in enforcing
    /// per-method mode requirements (e.g. `chmod` needs write mode).
    pub mode: String,
}

/// Shared open-fd table plus fresh-fd counter, both `Arc`-wrapped so
/// the whole `SystemProcesses` clone can share one physical table.
#[derive(Clone)]
pub struct FileHandleTable {
    inner: Arc<RwLock<HashMap<i64, Arc<FileHandle>>>>,
    next_fd: Arc<AtomicI64>,
}

impl FileHandleTable {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            // Start at 1 so 0 can double as a sentinel if the caller
            // ever needs one. No cost to reserving it.
            next_fd: Arc::new(AtomicI64::new(1)),
        }
    }

    /// Store a freshly-opened `tokio::fs::File` and return its fd.
    /// The fd is monotonically increasing; slots are never reused.
    pub async fn insert(
        &self,
        file: tokio::fs::File,
        canonical_path: std::path::PathBuf,
        mode: String,
    ) -> i64 {
        let fd = self.next_fd.fetch_add(1, Ordering::SeqCst);
        let handle = Arc::new(FileHandle {
            file: tokio::sync::Mutex::new(file),
            canonical_path,
            mode,
        });
        self.inner.write().await.insert(fd, handle);
        fd
    }

    /// Look up an fd. Returns `None` if the fd was never issued or
    /// has already been closed. Cloning the returned `Arc<FileHandle>`
    /// so the caller can drop the map's read lock before doing I/O
    /// against the file (which takes the per-file mutex).
    pub async fn get(&self, fd: i64) -> Option<Arc<FileHandle>> {
        self.inner.read().await.get(&fd).cloned()
    }

    /// Remove and return an fd's entry. Any outstanding
    /// `Arc<FileHandle>` clones held by concurrent handlers stay
    /// valid until they drop, keeping the underlying `tokio::fs::File`
    /// alive until the last reference goes away.
    pub async fn remove(&self, fd: i64) -> Option<Arc<FileHandle>> {
        self.inner.write().await.remove(&fd)
    }
}

impl Default for FileHandleTable {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn insert_then_get_returns_the_handle() {
        let table = FileHandleTable::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let file = tokio::fs::File::open(tmp.path()).await.unwrap();
        let fd = table
            .insert(file, tmp.path().to_path_buf(), "r".to_string())
            .await;
        let handle = table.get(fd).await.expect("fd should be present");
        assert_eq!(handle.canonical_path, tmp.path());
        assert_eq!(handle.mode, "r");
    }

    #[tokio::test]
    async fn remove_evicts_the_handle() {
        let table = FileHandleTable::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let file = tokio::fs::File::open(tmp.path()).await.unwrap();
        let fd = table
            .insert(file, tmp.path().to_path_buf(), "r".to_string())
            .await;
        table.remove(fd).await.expect("fd should be present");
        assert!(table.get(fd).await.is_none());
    }

    #[tokio::test]
    async fn fds_are_monotonically_increasing() {
        let table = FileHandleTable::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let f1 = tokio::fs::File::open(tmp.path()).await.unwrap();
        let f2 = tokio::fs::File::open(tmp.path()).await.unwrap();
        let fd1 = table
            .insert(f1, tmp.path().to_path_buf(), "r".to_string())
            .await;
        let fd2 = table
            .insert(f2, tmp.path().to_path_buf(), "r".to_string())
            .await;
        assert!(fd2 > fd1, "fd2 ({fd2}) should exceed fd1 ({fd1})");
    }

    #[tokio::test]
    async fn removed_fd_is_not_reissued() {
        // Guards the "monotonically increasing, never reused"
        // invariant. Replay determinism depends on this: a follower
        // that reads the same non-deterministic script must see the
        // same fds as the lead.
        let table = FileHandleTable::new();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let f1 = tokio::fs::File::open(tmp.path()).await.unwrap();
        let f2 = tokio::fs::File::open(tmp.path()).await.unwrap();
        let fd1 = table
            .insert(f1, tmp.path().to_path_buf(), "r".to_string())
            .await;
        table.remove(fd1).await;
        let fd2 = table
            .insert(f2, tmp.path().to_path_buf(), "r".to_string())
            .await;
        assert!(fd2 > fd1, "fd2 ({fd2}) must not reuse fd1's slot ({fd1})");
    }
}
