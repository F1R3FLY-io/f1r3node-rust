//! The HashSet outer store of the native spaces (P7 design v3, section 6, HS-1 to HS-8).
//!
//! A HashSet space is the HashMap store restricted to empty messages (decision 73, HS-A). The
//! key of a channel, the data channel `D(sid,key)` (`d(sid,key)` in an ephemeral space), the
//! counter channel and the random-tail names of `gensym` are those of the HashMap store, so the
//! store delegates them to [`HashMapOuter`] (HS-1). Two rules restrict the operations, before
//! any staging:
//!
//! * HS-2: a send carries the empty payload, `x!()`. Any other payload fails with
//!   `PayloadNotEmpty`.
//! * HS-3: every receive bind has the empty pattern list, with no binder and no remainder,
//!   `for ( <- x)`. A pattern, a binder or a remainder fails with `PatternNotEmpty`.
//!
//! An empty pattern list matches exactly the empty payload (`fold_match`), so every record of a
//! HashSet collection is a unit, and every waiter matches every unit. The inner collection then
//! orders the units and the waiters as in every other store (HS-5): `set` keeps one unit class
//! per channel, `bag` counts the units, `cell` holds one, and `queue`, `stack` and
//! `priorityqueue` order them. Joins run in the keyed join engine (HS-4). Every HashSet space
//! has the qualifier `seq` (`SpaceFactoryUrn::supports_qualifier`), so the Seq rules apply to
//! it (HS-6).
//!
//! Each check reads one length or one shape.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{BindPattern, ListParWithRandom, Par};

use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::OuterStore;
use crate::rust::interpreter::spaces::namespace::{CollectionKey, NativeSpace};
use crate::rust::interpreter::spaces::outer::hashmap::HashMapOuter;

/// The HashSet outer store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HashSetOuter;

impl OuterStore for HashSetOuter {
    const NAME: &'static str = "hashset";

    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> { HashMapOuter.key(channel) }

    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        HashMapOuter.data_channel(space, key)
    }

    fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        HashMapOuter.counter_channel(space, key)
    }

    /// HS-2: the payload of a send is empty.
    fn check_payload(&self, data: &ListParWithRandom) -> Result<(), SpaceError> {
        match data.pars.as_slice() {
            [] => Ok(()),
            [_, ..] => Err(SpaceError::PayloadNotEmpty),
        }
    }

    /// HS-3: the pattern list of a bind is empty, and the bind has no binder and no remainder.
    fn check_patterns(&self, pattern: &BindPattern) -> Result<(), SpaceError> {
        match (
            pattern.patterns.as_slice(),
            &pattern.remainder,
            pattern.free_count,
        ) {
            ([], None, 0) => Ok(()),
            _ => Err(SpaceError::PatternNotEmpty),
        }
    }

    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        HashMapOuter.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::Var;
    use models::rust::utils::{
        new_freevar_par, new_freevar_var, new_gint_par, new_gstring_par, new_wildcard_par,
    };

    use super::*;
    use crate::rust::interpreter::spaces::namespace::{Durability, SID_LEN};

    const SPACE: NativeSpace = NativeSpace {
        sid: [9; SID_LEN],
        durability: Durability::Ephemeral,
    };

    fn payload(pars: Vec<Par>) -> ListParWithRandom {
        ListParWithRandom {
            pars,
            random_state: Vec::new(),
            cost_authority: None,
            cost_stack: None,
        }
    }

    fn bind(patterns: Vec<Par>, remainder: Option<Var>, free_count: i32) -> BindPattern {
        BindPattern {
            patterns,
            remainder,
            free_count,
        }
    }

    #[test]
    fn space_hashset_keys_channels_and_names_are_those_of_the_hashmap_store() {
        let channel = new_gstring_par("member".to_string(), Vec::new(), false);
        let key = HashSetOuter.key(&channel).expect("a key");
        assert_eq!(key, HashMapOuter.key(&channel).expect("a key"));
        assert_eq!(
            HashSetOuter.data_channel(SPACE, &key),
            HashMapOuter.data_channel(SPACE, &key)
        );
        assert_eq!(
            HashSetOuter.counter_channel(SPACE, &key),
            HashMapOuter.counter_channel(SPACE, &key)
        );
        let call = Blake2b512Random::create_from_bytes(b"hashset gensym");
        assert_eq!(
            HashSetOuter.gensym(SPACE, &call),
            HashMapOuter.gensym(SPACE, &call)
        );
        assert_eq!(HashSetOuter::NAME, "hashset");
        assert!(!HashSetOuter::ALLOCATOR);
        assert!(!HashSetOuter::ALWAYS_ESCALATES);
    }

    #[test]
    fn space_hashset_accepts_exactly_the_empty_payload() {
        assert_eq!(HashSetOuter.check_payload(&payload(Vec::new())), Ok(()));
        let unit_par = Par::default();
        for pars in [
            vec![unit_par],
            vec![new_gint_par(1, Vec::new(), false)],
            vec![
                new_gint_par(1, Vec::new(), false),
                new_gint_par(2, Vec::new(), false),
            ],
        ] {
            assert_eq!(
                HashSetOuter.check_payload(&payload(pars)),
                Err(SpaceError::PayloadNotEmpty)
            );
        }
        // The HashMap store keeps the default hook, which accepts every payload.
        assert_eq!(
            HashMapOuter.check_payload(&payload(vec![new_gint_par(1, Vec::new(), false)])),
            Ok(())
        );
    }

    #[test]
    fn space_hashset_accepts_exactly_the_empty_pattern_list() {
        assert_eq!(
            HashSetOuter.check_patterns(&bind(Vec::new(), None, 0)),
            Ok(())
        );
        let remainder: Var = new_freevar_var(0);
        for pattern in [
            bind(vec![new_freevar_par(0, Vec::new())], None, 1),
            bind(vec![new_wildcard_par(Vec::new(), true)], None, 0),
            bind(vec![new_gint_par(42, Vec::new(), false)], None, 0),
            bind(Vec::new(), Some(remainder), 1),
            bind(Vec::new(), None, 1),
        ] {
            assert_eq!(
                HashSetOuter.check_patterns(&pattern),
                Err(SpaceError::PatternNotEmpty),
                "{pattern:?}"
            );
        }
        assert_eq!(
            HashMapOuter.check_patterns(&bind(vec![new_freevar_par(0, Vec::new())], None, 1)),
            Ok(())
        );
    }
}
