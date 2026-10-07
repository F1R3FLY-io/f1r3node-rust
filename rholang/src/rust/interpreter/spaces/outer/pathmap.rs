//! The PathMap outer store of the native spaces (P5e: plan "PathMap delivery", decision 9.3,
//! F9, P5 design v2 3.4 and v3 A.11).
//!
//! In a PathMap space a channel names a path, and a send on a path is also a send on every
//! prefix of the path (FIPS, "PathMap (For MeTTa)"). The path of a channel is
//! `spaces::pathmap::channel_path`: the elements of a plain list, and `[c]` for every other
//! channel `c`. So `@c` and `@[c]` name one path, and a fresh name roots a private subtree.
//!
//! The store keeps the state of a space on these channels:
//!
//! | Channel                       | Holds                                                      |
//! |-------------------------------|------------------------------------------------------------|
//! | `D(sid, path_key(p))`         | the datum records of the path `p`                          |
//! | `I(sid, prefix_key(q))`       | a marker for each datum under `q`, and the waiters on `q`  |
//! | `S(sid)`                      | the token of the space                                     |
//!
//! The token is the space counter `S(sid)`. The generic creation stages it with the value 0, so
//! a PathMap creation stages no other record ([`OuterStore::creation_records`]). Every operation
//! of an ordered discipline bumps the token, and so does every state change of a bag or a set
//! space (plan, "Per-space token"). A PathMap space has no collection counter: the counter channel
//! of every path is the token. A PathMap space has no allocator.
//!
//! **Keys.** The key of a path is its path key, so the data channel of the key of `@c` is
//! `D(sid, path_key(channel_path(c)))`. Routing keys an operation by the canonical bytes of its
//! channel (A5), because it runs before the family of the space is known. A PathMap operation
//! does not use that routed key: it derives every key from its channel. The PathMap engine opens
//! its own transaction over the channels of its paths, so it never uses `CollectionChannels`.
//!
//! **The path rules.** [`OuterStore::check_key`] accepts a channel only when its path has at most
//! `PATH_DEPTH_CAP = 64` elements (`PathTooDeep`, F9) and no element holds an `EPathMap` or an
//! `EZipper` at any depth (`UnsupportedPathElement`). The check runs before any staging. So every
//! prefix of an accepted path is accepted too, and a datum has at most 65 markers
//! (`PathMapPrefix.v`, `path_element_rules`).
//!
//! Every builder of this module has a metered form that reserves the name template and the id
//! buffer of each channel before it makes the channel (decision 42).

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::{CostAuthority, ListParWithRandom, Par};
use rspace_plus_plus::rspace::errors::RSpaceError;

use super::hashmap::HashMapOuter;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::spaces::errors::SpaceError;
use crate::rust::interpreter::spaces::generic::OuterStore;
use crate::rust::interpreter::spaces::namespace::{self, CollectionKey, NativeSpace};
use crate::rust::interpreter::spaces::pathmap::{channel_path, encode_path, path_key, EncodedPath};
use crate::rust::interpreter::spaces::types::config::SpaceConfig;

/// The PathMap outer store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PathMapOuter;

/// The channels of one path `p` of a PathMap space: the data channel `D(sid, path_key(p))` and
/// the index channel `I(sid, prefix_key(q))` of each prefix `q ⪯ p`, in depth order from `ε` to
/// `p`.
#[derive(Clone, Debug, PartialEq)]
pub struct PathChannels {
    data: Par,
    index: Vec<Par>,
}

impl PathChannels {
    /// The data channel `D(sid, path_key(p))`.
    pub fn data(&self) -> &Par { &self.data }

    /// The index channel of the prefix of `depth` elements, or `None` when the path is shorter.
    pub fn index(&self, depth: usize) -> Option<&Par> { self.index.get(depth) }

    /// The index channels of every prefix, in depth order from `ε` to the whole path.
    pub fn indexes(&self) -> &[Par] { &self.index }

    /// The number of elements of the path. A path of `n` elements has `n + 1` index channels.
    pub fn depth(&self) -> usize { self.index.split_first().map_or(0, |(_, rest)| rest.len()) }
}

impl PathMapOuter {
    /// The token `S(sid)` of `space`: its space counter.
    pub fn token(space: NativeSpace) -> Par { namespace::gprivate(namespace::space_counter(space)) }

    /// The data channel `D(sid, path_key)` of the path whose key is `path_key`.
    pub fn path_data(space: NativeSpace, path_key: &[u8]) -> Par {
        namespace::gprivate(namespace::path_data_channel(space, path_key))
    }

    /// The index channel `I(sid, prefix_key)` of the prefix whose key is `prefix_key`. The key of
    /// the prefix `ε` is empty, so its index channel has the 34 bytes of a space-level id.
    pub fn index(space: NativeSpace, prefix_key: &[u8]) -> Par {
        namespace::gprivate(namespace::path_index_channel(space, prefix_key))
    }

    /// [`PathMapOuter::token`] with the name and its id buffer reserved first.
    pub fn token_metered(space: NativeSpace, meter: &ScanMeter) -> Result<Par, SpaceError> {
        reserve_name(0, meter)?;
        Ok(Self::token(space))
    }

    /// [`PathMapOuter::path_data`] with the name and its id buffer reserved first.
    pub fn path_data_metered(
        space: NativeSpace,
        path_key: &[u8],
        meter: &ScanMeter,
    ) -> Result<Par, SpaceError> {
        reserve_name(path_key.len(), meter)?;
        Ok(Self::path_data(space, path_key))
    }

    /// [`PathMapOuter::index`] with the name and its id buffer reserved first.
    pub fn index_metered(
        space: NativeSpace,
        prefix_key: &[u8],
        meter: &ScanMeter,
    ) -> Result<Par, SpaceError> {
        reserve_name(prefix_key.len(), meter)?;
        Ok(Self::index(space, prefix_key))
    }

    /// The channels of the path that `encoded` holds: its data channel and the index channel of
    /// each of its prefixes in depth order. Each name and the list of the index channels are
    /// reserved first.
    pub fn path_channels(
        space: NativeSpace,
        encoded: &EncodedPath,
        meter: &ScanMeter,
    ) -> Result<PathChannels, SpaceError> {
        let data = Self::path_data_metered(space, encoded.path_key(), meter)?;
        let count = encoded
            .depth()
            .checked_add(1)
            .ok_or(RSpaceError::HostWorkRejected)?;
        let mut index = meter.buffer::<Par>(count)?;
        for prefix_key in encoded.prefix_keys() {
            index.push(Self::index_metered(space, prefix_key?, meter)?);
        }
        Ok(PathChannels { data, index })
    }
}

/// Reserves a `GPrivate` name and its id of a space-scoped id with a tail of `tail` bytes, as
/// `CollectionChannels::new_metered` reserves the channels of a collection.
fn reserve_name(tail: usize, meter: &ScanMeter) -> Result<(), SpaceError> {
    let bytes = namespace::SPACE_ID_PREFIX_LEN
        .checked_add(tail)
        .ok_or(RSpaceError::HostWorkRejected)?;
    meter.reserve_copy(&namespace::gprivate(Vec::new()))?;
    meter.reserve_buffer::<u8>(bytes)?;
    Ok(())
}

impl OuterStore for PathMapOuter {
    const NAME: &'static str = "pathmap";

    /// The path key of the path of `channel` (module docs, "Keys"). A path that breaks a path
    /// rule fails as [`OuterStore::check_key`] fails. A path key ends with its terminator, so it
    /// is never empty.
    fn key(&self, channel: &Par) -> Result<CollectionKey, SpaceError> {
        let unmetered = ScanMeter::new(None);
        let key = path_key(channel_path(channel, &unmetered)?, &unmetered)?;
        CollectionKey::from_bytes(key).ok_or(SpaceError::Inconsistent("a path key is empty"))
    }

    /// Accepts `channel` only when its path follows the path rules (module docs): at most
    /// `PATH_DEPTH_CAP` elements (`PathTooDeep`), and no element that holds an `EPathMap` or an
    /// `EZipper` at any depth (`UnsupportedPathElement`). The routed key is not part of the check,
    /// because the PathMap engine derives its keys from the channel. The read of the channel and
    /// the encode of the path are reserved before they are made.
    fn check_key(
        &self,
        _config: &SpaceConfig,
        channel: &Par,
        _key: &CollectionKey,
        meter: &ScanMeter,
    ) -> Result<(), SpaceError> {
        let path = channel_path(channel, meter)?;
        encode_path(path, meter).map(drop)
    }

    /// The data channel `D(sid, key)` of the path whose path key is `key`.
    fn data_channel(&self, space: NativeSpace, key: &CollectionKey) -> Par {
        Self::path_data(space, key.as_bytes())
    }

    /// The token `S(sid)`: a PathMap space has no collection counter.
    fn counter_channel(&self, space: NativeSpace, _key: &CollectionKey) -> Par {
        Self::token(space)
    }

    /// A PathMap space stages no record of its own at its creation. Its token is the space
    /// counter `S(sid) = 0`, which the generic creation stages, and its index channels hold only
    /// markers and waiters, so they need no first record.
    fn creation_records(
        &self,
        _space: NativeSpace,
        _random: &Blake2b512Random,
        _authority: &CostAuthority,
        _meter: &ScanMeter,
    ) -> Result<Vec<(Par, ListParWithRandom)>, SpaceError> {
        Ok(Vec::new())
    }

    /// A name `N(sid ‖ tail)` with a random tail, as the HashMap store makes it. The name is not a
    /// list, so its path is the one element `[N]`, and it roots a private subtree.
    fn gensym(&self, space: NativeSpace, random: &Blake2b512Random) -> Result<Par, SpaceError> {
        HashMapOuter.gensym(space, random)
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::g_unforgeable::UnfInstance;
    use models::rhoapi::{EList, EPathMap, ETuple, EZipper, Expr, GPrivate, GUnforgeable, Par};
    use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};

    use super::*;
    use crate::rust::interpreter::host_work::HostWorkBudget;
    use crate::rust::interpreter::reified::route::{classify_channel, ChannelClass};
    use crate::rust::interpreter::spaces::namespace::{
        decode_reified_id, reified_id_of, sole_gprivate_id, Durability, NameTail, RandomPurpose,
        ReifiedId, REIFIED_TAG, SID_LEN,
    };
    use crate::rust::interpreter::spaces::pathmap::PATH_DEPTH_CAP;
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    const SPACE: NativeSpace = NativeSpace {
        sid: [9; SID_LEN],
        durability: Durability::Durable,
    };

    fn expr(instance: ExprInstance) -> Par {
        Par {
            exprs: vec![Expr {
                expr_instance: Some(instance),
            }],
            ..Par::default()
        }
    }

    fn gint(value: i64) -> Par { expr(ExprInstance::GInt(value)) }

    fn gbool(value: bool) -> Par { expr(ExprInstance::GBool(value)) }

    fn gstring(text: &str) -> Par { expr(ExprInstance::GString(text.to_string())) }

    fn guri(text: &str) -> Par { expr(ExprInstance::GUri(text.to_string())) }

    fn gbytes(bytes: &[u8]) -> Par { expr(ExprInstance::GByteArray(bytes.to_vec())) }

    fn gdouble(value: f64) -> Par { expr(ExprInstance::GDouble(value.to_bits())) }

    fn glist(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EListBody(EList {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    fn gtuple(ps: Vec<Par>) -> Par {
        expr(ExprInstance::ETupleBody(ETuple {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
        }))
    }

    fn gprivate(id: &[u8]) -> Par {
        Par {
            unforgeables: vec![GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id: id.to_vec() })),
            }],
            ..Par::default()
        }
    }

    fn pathmap(ps: Vec<Par>) -> Par {
        expr(ExprInstance::EPathmapBody(EPathMap {
            ps,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    fn zipper() -> Par {
        expr(ExprInstance::EZipperBody(EZipper {
            pathmap: Some(EPathMap::default()),
            current_path: Vec::new(),
            is_write_zipper: false,
            locally_free: Vec::new(),
            connective_used: false,
        }))
    }

    fn unmetered() -> ScanMeter { ScanMeter::new(None) }

    fn config() -> SpaceConfig {
        SpaceConfig::new(
            SPACE.sid,
            PathMapOuter::NAME,
            "bag",
            SpaceQualifier::Default,
        )
    }

    fn checked(channel: &Par) -> Result<(), SpaceError> {
        let key = CollectionKey::of_channel(channel).expect("a routed key");
        PathMapOuter.check_key(&config(), channel, &key, &unmetered())
    }

    fn id_of(channel: &Par) -> Vec<u8> {
        sole_gprivate_id(channel)
            .expect("a channel is one GPrivate")
            .to_vec()
    }

    fn data_id(tail: &[u8]) -> Vec<u8> {
        [vec![REIFIED_TAG, b'D'], SPACE.sid.to_vec(), tail.to_vec()].concat()
    }

    fn index_id(tail: &[u8]) -> Vec<u8> {
        [vec![REIFIED_TAG, b'I'], SPACE.sid.to_vec(), tail.to_vec()].concat()
    }

    fn channels_of(channel: &Par) -> PathChannels {
        let path = channel_path(channel, &unmetered()).expect("a channel path");
        let encoded = encode_path(path, &unmetered()).expect("the path encodes");
        PathMapOuter::path_channels(SPACE, &encoded, &unmetered()).expect("the channels")
    }

    // The EPM1 vectors of v2 3.4 (the per-arm pins and the top-level split pins of
    // `canonical_path.rs` at `11c92e7cc`, transcribed): the channels of the store hold the
    // segment bytes of EPM1. The data channel of a channel holds the EPM1 split form of its path,
    // and the index channel of a one-element prefix holds the EPM1 segment of the element.
    #[test]
    fn pathmap_segment_codec_matches_epm1_vectors() {
        let segments: Vec<(Par, Vec<u8>)> = vec![
            (gbool(false), vec![0x01]),
            (gbool(true), vec![0x02]),
            (gint(0), vec![0x03, 0x00]),
            (gint(-1), vec![0x03, 0x01]),
            (gint(1), vec![0x03, 0x02]),
            (gint(5), vec![0x03, 0x0A]),
            (gstring("hi"), vec![0x04, 0x02, b'h', b'i']),
            (gstring(""), vec![0x04, 0x00]),
            (guri(""), vec![0x05, 0x00]),
            (gbytes(&[]), vec![0x06, 0x00]),
            (
                gdouble(1.5),
                [vec![0x07], 1.5f64.to_bits().to_be_bytes().to_vec()].concat(),
            ),
            (gtuple(Vec::new()), vec![0x0C, 0x00]),
            (glist(Vec::new()), vec![0x0B, 0x00]),
            (gprivate(&[0xAA]), vec![0x0E, 0x01, 0xAA]),
        ];
        for (element, segment) in segments {
            let split = [segment.clone(), vec![0x00]].concat();
            // A list channel splits into its elements, so `@[e]` names the path [e].
            let singleton = channels_of(&glist(vec![element.clone()]));
            assert_eq!(id_of(singleton.data()), data_id(&split), "{element:?}");
            assert_eq!(
                id_of(singleton.index(1).expect("the prefix [e]")),
                index_id(&segment)
            );
            assert_eq!(
                id_of(singleton.index(0).expect("the prefix ε")),
                index_id(&[])
            );
            // Every channel that is not a plain list names the path [c] (`@c ≡ @[c]`).
            match element == glist(Vec::new()) {
                true => {}
                false => assert_eq!(channels_of(&element), singleton, "{element:?}"),
            }
        }
        // `-0.0` and `+0.0` are distinct paths.
        assert_ne!(channels_of(&gdouble(-0.0)), channels_of(&gdouble(0.0)));
        // The split multi-segment pin of EPM1, with every prefix.
        let split = channels_of(&glist(vec![gstring("a"), gstring("b")]));
        assert_eq!(
            id_of(split.data()),
            data_id(&[0x04, 0x01, b'a', 0x04, 0x01, b'b', 0x00])
        );
        let prefixes: Vec<Vec<u8>> = split.indexes().iter().map(id_of).collect();
        assert_eq!(prefixes, vec![
            index_id(&[]),
            index_id(&[0x04, 0x01, b'a']),
            index_id(&[0x04, 0x01, b'a', 0x04, 0x01, b'b']),
        ]);
        // The empty list dissolves to the one-byte terminator path, and its only prefix is ε.
        let root = channels_of(&glist(Vec::new()));
        assert_eq!(id_of(root.data()), data_id(&[0x00]));
        assert_eq!(root.depth(), 0);
        assert_eq!(root.indexes().iter().map(id_of).collect::<Vec<_>>(), vec![
            index_id(&[])
        ]);
        // A nested empty list inside a split (the 0x0B arm with k = 0).
        assert_eq!(
            id_of(channels_of(&glist(vec![glist(Vec::new())])).data()),
            data_id(&[0x0B, 0x00, 0x00])
        );
        // The scalar and the singleton list share the EPM1 split path `03 0A 00`.
        assert_eq!(
            id_of(channels_of(&gint(5)).data()),
            data_id(&[0x03, 0x0A, 0x00])
        );
        // The EPM1 map arm 0x0D never occurs: a map is not a path element of this store.
        assert_eq!(
            checked(&pathmap(Vec::new())),
            Err(SpaceError::UnsupportedPathElement)
        );
        assert_eq!(
            checked(&glist(vec![pathmap(vec![pathmap(Vec::new())])])),
            Err(SpaceError::UnsupportedPathElement)
        );
    }

    #[test]
    fn check_key_follows_the_path_rules() {
        assert_eq!(checked(&gint(1)), Ok(()));
        assert_eq!(checked(&glist(vec![gint(0), gint(1), gint(2)])), Ok(()));
        let deepest = glist((0..PATH_DEPTH_CAP as i64).map(gint).collect());
        assert_eq!(checked(&deepest), Ok(()));
        let too_deep = glist((0..=PATH_DEPTH_CAP as i64).map(gint).collect());
        assert_eq!(checked(&too_deep), Err(SpaceError::PathTooDeep));
        assert_eq!(checked(&zipper()), Err(SpaceError::UnsupportedPathElement));
        assert_eq!(
            checked(&glist(vec![
                gint(0),
                gtuple(vec![gstring("a"), pathmap(Vec::new())])
            ])),
            Err(SpaceError::UnsupportedPathElement)
        );
        let mut sent = Par::default();
        sent.sends.push(models::rhoapi::Send {
            chan: Some(gint(1)),
            data: vec![zipper()],
            persistent: false,
            locally_free: Vec::new(),
            connective_used: false,
        });
        assert_eq!(
            checked(&glist(vec![sent])),
            Err(SpaceError::UnsupportedPathElement)
        );
        // An element outside the structural grammar is a path element: it takes the escape arm.
        assert_eq!(
            checked(&glist(vec![gint(1), expr(ExprInstance::GFloat32(0))])),
            Ok(())
        );
    }

    #[test]
    fn the_path_channels_are_storage_channels_of_the_space() {
        let channels = channels_of(&glist(vec![gint(0), gint(1)]));
        assert_eq!(channels.depth(), 2);
        assert_eq!(channels.indexes().len(), 3);
        assert_eq!(channels.index(3), None);
        assert_eq!(
            reified_id_of(channels.data()),
            Some(ReifiedId::Data(SPACE, &[0x03, 0x00, 0x03, 0x02, 0x00][..]))
        );
        for (depth, index) in channels.indexes().iter().enumerate() {
            match reified_id_of(index) {
                Some(ReifiedId::PathIndex(space, tail)) => {
                    assert_eq!(space, SPACE);
                    assert_eq!(tail.len(), 2 * depth);
                }
                other => panic!("an index channel decodes as {other:?}"),
            }
            assert_eq!(classify_channel(index), ChannelClass::Storage);
        }
        assert_eq!(classify_channel(channels.data()), ChannelClass::Storage);
        assert_eq!(
            id_of(channels.index(0).expect("ε")).len(),
            namespace::SPACE_ID_PREFIX_LEN
        );
    }

    #[test]
    fn the_token_is_the_space_counter_and_the_counter_of_every_path() {
        let token = PathMapOuter::token(SPACE);
        assert_eq!(reified_id_of(&token), Some(ReifiedId::SpaceCounter(SPACE)));
        let key = PathMapOuter.key(&gint(1)).expect("a path key");
        let other = PathMapOuter.key(&gint(2)).expect("a path key");
        assert_eq!(PathMapOuter.counter_channel(SPACE, &key), token);
        assert_eq!(PathMapOuter.counter_channel(SPACE, &other), token);
        assert_eq!(
            PathMapOuter.data_channel(SPACE, &key),
            PathMapOuter::path_data(SPACE, &[0x03, 0x02, 0x00])
        );
        assert_eq!(key.as_bytes(), &[0x03, 0x02, 0x00]);
        assert_eq!(
            PathMapOuter.key(&glist(vec![gint(1)])).expect("a path key"),
            key
        );
        assert_eq!(
            PathMapOuter.key(&pathmap(Vec::new())),
            Err(SpaceError::UnsupportedPathElement)
        );
        assert!(!<PathMapOuter as OuterStore>::ALLOCATOR);
        assert_eq!(PathMapOuter::NAME, "pathmap");
    }

    #[test]
    fn a_creation_stages_no_record_of_the_store() {
        let random = Blake2b512Random::create_from_bytes(b"factory call");
        assert_eq!(
            PathMapOuter.creation_records(SPACE, &random, &CostAuthority::default(), &unmetered()),
            Ok(Vec::new())
        );
    }

    #[test]
    fn gensym_names_root_a_private_subtree() {
        let call = Blake2b512Random::create_from_bytes(b"gensym call");
        let name = PathMapOuter.gensym(SPACE, &call).expect("a name");
        let mut tail = [0u8; 32];
        for (byte, value) in tail
            .iter_mut()
            .zip(RandomPurpose::Gensym.derive(&call).next())
        {
            *byte = value as u8;
        }
        assert_eq!(
            decode_reified_id(&id_of(&name)),
            Some(ReifiedId::Name(SPACE, NameTail::Random(tail)))
        );
        assert_eq!(classify_channel(&name), ChannelClass::SpaceName(SPACE));
        let path = channel_path(&name, &unmetered()).expect("a channel path");
        assert_eq!(path, std::slice::from_ref(&name));
        let channels = channels_of(&name);
        let segment = [vec![0x0E, 66], id_of(&name)].concat();
        assert_eq!(id_of(channels.index(1).expect("[N]")), index_id(&segment));
        let other = PathMapOuter
            .gensym(SPACE, &Blake2b512Random::create_from_bytes(b"another call"))
            .expect("a name");
        assert_ne!(channels_of(&other), channels);
    }

    #[test]
    fn the_metered_builders_reserve_their_work() {
        let channel = glist(vec![gint(0), gstring("a"), glist(vec![gint(2)])]);
        let unmetered_channels = channels_of(&channel);
        let run = |meter: &ScanMeter| -> Result<PathChannels, SpaceError> {
            let path = channel_path(&channel, meter)?;
            let encoded = encode_path(path, meter)?;
            PathMapOuter::path_channels(SPACE, &encoded, meter)
        };
        let dimensions = [
            HostWorkDimension::VerificationOperations,
            HostWorkDimension::VerificationBytes,
            HostWorkDimension::SearchStateBytes,
        ];
        let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
        assert_eq!(
            run(&ScanMeter::new(Some(budget.clone()))).expect("a budget with no bound"),
            unmetered_channels
        );
        let usage = dimensions.map(|dimension| budget.usage(dimension).get());
        assert!(usage.iter().all(|used| *used > 0));
        for (dimension, used) in dimensions.into_iter().zip(usage) {
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(dimension, HostWorkLimit::new(used - 1));
            assert_eq!(
                run(&ScanMeter::new(Some(HostWorkBudget::new(limits)))),
                Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
                "a cut of {dimension} rejects the channels"
            );
        }
        for (name, run) in [
            (
                "token",
                Box::new(|meter: &ScanMeter| PathMapOuter::token_metered(SPACE, meter))
                    as Box<dyn Fn(&ScanMeter) -> Result<Par, SpaceError>>,
            ),
            (
                "index",
                Box::new(|meter: &ScanMeter| PathMapOuter::index_metered(SPACE, &[3, 0], meter)),
            ),
        ] {
            let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX)));
            run(&ScanMeter::new(Some(budget.clone()))).expect("a budget with no bound");
            let bytes = budget.usage(HostWorkDimension::SearchStateBytes).get();
            let mut limits = HostWorkLimits::uniform(HostWorkLimit::new(u64::MAX));
            limits.set(
                HostWorkDimension::SearchStateBytes,
                HostWorkLimit::new(bytes - 1),
            );
            assert_eq!(
                run(&ScanMeter::new(Some(HostWorkBudget::new(limits)))),
                Err(SpaceError::Host(RSpaceError::HostWorkRejected)),
                "a cut rejects the {name} builder"
            );
        }
    }
}
