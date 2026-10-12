//! The ephemeral spaces of one root evaluation (P7: I-4, decision 19 D1, design v3 VR-1 to VR-10
//! and DG-1).
//!
//! A `temp` or `seq` space lives for one deploy. Its ids use the lowercase KINDs (I-4), and its
//! space id comes from the message that created it (I-19), so no other deploy can make its ids.
//! The end of the root evaluation that created it sweeps its records, its waiting continuations
//! and its join entries out of the hot store (`SpaceTxnHost::sweep_volatile`), so its state
//! never reaches a checkpoint.
//!
//! [`VolatileRegistry`] is the book of one root evaluation (VR-1). It holds the ephemeral spaces
//! that the evaluation created, with their qualifiers, and the volatile channels that the
//! transactions of the evaluation locked. It is node bookkeeping, not consensus state:
//!
//! * The creation job records a space after its commit, as the last step of the job (VR-2,
//!   decision 83). The factory replies after the job, so every holder of the handle or of a
//!   name of the space causally follows the record. The scheduler runs no reduction
//!   participant during a job, so the book changes only between two scheduling windows, and
//!   every read of one window sees one book.
//! * `SpaceEnv::begin` records each lock key with an ephemeral KIND. The lock keys of a
//!   transaction hold every channel that it stages (readiness review S2, FR-4), so the book
//!   holds every volatile channel that a commit can write.
//! * `ReifiedRuntime::begin_evaluation_scope` clears the book at the start of a root evaluation,
//!   and the sweep drains it at the end (VR-3, VR-4).
//!
//! An id with an ephemeral KIND whose space is not in the book belongs to a deploy that has
//! ended, so the entry points of DG-1 fail with `DanglingHandle` or `DanglingName` before any job.
//!
//! The book reserves nothing on the meter of a job, as the mirror journal reserves nothing: its
//! copies are bounded by the lock keys, which the agent reserved when it made them.
//!
//! **Seq claims (design v3 SQ-3, SQ-4, decision 73).** Each channel of a `seq` space has its own
//! causal-lineage claim in the book: the participant path of its last use. [`SeqClaims`] holds
//! the claims of one program job. A claim of a channel is admitted when the channel has no claim
//! or its claim is a prefix of the participant path of the job. The job stores its claims only
//! after its commit succeeded, so a failed job or a dropped transaction changes no claim. Its
//! metered work is reserved on the meter of the job (SQ-8).

use std::collections::{BTreeMap, BTreeSet};
use std::mem::size_of;
use std::sync::{Arc, Mutex};

use models::rhoapi::Par;
use rspace_plus_plus::rspace::operation_context::{self, PathSegment};

use super::errors::SpaceError;
use super::namespace::{
    decode_reified_id, sole_gprivate_id, Durability, NativeSpace, ReifiedId, SpaceId,
};
use super::types::qualifier::SpaceQualifier;
use crate::rust::interpreter::reified::costs::ScanMeter;

/// The space of a space-scoped id with an ephemeral KIND: a config, counter, data, path-index,
/// allocator, handle or name id of a `temp` or `seq` space. Every other id gives `None`.
pub fn ephemeral_space(id: &[u8]) -> Option<NativeSpace> {
    let space = match decode_reified_id(id)? {
        ReifiedId::Config(space)
        | ReifiedId::SpaceCounter(space)
        | ReifiedId::CollectionCounter(space, _)
        | ReifiedId::Data(space, _)
        | ReifiedId::PathIndex(space, _)
        | ReifiedId::Allocator(space)
        | ReifiedId::Handle(space)
        | ReifiedId::Name(space, _) => space,
        ReifiedId::Op(_)
        | ReifiedId::Matcher(..)
        | ReifiedId::Body(..)
        | ReifiedId::Reserved(..) => return None,
    };
    match space.durability {
        Durability::Ephemeral => Some(space),
        Durability::Durable => None,
    }
}

/// The book of one root evaluation: its ephemeral spaces and its volatile channels.
#[derive(Debug, Default)]
pub struct VolatileRegistry {
    state: Mutex<VolatileState>,
}

#[derive(Debug, Default)]
struct VolatileState {
    /// The ephemeral spaces that the evaluation created, by space id.
    spaces: BTreeMap<SpaceId, SpaceQualifier>,
    /// The volatile channels that the transactions of the evaluation locked, by `GPrivate` id.
    channels: BTreeMap<Vec<u8>, Par>,
    /// The claims of the channels of the `seq` spaces (SQ-3): the participant path of the last
    /// use of each claimed channel, by `GPrivate` id.
    claims: BTreeMap<Vec<u8>, Vec<PathSegment>>,
}

impl VolatileRegistry {
    fn state(&self) -> std::sync::MutexGuard<'_, VolatileState> {
        self.state.lock().expect("volatile registry lock")
    }

    /// Records each key of `keys` that is a space-scoped id with an ephemeral KIND.
    pub fn record_locked(&self, keys: &[Par]) {
        let mut volatile = keys.iter().filter_map(|key| {
            sole_gprivate_id(key)
                .filter(|id| ephemeral_space(id).is_some())
                .map(|id| (id.to_vec(), key.clone()))
        });
        let Some(first) = volatile.next() else {
            return;
        };
        let mut state = self.state();
        state.channels.insert(first.0, first.1);
        state.channels.extend(volatile);
    }

    /// Records the ephemeral space `space` with its qualifier, after its creation committed.
    pub fn record_space(&self, space: NativeSpace, qualifier: SpaceQualifier) {
        self.state().spaces.insert(space.sid, qualifier);
    }

    /// Records the space `space` that a creation job committed, when it is ephemeral (VR-2,
    /// decision 83). A durable space is not recorded. The creation job calls it as the last step
    /// of its commit, so the book changes only inside a job.
    pub fn record_created(&self, space: NativeSpace, qualifier: SpaceQualifier) {
        match space.durability {
            Durability::Ephemeral => self.record_space(space, qualifier),
            Durability::Durable => {}
        }
    }

    /// The qualifier of the ephemeral space `sid` when the current evaluation created it.
    pub fn qualifier(&self, sid: &SpaceId) -> Option<SpaceQualifier> {
        self.state().spaces.get(sid).copied()
    }

    /// True when the current evaluation created the ephemeral space `sid`.
    pub fn is_live(&self, sid: &SpaceId) -> bool { self.state().spaces.contains_key(sid) }

    /// True when a transaction of the current evaluation locked a volatile channel.
    pub fn has_channels(&self) -> bool { !self.state().channels.is_empty() }

    /// Takes the volatile channels of the evaluation, in the order of their ids, and leaves the
    /// set of channels empty. The spaces stay in the book until the next clear.
    pub fn take_channels(&self) -> Vec<Par> {
        std::mem::take(&mut self.state().channels)
            .into_values()
            .collect()
    }

    /// The `seq` spaces that the current evaluation created (SQ-1).
    pub fn seq_spaces(&self) -> BTreeSet<SpaceId> {
        self.state()
            .spaces
            .iter()
            .filter(|(_, qualifier)| **qualifier == SpaceQualifier::Seq)
            .map(|(sid, _)| *sid)
            .collect()
    }

    /// True when the current evaluation created a `seq` space.
    pub fn has_seq_space(&self) -> bool {
        self.state()
            .spaces
            .values()
            .any(|qualifier| *qualifier == SpaceQualifier::Seq)
    }

    /// The number of claimed channels.
    fn claim_count(&self) -> usize { self.state().claims.len() }

    /// The claim of the channel with the id `id`.
    fn claim_of(&self, id: &[u8]) -> Option<Vec<PathSegment>> {
        self.state().claims.get(id).cloned()
    }

    /// Sets the claim of each channel of `ids` to `path`.
    fn store_claims(&self, ids: BTreeSet<Vec<u8>>, path: &[PathSegment]) {
        let mut state = self.state();
        for id in ids {
            state.claims.insert(id, path.to_vec());
        }
    }

    /// Empties the book.
    pub fn clear(&self) {
        let mut state = self.state();
        state.spaces.clear();
        state.channels.clear();
        state.claims.clear();
    }
}

/// The role in which an operation uses a channel of a `seq` space (SQ-3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClaimRole {
    /// The channel holds the records that the operation stores, restores, delivers or removes.
    /// Only a data channel `d(sid, k)` is claimed in this role: a marker on a path-index channel
    /// is bookkeeping and claims nothing.
    Data,
    /// The channel is in the group of a waiting receive: a data channel `d(sid, k)` of a keyed
    /// store or a join, or a path-index channel `i(sid, q)` of a PathMap receive on the prefix
    /// `q`.
    Waiter,
}

/// The bytes of one segment of a participant path, for the reservation of a comparison.
const SEGMENT_BYTES: usize = size_of::<PathSegment>();

/// The claims of one program job on the channels of one `seq` space (SQ-3, SQ-4).
pub struct SeqClaims {
    registry: Arc<VolatileRegistry>,
    space: SpaceId,
    /// The participant path of the job: the path of its operation order without the last
    /// segment `(step, 0)` (F12).
    path: Vec<PathSegment>,
    /// The ids of the channels that the job claimed, which `store` makes its claims.
    pending: BTreeSet<Vec<u8>>,
}

impl SeqClaims {
    /// The claims of a job on `space`, or `None` when no claim applies: the space is not a `seq`
    /// space, or the job is internal (a re-issue or a settle job, SQ-3). A program job on a `seq`
    /// space without an operation order fails closed with `Inconsistent`.
    pub fn open(
        registry: &Arc<VolatileRegistry>,
        space: NativeSpace,
        qualifier: SpaceQualifier,
        internal: bool,
        meter: &ScanMeter,
    ) -> Result<Option<Self>, SpaceError> {
        if qualifier != SpaceQualifier::Seq || internal {
            return Ok(None);
        }
        let order = operation_context::current().ok_or(SpaceError::Inconsistent(
            "a program operation on a seq space runs without an operation order",
        ))?;
        meter.reserve_buffer::<PathSegment>(order.path.len())?;
        let mut path = order.path.to_vec();
        path.pop().ok_or(SpaceError::Inconsistent(
            "the operation order of a job has no step",
        ))?;
        Ok(Some(Self {
            registry: Arc::clone(registry),
            space: space.sid,
            path,
            pending: BTreeSet::new(),
        }))
    }

    /// Claims `channel` in the role `role` (SQ-3). A channel that is not claimable in that role,
    /// or that belongs to another space, claims nothing. The claim is admitted when the channel
    /// has no claim or its claim is a prefix of the participant path of the job, and it is then
    /// pending until [`SeqClaims::store`]. Otherwise the deploy aborts with `SeqConcurrent`. The
    /// lookups, the comparison and the copy of the id are reserved before they are made (SQ-8).
    pub fn claim(
        &mut self,
        channel: &Par,
        role: ClaimRole,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let Some(id) = sole_gprivate_id(channel) else {
            return Ok(());
        };
        let claimable = match (decode_reified_id(id), role) {
            (Some(ReifiedId::Data(space, _)), ClaimRole::Data | ClaimRole::Waiter)
            | (Some(ReifiedId::PathIndex(space, _)), ClaimRole::Waiter) => {
                space.sid == self.space && space.durability == Durability::Ephemeral
            }
            (Some(_), _) | (None, _) => false,
        };
        if !claimable {
            return Ok(());
        }
        meter.ordered_lookup(self.pending.len(), id.len())?;
        if self.pending.contains(id) {
            return Ok(());
        }
        meter.ordered_lookup(self.registry.claim_count(), id.len())?;
        if let Some(claim) = self.registry.claim_of(id) {
            meter.compare(claim.len() * SEGMENT_BYTES, self.path.len() * SEGMENT_BYTES)?;
            if !self.path.starts_with(&claim) {
                return Err(SpaceError::SeqConcurrent { space: self.space });
            }
        }
        meter.tree_growth::<Vec<u8>, ()>(self.pending.len(), 1)?;
        meter.reserve_buffer::<u8>(id.len())?;
        self.pending.insert(id.to_vec());
        Ok(())
    }

    /// Stores the pending claims with the participant path of the job (SQ-4). The job calls it
    /// only after its commit succeeded.
    pub fn store(self) { self.registry.store_claims(self.pending, &self.path); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::interpreter::spaces::namespace::{
        self, gprivate, CollectionKey, NameTail, SID_LEN,
    };

    fn space(seed: u8, durability: Durability) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability,
        }
    }

    fn key(bytes: &[u8]) -> CollectionKey {
        CollectionKey::from_bytes(bytes.to_vec()).expect("a collection key")
    }

    #[test]
    fn ephemeral_space_accepts_every_space_scoped_ephemeral_kind() {
        let temp = space(3, Durability::Ephemeral);
        let ids = [
            namespace::config_channel(temp),
            namespace::space_counter(temp),
            namespace::collection_counter(temp, &key(&[1])),
            namespace::data_channel(temp, &key(&[1])),
            namespace::path_index_channel(temp, &[2]),
            namespace::allocator_channel(temp),
            namespace::handle_channel(temp),
            namespace::name_id(temp, NameTail::Random([4; 32])),
            namespace::name_id(temp, NameTail::Index(7)),
        ];
        for id in ids {
            assert_eq!(ephemeral_space(&id), Some(temp), "{id:?}");
        }
    }

    #[test]
    fn ephemeral_space_rejects_durable_ids_ops_and_foreign_ids() {
        let durable = space(3, Durability::Durable);
        let ids = [
            namespace::config_channel(durable),
            namespace::data_channel(durable, &key(&[1])),
            namespace::handle_channel(durable),
            namespace::name_id(durable, NameTail::Random([4; 32])),
            namespace::op_channel(namespace::HANDLE_OP),
            vec![1; 32],
            Vec::new(),
        ];
        for id in ids {
            assert_eq!(ephemeral_space(&id), None, "{id:?}");
        }
    }

    #[test]
    fn volatile_registry_records_only_ephemeral_lock_keys_in_id_order() {
        let registry = VolatileRegistry::default();
        let temp = space(9, Durability::Ephemeral);
        let durable = space(9, Durability::Durable);
        let data = gprivate(namespace::data_channel(temp, &key(&[5])));
        let config = gprivate(namespace::config_channel(temp));
        registry.record_locked(&[
            gprivate(namespace::config_channel(durable)),
            data.clone(),
            config.clone(),
            data.clone(),
            Par::default(),
        ]);
        assert!(registry.has_channels());
        let mut expected = vec![config, data];
        expected.sort_by_key(|par| sole_gprivate_id(par).expect("an id").to_vec());
        assert_eq!(registry.take_channels(), expected);
        assert!(!registry.has_channels());
        assert!(registry.take_channels().is_empty());
    }

    /// Runs `body` with the claims of a program job on `space`, inside the operation order
    /// whose path is `path` followed by the step `(9, 0)`. So the participant path of the job is
    /// `path`.
    async fn job<T>(
        registry: &Arc<VolatileRegistry>,
        space: NativeSpace,
        path: &[PathSegment],
        body: impl FnOnce(SeqClaims) -> T,
    ) -> T {
        let mut order = path.to_vec();
        order.push((9, 0));
        let order = operation_context::OperationOrder {
            session: [7; 32],
            path: order.into(),
        };
        operation_context::scope(order, async {
            let claims = SeqClaims::open(
                registry,
                space,
                SpaceQualifier::Seq,
                false,
                &ScanMeter::new(None),
            )
            .expect("a seq job opens its claims")
            .expect("a program job on a seq space claims");
            body(claims)
        })
        .await
    }

    fn data(space: NativeSpace, byte: u8) -> Par {
        gprivate(namespace::data_channel(space, &key(&[byte])))
    }

    #[tokio::test]
    async fn seq_claims_apply_only_to_program_jobs_on_seq_spaces() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let meter = ScanMeter::new(None);
        for (qualifier, internal) in [
            (SpaceQualifier::Default, false),
            (SpaceQualifier::Temp, false),
            (SpaceQualifier::Seq, true),
        ] {
            let claims = SeqClaims::open(&registry, seq, qualifier, internal, &meter);
            assert!(matches!(claims, Ok(None)), "{qualifier:?} {internal}");
        }
        let unordered = SeqClaims::open(&registry, seq, SpaceQualifier::Seq, false, &meter);
        assert!(matches!(
            unordered,
            Err(SpaceError::Inconsistent(
                "a program operation on a seq space runs without an operation order"
            ))
        ));
        let opened = job(&registry, seq, &[(0, 1), (4, 2)], |claims| {
            claims.path.clone()
        })
        .await;
        assert_eq!(
            opened,
            vec![(0, 1), (4, 2)],
            "the participant path of the job"
        );
    }

    #[tokio::test]
    async fn seq_claims_admit_exactly_the_descendants_of_the_last_use() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let x = data(seq, 1);
        let id = sole_gprivate_id(&x).expect("an id").to_vec();
        let meter = ScanMeter::new(None);
        let used =
            |mut claims: SeqClaims| claims.claim(&x, ClaimRole::Data, &meter).map(|_| claims);
        // A job that fails or drops its transaction stores no claim (SQ-4).
        job(&registry, seq, &[(0, 1)], |claims| {
            used(claims).expect("a first use")
        })
        .await;
        assert_eq!(
            registry.claim_of(&id),
            None,
            "a pending claim is not stored"
        );
        // A job stores its claims after its commit succeeded.
        job(&registry, seq, &[(0, 1)], |claims| {
            used(claims).expect("a first use").store()
        })
        .await;
        assert_eq!(registry.claim_of(&id), Some(vec![(0, 1)]));
        // The same participant and its descendants use x again.
        for path in [vec![(0, 1)], vec![(0, 1), (2, 3)]] {
            job(&registry, seq, &path, |claims| {
                used(claims).expect("a descendant")
            })
            .await;
        }
        job(&registry, seq, &[(0, 1), (2, 3)], |claims| {
            used(claims).expect("a child").store()
        })
        .await;
        assert_eq!(registry.claim_of(&id), Some(vec![(0, 1), (2, 3)]));
        // A sibling, the parent and an unrelated participant are concurrent.
        for path in [vec![(0, 1), (2, 4)], vec![(0, 1)], vec![(0, 2)], vec![]] {
            let refused = job(&registry, seq, &path, |claims| used(claims).err()).await;
            assert_eq!(
                refused,
                Some(SpaceError::SeqConcurrent { space: seq.sid }),
                "{path:?}"
            );
        }
        // Another channel of the space has its own claim (decision 73).
        job(&registry, seq, &[(0, 2)], |mut claims| {
            claims
                .claim(&data(seq, 2), ClaimRole::Data, &meter)
                .expect("another channel");
            claims.store();
        })
        .await;
        registry.clear();
        assert_eq!(registry.claim_of(&id), None, "a clear forgets the claims");
    }

    #[tokio::test]
    async fn seq_claims_claim_only_user_channels_of_their_space() {
        let registry = Arc::new(VolatileRegistry::default());
        let seq = space(4, Durability::Ephemeral);
        let other = space(5, Durability::Ephemeral);
        let durable = space(4, Durability::Durable);
        let meter = ScanMeter::new(None);
        let index = gprivate(namespace::path_index_channel(seq, &[3]));
        let pending = job(&registry, seq, &[(0, 1)], |mut claims| {
            for (channel, role) in [
                (gprivate(namespace::config_channel(seq)), ClaimRole::Waiter),
                (gprivate(namespace::space_counter(seq)), ClaimRole::Waiter),
                (
                    gprivate(namespace::collection_counter(seq, &key(&[1]))),
                    ClaimRole::Data,
                ),
                (gprivate(namespace::allocator_channel(seq)), ClaimRole::Data),
                (gprivate(namespace::handle_channel(seq)), ClaimRole::Waiter),
                (
                    gprivate(namespace::name_id(seq, NameTail::Random([1; 32]))),
                    ClaimRole::Data,
                ),
                (index.clone(), ClaimRole::Data),
                (data(other, 1), ClaimRole::Data),
                (data(durable, 1), ClaimRole::Data),
                (Par::default(), ClaimRole::Waiter),
            ] {
                claims
                    .claim(&channel, role, &meter)
                    .expect("nothing to claim");
            }
            assert!(claims.pending.is_empty(), "{:?}", claims.pending);
            claims
                .claim(&index, ClaimRole::Waiter, &meter)
                .expect("a waiter on a prefix");
            claims
                .claim(&data(seq, 1), ClaimRole::Data, &meter)
                .expect("a data channel");
            claims.pending.len()
        })
        .await;
        assert_eq!(pending, 2);
    }

    #[test]
    fn volatile_registry_records_spaces_until_it_is_cleared() {
        let registry = VolatileRegistry::default();
        let temp = space(2, Durability::Ephemeral);
        assert!(!registry.is_live(&temp.sid));
        registry.record_space(temp, SpaceQualifier::Temp);
        registry.record_locked(&[gprivate(namespace::config_channel(temp))]);
        assert!(registry.is_live(&temp.sid));
        assert_eq!(registry.qualifier(&temp.sid), Some(SpaceQualifier::Temp));
        assert_eq!(registry.take_channels().len(), 1);
        assert!(
            registry.is_live(&temp.sid),
            "taking the channels keeps the spaces"
        );
        registry.record_locked(&[gprivate(namespace::config_channel(temp))]);
        registry.clear();
        assert!(!registry.is_live(&temp.sid));
        assert!(!registry.has_channels());
    }
}
