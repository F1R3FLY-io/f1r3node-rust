/**
 * See rspace/src/main/scala/coop/rchain/rspace/Match.scala
 *
 * Type trait for matching patterns with data, plus an optional
 * post-spatial commit hook that sees every bind's matched data so
 * cross-channel `where`-clause guards can fire after all spatial
 * binds succeed. See plan §7.12 / Phase 9.
 *
 * @tparam P A type representing patterns
 * @tparam A A type representing data and match result
 * @tparam K A type representing continuations (used by check_commit)
 */
use crate::rspace::errors::RSpaceError;
use crate::rspace::hashing::native_source::SourceMeter;

pub trait Match<P, A, K>: Send + Sync {
    // Takes pattern and data by reference so the matcher hot path can probe a
    // datum without cloning the whole pattern/data on every failed attempt.
    // Only the matched result (Option<A>) is allocated, on success.
    fn get(&self, p: &P, a: &A) -> Option<A>;

    /// Metered `get`. D-M1 (DR-88): an implementation reserves every read of
    /// the pattern and the datum on `meter` before it performs the read, so
    /// a caller does not inspect either value first.
    fn get_metered(
        &self,
        _p: &P,
        _a: &A,
        _meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<Option<A>, RSpaceError> {
        Err(RSpaceError::HostWorkRejected)
    }

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[A]) -> bool { true }

    /// Metered `check_commit`. D-M1 (DR-88): an implementation reserves every
    /// read of the continuation (in practice its guard) and of the matched
    /// data on `meter` before it performs the read, so a caller does not
    /// inspect the continuation first. D-M6 (DR-88): the matched data are
    /// passed by reference, so a caller does not copy them.
    // Changed by D-M6 (DR-88): matched data by reference.
    // fn check_commit_metered(
    //     &self,
    //     _k: &K,
    //     _matched: &[A],
    //     _meter: &(dyn SourceMeter + Send + Sync),
    // ) -> Result<bool, RSpaceError> {
    //     Err(RSpaceError::HostWorkRejected)
    // }
    fn check_commit_metered(
        &self,
        _k: &K,
        _matched: &[&A],
        _meter: &(dyn SourceMeter + Send + Sync),
    ) -> Result<bool, RSpaceError> {
        Err(RSpaceError::HostWorkRejected)
    }
}
