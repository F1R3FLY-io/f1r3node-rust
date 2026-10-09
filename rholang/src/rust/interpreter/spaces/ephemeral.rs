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
//! * The factory records a space after its creation commits and before it replies, so every
//!   holder of the handle or of a name of the space causally follows the record (VR-2).
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

use std::collections::BTreeMap;
use std::sync::Mutex;

use models::rhoapi::Par;

use super::namespace::{
    decode_reified_id, sole_gprivate_id, Durability, NativeSpace, ReifiedId, SpaceId,
};
use super::types::qualifier::SpaceQualifier;

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

    /// Empties the book.
    pub fn clear(&self) {
        let mut state = self.state();
        state.spaces.clear();
        state.channels.clear();
    }
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
        assert!(registry.is_live(&temp.sid), "taking the channels keeps the spaces");
        registry.record_locked(&[gprivate(namespace::config_channel(temp))]);
        registry.clear();
        assert!(!registry.is_live(&temp.sid));
        assert!(!registry.has_channels());
    }
}
