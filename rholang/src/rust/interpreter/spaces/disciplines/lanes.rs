//! Added by decision 128 (kind routing): the lanes window of the join engine (decision 38,
//! P5c). A discipline with `JoinWindow::Lanes` gives the steps of the window through
//! `Discipline::send_level` and `Discipline::receive_lane`, and the engine names only the types
//! of this module. The priority queue is the one discipline of the window, and it re-exports
//! these types.

/// The level of a record of a priority queue. The derived order puts the priority levels in
/// ascending order and the unprioritized level after them (F6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Level {
    /// The level `p ≥ 0` of a send with `priority = p`.
    Priority(i64),
    /// The level of a send without a priority.
    Unprioritized,
}

impl Level {
    /// The level of the priority slot `slot` of a datum record: `Some(p)` is the level `p`, and
    /// `None` is the unprioritized level.
    pub const fn of_slot(slot: Option<i64>) -> Self {
        match slot {
            Some(level) => Level::Priority(level),
            None => Level::Unprioritized,
        }
    }

    /// The priority slot of the level.
    pub const fn slot(self) -> Option<i64> {
        match self {
            Level::Priority(level) => Some(level),
            Level::Unprioritized => None,
        }
    }

    /// The waiter lanes whose heads a datum of this level meets in a produce plan: the waiter
    /// queue of the level for a priority level, and the unrestricted queue (decision 38). The
    /// unprioritized level has no waiter queue of its own.
    pub const fn candidate_lanes(self) -> [Option<Lane>; 2] {
        match self {
            Level::Priority(level) => [Some(Lane::Level(level)), Some(Lane::Any)],
            Level::Unprioritized => [None, Some(Lane::Any)],
        }
    }
}

/// The lane of a waiting continuation of a priority queue (decision 38).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lane {
    /// The waiter queue of the level `p`: a receive with `priority = p`.
    Level(i64),
    /// The unrestricted waiter queue: a receive without a priority.
    Any,
}

impl Lane {
    /// The lane of the policy slot `slot` of a parked continuation: `Some(p)` is the waiter queue
    /// of the level `p`, and `None` is the unrestricted queue.
    pub const fn of_slot(slot: Option<i64>) -> Self {
        match slot {
            Some(level) => Lane::Level(level),
            None => Lane::Any,
        }
    }

    /// The policy slot of the lane.
    pub const fn slot(self) -> Option<i64> {
        match self {
            Lane::Level(level) => Some(level),
            Lane::Any => None,
        }
    }

    /// True when a waiter of this lane sees the records of `level`. A level lane sees its own
    /// level only, and the unrestricted lane sees every level (decision 38).
    pub const fn sees(self, level: Level) -> bool {
        match (self, level) {
            (Lane::Any, _) => true,
            (Lane::Level(own), Level::Priority(other)) => own == other,
            (Lane::Level(_), Level::Unprioritized) => false,
        }
    }
}
