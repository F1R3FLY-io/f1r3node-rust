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
    // Takes pattern and data by reference so the matcher hot path can probe a
    // datum without cloning the whole pattern/data on every failed attempt.
    // Only the matched result (Option<A>) is allocated, on success.
    fn get(&self, p: &P, a: &A) -> Option<A>;

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[A]) -> bool { true }

    /// Added by DR-129 (E3-a): whether `check_commit` can return false for
    /// the continuation `k`, for any matched data. Under dev's random play
    /// order, play tries one greedy tuple and stores the operation when the
    /// commit vetoes it, so replay's greedy pass in its own order can find a
    /// tuple that passes. The Stored check of replay therefore skips such a
    /// continuation. The default is false, so a matcher that overrides
    /// `check_commit` must override this method too.
    fn commit_may_veto(&self, _k: &K) -> bool { false }

    /// Changed by DR-131 S6: `check_commit` over borrowed matched data,
    /// without a meter (D-I). The native replay passes the matched data by
    /// reference, so it does not copy them (D-M6, DR-88). The default copies
    /// the data and calls `check_commit`, so every matcher keeps its result.
    /// A matcher can override it to read the data in place.
    fn check_commit_by_reference(&self, k: &K, matched: &[&A]) -> bool
    where A: Clone {
        let owned: Vec<A> = matched.iter().map(|value| (*value).clone()).collect();
        self.check_commit(k, &owned)
    }
}
