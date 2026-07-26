// File-descriptor table.
//
// A per-runtime `Arc<RwLock<HashMap<u64, FileHandle>>>` mapping opaque u64
// fds to open `File` handles.  Fds are monotonic — the counter never
// rewinds — so a closed fd is never reused.  This preserves the invariant
// that a stale fd reliably observes `FSERR_CLOSED` rather than aliasing a
// later-opened file.
//
// Lifecycle: the plan calls for `snapshot_next_fd`/`truncate_to` for
// deploy-boundary rollback on the production deploy path.  Those live at
// the runtime layer (Phase 1 tail) and take an immutable snapshot of the
// counter; on rollback, any fds allocated past the snapshot are closed and
// removed from the table.

use std::collections::HashMap;
use std::fs::File;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::sync::RwLock;

use super::mode::AccessMode;

#[derive(Debug)]
pub struct FileHandle {
    pub file: File,
    pub canon_path: PathBuf,
    pub mode: AccessMode,
}

#[derive(Debug, Clone)]
pub struct FileHandleTable {
    inner: Arc<Inner>,
}

#[derive(Debug)]
struct Inner {
    table: RwLock<HashMap<u64, FileHandle>>,
    next_fd: AtomicU64,
}

impl FileHandleTable {
    pub fn new() -> Self {
        FileHandleTable {
            inner: Arc::new(Inner {
                table: RwLock::new(HashMap::new()),
                next_fd: AtomicU64::new(1),
            }),
        }
    }

    /// Allocate a fresh fd and register the handle.
    ///
    /// Returns `Err(())` when the per-runtime fd cap is reached; the
    /// handler translates that to `FSERR_QUOTA_EXCEEDED`.
    pub async fn insert(&self, handle: FileHandle) -> Result<u64, ()> {
        let mut table = self.inner.table.write().await;
        if table.len() >= super::MAX_OPEN_FDS {
            return Err(());
        }
        let fd = self.inner.next_fd.fetch_add(1, Ordering::SeqCst);
        table.insert(fd, handle);
        Ok(fd)
    }

    /// Remove and close the handle at `fd`.  Returns `true` if the fd was
    /// present.  Idempotent: closing an unknown fd is a no-op returning
    /// `false`.
    pub async fn remove(&self, fd: u64) -> bool {
        let mut table = self.inner.table.write().await;
        table.remove(&fd).is_some()
    }

    /// Snapshot the next-fd counter for deploy-boundary rollback.
    pub fn snapshot_next_fd(&self) -> u64 { self.inner.next_fd.load(Ordering::SeqCst) }

    /// Roll back to the snapshot, closing every fd allocated past it.
    pub async fn truncate_to(&self, snapshot: u64) {
        let mut table = self.inner.table.write().await;
        table.retain(|&fd, _| fd < snapshot);
        // Note: `next_fd` is monotonic even across rollback — that is the
        // invariant that prevents fd aliasing across deploys.
    }

    /// Run `f` against the handle at `fd` under a write lock.  Returns
    /// `None` if the fd is absent.
    pub async fn with_mut<F, R>(&self, fd: u64, f: F) -> Option<R>
    where F: FnOnce(&mut FileHandle) -> R {
        let mut table = self.inner.table.write().await;
        table.get_mut(&fd).map(f)
    }
}

impl Default for FileHandleTable {
    fn default() -> Self { Self::new() }
}
