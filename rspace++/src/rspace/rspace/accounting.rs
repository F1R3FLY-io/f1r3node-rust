// Added by G3 (DR-135, S5): an owner check keeps its first hard error.
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
// Added by G3 (DR-135, S5): the owner checks of a match.
use crate::rspace::internal::{ConsumeCandidate, WaitingContinuation};
use crate::rspace::operation_context::MatchAdmission;
// Changed by G3 (DR-135, S5): the COMM observation names its participants.
// use crate::rspace::rspace_interface::{
//     AccountingStart, RSpaceAccountingObserver, RSpaceOperationSource,
// };
use crate::rspace::rspace_interface::{
    AccountingStart, CommParticipant, RSpaceAccountingObserver, RSpaceOperationSource,
};
use crate::rspace::trace::event::{COMM, Consume, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    // Changed by DR-132: the start returns the observer and the name of the
    // recorded operation. The locked operation runs its rest under that name.
    // ) -> Result<Option<Arc<dyn RSpaceAccountingObserver<C, P, A, K>>>,
    // RSpaceError> {     ...
    //     if let Some(observer) = &observer {
    //         observer.observe_operation_start(source, channels, joins)?;
    //     }
    //     Ok(observer)
    pub(super) fn start_accounting_operation(
        &self,
        source: RSpaceOperationSource<'_>,
        channels: &[C],
        joins: &[Vec<C>],
    ) -> Result<AccountingStart<C, P, A, K>, RSpaceError> {
        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
        let name = match &observer {
            Some(observer) => observer.observe_operation_start(source, channels, joins)?,
            None => None,
        };
        Ok((observer, name))
    }

    pub(super) fn observe_comm(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        comm: &COMM,
        continuation: &K,
        continuation_persistent: bool,
        data_candidates: &[crate::rspace::internal::ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        // Changed by G3 (DR-135, S5): the observer sees the channel and the
        // stored value of each datum (R9). Its default observes the COMM as
        // before.
        // if let Some(observer) = observer {
        //     let data = data_candidates
        //         .iter()
        //         .map(|candidate| (&candidate.datum.a, candidate.datum.persist))
        //         .collect::<Vec<_>>();
        //     observer.observe_comm(
        //         comm, continuation, continuation_persistent, &data,
        //     )?;
        // }
        if let Some(observer) = observer {
            let mut participants = Vec::with_capacity(data_candidates.len());
            participants.extend(data_candidates.iter().map(|candidate| CommParticipant {
                channel: &candidate.channel,
                matched: &candidate.datum.a,
                stored: &candidate.removed_datum,
                persistent: candidate.datum.persist,
            }));
            observer.observe_comm_participants(
                comm,
                continuation,
                continuation_persistent,
                &participants,
            )?;
        }
        Ok(())
    }

    pub(super) fn observe_produce(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        source: &Produce,
        channel: &C,
        data: &A,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        if let Some(observer) = observer {
            observer.observe_produce(source, channel, data, persistent)?;
        }
        Ok(())
    }

    pub(super) fn observe_consume(
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        source: &Consume,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        if let Some(observer) = observer {
            observer.observe_consume(
                source,
                channels,
                patterns,
                continuation,
                persistent,
                peeks,
            )?;
        }
        Ok(())
    }
}

/// Added by G3 (DR-135, S5): the owner checks of one produce (R5, R6). The
/// item test asks the observer. The tuple test observes the COMM of the
/// tuple, so the observer charges it or refuses it with
/// `RSpaceError::CommRefused`. A refusal rolls the tuple back. The first
/// other error is kept, and every later check fails, so the produce returns
/// that error after the search.
pub(super) struct ProduceAdmission<'a, C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    space: &'a RSpace<C, P, A, K>,
    observer: &'a dyn RSpaceAccountingObserver<C, P, A, K>,
    produce_ref: &'a Produce,
    persistent: bool,
    channel: &'a C,
    error: RefCell<Option<RSpaceError>>,
}

impl<'a, C, P, A, K> ProduceAdmission<'a, C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn new(
        space: &'a RSpace<C, P, A, K>,
        observer: &'a dyn RSpaceAccountingObserver<C, P, A, K>,
        produce_ref: &'a Produce,
        persistent: bool,
        channel: &'a C,
    ) -> Self {
        Self {
            space,
            observer,
            produce_ref,
            persistent,
            channel,
            error: RefCell::new(None),
        }
    }

    /// The hard error that a check kept, if any.
    pub(super) fn take_error(&self) -> Option<RSpaceError> { self.error.borrow_mut().take() }

    fn failed(&self) -> bool { self.error.borrow().is_some() }

    fn keep(&self, error: RSpaceError) { self.error.borrow_mut().get_or_insert(error); }
}

impl<C, P, A, K> MatchAdmission<C, P, A, K> for ProduceAdmission<'_, C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + PartialEq + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> bool {
        if self.failed() {
            return false;
        }
        match self.observer.admit_item(
            continuation,
            continuation_persistent,
            channel_count,
            datum,
            datum_persistent,
        ) {
            Ok(admitted) => admitted,
            Err(error) => {
                self.keep(error);
                false
            }
        }
    }

    fn own_datum_channel(&self) -> Option<&C> { Some(self.channel) }

    fn admit_tuple(
        &self,
        waiting: &WaitingContinuation<P, K>,
        candidates: &[ConsumeCandidate<C, A>],
    ) -> bool {
        if self.failed() {
            return false;
        }
        let comm = self.space.produce_comm(
            candidates,
            &waiting.source,
            &waiting.peeks,
            self.produce_ref,
            self.persistent,
        );
        match RSpace::observe_comm(
            Some(self.observer),
            &comm,
            &waiting.continuation,
            waiting.persist,
            candidates,
        ) {
            Ok(()) => true,
            Err(RSpaceError::CommRefused) => false,
            Err(error) => {
                self.keep(error);
                false
            }
        }
    }
}

/// Added by G3 (DR-135, S5): the item test of one consume (R5). A consume
/// observes its COMM after the search, so it needs no tuple test here. The
/// first hard error is kept, as in `ProduceAdmission`.
pub(super) struct ConsumeAdmission<'a, C, P, A, K> {
    pub(super) observer: &'a dyn RSpaceAccountingObserver<C, P, A, K>,
    pub(super) error: RefCell<Option<RSpaceError>>,
}

impl<C, P: Clone + PartialEq, A: Clone, K: Clone + PartialEq> MatchAdmission<C, P, A, K>
    for ConsumeAdmission<'_, C, P, A, K>
{
    fn admit_item(
        &self,
        continuation: &K,
        continuation_persistent: bool,
        channel_count: usize,
        datum: &A,
        datum_persistent: bool,
    ) -> bool {
        if self.error.borrow().is_some() {
            return false;
        }
        match self.observer.admit_item(
            continuation,
            continuation_persistent,
            channel_count,
            datum,
            datum_persistent,
        ) {
            Ok(admitted) => admitted,
            Err(error) => {
                self.error.borrow_mut().get_or_insert(error);
                false
            }
        }
    }

    fn own_datum_channel(&self) -> Option<&C> { None }

    fn admit_tuple(&self, _: &WaitingContinuation<P, K>, _: &[ConsumeCandidate<C, A>]) -> bool {
        true
    }
}
