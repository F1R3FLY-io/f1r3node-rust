use std::collections::BTreeSet;
use std::fmt::Debug;
use std::hash::Hash;

use serde::Serialize;

use super::RSpace;
use crate::rspace::errors::RSpaceError;
use crate::rspace::trace::event::{COMM, Consume, Produce};

impl<C, P, A, K> RSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    pub(super) fn observe_comm(
        &self,
        comm: &COMM,
        continuation: &K,
        continuation_persistent: bool,
        data_candidates: &[crate::rspace::internal::ConsumeCandidate<C, A>],
    ) -> Result<(), RSpaceError> {
        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
        if let Some(observer) = observer {
            let data = data_candidates
                .iter()
                .map(|candidate| (&candidate.datum.a, candidate.datum.persist))
                .collect::<Vec<_>>();
            observer.observe_comm(comm, continuation, continuation_persistent, &data)?;
        }
        Ok(())
    }

    pub(super) fn observe_produce(
        &self,
        source: &Produce,
        channel: &C,
        data: &A,
        persistent: bool,
    ) -> Result<(), RSpaceError> {
        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
        if let Some(observer) = observer {
            observer.observe_produce(source, channel, data, persistent)?;
        }
        Ok(())
    }

    pub(super) fn observe_consume(
        &self,
        source: &Consume,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        persistent: bool,
        peeks: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        let observer = self
            .accounting_observer
            .read()
            .expect("accounting observer read lock")
            .clone();
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
