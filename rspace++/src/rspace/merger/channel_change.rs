// See rspace/src/main/scala/coop/rchain/rspace/merger/ChannelChange.scala

#[derive(Debug, Clone)]
pub struct ChannelChange<A> {
    pub added: Vec<A>,
    pub removed: Vec<A>,
}

impl<A> ChannelChange<A> {
    pub fn empty() -> Self {
        Self {
            added: Vec::new(),
            removed: Vec::new(),
        }
    }

    /// Multiset union with cross-set cancellation.
    ///
    /// 1. Multiset union per side: `max(count_self, count_other)` per element.
    ///    Prevents duplication when sibling blocks execute identical deploys.
    /// 2. Cross-set cancellation: items in BOTH `added` and `removed` after
    ///    union are intermediates — produced by one chain in the merge
    ///    aggregation and consumed by a later chain in the same chain
    ///    sequence. Net effect on the channel is zero for those items, so
    ///    they must drop from both sets. Without this, the multiset-diff
    ///    `vec_diff(init, removed) ++ added` in `make_trie_action` can fail
    ///    to remove an intermediate `D_X` from `init` (it isn't there) yet
    ///    add `D_X` from `added`, leaving the channel with `D_X` plus the
    ///    "real" terminal Datum — a multi-Datum write on a single-value
    ///    channel.
    pub fn combine(self, other: Self) -> Self
    where A: PartialEq {
        let added_only_in_other = Self::vec_diff(other.added, &self.added);
        let removed_only_in_other = Self::vec_diff(other.removed, &self.removed);
        let mut added: Vec<A> = self.added.into_iter().chain(added_only_in_other).collect();
        let mut removed: Vec<A> = self
            .removed
            .into_iter()
            .chain(removed_only_in_other)
            .collect();

        // Cross-cancel intermediates: any item present in both `added` and
        // `removed` is an intermediate that should disappear from the net
        // effect. Cancellation is per-element multiset (pair one add with
        // one removal at a time).
        let mut i = 0;
        while i < added.len() {
            if let Some(rem_pos) = removed.iter().position(|x| x == &added[i]) {
                removed.remove(rem_pos);
                added.remove(i);
            } else {
                i += 1;
            }
        }

        Self { added, removed }
    }

    /// Multiset difference: for each element in `to_remove`, removes at most
    /// one matching element from `from`.
    fn vec_diff(mut from: Vec<A>, to_remove: &[A]) -> Vec<A>
    where A: PartialEq {
        for item in to_remove {
            if let Some(pos) = from.iter().position(|x| x == item) {
                from.remove(pos);
            }
        }
        from
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rspace::merger::state_change::StateChange;

    #[test]
    fn combine_should_not_duplicate_when_combining_identical_changes_from_sibling_blocks() {
        // Two sibling blocks both transition channel state: remove A, add B
        let datum_a: Vec<u8> = vec![0xaa; 32];
        let datum_b: Vec<u8> = vec![0xbb; 32];

        let change = ChannelChange {
            added: vec![datum_b.clone()],
            removed: vec![datum_a.clone()],
        };
        let combined = change.clone().combine(change);

        // Applying mkTrieAction formula: (init diff removed) ++ added
        // With init = [A], correct result is [B] (not [B, B])
        let init = vec![datum_a];
        let mut merged_result = StateChange::multiset_diff(&init, &combined.removed);
        merged_result.extend(combined.added);

        assert_eq!(merged_result, vec![datum_b]);
    }
}
