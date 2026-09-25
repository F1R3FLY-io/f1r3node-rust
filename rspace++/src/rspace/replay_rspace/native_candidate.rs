use super::native_epoch::NativeCandidateIdentity;
use super::*;
use crate::rspace::space_matcher::deterministic_candidates;

pub(super) struct PreparedConsumeCandidate<C, A: Clone> {
    pub(super) data: Vec<ConsumeCandidate<C, A>>,
    pub(super) comm: COMM,
}

pub(super) struct PreparedProduceCandidate<C, P: Clone, A: Clone, K: Clone> {
    pub(super) candidate: ProduceCandidate<C, P, A, K>,
    pub(super) comm: COMM,
}

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn candidate_matches(
        &self,
        datum: &Datum<A>,
        expected: &dyn NativeCandidateIdentity,
        incoming: Option<&Produce>,
    ) -> bool {
        if !expected.matches_produce(&datum.source) {
            return false;
        }
        if datum.persist {
            return true;
        }
        let count = self.get_produce_count(&datum.source);
        let count = if incoming.is_some_and(|p| p == &datum.source) {
            count.checked_add(1)
        } else {
            Some(count)
        };
        count.is_some_and(|count| expected.repetition(&datum.source) == Some(count))
    }

    pub(super) fn prepare_native_consume_candidate(
        &self,
        channels: &[C],
        patterns: &[P],
        continuation: &K,
        consume_ref: &Consume,
        peeks: &BTreeSet<i32>,
        expected: Option<&dyn NativeCandidateIdentity>,
    ) -> Option<PreparedConsumeCandidate<C, A>> {
        if expected.is_some_and(|identity| !identity.matches_consume(consume_ref)) {
            return None;
        }
        let mut candidates: HashMap<_, _> = channels
            .iter()
            .map(|channel| {
                (channel.clone(), deterministic_candidates(self.get_store().get_data(channel)))
            })
            .collect();
        if let Some(expected) = expected {
            for entries in candidates.values_mut() {
                entries.retain(|(datum, _)| self.candidate_matches(datum, expected, None));
            }
        }
        let pairs: Vec<_> = channels
            .iter()
            .cloned()
            .zip(patterns.iter().cloned())
            .collect();
        let data: Vec<_> = self
            .extract_data_candidates(&self.matcher, &pairs, &mut candidates)
            .into_iter()
            .collect::<Option<Vec<_>>>()?;
        let matched: Vec<_> = data
            .iter()
            .map(|candidate| candidate.datum.a.clone())
            .collect();
        if !self.matcher.check_commit(continuation, &matched) {
            return None;
        }
        let comm = COMM::new(&data, consume_ref.clone(), peeks.clone(), |produces| {
            self.produce_counters(produces)
        });
        if expected.is_some_and(|identity| !identity.matches_comm(&comm)) {
            return None;
        }
        Some(PreparedConsumeCandidate { data, comm })
    }

    pub(super) fn prepare_native_produce_candidate(
        &self,
        channel: &C,
        data: &A,
        persist: bool,
        produce_ref: &Produce,
        grouped_channels: Vec<Vec<C>>,
        expected: Option<&dyn NativeCandidateIdentity>,
    ) -> Result<Option<PreparedProduceCandidate<C, P, A, K>>, RSpaceError> {
        let next_count = if persist {
            None
        } else {
            Some(
                self.get_produce_count(produce_ref)
                    .checked_add(1)
                    .ok_or_else(|| {
                        RSpaceError::InterpreterError(
                            "Native replay produce counter overflow".to_owned(),
                        )
                    })?,
            )
        };
        let candidate = self.run_matcher_for_channels(
            grouped_channels,
            |channels| {
                deterministic_candidates(self.get_store().get_continuations_arc(&channels))
                    .into_iter()
                    .filter(|(waiting, _)| {
                        expected.is_none_or(|identity| identity.matches_consume(&waiting.source))
                    })
                    .collect()
            },
            |c| {
                let mut entries = deterministic_candidates(self.get_store().get_data(&c));
                if &c == channel {
                    entries.insert(
                        0,
                        (
                            Datum {
                                a: data.clone(),
                                persist,
                                source: produce_ref.clone(),
                            },
                            -1,
                        ),
                    );
                }
                if let Some(expected) = expected {
                    entries.retain(|(datum, _)| {
                        self.candidate_matches(
                            datum,
                            expected,
                            if persist { None } else { Some(produce_ref) },
                        )
                    });
                }
                (c, entries)
            },
        );
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        let comm = COMM::new(
            &candidate.data_candidates,
            candidate.continuation.source.clone(),
            candidate.continuation.peeks.clone(),
            |produces| {
                let mut counters = self.produce_counters(produces);
                if let Some(count) = next_count {
                    if produces.contains(produce_ref) {
                        counters.insert(produce_ref.clone(), count);
                    }
                }
                counters
            },
        );
        if expected.is_some_and(|identity| !identity.matches_comm(&comm)) {
            return Ok(None);
        }
        Ok(Some(PreparedProduceCandidate { candidate, comm }))
    }
}

#[cfg(test)]
mod tests;
