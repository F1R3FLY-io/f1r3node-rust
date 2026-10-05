// Lock family — range + sequential lock acquire / release
// handlers against the per-runtime `LockRegistry`.  All
// non-verifying — lock state is per-runtime, not persistent
// across restarts, so there's no WAL journaling.  BUT the
// acquire outcome IS consensus-observable (both leader and
// follower must mint the same `LockId` for the same request,
// deterministically).
//
// # Status (Wave 4)
//
//   - `fs_lock_range` — range-based acquire; returns `LockId`
//     on success.  Added by slice 4.34.
//   - `fs_lock_sequential` — whole-file sequential acquire (no
//     offset / length / mode slots).  Added by slice 4.35.
//
// Yet to land:
//
//   - `fs_release_lock` — release by `LockId`.
//   - `fs_unlock_range` — release by (fd, offset, length)
//     without needing the LockId handle.
//
// Family: [`HandlerFamily::Lock`](super::super::handler_trait::family::HandlerFamily::Lock).
//
// # park_external_during deferral
//
// Fileio's lock handlers call
// `deterministic_reduction::park_external_during(admit)` on the
// parked-wait path so the reduction driver can advance other
// participants while the current task awaits the admit signal.
// `park_external_during` has NOT been ported to dev yet (Wave 5
// scope — same deferral as the observation + mutation handlers'
// blocking syscalls).  Under the Wave 4 shape, the parked-wait
// handler does `admit.await` directly without parking the
// reduction driver.  This matches fileio's SEMANTIC behavior
// (same success / failure replies, same deterministic LockId
// minting) but foregoes the concurrent-advance optimization —
// if two parked waiters are in the same deploy, the second
// blocks the deploy until the first grants.  Production load
// won't hit this often; integration tests exercise single-waiter
// shapes.  Documented at every parked-wait call site.

pub mod fs_lock_range;
pub mod fs_lock_sequential;
