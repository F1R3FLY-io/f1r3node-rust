use std::collections::HashMap;
use std::hash::Hash;

struct Links<K> {
    previous: Option<K>,
    next: Option<K>,
}

pub(super) struct CandidateRotation<K> {
    entries: HashMap<K, [Links<K>; 2]>,
    heads: [Option<K>; 2],
    tails: [Option<K>; 2],
}

impl<K: Clone + Eq + Hash> CandidateRotation<K> {
    pub(super) fn new() -> Self {
        Self {
            entries: HashMap::new(),
            heads: [None, None],
            tails: [None, None],
        }
    }

    pub(super) fn len(&self) -> usize { self.entries.len() }

    pub(super) fn insert(&mut self, key: K) -> bool {
        if self.entries.contains_key(&key) {
            return false;
        }
        self.entries.insert(
            key.clone(),
            std::array::from_fn(|order| Links {
                previous: self.tails[order].clone(),
                next: None,
            }),
        );
        for order in 0..2 {
            if let Some(previous) = &self.tails[order] {
                self.entries
                    .get_mut(previous)
                    .expect("candidate tail exists")[order]
                    .next = Some(key.clone());
            } else {
                self.heads[order] = Some(key.clone());
            }
            self.tails[order] = Some(key.clone());
        }
        true
    }

    pub(super) fn remove(&mut self, key: &K) -> bool {
        let Some(links) = self.entries.remove(key) else {
            return false;
        };
        for (order, links) in links.into_iter().enumerate() {
            if let Some(previous) = &links.previous {
                self.entries
                    .get_mut(previous)
                    .expect("candidate predecessor exists")[order]
                    .next = links.next.clone();
            } else {
                self.heads[order] = links.next.clone();
            }
            if let Some(next) = &links.next {
                self.entries
                    .get_mut(next)
                    .expect("candidate successor exists")[order]
                    .previous = links.previous;
            } else {
                self.tails[order] = links.previous;
            }
        }
        true
    }

    pub(super) fn next(&mut self) -> Option<K> { self.next_for(0) }

    pub(super) fn next_expiry(&mut self) -> Option<K> { self.next_for(1) }

    fn next_for(&mut self, order: usize) -> Option<K> {
        let head = self.heads[order].clone()?;
        if self.heads[order] == self.tails[order] {
            return Some(head);
        }
        let next = self.entries[&head][order]
            .next
            .clone()
            .expect("nonterminal candidate has successor");
        self.entries
            .get_mut(&next)
            .expect("candidate successor exists")[order]
            .previous = None;
        self.heads[order] = Some(next);
        let tail = self.tails[order]
            .as_ref()
            .expect("nonempty candidate queue has tail");
        let old_head = &mut self.entries.get_mut(&head).expect("candidate head exists")[order];
        old_head.previous = Some(tail.clone());
        old_head.next = None;
        self.entries.get_mut(tail).expect("candidate tail exists")[order].next = Some(head.clone());
        self.tails[order] = Some(head.clone());
        Some(head)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};

    use proptest::prelude::*;

    use super::*;

    fn assert_links(queue: &CandidateRotation<u16>, expected: &VecDeque<u16>) {
        assert_order_links(queue, expected, 0);
    }

    fn assert_order_links(queue: &CandidateRotation<u16>, expected: &VecDeque<u16>, order: usize) {
        assert_eq!(queue.len(), expected.len());
        assert_eq!(queue.heads[order], expected.front().copied());
        assert_eq!(queue.tails[order], expected.back().copied());
        let mut visited = HashSet::new();
        let mut current = queue.heads[order];
        let mut previous = None;
        let mut actual = VecDeque::new();
        while let Some(key) = current {
            assert!(visited.insert(key), "candidate links contain a cycle");
            let links = &queue.entries[&key][order];
            assert_eq!(links.previous, previous);
            actual.push_back(key);
            previous = Some(key);
            current = links.next;
        }
        assert_eq!(&actual, expected);
        assert_eq!(visited.len(), queue.entries.len());
    }

    proptest! {
        #[test]
        fn independent_consumers_match_two_reference_orders(
            operations in proptest::collection::vec((0u8..4, 0u16..64), 0..1000)
        ) {
            let mut queue = CandidateRotation::new();
            let mut expected = [VecDeque::new(), VecDeque::new()];
            for (operation, key) in operations {
                match operation {
                    0 => {
                        let fresh = !expected[0].contains(&key);
                        assert_eq!(queue.insert(key), fresh);
                        if fresh {
                            for order in &mut expected { order.push_back(key); }
                        }
                    }
                    1 => {
                        assert_eq!(queue.remove(&key), expected[0].contains(&key));
                        for order in &mut expected { order.retain(|value| *value != key); }
                    }
                    _ => {
                        let order = usize::from(operation - 2);
                        let first = expected[order].pop_front();
                        if let Some(first) = first { expected[order].push_back(first); }
                        let actual = if order == 0 { queue.next() } else { queue.next_expiry() };
                        assert_eq!(actual, first);
                    }
                }
                for (order, expected) in expected.iter().enumerate() {
                    assert_order_links(&queue, expected, order);
                }
            }
        }

        #[test]
        fn expiry_pass_covers_survivors_under_recovery_and_membership_changes(
            initial in 1u16..128,
            operations in proptest::collection::vec((0u8..4, 0u16..256), 0..1000)
        ) {
            let mut queue = CandidateRotation::new();
            for key in 0..initial { queue.insert(key); }
            let count = queue.len();
            let mut obligations: HashSet<_> = (0..initial).collect();
            let mut actions = operations.into_iter();
            for _ in 0..count {
                for _ in 0..4 {
                    if let Some((operation, key)) = actions.next() {
                        match operation {
                            0 => { queue.insert(key); }
                            1 => { queue.remove(&key); obligations.remove(&key); }
                            _ => { queue.next(); }
                        }
                    }
                }
                if let Some(key) = queue.next_expiry() { obligations.remove(&key); }
            }
            prop_assert!(obligations.is_empty());
        }

        #[test]
        fn mixed_operations_preserve_exact_order_and_bidirectional_links(
            operations in proptest::collection::vec((0u8..3, 0u16..64), 0..1000)
        ) {
            let mut queue = CandidateRotation::new();
            let mut expected = VecDeque::new();
            for (operation, key) in operations {
                match operation {
                    0 => {
                        let fresh = !expected.contains(&key);
                        assert_eq!(queue.insert(key), fresh);
                        if fresh { expected.push_back(key); }
                    }
                    1 => {
                        let present = expected.contains(&key);
                        assert_eq!(queue.remove(&key), present);
                        expected.retain(|value| *value != key);
                    }
                    _ => {
                        let first = expected.pop_front();
                        if let Some(first) = first { expected.push_back(first); }
                        assert_eq!(queue.next(), first);
                    }
                }
                assert_links(&queue, &expected);
            }
        }

        #[test]
        fn later_arrivals_and_duplicates_cannot_overtake_an_existing_candidate(initial in 1u16..128) {
            let mut queue = CandidateRotation::new();
            for key in 0..initial { queue.insert(key); }
            let target = initial - 1;
            for examined in 0..initial {
                queue.insert(initial + examined);
                queue.insert(target);
                assert_eq!(queue.next(), Some(examined));
            }
        }
    }

    #[test]
    fn alternating_consumers_do_not_partition_the_membership() {
        let mut queue = CandidateRotation::new();
        queue.insert(1u16);
        queue.insert(2);
        for expected in [1, 2, 1, 2] {
            assert_eq!(queue.next(), Some(expected));
            assert_eq!(queue.next_expiry(), Some(expected));
        }
    }

    #[test]
    fn capacity_exit_after_one_examination_preserves_rotation() {
        let mut queue = CandidateRotation::new();
        queue.insert(1u16);
        queue.insert(2);
        for expected in [1, 2, 1, 2] {
            assert_eq!(queue.next(), Some(expected));
        }
    }

    #[test]
    fn removal_of_head_tail_and_last_entry_preserves_membership() {
        let mut queue = CandidateRotation::new();
        for key in [1u16, 2, 3, 4] {
            queue.insert(key);
        }
        for key in [1, 4, 2, 3] {
            assert!(queue.remove(&key));
        }
        assert_links(&queue, &VecDeque::new());
        assert_eq!(queue.next(), None);
        assert!(!queue.remove(&3));
        queue.insert(5);
        assert_eq!(queue.next(), Some(5));
        assert_links(&queue, &VecDeque::from([5]));
    }
}
