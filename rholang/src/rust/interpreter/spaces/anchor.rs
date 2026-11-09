//! The anchor of a space transaction (decision 92).
//!
//! Replay orders the transactions of a space by their rows. Every row of a space names the space
//! counter `S(sid)` in its footprint, so the rows of the space form one chain in play order.
//! Replay checks only the first row of a transaction against the journal. So the first row must
//! be unique to the job of the transaction (`SpaceReplayOrder.tla`).
//!
//! The first operation of most transactions is the job's own: its identity carries the random
//! state of the job ([`SpaceJob::owns`]). A transaction whose first operation is not its own
//! stages an anchor first. Added by decision 102 (plan v4 C7.22): a re-offer of a stored
//! persistent record is never the job's own ([`SpaceJob::owns_first_reoffer`]), because every
//! re-offer of that record shares its source. A transaction that stages nothing commits an
//! anchor only. A
//! transaction that fails at a position of the state also commits an anchor only, and then it
//! fails ([`SpaceError::is_positional`]). The anchor is a non-peek consume of the persistent
//! config record on `C(sid)`, the form of dev and base. Its COMM leaves the record in place, so
//! the anchor changes no state of the space.
//!
//! | # | Operation | Outcome | Events |
//! |---|-----------|---------|--------|
//! | 0 | consume `C(sid)` with the counter pattern and `K_anchor` | fire with the config record | `Consume`, `COMM` |
//!
//! The continuation `K_anchor` has the body `GInt(0)` and the random state
//! `RandomPurpose::Anchor.derive(r).split_byte(0)` of the random state `r` of the job (D3). The
//! anchor carries the cost authority of the counter continuations of the operation, and its
//! consume registers the introduction of the operation, as a counter take does (D2).
//!
//! [`SpaceError::is_positional`]: super::errors::SpaceError::is_positional

use std::collections::BTreeSet;
use std::sync::{Arc, LazyLock};

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, CostAuthority, ListParWithRandom, Par, TaggedContinuation};
use rspace_plus_plus::rspace::internal::{ConsumeCandidate, Datum};
use rspace_plus_plus::rspace::space_store_handle::{ConsumeOutcome, SpaceConsume};

use super::agent::{ParSpaceTxn, SpaceOp};
use super::counter::{counter_continuation, counter_pattern};
use super::errors::SpaceError;
use super::generic::Offered;
use super::matching::SpaceMatcher;
use super::namespace::{self, NativeSpace, RandomPurpose};
use super::record::{self, Introduction};

/// The template of the anchor pattern: the counter pattern, whose two free variables bind the
/// config record `[envelope, config]`.
static ANCHOR_PATTERN: LazyLock<BindPattern> = LazyLock::new(counter_pattern);

/// The job of a space transaction: what the `own` flag of each staged operation and the anchor
/// of the transaction take from the operation of the job.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SpaceJob {
    /// The space of the operation.
    pub(crate) space: NativeSpace,
    /// The random state of the operation: the random state of the datum of a produce, of the
    /// body of a consume or a join, or of a settle job.
    pub(crate) random_state: Vec<u8>,
    /// The cost authority of the counter continuations of the operation: the cost authority of
    /// the datum of a produce, and the stamped authority of a consume, a join or a settle job
    /// (`record::OperationAuthority`).
    pub(crate) authority: Option<CostAuthority>,
}

impl SpaceJob {
    /// The job of the operation `op`.
    pub(crate) fn of(op: &SpaceOp) -> Self {
        let (space, random_state, authority) = match op {
            SpaceOp::Produce(op) => (
                op.space,
                &op.data.random_state,
                op.data.cost_authority.as_ref(),
            ),
            SpaceOp::Consume(op) => (
                op.space,
                &op.body.random_state,
                record::stamped(&op.authority),
            ),
            SpaceOp::Settle(op) => (op.space, &op.random_state, record::stamped(&op.authority)),
            SpaceOp::Join(op) => (
                op.space,
                &op.body.random_state,
                record::stamped(&op.authority),
            ),
        };
        Self {
            space,
            random_state: random_state.clone(),
            authority: authority.cloned(),
        }
    }

    /// True when the random state `random_state` of the record of a produce or of the
    /// continuation of a consume is the random state of the job. Such an operation is the job's
    /// own. Only the jobs of one re-issue chain share a random state: a transaction issues at
    /// most one re-issue or settle job with its random state, and that job starts after the
    /// commit. So when a job checks its first row, every row of the earlier jobs of its chain is
    /// complete, and no live job has the same source.
    ///
    /// Added by decision 102 (plan v4 C7.22, F22): this holds for a datum in flight. A re-offer of
    /// a stored persistent record keeps the source of that record, and a job of another chain can
    /// re-offer the same record. So [`SpaceJob::owns_first_reoffer`] decides the own flag of a
    /// produce-side fire.
    pub(crate) fn owns(&self, random_state: &[u8]) -> bool { random_state == self.random_state }

    /// Added by decision 102 (plan v4 C7.22, rule CR-19 case 3, F22): true when the produce-side
    /// fire of a datum with the random state `random_state`, which offers `offered`, is the job's
    /// own. A datum in flight is the job's own when it carries the random state of the job
    /// ([`SpaceJob::owns`]). A re-offer of a stored persistent record is never the job's own. It
    /// keeps the source of the record, and every re-offer of that record has that source. So a
    /// transaction whose first operation re-offers such a record stages its anchor first
    /// (decision 92). Without the anchor, the lookup of a later job by that source and its
    /// predecessor row could take the first row of a job of another chain that re-offers the same
    /// record. The flag is read only for the first operation (the commit check of decision 92), so
    /// the answer for a later re-offer changes nothing.
    pub(crate) fn owns_first_reoffer(&self, random_state: &[u8], offered: Offered) -> bool {
        match offered {
            Offered::InFlight => self.owns(random_state),
            Offered::StoredPersistent => false,
        }
    }

    /// True when the continuation `continuation` of a consume carries the random state of the
    /// job ([`SpaceJob::owns`]). A continuation without a `ParBody` is not the job's own.
    pub(crate) fn owns_continuation(&self, continuation: &TaggedContinuation) -> bool {
        match record::continuation_random_state(continuation) {
            Some(random_state) => self.owns(random_state),
            None => false,
        }
    }

    /// True when the continuation `continuation` of a lift carries the lift random state of the
    /// job, `RandomPurpose::Lift.derive(r)` of the random state `r` of the job. A settle and a
    /// plan of the job derive it from the job (`settle::SettleOrigin`), and a restore derives it
    /// from its record.
    pub(crate) fn owns_lift(&self, continuation: &TaggedContinuation) -> bool {
        match record::continuation_random_state(continuation) {
            Some(random_state) => {
                let lift =
                    RandomPurpose::Lift.derive(&Blake2b512Random::from_bytes(&self.random_state));
                random_state == lift.to_bytes().as_slice()
            }
            None => false,
        }
    }
}

/// Stages the anchor of a transaction of the job `job` in `txn`, with the matcher `matcher` and
/// the introduction `introduction` of the operation: a non-peek consume of the persistent config
/// record on `C(sid)`, which fires and leaves the record in place. The lock keys of `txn` hold
/// `C(sid)`, and its phase B is taken. A space without exactly one persistent config record is
/// `Inconsistent`.
pub(crate) fn stage(
    txn: &mut ParSpaceTxn,
    job: &SpaceJob,
    matcher: &SpaceMatcher,
    introduction: &Arc<Introduction>,
) -> Result<(), SpaceError> {
    let config = namespace::gprivate(namespace::config_channel(job.space));
    let stored = match txn.data(&config)? {
        [stored] => stored.clone(),
        [] => return Err(SpaceError::Inconsistent("an anchor finds no config record")),
        _ => return Err(SpaceError::Inconsistent("a space has two config records")),
    };
    if !stored.persist {
        return Err(SpaceError::Inconsistent(
            "a config record is not persistent",
        ));
    }
    let matched = matcher
        .bind(&ANCHOR_PATTERN, &stored.a)
        .ok_or(SpaceError::Inconsistent(
            "a config record does not match the anchor pattern",
        ))?;
    let random = RandomPurpose::Anchor
        .derive(&Blake2b512Random::from_bytes(&job.random_state))
        .split_byte(0);
    // Added by decision 124 (plan v2 C7.4, decision 99): `K_anchor` takes the machinery seal,
    // and the payer introduction registers the anchor, so it has no owner part (CR-15).
    let seal = introduction.seal_of(job.authority.as_ref())?;
    let continuation = counter_continuation(0, &random, seal);
    let payer = introduction.payer();
    let mut channels = Vec::<Par>::with_capacity(1);
    channels.push(config.clone());
    let mut patterns = Vec::<BindPattern>::with_capacity(1);
    patterns.push((*ANCHOR_PATTERN).clone());
    let mut data = Vec::<ConsumeCandidate<Par, ListParWithRandom>>::with_capacity(1);
    let Datum { a, persist, source } = stored;
    data.push(ConsumeCandidate {
        channel: config,
        datum: Datum {
            a: matched,
            persist,
            source,
        },
        removed_datum: a,
        datum_index: 0,
    });
    txn.consume(SpaceConsume {
        channels,
        patterns,
        continuation,
        persist: false,
        peeks: BTreeSet::new(),
        outcome: ConsumeOutcome::Fire { data },
        // Added by decision 124 (plan v2 C7.4): the payer introduction.
        before_observe: Some(record::register_introduction(&payer)),
        own: true,
        // Added by decision 123 (plan v4, V3-9): no row search.
        search: None,
    })?;
    // Added by plan v4 errata (V4-12, C9 of the anchor-only arbiter): the anchor is the leading
    // anchor of the transaction (rule CR-5). Its row is not a followed row for CR-6 and CR-32.
    // This one call covers the four cases of CR-19. After `anchor_if_empty` and
    // `prepare_failure` no stage call follows, so the mark changes nothing there.
    txn.mark_leading_anchor();
    Ok(())
}

#[cfg(test)]
mod tests {
    use models::rhoapi::cost_signature::Value as CostSignatureValue;
    use models::rhoapi::{CostRegion, CostSignature};
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::rspace_interface::{ISpace, RSpaceOperationSource};
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
    use rspace_plus_plus::rspace::space_store_handle::{
        ProduceOutcome, SpaceProduce, SpaceStoreHandle,
    };
    use rspace_plus_plus::rspace::trace::event::{Event, IOEvent};

    use super::*;
    use crate::rust::interpreter::accounting::costs::Cost;
    use crate::rust::interpreter::accounting::RuntimeBudget;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::spaces::agent::SpaceEnv;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};

    type ParRSpace = RSpace<Par, BindPattern, ListParWithRandom, TaggedContinuation>;

    const SPACE: NativeSpace = NativeSpace {
        sid: [7; SID_LEN],
        durability: Durability::Durable,
    };

    /// Added by decision 124 (plan v2 C7.4): a cost authority with one unpriced region of
    /// `byte`.
    fn region(byte: u8) -> CostAuthority {
        CostAuthority {
            regions: vec![CostRegion {
                instance_id: vec![byte; 32],
                signature: Some(CostSignature {
                    value: Some(CostSignatureValue::Ground(vec![byte])),
                }),
                price: 0,
                caller: None,
            }],
        }
    }

    /// Added by decision 124 (plan v2 C7.4, decision 99, rule CR-15): the anchor registers the
    /// payer introduction of the operation, which carries no authority. So the anchor has no
    /// owner part, the base of the `prepare_failure` check.
    #[tokio::test]
    async fn anchor_has_no_owner_part() {
        let mut stores = InMemoryStoreManager::new();
        let host = ParRSpace::create(
            stores.r_space_stores().await.expect("in-memory stores"),
            Arc::new(Box::new(Matcher)),
        )
        .expect("an in-memory space");
        let env = SpaceEnv {
            store: SpaceStoreHandle::new(host.clone()),
            budget: RuntimeBudget::new(Cost::create(1_000, "anchor test")),
            volatile: Default::default(),
            origin: Default::default(),
        };
        let _scope = env.budget.enter_comm_accounting_scope();
        let config = namespace::gprivate(namespace::config_channel(SPACE));
        let mut txn = env
            .store
            .begin(std::slice::from_ref(&config))
            .await
            .expect("a transaction");
        txn.produce(SpaceProduce {
            channel: config.clone(),
            data: record::config_record(
                Par::default(),
                &Blake2b512Random::create_from_bytes(b"config"),
            ),
            persist: true,
            outcome: ProduceOutcome::Store,
            before_observe: None,
            own: false,
            search: None,
        })
        .expect("the config record stages");
        txn.commit().await.expect("the config record commits");
        host.take_event_log().await;
        let job = SpaceJob {
            space: SPACE,
            random_state: Blake2b512Random::create_from_bytes(b"anchored job").to_bytes(),
            authority: None,
        };
        let introduction = Introduction::new(&env.budget, &region(4));
        let mut txn = env
            .store
            .begin(std::slice::from_ref(&config))
            .await
            .expect("a transaction");
        stage(&mut txn, &job, &SpaceMatcher::new(&env), &introduction).expect("the anchor stages");
        txn.commit().await.expect("the anchor commits");
        let log = host.take_event_log().await;
        let [Event::IoEvent(IOEvent::Consume(anchor)), Event::Comm(_)] = log.as_slice() else {
            panic!("the anchor fires: {log:?}");
        };
        let payer = introduction.payer();
        assert_eq!(payer.authority(), &CostAuthority::default());
        assert_eq!(
            record::register_introduction(&payer)(RSpaceOperationSource::Consume(anchor)),
            Ok(())
        );
    }

    /// Added by decision 102 (plan v4 C7.22, rule CR-19 case 3, F22): the fire of a datum in
    /// flight is the job's own exactly when the datum carries the random state of the job. The
    /// re-offer of a stored persistent record is never the job's own.
    #[test]
    fn space_job_owns_only_a_first_reoffer_of_a_datum_in_flight() {
        let job = SpaceJob {
            space: SPACE,
            random_state: Blake2b512Random::create_from_bytes(b"re-offer job").to_bytes(),
            authority: None,
        };
        let other = Blake2b512Random::create_from_bytes(b"other job").to_bytes();
        assert!(job.owns_first_reoffer(&job.random_state, Offered::InFlight));
        assert!(!job.owns_first_reoffer(&other, Offered::InFlight));
        assert!(!job.owns_first_reoffer(&job.random_state, Offered::StoredPersistent));
        assert!(!job.owns_first_reoffer(&other, Offered::StoredPersistent));
    }
}
