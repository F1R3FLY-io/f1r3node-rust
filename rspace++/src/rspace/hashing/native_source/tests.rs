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

#[derive(serde::Serialize)]
struct CostWire(u64);

impl StableHashSerialize for CostWire {
    fn stable_hash_contains_cost(&self) -> Result<bool> { Ok(true) }
}

#[test]
fn epoch_native_sources_match_value_constructors() {
    let channels = vec!["ordinary-channel".to_owned(), "second-channel".to_owned()];
    let patterns = vec!["one".to_owned(), "two".to_owned()];
    let data = "ordinary-data".to_owned();
    for epoch in [EventCodecEpoch::Legacy, EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
        let (source, keys) = consume_keys_for_epoch(epoch, &channels, &patterns, &data, false)
            .expect("epoch source");
        assert_eq!(
            source,
            Consume::create_for_epoch(epoch, &channels, &patterns, &data, false)
                .expect("value source")
        );
        assert_eq!(keys[0], channel_key(&channels[0]).expect("ordinary key"));
        if epoch == EventCodecEpoch::Legacy {
            assert_eq!(source, Consume::create(&channels, &patterns, &data, false));
        }
    }
}

#[test]
fn native_store_keys_follow_authenticated_epoch() {
    let ordinary = "registry".to_owned();
    let cost = CostWire(13);
    let legacy = channel_key_for_epoch(EventCodecEpoch::Legacy, &ordinary).expect("legacy key");
    for epoch in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
        let active = channel_key_for_epoch(epoch, &ordinary).expect("ordinary active key");
        assert_eq!(legacy, active);
        assert_eq!(legacy, channel_key(&ordinary).expect("unversioned ordinary key"));
        let cost_key = channel_key_for_epoch(epoch, &cost).expect("cost key");
        assert_eq!(
            cost_key.0.as_slice(),
            channel_hash_for_epoch(epoch, &cost)
                .expect("cost hash")
                .0
                .as_slice()
        );
        assert_ne!(cost_key, legacy);
        let group =
            GroupKeys::build_for_epoch(epoch, &[CostWire(13), CostWire(14)]).expect("epoch group");
        assert_eq!(group.channels[0], cost_key);
        assert_eq!(group.group, group_key(&group.channels));
    }
    assert!(channel_key_for_epoch(EventCodecEpoch::Legacy, &cost).is_err());
}

#[test]
fn epoch_consume_keys_reuse_source_channel_digests() {
    let channels = [CostWire(13), CostWire(14), CostWire(13)];
    let patterns = ["pattern"];
    let continuation = "body";
    for epoch in [EventCodecEpoch::CostV1, EventCodecEpoch::CostV2] {
        let source = Consume::create_for_epoch(epoch, &channels, &patterns, &continuation, false)
            .expect("epoch source");
        let (keyed_source, keys) =
            consume_keys_for_epoch(epoch, &channels, &patterns, &continuation, false)
                .expect("keyed source");
        assert_eq!(keyed_source, source);
        assert_eq!(keys[0], keys[2]);
        assert_ne!(keys[0], keys[1]);
        for (channel, key) in channels.iter().zip(&keys) {
            assert_eq!(*key, channel_key_for_epoch(epoch, channel).expect("epoch key"));
        }
    }
}

#[test]
fn cost_v2_preserves_preexisting_event_constructors() {
    use crate::rspace::hashing::stable_hash_provider::event_leg_bytes_for_epoch;
    use crate::rspace::trace::event::{Produce, recorded_removal_for_epoch};
    let channels = vec![CostWire(13), CostWire(14), CostWire(13)];
    let patterns = vec!["first", "second", "third"];
    let datum = CostWire(21);
    for persistent in [false, true] {
        let v1_produce =
            Produce::create_for_epoch(EventCodecEpoch::CostV1, &channels[0], &datum, persistent)
                .expect("v1 produce");
        let v2_produce =
            Produce::create_for_epoch(EventCodecEpoch::CostV2, &channels[0], &datum, persistent)
                .expect("v2 produce");
        assert_eq!(v1_produce, v2_produce);
        assert_eq!(
            bincode::serialize(&v1_produce).expect("v1 bytes"),
            bincode::serialize(&v2_produce).expect("v2 bytes")
        );
        let v1_consume = Consume::create_for_epoch(
            EventCodecEpoch::CostV1,
            &channels,
            &patterns,
            &datum,
            persistent,
        )
        .expect("v1 consume");
        let v2_consume = Consume::create_for_epoch(
            EventCodecEpoch::CostV2,
            &channels,
            &patterns,
            &datum,
            persistent,
        )
        .expect("v2 consume");
        assert_eq!(v1_consume, v2_consume);
        assert_eq!(
            bincode::serialize(&v1_consume).expect("v1 bytes"),
            bincode::serialize(&v2_consume).expect("v2 bytes")
        );
    }
    assert_eq!(
        event_leg_bytes_for_epoch(EventCodecEpoch::CostV1, &datum).expect("v1 leg"),
        event_leg_bytes_for_epoch(EventCodecEpoch::CostV2, &datum).expect("v2 leg")
    );
    assert_eq!(
        channel_hash_for_epoch(EventCodecEpoch::CostV1, &channels[0]).expect("v1 key"),
        channel_hash_for_epoch(EventCodecEpoch::CostV2, &channels[0]).expect("v2 key")
    );
    assert_eq!(
        recorded_removal_for_epoch(
            EventCodecEpoch::CostV1,
            &channels[0],
            &Produce::create_for_epoch(EventCodecEpoch::CostV1, &channels[0], &datum, false)
                .expect("v1 produce"),
            b"removal"
        )
        .expect("v1 removal"),
        recorded_removal_for_epoch(
            EventCodecEpoch::CostV2,
            &channels[0],
            &Produce::create_for_epoch(EventCodecEpoch::CostV2, &channels[0], &datum, false)
                .expect("v2 produce"),
            b"removal"
        )
        .expect("v2 removal")
    );
}
