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
// scope and sweeps five per-deploy surfaces in order:
//
//   1. WAL truncation: on uncommitted drop, `wal.truncate_to(mark)`
//      drops entries the deploy appended past its begin mark.
//      A deploy that succeeds calls `commit()` to flip the flag
//      and skip the truncation.
//   2. Waiter drain: `cancel_all_waiters_for_deploy(&deploy_scope)`
//      removes any waiter THIS deploy parked on a conflicting
//      acquire — `release_all_for_deploy` only touches HOLDERS,
//      so this step completes the X-2/G-02 three-step chain
//      documented in `lock.rs::wake_waiters`.
//   3. Lock release: `release_all_for_deploy(&deploy_scope)` drops
//      every range lock the deploy acquired.  Internally runs
//      `wake_waiters` which admits OTHER-deploy waiters.
//   4. Handle-table deploy-cell reset:
//      `set_current_deploy([0;32], Vec::new())` clears the
//      per-runtime deploy cells back to sentinel in a single
//      compound write.
//   5. Handle-table fd/dir-stream sweep: `close_all_for_deploy(
//      &deploy_scope)` is spawned fire-and-forget to close every
//      fd + dir-stream the deploy left open.  Fire-and-forget is
//      sound under the sequential-deploy execution model triage
//      uses: the next deploy's fs_open calls allocate fresh
//      monotonic fds, so stale close_all_for_deploy sweeps can't
//      race against them.
//
// # Sentinel guard
//
// `deploy_scope` MUST be non-sentinel (`!= [0; 32]`).  A
// sentinel scope would trip the `release_all_for_deploy`
// debug_assert and could sweep every sentinel-scoped entry on
// the registry.  Scope is derived internally via
// `Blake2b256(scope_input ‖ 128-bit nonce ‖ domain-tag)`;
// Blake2b256 output is non-sentinel with probability
// `1 - 2⁻²⁵⁶`, so the enforcement is structural.  The `assert!`
// is kept out of debug_assert so release builds also catch a
// future refactor (e.g., a hash-provider swap) that could
// introduce a sentinel-scope path.
//
// # Per-execution isolation
//
// Scope derivation mixes 128 bits of fresh entropy at construction
// time so two guards built from the same seed get distinct scopes.
// This is load-bearing when a `SharedLockRegistry` is broadcast
// across multiple runtimes (via `RuntimeManager::broadcast_fs_slots`)
// and two of them execute the SAME deploy concurrently — e.g., a
// leader proposing a block while a follower replays the same block.
// Without the nonce, both executions would derive identical scopes
// from `Blake2b256(deploy.sig)`, and dropping either guard would
// sweep the other's lock holders + waiters.  Scope values are not
// consensus-observable (never serialized into `WalEntry`), so the
// per-guard RNG is safe.
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

use crypto::rust::hash::blake2b256::Blake2b256;
use rand::RngCore;
use rholang::rust::interpreter::io::handle_table::FileHandleTable;
use rholang::rust::interpreter::io::lock::{DeployScope, SharedLockRegistry};
use rholang::rust::interpreter::io::wal::{Wal, WalMark};

/// Domain-separation tag mixed into every per-execution scope hash.
/// Keep this constant stable — a rename would silently invalidate
/// scope-collision reproducibility in regression tests.
const SCOPE_DOMAIN_TAG: &[u8] = b"WalDeployScope-v1";

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
    /// Production constructor.  Called at deploy entry with a
    /// per-deploy-type seed + the deploy sig for the payload-source
    /// recorder.
    ///
    /// # Per-execution scope isolation
    ///
    /// Scope is derived as
    /// `Blake2b256(scope_input ‖ random_nonce ‖ SCOPE_DOMAIN_TAG)`.
    /// The 128-bit nonce makes the scope UNIQUE per guard construction
    /// (collision probability 2⁻¹²⁸), so concurrent executions of the
    /// same deploy — e.g., play + replay running against separate
    /// runtimes that share a `SharedLockRegistry` via
    /// `RuntimeManager::broadcast_fs_slots` — never share scopes.
    /// Without this isolation, dropping one guard's
    /// `release_all_for_deploy(&scope)` would sweep the other
    /// execution's holders and
    /// `cancel_all_waiters_for_deploy(&scope)` would cancel its
    /// parked waiters.
    ///
    /// # Determinism posture
    ///
    /// Scope is NOT consensus-observable: it exists only in the
    /// `LockRegistry` (per-manager in-memory map) and in each
    /// runtime's `FileHandleTable.current_deploy_scope` cell.
    /// Neither surface is replicated across validators nor serialized
    /// into WAL entries (`WalEntry` has no scope field).  So the
    /// per-guard RNG is safe — it doesn't drift consensus state.
    ///
    /// # Sentinel safety
    ///
    /// Blake2b256 output is non-sentinel (`!= [0; 32]`) with
    /// probability `1 - 2⁻²⁵⁶`.  The `assert!` is retained for
    /// defense-in-depth against a hypothetical hash-provider swap.
    ///
    /// # Parameters
    ///
    /// * `scope_input` — a per-deploy-type seed.  For user deploys,
    ///   pass `&deploy.sig`; for standalone system deploys, pass a
    ///   domain-tagged derivation like
    ///   `[b"system-deploy:", state_hash].concat()`.
    /// * `deploy_sig` — the real signature bytes installed into
    ///   `FileHandleTable.current_deploy_sig` so `journal_write`'s
    ///   payload-source recorder keys the `payload_hash → deploy_sig`
    ///   mapping correctly.  Pass `Vec::new()` for sig-less
    ///   system deploys.
    pub fn new_with_lock_sweep(
        wal: Wal,
        lock_registry: SharedLockRegistry,
        scope_input: &[u8],
        deploy_sig: Vec<u8>,
        fs_handles: FileHandleTable,
    ) -> Self {
        let deploy_scope = Self::derive_scope(scope_input);
        assert!(
            deploy_scope != [0u8; 32],
            "WalDeployScope::new_with_lock_sweep derived sentinel scope [0; 32]; \
             a sentinel scope would trip the release_all_for_deploy guard on Drop.  \
             This should be structurally impossible — Blake2b256 output is \
             non-sentinel with probability 1 - 2⁻²⁵⁶ — so a hash-provider regression \
             is the likely cause."
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

    /// Derive a per-execution scope from a seed.  Mixes 128 bits of
    /// fresh entropy + a domain-separation tag with the caller's
    /// seed so two guards built from identical seeds (concurrent
    /// play + replay of the same deploy sig) still get distinct
    /// scopes.  Blake2b256 output is 32 bytes.
    fn derive_scope(scope_input: &[u8]) -> DeployScope {
        let mut nonce = [0u8; 16];
        rand::rng().fill_bytes(&mut nonce);
        let mut input =
            Vec::with_capacity(scope_input.len() + nonce.len() + SCOPE_DOMAIN_TAG.len());
        input.extend_from_slice(scope_input);
        input.extend_from_slice(&nonce);
        input.extend_from_slice(SCOPE_DOMAIN_TAG);
        let digest = Blake2b256::hash(input);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        out
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
        // 2. Waiter drain.  Any waiter THIS deploy parked on a
        //    conflicting acquire survives `release_all_for_deploy`
        //    (which only reaches HOLDERS) unless cancelled first.
        //    This call drops them deterministically so a Drop that
        //    fires with parked-under-same-scope waiters doesn't
        //    leak them.  Completes the X-2/G-02 three-step chain
        //    documented in `lock.rs::wake_waiters`.
        let _cancelled = self
            .lock_registry
            .cancel_all_waiters_for_deploy(&self.deploy_scope);
        // 3. Lock sweep.  Snapshot the registry out of the
        //    Shared slot so a concurrent `share_from` on the
        //    manager side can't swap the backing mid-sweep.
        //    `release_all_for_deploy` internally calls
        //    `wake_waiters` which admits OTHER-deploy waiters;
        //    see `lock.rs::release_all_for_deploy` X-2/G-02
        //    chain docstring.
        let _released = self
            .lock_registry
            .release_all_for_deploy(&self.deploy_scope);
        // 4. Clear the per-runtime deploy cells back to sentinel
        //    (compound write under a single guard).
        self.fs_handles.set_current_deploy([0u8; 32], Vec::new());
        // 5. Fire-and-forget fd + dir-stream sweep.  Spawn a
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

    /// Scope derivation is non-sentinel for every input (Blake2b256
    /// output is non-sentinel with probability 1 − 2⁻²⁵⁶).  Pins the
    /// structural safety that lets `new_with_lock_sweep` skip the
    /// pre-derivation sentinel check and rely on the post-derivation
    /// `assert!`.
    #[test]
    fn derive_scope_is_never_sentinel() {
        for seed in &[
            &[][..],
            b"user-deploy-sig",
            b"system-deploy:\x00\x01\x02",
            &[0u8; 64][..],
            &[0xFF; 128][..],
        ] {
            let scope = WalDeployScope::derive_scope(seed);
            assert_ne!(scope, [0u8; 32], "derive_scope({seed:?}) returned sentinel");
        }
    }

    /// Two guards built from the SAME seed derive DISTINCT scopes.
    /// Load-bearing: without this, concurrent play+replay of the same
    /// deploy would share a scope and sweep each other's holders.
    #[tokio::test]
    async fn two_scopes_from_same_seed_are_distinct() {
        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles_a = FileHandleTable::new();
        let fs_handles_b = FileHandleTable::new();
        let seed = b"same-deploy-sig";
        let guard_a = WalDeployScope::new_with_lock_sweep(
            wal.clone(),
            lock_registry.clone(),
            seed,
            seed.to_vec(),
            fs_handles_a,
        );
        let guard_b = WalDeployScope::new_with_lock_sweep(
            wal,
            lock_registry,
            seed,
            seed.to_vec(),
            fs_handles_b,
        );
        assert_ne!(
            guard_a.deploy_scope(),
            guard_b.deploy_scope(),
            "two guards from the same seed must derive distinct scopes"
        );
    }

    /// Regression for the cross-execution sweep bug flagged on PR
    /// review.  Two WalDeployScope guards built from the SAME seed
    /// (modeling concurrent play + replay of one deploy against a
    /// manager-shared `SharedLockRegistry`) each take an active lock
    /// + park a conflicting waiter.  Dropping guard A must NOT
    /// release guard B's lock nor cancel guard B's parked waiter.
    #[tokio::test]
    async fn concurrent_scopes_dont_sweep_each_others_locks() {
        use rholang::rust::interpreter::io::lock::{AcquireOutcome, HolderId, LockMode};

        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles_a = FileHandleTable::new();
        let fs_handles_b = FileHandleTable::new();
        let seed = b"concurrent-deploy-sig";
        let dev_inode_a = (1u64, 10u64);
        let dev_inode_b = (1u64, 20u64);
        let holder_a_lock = HolderId::from_bytes([0xA1; 32]);
        let holder_b_lock = HolderId::from_bytes([0xB1; 32]);
        let holder_a_waiter = HolderId::from_bytes([0xA2; 32]);
        let holder_b_waiter = HolderId::from_bytes([0xB2; 32]);
        let external_a = HolderId::from_bytes([0xEA; 32]);
        let external_b = HolderId::from_bytes([0xEB; 32]);

        let guard_a = WalDeployScope::new_with_lock_sweep(
            wal.clone(),
            lock_registry.clone(),
            seed,
            seed.to_vec(),
            fs_handles_a,
        );
        let scope_a = guard_a.deploy_scope();
        let guard_b = WalDeployScope::new_with_lock_sweep(
            wal,
            lock_registry.clone(),
            seed,
            seed.to_vec(),
            fs_handles_b,
        );
        let scope_b = guard_b.deploy_scope();

        // Both guards take an active Write lock on their own
        // (dev, inode).
        let _lock_a = lock_registry
            .try_acquire_range(
                dev_inode_a,
                0,
                1024,
                LockMode::Write,
                holder_a_lock,
                scope_a,
            )
            .expect("A acquires");
        let _lock_b = lock_registry
            .try_acquire_range(
                dev_inode_b,
                0,
                1024,
                LockMode::Write,
                holder_b_lock,
                scope_b,
            )
            .expect("B acquires");

        // Set up a conflicting external holder on each range so each
        // guard can park a waiter on its own (dev, inode) at a
        // DIFFERENT offset.
        let _ext_a = lock_registry
            .try_acquire_range(
                dev_inode_a,
                2048,
                1024,
                LockMode::Write,
                external_a,
                [0xE1; 32],
            )
            .expect("external A acquires");
        let _ext_b = lock_registry
            .try_acquire_range(
                dev_inode_b,
                2048,
                1024,
                LockMode::Write,
                external_b,
                [0xE2; 32],
            )
            .expect("external B acquires");

        // Each guard parks a waiter.
        let admit_a = match lock_registry
            .try_acquire_range_wait(
                dev_inode_a,
                2048,
                1024,
                LockMode::Write,
                holder_a_waiter,
                scope_a,
            )
            .expect("A parks")
        {
            AcquireOutcome::Parked { admit, .. } => admit,
            AcquireOutcome::Immediate(_) => panic!("A waiter: expected Parked"),
        };
        let admit_b = match lock_registry
            .try_acquire_range_wait(
                dev_inode_b,
                2048,
                1024,
                LockMode::Write,
                holder_b_waiter,
                scope_b,
            )
            .expect("B parks")
        {
            AcquireOutcome::Parked { admit, .. } => admit,
            AcquireOutcome::Immediate(_) => panic!("B waiter: expected Parked"),
        };

        // Drop guard A — must only sweep A's state, leaving B's
        // active lock + parked waiter intact.
        drop(guard_a);

        assert!(
            !lock_registry.is_locked(dev_inode_a, (0, 1024)),
            "A's lock swept"
        );
        assert!(
            lock_registry.is_locked(dev_inode_b, (0, 1024)),
            "B's lock survives drop of A — the regression"
        );
        // A's waiter should be cancelled; B's waiter should remain.
        // We can't `.await` on admit_b indefinitely in a unit test,
        // so verify by `.try_recv`-style: a resolved admit (via a
        // non-blocking poll) implies cancellation.  Use `tokio::time::
        // timeout` with a short deadline to detect "still parked".
        let a_resolved = tokio::time::timeout(std::time::Duration::from_millis(50), admit_a)
            .await
            .expect("A waiter resolved on A drop")
            .expect("A sender present");
        assert!(
            matches!(a_resolved, Err(_)),
            "A waiter cancelled on A drop: {:?}",
            a_resolved
        );
        let b_still_parked =
            tokio::time::timeout(std::time::Duration::from_millis(50), admit_b).await;
        assert!(
            b_still_parked.is_err(),
            "B waiter must remain parked after A drops — got {:?}",
            b_still_parked
        );

        // Drop guard B — tears down B's state too.
        drop(guard_b);
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
                b"test-sig",
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
                b"test-sig",
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
    /// `([0; 32], Vec::new())`, never a mixed pair.  The in-scope
    /// value is observed via `guard.deploy_scope()` since the
    /// constructor now derives it internally.
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
            let guard = WalDeployScope::new_with_lock_sweep(
                wal.clone(),
                lock_registry,
                b"seed-cells",
                b"abc".to_vec(),
                fs_handles.clone(),
            );
            assert_eq!(
                fs_handles.current_deploy_scope(),
                guard.deploy_scope(),
                "in-scope: deploy scope matches derived"
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
        let holder = HolderId::from_bytes([0u8; 32]);
        let dev_inode = (1u64, 1u64);
        {
            let guard = WalDeployScope::new_with_lock_sweep(
                wal,
                lock_registry.clone(),
                b"seed-release",
                b"deploy-sig".to_vec(),
                fs_handles,
            );
            let scope = guard.deploy_scope();
            // Acquire a range lock under this deploy scope.
            let _lock_id = lock_registry
                .try_acquire_range(dev_inode, 0, 1024, LockMode::Write, holder, scope)
                .expect("acquire range lock");
            assert!(
                lock_registry.is_locked(dev_inode, (0, 1024)),
                "lock is held while scope is live"
            );
            // guard drops here.
        }
        // Lock released; is_locked returns false (or the registry
        // evicted the empty FileLockState entry).
        assert!(
            !lock_registry.is_locked(dev_inode, (0, 1024)),
            "lock released on scope drop"
        );
    }

    /// Dropping a scope that has a PARKED waiter under its own
    /// deploy scope cancels that waiter via
    /// `cancel_all_waiters_for_deploy`.  Pins step 2 of the Drop
    /// sweep — without this call, parked-under-same-scope
    /// waiters would survive `release_all_for_deploy` (which only
    /// touches holders).  Completes the X-2/G-02 three-step chain
    /// documented in `lock.rs::wake_waiters`.
    #[tokio::test]
    async fn drop_scope_cancels_parked_waiters() {
        use rholang::rust::interpreter::io::lock::{AcquireOutcome, HolderId, LockMode};

        let wal = Wal::new();
        let lock_registry = SharedLockRegistry::new();
        let fs_handles = FileHandleTable::new();

        let holder_a = HolderId::from_bytes([0xAA; 32]);
        let holder_b = HolderId::from_bytes([0x55; 32]);
        let scope_a: DeployScope = [0xAA; 32];
        let dev_inode = (1u64, 1u64);

        // External holder A takes a Write lock under a DIFFERENT
        // scope so B's acquire conflicts and parks.
        let _lock_a = lock_registry
            .try_acquire_range(dev_inode, 0, 1024, LockMode::Write, holder_a, scope_a)
            .expect("A acquires");

        let admit = {
            let guard = WalDeployScope::new_with_lock_sweep(
                wal,
                lock_registry.clone(),
                b"seed-cancel",
                b"sig-b".to_vec(),
                fs_handles,
            );
            let scope_b = guard.deploy_scope();
            // Under scope_b, holder B tries a conflicting Write — parks.
            match lock_registry
                .try_acquire_range_wait(dev_inode, 0, 1024, LockMode::Write, holder_b, scope_b)
                .expect("B parks")
            {
                AcquireOutcome::Parked { admit, .. } => admit,
                AcquireOutcome::Immediate(_) => panic!("expected Parked, got Immediate"),
            }
            // guard drops here -> step 2 cancels B's parked waiter.
        };

        // Parked waiter resolves to Cancelled (via oneshot sender send).
        let resolved = admit.await.expect("sender present");
        assert!(
            matches!(resolved, Err(_)),
            "parked waiter cancelled on scope drop: {:?}",
            resolved
        );
    }
}
