//! Added by decision 90 (DR-132): the readiness wait of the space transactions of Reified
//! RSpaces in native replay (`NativeReplayStall.tla`, the transaction actions `TxnCheck` and
//! `TxnPark`).
//!
//! A space transaction stages several operations under its stripes and commits them in order,
//! so it cannot wait for a row before it locks, as a host operation does
//! (`native_session/operations.rs`).
//!
//! Changed by decision 92: every row of a space transaction names the footprint key `S(sid)`
//! of its space as one more join group. The predecessors of a row are the last row on each
//! channel of its footprint. So the rows of one space form one chain in play order. The first
//! row of a committed transaction is unique to its job: its own row, or an anchor that the job
//! stages first (`SpaceReplayOrder.tla`).
//!
//! Before the first operation commits, the native space backend checks the first staged
//! operation only, with [`NativeOperationReplay::check_space_transaction`]. The check is
//! `FirstReady` of `SpaceReplayOrder.tla` and `TxnReady` of `NativeReplayStall.tla`:
//!
//! 1. Under the state lock, the first operation selects the first available row of its source,
//!    as `select_slot` selects the row of a host operation.
//! 2. The selected row is ready when its predecessors are complete, as `ready` decides for a
//!    host operation.
//! 3. A source without a row, and a source whose rows are all taken, are `NotReady`, not
//!    errors. A stale view can stage another first operation than play staged (review
//!    amendment (a)). `UnknownSourceFailsUnsafe.cfg` refutes the opposite rule.
//!
//! When the first row is ready, every earlier row of the space is complete and no later row is
//! complete, because `S(sid)` chains the rows. So the space is in the state that play gave the
//! transaction. Changed by decision 102 (decision 119 S2): the staging follows the recorded
//! rows of the transaction ([`NativeOperationReplay::space_recorded_row`]). Operation 0 takes
//! the first open row of its source. Each later operation takes the row of its source whose
//! `S(sid)` predecessor is the row of the operation before it. So replay repeats the random
//! picks of play.
//! A later row that is not ready, or whose recorded candidate differs, shows a defect or
//! forged evidence. So it fails closed at its commit (`begin_row` in
//! `native_session/space.rs`).
//!
//! A transaction that is `NotReady` applies nothing. The job releases its stripes and calls
//! [`NativeOperationReplay::wait_space_change`] with the version of the check. The wait returns
//! at once when the version changed since the check. Otherwise it parks as a counted waiter of
//! the stall rule (`progress.rs`) until a change notifies it. Then the job stages again.
//!
//! Changed by decision 95: the session epoch calls the check and the wait
//! (`NativeOperationEpoch::space_transaction_ready` and
//! `NativeOperationEpoch::wait_space_change`). So the space store takes no readiness wait.
//!
//! While the transaction holds its stripes, no other operation can reserve or publish a row of
//! a footprint of the transaction. So a passed check stays valid through the commit, and the
//! first operation then reserves the row that the check selected. The notification registers
//! before the version comparison. So a change before the registration shows as a new version,
//! and a change after it notifies. `NativeReplayStallTxnRegisterLateUnsafe.cfg` refutes the
//! opposite order.

// Changed by decisions 92 and 95: the check reads the first row only, and the session epoch
// calls it.
// Changed by decision 102 (decision 119 S2): the lookup of the recorded rows of a space
// transaction reads their outcome, their COMM and their footprint.
use std::sync::Arc;

use models::rhoapi::Par;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::replay_rspace::native_epoch::{
    NativeReplayOutcome, SpaceTransactionReadiness,
};
// Changed by decision 124 (LP-8): the completion of a row is read by the base body only.
use rspace_plus_plus::rspace::rspace_interface::RSpaceOperationSource;
use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

use super::super::super::progress::{self, Registration, WaitDecision};
use super::super::super::{NativeOperationReplay, NativeReplayError, Wait};
// Added by decision 102 (decision 119 S2).
use crate::rust::interpreter::accounting::native_runtime::operation_sources::{
    channel_bytes, NativeCommSource, NativeOperationSource, NativeProduceSource,
};
// Changed by decision 124 (LP-8): the link of an introduction is read by the base body only.

// Disabled by decision 95: the session epoch calls the check and the wait
// (`NativeOperationEpoch::space_transaction_ready` and
// `NativeOperationEpoch::wait_space_change`), so the space store takes no readiness wait.

// Disabled by decision 92: the check of the first row returns
// [`SpaceTransactionReadiness`], with the version of a row that is not ready.

impl NativeOperationReplay {
    /// Added by decision 92: the check of `FirstReady` for the first staged operation `first`
    /// of one space transaction (see the module documentation). It is `Ready` when the row of
    /// `first` is open and its predecessors are complete. It is `NotReady` with the state
    /// version `seen` when the row waits for a predecessor, and also when no row records the
    /// source or every row of the source is taken. It fails when the replay is closed or
    /// stalled, when the task runs outside the counted tasks of an armed evaluation (review
    /// amendment (c)), or when the source is malformed. The check reserves nothing.
    pub(crate) fn check_space_transaction(
        &self,
        first: RSpaceOperationSource<'_>,
    ) -> Result<SpaceTransactionReadiness, NativeReplayError> {
        self.counted()?;
        let live = Self::live_source(first)?;
        let state = self.inner.state.lock().expect("native replay state");
        state.open()?;
        let ready = match self.select_slot(&state, &live) {
            Ok(slot) => self.ready(&state, slot)?,
            Err(NativeReplayError::Source | NativeReplayError::Unavailable) => false,
            Err(failure) => return Err(failure),
        };
        match ready {
            true => Ok(SpaceTransactionReadiness::Ready),
            false => Ok(SpaceTransactionReadiness::NotReady {
                seen: state.version(),
            }),
        }
    }

    // Disabled by decision 92: the check reads the first staged row only
    // (`check_space_transaction`). A later row that is not ready, or that does not fit, fails
    // closed at its commit (`begin_row`). So the check selects no later row and compares no
    // recorded outcome or COMM.

    /// Added by decision 102 (decision 119 S2, plan v3 F1, F2 and F9): the recorded row of
    /// `source` in a space transaction (`NativeOperationEpoch::space_recorded_row`, rule CR-5
    /// of the plan). Every index is a journal index.
    ///
    /// - With `previous` None it is the first open row of the source. `select_slot` selects it
    ///   under the state lock, as in `check_space_transaction`. A source that no row records
    ///   and a source whose rows are all taken give None (G7).
    /// - With `previous` Some(p) it is the first row of the source after p, in journal order,
    ///   whose S(sid) predecessor is p (`space_successor`). These reads take no lock, because
    ///   the trace and its indexes never change.
    ///
    /// The answer holds the outcome that a reservation of the row expects, and the COMM of
    /// the row. The lookup reserves nothing and never waits.
    pub(crate) fn space_recorded_row(
        &self,
        source: RSpaceOperationSource<'_>,
        previous: Option<usize>,
        footprint_key: &Par,
    ) -> Result<Option<(usize, NativeReplayOutcome, Option<COMM>)>, NativeReplayError> {
        let live = Self::live_source(source)?;
        let slot = match previous {
            None => {
                let state = self.inner.state.lock().expect("native replay state");
                state.open()?;
                match self.select_slot(&state, &live) {
                    Ok(slot) => slot,
                    Err(NativeReplayError::Source | NativeReplayError::Unavailable) => {
                        return Ok(None);
                    }
                    Err(failure) => return Err(failure),
                }
            }
            Some(previous) => {
                let key = channel_bytes(footprint_key)?;
                match self.space_successor(&live, previous, &key) {
                    Some(slot) => slot,
                    None => return Ok(None),
                }
            }
        };
        let journal = self
            .inner
            .trace
            .journal_index(slot)
            .ok_or(NativeReplayError::Stage)?;
        let row = self
            .inner
            .trace
            .operation(slot)
            .ok_or(NativeReplayError::Stage)?;
        Ok(Some((
            journal,
            // Changed by decision 124 (LP-8): the base body in place of the copy.
            self.expected_outcome_at(slot),
            row.comm.as_ref().map(|comm| recorded_comm(&comm.source)),
        )))
    }

    /// Added by decision 102 (plan v3 F1): the slot of the first row of the source `live`
    /// after the journal row `previous`, in journal order, whose footprint holds the S(sid)
    /// bytes `key` and whose S(sid) predecessor is `previous`. Every row of a space names
    /// S(sid) (decision 92), so a row without it is not on the chain of the space.
    fn space_successor(
        &self,
        live: &NativeOperationSource,
        previous: usize,
        key: &[u8],
    ) -> Option<usize> {
        let index = &self.inner.source_index;
        let start = index.partition_point(|(source, _, _)| source < live);
        let end = start + index[start..].partition_point(|(source, _, _)| source == live);
        let rows = &index[start..end];
        let after = rows.partition_point(|(_, journal, _)| *journal as usize <= previous);
        rows[after..]
            .iter()
            .map(|(_, _, slot)| *slot as usize)
            .find(|slot| {
                self.inner
                    .trace
                    .operation(*slot)
                    .is_some_and(|row| footprint_holds(&row.footprint, key))
                    && self.space_predecessor(*slot, key) == Some(previous)
            })
    }

    /// Added by decision 102 (plan v3 F1): the S(sid) predecessor of the row `slot`. It is the
    /// largest journal index among the recorded channel predecessors of the row whose
    /// footprint holds the S(sid) bytes `key`. The retry predecessors never count.
    fn space_predecessor(&self, slot: usize, key: &[u8]) -> Option<usize> {
        let trace = &self.inner.trace;
        trace
            .operation(slot)?
            .predecessors
            .iter()
            .copied()
            .filter(|journal| {
                self.inner
                    .journal_slots
                    .get(*journal)
                    .and_then(|predecessor| trace.operation(*predecessor))
                    .is_some_and(|row| footprint_holds(&row.footprint, key))
            })
            .max()
    }

    // Retired by decision 124 (LP-8): a copy of the base body of `expected_outcome`. This file
    // calls `NativeOperationReplay::expected_outcome_at`.

    // Retired by decision 124 (LP-8): a copy of the base body of `introduction_owner_refused`.
    // This file calls `NativeOperationReplay::expected_outcome_at`.

    /// The wait of `TxnPark` of a job whose transaction was `NotReady` at the version `seen`,
    /// after the job released its stripes. It registers its notification, then, under the state
    /// lock, returns at once when the version changed since `seen`, and otherwise parks as a
    /// counted waiter (`progress::check_and_park`) until a change notifies it. A park that
    /// leaves every live task parked declares the stall. The registration leaves on every path
    /// (`Wait`), so the parked count stays exact.
    pub(crate) async fn wait_space_change(&self, seen: u64) -> Result<(), NativeReplayError> {
        let counted = self.counted()?;
        let mut wait = Wait {
            replay: self,
            registration: Registration::default(),
        };
        let notification = self.inner.changed.notified();
        tokio::pin!(notification);
        notification.as_mut().enable();
        {
            let mut state = self.inner.state.lock().expect("native replay state");
            state.open()?;
            if state.version() != seen {
                return Ok(());
            }
            match progress::check_and_park(
                &mut state,
                &self.inner.progress,
                &mut wait.registration,
                Ok::<bool, NativeReplayError>(false),
                counted,
            )? {
                WaitDecision::Park | WaitDecision::Ready => {}
                WaitDecision::Stall => {
                    drop(state);
                    self.inner.changed.notify_waiters();
                    return Err(NativeReplayError::Stalled);
                }
            }
        }
        notification.await;
        Ok(())
    }
}

/// Added by decision 102 (plan v3 F1): whether the recorded footprint `footprint` holds the
/// channel bytes `key`.
fn footprint_holds(footprint: &[Arc<[u8]>], key: &[u8]) -> bool {
    footprint.iter().any(|channel| channel.as_ref() == key)
}

/// Added by decision 102 (plan v3 F2): the COMM of a recorded row, built from its recorded
/// source. The produces take the default telemetry of `Produce::new`. The base `Produce`
/// equality compares the hash only, so the telemetry never changes a comparison.
fn recorded_comm(source: &NativeCommSource) -> COMM {
    let produce = |source: &NativeProduceSource| {
        Produce::new(
            Blake2b256Hash::from_bytes(source.channel.to_vec()),
            Blake2b256Hash::from_bytes(source.hash.to_vec()),
            source.persistent,
        )
    };
    COMM {
        consume: Consume {
            channel_hashes: source
                .consume
                .channels
                .iter()
                .map(|channel| Blake2b256Hash::from_bytes(channel.to_vec()))
                .collect(),
            hash: Blake2b256Hash::from_bytes(source.consume.hash.to_vec()),
            persistent: source.consume.persistent,
        },
        produces: source.produces.iter().map(produce).collect(),
        peeks: source.peeks.iter().copied().collect(),
        times_repeated: source
            .repetitions
            .iter()
            .map(|(repeated, count)| (produce(repeated), *count))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use std::time::Duration;

    use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
    // Added by decision 102 (decision 119 S2): the test of the forward of the session epoch.
    use rspace_plus_plus::rspace::replay_rspace::native_epoch::NativeOperationEpoch;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceOperationSource};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    // Changed by decision 92: the check compares no COMM, so the tests keep no recorded COMM.
    // Changed by decision 102 (decision 119 S2): the lookup of the recorded rows returns the
    // recorded COMM.
    use rspace_plus_plus::rspace::trace::event::{Consume, Event, Produce, COMM};

    use super::super::Session;
    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::native_runtime::tests::{
        authority, config, journal_limits, with_contract,
    };
    use crate::rust::interpreter::accounting::native_runtime::NativeOperationTraceLimits;
    use crate::rust::interpreter::accounting::{observation_construction, RuntimeBudget};
    use crate::rust::interpreter::errors::InterpreterError;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::reduce::{spawn_counted, with_counted_evaluation};
    use crate::rust::interpreter::rho_runtime::test_accounting_observer;

    const WEIGHTS: [u64; 4] = [0, 1, 1, 0];
    const DEADLINE: Duration = Duration::from_secs(10);
    const TRACE_LIMITS: NativeOperationTraceLimits = NativeOperationTraceLimits {
        events: 1_000,
        source_entries: 10_000,
        source_bytes: 10_000_000,
        telemetry_items: 1_000,
        telemetry_bytes: 1_000_000,
    };

    fn integer(value: i64) -> Par {
        Par {
            exprs: vec![models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(value)),
            }],
            ..Default::default()
        }
    }

    /// The journal of one play: the receive `for (_ <- a)` parks as row 0, and the send
    /// `a!(1)` fires it as row 1, whose channel predecessor is row 0. A space transaction can
    /// stage either one, so the rows stand for the rows of two transactions.
    struct Played {
        a: Par,
        one: ListParWithRandom,
        pattern: BindPattern,
        continuation: TaggedContinuation,
        auth: CostAuthority,
        session: Arc<Session>,
        receive: Consume,
        send: Produce,
        // Disabled by decision 92: the check compares no COMM.
        // comm: COMM,
        // Added by decision 102 (decision 119 S2): the lookup of the recorded rows returns the
        // recorded COMM.
        comm: COMM,
    }

    impl Played {
        async fn play() -> Self {
            let auth = authority(3);
            let a = integer(1);
            let one = ListParWithRandom {
                pars: vec![integer(1)],
                random_state: vec![1; 17],
                cost_authority: Some(auth.clone()),
                ..Default::default()
            };
            let pattern = BindPattern {
                patterns: vec![models::rust::utils::new_freevar_par(0, Vec::new())],
                free_count: 1,
                remainder: None,
            };
            let continuation = TaggedContinuation {
                cost_authority: Some(auth.clone()),
                ..Default::default()
            };
            let receive = Consume::create(
                &vec![a.clone()],
                &vec![pattern.clone()],
                &continuation,
                false,
            );
            let send = Produce::create(&a, &one, false);
            let mut stores = InMemoryStoreManager::new();
            let (play, _) = RSpace::create_with_replay(
                stores.r_space_stores().await.expect("in-memory stores"),
                Arc::new(Box::new(Matcher)),
            )
            .expect("a play space");
            play.create_checkpoint().await.expect("the pre-state");
            let history = play.get_history_repository();
            let budget = RuntimeBudget::new(Cost::unsafe_max());
            budget
                .reset_for_native_execution(config(u64::MAX, WEIGHTS))
                .expect("the budget resets");
            let _scope = budget.enter_comm_accounting_scope();
            let consume = observation_construction::consume_introduction(
                &receive,
                std::slice::from_ref(&a),
                std::slice::from_ref(&pattern),
                &continuation,
                &auth,
            )
            .expect("the consume introduction is measured");
            budget
                .register_introduction_authority(consume.event_id, consume.kind, &auth)
                .expect("the consume authority registers");
            let produce = observation_construction::produce_introduction(&send, &a, &one, &auth)
                .expect("the produce introduction is measured");
            budget
                .register_introduction_authority(produce.event_id, produce.kind, &auth)
                .expect("the produce authority registers");
            play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
            play.consume(
                vec![a.clone()],
                vec![pattern.clone()],
                continuation.clone(),
                false,
                BTreeSet::new(),
            )
            .await
            .expect("the receive plays");
            let fired = play
                .produce(a.clone(), one.clone(), false)
                .await
                .expect("the send plays");
            assert!(fired.is_some(), "the send fires the receive");
            let recording = budget
                .native_budget_recording()
                .expect("the recording reads")
                .expect("the play has a budget recording");
            let rows = budget
                .native_operation_recording()
                .expect("the journal reads")
                .expect("the play has an operation journal");
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1].predecessors.as_ref(), &[0]);
            let log = play.create_soft_checkpoint().await.log;
            // Disabled by decision 92: the check compares no COMM.
            // Added by decision 102 (decision 119 S2).
            let comm = log
                .iter()
                .find_map(|event| match event {
                    Event::Comm(comm) => Some(comm.clone()),
                    _ => None,
                })
                .expect("the play records its COMM");
            let trace = with_contract(u64::MAX, WEIGHTS, |contract| {
                contract.check_operation_journal(
                    recording.session,
                    &recording,
                    Arc::clone(&rows),
                    journal_limits(),
                )
            })
            .expect("the journal checks")
            .bind_trace(log.into(), TRACE_LIMITS)
            .expect("the log binds to the rows");
            let session = Arc::new(
                trace
                    .into_session(history, Arc::new(Box::new(Matcher)))
                    .expect("the replay session opens"),
            );
            Self {
                a,
                one,
                pattern,
                continuation,
                auth,
                session,
                receive,
                send,
                // Disabled by decision 92: the check compares no COMM.
                // comm,
                // Added by decision 102 (decision 119 S2).
                comm,
            }
        }

        fn replay(&self) -> &NativeOperationReplay { &self.session.replay }

        // Changed by decision 92: the check reads the source of the first staged operation
        // only, so a staged fire and a staged store of the send are one source.

        /// The source of row 0, which the receive parks.
        fn park(&self) -> RSpaceOperationSource<'_> {
            RSpaceOperationSource::Consume(&self.receive)
        }

        /// The source of row 1, whose send fires the receive.
        fn fire(&self) -> RSpaceOperationSource<'_> { RSpaceOperationSource::Produce(&self.send) }

        /// The verdict of the check of a transaction whose first staged operation is `first`.
        fn check(&self, first: RSpaceOperationSource<'_>) -> SpaceTransactionReadiness {
            self.replay()
                .check_space_transaction(first)
                .expect("the check reaches a verdict")
        }

        /// The version that the check of the first staged operation `first` reports as not
        /// ready.
        fn not_ready(&self, first: RSpaceOperationSource<'_>) -> u64 {
            match self.check(first) {
                SpaceTransactionReadiness::NotReady { seen } => seen,
                SpaceTransactionReadiness::Ready => panic!("the transaction is not ready"),
            }
        }

        /// The receive in native replay, as a host operation: it completes row 0.
        async fn replay_receive(&self) {
            self.session
                .consume(
                    vec![self.a.clone()],
                    vec![self.pattern.clone()],
                    self.continuation.clone(),
                    false,
                    BTreeSet::new(),
                    &self.auth,
                )
                .await
                .expect("the receive replays");
        }
    }

    // Disabled by decision 92: the check reads the first staged operation only. A later row
    // that is not ready, or that does not fit, fails closed at its commit (`begin_row`), and
    // the tests of `native_session/space.rs` check that commit.

    // Added by decision 92, steps 1 to 3: the first staged operation selects its row by source,
    // and the transaction is ready only when that row is open and its predecessors are complete.
    // Each failing case is `NotReady`, not an error (review amendment (a)), and a check changes
    // nothing.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_readiness_checks_the_first_row_against_the_journal() {
        let played = Played::play().await;
        let replay = played.replay();
        // Row 1 waits for row 0, which no operation completed.
        let seen = played.not_ready(played.fire());
        // Row 0 has no predecessor, so a transaction that stages it first is ready.
        assert_eq!(
            played.check(played.park()),
            SpaceTransactionReadiness::Ready
        );
        // A source that no row records is not ready either: a stale view staged it.
        let unknown = Produce::create(&integer(2), &played.one, false);
        assert_eq!(
            played.not_ready(RSpaceOperationSource::Produce(&unknown)),
            seen
        );
        // No check reserved or completed a row.
        assert!(replay.check_complete().is_err());
        assert_eq!(played.not_ready(played.fire()), seen);
        // The receive completes row 0, which is a change. Then row 1 is ready, and the source
        // of row 0 has no open row, so a transaction that stages it first is not ready.
        played.replay_receive().await;
        assert_eq!(
            played.check(played.fire()),
            SpaceTransactionReadiness::Ready
        );
        assert_ne!(played.not_ready(played.park()), seen);
    }

    // Added by decision 102 (decision 119 S2, the tests of C2 in plan v2): the lookup of the
    // recorded rows. The rows of the fixture are host rows, so the channel `a` stands for
    // S(sid). Both rows hold it, and row 1 names row 0 as its channel predecessor.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_recorded_row_follows_the_chain_of_the_footprint_key() {
        let played = Played::play().await;
        let replay = played.replay();
        let key = &played.a;
        let lookup = |source: RSpaceOperationSource<'_>, previous: Option<usize>, key: &Par| {
            replay
                .space_recorded_row(source, previous, key)
                .expect("the lookup reads the journal")
        };
        let fired = Some((1, NativeReplayOutcome::Matched, Some(played.comm.clone())));
        // Operation 0 takes the first open row of its source, with its outcome and its COMM.
        assert_eq!(
            lookup(played.park(), None, key),
            Some((0, NativeReplayOutcome::Stored, None))
        );
        assert_eq!(lookup(played.fire(), None, key), fired);
        // The chain successor of row 0 for the source of the send is row 1.
        assert_eq!(lookup(played.fire(), Some(0), key), fired);
        // No row of the receive follows row 0, and no row of the send follows row 1.
        assert_eq!(lookup(played.park(), Some(0), key), None);
        assert_eq!(lookup(played.fire(), Some(1), key), None);
        // A footprint key that no row holds gives no successor.
        assert_eq!(lookup(played.fire(), Some(0), &integer(9)), None);
        // A source that no row records gives None, also as the first operation of a stale
        // view.
        let unknown = Produce::create(&integer(2), &played.one, false);
        for previous in [None, Some(0)] {
            assert_eq!(
                lookup(RSpaceOperationSource::Produce(&unknown), previous, key),
                None
            );
        }
        // No lookup reserved or completed a row, so the receive still takes row 0.
        assert!(replay.check_complete().is_err());
        played.replay_receive().await;
        // The source of the receive has no open row now: `Unavailable` gives None (G7).
        assert_eq!(lookup(played.park(), None, key), None);
        // The successor reads do not read the state, so row 1 still follows row 0.
        assert_eq!(lookup(played.fire(), Some(0), key), fired);
        // The session epoch forwards the lookup (C1.2).
        let epoch = super::super::super::SessionEpoch {
            replay: replay.clone(),
        };
        assert_eq!(
            <super::super::super::SessionEpoch as NativeOperationEpoch<
                Par,
                BindPattern,
                ListParWithRandom,
                TaggedContinuation,
            >>::space_recorded_row(&epoch, played.fire(), Some(0), key)
            .expect("the forward reads the journal"),
            fired
        );
    }

    // `TxnPark`: a job whose transaction was not ready parks as a counted waiter at the version
    // of its check. The receive of another counted task completes row 0, which wakes it, and
    // its transaction is then ready.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_readiness_waits_until_a_change() {
        let played = Arc::new(Played::play().await);
        let session = Arc::clone(&played.session);
        let waiter = Arc::clone(&played);
        let ((), errors) = tokio::time::timeout(
            DEADLINE,
            with_counted_evaluation(session.evaluation_progress(), async move {
                // Changed by decision 92: the check reads the first staged operation only.
                let seen = played.not_ready(played.fire());
                spawn_counted(async move {
                    waiter
                        .replay()
                        .wait_space_change(seen)
                        .await
                        .map_err(|error| InterpreterError::BugFoundError(error.to_string()))?;
                    // Changed by decision 92: the check reads the first staged operation only.
                    match waiter.replay().check_space_transaction(waiter.fire()) {
                        Ok(SpaceTransactionReadiness::Ready) => Ok(()),
                        other => Err(InterpreterError::BugFoundError(format!(
                            "still not ready: {other:?}"
                        ))),
                    }
                });
                let mut polls = 0;
                while played.session.progress_counts().1 == 0 {
                    polls += 1;
                    assert!(polls < 10_000, "the job never parked");
                    tokio::task::yield_now().await;
                }
                assert_eq!(played.session.progress_counts(), (2, 1));
                played.replay_receive().await;
            }),
        )
        .await
        .expect("the evaluation ends")
        .expect("the evaluation begins");
        assert!(errors.is_empty(), "{errors:?}");
        assert!(!session.progress_stalled());
    }

    // `TxnPark`: a change after the check and before the wait shows as a new version, so the
    // job runs again at once and does not park.
    // Changed by decision 93: the tests run on a multi-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_readiness_returns_at_once_after_a_change() {
        let played = Played::play().await;
        // Changed by decision 92: the check reads the first staged operation only.
        let seen = played.not_ready(played.fire());
        played.replay_receive().await;
        tokio::time::timeout(DEADLINE, played.replay().wait_space_change(seen))
            .await
            .expect("the wait returns at once")
            .expect("the replay is open");
        // Changed by decision 92: the check reads the first staged operation only.
        assert_eq!(
            played.check(played.fire()),
            SpaceTransactionReadiness::Ready
        );
    }

    // The stall rule: a counted job that is the only live task and whose transaction is never
    // ready parks, and its park finds every live task parked, so it declares the stall.
    // Changed by decision 93: the tests run on a multi-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_readiness_park_declares_the_stall() {
        let played = Played::play().await;
        let session = Arc::clone(&played.session);
        let (waited, errors) = tokio::time::timeout(
            DEADLINE,
            with_counted_evaluation(session.evaluation_progress(), async {
                // Changed by decision 92: the check reads the first staged operation only.
                let seen = played.not_ready(played.fire());
                played.replay().wait_space_change(seen).await
            }),
        )
        .await
        .expect("the single job reaches a verdict")
        .expect("the evaluation begins");
        assert!(
            matches!(waited, Err(NativeReplayError::Stalled)),
            "{waited:?}"
        );
        assert!(errors.is_empty(), "{errors:?}");
        assert!(session.progress_stalled());
        // Changed by decision 92: the check reads the first staged operation only, and it
        // reports the stall itself.
        let checked = played.replay().check_space_transaction(played.park());
        assert!(
            matches!(checked, Err(NativeReplayError::Stalled)),
            "{checked:?}"
        );
    }

    // Review amendment (c): while an evaluation is armed, a check from a task that the
    // evaluation does not count is refused, as `reserve_ready` refuses its operation.
    // Changed by decision 93: the tests run on a multi-thread runtime.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn space_readiness_refuses_an_uncounted_check() {
        let played = Arc::new(Played::play().await);
        let session = Arc::clone(&played.session);
        let outside = Arc::clone(&played);
        let (checked, errors) = tokio::time::timeout(
            DEADLINE,
            with_counted_evaluation(session.evaluation_progress(), async move {
                // Changed by decision 92: the check reads the first staged operation only.
                // tokio::spawn(async move { outside.replay().check_space_rows(&[outside.park()]) })
                tokio::spawn(
                    async move { outside.replay().check_space_transaction(outside.park()) },
                )
                .await
                .expect("the uncounted task runs")
            }),
        )
        .await
        .expect("the evaluation ends")
        .expect("the evaluation begins");
        // Changed by decision 92: the check reports the refusal itself.
        assert!(
            matches!(&checked, Err(NativeReplayError::Uncounted)),
            "{checked:?}"
        );
        assert!(errors.is_empty(), "{errors:?}");
    }
}
