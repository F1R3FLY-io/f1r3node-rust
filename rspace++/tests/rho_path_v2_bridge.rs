use std::ops::RangeInclusive;

use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use rspace_plus_plus::rspace::channel_image_bridge::{
    Blake2bLegacyDigest, ChannelImageBridge, ChannelImageBridgeError, ChannelRepresentatives,
    LegacyChannelDigest, NoRepresentatives,
};
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider::{StableHashSerialize, hash};
use rspace_plus_plus::rspace::history::cold_store::PersistedDataKind;
use rspace_plus_plus::rspace::history::history_reader::HistoryReaderBase;
use rspace_plus_plus::rspace::hot_store::{HotStore, HotStoreInstances, HotStoreState};
use rspace_plus_plus::rspace::hot_store_action::{HotStoreAction, InsertAction};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::rho_path::{
    ReceiveChannelProfile, ReceiveChannelQuery, RhoPathChannelImage, RhoPathEncode, RhoPathGrammar,
    RhoPathKeySyntax, RhoPathV1,
};
use rspace_plus_plus::rspace::serializers::cold_store_decode::{
    ColdStoreDecode, ColdStoreDecodeError, legacy_prefix,
};
use serde::{Deserialize, Serialize};

const GENERATED_CASES: u32 = 256;
const FORCED_DIGEST: [u8; 32] = [0xC5; 32];
const NAMES: &[&str] = &["", "a", "b", "ab", "\u{0}"];

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
struct ProbeChannel {
    name: String,
    legacy_tag: u8,
    #[serde(skip)]
    annotation: u8,
}

impl StableHashSerialize for ProbeChannel {}

impl RhoPathEncode for ProbeChannel {}

impl RhoPathChannelImage for ProbeChannel {
    fn rho_path_channel_image(&self) -> Vec<u8> { self.name.as_bytes().to_vec() }

    fn from_rho_path_channel_image(image: &[u8]) -> Result<Self, String> {
        match std::str::from_utf8(image) {
            Ok(name) => Ok(probe(name, 0, 0)),
            Err(error) => Err(format!("a probe channel image is not valid UTF-8: {error}")),
        }
    }
}

impl ColdStoreDecode for ProbeChannel {
    fn cold_decode_prefix(bytes: &[u8]) -> Result<(Self, usize), ColdStoreDecodeError> {
        legacy_prefix(bytes)
    }
}

enum ForcedCollisionDigest {}

impl LegacyChannelDigest for ForcedCollisionDigest {
    fn legacy_digest(_identity: &[u8]) -> [u8; 32] { FORCED_DIGEST }
}

struct EmptyHistory;

impl HistoryReaderBase<ProbeChannel, String, String, String> for EmptyHistory {
    fn get_data_canonical(&self, _key: &ProbeChannel) -> Vec<Datum<String>> { Vec::new() }

    fn get_continuations_canonical(
        &self,
        _key: &[ProbeChannel],
    ) -> Vec<WaitingContinuation<String, String>> {
        Vec::new()
    }

    fn get_joins_canonical(&self, _key: &ProbeChannel) -> Vec<Vec<ProbeChannel>> { Vec::new() }
}

fn probe(name: &str, legacy_tag: u8, annotation: u8) -> ProbeChannel {
    ProbeChannel {
        name: name.to_owned(),
        legacy_tag,
        annotation,
    }
}

fn probe_channel() -> impl Strategy<Value = ProbeChannel> {
    (proptest::sample::select(NAMES), 0_u8..3, 0_u8..3)
        .prop_map(|(name, legacy_tag, annotation)| probe(name, legacy_tag, annotation))
}

fn channel_corpus(sizes: RangeInclusive<usize>) -> impl Strategy<Value = Vec<ProbeChannel>> {
    proptest::collection::vec(probe_channel(), sizes)
}

fn run_generated<S: Strategy>(strategy: S, check: impl Fn(S::Value)) {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: GENERATED_CASES,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&strategy, |value| {
            check(value);
            Ok(())
        })
        .expect("every generated case satisfies the property");
}

fn collision_witness() -> Vec<ProbeChannel> {
    vec![
        probe("img-B-V1", 0, 0),
        probe("img-B-V2", 0, 0),
        probe("img-B-V1", 1, 0),
        probe("img-B-V1", 0, 1),
        probe("img-T-V1", 0, 0),
    ]
}

fn alias_witness() -> Vec<ProbeChannel> {
    vec![probe("a", 0, 0), probe("b", 0, 0), probe("a", 1, 0), probe("a", 0, 1)]
}

fn admit_all<D: LegacyChannelDigest>(
    corpus: &[ProbeChannel],
) -> ChannelImageBridge<ProbeChannel, D> {
    let mut bridge = ChannelImageBridge::new();
    for channel in corpus {
        bridge
            .admit(channel)
            .expect("a bridge without an image limit admits every channel");
    }
    bridge
}

fn first_representatives(corpus: &[ProbeChannel]) -> Vec<(Vec<u8>, ProbeChannel)> {
    let mut first: Vec<(Vec<u8>, ProbeChannel)> = Vec::with_capacity(corpus.len());
    for channel in corpus {
        let image = channel.rho_path_channel_image();
        if first.iter().all(|(known, _)| *known != image) {
            first.push((image, channel.clone()));
        }
    }
    first
}

fn history_inputs<R: ChannelRepresentatives<ProbeChannel>>(
    representatives: &R,
    group: &[ProbeChannel],
) -> Vec<ProbeChannel> {
    let mut inputs = Vec::with_capacity(group.len());
    for channel in group {
        let input = match representatives.representative(&channel.rho_path_channel_image()) {
            Some(representative) => representative.clone(),
            None => channel.clone(),
        };
        inputs.push(input);
    }
    inputs
}

fn continuation_prefix(group: &[ProbeChannel]) -> Vec<u8> {
    <RhoPathV1 as RhoPathGrammar<ProbeChannel>>::continuation_prefix(group)
}

fn legacy_group_identity(group: &[ProbeChannel]) -> Vec<u8> {
    let mut digests = group
        .iter()
        .map(|channel| Blake2bLegacyDigest::legacy_digest(&channel.rho_path_identity()))
        .collect::<Vec<_>>();
    digests.sort_unstable();
    let mut concatenated = Vec::with_capacity(digests.len() * 32);
    for digest in &digests {
        concatenated.extend_from_slice(digest);
    }
    Blake2b256Hash::new(&concatenated).0
}

fn checkpoint_group_identity(group: &[ProbeChannel]) -> Vec<u8> {
    let hot_store: Box<dyn HotStore<ProbeChannel, String, String, String>> =
        HotStoreInstances::create_from_hs_and_hr(HotStoreState::default(), Box::new(EmptyHistory));
    assert_eq!(
        hot_store.put_continuation(group, WaitingContinuation::default()),
        Some(true),
        "the hot store holds one waiting continuation on the group"
    );
    let actions = hot_store
        .checkpoint_changes()
        .expect("the hot store checkpoints its changes");
    let mut identities = actions.iter().filter_map(|action| match action {
        HotStoreAction::Insert(InsertAction::InsertCanonicalCollection(insert))
            if insert.persisted_kind() == PersistedDataKind::Continuations =>
        {
            Some(insert.hash().0.clone())
        }
        _ => None,
    });
    let identity = identities
        .next()
        .expect("the checkpoint inserts the continuation group");
    assert!(identities.next().is_none(), "the checkpoint inserts one continuation group");
    identity
}

fn check_forced_collisions(corpus: &[ProbeChannel]) {
    let first = first_representatives(corpus);
    let forced = admit_all::<ForcedCollisionDigest>(corpus);
    let expected_images = first
        .iter()
        .map(|(image, _)| image.as_slice())
        .collect::<Vec<_>>();
    assert_eq!(
        forced
            .images_with_legacy_digest(&FORCED_DIGEST)
            .collect::<Vec<_>>(),
        expected_images,
        "the forced digest bucket keeps every distinct full image once, in first-admission order"
    );
    for (image, representative) in &first {
        let held = forced
            .representative(image)
            .expect("every distinct image under the forced digest keeps a representative");
        assert_eq!(
            held.rho_path_channel_image(),
            *image,
            "a forced digest collision aliased two distinct images"
        );
        assert_eq!(
            held, representative,
            "the first exact representative of a distinct image was replaced"
        );
    }
    assert_eq!(forced.len(), first.len(), "the bridge holds one entry per distinct image");
    let real = admit_all::<Blake2bLegacyDigest>(corpus);
    for (image, representative) in &first {
        let legacy_digest = Blake2bLegacyDigest::legacy_digest(&representative.rho_path_identity());
        assert_eq!(
            real.images_with_legacy_digest(&legacy_digest)
                .collect::<Vec<_>>(),
            [image.as_slice()],
            "without a forced collision each distinct image is alone under the legacy digest of \
             its first representative"
        );
        assert_eq!(
            real.representative(image),
            Some(representative),
            "the Blake2b-256 bridge keeps the same first exact representatives"
        );
    }
}

fn check_first_representatives(corpus: &[ProbeChannel]) {
    let first = first_representatives(corpus);
    let bridge = admit_all::<Blake2bLegacyDigest>(corpus);
    assert_eq!(bridge.len(), first.len(), "equal images share one entry of the bridge");
    for (image, representative) in &first {
        assert_eq!(
            bridge.representative(image),
            Some(representative),
            "an equal image reuses the first exact legacy representative and never overwrites it"
        );
        let first_digest = Blake2bLegacyDigest::legacy_digest(&representative.rho_path_identity());
        assert!(
            bridge
                .images_with_legacy_digest(&first_digest)
                .any(|filed| filed == image.as_slice()),
            "an image is filed under the legacy digest of its first exact representative"
        );
    }
    for channel in corpus {
        let image = channel.rho_path_channel_image();
        let representative = bridge
            .representative(&image)
            .expect("every admitted image has a representative");
        if representative.rho_path_identity() != channel.rho_path_identity() {
            let alias_digest = Blake2bLegacyDigest::legacy_digest(&channel.rho_path_identity());
            assert!(
                bridge
                    .images_with_legacy_digest(&alias_digest)
                    .all(|filed| filed != image.as_slice()),
                "an alias files no second entry under its own legacy digest"
            );
        }
    }
}

fn check_atomic_failures(image_limit: usize, corpus: &[ProbeChannel]) {
    let mut bridge: ChannelImageBridge<ProbeChannel> =
        ChannelImageBridge::with_image_limit(image_limit);
    assert_eq!(bridge.image_limit(), image_limit);
    let mut admitted_images: Vec<Vec<u8>> = Vec::with_capacity(corpus.len().min(image_limit));
    for channel in corpus {
        let image = channel.rho_path_channel_image();
        let held = admitted_images.contains(&image);
        let before = bridge.clone();
        match bridge.admit(channel) {
            Ok(()) => {
                assert!(
                    held || admitted_images.len() < image_limit,
                    "the bridge admitted an image beyond its limit"
                );
                assert!(
                    bridge.representative(&image).is_some(),
                    "an admitted image has a representative"
                );
                if !held {
                    admitted_images.push(image);
                }
            }
            Err(error) => {
                assert!(
                    !held && admitted_images.len() == image_limit,
                    "the bridge refused an image within its limit"
                );
                assert_eq!(error, ChannelImageBridgeError::ImageLimitReached {
                    limit: image_limit
                });
                assert_eq!(bridge, before, "a failed admission leaves the bridge unchanged");
                assert!(
                    bridge.representative(&image).is_none(),
                    "a refused image has no representative"
                );
            }
        }
        assert_eq!(
            bridge.len(),
            admitted_images.len(),
            "the bridge holds exactly the admitted images"
        );
    }
    assert_eq!(
        std::mem::size_of::<NoRepresentatives>(),
        0,
        "the V1 representatives are zero-sized"
    );
    let mut none = NoRepresentatives;
    for channel in corpus {
        assert_eq!(
            ChannelRepresentatives::<ProbeChannel>::admit(&mut none, channel),
            Ok(()),
            "the V1 representatives admit every channel"
        );
        assert!(
            ChannelRepresentatives::<ProbeChannel>::representative(
                &none,
                &channel.rho_path_channel_image()
            )
            .is_none(),
            "the V1 representatives hold no channel"
        );
    }
}

fn assert_positional_group(inputs: &[ProbeChannel]) {
    let identities = inputs
        .iter()
        .map(RhoPathEncode::rho_path_identity)
        .collect::<Vec<_>>();
    let prefix = continuation_prefix(inputs);
    let group = <RhoPathV1 as RhoPathKeySyntax>::parse_continuation_group(&prefix)
        .expect("an ordered receive group parses back from its prefix");
    assert_eq!(group.profile(), ReceiveChannelProfile::LegacyV1);
    assert_eq!(
        group.channel_count(),
        inputs.len(),
        "the receive group keeps every position, duplicates included"
    );
    for identity in &identities {
        let mut expected = Vec::with_capacity(identities.len());
        for (position, other) in identities.iter().enumerate() {
            if other == identity {
                expected.push(position);
            }
        }
        let found = group
            .channel_positions_for(ReceiveChannelQuery::LegacyV1(identity))
            .expect("a legacy receive group answers a legacy query")
            .map(|position| position.index())
            .collect::<Vec<_>>();
        assert_eq!(
            found, expected,
            "the receive group answers every position of a channel, in order"
        );
    }
    if let Some(other) = identities
        .iter()
        .position(|identity| *identity != identities[0])
    {
        let mut swapped = inputs.to_vec();
        swapped.swap(0, other);
        assert_ne!(
            continuation_prefix(&swapped),
            prefix,
            "swapping two distinct channels changes the ordered receive group"
        );
    }
    if let Some(repeated) = (1..identities.len())
        .find(|&position| identities[..position].contains(&identities[position]))
    {
        let mut shortened = inputs.to_vec();
        shortened.remove(repeated);
        let shortened_prefix = continuation_prefix(&shortened);
        let shortened_group =
            <RhoPathV1 as RhoPathKeySyntax>::parse_continuation_group(&shortened_prefix)
                .expect("a shortened receive group parses back from its prefix");
        assert_eq!(shortened_group.channel_count(), inputs.len() - 1);
        assert_ne!(
            shortened_prefix, prefix,
            "dropping a duplicate position changes the ordered receive group"
        );
    }
}

fn check_ordered_group(group: &[ProbeChannel]) {
    let bridge = admit_all::<Blake2bLegacyDigest>(group);
    let legacy_inputs = history_inputs(&NoRepresentatives, group);
    assert_eq!(
        legacy_inputs, group,
        "without representatives every position keeps its own channel"
    );
    let bridged_inputs = history_inputs(&bridge, group);
    assert_eq!(bridged_inputs.len(), group.len(), "the bridge keeps every position of the group");
    for (position, (input, channel)) in bridged_inputs.iter().zip(group).enumerate() {
        let image = channel.rho_path_channel_image();
        assert_eq!(input.rho_path_channel_image(), image, "position {position} keeps its image");
        assert_eq!(
            bridge.representative(&image),
            Some(input),
            "position {position} reads the first exact representative of its image"
        );
    }
    assert_positional_group(&legacy_inputs);
    assert_positional_group(&bridged_inputs);
}

fn check_history_identity(group: &[ProbeChannel], reordered: &[ProbeChannel]) {
    for channel in group {
        assert_eq!(
            Blake2bLegacyDigest::legacy_digest(&channel.rho_path_identity()).as_slice(),
            hash(channel).0.as_slice(),
            "the default legacy digest is the history hash of the channel"
        );
    }
    let unbridged_identity = checkpoint_group_identity(group);
    assert_eq!(
        unbridged_identity,
        legacy_group_identity(group),
        "the checkpoint identity hashes the sorted legacy digests of the group"
    );
    assert_eq!(
        checkpoint_group_identity(reordered),
        unbridged_identity,
        "the history identity of today ignores channel order"
    );
    let bridge = admit_all::<Blake2bLegacyDigest>(group);
    let bridged = history_inputs(&bridge, group);
    let bridged_identity = checkpoint_group_identity(&bridged);
    assert_eq!(
        bridged_identity,
        legacy_group_identity(&bridged),
        "the bridged checkpoint identity hashes the sorted legacy digests of the representatives"
    );
    assert_eq!(
        checkpoint_group_identity(&history_inputs(&bridge, reordered)),
        bridged_identity,
        "the bridged history identity ignores channel order"
    );
    let mut repeated = bridged.clone();
    repeated.push(bridged[0].clone());
    assert_ne!(
        checkpoint_group_identity(&repeated),
        bridged_identity,
        "the history identity counts every position"
    );
    let unaliased = bridged
        .iter()
        .zip(group)
        .all(|(input, channel)| input.rho_path_identity() == channel.rho_path_identity());
    if unaliased {
        assert_eq!(
            bridged_identity, unbridged_identity,
            "a group without aliases keeps the history identity of today"
        );
    } else {
        assert_ne!(
            bridged_identity, unbridged_identity,
            "an alias is read through the legacy identity of its first representative"
        );
    }
}

#[test]
fn forced_digest_collisions_retain_distinct_full_images() {
    check_forced_collisions(&collision_witness());
    run_generated(channel_corpus(0..=12), |channels| check_forced_collisions(&channels));
}

#[test]
fn equal_images_keep_the_first_exact_legacy_representative() {
    check_first_representatives(&alias_witness());
    run_generated(channel_corpus(0..=12), |channels| check_first_representatives(&channels));
}

#[test]
fn bridge_failures_are_atomic() {
    check_atomic_failures(0, &alias_witness());
    check_atomic_failures(1, &alias_witness());
    run_generated((0_usize..=4, channel_corpus(0..=12)), |(image_limit, channels)| {
        check_atomic_failures(image_limit, &channels)
    });
}

#[test]
fn ordered_receive_groups_stay_positional_with_duplicates() {
    check_ordered_group(&alias_witness());
    run_generated(channel_corpus(1..=5), |group| check_ordered_group(&group));
}

#[test]
fn unordered_history_group_identity_ignores_channel_order() {
    let group = alias_witness();
    let mut reordered = group.clone();
    reordered.reverse();
    check_history_identity(&group, &reordered);
    run_generated(
        channel_corpus(1..=4)
            .prop_flat_map(|group| (Just(group.clone()), Just(group).prop_shuffle())),
        |(group, reordered)| check_history_identity(&group, &reordered),
    );
}
