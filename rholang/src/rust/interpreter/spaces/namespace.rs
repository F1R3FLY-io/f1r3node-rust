//! The Reified RSpaces namespace (plan decision 7, P4 design B.1).
//!
//! Every Reified name is a `GPrivate` whose id starts with the tag `0xE0`. A program cannot
//! forge a `GPrivate` id, so only the Reified machinery can make these names.
//!
//! | Id                 | Length       | Name                                               |
//! |--------------------|--------------|----------------------------------------------------|
//! | `E0‖op`            | 2            | a system channel, a syntax marker or the agent tag |
//! | `E0‖C‖sid`         | 34           | the config channel of the space `sid`              |
//! | `E0‖S‖sid`         | 34           | the space counter                                  |
//! | `E0‖S‖sid‖key`     | more than 34 | the counter of the collection `key`                |
//! | `E0‖D‖sid‖key`     | more than 34 | the data channel of the collection `key`           |
//! | `E0‖I‖sid‖prefix`  | 34 or more   | a path index (P5)                                  |
//! | `E0‖A‖sid`         | 34           | the allocator (P5)                                 |
//! | `E0‖H‖sid`         | 34           | the handle `this` of the space                     |
//! | `E0‖N‖sid‖tail`    | 66 or 42     | a name of the space                                |
//! | `E0‖X‖state`       | 34 or more   | a matcher (P8)                                     |
//! | `E0‖B‖tag`         | 34           | a body (P8)                                        |
//! | `E0‖M‖…`, `E0‖T‖…` | 34 or more   | reserved                                           |
//!
//! The ephemeral form of a KIND is its lowercase letter, `kind | 0x20` (I-4). `M` and `T` have
//! no ephemeral form.

use crypto::rust::hash::blake2b512_random::Blake2b512Random;
use models::rhoapi::g_unforgeable::UnfInstance;
use models::rhoapi::{GPrivate, GUnforgeable, Par};
pub use models::rust::reified::encoding::{Marker, REIFIED_TAG};
use models::rust::rholang::sorter::par_sort_matcher::ParSortMatcher;
use models::rust::rholang::sorter::sortable::Sortable;
use rspace_plus_plus::rspace::space_store_handle::SpaceTxnError;

use super::errors::SpaceError;

/// The length of a space id.
pub const SID_LEN: usize = 32;

/// The id of a native space (I-19).
pub type SpaceId = [u8; SID_LEN];

/// The length of a space-scoped id without a tail: the tag, the KIND byte and the space id.
pub const SPACE_ID_PREFIX_LEN: usize = 2 + SID_LEN;

/// The length of a name id with a random tail.
pub const NAME_ID_RANDOM_LEN: usize = SPACE_ID_PREFIX_LEN + 32;

/// The length of a name id with an index tail (Array and Vector, P5).
pub const NAME_ID_INDEX_LEN: usize = SPACE_ID_PREFIX_LEN + 8;

/// The bit that turns a KIND letter into its ephemeral form.
pub const EPHEMERAL_BIT: u8 = 0x20;

/// The last factory op. The factory channels are `[0xE0, 0x00]` to `[0xE0, 0x1F]`.
pub const FACTORY_LAST: u8 = 0x1F;

/// The op reserved for the factories of native engines (`rho:space:metta` first, held).
pub const NATIVE_ENGINE_FACTORY: u8 = 0x20;

/// The op of `rho:space:getSpaceAgent`.
pub const GET_SPACE_AGENT: u8 = 0x21;

/// The op of `rho:assert:assertEqual`.
pub const ASSERT_EQUAL: u8 = 0x22;

/// The op of `rho:lang:vector` (P6).
pub const VECTOR: u8 = 0x23;

/// The op of `rho:vectordb:embed`, a recorded process (P6, decision 57).
pub const EMBED: u8 = 0x24;

/// The op of `rho:vectordb:ann`, a recorded process (P6, decision 57).
pub const ANN: u8 = 0x25;

/// The op of the internal handle process.
pub const HANDLE_OP: u8 = 0x30;

/// The op of the internal matcher process (P8).
pub const MATCHER_OP: u8 = 0x31;

/// The op of the internal sink process (P8).
pub const SINK_OP: u8 = 0x32;

/// The second byte of the agent tag `[0xE0, 0xFF]`. It is never installed.
pub const AGENT_TAG: u8 = 0xFF;

/// The KIND byte of a space-scoped id. The discriminant is the uppercase letter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
pub enum Kind {
    Data = b'D',
    Config = b'C',
    PathIndex = b'I',
    Allocator = b'A',
    Sequence = b'S',
    Handle = b'H',
    Name = b'N',
    Matcher = b'X',
    Body = b'B',
    /// Reserved for engine-owned state.
    Engine = b'M',
    /// Reserved for theory and space-spec content.
    Theory = b'T',
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 11] = [
        Kind::Data,
        Kind::Config,
        Kind::PathIndex,
        Kind::Allocator,
        Kind::Sequence,
        Kind::Handle,
        Kind::Name,
        Kind::Matcher,
        Kind::Body,
        Kind::Engine,
        Kind::Theory,
    ];

    /// True when the kind has an ephemeral form. `M` and `T` have none.
    pub const fn has_ephemeral_form(self) -> bool { !matches!(self, Kind::Engine | Kind::Theory) }

    /// The KIND byte of the kind in `durability`, or `None` for an ephemeral form that does
    /// not exist.
    pub const fn byte(self, durability: Durability) -> Option<u8> {
        match durability {
            Durability::Durable => Some(self as u8),
            Durability::Ephemeral if self.has_ephemeral_form() => Some(self as u8 | EPHEMERAL_BIT),
            Durability::Ephemeral => None,
        }
    }

    /// The kind and the durability of a KIND byte.
    pub const fn from_byte(byte: u8) -> Option<(Kind, Durability)> {
        let kind = match byte & !EPHEMERAL_BIT {
            b'D' => Kind::Data,
            b'C' => Kind::Config,
            b'I' => Kind::PathIndex,
            b'A' => Kind::Allocator,
            b'S' => Kind::Sequence,
            b'H' => Kind::Handle,
            b'N' => Kind::Name,
            b'X' => Kind::Matcher,
            b'B' => Kind::Body,
            b'M' => Kind::Engine,
            b'T' => Kind::Theory,
            _ => return None,
        };
        match ((byte & EPHEMERAL_BIT) == 0, kind.has_ephemeral_form()) {
            (true, _) => Some((kind, Durability::Durable)),
            (false, true) => Some((kind, Durability::Ephemeral)),
            (false, false) => None,
        }
    }
}

/// Durable state is consensus state. Ephemeral state lives for one deploy (temp and seq
/// spaces, P7).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Durability {
    Durable,
    Ephemeral,
}

/// A native space: its id and the durability of its channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NativeSpace {
    pub sid: SpaceId,
    pub durability: Durability,
}

/// The tail of a name id: 32 random bytes, or a big-endian index for Array and Vector (P5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameTail {
    Random([u8; 32]),
    Index(u64),
}

/// The key of a collection: the canonical bytes of its inner channel (A5). A key is never
/// empty, so a collection id is longer than a space-level id.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CollectionKey(Vec<u8>);

impl CollectionKey {
    /// The key of the collection that `channel` names.
    pub fn of_channel(channel: &Par) -> Result<Self, SpaceError> {
        let bytes = canonical_inner_channel_bytes(channel)?;
        Self::from_bytes(bytes).ok_or(SpaceError::Inconsistent("a channel has no canonical bytes"))
    }

    /// A key from its bytes, or `None` for empty bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        match bytes.is_empty() {
            true => None,
            false => Some(Self(bytes)),
        }
    }

    /// The bytes of the key.
    pub fn as_bytes(&self) -> &[u8] { &self.0 }
}

/// `bincode(ParSortMatcher::sort_match(channel).term)`, the preimage of the store identity of
/// the channel (A5). Serde writes `locally_free` as empty bytes, so channels that are equal as
/// `Par` values have one key.
pub fn canonical_inner_channel_bytes(channel: &Par) -> Result<Vec<u8>, SpaceError> {
    bincode::serialize(&ParSortMatcher::sort_match(channel).term).map_err(|_| {
        SpaceError::Substrate(SpaceTxnError::InvalidOperation(
            "a channel is not serializable",
        ))
    })
}

/// A `Par` that holds exactly the name `GPrivate(id)`, built as `system_processes::byte_name`
/// builds it.
pub fn gprivate(id: Vec<u8>) -> Par {
    Par::default().with_unforgeables(vec![GUnforgeable {
        unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
    }])
}

fn kind_byte(kind: Kind, durability: Durability) -> u8 {
    match durability {
        Durability::Durable => kind as u8,
        Durability::Ephemeral => kind as u8 | EPHEMERAL_BIT,
    }
}

fn space_scoped_id(kind: Kind, space: NativeSpace, tail: &[u8]) -> Vec<u8> {
    let mut id = Vec::with_capacity(SPACE_ID_PREFIX_LEN + tail.len());
    id.push(REIFIED_TAG);
    id.push(kind_byte(kind, space.durability));
    id.extend_from_slice(&space.sid);
    id.extend_from_slice(tail);
    id
}

/// `E0‖C‖sid`: the config channel.
pub fn config_channel(space: NativeSpace) -> Vec<u8> { space_scoped_id(Kind::Config, space, &[]) }

/// `E0‖S‖sid`: the space counter.
pub fn space_counter(space: NativeSpace) -> Vec<u8> { space_scoped_id(Kind::Sequence, space, &[]) }

/// `E0‖S‖sid‖key`: the counter of the collection `key`.
pub fn collection_counter(space: NativeSpace, key: &CollectionKey) -> Vec<u8> {
    space_scoped_id(Kind::Sequence, space, key.as_bytes())
}

/// `E0‖D‖sid‖key`: the data channel of the collection `key`.
pub fn data_channel(space: NativeSpace, key: &CollectionKey) -> Vec<u8> {
    space_scoped_id(Kind::Data, space, key.as_bytes())
}

/// `E0‖H‖sid`: the handle `this` of the space.
pub fn handle_channel(space: NativeSpace) -> Vec<u8> { space_scoped_id(Kind::Handle, space, &[]) }

/// `E0‖A‖sid`: the allocator of the index names of an Array or Vector space (P5d).
pub fn allocator_channel(space: NativeSpace) -> Vec<u8> {
    space_scoped_id(Kind::Allocator, space, &[])
}

/// `E0‖D‖sid‖path_key`: the data channel of the path with the key `path_key` in a PathMap space
/// (P5e). A path key ends with its terminator, so it is never empty.
pub fn path_data_channel(space: NativeSpace, path_key: &[u8]) -> Vec<u8> {
    space_scoped_id(Kind::Data, space, path_key)
}

/// `E0‖I‖sid‖prefix_key`: the index channel of the prefix with the key `prefix_key` in a PathMap
/// space (P5e). It holds the markers of the stored data under the prefix and the waiters on the
/// prefix. The key of the empty prefix is empty.
pub fn path_index_channel(space: NativeSpace, prefix_key: &[u8]) -> Vec<u8> {
    space_scoped_id(Kind::PathIndex, space, prefix_key)
}

/// `E0‖N‖sid‖tail`: a name of the space.
pub fn name_id(space: NativeSpace, tail: NameTail) -> Vec<u8> {
    match tail {
        NameTail::Random(bytes) => space_scoped_id(Kind::Name, space, &bytes),
        NameTail::Index(index) => space_scoped_id(Kind::Name, space, &index.to_be_bytes()),
    }
}

/// `E0‖op`: a 2-byte op id.
pub fn op_channel(op: u8) -> Vec<u8> { vec![REIFIED_TAG, op] }

/// `E0‖X‖state`: the matcher capability of one bind of a translated receive (P8, design v2
/// TR-5). The translation mints only the uppercase (durable) form, and `state` is the canonical
/// encoding of the matcher state (`reified::user_agent::MatcherState`), which has more than
/// [`SID_LEN`] bytes.
pub fn matcher_channel(state: &[u8]) -> Vec<u8> {
    let mut id = Vec::with_capacity(2 + state.len());
    id.push(REIFIED_TAG);
    id.push(Kind::Matcher as u8);
    id.extend_from_slice(state);
    id
}

/// `E0‖B‖tag`: the body capability of a translated receive (P8, design v2 TR-6). The
/// translation mints only the uppercase (durable) form.
pub fn body_channel(tag: &[u8; SID_LEN]) -> Vec<u8> {
    let mut id = Vec::with_capacity(SPACE_ID_PREFIX_LEN);
    id.push(REIFIED_TAG);
    id.push(Kind::Body as u8);
    id.extend_from_slice(tag);
    id
}

/// A decoded Reified id. Each slice borrows from the decoded id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReifiedId<'a> {
    /// `E0‖op`: a system channel, a syntax marker or the agent tag.
    Op(u8),
    Config(NativeSpace),
    SpaceCounter(NativeSpace),
    CollectionCounter(NativeSpace, &'a [u8]),
    Data(NativeSpace, &'a [u8]),
    PathIndex(NativeSpace, &'a [u8]),
    Allocator(NativeSpace),
    Handle(NativeSpace),
    Name(NativeSpace, NameTail),
    Matcher(Durability, &'a [u8]),
    Body(Durability, &'a [u8]),
    /// An id of the reserved kind `M` or `T`, with its payload after the KIND byte.
    Reserved(Kind, &'a [u8]),
}

fn name_tail(tail: &[u8]) -> Option<NameTail> {
    match tail.len() {
        32 => <[u8; 32]>::try_from(tail).ok().map(NameTail::Random),
        8 => <[u8; 8]>::try_from(tail)
            .ok()
            .map(|index| NameTail::Index(u64::from_be_bytes(index))),
        _ => None,
    }
}

/// Decodes a `GPrivate` id. The function is total: it returns `None` for every id that no
/// constructor of this module can make. A 2-byte id with the tag is an `Op`. Every other id
/// has at least 34 bytes and a KIND byte of the table, and its length must match its kind.
pub fn decode_reified_id(id: &[u8]) -> Option<ReifiedId<'_>> {
    let (&tag, rest) = id.split_first()?;
    if tag != REIFIED_TAG {
        return None;
    }
    let (&kind, payload) = rest.split_first()?;
    if payload.is_empty() {
        return Some(ReifiedId::Op(kind));
    }
    let (kind, durability) = Kind::from_byte(kind)?;
    let (sid, tail) = payload.split_at_checked(SID_LEN)?;
    let space = NativeSpace {
        sid: <SpaceId>::try_from(sid).ok()?,
        durability,
    };
    match (kind, tail.is_empty()) {
        (Kind::Config, true) => Some(ReifiedId::Config(space)),
        (Kind::Handle, true) => Some(ReifiedId::Handle(space)),
        (Kind::Allocator, true) => Some(ReifiedId::Allocator(space)),
        (Kind::Body, true) => Some(ReifiedId::Body(durability, sid)),
        (Kind::Sequence, true) => Some(ReifiedId::SpaceCounter(space)),
        (Kind::Sequence, false) => Some(ReifiedId::CollectionCounter(space, tail)),
        (Kind::Data, false) => Some(ReifiedId::Data(space, tail)),
        (Kind::PathIndex, _) => Some(ReifiedId::PathIndex(space, tail)),
        (Kind::Name, _) => name_tail(tail).map(|tail| ReifiedId::Name(space, tail)),
        (Kind::Matcher, _) => Some(ReifiedId::Matcher(durability, payload)),
        (Kind::Engine | Kind::Theory, _) => Some(ReifiedId::Reserved(kind, payload)),
        (Kind::Config | Kind::Handle | Kind::Allocator | Kind::Body, false)
        | (Kind::Data, true) => None,
    }
}

/// The unforgeable of a `Par` that holds exactly one unforgeable and nothing else. The bit
/// sets do not count.
pub fn sole_unforgeable(par: &Par) -> Option<&GUnforgeable> {
    let Par {
        sends,
        receives,
        news,
        exprs,
        matches,
        unforgeables,
        bundles,
        connectives,
        conditionals,
        locally_free: _,
        connective_used: _,
        cost_signed_terms,
        cost_stacks,
        use_blocks,
    } = par;
    let structural = sends.is_empty()
        && receives.is_empty()
        && news.is_empty()
        && exprs.is_empty()
        && matches.is_empty()
        && bundles.is_empty()
        && connectives.is_empty()
        && conditionals.is_empty()
        && cost_signed_terms.is_empty()
        && use_blocks.is_empty()
        && cost_stacks.is_empty();
    match (structural, unforgeables.as_slice()) {
        (true, [unforgeable]) => Some(unforgeable),
        (true, _) | (false, _) => None,
    }
}

/// The id of a `Par` that is exactly one `GPrivate`.
pub fn sole_gprivate_id(par: &Par) -> Option<&[u8]> {
    match sole_unforgeable(par)? {
        GUnforgeable {
            unf_instance: Some(UnfInstance::GPrivateBody(GPrivate { id })),
        } => Some(id),
        GUnforgeable { .. } => None,
    }
}

/// The decoded id of a `Par` that is exactly one `GPrivate` with a Reified id.
pub fn reified_id_of(par: &Par) -> Option<ReifiedId<'_>> {
    sole_gprivate_id(par).and_then(decode_reified_id)
}

/// The purpose bytes of the random states that the Reified machinery derives (P4 design
/// C.11). Every purpose is a split under `split_byte(0xE0)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RandomPurpose {
    /// The space id of a factory call.
    SpaceId = 0x01,
    /// The tail of a name that `gensym` makes.
    Gensym = 0x02,
    /// The translation of a user-agent space (P8).
    Xlate = 0x03,
    /// A path marker (P5).
    PathMarker = 0x04,
    /// The config datum of a space.
    Config = 0x05,
    /// The handle continuation of a space.
    Handle = 0x06,
    /// The space counter: `.split_byte(0)` for its continuation, `.split_byte(1)` for its datum.
    SpaceCounter = 0x07,
    /// A collection counter: `.split_byte(0)` for its continuation, `.split_byte(1)` for its
    /// datum.
    Counter = 0x08,
    /// The consume that retires a stored member of a set class (P5a, F3).
    Retire = 0x09,
    /// The consume that lifts a stored record out of a collection during a settle (P5b, F2).
    Lift = 0x0A,
    /// The allocator of an Array or Vector space: `.split_byte(0)` for its continuation,
    /// `.split_byte(1)` for its datum (P5d).
    Allocator = 0x0B,
    /// Added by decision 92: the anchor of a space transaction, `.split_byte(0)` for its
    /// continuation (`spaces::anchor`).
    Anchor = 0x0C,
}

impl RandomPurpose {
    /// `random.split_byte(0xE0).split_byte(purpose)`.
    pub fn derive(self, random: &Blake2b512Random) -> Blake2b512Random {
        random.split_byte(REIFIED_TAG).split_byte(self as u8)
    }
}

#[cfg(test)]
mod tests {
    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{EList, Expr};
    use models::rust::utils::{new_gint_par, new_gstring_par};

    use super::*;

    fn space(seed: u8, durability: Durability) -> NativeSpace {
        NativeSpace {
            sid: [seed; SID_LEN],
            durability,
        }
    }

    fn key(text: &str) -> CollectionKey {
        CollectionKey::of_channel(&new_gstring_par(text.to_string(), Vec::new(), false))
            .expect("a string channel has canonical bytes")
    }

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
    fn kind_bytes_have_one_durable_and_at_most_one_ephemeral_form() {
        for kind in Kind::ALL {
            let durable = kind
                .byte(Durability::Durable)
                .expect("every kind is durable");
            assert!(durable.is_ascii_uppercase());
            assert_eq!(Kind::from_byte(durable), Some((kind, Durability::Durable)));
            match kind.byte(Durability::Ephemeral) {
                Some(ephemeral) => {
                    assert_eq!(ephemeral, durable | EPHEMERAL_BIT);
                    assert_eq!(
                        Kind::from_byte(ephemeral),
                        Some((kind, Durability::Ephemeral))
                    );
                }
                None => {
                    assert!(matches!(kind, Kind::Engine | Kind::Theory));
                    assert_eq!(Kind::from_byte(durable | EPHEMERAL_BIT), None);
                }
            }
        }
        let letters: std::collections::BTreeSet<u8> =
            Kind::ALL.iter().map(|kind| *kind as u8).collect();
        assert_eq!(letters.len(), Kind::ALL.len());
        assert_eq!(Kind::from_byte(b'Z'), None);
        assert_eq!(Kind::from_byte(0x00), None);
    }

    #[test]
    fn constructors_have_the_documented_layouts() {
        let durable = space(7, Durability::Durable);
        let collection = key("queue");
        assert_eq!(config_channel(durable).len(), SPACE_ID_PREFIX_LEN);
        assert_eq!(space_counter(durable).len(), SPACE_ID_PREFIX_LEN);
        assert_eq!(handle_channel(durable).len(), SPACE_ID_PREFIX_LEN);
        assert!(collection_counter(durable, &collection).len() > SPACE_ID_PREFIX_LEN);
        assert!(data_channel(durable, &collection).len() > SPACE_ID_PREFIX_LEN);
        assert_eq!(
            name_id(durable, NameTail::Random([1; 32])).len(),
            NAME_ID_RANDOM_LEN
        );
        assert_eq!(
            name_id(durable, NameTail::Index(3)).len(),
            NAME_ID_INDEX_LEN
        );
        assert_eq!(op_channel(HANDLE_OP), vec![REIFIED_TAG, HANDLE_OP]);
        assert_eq!(&config_channel(durable)[..2], &[REIFIED_TAG, b'C']);
        assert_eq!(&handle_channel(space(7, Durability::Ephemeral))[..2], &[
            REIFIED_TAG,
            b'h'
        ]);
        assert_eq!(
            &data_channel(durable, &collection)[2..SPACE_ID_PREFIX_LEN],
            &[7; SID_LEN]
        );
        assert_eq!(gprivate(op_channel(GET_SPACE_AGENT)), {
            let mut expected = Par::default();
            expected.unforgeables.push(GUnforgeable {
                unf_instance: Some(UnfInstance::GPrivateBody(GPrivate {
                    id: vec![REIFIED_TAG, GET_SPACE_AGENT],
                })),
            });
            expected
        });
    }

    #[test]
    fn decode_inverts_every_constructor() {
        for durability in [Durability::Durable, Durability::Ephemeral] {
            let space = space(9, durability);
            let collection = key("collection");
            assert_eq!(
                decode_reified_id(&config_channel(space)),
                Some(ReifiedId::Config(space))
            );
            assert_eq!(
                decode_reified_id(&space_counter(space)),
                Some(ReifiedId::SpaceCounter(space))
            );
            assert_eq!(
                decode_reified_id(&collection_counter(space, &collection)),
                Some(ReifiedId::CollectionCounter(space, collection.as_bytes()))
            );
            assert_eq!(
                decode_reified_id(&data_channel(space, &collection)),
                Some(ReifiedId::Data(space, collection.as_bytes()))
            );
            assert_eq!(
                decode_reified_id(&handle_channel(space)),
                Some(ReifiedId::Handle(space))
            );
            for tail in [NameTail::Random([5; 32]), NameTail::Index(u64::MAX)] {
                assert_eq!(
                    decode_reified_id(&name_id(space, tail)),
                    Some(ReifiedId::Name(space, tail))
                );
            }
            assert_eq!(
                decode_reified_id(&allocator_channel(space)),
                Some(ReifiedId::Allocator(space))
            );
            let path_key = [0x01, 0x07, 0x00];
            assert_eq!(
                decode_reified_id(&path_data_channel(space, &path_key)),
                Some(ReifiedId::Data(space, &path_key[..]))
            );
            for prefix_key in [&[][..], &[0x01, 0x07][..]] {
                assert_eq!(
                    decode_reified_id(&path_index_channel(space, prefix_key)),
                    Some(ReifiedId::PathIndex(space, prefix_key))
                );
            }
        }
        for op in 0..=u8::MAX {
            assert_eq!(decode_reified_id(&op_channel(op)), Some(ReifiedId::Op(op)));
        }
        let marker = gprivate(Marker::Use.id().to_vec());
        assert_eq!(
            reified_id_of(&marker),
            Some(ReifiedId::Op(Marker::Use as u8))
        );
    }

    #[test]
    fn decode_rejects_foreign_and_malformed_ids() {
        let durable = space(3, Durability::Durable);
        let mut wrong_tag = config_channel(durable);
        wrong_tag[0] = 0xF0;
        let mut short = config_channel(durable);
        short.truncate(SPACE_ID_PREFIX_LEN - 1);
        let mut long_config = config_channel(durable);
        long_config.push(0);
        let mut unknown_kind = config_channel(durable);
        unknown_kind[1] = b'Z';
        let mut lowercase_reserved = config_channel(durable);
        lowercase_reserved[1] = b'm';
        let mut short_name = name_id(durable, NameTail::Index(1));
        short_name.pop();
        for id in [
            Vec::new(),
            vec![REIFIED_TAG],
            vec![0x25],
            vec![0x25, 0x01],
            vec![REIFIED_TAG; 3],
            vec![REIFIED_TAG; SID_LEN],
            wrong_tag,
            short,
            long_config,
            unknown_kind,
            lowercase_reserved,
            short_name,
            data_channel(durable, &key("k"))[..SPACE_ID_PREFIX_LEN].to_vec(),
        ] {
            assert_eq!(decode_reified_id(&id), None, "{id:?}");
        }
        let ordinary = gprivate(vec![REIFIED_TAG; SID_LEN]);
        assert_eq!(reified_id_of(&ordinary), None);
        let byte_name = gprivate(vec![REIFIED_TAG]);
        assert_eq!(reified_id_of(&byte_name), None);
        let mut with_expr = gprivate(config_channel(durable));
        with_expr.exprs.push(Expr {
            expr_instance: Some(ExprInstance::GInt(1)),
        });
        assert_eq!(reified_id_of(&with_expr), None);
    }

    #[test]
    fn canonical_bytes_identify_par_equal_channels() {
        let one = new_gint_par(1, Vec::new(), false);
        let two = new_gint_par(2, Vec::new(), false);
        let mut forward = one.clone();
        forward.exprs.extend(two.exprs.clone());
        let mut backward = two.clone();
        backward.exprs.extend(one.exprs.clone());
        assert_eq!(
            canonical_inner_channel_bytes(&forward).expect("canonical bytes"),
            canonical_inner_channel_bytes(&backward).expect("canonical bytes")
        );
        let mut with_bits = forward.clone();
        with_bits.locally_free = vec![0b101];
        assert_eq!(
            CollectionKey::of_channel(&with_bits).expect("key"),
            CollectionKey::of_channel(&forward).expect("key")
        );
        assert_ne!(
            CollectionKey::of_channel(&list(vec![one.clone(), two.clone()])).expect("key"),
            CollectionKey::of_channel(&list(vec![two, one])).expect("key")
        );
        assert_eq!(CollectionKey::from_bytes(Vec::new()), None);
    }

    #[test]
    fn space_and_collection_counters_and_handles_are_distinct() {
        let durable = space(4, Durability::Durable);
        let ephemeral = space(4, Durability::Ephemeral);
        let collection = key("c");
        let ids = [
            config_channel(durable),
            space_counter(durable),
            collection_counter(durable, &collection),
            data_channel(durable, &collection),
            handle_channel(durable),
            name_id(durable, NameTail::Random([0; 32])),
            config_channel(ephemeral),
            space_counter(ephemeral),
            collection_counter(ephemeral, &collection),
            data_channel(ephemeral, &collection),
            handle_channel(ephemeral),
            name_id(ephemeral, NameTail::Random([0; 32])),
        ];
        let distinct: std::collections::BTreeSet<&Vec<u8>> = ids.iter().collect();
        assert_eq!(distinct.len(), ids.len());
        assert_ne!(
            handle_channel(durable),
            handle_channel(space(5, Durability::Durable))
        );
    }

    #[test]
    fn random_purposes_split_under_the_tag() {
        let random = Blake2b512Random::create_from_bytes(b"reified namespace");
        let purposes = [
            RandomPurpose::SpaceId,
            RandomPurpose::Gensym,
            RandomPurpose::Xlate,
            RandomPurpose::PathMarker,
            RandomPurpose::Config,
            RandomPurpose::Handle,
            RandomPurpose::SpaceCounter,
            RandomPurpose::Counter,
            RandomPurpose::Retire,
            RandomPurpose::Lift,
            RandomPurpose::Allocator,
            RandomPurpose::Anchor,
        ];
        let mut outputs = std::collections::BTreeSet::new();
        for (index, purpose) in purposes.into_iter().enumerate() {
            assert_eq!(purpose as u8 as usize, index + 1);
            let mut derived = purpose.derive(&random);
            let mut expected = random.split_byte(REIFIED_TAG).split_byte(purpose as u8);
            let output = derived.next();
            assert_eq!(output, expected.next());
            outputs.insert(output);
        }
        assert_eq!(outputs.len(), purposes.len());
    }
}
