use super::rho_path::RhoPathQuery;

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
pub trait Match<P, A, K>: Send + Sync {
    fn get(&self, p: &P, a: &A) -> Option<A>;

    fn retained_pattern_path(&self, _pattern: &P) -> Option<Vec<u8>> { None }

    fn retained_datum_path(&self, _datum: &A) -> Option<Vec<u8>> { None }

    fn rho_path_query(&self, _pattern: &P) -> Option<RhoPathQuery> { None }

    /// Opt in to pruning resting data and waiting continuations by the PathMap
    /// paths and structural queries returned by this matcher.
    ///
    /// Returning `true` is a soundness assertion with three obligations:
    ///
    /// 1. every datum path is exactly the unframed byte sequence returned by
    ///    `RhoPathEncode::rho_path_structure` for that datum; and
    /// 2. whenever `get(pattern, datum)` succeeds and the pattern supplies a
    ///    retained path, the datum path starts with the pattern path; and
    /// 3. whenever `get(pattern, datum)` succeeds and the pattern supplies a
    ///    `rho_path_query`, that query accepts the datum path.
    ///
    /// A false positive merely reaches the spatial matcher. A false negative
    /// would hide an enabled COMM, so the conservative default is `false`.
    fn supports_rho_path_candidate_pruning(&self) -> bool { false }

    #[cfg(feature = "retained-flat-dispatch-experiment")]
    fn retained_flat_dispatch_experiment(&self) -> bool { false }

    /// Called once per candidate consume after every spatial bind has
    /// matched and the continuation is about to commit. Default is
    /// always-true (no guard, no veto). Returning `false` rolls the
    /// consume back so the messages stay in the tuple space and the
    /// continuation stays installed.
    fn check_commit(&self, _k: &K, _matched: &[&A]) -> bool { true }
}
