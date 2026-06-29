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
        let mut added = Self::vec_union(self.added, other.added);
        let mut removed = Self::vec_union(self.removed, other.removed);
        Self::cancel_common(&mut added, &mut removed);
        Self { added, removed }
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
}
