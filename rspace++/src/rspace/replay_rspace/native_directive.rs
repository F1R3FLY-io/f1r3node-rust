use super::native_epoch::NativeCandidateIdentity;
use super::*;
use crate::rspace::rspace_interface::ReplayOperationDirective;

fn mismatch() -> RSpaceError {
    RSpaceError::InterpreterError("Native replay directive does not match execution".to_string())
}

pub(super) fn exact_produce(actual: &Produce, expected: &Produce) -> bool {
    actual.hash == expected.hash &&
        actual.channel_hash == expected.channel_hash &&
        actual.persistent == expected.persistent &&
        actual.is_deterministic == expected.is_deterministic &&
        actual.output_value == expected.output_value &&
        actual.failed == expected.failed
}

fn exact_comm(actual: &COMM, expected: &COMM) -> bool {
    compare_comm(actual, expected, exact_produce)
}

struct LegacyCandidateIdentity<'a>(&'a COMM);

impl NativeCandidateIdentity for LegacyCandidateIdentity<'_> {
    fn matches_consume(&self, source: &Consume) -> bool { source == &self.0.consume }

    fn matches_produce(&self, source: &Produce) -> bool {
        self.0
            .produces
            .iter()
            .any(|expected| exact_produce(source, expected))
    }

    fn repetition(&self, source: &Produce) -> Option<i32> {
        self.0.times_repeated.get(source).copied()
    }

    fn matches_comm(&self, source: &COMM) -> bool { exact_comm(source, self.0) }
}

fn compare_comm(
    actual: &COMM,
    expected: &COMM,
    source: impl Fn(&Produce, &Produce) -> bool,
) -> bool {
    actual.consume == expected.consume &&
        actual.peeks == expected.peeks &&
        actual.produces.len() == expected.produces.len() &&
        actual
            .produces
            .iter()
            .zip(&expected.produces)
            .all(|(a, e)| source(a, e)) &&
        actual.times_repeated.len() == expected.times_repeated.len() &&
        actual
            .times_repeated
            .iter()
            .zip(&expected.times_repeated)
            .all(|((a, n), (e, m))| n == m && source(a, e))
}

fn require_grant(result: Result<(), RSpaceError>) -> Result<(), RSpaceError> {
    result.map_err(|error| match error {
        RSpaceError::OutOfPhlogistons => mismatch(),
        other => other,
    })
}

fn require_denial<T>(result: Result<(), RSpaceError>) -> Result<T, RSpaceError> {
    match result {
        Err(RSpaceError::OutOfPhlogistons) => Err(RSpaceError::OutOfPhlogistons),
        Err(other) => Err(other),
        Ok(()) => Err(mismatch()),
    }
}

impl<C, P, A, K> ReplayRSpace<C, P, A, K>
where
    C: Clone + Debug + Default + Serialize + Hash + Ord + Eq + 'static + Sync + Send,
    P: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    A: Clone + Debug + Default + Serialize + 'static + Sync + Send,
    K: Clone + Debug + Default + Serialize + 'static + Sync + Send,
{
    fn directive_comm(directive: ReplayOperationDirective) -> (Arc<COMM>, bool) {
        match directive {
            ReplayOperationDirective::AcceptedComm(comm) => (comm, false),
            ReplayOperationDirective::RejectedComm(comm) => (comm, true),
            ReplayOperationDirective::Store | ReplayOperationDirective::RejectedIntroduction => {
                unreachable!()
            }
        }
    }

    fn native_binding(&self, expected: &COMM) -> Result<COMM, RSpaceError> {
        let bindings = self.replay_data.lock().expect("replay data lock");
        let sources = std::iter::once(IOEvent::Consume(expected.consume.clone()))
            .chain(expected.produces.iter().cloned().map(IOEvent::Produce));
        for source in sources {
            if let Some(comms) = bindings.map.get(&source) {
                if let Some((comm, _)) = comms.iter().find(|(comm, count)| {
                    **count > 0 &&
                        compare_comm(comm, expected, |a, e| {
                            a.hash == e.hash &&
                                a.channel_hash == e.channel_hash &&
                                a.persistent == e.persistent
                        })
                }) {
                    return Ok(comm.clone());
                }
            }
        }
        Err(mismatch())
    }

    pub(super) fn replay_native_consume(
        &self,
        channels: Vec<C>,
        patterns: Vec<P>,
        continuation: K,
        persist: bool,
        peeks: BTreeSet<i32>,
        consume_ref: Consume,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        directive: ReplayOperationDirective,
    ) -> Result<MaybeConsumeResult<C, P, A, K>, RSpaceError> {
        if matches!(directive, ReplayOperationDirective::Store) {
            require_grant(Self::observe_consume(
                observer,
                &consume_ref,
                &channels,
                &patterns,
                &continuation,
                persist,
                &peeks,
            ))?;
            if self
                .prepare_native_consume_candidate(
                    &channels,
                    &patterns,
                    &continuation,
                    &consume_ref,
                    &peeks,
                    None,
                )
                .is_some()
            {
                return Err(mismatch());
            }
            self.log_consume(
                consume_ref.clone(),
                &channels,
                &patterns,
                &continuation,
                persist,
                &peeks,
            );
            return Ok(self.store_waiting_continuation(channels, WaitingContinuation {
                patterns,
                continuation,
                persist,
                peeks,
                source: consume_ref,
            }));
        }
        if matches!(directive, ReplayOperationDirective::RejectedIntroduction) {
            return require_denial(Self::observe_consume(
                observer,
                &consume_ref,
                &channels,
                &patterns,
                &continuation,
                persist,
                &peeks,
            ));
        }
        let (expected, denied) = Self::directive_comm(directive);
        if expected.consume != consume_ref || expected.peeks != peeks {
            return Err(mismatch());
        }
        require_grant(Self::observe_consume(
            observer,
            &consume_ref,
            &channels,
            &patterns,
            &continuation,
            persist,
            &peeks,
        ))?;
        let prepared = self
            .prepare_native_consume_candidate(
                &channels,
                &patterns,
                &continuation,
                &consume_ref,
                &peeks,
                Some(&LegacyCandidateIdentity(&expected)),
            )
            .ok_or_else(mismatch)?;
        let data = prepared.data;
        let actual = prepared.comm;
        if !denied {
            self.native_binding(&actual)?;
        }
        let observed = Self::observe_comm(observer, &actual, &continuation, persist, &data);
        if denied {
            return require_denial(observed);
        }
        require_grant(observed)?;
        let waiting = WaitingContinuation {
            patterns,
            continuation,
            persist,
            peeks,
            source: consume_ref.clone(),
        };
        self.log_consume(
            consume_ref.clone(),
            &channels,
            &waiting.patterns,
            &waiting.continuation,
            persist,
            &waiting.peeks,
        );
        self.log_comm(&data, &channels, waiting.clone(), actual.clone(), "comm.consume");
        self.store_persistent_data(&data);
        self.remove_bindings_for(actual);
        Ok(self.wrap_result(channels, waiting, consume_ref, data))
    }

    pub(super) fn replay_native_produce(
        &self,
        channel: C,
        data: A,
        persist: bool,
        produce_ref: Produce,
        grouped_channels: Vec<Vec<C>>,
        observer: Option<&dyn RSpaceAccountingObserver<C, P, A, K>>,
        directive: ReplayOperationDirective,
    ) -> Result<MaybeProduceResult<C, P, A, K>, RSpaceError> {
        if matches!(directive, ReplayOperationDirective::Store) {
            require_grant(Self::observe_produce(observer, &produce_ref, &channel, &data, persist))?;
            if self
                .prepare_native_produce_candidate(
                    &channel,
                    &data,
                    persist,
                    &produce_ref,
                    grouped_channels,
                    None,
                )?
                .is_some()
            {
                return Err(mismatch());
            }
            self.increment_produce_counter(&produce_ref, persist);
            self.log_produce(produce_ref.clone(), &channel, &data, persist);
            return Ok(self.store_data(channel, data, persist, produce_ref));
        }
        if matches!(directive, ReplayOperationDirective::RejectedIntroduction) {
            return require_denial(Self::observe_produce(
                observer,
                &produce_ref,
                &channel,
                &data,
                persist,
            ));
        }
        let (expected, denied) = Self::directive_comm(directive);
        require_grant(Self::observe_produce(observer, &produce_ref, &channel, &data, persist))?;
        let prepared = self
            .prepare_native_produce_candidate(
                &channel,
                &data,
                persist,
                &produce_ref,
                grouped_channels,
                Some(&LegacyCandidateIdentity(&expected)),
            )?
            .ok_or_else(mismatch)?;
        let candidate = prepared.candidate;
        let actual = prepared.comm;
        let returned_produce = if denied {
            produce_ref.clone()
        } else {
            self.native_binding(&actual)?
                .produces
                .into_iter()
                .find(|p| p.hash == produce_ref.hash)
                .unwrap_or_else(|| produce_ref.clone())
        };
        let observed = Self::observe_comm(
            observer,
            &actual,
            &candidate.continuation.continuation,
            candidate.continuation.persist,
            &candidate.data_candidates,
        );
        if denied {
            return require_denial(observed);
        }
        require_grant(observed)?;
        self.increment_produce_counter(&produce_ref, persist);
        self.log_produce(produce_ref.clone(), &channel, &data, persist);
        self.log_comm(
            &candidate.data_candidates,
            &candidate.channels,
            candidate.continuation.clone(),
            actual.clone(),
            "comm.produce",
        );
        if !candidate.continuation.persist {
            self.get_store()
                .remove_continuation(&candidate.channels, candidate.continuation_index);
        }
        self.mark_replay_waiting_continuation_match();
        self.remove_matched_datum_and_join(&candidate.channels, &candidate.data_candidates);
        self.remove_bindings_for(actual);
        Ok(self
            .wrap_result(
                candidate.channels,
                candidate.continuation.clone(),
                candidate.continuation.source,
                candidate.data_candidates,
            )
            .map(|(continuation, data)| (continuation, data, returned_produce)))
    }
}
