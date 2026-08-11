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

    pub fn combine(self, other: Self) -> Self
    where A: PartialEq {
        self.join(other).normalized()
    }

    pub fn join(self, other: Self) -> Self
    where A: PartialEq {
        Self {
            added: Self::vec_union(self.added, other.added),
            removed: Self::vec_union(self.removed, other.removed),
        }
    }

    pub fn additive_join(mut self, other: Self) -> Self {
        self.added.extend(other.added);
        self.removed.extend(other.removed);
        self
    }

    pub fn normalized(self) -> Self
    where A: PartialEq {
        let mut added = self.added;
        let mut removed = self.removed;
        Self::cancel_common(&mut added, &mut removed);
        Self { added, removed }
    }

    pub fn canonicalized(self) -> Self
    where A: Ord {
        let mut normalized = self.normalized();
        normalized.added.sort();
        normalized.removed.sort();
        normalized
    }

    fn vec_union(left: Vec<A>, right: Vec<A>) -> Vec<A>
    where A: PartialEq {
        let mut result = left;
        for item in Self::vec_diff(right, &result) {
            result.push(item);
        }
        result
    }

    fn cancel_common(added: &mut Vec<A>, removed: &mut Vec<A>)
    where A: PartialEq {
        let mut idx = 0;
        while idx < added.len() {
            if let Some(pos) = removed.iter().position(|item| item == &added[idx]) {
                added.remove(idx);
                removed.remove(pos);
            } else {
                idx += 1;
            }
        }
    }

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
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;
    use crate::rspace::merger::state_change::StateChange;

    #[test]
    fn combine_should_not_duplicate_when_combining_identical_changes_from_sibling_blocks() {
        let datum_a: Vec<u8> = vec![0xaa; 32];
        let datum_b: Vec<u8> = vec![0xbb; 32];

        let change = ChannelChange {
            added: vec![datum_b.clone()],
            removed: vec![datum_a.clone()],
        };
        let combined = change.clone().combine(change);

        let init = vec![datum_a];
        let mut merged_result = StateChange::multiset_diff(&init, &combined.removed);
        merged_result.extend(combined.added);

        assert_eq!(merged_result, vec![datum_b]);
    }

    #[test]
    fn combine_should_net_dependent_chain_intermediate_data() {
        let datum_a: Vec<u8> = vec![0xaa; 32];
        let datum_b: Vec<u8> = vec![0xbb; 32];
        let datum_c: Vec<u8> = vec![0xcc; 32];

        let first = ChannelChange {
            added: vec![datum_b.clone()],
            removed: vec![datum_a.clone()],
        };
        let second = ChannelChange {
            added: vec![datum_c.clone()],
            removed: vec![datum_b],
        };
        let combined = first.combine(second);

        let init = vec![datum_a];
        let mut merged_result = StateChange::multiset_diff(&init, &combined.removed);
        merged_result.extend(combined.added);

        assert_eq!(merged_result, vec![datum_c]);
    }

    // === GAP-1: ChannelChange merge algebra =================================
    //
    // Rust modality companion to
    // formal/rocq/merge_algebra/theories/ChannelNetting.v (and the Z3 witness
    // formal/z3/merge_algebra/channel_netting_monoid.py). A per-datum change is
    // (added multiplicity, removed multiplicity); `combine` is max-multiset
    // union then cancel_common. These pin the legacy inline-normalization
    // counterexample and both order-independent deferred-normalization algebras.

    fn sorted<T: Ord + Clone>(v: &[T]) -> Vec<T> {
        let mut w = v.to_vec();
        w.sort();
        w
    }

    // Finding A, PINNED. The legacy inline-normalized combine is
    // NON-associative: with
    // a = add(x), b = add(x), c = remove(x),
    //   (a . b) . c   cancels x away   -> {}
    //   a . (b . c)   leaves x         -> {x}
    // so the fold RESULT depends on the association (hence, in a multi-branch
    // fold, on order). Mirror of Rocq `combine_not_assoc_exhibit`. This is a
    // historical counterexample retained as a negative-model regression.
    #[test]
    #[ignore = "negative model: legacy inline-normalized max-union is non-associative"]
    fn finding_a_legacy_inline_combine_is_non_associative() {
        let x: u8 = 0x42;
        let add_x = || ChannelChange {
            added: vec![x],
            removed: vec![],
        };
        let rem_x = || ChannelChange {
            added: vec![],
            removed: vec![x],
        };

        // (a . b) . c
        let ab = add_x().combine(add_x());
        let ab_c = ab.combine(rem_x());
        // a . (b . c)
        let bc = add_x().combine(rem_x());
        let a_bc = add_x().combine(bc);

        assert_eq!(sorted(&ab_c.added), Vec::<u8>::new(), "(a.b).c must cancel x away");
        assert_eq!(sorted(&a_bc.added), vec![x], "a.(b.c) must leave x");
        assert_ne!(
            (sorted(&ab_c.added), sorted(&ab_c.removed)),
            (sorted(&a_bc.added), sorted(&a_bc.removed)),
            "max-union combine is non-associative (Finding A)"
        );
    }

    // Projection of additive composition into signed multiplicities.
    fn fixed_net(changes: &[ChannelChange<u8>]) -> BTreeMap<u8, i64> {
        let mut net: BTreeMap<u8, i64> = BTreeMap::new();
        for ch in changes {
            for a in &ch.added {
                *net.entry(*a).or_insert(0) += 1;
            }
            for r in &ch.removed {
                *net.entry(*r).or_insert(0) -= 1;
            }
        }
        net.retain(|_, v| *v != 0);
        net
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(300))]

        // (1) commutativity HOLDS: combine(a, b) and combine(b, a) produce the
        // same added/removed MULTISETS (order aside). Rocq: combine_max_comm.
        #[test]
        fn combine_is_commutative_as_multiset(
            a_add in prop::collection::vec(0u8..6, 0..5),
            a_rem in prop::collection::vec(0u8..6, 0..5),
            b_add in prop::collection::vec(0u8..6, 0..5),
            b_rem in prop::collection::vec(0u8..6, 0..5),
        ) {
            let a = ChannelChange { added: a_add, removed: a_rem };
            let b = ChannelChange { added: b_add, removed: b_rem };
            let ab = a.clone().combine(b.clone());
            let ba = b.combine(a);
            prop_assert_eq!(sorted(&ab.added), sorted(&ba.added), "combine added must be commutative");
            prop_assert_eq!(sorted(&ab.removed), sorted(&ba.removed), "combine removed must be commutative");
        }

        #[test]
        fn additive_join_is_order_independent(
            specs in prop::collection::vec(
                (prop::collection::vec(0u8..6, 0..4), prop::collection::vec(0u8..6, 0..4)),
                1..6),
            rot in 0usize..6,
        ) {
            let changes: Vec<ChannelChange<u8>> = specs
                .into_iter()
                .map(|(added, removed)| ChannelChange { added, removed })
                .collect();
            let n = changes.len();
            let mut permuted = changes.clone();
            permuted.rotate_left(rot % n.max(1));
            let fold = |items: &[ChannelChange<u8>]| {
                items
                    .iter()
                    .cloned()
                    .fold(ChannelChange::empty(), ChannelChange::additive_join)
                    .canonicalized()
            };
            let expected = fold(&changes);
            let actual = fold(&permuted);
            prop_assert_eq!(expected.added, actual.added);
            prop_assert_eq!(expected.removed, actual.removed);
            prop_assert_eq!(
                fixed_net(&changes),
                fixed_net(&permuted),
                "sum-union net must be independent of branch fold order"
            );
        }

        // The legacy inline-normalized max-union combine is nonetheless
        // ORDER-INDEPENDENT when no datum is contributed to a side by >= 2 distinct
        // survivors. Empirical companion to Rocq
        // `combine_max_order_independent_under_no_dup`: generate survivor changes in
        // which every datum has AT MOST ONE adder and AT MOST ONE remover, then fold
        // the shipped `combine` in two different orders and require the same result.
        #[test]
        fn legacy_inline_combine_is_order_independent_under_no_duplication(
            n in 1usize..5,
            // For each datum value (0..len), which survivor adds it / removes it.
            assignments in prop::collection::vec(
                (prop::option::of(0usize..4), prop::option::of(0usize..4)),
                0..6),
            rot in 0usize..6,
        ) {
            let mut survivors: Vec<ChannelChange<u8>> =
                (0..n).map(|_| ChannelChange::empty()).collect();
            for (datum, &(adder, remover)) in assignments.iter().enumerate() {
                if let Some(a) = adder { survivors[a % n].added.push(datum as u8); }
                if let Some(r) = remover { survivors[r % n].removed.push(datum as u8); }
            }
            let fold = |order: &[usize]| -> (Vec<u8>, Vec<u8>) {
                let mut acc = ChannelChange::empty();
                for &i in order { acc = acc.combine(survivors[i].clone()); }
                (sorted(&acc.added), sorted(&acc.removed))
            };
            let ident: Vec<usize> = (0..n).collect();
            let mut rotated = ident.clone();
            rotated.rotate_left(rot % n);
            let reversed: Vec<usize> = ident.iter().rev().copied().collect();
            prop_assert_eq!(fold(&ident), fold(&rotated),
                "legacy inline max-union fold must be order-independent under no-dup");
            prop_assert_eq!(fold(&ident), fold(&reversed),
                "legacy inline max-union fold must be order-independent under no-dup");
        }

        #[test]
        fn additive_join_is_associative_and_canonical(
            a_add in prop::collection::vec(0u8..6, 0..5),
            a_rem in prop::collection::vec(0u8..6, 0..5),
            b_add in prop::collection::vec(0u8..6, 0..5),
            b_rem in prop::collection::vec(0u8..6, 0..5),
            c_add in prop::collection::vec(0u8..6, 0..5),
            c_rem in prop::collection::vec(0u8..6, 0..5),
        ) {
            let a = ChannelChange { added: a_add, removed: a_rem };
            let b = ChannelChange { added: b_add, removed: b_rem };
            let c = ChannelChange { added: c_add, removed: c_rem };
            let left = a.clone().additive_join(b.clone()).additive_join(c.clone()).canonicalized();
            let right = a.additive_join(b.additive_join(c)).canonicalized();
            prop_assert_eq!(left.added, right.added);
            prop_assert_eq!(left.removed, right.removed);
        }

        #[test]
        fn additive_join_preserves_multiplicity_under_permutation(
            specs in prop::collection::vec(
                (prop::collection::vec(0u8..6, 0..4), prop::collection::vec(0u8..6, 0..4)),
                1..6),
            rot in 0usize..6,
        ) {
            let changes: Vec<ChannelChange<u8>> = specs
                .into_iter()
                .map(|(added, removed)| ChannelChange { added, removed })
                .collect();
            let fold = |items: &[ChannelChange<u8>]| {
                items
                    .iter()
                    .cloned()
                    .fold(ChannelChange::empty(), ChannelChange::additive_join)
                    .canonicalized()
            };
            let mut permuted = changes.clone();
            let n = permuted.len();
            permuted.rotate_left(rot % n);
            let expected = fold(&changes);
            let actual = fold(&permuted);
            prop_assert_eq!(expected.added, actual.added);
            prop_assert_eq!(expected.removed, actual.removed);
        }
    }
}
