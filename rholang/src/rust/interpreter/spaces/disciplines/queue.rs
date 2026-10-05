//! The queue discipline of the native spaces (decision 11, P4 design B.4 and B.8).
//!
//! A queue is head-only on both sides. The head of the records and the head of the waiting
//! continuations are the entries with the least `ord`, and only the two heads interact. A head
//! that does not match blocks the entries behind it. An incoming element joins the tail of its
//! side, so it interacts only when its side is empty.
//!
//! The order comes from the `ord` of the envelopes, never from the list order of the store: the
//! hot store inserts at the front, and a restart can load a list in another order. Two entries
//! with one `ord` order by their bincode bytes, so the head is a function of the entry set.
//! The legacy queue popped the front of a `VecDeque`, and its waiter side scanned every
//! continuation. Both are replaced.

use std::cmp::Ordering;

use serde::Serialize;

use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{Discipline, Ordered, Side};

/// The queue discipline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct QueueDiscipline;

impl Discipline for QueueDiscipline {
    const NAME: &'static str = "queue";
    const USES_COUNTER: bool = true;
    const JOINS: bool = false;
    const SEND_PARAMS: &'static [&'static str] = &[];

    /// The entry with the least `(ord, bincode bytes)`, on both sides.
    fn head<T: Serialize>(
        &self,
        _side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        let mut head: Option<(usize, &Ordered<'_, T>)> = None;
        for (index, entry) in entries.iter().enumerate() {
            head = match head {
                None => Some((index, entry)),
                Some((current, best)) => match precedes(entry, best)? {
                    true => Some((index, entry)),
                    false => Some((current, best)),
                },
            };
        }
        Ok(head.map(|(index, _)| index))
    }

    /// An incoming element joins the tail, so it is the head only of an empty side.
    fn reaches_head(&self, _side: Side, queued: usize) -> bool { queued == 0 }
}

// True when `entry` comes strictly before `best`: a lesser `ord`, or an equal `ord` and lesser
// bytes.
fn precedes<T: Serialize>(
    entry: &Ordered<'_, T>,
    best: &Ordered<'_, T>,
) -> Result<bool, SpaceError> {
    match entry.ord.cmp(&best.ord) {
        Ordering::Less => Ok(true),
        Ordering::Greater => Ok(false),
        Ordering::Equal => Ok(bytes(entry.entry)? < bytes(best.entry)?),
    }
}

fn bytes<T: Serialize>(entry: &T) -> Result<Vec<u8>, SpaceError> {
    bincode::serialize(entry)
        .map_err(|_| SpaceError::Inconsistent("a collection entry is not serializable"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ordered(entries: &[(i64, String)]) -> Vec<Ordered<'_, String>> {
        entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect()
    }

    fn head_of(entries: &[(i64, String)], side: Side) -> Option<(i64, String)> {
        QueueDiscipline
            .head(side, &ordered(entries))
            .expect("strings serialize")
            .and_then(|index| entries.get(index).cloned())
    }

    fn permutations(entries: Vec<(i64, String)>) -> Vec<Vec<(i64, String)>> {
        match entries.len() {
            0 | 1 => vec![entries],
            _ => {
                let mut all = Vec::new();
                for index in 0..entries.len() {
                    let mut rest = entries.clone();
                    let first = rest.remove(index);
                    for mut tail in permutations(rest) {
                        tail.insert(0, first.clone());
                        all.push(tail);
                    }
                }
                all
            }
        }
    }

    #[test]
    fn space_queue_head_is_the_least_ord_on_both_sides() {
        let entries = vec![
            (4, "d".to_string()),
            (1, "a".to_string()),
            (3, "c".to_string()),
            (2, "b".to_string()),
        ];
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(head_of(&entries, side), Some((1, "a".to_string())));
        }
    }

    #[test]
    fn space_queue_head_ignores_the_list_order() {
        let entries = vec![
            (7, "x".to_string()),
            (2, "y".to_string()),
            (9, "z".to_string()),
            (5, "w".to_string()),
        ];
        let orders = permutations(entries);
        assert_eq!(orders.len(), 24);
        for order in orders {
            assert_eq!(head_of(&order, Side::Data), Some((2, "y".to_string())));
        }
    }

    #[test]
    fn space_queue_equal_ords_order_by_their_bytes() {
        let entries = vec![
            (3, "b".to_string()),
            (3, "a".to_string()),
            (3, "c".to_string()),
            (4, "0".to_string()),
        ];
        let expected = entries
            .iter()
            .filter(|(ord, _)| *ord == 3)
            .min_by_key(|(_, entry)| bincode::serialize(entry).expect("strings serialize"))
            .cloned();
        assert_eq!(expected, Some((3, "a".to_string())));
        for order in permutations(entries) {
            assert_eq!(head_of(&order, Side::Waiters), expected);
        }
        let duplicated = vec![(1, "s".to_string()), (1, "s".to_string())];
        assert_eq!(
            QueueDiscipline.head(Side::Data, &ordered(&duplicated)),
            Ok(Some(0))
        );
    }

    #[test]
    fn space_queue_empty_side_has_no_head() {
        for side in [Side::Data, Side::Waiters] {
            assert_eq!(QueueDiscipline.head::<String>(side, &[]), Ok(None));
        }
    }

    #[test]
    fn space_queue_incoming_elements_reach_the_head_of_an_empty_side_only() {
        for side in [Side::Data, Side::Waiters] {
            assert!(QueueDiscipline.reaches_head(side, 0));
            for queued in [1, 2, usize::MAX] {
                assert!(!QueueDiscipline.reaches_head(side, queued));
            }
        }
    }

    #[test]
    fn space_queue_discipline_uses_a_counter_without_joins_or_params() {
        assert_eq!(QueueDiscipline::NAME, "queue");
        assert!(QueueDiscipline::USES_COUNTER);
        assert!(!QueueDiscipline::JOINS);
        assert!(QueueDiscipline::SEND_PARAMS.is_empty());
    }
}
