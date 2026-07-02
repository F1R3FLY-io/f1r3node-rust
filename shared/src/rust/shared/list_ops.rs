// See shared/src/main/scala/coop/rchain/catscontrib/listOps.scala

use std::collections::HashMap;
use std::hash::Hash;

pub struct ListOps;

impl ListOps {
    /// Direct port of Scala's ListContrib.sortBy
    ///
    /// Original Scala:
    /// ```scala
    /// def sortBy[A, K: Monoid](list: List[A], map: collection.Map[A, K])(
    ///     implicit ord: Ordering[(K, A)]
    /// ): List[A] =
    ///   list.sortBy(e => (map.getOrElse(e, Monoid[K].empty), e))(ord)
    /// ```
    pub fn sort_by<A, K>(list: Vec<A>, map: &HashMap<A, K>) -> Vec<A>
    where
        A: Clone + Hash + Eq + Ord,
        K: Clone + Default + Ord + std::fmt::Debug,
    {
        let mut scored_items: Vec<(K, A)> = list
            .into_iter()
            .map(|e| {
                let score = map.get(&e).cloned().unwrap_or_default(); // map.getOrElse(e, Monoid[K].empty)
                (score, e) // (score, e) tuple
            })
            .collect();

        scored_items.sort(); // sortBy(...)(ord) - uses standard Ord for (K, A)
        scored_items.into_iter().map(|(_, item)| item).collect()
    }

    /// Port of Scala's ListContrib.sortBy with decreasingOrder
    ///
    /// Scala decreasingOrder is:
    /// ```scala
    /// implicit val decreasingOrder = Ordering.Tuple2(
    ///   Ordering[Long].reverse,  // Score descending (higher first)
    ///   Ordering.by((b: ByteString) => b.toByteArray.toIterable)  // Hash ascending
    /// )
    /// ```
    pub fn sort_by_with_decreasing_order<A>(list: Vec<A>, map: &HashMap<A, i64>) -> Vec<A>
    where A: Clone + Hash + Eq + Ord {
        let mut scored_items: Vec<(i64, A)> = list
            .into_iter()
            .map(|e| {
                let score = map.get(&e).cloned().unwrap_or(0); // map.getOrElse(e, 0L)
                (score, e) // (score, e) tuple
            })
            .collect();

        // Sort with custom ordering: score descending, then item ascending
        scored_items.sort_by(|(score_a, item_a), (score_b, item_b)| {
            // First by score descending (higher score first) - matches Ordering[Long].reverse
            match score_b.cmp(score_a) {
                std::cmp::Ordering::Equal => {
                    // Then by item ascending - matches Ordering.by(hash bytes)
                    item_a.cmp(item_b)
                }
                other_ordering => other_ordering,
            }
        });

        scored_items.into_iter().map(|(_, item)| item).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sort_by_basic() {
        let items = vec!["a", "b", "c", "a"]; // with duplicate
        let mut scores = HashMap::new();
        scores.insert("a", 10);
        scores.insert("b", 5);
        scores.insert("c", 15);

        let sorted = ListOps::sort_by(items, &scores);

        // Expected: sorted by score ascending (5, 10, 15), then by item
        // Note: (K, A) sorts by K first, then A
        // Direct port does NOT remove duplicates, so "a" appears twice
        assert_eq!(sorted, vec!["b", "a", "a", "c"]);
    }

    #[test]
    fn test_sort_by_with_decreasing_order() {
        let items = vec!["a", "b", "c", "d", "e", "f"];
        let mut scores = HashMap::new();
        scores.insert("a", 10); // mid score
        scores.insert("b", 20); // highest score
        scores.insert("c", 20); // same as b - should be sorted by item name
        scores.insert("d", 5); // low score
        scores.insert("e", 10); // same as a - should be sorted by item name
                                // f has no score, so gets default 0

        let sorted = ListOps::sort_by_with_decreasing_order(items, &scores);

        // Expected order:
        // 1. Score 20 (descending): b, c (then by item ascending: b < c)
        // 2. Score 10 (descending): a, e (then by item ascending: a < e)
        // 3. Score 5: d
        // 4. Score 0 (default): f
        assert_eq!(sorted, vec!["b", "c", "a", "e", "d", "f"]);
    }

    #[test]
    fn test_sort_by_with_decreasing_order_empty_scores() {
        let items = vec!["z", "a", "m"];
        let scores = HashMap::new(); // All items get default score 0

        let sorted = ListOps::sort_by_with_decreasing_order(items, &scores);

        // All have same score (0), so sorted by item ascending
        assert_eq!(sorted, vec!["a", "m", "z"]);
    }

    // Fork-choice determinism (S1) linchpin — the tie-break is a TOTAL order on
    // DISTINCT items: `(score desc, item asc)`. So every input permutation sorts to
    // the identical output, which is exactly why the HashSet/HashMap iteration order
    // upstream in the estimator (`estimator.rs` dedup + score map) can never leak
    // into the fork-choice result. This is the confirming witness for TieBreak
    // `sort_total_order` / `output_indep_of_input_perm` in the fork-choice FV.
    #[test]
    fn tie_break_is_total_shuffle_invariant_on_distinct() {
        let mut scores = HashMap::new();
        scores.insert("h1", 20);
        scores.insert("h2", 20);
        scores.insert("h3", 20); // deliberate score ties across distinct items
        scores.insert("h4", 5);
        scores.insert("h5", 5);
        // Canonical total order: score desc, then item asc.
        let expected = vec!["h1", "h2", "h3", "h4", "h5"];
        for p in [
            vec!["h1", "h2", "h3", "h4", "h5"],
            vec!["h5", "h4", "h3", "h2", "h1"],
            vec!["h3", "h1", "h5", "h2", "h4"],
            vec!["h2", "h5", "h1", "h4", "h3"],
        ] {
            assert_eq!(
                ListOps::sort_by_with_decreasing_order(p.clone(), &scores),
                expected,
                "tie-break must be total: permutation {p:?} must sort to the canonical order"
            );
        }
    }
}
