/**
 * See rspace/src/main/scala/coop/rchain/rspace/Match.scala
 *
 * Type trait for matching patterns with data, plus an optional
 * post-spatial commit hook that sees every bind's matched data so
 * cross-channel `where`-clause guards can fire after all spatial
 * binds succeed. See plan §7.12 / Phase 9.
 *
 * Both
 * methods take their inputs BY REFERENCE. `get` previously took the
 * pattern and the candidate datum by value — the space cloned both per
 * candidate per match attempt (space_matcher.rs), the dominant
 * transport copy of §0.C. The matcher now borrows and clones only what
 * it BINDS into the returned match result; a failing attempt allocates
 * nothing at this boundary. `check_commit` likewise receives borrowed
 * matched data (`&[&A]`) — the guard evaluates against them read-only.
 *
 * @tparam P A type representing patterns
 * @tparam A A type representing data and match result
 * @tparam K A type representing continuations (used by check_commit)
 */
use std::sync::Arc;

use super::errors::RSpaceError;
use super::rspace_interface::{ProduceCommitGuard, commit_produce};

/// Fresh candidate evidence retained from selection until the actual mutation.
/// The guard performs no semantic evaluation while its authority lock is held.
#[derive(Clone, Default)]
pub struct PreparedCommit(Option<Arc<dyn ProduceCommitGuard>>);

impl std::fmt::Debug for PreparedCommit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PreparedCommit")
            .field(&self.0.is_some())
            .finish()
    }
}

impl PreparedCommit {
    pub fn guarded(guard: Arc<dyn ProduceCommitGuard>) -> Self { Self(Some(guard)) }
    pub fn commit<T>(&self, mutation: impl FnOnce() -> T) -> Result<T, RSpaceError> {
        commit_produce(self.0.as_deref(), mutation)
    }
    pub fn commit_with<T>(
        &self,
        outer: Option<&dyn ProduceCommitGuard>,
        mutation: impl FnOnce() -> T,
    ) -> Result<T, RSpaceError> {
        // Two opaque guards may acquire the same registry lock. Refuse before
        // either lock or mutation rather than recursively acquiring a lock.
        if outer.is_some() && self.0.is_some() {
            return Err(RSpaceError::ProduceCommitDenied);
        }
        commit_produce(outer, || self.commit(mutation))?
    }
}

pub trait Match<P, A, K>: Send + Sync {
    fn get(&self, p: &P, a: &A) -> Option<A>;

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[&A]) -> bool { true }

    /// Backwards-compatible preparation hook. Stateful authorities override
    /// this instead of turning check_commit's Boolean into a lease.
    fn prepare_commit(&self, k: &K, matched: &[&A]) -> Option<PreparedCommit> {
        self.check_commit(k, matched).then(PreparedCommit::default)
    }
}
