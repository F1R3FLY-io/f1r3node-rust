// Per-deploy RAII guard that brackets a deploy's filesystem
// side effects.  Mirrors fileio's `WalDeployScope`, simplified
// for triage (no cost-accounting-aware WAL commitment ladder —
// triage treats every unaborted deploy as committed by default).
//
// # What this guards
//
// A deploy that acquires range locks, opens fds, or appends WAL
// entries can leave any of those behind if its Rholang term
// panics or returns without calling the matching release
// operations.  Without a guard the leaks accumulate across the
// block.  The Drop impl here fires at the end of every deploy
// scope and sweeps three per-deploy surfaces:
//
//   1. Lock registry: `release_all_for_deploy(&deploy_scope)`
//      drops every range lock the deploy acquired.  Pairs with
//      [`cancel_all_waiters_for_deploy`](super::super::super::rholang::super::rust::interpreter::io::lock::LockRegistry::cancel_all_waiters_for_deploy)
//      which drains PARKED waiters first — ordering matters so
//      the X-2/G-02 defense-in-depth chain holds (see
//      `lock.rs::wake_waiters` docstring).
//   2. WAL truncation: on uncommitted drop, `wal.truncate_to(mark)`
//      drops entries the deploy appended past its begin mark.
//      A deploy that succeeds calls `commit()` to flip the flag
//      and skip the truncation.
//   3. Handle-table sweep: `set_current_deploy([0;32], Vec::new())`
//      clears the per-runtime deploy cells back to sentinel, and
//      `close_all_for_deploy(&deploy_scope)` is spawned fire-and-
//      forget to close every fd + dir-stream the deploy left
//      open.  Fire-and-forget is sound under the sequential-
//      deploy execution model triage uses: the next deploy's
//      fs_open calls allocate fresh monotonic fds, so stale
//      close_all_for_deploy sweeps can't race against them.
//
// # Sentinel guard
//
// `deploy_scope` MUST be non-sentinel (`!= [0; 32]`).  A
// sentinel scope would trip the `release_all_for_deploy`
// debug_assert and could sweep every sentinel-scoped entry on
// the registry.  Production call sites derive the scope via
// Blake2b256(deploy.sig); Blake2b256 output is non-sentinel
// with probability `1 - 2⁻²⁵⁶`, so the enforcement is
// structural.  The `assert!` is kept out of debug_assert so
// release builds also catch a future refactor that introduces
// a sentinel-scope path.
//
// # Interaction with cost accounting
//
// Fileio's `WalDeployScope` additionally brackets cost
// reservations through its own `RuntimeBudget` field.  Triage
// doesn't yet port RuntimeBudget (deliberately out-of-scope per
// the Item A plan), so this slice ports only the parts
// orthogonal to cost accounting.  When RuntimeBudget lands on
// triage, this guard can grow the matching field and Drop
// branch without disturbing current callers.

use rholang::rust::interpreter::io::handle_table::FileHandleTable;
use rholang::rust::interpreter::io::lock::{DeployScope, SharedLockRegistry};
use rholang::rust::interpreter::io::wal::{Wal, WalMark};

/// RAII guard that cleans up a deploy's filesystem-side state at
/// scope exit.  Construct via
/// [`WalDeployScope::new_with_lock_sweep`] at deploy entry; call
/// [`WalDeployScope::commit`] at the top of the happy path and
/// let Drop handle the sweep on either outcome.
#[must_use = "WalDeployScope must be held for the duration of a deploy; dropping \
              immediately clears the deploy cells back to sentinel and reverts \
              the deploy's WAL contribution"]
pub struct WalDeployScope {
    wal: Wal,
    mark: WalMark,
    committed: bool,
    lock_registry: SharedLockRegistry,
    deploy_scope: DeployScope,
    fs_handles: FileHandleTable,
}

impl WalDeployScope {
    /// Production constructor.  Called at deploy entry with the
    /// deploy's derived scope + a sig for the payload-source
    /// recorder.  Sets the per-runtime deploy cells to
    /// `(deploy_scope, deploy_sig)` so handlers tag their lock
    /// acquires + journal_write records correctly, and takes a
    /// `begin_deploy` mark against the WAL so uncommitted-drop
    /// can revert.
    ///
    /// `deploy_scope` MUST be non-sentinel — see type-level
    /// docstring for the structural rationale.
    pub fn new_with_lock_sweep(
        wal: Wal,
        lock_registry: SharedLockRegistry,
        deploy_scope: DeployScope,
        deploy_sig: Vec<u8>,
        fs_handles: FileHandleTable,
    ) -> Self {
        assert!(
            deploy_scope != [0u8; 32],
            "WalDeployScope::new_with_lock_sweep called with sentinel scope [0; 32]; \
             a sentinel scope would trip the release_all_for_deploy guard on Drop.  \
             Callers must derive a non-sentinel scope (e.g., Blake2b256(deploy.sig))."
        );
        let mark = wal.begin_deploy();
        // Compound setter preserves the "(scope, sig) populated
        // or both sentinel" invariant atomically.
        fs_handles.set_current_deploy(deploy_scope, deploy_sig);
        Self {
            wal,
            mark,
            committed: false,
            lock_registry,
            deploy_scope,
            fs_handles,
        }
    }

    /// Mark the deploy as committed.  Prevents the Drop impl
    /// from truncating the deploy's WAL contribution.  The lock
    /// + fs-handle sweeps still fire on Drop — commitment only
    /// affects WAL rollback.
    pub fn commit(&mut self) { self.committed = true; }

    /// The deploy scope this guard tracks.  Exposed for tests
    /// that need to construct an `is_locked` query under the
    /// same scope.
    pub fn deploy_scope(&self) -> DeployScope { self.deploy_scope }
}

impl Drop for WalDeployScope {
    fn drop(&mut self) {
        // 1. WAL rollback on uncommitted drop (deploy panicked
        //    or returned without calling commit).  Committed
        //    deploys leave their WAL contributions in place for
        //    the block-level snapshot.
        if !self.committed {
            self.wal.truncate_to(self.mark);
        }
        // 2. Lock sweep.  Snapshot the registry out of the
        //    Shared slot so a concurrent `share_from` on the
        //    manager side can't swap the backing mid-sweep.
        //    `release_all_for_deploy` internally calls
        //    `wake_waiters` which admits OTHER-deploy waiters;
        //    see `lock.rs::release_all_for_deploy` X-2/G-02
        //    chain docstring.
        let _released = self
            .lock_registry
            .release_all_for_deploy(&self.deploy_scope);
        // 3. Clear the per-runtime deploy cells back to sentinel
        //    (compound write under a single guard).
        self.fs_handles.set_current_deploy([0u8; 32], Vec::new());
        // 4. Fire-and-forget fd + dir-stream sweep.  Spawn a
        //    detached tokio task holding Arc-clones of the
        //    handle table; the sweep runs on the shared
        //    runtime pool.  Within a block, deploys run
        //    sequentially so stale sweeps can't race the next
        //    deploy's fs_open fd allocations.
        let fhs = self.fs_handles.clone();
        let scope = self.deploy_scope;
        tokio::spawn(async move {
            fhs.close_all_for_deploy(&scope).await;
        });
    }
}

#[cfg(test)]
mod tests {
    use rholang::rust::interpreter::io::handle_table::FileHandleTable;
    use rholang::rust::interpreter::io::lock::SharedLockRegistry;
    use rholang::rust::interpreter::io::wal::Wal;

    use super::*;

    fn mk_scope(byte: u8) -> DeployScope { [byte; 32] }

    #[tokio::test]
    #[should_panic(expected = "sentinel scope [0; 32]")]
    async fn new_with_lock_sweep_panics_on_sentinel_scope() {
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();
        let _ = WalDeployScope::new_with_lock_sweep(
            wal,
            lock_registry,
            [0u8; 32],
            Vec::new(),
            fs_handles,
        );
    }

    /// Dropping an uncommitted scope truncates the WAL back to
    /// the begin mark.  Pin that a deploy that appended entries
    /// and then dropped without committing leaves no WAL residue.
    #[tokio::test]
    async fn drop_uncommitted_scope_truncates_wal() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry,
                mk_scope(0xAB),
                b"test-sig".to_vec(),
                fs_handles,
            );
            // Append a stub entry while the scope is live.
            let _ = wal.append_with_ack(
                WalEntry {
                    op: WalOp::Exists,
                    path: std::path::PathBuf::from("/tmp/x"),
                    extra_path: None,
                    offset: None,
                    length: None,
                    payload_ref: Some(PayloadRef::Hash([1u8; 32])),
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                },
                [2u8; 32],
            );
            assert_eq!(wal.len(), 1, "entry visible while scope is live");
            // _scope drops here — uncommitted → truncate.
        }
        assert_eq!(wal.len(), 0, "WAL truncated on uncommitted drop");
    }

    /// Dropping a committed scope leaves WAL entries intact.
    #[tokio::test]
    async fn drop_committed_scope_preserves_wal() {
        use rholang::rust::interpreter::io::wal::{PayloadRef, WalEntry, WalOp, WalOutcome};
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();
        {
            let mut scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry,
                mk_scope(0xCD),
                b"test-sig".to_vec(),
                fs_handles,
            );
            let _ = wal.append_with_ack(
                WalEntry {
                    op: WalOp::Exists,
                    path: std::path::PathBuf::from("/tmp/y"),
                    extra_path: None,
                    offset: None,
                    length: None,
                    payload_ref: Some(PayloadRef::Hash([3u8; 32])),
                    mode_bits: None,
                    owner: None,
                    group: None,
                    outcome: WalOutcome::Success,
                },
                [4u8; 32],
            );
            scope.commit();
            // scope drops here — committed → no truncate.
        }
        assert_eq!(wal.len(), 1, "committed WAL entry survives drop");
    }

    /// Entering a scope sets the per-runtime deploy cells;
    /// dropping clears them back to sentinel.  Pins the
    /// invariant that handlers observe either `(scope, sig)` or
    /// `([0; 32], Vec::new())`, never a mixed pair.
    #[tokio::test]
    async fn scope_lifecycle_toggles_deploy_cells() {
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();
        assert_eq!(
            fs_handles.current_deploy_scope(),
            [0u8; 32],
            "pre-scope: sentinel scope"
        );
        {
            let _scope = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry,
                mk_scope(0xEF),
                b"abc".to_vec(),
                fs_handles.clone(),
            );
            assert_eq!(
                fs_handles.current_deploy_scope(),
                mk_scope(0xEF),
                "in-scope: deploy scope set"
            );
            assert_eq!(
                fs_handles.current_deploy_sig(),
                b"abc",
                "in-scope: deploy sig set"
            );
        }
        assert_eq!(
            fs_handles.current_deploy_scope(),
            [0u8; 32],
            "post-scope: scope cleared"
        );
        assert_eq!(
            fs_handles.current_deploy_sig().as_slice(),
            b"",
            "post-scope: sig cleared"
        );
    }

    /// Dropping a scope that acquired range locks releases them
    /// via `release_all_for_deploy`.  Pins the lock-sweep arm
    /// of Drop.
    #[tokio::test]
    async fn drop_scope_releases_locks() {
        use rholang::rust::interpreter::io::lock::{HolderId, LockMode};
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();
        let scope = mk_scope(0x12);
        let holder = HolderId::from_bytes([0u8; 32]);
        {
            let _guard = WalDeployScope::new_with_lock_sweep(
                wal,
                lock_registry.clone(),
                scope,
                b"deploy-sig".to_vec(),
                fs_handles,
            );
            // Acquire a range lock under this deploy scope.
            let dev_inode = (1u64, 1u64);
            let _lock_id = lock_registry
                .try_acquire_range(dev_inode, 0, 1024, LockMode::Write, holder, scope)
                .expect("acquire range lock");
            assert!(
                lock_registry.is_locked(dev_inode, (0, 1024)),
                "lock is held while scope is live"
            );
            // _guard drops here.
        }
        // Lock released; is_locked returns false (or the registry
        // evicted the empty FileLockState entry).
        assert!(
            !lock_registry.is_locked((1u64, 1u64), (0, 1024)),
            "lock released on scope drop"
        );
    }
}
