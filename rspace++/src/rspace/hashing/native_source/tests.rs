use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use proptest::prelude::*;

use super::*;
use crate::rspace::hashing::stable_hash_provider::hash_from_vec;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S1 (D-C2a, DR-96): `consume_keys` returns the source of
    /// `Consume::create` and one key per channel in channel order, repeats
    /// included.
    #[test]
    fn consume_keys_follow_channel_order(
        channels in prop::collection::vec("[a-c]{0,3}", 1..6),
        patterns in prop::collection::vec("[p-r]{0,3}", 1..6),
        persistent in any::<bool>(),
    ) {
        let (source, keys) =
            consume_keys(&channels, &patterns, &"body", persistent).expect("consume keys");
        prop_assert_eq!(&source, &Consume::create(&channels, &patterns, &"body", persistent));
        prop_assert_eq!(keys.len(), channels.len());
        for (key, channel) in keys.iter().zip(&channels) {
            prop_assert_eq!(*key, channel_key(channel).expect("channel key"));
        }
    }

    /// D-S1 (D-C2a, DR-96): a channel key is the channel's history digest.
    #[test]
    fn channel_key_equals_hash(channel in "[a-z]{0,40}") {
        let key = channel_key(&channel).expect("channel key");
        let digest = hash(&channel);
        prop_assert_eq!(key.0.as_slice(), digest.0.as_slice());
        prop_assert_eq!(key, StoreKey::from_digest(&digest).expect("a 32-byte digest"));
    }
}

/// D-S1 (D-C2a, DR-96): a group key hashes the domain separator and the
/// channel keys in channel order, so a reordered or repeated group has a
/// different key; join groups keep their positions.
#[test]
fn group_key_preserves_order() {
    let a = channel_key(&"a").expect("channel key");
    let b = channel_key(&"b").expect("channel key");
    let ab = group_key(&[a, b]);
    assert_ne!(ab, group_key(&[b, a]));
    assert_ne!(group_key(&[a]), group_key(&[a, a]));
    let mut reference = Blake2b::<U32>::new();
    reference.update(GROUP_KEY_DOMAIN);
    reference.update(a.0);
    reference.update(b.0);
    assert_eq!(ab.0.as_slice(), &reference.finalize()[..]);
    let built = GroupKeys::build(&["a", "b"]).expect("group keys");
    assert_eq!(built.channels, vec![a, b]);
    assert_eq!(built.group, ab);
    let operation =
        OperationKeys::build(&[vec!["b", "a"], vec!["a", "b"]]).expect("operation keys");
    assert_eq!(operation.groups.len(), 2);
    assert_eq!(operation.groups[0].channels, vec![b, a]);
    assert_eq!(operation.groups[1].group, ab);
    assert_ne!(operation.groups[0].group, operation.groups[1].group);
}

/// Changed by DR-131 S6: a digest that does not have 32 bytes is a broken
/// invariant of the store keys.
#[test]
fn a_short_digest_is_a_broken_invariant() {
    let short = Blake2b256Hash::from_bytes(vec![7; 31]);
    assert!(matches!(StoreKey::from_digest(&short), Err(RSpaceError::BugFoundError(_))));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// D-S1 (D-C2d, DR-96): the projection of a channel group from the store
    /// keys of its channels has the bytes of `hash_from_vec`, for any order,
    /// length and repetition of the channels. No channel is serialized.
    #[test]
    fn channels_hash_from_keys_matches_channels_hash(
        channels in prop::collection::vec("[a-c]{0,3}", 0..12),
    ) {
        let keys = GroupKeys::build(&channels).expect("group keys").channels;
        let projection = channels_hash_from_keys(&keys);
        let reference = hash_from_vec(&channels);
        prop_assert_eq!(projection.as_slice(), reference.0.as_slice());
        let mut reversed = keys.clone();
        reversed.reverse();
        prop_assert_eq!(channels_hash_from_keys(&reversed), projection);
    }
}
