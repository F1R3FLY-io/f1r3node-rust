/**
 * See rspace/src/main/scala/coop/rchain/rspace/Match.scala
 *
 * Type trait for matching patterns with data.
 *
 * @tparam P A type representing patterns
 * @tparam A A type representing data and match result
 */
pub trait Match<P, A>: Send + Sync {
    // Takes pattern and data by reference so the matcher hot path can probe a
    // datum without cloning the whole pattern/data on every failed attempt.
    // Only the matched result (Option<A>) is allocated, on success.
    fn get(&self, p: &P, a: &A) -> Option<A>;
}
