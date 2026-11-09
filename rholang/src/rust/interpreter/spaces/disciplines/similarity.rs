//! Added by decision 128 (kind routing): the similarity window of the join engine (P6, design
//! v3 4.4 and 4.6). A discipline with `JoinWindow::Similarity` gives the steps of the window
//! through the `Discipline` methods from `Discipline::similarity_config` on, and the engine names
//! only the types of this module. The VectorDB discipline is the one discipline of the window,
//! and it re-exports these types under its own names.

use models::rhoapi::ListParWithRandom;
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::trace::event::Produce;

use crate::rust::interpreter::spaces::generic::ParDatum;
use crate::rust::interpreter::tensor::exact::{Bound, Metric, Query, Score};
use crate::rust::interpreter::tensor::num::ExactVector;

/// The key of the query vector in the policy of a parked waiter.
pub const QUERY_VECTOR_KEY: &str = "q";

/// The length of a record id: the hash of the source produce of the record.
pub const ID_LEN: usize = 32;

/// The error of a similarity bind in a space without this window.
pub const BIND_OUTSIDE_WINDOW: &str = "a similarity bind outside a VectorDB space";

/// The error of a list waiter in a space without this window.
pub const LIST_OUTSIDE_WINDOW: &str = "a list waiter outside a VectorDB space";

/// The error of a record of this window that the engine would build without the params of its
/// send: the record holds the vector of the send.
pub const RECORD_FROM_SEND_PARAMS: &str = "a VectorDB record is built from the params of its send";

/// The params of a space of the similarity window (design v3 4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SimilarityConfig {
    /// The most records of each collection.
    pub capacity: usize,
    /// The length of every vector of the space.
    pub dimensions: usize,
    /// The metric of a query without `metric`.
    pub metric: Metric,
}

/// The mode of a query: one record, or a list of `k` records (`None` for all of them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// One record.
    Single,
    /// The list of the first `k` eligible records, or of all of them for `None`.
    List(Option<usize>),
}

/// A checked query of a receive (design v3 4.4).
pub struct SimilarityQuery {
    /// The query vector with its metric, or `None` for a read in canonical order.
    pub exact: Option<Query>,
    /// The `threshold` or the `radius`, or `None`.
    pub bound: Option<Bound>,
    /// Single mode or list mode.
    pub mode: Mode,
    /// True for `scores = true`.
    pub scores: bool,
    /// The ids of `ids`, sorted and without duplicates, or `None`.
    pub ids: Option<Vec<[u8; ID_LEN]>>,
}

/// An element in flight as a selection sees it: its record, its payload, the identity of its
/// produce, its persistence and its vector.
pub struct Flight<'a> {
    /// The record: the envelope with its vector, then the payload.
    pub record: &'a ListParWithRandom,
    /// The payload of the record.
    pub payload: &'a ListParWithRandom,
    /// The identity of the produce of the record.
    pub source: &'a Produce,
    /// True for a persistent element.
    pub persist: bool,
    /// The vector of the record, or `None`.
    pub vector: Option<&'a ExactVector>,
}

/// An eligible record: its index in the staged view of its channel, or `None` for the element
/// in flight, and its score (`None` for a query without a vector).
pub struct Eligible {
    /// The index in the staged view, or `None` for the element in flight.
    pub index: Option<usize>,
    /// The score, or `None` for a query without a vector.
    pub score: Option<Score>,
}

/// Added by decision 102 (plan v4 C5.2, rules CR-1, CR-3 and CR-4): the order in which the
/// selection (`Discipline::eligible`) reads the records, before a query with a vector ranks them
/// by score.
#[derive(Clone, Copy, Debug)]
pub enum EligibleOrder<'s> {
    /// The canonical order of DR-75. A list keeps it for its members and its ties (Q-102-1),
    /// and so does a join bind on a collection that the join names more than once (E1).
    Canonical,
    /// The order of a pick of the operation `source` (`Cx::pick_data_order`): a single-mode
    /// query, and a join bind on a collection that the join names once. The ties of a score
    /// keep the pick order. Play shuffles them, and replay puts the named record first.
    Pick(RSpaceOperationSource<'s>),
}

/// A list that a list-mode bind binds: the values that the pattern binds, and the members of
/// the list in delivery order, each with its index in the staged view, or `None` for the element
/// in flight.
pub struct ListFire {
    /// The values that the pattern binds.
    pub matched: ListParWithRandom,
    /// The members of the list in delivery order: the stored record and its index, or `None`
    /// for the element in flight.
    pub members: Vec<Option<(ParDatum, usize)>>,
}

impl ListFire {
    /// Added by decision 124 (plan v4 C7.9, ruling RADD 1.7): the values that the pattern binds
    /// and the stored members of the list in list order, without the place of the element: the
    /// form that the staging guard and the list retires read.
    pub fn into_stored(self) -> (ListParWithRandom, Vec<ParDatum>) {
        let mut others = Vec::<ParDatum>::with_capacity(self.members.len());
        others.extend(self.members.into_iter().flatten().map(|(record, _)| record));
        (self.matched, others)
    }
}
