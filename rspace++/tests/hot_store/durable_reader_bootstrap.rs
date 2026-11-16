use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use tokio::sync::RwLock;

use super::*;
use crate::rspace::persistence::{
    CatalogPackStateV1, DagTransactionLimits, DurableAdmissionLimitsV1, DurableLimitConfig,
    DurableLimits, DurableRSpaceAuthorityV1, DurableRSpacePublicationLimitsV1,
    DurableRSpaceTransactionError, DurableViewReaderV1, INDEX_HEADER_LEN_V1, LogPackAddress,
    LogPackDecodeLimits, LogPackSequence, ObjectDigest, PACK_HEADER_LEN_V1, PackId, PhysicalDigest,
    SemanticProfileId, StoreId, TRANSACTION_COMMIT_RECORD_LEN_V1, TrustedSuccessorPolicyV1,
    create_empty_store_v1, recover_read_authority,
};

type Profile = RSpaceSemanticProfile<String, String, String, String>;
type View = DurableAuthorityViewV1<Profile>;
type Owner = DurableRSpaceAuthorityV1<Profile>;
type Store = Arc<Box<dyn HotStore<String, String, String, String>>>;
type Reader = Box<dyn HistoryReaderBase<String, String, String, String>>;
type Cache = HotStoreState<String, String, String, String>;
type Continuation = WaitingContinuation<String, String>;

const CHANNELS: [&str; 3] = ["a", "b", "c"];
const VALUES: [&str; 3] = ["v0", "v1", "v2"];
const BODIES: [&str; 2] = ["k0", "k1"];
const STORED_GROUPS: [&[&str]; 6] =
    [&["a"], &["b"], &["a", "b"], &["b", "a"], &["a", "a"], &["c", "a", "b"]];
const UNSTORED_GROUPS: [&[&str]; 3] = [&["c"], &["c", "a"], &["a", "b", "c"]];
const PROBED_KEYS: usize = 2 * CHANNELS.len() + STORED_GROUPS.len() + UNSTORED_GROUPS.len();
const AGREEMENT_CASES: u32 = 32;
const CHECKPOINT_CASES: u32 = 16;
const BIND_CASES: u32 = 256;
const WRITER_LOCK_FILE: &str = "writer.lock.v1";
const BASE_MANIFEST: SyntheticManifest = SyntheticManifest {
    digest_seed: 0,
    encoded_len: 32,
};

fn channel(name: &str) -> String { name.to_owned() }

fn group(names: &[&str]) -> Vec<String> { names.iter().copied().map(channel).collect() }

fn datum(channel_name: &str, value: &str) -> Datum<String> {
    Datum::create(&channel(channel_name), value.to_owned(), false)
}

fn continuation(names: &[&str], body: &str) -> Continuation {
    let patterns: Vec<String> = names.iter().map(|name| format!("pattern-{name}")).collect();
    WaitingContinuation::create(&group(names), &patterns, &body.to_owned(), false, BTreeSet::new())
}

fn index_of(names: &[&str], value: &str) -> usize {
    names
        .iter()
        .position(|name| *name == value)
        .expect("an enumerated value belongs to the test universe")
}

fn stored_group(names: &[&str]) -> usize {
    STORED_GROUPS
        .iter()
        .position(|stored| *stored == names)
        .expect("the group is one of the stored groups")
}

fn probed_groups() -> impl Iterator<Item = &'static [&'static str]> {
    STORED_GROUPS.into_iter().chain(UNSTORED_GROUPS)
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ReadKey {
    Data(String),
    Continuations(Vec<String>),
    Joins(String),
}

fn probed_keys() -> Vec<ReadKey> {
    let mut keys = Vec::with_capacity(PROBED_KEYS);
    keys.extend(
        CHANNELS
            .into_iter()
            .map(|name| ReadKey::Data(channel(name))),
    );
    keys.extend(probed_groups().map(|names| ReadKey::Continuations(group(names))));
    keys.extend(
        CHANNELS
            .into_iter()
            .map(|name| ReadKey::Joins(channel(name))),
    );
    keys
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CacheMask(u16);

impl CacheMask {
    const NONE: Self = Self(0);
    const EVERY_KEY: Self = Self((1 << PROBED_KEYS) - 1);

    fn of(keys: &[ReadKey]) -> Self {
        let probed = probed_keys();
        Self(keys.iter().fold(0, |bits, key| {
            let position = probed
                .iter()
                .position(|probe| probe == key)
                .expect("a cached key is one of the probed keys");
            bits | (1 << position)
        }))
    }

    fn caches(self, key: &ReadKey) -> bool {
        match probed_keys().iter().position(|probe| probe == key) {
            Some(position) => (self.0 >> position) & 1 == 1,
            None => false,
        }
    }
}

#[derive(Default)]
struct ReaderLedger {
    reads: Mutex<Vec<(ReadKey, usize)>>,
}

impl ReaderLedger {
    fn record(&self, key: ReadKey, records: usize) {
        self.reads
            .lock()
            .expect("the reader ledger is never poisoned")
            .push((key, records));
    }

    fn reads(&self) -> Vec<(ReadKey, usize)> {
        self.reads
            .lock()
            .expect("the reader ledger is never poisoned")
            .clone()
    }

    fn reads_with_records(&self) -> usize {
        self.reads
            .lock()
            .expect("the reader ledger is never poisoned")
            .iter()
            .filter(|(_, records)| *records > 0)
            .count()
    }
}

struct CountingReader {
    inner: Reader,
    ledger: Arc<ReaderLedger>,
}

impl HistoryReaderBase<String, String, String, String> for CountingReader {
    fn get_data_canonical(&self, key: &String) -> Vec<Datum<String>> {
        let records = self.inner.get_data_canonical(key);
        self.ledger
            .record(ReadKey::Data(key.clone()), records.len());
        records
    }

    fn get_continuations_canonical(&self, key: &[String]) -> Vec<Continuation> {
        let records = self.inner.get_continuations_canonical(key);
        self.ledger
            .record(ReadKey::Continuations(key.to_vec()), records.len());
        records
    }

    fn get_joins_canonical(&self, key: &String) -> Vec<Vec<String>> {
        let records = self.inner.get_joins_canonical(key);
        self.ledger
            .record(ReadKey::Joins(key.clone()), records.len());
        records
    }
}

#[derive(Clone, Copy, Debug)]
enum ReaderChoice {
    Durable,
    Closed,
}

struct TrialDirectory(PathBuf);

impl TrialDirectory {
    fn new(label: &str) -> Self {
        static ROOT: OnceLock<shared::rust::test_scratch::ProcessScratchDir> = OnceLock::new();
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let root = ROOT.get_or_init(|| {
            shared::rust::test_scratch::acquire("rspace-hot-store-durable-reader-")
        });
        let path = root
            .path()
            .join(format!("{label}-{}", SEQUENCE.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&path).expect("create an isolated durable trial directory");
        Self(path)
    }

    fn path(&self) -> &Path { &self.0 }
}

impl Drop for TrialDirectory {
    fn drop(&mut self) {
        match (std::fs::remove_dir_all(&self.0), std::thread::panicking()) {
            (Err(error), false) => {
                panic!("cannot remove the durable trial directory {}: {error}", self.0.display())
            }
            (Ok(()), _) | (Err(_), true) => {}
        }
    }
}

fn recovery_limits() -> DurableLimits {
    DurableLimits::new(DurableLimitConfig {
        max_live_packs: 8,
        max_pack_bytes: 16 * 1024 * 1024,
        max_transactions: 1024,
        max_object_records: 65_536,
        max_total_scan_bytes: 64 * 1024 * 1024,
        max_total_physical_read_bytes: 128 * 1024 * 1024,
        max_index_bytes: 16 * 1024 * 1024,
        max_index_records: 65_536,
        max_full_byte_comparison_bytes: 16 * 1024 * 1024,
        max_aggregate_full_byte_comparison_bytes: 64 * 1024 * 1024,
    })
    .expect("the durable trial limits are positive")
}

fn publication_limits() -> DurableRSpacePublicationLimitsV1 {
    DurableRSpacePublicationLimitsV1 {
        transaction: DagTransactionLimits::UNBOUNDED,
        recovery: recovery_limits(),
        wal: LogPackDecodeLimits::UNBOUNDED,
        admission: DurableAdmissionLimitsV1::UNBOUNDED,
        trusted: TrustedSuccessorPolicyV1::UNBOUNDED,
    }
}

fn recover(directory: &Path) -> View {
    recover_read_authority::<Profile>(
        directory,
        recovery_limits(),
        LogPackDecodeLimits::UNBOUNDED,
        DurableAdmissionLimitsV1::UNBOUNDED,
    )
    .expect("recover a durable view of the trial store")
}

fn eager_store(view: &View) -> Store {
    Arc::new(
        HotStoreInstances::create_from_durable(view, DurableHotStoreBootstrapLimits::default())
            .expect("the eager bootstrap enumerates the base state"),
    )
}

fn lazy_store(
    view: &View,
    cache: Cache,
    choice: ReaderChoice,
    ledger: &Arc<ReaderLedger>,
) -> Store {
    let inner: Reader = match choice {
        ReaderChoice::Durable => Box::new(DurableViewReaderV1::new(
            view.read_handle()
                .expect("take a read handle on the view's generation"),
            view.semantic_roots()
                .expect("read the semantic roots of the view's state"),
        )),
        ReaderChoice::Closed => Box::new(ClosedDurableHistoryReader),
    };
    let reader = CountingReader {
        inner,
        ledger: Arc::clone(ledger),
    };
    Arc::new(
        HotStoreInstances::create_from_durable_reader(
            Box::new(reader),
            cache,
            DurableJournalBaseV1 {
                manifest: view.manifest(),
                registry: view.generation_leases(),
            },
        )
        .expect("a cache read from the base state loads into the lazy store"),
    )
}

fn owner_of(view: View, hot: &Store) -> Owner {
    view.into_rspace_authority(hot).unwrap_or_else(|refusal| {
        panic!("the view refused to hand its authority to a bound hot store: {:?}", refusal.1)
    })
}

fn complete_uncontended<T>(future: impl std::future::Future<Output = T>) -> T {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let mut future = std::pin::pin!(tokio::task::unconstrained(future));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("an uncontended owner checkpoint completes on its first poll"),
    }
}

fn checkpoint_through(
    owner: &mut Owner,
    attached: &Store,
    published: &Store,
) -> Result<StateVersionV1, Box<DurableRSpaceTransactionError>> {
    let gate = Arc::new(RwLock::new(()));
    complete_uncontended(async {
        let permit = Arc::clone(&gate).write_owned().await;
        match owner.attach(attached) {
            Ok(attachment) => owner
                .checkpoint_view(permit, attachment, &gate, published, publication_limits())
                .await
                .map_err(Box::new),
            Err(refusal) => Err(Box::new(refusal)),
        }
    })
}

fn checkpoint(owner: &mut Owner, hot: &Store) -> StateVersionV1 {
    checkpoint_through(owner, hot, hot)
        .expect("the owner publishes and acknowledges the hot store's journal")
}

fn put_continuation_with_joins(hot: &Store, names: &[&str], body: &str) -> Option<bool> {
    let channels = group(names);
    let inserted = hot.put_continuation(&channels, continuation(names, body));
    for member in channels.iter().collect::<BTreeSet<_>>() {
        hot.put_join(member, &channels)
            .expect("the hot store takes every join");
    }
    inserted
}

fn remove_continuation_with_joins(hot: &Store, names: &[&str], index: i32) -> Option<()> {
    let channels = group(names);
    let removed = hot.remove_continuation(&channels, index);
    if removed.is_some() {
        for member in channels.iter().collect::<BTreeSet<_>>() {
            hot.remove_join(member, &channels)
                .expect("the hot store answers every join removal");
        }
    }
    removed
}

#[derive(Clone, Debug)]
struct BaseState {
    data: Vec<(usize, usize)>,
    continuations: Vec<(usize, usize)>,
}

fn base_state(data: &[(&str, &str)], continuations: &[(&[&str], &str)]) -> BaseState {
    BaseState {
        data: data
            .iter()
            .map(|&(name, value)| (index_of(&CHANNELS, name), index_of(&VALUES, value)))
            .collect(),
        continuations: continuations
            .iter()
            .map(|&(names, body)| (stored_group(names), index_of(&BODIES, body)))
            .collect(),
    }
}

fn seed_bases() -> [BaseState; 3] {
    [
        base_state(&[], &[]),
        base_state(&[("a", "v0"), ("a", "v0"), ("a", "v1"), ("b", "v2")], &[
            (&["a", "b"], "k0"),
            (&["a", "b"], "k1"),
            (&["b", "a"], "k0"),
            (&["b", "a"], "k1"),
            (&["a", "a"], "k1"),
            (&["c", "a", "b"], "k0"),
        ]),
        base_state(&[("c", "v2")], &[(&["b"], "k1")]),
    ]
}

struct PublishedBase {
    directory: TrialDirectory,
    genesis: CatalogSlotV1,
}

fn publish_base(base: &BaseState) -> PublishedBase {
    let directory = TrialDirectory::new("base");
    let genesis = create_empty_store_v1::<Profile>(directory.path(), recovery_limits())
        .expect("an empty trial directory receives the empty store");
    let view = recover(directory.path());
    let hot = eager_store(&view);
    let mut owner = owner_of(view, &hot);
    for &(channel_index, value_index) in &base.data {
        let name = CHANNELS[channel_index];
        hot.put_datum(&channel(name), datum(name, VALUES[value_index]));
    }
    for &(group_index, body_index) in &base.continuations {
        put_continuation_with_joins(&hot, STORED_GROUPS[group_index], BODIES[body_index]);
    }
    checkpoint(&mut owner, &hot);
    PublishedBase { directory, genesis }
}

fn store_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(root)
        .expect("list the trial store directory")
        .map(|entry| entry.expect("read an entry of the trial store directory"))
        .filter_map(|entry| match entry.file_name().into_string() {
            Ok(name) if name == WRITER_LOCK_FILE => None,
            Ok(name) => Some((name, std::fs::read(entry.path()).expect("read a store file"))),
            Err(name) => panic!("a trial store file name is not UTF-8: {name:?}"),
        })
        .collect()
}

fn twin_of(original: &TrialDirectory) -> TrialDirectory {
    let twin = TrialDirectory::new("twin");
    let files = store_files(original.path());
    for name in files.keys() {
        std::fs::copy(original.path().join(name), twin.path().join(name))
            .expect("copy a store file into the twin directory");
    }
    assert_eq!(
        store_files(twin.path()),
        files,
        "the twin directory holds the original's store files byte for byte"
    );
    twin
}

fn cache_of(eager: &Store, mask: CacheMask) -> Cache {
    let mut cached = Vec::with_capacity(
        usize::try_from(mask.0.count_ones()).expect("a count of probed keys fits usize"),
    );
    cached.extend(probed_keys().into_iter().filter(|key| mask.caches(key)));
    let (data, continuations, joins) =
        cached
            .iter()
            .fold((0, 0, 0), |(data, continuations, joins), key| match key {
                ReadKey::Data(_) => (data + 1, continuations, joins),
                ReadKey::Continuations(_) => (data, continuations + 1, joins),
                ReadKey::Joins(_) => (data, continuations, joins + 1),
            });
    let mut cache = Cache {
        continuations: HashMap::with_capacity(continuations),
        installed_continuations: HashMap::new(),
        data: HashMap::with_capacity(data),
        joins: HashMap::with_capacity(joins),
        installed_joins: HashMap::new(),
    };
    for key in cached {
        match key {
            ReadKey::Data(name) => {
                let records = eager.get_data(&name);
                cache.data.insert(name, records);
            }
            ReadKey::Continuations(channels) => {
                let records = eager.get_continuations(&channels);
                cache.continuations.insert(channels, records);
            }
            ReadKey::Joins(name) => {
                let records = eager.get_joins(&name);
                cache.joins.insert(name, records);
            }
        }
    }
    cache
}

#[derive(Clone, Copy, Debug)]
enum StoreOperation {
    ReadData {
        channel_index: usize,
    },
    PutDatum {
        channel_index: usize,
        value_index: usize,
    },
    RemoveDatum {
        channel_index: usize,
        selector: usize,
    },
    ReadContinuations {
        group_index: usize,
    },
    PutContinuation {
        group_index: usize,
        body_index: usize,
    },
    RemoveContinuation {
        group_index: usize,
        selector: usize,
    },
    ReadJoins {
        channel_index: usize,
    },
    PutJoin {
        group_index: usize,
        member_index: usize,
    },
    RemoveJoin {
        group_index: usize,
        member_index: usize,
    },
}

impl StoreOperation {
    fn read_data(name: &str) -> Self {
        Self::ReadData {
            channel_index: index_of(&CHANNELS, name),
        }
    }

    fn put_datum(name: &str, value: &str) -> Self {
        Self::PutDatum {
            channel_index: index_of(&CHANNELS, name),
            value_index: index_of(&VALUES, value),
        }
    }

    fn remove_datum(name: &str, selector: usize) -> Self {
        Self::RemoveDatum {
            channel_index: index_of(&CHANNELS, name),
            selector,
        }
    }

    fn read_continuations(names: &[&str]) -> Self {
        Self::ReadContinuations {
            group_index: stored_group(names),
        }
    }

    fn put_continuation(names: &[&str], body: &str) -> Self {
        Self::PutContinuation {
            group_index: stored_group(names),
            body_index: index_of(&BODIES, body),
        }
    }

    fn remove_continuation(names: &[&str], selector: usize) -> Self {
        Self::RemoveContinuation {
            group_index: stored_group(names),
            selector,
        }
    }

    fn read_joins(name: &str) -> Self {
        Self::ReadJoins {
            channel_index: index_of(&CHANNELS, name),
        }
    }

    fn put_join(names: &[&str], member: &str) -> Self {
        Self::PutJoin {
            group_index: stored_group(names),
            member_index: index_of(names, member),
        }
    }

    fn remove_join(names: &[&str], member: &str) -> Self {
        Self::RemoveJoin {
            group_index: stored_group(names),
            member_index: index_of(names, member),
        }
    }

    fn mutates(self) -> bool {
        match self {
            Self::ReadData { .. } | Self::ReadContinuations { .. } | Self::ReadJoins { .. } => {
                false
            }
            Self::PutDatum { .. } |
            Self::RemoveDatum { .. } |
            Self::PutContinuation { .. } |
            Self::RemoveContinuation { .. } |
            Self::PutJoin { .. } |
            Self::RemoveJoin { .. } => true,
        }
    }
}

#[derive(Debug, PartialEq)]
enum Answer {
    Data(Vec<Datum<String>>),
    Continuations(Vec<Continuation>),
    Joins(Vec<Vec<String>>),
    DatumPut,
    DatumRemoved(Result<(), RSpaceError>),
    ContinuationPut(Option<bool>),
    ContinuationRemoved(Option<()>),
    JoinChanged(Option<()>),
}

impl Answer {
    fn records(&self) -> usize {
        match self {
            Self::Data(records) => records.len(),
            Self::Continuations(records) => records.len(),
            Self::Joins(records) => records.len(),
            Self::DatumPut |
            Self::DatumRemoved(_) |
            Self::ContinuationPut(_) |
            Self::ContinuationRemoved(_) |
            Self::JoinChanged(_) => 0,
        }
    }
}

fn removal_index_on(oracle: &Store, operation: StoreOperation) -> i32 {
    let (length, selector) = match operation {
        StoreOperation::RemoveDatum {
            channel_index,
            selector,
        } => (oracle.get_data(&channel(CHANNELS[channel_index])).len(), selector),
        StoreOperation::RemoveContinuation {
            group_index,
            selector,
        } => (
            oracle
                .get_continuations(&group(STORED_GROUPS[group_index]))
                .len(),
            selector,
        ),
        StoreOperation::ReadData { .. } |
        StoreOperation::PutDatum { .. } |
        StoreOperation::ReadContinuations { .. } |
        StoreOperation::PutContinuation { .. } |
        StoreOperation::ReadJoins { .. } |
        StoreOperation::PutJoin { .. } |
        StoreOperation::RemoveJoin { .. } => (0, 0),
    };
    i32::try_from(selector.checked_rem(length).unwrap_or(0)).expect("a removal index fits i32")
}

fn apply_operation(hot: &Store, operation: StoreOperation, index: i32) -> Answer {
    match operation {
        StoreOperation::ReadData { channel_index } => {
            Answer::Data(hot.get_data(&channel(CHANNELS[channel_index])))
        }
        StoreOperation::PutDatum {
            channel_index,
            value_index,
        } => {
            let name = CHANNELS[channel_index];
            hot.put_datum(&channel(name), datum(name, VALUES[value_index]));
            Answer::DatumPut
        }
        StoreOperation::RemoveDatum { channel_index, .. } => {
            Answer::DatumRemoved(hot.remove_datum(&channel(CHANNELS[channel_index]), index))
        }
        StoreOperation::ReadContinuations { group_index } => {
            Answer::Continuations(hot.get_continuations(&group(STORED_GROUPS[group_index])))
        }
        StoreOperation::PutContinuation {
            group_index,
            body_index,
        } => Answer::ContinuationPut(put_continuation_with_joins(
            hot,
            STORED_GROUPS[group_index],
            BODIES[body_index],
        )),
        StoreOperation::RemoveContinuation { group_index, .. } => Answer::ContinuationRemoved(
            remove_continuation_with_joins(hot, STORED_GROUPS[group_index], index),
        ),
        StoreOperation::ReadJoins { channel_index } => {
            Answer::Joins(hot.get_joins(&channel(CHANNELS[channel_index])))
        }
        StoreOperation::PutJoin {
            group_index,
            member_index,
        } => {
            let names = STORED_GROUPS[group_index];
            Answer::JoinChanged(
                hot.put_join(&channel(names[member_index % names.len()]), &group(names)),
            )
        }
        StoreOperation::RemoveJoin {
            group_index,
            member_index,
        } => {
            let names = STORED_GROUPS[group_index];
            Answer::JoinChanged(
                hot.remove_join(&channel(names[member_index % names.len()]), &group(names)),
            )
        }
    }
}

fn read_key(hot: &Store, key: &ReadKey) -> Answer {
    match key {
        ReadKey::Data(name) => Answer::Data(hot.get_data(name)),
        ReadKey::Continuations(channels) => Answer::Continuations(hot.get_continuations(channels)),
        ReadKey::Joins(name) => Answer::Joins(hot.get_joins(name)),
    }
}

fn join_occurrences(hot: &Store) -> usize {
    CHANNELS
        .into_iter()
        .map(|name| hot.get_joins(&channel(name)).len())
        .sum()
}

fn multiset<T: Eq + Hash>(records: &[T]) -> HashMap<&T, usize> {
    let mut counts = HashMap::with_capacity(records.len());
    for record in records {
        *counts.entry(record).or_insert(0) += 1;
    }
    counts
}

#[derive(Clone, Copy)]
enum RecordOrder {
    Compared,
    Ignored,
}

fn check_same_records<T: Debug + Eq + Hash>(
    left: &[T],
    right: &[T],
    key: &ReadKey,
    order: RecordOrder,
) -> Result<(), TestCaseError> {
    prop_assert_eq!(
        multiset(left),
        multiset(right),
        "the stores hold different records under {:?}",
        key
    );
    if let RecordOrder::Compared = order {
        prop_assert_eq!(left, right, "the stores order the records under {:?} differently", key);
    }
    Ok(())
}

fn check_equal_contents(
    left: &Store,
    right: &Store,
    order: RecordOrder,
) -> Result<(), TestCaseError> {
    for key in probed_keys() {
        match &key {
            ReadKey::Data(name) => {
                check_same_records(&left.get_data(name), &right.get_data(name), &key, order)?
            }
            ReadKey::Continuations(channels) => check_same_records(
                &left.get_continuations(channels),
                &right.get_continuations(channels),
                &key,
                order,
            )?,
            ReadKey::Joins(name) => {
                check_same_records(&left.get_joins(name), &right.get_joins(name), &key, order)?
            }
        }
    }
    Ok(())
}

fn same_paths<V: Clone + Send + Sync + Unpin + PartialEq>(
    left: &Option<PathMap<V>>,
    right: &Option<PathMap<V>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.iter().eq(right.iter()),
        (None, None) => true,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

fn same_pending(left: &PendingDeltas, right: &PendingDeltas) -> bool {
    left.data == right.data &&
        left.continuations == right.continuations &&
        left.joins == right.joins &&
        same_paths(&left.continuation_content, &right.continuation_content) &&
        same_paths(&left.continuation_source_bytes, &right.continuation_source_bytes)
}

fn same_observation(left: &PendingMutationObservation, right: &PendingMutationObservation) -> bool {
    same_pending(&left.pending, &right.pending) &&
        left.data_counts.iter().eq(right.data_counts.iter()) &&
        left.join_counts.iter().eq(right.join_counts.iter())
}

fn journal_is_empty(pending: &PendingDeltas) -> bool {
    pending
        .data
        .as_ref()
        .is_some_and(|journal| journal.is_empty()) &&
        pending
            .continuations
            .as_ref()
            .is_some_and(|journal| journal.is_empty()) &&
        pending
            .continuation_content
            .as_ref()
            .is_some_and(|journal| journal.is_empty()) &&
        pending
            .continuation_source_bytes
            .as_ref()
            .is_some_and(|journal| journal.is_empty()) &&
        pending
            .joins
            .as_ref()
            .is_some_and(|journal| journal.is_empty())
}

#[derive(Clone, Debug)]
struct AgreementCase {
    base: BaseState,
    cache: CacheMask,
    operations: Vec<StoreOperation>,
}

fn seed_agreement_cases() -> [AgreementCase; 3] {
    let [empty, rich, small] = seed_bases();
    [
        AgreementCase {
            base: empty,
            cache: CacheMask::NONE,
            operations: vec![
                StoreOperation::put_datum("a", "v0"),
                StoreOperation::put_datum("a", "v0"),
                StoreOperation::remove_datum("a", 0),
                StoreOperation::put_continuation(&["a", "b"], "k0"),
                StoreOperation::read_joins("a"),
                StoreOperation::remove_continuation(&["a", "b"], 0),
                StoreOperation::read_data("b"),
            ],
        },
        AgreementCase {
            base: rich,
            cache: CacheMask::of(&[
                ReadKey::Data(channel("a")),
                ReadKey::Continuations(group(&["a", "b"])),
            ]),
            operations: vec![
                StoreOperation::remove_datum("b", 0),
                StoreOperation::put_datum("a", "v2"),
                StoreOperation::read_data("a"),
                StoreOperation::remove_continuation(&["b", "a"], 1),
                StoreOperation::remove_continuation(&["a", "a"], 0),
                StoreOperation::put_continuation(&["c", "a", "b"], "k1"),
                StoreOperation::read_continuations(&["a", "b"]),
                StoreOperation::remove_join(&["b"], "b"),
                StoreOperation::read_joins("c"),
                StoreOperation::read_data("c"),
                StoreOperation::put_join(&["a"], "a"),
            ],
        },
        AgreementCase {
            base: small,
            cache: CacheMask::EVERY_KEY,
            operations: vec![
                StoreOperation::read_data("c"),
                StoreOperation::remove_datum("c", 0),
                StoreOperation::read_continuations(&["b"]),
                StoreOperation::put_continuation(&["b"], "k0"),
                StoreOperation::remove_join(&["b"], "b"),
                StoreOperation::read_joins("b"),
            ],
        },
    ]
}

fn base_states() -> impl Strategy<Value = BaseState> {
    (
        prop::collection::vec((0..CHANNELS.len(), 0..VALUES.len()), 0..8),
        prop::collection::vec((0..STORED_GROUPS.len(), 0..BODIES.len()), 0..6),
    )
        .prop_map(|(data, continuations)| BaseState {
            data,
            continuations,
        })
}

fn store_operations() -> impl Strategy<Value = StoreOperation> {
    let channels = 0..CHANNELS.len();
    let groups = 0..STORED_GROUPS.len();
    prop_oneof![
        channels
            .clone()
            .prop_map(|channel_index| StoreOperation::ReadData { channel_index }),
        (channels.clone(), 0..VALUES.len()).prop_map(|(channel_index, value_index)| {
            StoreOperation::PutDatum {
                channel_index,
                value_index,
            }
        }),
        (channels.clone(), 0..8_usize).prop_map(|(channel_index, selector)| {
            StoreOperation::RemoveDatum {
                channel_index,
                selector,
            }
        }),
        groups
            .clone()
            .prop_map(|group_index| StoreOperation::ReadContinuations { group_index }),
        (groups.clone(), 0..BODIES.len()).prop_map(|(group_index, body_index)| {
            StoreOperation::PutContinuation {
                group_index,
                body_index,
            }
        }),
        (groups.clone(), 0..8_usize).prop_map(|(group_index, selector)| {
            StoreOperation::RemoveContinuation {
                group_index,
                selector,
            }
        }),
        channels.prop_map(|channel_index| StoreOperation::ReadJoins { channel_index }),
        (groups.clone(), 0..3_usize).prop_map(|(group_index, member_index)| {
            StoreOperation::PutJoin {
                group_index,
                member_index,
            }
        }),
        (groups, 0..3_usize).prop_map(|(group_index, member_index)| {
            StoreOperation::RemoveJoin {
                group_index,
                member_index,
            }
        }),
    ]
}

fn agreement_cases() -> impl Strategy<Value = AgreementCase> {
    (
        base_states(),
        (0..=CacheMask::EVERY_KEY.0).prop_map(CacheMask),
        prop::collection::vec(store_operations(), 0..24),
    )
        .prop_map(|(base, cache, operations)| AgreementCase {
            base,
            cache,
            operations,
        })
}

#[derive(Clone, Copy, Debug, Default)]
struct AgreementCoverage {
    cases: u32,
    durable_fills: u32,
    mutation_fills: u32,
    cached_records: u32,
    nonempty_journals: u32,
    datum_removals: u32,
    continuation_removals: u32,
    join_removals: u32,
    both_orders_of_one_pair: u32,
}

impl AgreementCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            durable_fills,
            mutation_fills,
            cached_records,
            nonempty_journals,
            datum_removals,
            continuation_removals,
            join_removals,
            both_orders_of_one_pair,
        } = other;
        self.cases += cases;
        self.durable_fills += durable_fills;
        self.mutation_fills += mutation_fills;
        self.cached_records += cached_records;
        self.nonempty_journals += nonempty_journals;
        self.datum_removals += datum_removals;
        self.continuation_removals += continuation_removals;
        self.join_removals += join_removals;
        self.both_orders_of_one_pair += both_orders_of_one_pair;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            durable_fills,
            mutation_fills,
            cached_records,
            nonempty_journals,
            datum_removals,
            continuation_removals,
            join_removals,
            both_orders_of_one_pair,
        } = *self;
        assert!(
            [
                cases,
                durable_fills,
                mutation_fills,
                cached_records,
                nonempty_journals,
                datum_removals,
                continuation_removals,
                join_removals,
                both_orders_of_one_pair,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of lazy bootstrap behaviour never occurred: {self:?}"
        );
    }
}

fn check_store_agreement(
    case: &AgreementCase,
    choice: ReaderChoice,
) -> Result<AgreementCoverage, TestCaseError> {
    let published = publish_base(&case.base);
    let view = recover(published.directory.path());
    let eager = eager_store(&view);
    let cache = cache_of(&eager, case.cache);
    let cached_records = cache.data.values().any(|records| !records.is_empty()) ||
        cache
            .continuations
            .values()
            .any(|records| !records.is_empty()) ||
        cache.joins.values().any(|records| !records.is_empty());
    let ledger = Arc::new(ReaderLedger::default());
    let lazy = lazy_store(&view, cache, choice, &ledger);
    prop_assert!(
        ledger.reads().is_empty(),
        "constructing the lazy store read the durable view: {:?}",
        ledger.reads()
    );
    let mut coverage = AgreementCoverage {
        cases: 1,
        cached_records: u32::from(cached_records),
        ..AgreementCoverage::default()
    };
    for &operation in &case.operations {
        let index = removal_index_on(&eager, operation);
        let joins_before = join_occurrences(&eager);
        let fills_before = ledger.reads_with_records();
        let expected = apply_operation(&eager, operation, index);
        let observed = apply_operation(&lazy, operation, index);
        prop_assert_eq!(&observed, &expected, "the stores answered {:?} differently", operation);
        coverage.mutation_fills +=
            u32::from(operation.mutates() && ledger.reads_with_records() > fills_before);
        coverage.datum_removals += u32::from(matches!(expected, Answer::DatumRemoved(Ok(()))));
        coverage.continuation_removals +=
            u32::from(matches!(expected, Answer::ContinuationRemoved(Some(()))));
        coverage.join_removals += u32::from(join_occurrences(&eager) < joins_before);
    }
    check_equal_contents(&lazy, &eager, RecordOrder::Compared)?;
    let lazy_journal = lazy
        .pending_deltas()
        .expect("the lazy store exposes its journals");
    let eager_journal = eager
        .pending_deltas()
        .expect("the eager store exposes its journals");
    prop_assert!(
        same_pending(&lazy_journal, &eager_journal),
        "the journals differ: {:?} against {:?}",
        lazy_journal,
        eager_journal
    );
    let base = view.catalog();
    prop_assert_eq!(lazy.bind_journal_authority(base), Ok(()));
    let lazy_capture = lazy
        .capture_pending_journal(base)
        .expect("the bound lazy store captures its journal");
    let eager_capture = eager
        .capture_pending_journal(base)
        .expect("the eager store captures its journal");
    prop_assert_eq!(lazy_capture.base, eager_capture.base);
    prop_assert_eq!(lazy_capture.version.revision, eager_capture.version.revision);
    prop_assert!(
        same_observation(&lazy_capture.observation, &eager_capture.observation),
        "the captured observations differ: {:?} against {:?}",
        lazy_capture.observation,
        eager_capture.observation
    );
    let reads = ledger.reads();
    prop_assert!(
        reads.iter().all(|(key, _)| !case.cache.caches(key)),
        "a cached key was read from the durable view: {:?}",
        reads
    );
    coverage.durable_fills = u32::from(reads.iter().any(|(_, records)| *records > 0));
    coverage.nonempty_journals = u32::from(!journal_is_empty(&eager_journal));
    coverage.both_orders_of_one_pair = u32::from(
        !eager.get_continuations(&group(&["a", "b"])).is_empty() &&
            !eager.get_continuations(&group(&["b", "a"])).is_empty(),
    );
    Ok(coverage)
}

#[test]
fn lazy_and_eager_stores_agree_on_contents_reads_and_journals() {
    let mut seeded = AgreementCoverage::default();
    for case in seed_agreement_cases() {
        seeded.merge(
            check_store_agreement(&case, ReaderChoice::Durable)
                .expect("a seeded case satisfies VR-04"),
        );
    }
    println!("lazy durable bootstrap seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(AgreementCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: AGREEMENT_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&agreement_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_store_agreement(&case, ReaderChoice::Durable)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated cases satisfy VR-04");
    assert!(generated.get().cases >= AGREEMENT_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("lazy durable bootstrap: {merged:?}");
    merged.assert_complete();
}

#[test]
fn the_closed_reader_control_breaks_the_lazy_and_eager_agreement() {
    let agreements: Vec<bool> = seed_agreement_cases()
        .iter()
        .map(|case| check_store_agreement(case, ReaderChoice::Closed).is_ok())
        .collect();
    assert_eq!(
        agreements,
        [true, false, true],
        "the closed reader must agree exactly where no uncached base key holds a record"
    );
}

#[test]
fn lazy_bootstrap_makes_no_reader_call_during_construction() {
    let caches = [
        CacheMask::NONE,
        CacheMask::of(&[
            ReadKey::Data(channel("a")),
            ReadKey::Continuations(group(&["a", "b"])),
            ReadKey::Continuations(group(&["c"])),
            ReadKey::Joins(channel("b")),
        ]),
        CacheMask::EVERY_KEY,
    ];
    let mut uncached_reads_with_records = 0_u32;
    let mut cached_reads_with_records = 0_u32;
    for base in seed_bases() {
        let published = publish_base(&base);
        let view = recover(published.directory.path());
        let eager = eager_store(&view);
        for mask in caches {
            let ledger = Arc::new(ReaderLedger::default());
            let lazy = lazy_store(&view, cache_of(&eager, mask), ReaderChoice::Durable, &ledger);
            assert!(
                ledger.reads().is_empty(),
                "constructing the lazy store over {base:?} with {mask:?} read the durable view: \
                 {:?}",
                ledger.reads()
            );
            for key in probed_keys() {
                let calls_before = ledger.reads().len();
                let answer = read_key(&lazy, &key);
                assert_eq!(
                    answer,
                    read_key(&eager, &key),
                    "the lazy store read {key:?} differently"
                );
                let calls = ledger.reads().split_off(calls_before);
                match (mask.caches(&key), calls.as_slice()) {
                    (true, []) => cached_reads_with_records += u32::from(answer.records() > 0),
                    (false, [(read, records)]) if *read == key => {
                        uncached_reads_with_records += u32::from(*records > 0)
                    }
                    (cached, calls) => {
                        panic!("reading {key:?}, cached: {cached}, made the reader calls {calls:?}")
                    }
                }
            }
        }
    }
    assert!(uncached_reads_with_records > 0, "no uncached read returned a durable record");
    assert!(cached_reads_with_records > 0, "no cached read returned a record");
}

#[derive(Clone, Copy, Debug, Default)]
struct CheckpointCoverage {
    cases: u32,
    published: u32,
    unchanged: u32,
    durable_fills: u32,
}

impl CheckpointCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            published,
            unchanged,
            durable_fills,
        } = other;
        self.cases += cases;
        self.published += published;
        self.unchanged += unchanged;
        self.durable_fills += durable_fills;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            published,
            unchanged,
            durable_fills,
        } = *self;
        assert!(
            [cases, published, unchanged, durable_fills]
                .into_iter()
                .all(|count| count > 0),
            "a class of bound lazy checkpoint never occurred: {self:?}"
        );
    }
}

fn seed_checkpoint_cases() -> [AgreementCase; 3] {
    let [_, publishing, _] = seed_agreement_cases();
    let [_, rich, _] = seed_bases();
    let removed_and_put_back = AgreementCase {
        base: base_state(&[], &[(&["b", "a"], "k1")]),
        cache: CacheMask::NONE,
        operations: vec![
            StoreOperation::remove_continuation(&["b", "a"], 0),
            StoreOperation::put_continuation(&["b", "a"], "k1"),
        ],
    };
    [publishing, removed_and_put_back, AgreementCase {
        base: rich,
        cache: CacheMask::NONE,
        operations: vec![
            StoreOperation::read_data("a"),
            StoreOperation::put_continuation(&["a", "b"], "k0"),
            StoreOperation::remove_join(&["a", "b"], "b"),
            StoreOperation::read_joins("c"),
            StoreOperation::read_continuations(&["c", "a", "b"]),
        ],
    }]
}

fn check_bound_checkpoint(case: &AgreementCase) -> Result<CheckpointCoverage, TestCaseError> {
    let published = publish_base(&case.base);
    let twin = twin_of(&published.directory);
    let lazy_view = recover(published.directory.path());
    let eager_view = recover(twin.path());
    let base = lazy_view.catalog();
    prop_assert_eq!(
        eager_view.catalog(),
        base,
        "the twin directory recovered another catalog slot"
    );
    let eager = eager_store(&eager_view);
    let ledger = Arc::new(ReaderLedger::default());
    let lazy = lazy_store(&lazy_view, cache_of(&eager, case.cache), ReaderChoice::Durable, &ledger);
    for &operation in &case.operations {
        let index = removal_index_on(&eager, operation);
        let expected = apply_operation(&eager, operation, index);
        prop_assert_eq!(
            apply_operation(&lazy, operation, index),
            expected,
            "the stores answered {:?} differently",
            operation
        );
    }
    let lazy_view = match lazy_view.into_rspace_authority(&lazy) {
        Ok(_) => {
            return Err(TestCaseError::fail(
                "the owner took a lazy store whose journal is detached",
            ));
        }
        Err(refusal) => {
            let (view, error) = *refusal;
            prop_assert!(
                matches!(
                    error,
                    DurableRSpaceTransactionError::Journal(RSpaceError::JournalConflict(_))
                ),
                "the detached lazy store was refused with {:?}",
                error
            );
            prop_assert_eq!(view.catalog(), base);
            view
        }
    };
    prop_assert_eq!(lazy.bind_journal_authority(base), Ok(()));
    let mut lazy_owner = match lazy_view.into_rspace_authority(&lazy) {
        Ok(owner) => owner,
        Err(refusal) => {
            return Err(TestCaseError::fail(format!(
                "the owner refused the bound lazy store: {:?}",
                refusal.1
            )));
        }
    };
    let mut eager_owner = owner_of(eager_view, &eager);
    let lazy_state = checkpoint(&mut lazy_owner, &lazy);
    let eager_state = checkpoint(&mut eager_owner, &eager);
    prop_assert_eq!(lazy_state, eager_state);
    let lazy_successor = lazy_owner
        .attach(&lazy)
        .expect("the lazy store stays attached after its checkpoint")
        .base();
    let eager_successor = eager_owner
        .attach(&eager)
        .expect("the eager store stays attached after its checkpoint")
        .base();
    prop_assert_eq!(lazy_successor, eager_successor);
    prop_assert_eq!(lazy_successor.state(), lazy_state);
    prop_assert!(journal_is_empty(
        &lazy
            .pending_deltas()
            .expect("the lazy store exposes its journals")
    ));
    prop_assert!(journal_is_empty(
        &eager
            .pending_deltas()
            .expect("the eager store exposes its journals")
    ));
    check_equal_contents(&lazy, &eager, RecordOrder::Compared)?;
    drop(lazy_owner);
    drop(eager_owner);
    prop_assert_eq!(
        store_files(published.directory.path()),
        store_files(twin.path()),
        "the lazy and eager checkpoints left different store files"
    );
    Ok(CheckpointCoverage {
        cases: 1,
        published: u32::from(lazy_state.generation() > base.state().generation()),
        unchanged: u32::from(lazy_state == base.state()),
        durable_fills: u32::from(ledger.reads_with_records() > 0),
    })
}

#[test]
fn a_bound_lazy_store_checkpoints_to_the_eager_successor_slot() {
    let mut seeded = CheckpointCoverage::default();
    for case in seed_checkpoint_cases() {
        seeded.merge(check_bound_checkpoint(&case).expect("a seeded case satisfies VR-06"));
    }
    println!("bound lazy checkpoint seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(CheckpointCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: CHECKPOINT_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&agreement_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_bound_checkpoint(&case)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated cases satisfy VR-06");
    assert!(generated.get().cases >= CHECKPOINT_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("bound lazy checkpoint: {merged:?}");
    merged.assert_complete();
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct SyntheticManifest {
    digest_seed: u8,
    encoded_len: u64,
}

impl SyntheticManifest {
    fn reference(self) -> ObjectRefV1 {
        ObjectRefV1::new(ObjectDigest::of_canonical_bytes(&[self.digest_seed]), self.encoded_len)
            .expect("a synthetic manifest is at least one object envelope header long")
    }
}

fn synthetic_slot(manifest: SyntheticManifest, generation: u64) -> CatalogSlotV1 {
    let reference = manifest.reference();
    let address = LogPackAddress::from_pack(
        PackId::GENESIS,
        PACK_HEADER_LEN_V1,
        TRANSACTION_COMMIT_RECORD_LEN_V1,
    )
    .expect("the first commit record of the genesis pack has an address");
    let pack =
        CatalogPackStateV1::new(1, 1, address.end(), 1, INDEX_HEADER_LEN_V1, reference, 64, 64)
            .expect("a synthetic pack state is well formed");
    CatalogSlotV1::new(
        StoreId::from_array([7; 16]),
        1,
        StateVersionV1::new(generation, reference),
        SemanticProfileId::from_array(*b"RSpace.Test.V1\0\0"),
        PhysicalDigest::of_bytes(b"synthetic"),
        LogPackSequence::FIRST,
        address,
        pack,
    )
    .expect("a synthetic catalog slot is well formed")
}

fn synthetic_lazy_store(registry: &GenerationLeaseRegistryV1) -> Store {
    Arc::new(
        HotStoreInstances::create_from_durable_reader(
            Box::new(ClosedDurableHistoryReader),
            Cache::default(),
            DurableJournalBaseV1 {
                manifest: BASE_MANIFEST.reference(),
                registry: registry.clone(),
            },
        )
        .expect("an empty cache loads into the lazy store"),
    )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JournalStart {
    Detached,
    BoundToBase,
}

#[derive(Clone, Debug)]
struct BindCase {
    start: JournalStart,
    operations: Vec<StoreOperation>,
    invalidate: bool,
    slot_manifest: SyntheticManifest,
    slot_generation: u64,
}

fn seed_bind_cases() -> [BindCase; 5] {
    let journal = vec![
        StoreOperation::put_datum("a", "v0"),
        StoreOperation::put_continuation(&["a", "b"], "k0"),
    ];
    [
        BindCase {
            start: JournalStart::Detached,
            operations: journal.clone(),
            invalidate: false,
            slot_manifest: BASE_MANIFEST,
            slot_generation: 1,
        },
        BindCase {
            start: JournalStart::Detached,
            operations: journal.clone(),
            invalidate: false,
            slot_manifest: SyntheticManifest {
                digest_seed: BASE_MANIFEST.digest_seed,
                encoded_len: BASE_MANIFEST.encoded_len + 1,
            },
            slot_generation: 0,
        },
        BindCase {
            start: JournalStart::Detached,
            operations: Vec::new(),
            invalidate: true,
            slot_manifest: BASE_MANIFEST,
            slot_generation: 0,
        },
        BindCase {
            start: JournalStart::BoundToBase,
            operations: journal.clone(),
            invalidate: false,
            slot_manifest: BASE_MANIFEST,
            slot_generation: 1,
        },
        BindCase {
            start: JournalStart::BoundToBase,
            operations: journal,
            invalidate: false,
            slot_manifest: SyntheticManifest {
                digest_seed: BASE_MANIFEST.digest_seed + 1,
                encoded_len: BASE_MANIFEST.encoded_len,
            },
            slot_generation: 1,
        },
    ]
}

fn bind_cases() -> impl Strategy<Value = BindCase> {
    (
        prop_oneof![Just(JournalStart::Detached), Just(JournalStart::BoundToBase)],
        prop::collection::vec(store_operations(), 0..12),
        prop::bool::weighted(0.25),
        0..3_u8,
        32..34_u64,
        0..2_u64,
    )
        .prop_map(
            |(start, operations, invalidate, digest_seed, encoded_len, slot_generation)| BindCase {
                start,
                operations,
                invalidate,
                slot_manifest: SyntheticManifest {
                    digest_seed,
                    encoded_len,
                },
                slot_generation,
            },
        )
}

#[derive(Clone, Copy, Debug, Default)]
struct BindCoverage {
    cases: u32,
    accepted: u32,
    accepted_at_another_generation: u32,
    refused_with_journal: u32,
    refused_same_digest_other_length: u32,
    refused_invalidated: u32,
    rebound_to_another_slot_of_the_same_state: u32,
    rebind_refused_for_another_state: u32,
}

impl BindCoverage {
    fn merge(&mut self, other: Self) {
        let Self {
            cases,
            accepted,
            accepted_at_another_generation,
            refused_with_journal,
            refused_same_digest_other_length,
            refused_invalidated,
            rebound_to_another_slot_of_the_same_state,
            rebind_refused_for_another_state,
        } = other;
        self.cases += cases;
        self.accepted += accepted;
        self.accepted_at_another_generation += accepted_at_another_generation;
        self.refused_with_journal += refused_with_journal;
        self.refused_same_digest_other_length += refused_same_digest_other_length;
        self.refused_invalidated += refused_invalidated;
        self.rebound_to_another_slot_of_the_same_state += rebound_to_another_slot_of_the_same_state;
        self.rebind_refused_for_another_state += rebind_refused_for_another_state;
    }

    fn assert_complete(&self) {
        let Self {
            cases,
            accepted,
            accepted_at_another_generation,
            refused_with_journal,
            refused_same_digest_other_length,
            refused_invalidated,
            rebound_to_another_slot_of_the_same_state,
            rebind_refused_for_another_state,
        } = *self;
        assert!(
            [
                cases,
                accepted,
                accepted_at_another_generation,
                refused_with_journal,
                refused_same_digest_other_length,
                refused_invalidated,
                rebound_to_another_slot_of_the_same_state,
                rebind_refused_for_another_state,
            ]
            .into_iter()
            .all(|count| count > 0),
            "a class of journal binding never occurred: {self:?}"
        );
    }
}

fn check_unchanged_by_refused_bind(
    subject: &Store,
    twin: &Store,
    slot: CatalogSlotV1,
    base: CatalogSlotV1,
    registry: &GenerationLeaseRegistryV1,
) -> Result<(), TestCaseError> {
    let subject_journal = subject
        .pending_deltas()
        .expect("the lazy store exposes its journals");
    let twin_journal = twin
        .pending_deltas()
        .expect("the lazy store exposes its journals");
    prop_assert!(
        same_pending(&subject_journal, &twin_journal),
        "a refused bind changed the journal: {:?} against {:?}",
        subject_journal,
        twin_journal
    );
    for probe in [slot, base] {
        prop_assert_eq!(
            subject.capture_pending_journal(probe).err(),
            twin.capture_pending_journal(probe).err()
        );
        prop_assert_eq!(
            subject.validate_journal_authority(probe, registry),
            twin.validate_journal_authority(probe, registry)
        );
    }
    prop_assert_eq!(subject.bind_journal_authority(base), twin.bind_journal_authority(base));
    match (subject.capture_pending_journal(base), twin.capture_pending_journal(base)) {
        (Ok(subject_capture), Ok(twin_capture)) => {
            prop_assert_eq!(subject_capture.base, twin_capture.base);
            prop_assert_eq!(subject_capture.version.revision, twin_capture.version.revision);
            prop_assert!(
                same_observation(&subject_capture.observation, &twin_capture.observation),
                "a refused bind changed the captured observation: {:?} against {:?}",
                subject_capture.observation,
                twin_capture.observation
            );
        }
        (Err(subject_refusal), Err(twin_refusal)) => {
            prop_assert_eq!(subject_refusal, twin_refusal)
        }
        (subject_capture, twin_capture) => {
            return Err(TestCaseError::fail(format!(
                "the twin stores captured differently: {:?} against {:?}",
                subject_capture.err(),
                twin_capture.err()
            )));
        }
    }
    Ok(())
}

fn check_rebind_kept_the_journal(
    previous: &CapturedHotJournalV1,
    current: &CapturedHotJournalV1,
    rebind: &str,
) -> Result<(), TestCaseError> {
    prop_assert!(
        previous.version.matches(&current.version),
        "{} changed the journal's version from revision {} to revision {}",
        rebind,
        previous.version.revision,
        current.version.revision
    );
    prop_assert!(
        same_observation(&previous.observation, &current.observation),
        "{} changed the journal's contents: {:?} against {:?}",
        rebind,
        previous.observation,
        current.observation
    );
    Ok(())
}

fn check_bind(case: &BindCase) -> Result<BindCoverage, TestCaseError> {
    let registry = GenerationLeaseRegistryV1::default();
    let subject = synthetic_lazy_store(&registry);
    let twin = synthetic_lazy_store(&registry);
    let base = synthetic_slot(BASE_MANIFEST, 0);
    if let JournalStart::BoundToBase = case.start {
        prop_assert_eq!(subject.bind_journal_authority(base), Ok(()));
        prop_assert_eq!(twin.bind_journal_authority(base), Ok(()));
    }
    for &operation in &case.operations {
        let index = removal_index_on(&twin, operation);
        let expected = apply_operation(&twin, operation, index);
        prop_assert_eq!(
            apply_operation(&subject, operation, index),
            expected,
            "the twin stores answered {:?} differently",
            operation
        );
    }
    if case.invalidate {
        subject.clear();
        twin.clear();
    }
    let journal = subject
        .pending_deltas()
        .expect("the lazy store exposes its journals");
    let slot = synthetic_slot(case.slot_manifest, case.slot_generation);
    let bound_before = matches!((case.start, case.invalidate), (JournalStart::BoundToBase, false));
    let capture_before_bind = subject.capture_pending_journal(base);
    prop_assert_eq!(
        capture_before_bind.is_ok(),
        bound_before,
        "before the bind the journal's capture at the base slot disagrees with its start {:?}",
        case.start
    );
    prop_assert_eq!(
        subject.capture_pending_journal(slot).is_ok(),
        bound_before && slot == base,
        "before the bind the journal was captured at {:?} against its start {:?}",
        slot,
        case.start
    );
    let acceptable = case.slot_manifest == BASE_MANIFEST && !case.invalidate;
    match (subject.bind_journal_authority(slot), acceptable) {
        (Ok(()), true) => {
            let captured = subject.capture_pending_journal(slot).map_err(|error| {
                TestCaseError::fail(format!("the bound journal refused its capture: {error:?}"))
            })?;
            prop_assert_eq!(captured.base, slot);
            prop_assert_eq!(subject.validate_journal_authority(slot, &registry), Ok(()));
            prop_assert!(
                slot == base || subject.capture_pending_journal(base).is_err(),
                "a journal bound to {:?} was captured at {:?}",
                slot,
                base
            );
            prop_assert_eq!(
                twin.capture_pending_journal(slot).is_ok(),
                bound_before && slot == base,
                "binding one store changed the binding of its twin"
            );
            prop_assert!(
                same_pending(
                    &subject
                        .pending_deltas()
                        .expect("the lazy store exposes its journals"),
                    &journal
                ),
                "an accepted bind changed the journal"
            );
            if let Ok(previous) = &capture_before_bind {
                check_rebind_kept_the_journal(
                    previous,
                    &captured,
                    "re-binding a bound journal to another slot of the same state",
                )?;
            }
            Ok(BindCoverage {
                cases: 1,
                accepted: 1,
                accepted_at_another_generation: u32::from(case.slot_generation != 0),
                rebound_to_another_slot_of_the_same_state: u32::from(bound_before && slot != base),
                ..BindCoverage::default()
            })
        }
        (Err(RSpaceError::JournalConflict(_)), false) => {
            if let Ok(previous) = &capture_before_bind {
                let current = subject.capture_pending_journal(base).map_err(|error| {
                    TestCaseError::fail(format!(
                        "a refused re-bind unbound the journal from the base slot: {error:?}"
                    ))
                })?;
                prop_assert_eq!(current.base, base);
                check_rebind_kept_the_journal(
                    previous,
                    &current,
                    "a refused re-bind to a slot of another state",
                )?;
            }
            check_unchanged_by_refused_bind(&subject, &twin, slot, base, &registry)?;
            Ok(BindCoverage {
                cases: 1,
                refused_with_journal: u32::from(!case.invalidate && !journal_is_empty(&journal)),
                refused_same_digest_other_length: u32::from(
                    !case.invalidate &&
                        case.slot_manifest.digest_seed == BASE_MANIFEST.digest_seed &&
                        case.slot_manifest.encoded_len != BASE_MANIFEST.encoded_len,
                ),
                refused_invalidated: u32::from(case.invalidate),
                rebind_refused_for_another_state: u32::from(bound_before),
                ..BindCoverage::default()
            })
        }
        (bind, acceptable) => Err(TestCaseError::fail(format!(
            "binding the journal to {slot:?} returned {bind:?} where acceptance was {acceptable}"
        ))),
    }
}

#[test]
fn detached_journals_cannot_be_published_and_mismatched_binds_change_nothing() {
    let [_, rich, _] = seed_bases();
    let published = publish_base(&rich);
    let view = recover(published.directory.path());
    let base = view.catalog();
    assert_ne!(
        published.genesis.state().manifest(),
        base.state().manifest(),
        "the base state differs from the genesis state"
    );
    let registry = view.generation_leases();
    let ledger = Arc::new(ReaderLedger::default());
    let lazy = lazy_store(&view, Cache::default(), ReaderChoice::Durable, &ledger);
    lazy.put_datum(&channel("a"), datum("a", "v2"));
    put_continuation_with_joins(&lazy, &["a", "a"], "k0");
    let journal = lazy
        .pending_deltas()
        .expect("the lazy store exposes its journals");
    assert!(!journal_is_empty(&journal), "the detached journal holds the two mutations");
    assert!(ledger.reads_with_records() > 0, "the mutations filled keys from the durable view");
    let files = store_files(published.directory.path());
    let detached = lazy.capture_pending_journal(base).err();
    assert!(matches!(detached, Some(RSpaceError::JournalConflict(_))), "{detached:?}");
    let validation = lazy.validate_journal_authority(base, &registry);
    assert!(matches!(validation, Err(RSpaceError::JournalConflict(_))), "{validation:?}");
    let commit = lazy.commit_pending_deltas(&journal);
    assert!(matches!(commit, Err(RSpaceError::JournalConflict(_))), "{commit:?}");
    let view = match view.into_rspace_authority(&lazy) {
        Ok(_) => panic!("the owner took a lazy store whose journal is detached"),
        Err(refusal) => {
            let (view, error) = *refusal;
            assert!(
                matches!(
                    error,
                    DurableRSpaceTransactionError::Journal(RSpaceError::JournalConflict(_))
                ),
                "{error:?}"
            );
            view
        }
    };
    assert_eq!(view.catalog(), base);
    let eager = eager_store(&view);
    let mut owner = owner_of(view, &eager);
    match checkpoint_through(&mut owner, &eager, &lazy) {
        Ok(state) => panic!("the owner published a detached journal as {state:?}"),
        Err(refusal) => assert!(
            matches!(
                *refusal,
                DurableRSpaceTransactionError::Journal(RSpaceError::JournalConflict(_))
            ),
            "{refusal:?}"
        ),
    }
    assert_eq!(store_files(published.directory.path()), files);
    assert!(same_pending(
        &lazy
            .pending_deltas()
            .expect("the lazy store exposes its journals"),
        &journal
    ));
    let mismatched = lazy.bind_journal_authority(published.genesis);
    assert!(
        matches!(mismatched, Err(RSpaceError::JournalConflict(_))),
        "binding the detached journal to the genesis slot of another state returned {mismatched:?}"
    );
    assert!(same_pending(
        &lazy
            .pending_deltas()
            .expect("the lazy store exposes its journals"),
        &journal
    ));
    assert_eq!(lazy.capture_pending_journal(base).err(), detached);
    assert!(matches!(
        lazy.capture_pending_journal(published.genesis),
        Err(RSpaceError::JournalConflict(_))
    ));
    assert_eq!(lazy.bind_journal_authority(base), Ok(()));
    let successor = checkpoint(&mut owner, &lazy);
    assert_eq!(successor.generation(), base.state().generation() + 1);
    assert!(journal_is_empty(
        &lazy
            .pending_deltas()
            .expect("the lazy store exposes its journals")
    ));
    let successor_slot = owner
        .attach(&lazy)
        .expect("the owner attaches the lazy store whose journal it acknowledged")
        .base();
    assert_eq!(successor_slot.state(), successor);
    let acknowledged = lazy
        .capture_pending_journal(successor_slot)
        .expect("the acknowledged journal is bound to the successor slot");
    let rebind = lazy.bind_journal_authority(base);
    assert!(
        matches!(rebind, Err(RSpaceError::JournalConflict(_))),
        "re-binding the acknowledged journal to the base slot of another state returned {rebind:?}"
    );
    let after_refusal = lazy
        .capture_pending_journal(successor_slot)
        .expect("a refused re-bind keeps the journal bound to the successor slot");
    check_rebind_kept_the_journal(
        &acknowledged,
        &after_refusal,
        "a refused re-bind of the acknowledged journal",
    )
    .expect("a refused re-bind changes neither the journal's version nor its contents");
    assert_eq!(
        owner
            .attach(&lazy)
            .expect("the owner still attaches the lazy store after the refused re-bind")
            .base(),
        successor_slot
    );
    drop(owner);
    let reopened = recover(published.directory.path());
    assert_eq!(reopened.state(), successor);
    check_equal_contents(&lazy, &eager_store(&reopened), RecordOrder::Ignored)
        .expect("the published successor holds the lazy store's records");

    let mut seeded = BindCoverage::default();
    for case in seed_bind_cases() {
        seeded.merge(check_bind(&case).expect("a seeded bind satisfies VR-08"));
    }
    println!("journal binding seeds: {seeded:?}");
    seeded.assert_complete();
    let generated = Cell::new(BindCoverage::default());
    prop::test_runner::TestRunner::new(ProptestConfig {
        cases: BIND_CASES,
        source_file: Some(file!()),
        ..ProptestConfig::default()
    })
    .run(&bind_cases(), |case| {
        let mut total = generated.get();
        total.merge(check_bind(&case)?);
        generated.set(total);
        Ok(())
    })
    .expect("generated binds satisfy VR-08");
    assert!(generated.get().cases >= BIND_CASES);
    let mut merged = seeded;
    merged.merge(generated.get());
    println!("journal binding: {merged:?}");
    merged.assert_complete();
}
