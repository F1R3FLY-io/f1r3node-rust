use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::support::{PersistenceTrialDir, TEST_SEMANTIC_PROFILE_ID, trial_scratch};
use crate::rspace::hashing::stable_hash_provider::{hash, hash_from_vec};
use crate::rspace::persistence::SemanticProfileId;
use crate::rspace::persistence::durable::{
    DurableDirectoryV1, PREIMAGE_FILE_NAME_V1, PreimageEntryV1, PreimageIndexError,
    PreimageIndexV1, PreimageKindV1, arm_preimage_partial_write_hook,
};

const TEST_CRC32C: crc::Crc<u32> = crc::Crc::<u32>::new(&crc::CRC_32_ISCSI);
const HEADER_LEN: usize = 48;
const HEADER_AUTH_LEN: usize = 40;
const HEADER_VERSION_OFFSET: usize = 16;
const HEADER_PROFILE_OFFSET: usize = 24;
const ENTRY_PREFIX_LEN: usize = 37;
const ENTRY_OVERHEAD_LEN: usize = 45;
const ALPHA: &str = "preimage-alpha";
const BETA: &str = "preimage-beta";
const GAMMA: &str = "preimage-gamma";
const DELTA: &str = "preimage-delta";
const OTHER_PROFILE_ID: SemanticProfileId = SemanticProfileId::from_array(*b"RSpace.Other.V1\0");
const READERS: usize = 4;
const FREE_RUNNING_BATCHES: usize = 200;
const CHANNELS_PER_BATCH: usize = 6;
const LOOKUP_STRIDE: usize = 997;
const GOLDEN_HEADER: &str = concat!(
    "525370616365507265696d672e563100",
    "0100000000000000",
    "5253706163652e546573742e56310000",
    "cc91267300000000",
);
const GOLDEN_CHANNEL_ENTRY: &str = concat!(
    "01",
    "55a263b5232a80c3f294b3a2f4f75409690d58bc351eb5d625d0fc9153c04948",
    "16000000",
    "0e00000000000000707265696d6167652d616c706861",
    "8153a39500000000",
);
const GOLDEN_GROUP_ENTRY: &str = concat!(
    "02",
    "f2791e7548ed90894238d77987abf1778e81504f05324ee0efe5e213d5b6f7af",
    "40000000",
    "55a263b5232a80c3f294b3a2f4f75409690d58bc351eb5d625d0fc9153c04948",
    "007b3c626f88b96fad5ae28772b27bec374cfeaf6c3c9214e0612fbca3c91fd7",
    "45a06be600000000",
);

type ChannelExpectation = ([u8; 32], Vec<u8>);
type GroupExpectation = ([u8; 32], Vec<Vec<[u8; 32]>>);

struct PreimageTrial {
    directory: DurableDirectoryV1,
    trial: PersistenceTrialDir,
}

impl PreimageTrial {
    fn new(prefix: &str) -> Self {
        let trial = trial_scratch(prefix);
        let directory =
            DurableDirectoryV1::open(trial.path()).expect("open the preimage trial directory");
        Self { directory, trial }
    }

    fn file(&self) -> PathBuf { self.trial.path().join(PREIMAGE_FILE_NAME_V1) }

    fn open(&self) -> PreimageIndexV1 {
        self.open_as(TEST_SEMANTIC_PROFILE_ID)
            .expect("open the preimage index")
    }

    fn open_as(&self, profile: SemanticProfileId) -> Result<PreimageIndexV1, PreimageIndexError> {
        PreimageIndexV1::open(&self.directory, profile)
    }

    fn bytes(&self) -> Vec<u8> { fs::read(self.file()).expect("read the preimage file") }

    fn write(&self, bytes: &[u8]) {
        let file = self.file();
        fs::write(&file, bytes).expect("write the preimage file");
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))
            .expect("restrict the preimage file to its owner");
    }
}

#[derive(Default)]
struct Observations {
    absent: usize,
    partial: usize,
    whole: usize,
}

enum Visibility {
    Absent,
    Partial,
    Whole,
}

impl Observations {
    fn record(&mut self, visibility: Visibility) {
        match visibility {
            Visibility::Absent => self.absent += 1,
            Visibility::Partial => self.partial += 1,
            Visibility::Whole => self.whole += 1,
        }
    }

    fn lookups(&self) -> usize { self.absent + self.partial + self.whole }
}

struct FreeRunningShared {
    channels: Vec<ChannelExpectation>,
    groups: Vec<GroupExpectation>,
    ready: AtomicUsize,
    progress: AtomicUsize,
    finished: AtomicUsize,
    done: AtomicBool,
}

struct CountOnDrop<'a>(&'a AtomicUsize);

impl Drop for CountOnDrop<'_> {
    fn drop(&mut self) { self.0.fetch_add(1, Ordering::SeqCst); }
}

struct SetOnDrop<'a>(&'a AtomicBool);

impl Drop for SetOnDrop<'_> {
    fn drop(&mut self) { self.0.store(true, Ordering::SeqCst); }
}

fn channel_entry(name: &str) -> PreimageEntryV1 { PreimageEntryV1::channel(&name.to_string()) }

fn group_entry(names: &[&str]) -> PreimageEntryV1 {
    let channels: Vec<String> = names.iter().map(|name| (*name).to_string()).collect();
    PreimageEntryV1::ordered_group(&channels)
}

fn entry_key(entry: &PreimageEntryV1) -> [u8; 32] {
    match entry {
        PreimageEntryV1::Channel { key, .. } | PreimageEntryV1::OrderedGroup { key, .. } => *key,
    }
}

fn value_bytes(entry: &PreimageEntryV1) -> &[u8] {
    match entry {
        PreimageEntryV1::Channel { identity, .. } => identity,
        PreimageEntryV1::OrderedGroup { channels, .. } => channels.as_flattened(),
    }
}

fn entry_extent(entry: &PreimageEntryV1) -> usize { ENTRY_OVERHEAD_LEN + value_bytes(entry).len() }

fn group_channels(entry: &PreimageEntryV1) -> Vec<[u8; 32]> {
    match entry {
        PreimageEntryV1::OrderedGroup { channels, .. } => channels.clone(),
        PreimageEntryV1::Channel { .. } => panic!("a channel entry holds no ordered group"),
    }
}

fn channel_hash(name: &str) -> [u8; 32] {
    hash(&name.to_string())
        .0
        .try_into()
        .expect("a channel hash has 32 bytes")
}

fn offset_of(len: usize) -> u64 { u64::try_from(len).expect("a test offset fits u64") }

fn golden(hex_text: &str) -> Vec<u8> {
    hex::decode(hex_text).expect("decode golden preimage bytes")
}

fn raw_entry(kind: u8, key: &[u8; 32], value: &[u8]) -> Vec<u8> {
    let mut entry = Vec::with_capacity(ENTRY_OVERHEAD_LEN + value.len());
    entry.push(kind);
    entry.extend_from_slice(key);
    entry.extend_from_slice(
        &u32::try_from(value.len())
            .expect("a test value length fits u32")
            .to_le_bytes(),
    );
    entry.extend_from_slice(value);
    let checksum = TEST_CRC32C.checksum(&entry);
    entry.extend_from_slice(&u64::from(checksum).to_le_bytes());
    entry
}

fn reseal_header(bytes: &mut [u8]) {
    let checksum = TEST_CRC32C.checksum(&bytes[..HEADER_AUTH_LEN]);
    bytes[HEADER_AUTH_LEN..HEADER_LEN].copy_from_slice(&u64::from(checksum).to_le_bytes());
}

fn flipped(bytes: &[u8], position: usize, mask: u8) -> Vec<u8> {
    let mut damaged = bytes.to_vec();
    damaged[position] ^= mask;
    damaged
}

fn assert_entry_held(index: &PreimageIndexV1, entry: &PreimageEntryV1) {
    match entry {
        PreimageEntryV1::Channel { key, identity } => assert_eq!(
            index.channel(key).expect("look up a channel preimage"),
            Some(identity.clone()),
            "a held channel preimage was not returned"
        ),
        PreimageEntryV1::OrderedGroup { key, channels } => assert!(
            index
                .ordered_groups(key)
                .expect("look up ordered groups")
                .contains(channels),
            "a held ordered group was not returned for its bucket key"
        ),
    }
}

fn assert_entry_unpublished(
    index: &PreimageIndexV1,
    entry: &PreimageEntryV1,
    settled: &[PreimageEntryV1],
) {
    match entry {
        PreimageEntryV1::Channel { key, .. } => assert_eq!(
            index.channel(key).expect("look up an in-flight channel"),
            None,
            "an in-flight channel entry was visible before its batch was whole"
        ),
        PreimageEntryV1::OrderedGroup { key, .. } => {
            let settled_groups: Vec<Vec<[u8; 32]>> = settled
                .iter()
                .filter(|candidate| entry_key(candidate) == *key)
                .map(group_channels)
                .collect();
            assert_eq!(
                index
                    .ordered_groups(key)
                    .expect("look up an in-flight ordered group"),
                settled_groups,
                "an in-flight ordered group was visible before its batch was whole"
            );
        }
    }
}

fn assert_entry_absent(index: &PreimageIndexV1, entry: &PreimageEntryV1) {
    match entry {
        PreimageEntryV1::Channel { key, .. } => assert_eq!(
            index
                .channel(key)
                .expect("look up a channel dropped by the cut"),
            None,
            "a channel recorded at or after the damage must be absent"
        ),
        PreimageEntryV1::OrderedGroup { key, .. } => assert!(
            index
                .ordered_groups(key)
                .expect("look up an ordered group dropped by the cut")
                .is_empty(),
            "an ordered group recorded at or after the damage must be absent"
        ),
    }
}

fn assert_cut_and_repaired(
    trial: &PreimageTrial,
    damaged: &[u8],
    pristine: &[u8],
    entries: &[PreimageEntryV1],
    kept_len: usize,
    kept_entries: usize,
) {
    trial.write(damaged);
    let reopened = trial.open();
    assert_eq!(
        trial.bytes(),
        &pristine[..kept_len],
        "open must cut the file at the first damaged entry"
    );
    let (kept, dropped) = entries.split_at(kept_entries);
    for entry in kept {
        assert_entry_held(&reopened, entry);
    }
    for entry in dropped {
        assert_entry_absent(&reopened, entry);
    }
    let fresh = channel_entry(DELTA);
    let mut batch = Vec::with_capacity(dropped.len() + 1);
    batch.extend_from_slice(dropped);
    batch.push(fresh.clone());
    assert!(
        reopened.append(&batch).expect("append after the cut"),
        "an append after the cut must write the dropped entries and a fresh one"
    );
    let repaired =
        [pristine, raw_entry(1, &entry_key(&fresh), value_bytes(&fresh)).as_slice()].concat();
    assert_eq!(trial.bytes(), repaired, "an append after the cut must land directly after it");
    for entry in entries.iter().chain([&fresh]) {
        assert_entry_held(&reopened, entry);
    }
    reopened
        .sync()
        .expect("sync the entries appended after the cut");
    drop(reopened);
    let again = trial.open();
    assert_eq!(trial.bytes(), repaired, "reopening the repaired file must not change it");
    for entry in entries.iter().chain([&fresh]) {
        assert_entry_held(&again, entry);
    }
}

fn refused_header(
    trial: &PreimageTrial,
    bytes: &[u8],
    profile: SemanticProfileId,
) -> PreimageIndexError {
    trial.write(bytes);
    match trial.open_as(profile) {
        Err(error) => {
            assert_eq!(trial.bytes(), bytes, "a refused open changed the preimage file");
            error
        }
        Ok(_) => panic!("a foreign or unknown header was admitted at open"),
    }
}

fn observe_channel(index: &PreimageIndexV1, expectation: &ChannelExpectation) -> Visibility {
    let (key, identity) = expectation;
    match index
        .channel(key)
        .expect("a concurrent channel lookup failed")
    {
        None => Visibility::Absent,
        Some(found) => {
            assert_eq!(&found, identity, "a concurrent channel lookup returned foreign bytes");
            Visibility::Whole
        }
    }
}

fn observe_groups(index: &PreimageIndexV1, expectation: &GroupExpectation) -> Visibility {
    let (bucket, orders) = expectation;
    let groups = index
        .ordered_groups(bucket)
        .expect("a concurrent ordered group lookup failed");
    assert!(
        orders.starts_with(&groups),
        "a concurrent ordered group lookup returned lists that are not a prefix of the append \
         order"
    );
    match groups.len() {
        0 => Visibility::Absent,
        found if found == orders.len() => Visibility::Whole,
        _ => Visibility::Partial,
    }
}

fn read_free_running(
    index: &PreimageIndexV1,
    shared: &FreeRunningShared,
    seed: usize,
) -> Observations {
    let _exit = CountOnDrop(&shared.finished);
    for expectation in &shared.channels {
        assert!(matches!(observe_channel(index, expectation), Visibility::Absent));
    }
    for expectation in &shared.groups {
        assert!(matches!(observe_groups(index, expectation), Visibility::Absent));
    }
    shared.ready.fetch_add(1, Ordering::SeqCst);
    let total = shared.channels.len() + shared.groups.len();
    let mut cursor = seed;
    let mut observations = Observations::default();
    while !shared.done.load(Ordering::SeqCst) {
        cursor = (cursor + LOOKUP_STRIDE) % total;
        let visibility = match cursor.checked_sub(shared.channels.len()) {
            None => observe_channel(index, &shared.channels[cursor]),
            Some(group) => observe_groups(index, &shared.groups[group]),
        };
        observations.record(visibility);
        shared.progress.fetch_add(1, Ordering::SeqCst);
    }
    for expectation in &shared.channels {
        assert!(matches!(observe_channel(index, expectation), Visibility::Whole));
    }
    for expectation in &shared.groups {
        assert!(matches!(observe_groups(index, expectation), Visibility::Whole));
    }
    observations
}

#[test]
fn entries_survive_a_reopen_and_match_the_golden_bytes() {
    let trial = PreimageTrial::new("preimage-golden-");
    let index = trial.open();
    let alpha = channel_entry(ALPHA);
    let group = group_entry(&[ALPHA, BETA]);
    assert_eq!(
        index
            .channel(&entry_key(&alpha))
            .expect("look up an empty index"),
        None
    );
    assert!(
        index
            .ordered_groups(&entry_key(&group))
            .expect("look up groups in an empty index")
            .is_empty()
    );
    index.sync().expect("sync an index that has no file yet");
    assert!(
        !trial.file().exists(),
        "a missing preimage file must stay missing until the first append"
    );

    assert_eq!(entry_key(&alpha), channel_hash(ALPHA));
    assert_eq!(
        entry_key(&group).to_vec(),
        hash_from_vec(&vec![ALPHA.to_string(), BETA.to_string()]).0
    );
    assert_eq!(group_channels(&group), vec![channel_hash(ALPHA), channel_hash(BETA)]);
    let golden_header = golden(GOLDEN_HEADER);
    let golden_channel = golden(GOLDEN_CHANNEL_ENTRY);
    let golden_group = golden(GOLDEN_GROUP_ENTRY);
    assert_eq!(golden_header.len(), HEADER_LEN);
    assert_eq!(raw_entry(1, &entry_key(&alpha), value_bytes(&alpha)), golden_channel);
    assert_eq!(raw_entry(2, &entry_key(&group), value_bytes(&group)), golden_group);

    assert!(
        index
            .append(std::slice::from_ref(&alpha))
            .expect("append the channel entry")
    );
    assert_eq!(trial.bytes(), [golden_header.as_slice(), golden_channel.as_slice()].concat());
    let mode = fs::metadata(trial.file())
        .expect("stat the preimage file")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "the first append must create an owner-only file");
    assert!(
        index
            .append(std::slice::from_ref(&group))
            .expect("append the ordered group entry")
    );
    let golden_file = [golden_header, golden_channel, golden_group].concat();
    assert_eq!(trial.bytes(), golden_file);
    assert!(
        !index
            .append(&[alpha.clone(), group.clone(), alpha.clone()])
            .expect("re-append held entries"),
        "re-appending held entries must report that nothing was appended"
    );
    assert_eq!(trial.bytes(), golden_file, "skipping held entries must write nothing");

    let mut entries = Vec::with_capacity(64);
    entries.extend((0..48).map(|n| channel_entry(&format!("preimage-channel-{n}"))));
    entries.extend((0..16).map(|n| {
        group_entry(&[&format!("preimage-channel-{n}"), &format!("preimage-channel-{}", n + 16)])
    }));
    assert!(index.append(&entries).expect("append a mixed batch"));
    index.sync().expect("sync the appended entries");
    let synced = trial.bytes();
    assert_eq!(synced.len(), golden_file.len() + entries.iter().map(entry_extent).sum::<usize>());
    assert_eq!(synced[..golden_file.len()], golden_file[..]);
    drop(index);

    let reopened = trial.open();
    assert_eq!(trial.bytes(), synced, "reopening a whole file must not change it");
    for entry in [&alpha, &group].into_iter().chain(&entries) {
        assert_entry_held(&reopened, entry);
    }
    assert_eq!(
        reopened
            .ordered_groups(&entry_key(&group))
            .expect("look up the golden group after reopening"),
        vec![group_channels(&group)]
    );
    assert!(
        !reopened
            .append(&entries)
            .expect("re-append after reopening")
    );
    assert_eq!(trial.bytes(), synced);
}

#[test]
fn open_cuts_the_file_at_the_first_damaged_entry() {
    let trial = PreimageTrial::new("preimage-cut-");
    let entries = vec![
        channel_entry(ALPHA),
        channel_entry(BETA),
        group_entry(&[ALPHA, BETA]),
        channel_entry(GAMMA),
    ];
    let index = trial.open();
    assert!(index.append(&entries).expect("append the pristine entries"));
    index.sync().expect("sync the pristine entries");
    drop(index);
    let pristine = trial.bytes();
    let mut offsets = Vec::with_capacity(entries.len() + 1);
    let mut position = HEADER_LEN;
    for entry in &entries {
        offsets.push(position);
        position += entry_extent(entry);
    }
    offsets.push(position);
    assert_eq!(pristine.len(), position);
    let last = entries.len() - 1;
    let last_offset = offsets[last];
    let last_extent = entry_extent(&entries[last]);

    for cut in [
        1,
        ENTRY_PREFIX_LEN - 1,
        ENTRY_PREFIX_LEN,
        ENTRY_PREFIX_LEN + 3,
        last_extent - 8,
        last_extent - 1,
    ] {
        let torn = &pristine[..last_offset + cut];
        assert_cut_and_repaired(&trial, torn, &pristine, &entries, last_offset, last);
    }

    let mut zero_tail = pristine.clone();
    zero_tail.resize(pristine.len() + 4096, 0);
    assert_cut_and_repaired(&trial, &zero_tail, &pristine, &entries, pristine.len(), entries.len());

    let damaged_last = flipped(&pristine, last_offset + ENTRY_PREFIX_LEN, 0x01);
    assert_cut_and_repaired(&trial, &damaged_last, &pristine, &entries, last_offset, last);

    let damaged_offset = offsets[1];
    let damaged_extent = entry_extent(&entries[1]);
    for (position, mask) in [
        (damaged_offset, 0x02),
        (damaged_offset + 1, 0x80),
        (damaged_offset + ENTRY_PREFIX_LEN - 4, 0x01),
        (damaged_offset + ENTRY_PREFIX_LEN - 1, 0x40),
        (damaged_offset + ENTRY_PREFIX_LEN, 0x01),
        (damaged_offset + damaged_extent - 9, 0x10),
        (damaged_offset + damaged_extent - 8, 0x04),
        (damaged_offset + damaged_extent - 1, 0x01),
    ] {
        let damaged = flipped(&pristine, position, mask);
        assert_cut_and_repaired(&trial, &damaged, &pristine, &entries, damaged_offset, 1);
    }

    let mut unknown_kind = pristine[..damaged_offset].to_vec();
    unknown_kind.extend(raw_entry(7, &entry_key(&entries[1]), value_bytes(&entries[1])));
    unknown_kind.extend_from_slice(&pristine[offsets[2]..]);
    assert_cut_and_repaired(&trial, &unknown_kind, &pristine, &entries, damaged_offset, 1);
    let mut ragged_group = pristine[..offsets[2]].to_vec();
    ragged_group.extend(raw_entry(2, &entry_key(&entries[2]), &value_bytes(&entries[2])[..33]));
    ragged_group.extend_from_slice(&pristine[offsets[3]..]);
    assert_cut_and_repaired(&trial, &ragged_group, &pristine, &entries, offsets[2], 2);

    for damaged_header in [
        flipped(&pristine, HEADER_PROFILE_OFFSET, 0x01),
        flipped(&pristine, HEADER_LEN - 1, 0x01),
        pristine[..HEADER_LEN - 1].to_vec(),
    ] {
        assert_cut_and_repaired(&trial, &damaged_header, &pristine, &entries, 0, 0);
    }

    let error = refused_header(&trial, &pristine, OTHER_PROFILE_ID);
    assert!(
        matches!(
            error,
            PreimageIndexError::ForeignProfile { expected, actual }
                if expected == OTHER_PROFILE_ID && actual == TEST_SEMANTIC_PROFILE_ID
        ),
        "{error}"
    );
    let mut future_version = pristine.clone();
    future_version[HEADER_VERSION_OFFSET] = 2;
    reseal_header(&mut future_version);
    let error = refused_header(&trial, &future_version, TEST_SEMANTIC_PROFILE_ID);
    assert!(matches!(error, PreimageIndexError::UnsupportedVersion(2)), "{error}");
    let error = refused_header(&trial, &flipped(&pristine, 0, 0x01), TEST_SEMANTIC_PROFILE_ID);
    assert!(matches!(error, PreimageIndexError::UnknownMagic), "{error}");
}

#[test]
fn a_forged_entry_is_refused_at_append_and_at_lookup() {
    let trial = PreimageTrial::new("preimage-forged-");
    let index = trial.open();
    let alpha = channel_entry(ALPHA);
    let beta = channel_entry(BETA);
    let group = group_entry(&[ALPHA, BETA]);
    let gamma_beta = group_entry(&[GAMMA, BETA]);

    let unhashed_channel = PreimageEntryV1::Channel {
        key: entry_key(&alpha),
        identity: value_bytes(&beta).to_vec(),
    };
    match index.append(std::slice::from_ref(&unhashed_channel)) {
        Err(PreimageIndexError::UnhashedEntry {
            kind: PreimageKindV1::Channel,
            key,
        }) => assert_eq!(key, entry_key(&alpha)),
        Err(other) => panic!("an unhashed channel entry was refused with the wrong error: {other}"),
        Ok(appended) => panic!("an unhashed channel entry was admitted (appended: {appended})"),
    }
    let unhashed_group = PreimageEntryV1::OrderedGroup {
        key: entry_key(&gamma_beta),
        channels: group_channels(&group),
    };
    match index.append(&[alpha.clone(), unhashed_group]) {
        Err(PreimageIndexError::UnhashedEntry {
            kind: PreimageKindV1::OrderedGroup,
            key,
        }) => assert_eq!(key, entry_key(&gamma_beta)),
        Err(other) => panic!("an unhashed group entry was refused with the wrong error: {other}"),
        Ok(appended) => panic!("an unhashed group entry was admitted (appended: {appended})"),
    }
    for (inadmissible, expected_kind) in [
        (
            PreimageEntryV1::Channel {
                key: [0; 32],
                identity: Vec::new(),
            },
            PreimageKindV1::Channel,
        ),
        (
            PreimageEntryV1::OrderedGroup {
                key: [0; 32],
                channels: Vec::new(),
            },
            PreimageKindV1::OrderedGroup,
        ),
    ] {
        match index.append(&[inadmissible]) {
            Err(PreimageIndexError::InvalidEntryLength { kind, length: 0 }) => {
                assert_eq!(kind, expected_kind)
            }
            Err(other) => panic!("an empty value was refused with the wrong error: {other}"),
            Ok(appended) => panic!("an empty value was admitted (appended: {appended})"),
        }
    }
    assert!(
        !trial.file().exists(),
        "refused batches must append nothing, not even their admissible entries"
    );

    assert!(
        index
            .append(&[alpha.clone(), group.clone()])
            .expect("append the genuine entries")
    );
    index.sync().expect("sync the genuine entries");
    drop(index);
    let mut bytes = trial.bytes();
    let forged_channel_offset = offset_of(bytes.len());
    bytes.extend(raw_entry(1, &entry_key(&beta), value_bytes(&alpha)));
    let forged_group_offset = offset_of(bytes.len());
    bytes.extend(raw_entry(2, &entry_key(&gamma_beta), value_bytes(&group)));
    trial.write(&bytes);
    let reopened = trial.open();
    assert_eq!(trial.bytes(), bytes, "forged entries with valid checksums are whole at open");

    match reopened.channel(&entry_key(&beta)) {
        Err(PreimageIndexError::ForgedEntry {
            offset,
            kind: PreimageKindV1::Channel,
        }) => assert_eq!(offset, forged_channel_offset),
        Ok(Some(_)) => panic!("a forged preimage entry was returned by lookup"),
        Ok(None) => panic!("the forged channel entry was not indexed at open"),
        Err(other) => panic!("a forged channel entry was refused with the wrong error: {other}"),
    }
    match reopened.ordered_groups(&entry_key(&gamma_beta)) {
        Err(PreimageIndexError::ForgedEntry {
            offset,
            kind: PreimageKindV1::OrderedGroup,
        }) => assert_eq!(offset, forged_group_offset),
        Ok(groups) if groups.is_empty() => panic!("the forged group entry was not indexed at open"),
        Ok(_) => panic!("a forged preimage entry was returned by lookup"),
        Err(other) => panic!("a forged group entry was refused with the wrong error: {other}"),
    }
    assert_entry_held(&reopened, &alpha);
    assert_entry_held(&reopened, &group);
    match reopened.append(std::slice::from_ref(&beta)) {
        Err(PreimageIndexError::ForgedEntry {
            offset,
            kind: PreimageKindV1::Channel,
        }) => assert_eq!(offset, forged_channel_offset),
        Err(other) => panic!("an append over a forged entry failed with the wrong error: {other}"),
        Ok(appended) => panic!("an append over a forged entry succeeded (appended: {appended})"),
    }
    assert_eq!(trial.bytes(), bytes, "a refused append must write nothing");
}

#[test]
fn two_orders_of_one_channel_set_share_a_bucket_key() {
    let trial = PreimageTrial::new("preimage-orders-");
    let index = trial.open();
    let alpha_hash = channel_hash(ALPHA);
    let beta_hash = channel_hash(BETA);
    let gamma_hash = channel_hash(GAMMA);
    let forward = group_entry(&[ALPHA, BETA]);
    let backward = group_entry(&[BETA, ALPHA]);
    let bucket = entry_key(&forward);
    assert_eq!(entry_key(&backward), bucket, "(a,b) and (b,a) must share their bucket key");
    assert_eq!(bucket.to_vec(), hash_from_vec(&vec![ALPHA.to_string(), BETA.to_string()]).0);
    assert_eq!(bucket.to_vec(), hash_from_vec(&vec![BETA.to_string(), ALPHA.to_string()]).0);
    assert_eq!(group_channels(&forward), vec![alpha_hash, beta_hash]);
    assert_eq!(group_channels(&backward), vec![beta_hash, alpha_hash]);

    assert!(
        index
            .append(std::slice::from_ref(&forward))
            .expect("append (a,b)")
    );
    assert_eq!(index.ordered_groups(&bucket).expect("look up (a,b)"), vec![vec![
        alpha_hash, beta_hash
    ]]);
    assert!(
        index
            .append(std::slice::from_ref(&backward))
            .expect("append (b,a)"),
        "(b,a) is another list under a held key and must be appended"
    );
    let both = vec![vec![alpha_hash, beta_hash], vec![beta_hash, alpha_hash]];
    assert_eq!(index.ordered_groups(&bucket).expect("look up both orders"), both);
    assert!(
        !index
            .append(&[backward.clone(), forward.clone()])
            .expect("re-append both orders")
    );

    let triple_orders = [
        [ALPHA, BETA, GAMMA],
        [ALPHA, GAMMA, BETA],
        [BETA, ALPHA, GAMMA],
        [BETA, GAMMA, ALPHA],
        [GAMMA, ALPHA, BETA],
        [GAMMA, BETA, ALPHA],
    ];
    let triples: Vec<PreimageEntryV1> = triple_orders
        .iter()
        .map(|order| group_entry(order))
        .collect();
    let triple_bucket = entry_key(&triples[0]);
    assert!(
        triples
            .iter()
            .all(|entry| entry_key(entry) == triple_bucket)
    );
    assert_ne!(triple_bucket, bucket);
    assert_eq!(
        triple_bucket.to_vec(),
        hash_from_vec(&vec![GAMMA.to_string(), ALPHA.to_string(), BETA.to_string()]).0
    );
    assert!(
        index
            .append(&triples)
            .expect("append all six orders of three channels")
    );
    let six: Vec<Vec<[u8; 32]>> = triples.iter().map(group_channels).collect();
    assert_eq!(six[3], vec![beta_hash, gamma_hash, alpha_hash]);
    assert_eq!(
        index
            .ordered_groups(&triple_bucket)
            .expect("look up six orders"),
        six
    );
    index.sync().expect("sync the ordered groups");
    drop(index);

    let reopened = trial.open();
    assert_eq!(
        reopened
            .ordered_groups(&bucket)
            .expect("look up both orders after reopening"),
        both
    );
    assert_eq!(
        reopened
            .ordered_groups(&triple_bucket)
            .expect("look up six orders after reopening"),
        six
    );
    assert_eq!(
        reopened
            .channel(&bucket)
            .expect("look up a bucket key as a channel"),
        None
    );
}

#[test]
fn lookups_on_other_threads_see_only_whole_entries() {
    let trial = PreimageTrial::new("preimage-threads-");
    let index = Arc::new(trial.open());
    let mut settled: Vec<PreimageEntryV1> = Vec::with_capacity(9);
    settled.extend((0..8).map(|n| channel_entry(&format!("settled-{n}"))));
    settled.push(group_entry(&["settled-0", "settled-1"]));
    assert!(index.append(&settled).expect("append the settled entries"));
    let in_flight = vec![
        channel_entry("in-flight-0"),
        channel_entry("in-flight-1"),
        group_entry(&["settled-1", "settled-0"]),
        channel_entry("in-flight-2"),
    ];
    let start = trial.bytes().len();
    let batch_len: usize = in_flight.iter().map(entry_extent).sum();
    let prefix_len = entry_extent(&in_flight[0]) + ENTRY_PREFIX_LEN / 2;
    assert!(prefix_len < batch_len);
    let observers = Arc::new(AtomicUsize::new(0));
    arm_preimage_partial_write_hook(prefix_len, {
        let index = Arc::clone(&index);
        let observers = Arc::clone(&observers);
        let settled = settled.clone();
        let in_flight = in_flight.clone();
        let file = trial.file();
        move || {
            let length = fs::metadata(&file)
                .expect("stat the half-written preimage file")
                .len();
            assert_eq!(
                length,
                offset_of(start + prefix_len),
                "the hook must run while the file holds a strict prefix of the batch"
            );
            std::thread::scope(|scope| {
                let readers: Vec<_> = (0..READERS)
                    .map(|_| {
                        scope.spawn(|| {
                            for entry in &settled {
                                assert_entry_held(&index, entry);
                            }
                            for entry in &in_flight {
                                assert_entry_unpublished(&index, entry, &settled);
                            }
                            observers.fetch_add(1, Ordering::SeqCst);
                        })
                    })
                    .collect();
                for reader in readers {
                    reader
                        .join()
                        .expect("a reader of the half-written batch panicked");
                }
            });
        }
    });
    assert!(
        index
            .append(&in_flight)
            .expect("append the in-flight batch")
    );
    assert_eq!(
        observers.load(Ordering::SeqCst),
        READERS,
        "every reader must have looked up the index while the batch was half written"
    );
    for entry in settled.iter().chain(&in_flight) {
        assert_entry_held(&index, entry);
    }
    assert_eq!(
        index
            .ordered_groups(&entry_key(&in_flight[2]))
            .expect("look up the shared bucket after the batch"),
        vec![group_channels(&settled[8]), group_channels(&in_flight[2])]
    );

    let channel_count = FREE_RUNNING_BATCHES * CHANNELS_PER_BATCH;
    let free_channels: Vec<PreimageEntryV1> = (0..channel_count)
        .map(|n| channel_entry(&format!("free-running-{n}")))
        .collect();
    let forward: Vec<PreimageEntryV1> = (0..FREE_RUNNING_BATCHES)
        .map(|n| {
            group_entry(&[
                &format!("free-running-{}", 2 * n),
                &format!("free-running-{}", 2 * n + 1),
            ])
        })
        .collect();
    let backward: Vec<PreimageEntryV1> = (0..FREE_RUNNING_BATCHES)
        .map(|n| {
            group_entry(&[
                &format!("free-running-{}", 2 * n + 1),
                &format!("free-running-{}", 2 * n),
            ])
        })
        .collect();
    let mut batches: Vec<Vec<PreimageEntryV1>> = (0..FREE_RUNNING_BATCHES)
        .map(|_| Vec::with_capacity(CHANNELS_PER_BATCH + 3))
        .collect();
    for (n, entry) in free_channels.iter().enumerate() {
        batches[n / CHANNELS_PER_BATCH].push(entry.clone());
    }
    for (n, (forward_entry, backward_entry)) in forward.iter().zip(&backward).enumerate() {
        batches[n].push(forward_entry.clone());
        let backward_batch = match n < FREE_RUNNING_BATCHES / 2 {
            true => n + FREE_RUNNING_BATCHES / 2,
            false => n,
        };
        batches[backward_batch].push(backward_entry.clone());
    }
    let shared = FreeRunningShared {
        channels: free_channels
            .iter()
            .map(|entry| (entry_key(entry), value_bytes(entry).to_vec()))
            .collect(),
        groups: forward
            .iter()
            .zip(&backward)
            .map(|(forward_entry, backward_entry)| {
                (entry_key(forward_entry), vec![
                    group_channels(forward_entry),
                    group_channels(backward_entry),
                ])
            })
            .collect(),
        ready: AtomicUsize::new(0),
        progress: AtomicUsize::new(0),
        finished: AtomicUsize::new(0),
        done: AtomicBool::new(false),
    };
    let index: &PreimageIndexV1 = &index;
    let observations: Vec<Observations> = std::thread::scope(|scope| {
        let shared = &shared;
        let readers: Vec<_> = (0..READERS)
            .map(|reader| scope.spawn(move || read_free_running(index, shared, reader * 350)))
            .collect();
        let writer_exit = SetOnDrop(&shared.done);
        while shared.ready.load(Ordering::SeqCst) < READERS &&
            shared.finished.load(Ordering::SeqCst) == 0
        {
            std::thread::yield_now();
        }
        for (batch_number, batch) in batches.iter().enumerate() {
            while shared.progress.load(Ordering::SeqCst) < (batch_number + 1) * READERS &&
                shared.finished.load(Ordering::SeqCst) == 0
            {
                std::thread::yield_now();
            }
            assert!(index.append(batch).expect("append a free-running batch"));
        }
        drop(writer_exit);
        readers
            .into_iter()
            .map(|reader| reader.join().expect("a free-running reader panicked"))
            .collect()
    });
    assert_eq!(observations.len(), READERS);
    assert!(
        observations
            .iter()
            .map(Observations::lookups)
            .sum::<usize>() >=
            FREE_RUNNING_BATCHES * READERS,
        "the readers must have looked up the index between the free-running appends"
    );
    for entry in free_channels.iter().chain(&forward).chain(&backward) {
        assert_entry_held(index, entry);
    }
}
