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
pub trait Match<P, A, K>: Send + Sync {
    fn get(&self, p: P, a: A) -> Option<A>;

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[A]) -> bool { true }
}
