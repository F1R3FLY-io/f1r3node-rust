//! The HashMap outer store of the native spaces (P4 design B.4 and B.8).
//!
//! A HashMap space keeps one collection for each channel. The key of a channel is its canonical
//! inner bytes (A5), so channels that are equal as `Par` values share one collection, and the
//! order of a list stays significant. The records and the waiting continuations of the
//! collection `key` live on `D(sid,key)`, and its counter on `S(sid,key)`. A name that `gensym`
//! makes has a random tail, so it routes to the space by its own bytes (I-3).
//!
//! The store keeps the defaults of the outer-store hooks of P5 (P5 design v3, A.3.3). It
//! accepts every routed key, it stages no records at a creation, and it has no allocator, so
//! every allocation in a HashMap space is `Allocation::Baseline`.

// Changed by decision 128 (kind routing): the store takes the routed layout of A5 and the
// random-tail names of P4, which are the defaults of `OuterStore`, so it imports only the trait.
// The text before:
// ```
// use crypto::rust::hash::blake2b512_random::Blake2b512Random;
// use models::rhoapi::Par;
//
// use crate::rust::interpreter::spaces::errors::SpaceError;
// use crate::rust::interpreter::spaces::generic::OuterStore;
// use crate::rust::interpreter::spaces::namespace::{
//     self, CollectionKey, NameTail, NativeSpace, RandomPurpose,
// };
// ```
use crate::rust::interpreter::spaces::generic::OuterStore;

/// The HashMap outer store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HashMapOuter;

impl OuterStore for HashMapOuter {
    const NAME: &'static str = "hashmap";

    // Changed by decision 128 (kind routing): the routed layout of A5 and the random-tail names of
    // P4 are the defaults of `OuterStore`, with these bodies, so the store keeps its name only.
    // The text before:
    // ```
    // fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> {
    //     CollectionKey::of_channel(channel)
    // }
    //
    // fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
    //     namespace::gprivate(namespace::data_channel(space, key))
    // }
    //
    // fn counter_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
    //     namespace::gprivate(namespace::collection_counter(space, key))
    // }
    //
    // /// `N(sid ‖ tail)` with the 32-byte tail `random.split_byte(0xE0).split_byte(GENSYM).next()`
    // /// (P4 design C.8). It touches no storage.
    // fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
    //     let mut tail = [0u8; 32];
    //     for (byte, value) in tail
    //         .iter_mut()
    //         .zip(RandomPurpose::Gensym.derive(random).next())
    //     {
    //         *byte = value as u8;
    //     }
    //     Ok(namespace::gprivate(namespace::name_id(
    //         space,
    //         NameTail::Random(tail),
    //     )))
    // }
    // ```
}

#[cfg(test)]
mod tests {
    // Added by decision 128 (kind routing): the store module no longer imports these names.
    use crypto::rust::hash::blake2b512_random::Blake2b512Random;
    use models::rhoapi::expr::ExprInstance;
    // Changed by decision 128 (kind routing): the tests name `Par` themselves. The text before:
    // use models::rhoapi::{EList, Expr};
    use models::rhoapi::{EList, Expr, Par};
    use models::rust::utils::{new_gint_par, new_gstring_par};

    use super::*;
    use crate::rust::interpreter::reified::route::{classify_channel, ChannelClass};
    // Changed by decision 128 (kind routing): the tests name the types of the layout. The text
    // before:
    // ```
    // use crate::rust::interpreter::spaces::namespace::{
    //     canonical_inner_channel_bytes, decode_reified_id, reified_id_of, sole_gprivate_id,
    //     Durability, ReifiedId, REIFIED_TAG, SID_LEN,
    // };
    // ```
    use crate::rust::interpreter::spaces::namespace::{
        canonical_inner_channel_bytes, decode_reified_id, reified_id_of, sole_gprivate_id,
        CollectionKey, Durability, NameTail, NativeSpace, ReifiedId, REIFIED_TAG, SID_LEN,
    };

    const SPACE: NativeSpace = NativeSpace {
        sid: [8; SID_LEN],
        durability: Durability::Durable,
    };

    fn list(ps: Vec<Par>) -> Par {
        Par::default().with_exprs(vec![Expr {
            expr_instance: Some(ExprInstance::EListBody(EList {
                ps,
                locally_free: Vec::new(),
                connective_used: false,
                remainder: None,
            })),
        }])
    }

    #[test]
    fn space_hashmap_keys_are_the_canonical_channel_bytes() {
        let channel = new_gstring_par("queue".to_string(), Vec::new(), false);
        let key = HashMapOuter.key(&channel).expect("a key");
        assert_eq!(
            key.as_bytes(),
            canonical_inner_channel_bytes(&channel)
                .expect("canonical bytes")
                .as_slice()
        );
        assert_eq!(key, CollectionKey::of_channel(&channel).expect("a key"));
        let one = new_gint_par(1, Vec::new(), false);
        let two = new_gint_par(2, Vec::new(), false);
        let mut forward = one.clone();
        forward.exprs.extend(two.exprs.clone());
        let mut backward = two.clone();
        backward.exprs.extend(one.exprs.clone());
        assert_eq!(
            HashMapOuter.key(&forward).expect("a key"),
            HashMapOuter.key(&backward).expect("a key")
        );
        assert_ne!(
            HashMapOuter
                .key(&list(vec![one.clone(), two.clone()]))
                .expect("a key"),
            HashMapOuter.key(&list(vec![two, one])).expect("a key")
        );
        assert_eq!(HashMapOuter::NAME, "hashmap");
    }

    #[test]
    fn space_hashmap_channels_hold_the_collection_of_their_key() {
        let key = HashMapOuter
            .key(&new_gstring_par("queue".to_string(), Vec::new(), false))
            .expect("a key");
        let data = HashMapOuter.data_channel(SPACE, &key);
        let counter = HashMapOuter.counter_channel(SPACE, &key);
        assert_eq!(
            reified_id_of(&data),
            Some(ReifiedId::Data(SPACE, key.as_bytes()))
        );
        assert_eq!(
            reified_id_of(&counter),
            Some(ReifiedId::CollectionCounter(SPACE, key.as_bytes()))
        );
        assert_ne!(data, counter);
        let other = HashMapOuter
            .key(&new_gstring_par("other".to_string(), Vec::new(), false))
            .expect("a key");
        assert_ne!(data, HashMapOuter.data_channel(SPACE, &other));
        assert_ne!(counter, HashMapOuter.counter_channel(SPACE, &other));
        assert_eq!(classify_channel(&data), ChannelClass::Storage);
        assert_eq!(classify_channel(&counter), ChannelClass::Storage);
    }

    #[test]
    fn space_hashmap_gensym_names_route_to_their_space() {
        let call = Blake2b512Random::create_from_bytes(b"gensym call");
        let name = HashMapOuter.gensym(SPACE, &call).expect("a name");
        let mut draw = call.split_byte(REIFIED_TAG).split_byte(0x02);
        let mut tail = [0u8; 32];
        for (byte, value) in tail.iter_mut().zip(draw.next()) {
            *byte = value as u8;
        }
        let id = sole_gprivate_id(&name).expect("a name is one GPrivate");
        assert_eq!(
            decode_reified_id(id),
            Some(ReifiedId::Name(SPACE, NameTail::Random(tail)))
        );
        assert_eq!(classify_channel(&name), ChannelClass::SpaceName(SPACE));
        assert_eq!(HashMapOuter.gensym(SPACE, &call), Ok(name.clone()));
        let other = Blake2b512Random::create_from_bytes(b"another call");
        assert_ne!(HashMapOuter.gensym(SPACE, &other), Ok(name));
    }
}
