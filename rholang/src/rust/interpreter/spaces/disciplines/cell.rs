//! The cell discipline of the native spaces (P5 design v3 A.7, F15).
//!
//! A cell holds at most one record and at most one waiting continuation (F15). A store or a park
//! on a full side fails with `CellOccupied` before it stages anything (`Cx::check_capacity`). The
//! job then drops the transaction, so the failed operation applies nothing and leaves the counter
//! unchanged (decision 19, D6). The legacy cell failed only a second `put` (`CellAlreadyFull`) and
//! kept its waiters in a bag. Here the waiter side has one slot too.
//!
//! The cell runs the head driver of the engine (`offer_head`, `receive_head`) with these rules:
//!
//! * Every operation bumps the collection counter (decision 31, M-1). So two branches that
//!   operate on one cell race under merge check #1, and a merge never puts two records into one
//!   cell.
//! * A fresh element always reaches the head of its side ([`Discipline::reaches_head`]). A send
//!   meets the waiter of the cell, and a receive meets the record of the cell. An element that
//!   does not fire is stored or parked, and that needs a free slot.
//! * A re-issued persistent element keeps the order of its first round, so it keeps its record
//!   and its identity (decision 43). It re-enters as a fresh element ([`Discipline::precedes`],
//!   section 43.8 of the design of decision 43). An older entry on its side does not hold it
//!   back, and a store or a park on a full slot fails with `CellOccupied`.
//!
//! The head of a side is the entry with the least `(ord, bincode bytes)`, as for a queue. A
//! well-formed cell holds at most one entry on each side, so the head is that entry. Only the head
//! interacts (decision 11). On the produce side, a guard refusal of the waiter stores the datum.
//! On the consume side, the guard is checked once on the record (decision 37).
//!
//! A consume-side peek fire removes a non-persistent record, as the host COMM does. The default
//! restore then stores the same record again in the same transaction (decision 45, Rule E). The
//! fire left the record slot empty, so the restore never meets a full slot
//! (`cell_restore_never_occupied` in `CellDiscipline.v`). A produce-side peek waiter keeps the
//! datum in flight (decision 30). The cell holds no other waiter, so the datum is then stored, or
//! the operation fails with `CellOccupied` when the record slot is full.
//!
//! Each side holds at most one entry, so a cell operation fires at most once and never reaches
//! `FIRE_ROUND_CAP`. A divergent pair re-issues after one round (A10): a persistent send with a
//! persistent waiter, or a persistent receive with a persistent record.
//!
//! The cell takes no send params and no query params. Its head scan is the scan of a queue. Each
//! capacity check compares the size of a side with its bound, and the re-entry rule compares
//! nothing.

use serde::Serialize;

use crate::rust::interpreter::spaces::disciplines::queue::QueueDiscipline;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::{Discipline, Ordered, Side};

/// The cell discipline: one record slot and one waiter slot (F15).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct CellDiscipline;

impl Discipline for CellDiscipline {
    const NAME: &'static str = "cell";
    const USES_COUNTER: bool = true;
    const SEND_PARAMS: &'static [&'static str] = &[];
    const CAPACITY: Option<usize> = Some(1);

    /// The entry with the least `(ord, bincode bytes)`, on both sides. The rule is the rule of
    /// the queue ([`QueueDiscipline::head`]). A well-formed cell holds at most one entry on each
    /// side, so the head is that entry. On any side, the head is a function of the entry set.
    fn head<T: Serialize>(
        &self,
        side: Side,
        entries: &[Ordered<'_, T>],
    ) -> Result<Option<usize>, SpaceError> {
        QueueDiscipline.head(side, entries)
    }

    /// A fresh element always reaches the head of its side. A send meets the waiter of the cell,
    /// and a receive meets the record of the cell. The count of queued entries plays no part.
    fn reaches_head(&self, _side: Side, _queued: usize) -> bool { true }

    /// A re-issued persistent element re-enters as a fresh element (decision 43, section 43.8 of
    /// its design). It keeps the order of its first round, and it reaches the head also when an
    /// older entry holds its side. The rule compares nothing.
    fn precedes(&self, _side: Side, _ord: i64, _head: i64) -> Result<bool, SpaceError> { Ok(true) }
}

#[cfg(test)]
mod tests {

    use super::*;

    /// The entries as borrowed `Ordered` views, in list order.
    fn ordered(entries: &[(i64, String)]) -> Vec<Ordered<'_, String>> {
        entries
            .iter()
            .map(|(ord, entry)| Ordered { ord: *ord, entry })
            .collect()
    }

    /// The head entry of `entries` on `side` under `discipline`.
    fn head_of<D: Discipline>(
        discipline: &D,
        entries: &[(i64, String)],
        side: Side,
    ) -> Option<(i64, String)> {
        discipline
            .head(side, &ordered(entries))
            .expect("strings serialize")
            .and_then(|index| entries.get(index).cloned())
    }

    /// Every order of `entries`.
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
    fn space_cell_discipline_has_one_slot_a_counter_and_no_params() {
        assert_eq!(CellDiscipline::NAME, "cell");
        assert!(
            CellDiscipline::USES_COUNTER,
            "every cell operation bumps the counter"
        );
        assert_eq!(
            CellDiscipline::CAPACITY,
            Some(1),
            "one record and one waiter (F15)"
        );
        assert!(CellDiscipline::SEND_PARAMS.is_empty());
        assert!(CellDiscipline::QUERY_PARAMS.is_empty());
    }

    #[test]
    fn space_cell_head_is_the_least_ord_and_bytes_as_for_a_queue() {
        let entries = vec![
            (3, "b".to_string()),
            (3, "a".to_string()),
            (5, "c".to_string()),
            (4, "0".to_string()),
        ];
        let expected = entries
            .iter()
            .filter(|(ord, _)| *ord == 3)
            .min_by_key(|(_, entry)| bincode::serialize(entry).expect("strings serialize"))
            .cloned();
        assert_eq!(expected, Some((3, "a".to_string())));
        let orders = permutations(entries);
        assert_eq!(orders.len(), 24);
        for order in orders {
            for side in [Side::Data, Side::Waiters] {
                assert_eq!(head_of(&CellDiscipline, &order, side), expected);
                assert_eq!(
                    head_of(&CellDiscipline, &order, side),
                    head_of(&QueueDiscipline, &order, side)
                );
            }
        }
        // A well-formed cell holds one entry on a side at most, so the head is that entry.
        for side in [Side::Data, Side::Waiters] {
            let sole = vec![(7, "x".to_string())];
            assert_eq!(
                head_of(&CellDiscipline, &sole, side),
                Some((7, "x".to_string()))
            );
            assert_eq!(CellDiscipline.head::<String>(side, &[]), Ok(None));
        }
    }

    #[test]
    fn space_cell_head_scan_is_the_queue_head_scan() {
        let entries = vec![
            (2, "tie-b".to_string()),
            (2, "tie-a".to_string()),
            (1, "older".to_string()),
        ];
        let cell = CellDiscipline
            .head(Side::Data, &ordered(&entries))
            .expect("the scan gives a head");
        let queue = QueueDiscipline
            .head(Side::Data, &ordered(&entries))
            .expect("the scan gives a head");
        assert_eq!(cell, queue);
    }

    #[test]
    fn space_cell_every_element_reaches_the_head() {
        for side in [Side::Data, Side::Waiters] {
            for queued in [0, 1, 2, usize::MAX] {
                assert!(
                    CellDiscipline.reaches_head(side, queued),
                    "a fresh element meets the other side ({side:?}, {queued})"
                );
            }
            for (ord, head) in [(0, 1), (1, 0), (5, 9), (9, 5), (i64::MAX, 0)] {
                assert_eq!(
                    CellDiscipline.precedes(side, ord, head),
                    Ok(true),
                    "a re-entered element is at the head ({side:?}, {ord}, {head})"
                );
            }
        }
        // Negative control: the rule of the queue holds an element behind an older head.
        assert_eq!(QueueDiscipline.precedes(Side::Data, 5, 2), Ok(false));
        assert!(!QueueDiscipline.reaches_head(Side::Data, 1));
    }
}
