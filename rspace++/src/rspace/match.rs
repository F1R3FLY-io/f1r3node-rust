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
use super::search_charge::GuardSize;

pub trait Match<P, A, K>: Send + Sync {
    // Takes pattern and data by reference so the matcher hot path can probe a
    // datum without cloning the whole pattern/data on every failed attempt.
    // Only the matched result (Option<A>) is allocated, on success.
    fn get(&self, p: &P, a: &A) -> Option<A>;

    /// The size of a pattern, in the unit `get`'s work grows with: matching
    /// `p` against `a` costs at most in proportion to
    /// `pattern_size(p) + data_size(a)`.
    fn pattern_size(&self, p: &P) -> usize;

    /// The size of a datum, in the same unit as `pattern_size`.
    fn data_size(&self, a: &A) -> usize;

    /// The size of the guard `check_commit` evaluates for `k`, in the same
    /// unit, or `None` when `k` has no guard.
    fn guard_size(&self, _k: &K) -> Option<GuardSize> { None }

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[A]) -> bool { true }
}
