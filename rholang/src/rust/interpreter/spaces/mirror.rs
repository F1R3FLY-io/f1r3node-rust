//! The mirror journal of the VectorDB spaces with a mirror (P6 step 7b, design v3 4.9; plan
//! decisions 58 and 66).
//!
//! A VectorDB space that its factory creates with `mirror` keeps a copy of its records in the
//! ChromaDB mirror of each node. The mirror is a hint index for `rho:vectordb:ann`: it never
//! decides a COMM, and nothing in consensus reads it.
//!
//! Each plan of such a space captures its writes, and the runtime collects them in the journal
//! of the evaluation:
//!
//! * [`MirrorCapture`] records each store that a plan stages on a data channel of the space.
//!   After the commit, [`MirrorCapture::writes`] turns the receipt of the transaction into its
//!   net writes. Capture has no meter, no error path and no await, so it never changes the
//!   outcome of the transaction.
//! * [`MirrorJournal`] keeps the last write of each record id of each collection, in deploy
//!   order. The runtime publishes it when the final result of the deploy has no errors, and it
//!   drops it otherwise (M1, decision 66).
//!
//! The net write of a record id in one transaction follows its first and its last event:
//!
//! | First event | Last event | Meaning | Write |
//! |---|---|---|---|
//! | remove | store | Rule E: a peek removes the record and the transaction stores it again | none |
//! | store | store | a new record | upsert |
//! | remove | remove | a record leaves the collection | delete |
//! | store | remove | a new record leaves in the same transaction | none |

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use models::rhoapi::expr::ExprInstance;
use models::rhoapi::{ListParWithRandom, Par};
use models::rust::space_factory_urn::SpaceInner;
use rspace_plus_plus::rspace::hashing::blake2b256_hash::Blake2b256Hash;
use rspace_plus_plus::rspace::hashing::stable_hash_provider;
use rspace_plus_plus::rspace::space_store_handle::SpaceOpReceipt;

use super::agent::ParSpaceReceipt;
use super::disciplines::vectordb::{vdb_config, ID_LEN};
use super::factory::constructor::family_of;
use super::factory::defaults::MIRROR_PARAM;
use super::namespace::{self, CollectionKey, ReifiedId, SpaceId};
use super::record::{self, sole_expr};
use super::types::config::SpaceConfig;
use crate::rust::interpreter::reified::costs::ScanMeter;
use crate::rust::interpreter::reified::vectordb_services::{channel_digest, collection_name};
use crate::rust::interpreter::tensor::exact::Metric;
use crate::rust::interpreter::tensor::num;

/// The distance of a mirror collection, the `hnsw:space` of ChromaDB. Only the metrics that the
/// mirror supports take a mirror (gate 9 of the factory).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirrorMetric {
    Cosine,
    InnerProduct,
    L2,
}

impl MirrorMetric {
    /// The mirror distance of `metric`, or `None` for a metric that the mirror does not support.
    pub const fn of(metric: Metric) -> Option<Self> {
        match metric {
            Metric::Cosine => Some(MirrorMetric::Cosine),
            Metric::Dot => Some(MirrorMetric::InnerProduct),
            Metric::Euclidean => Some(MirrorMetric::L2),
            Metric::Manhattan | Metric::Hamming | Metric::Jaccard => None,
        }
    }

    /// The `hnsw:space` value of the distance.
    pub const fn space(self) -> &'static str {
        match self {
            MirrorMetric::Cosine => "cosine",
            MirrorMetric::InnerProduct => "ip",
            MirrorMetric::L2 => "l2",
        }
    }
}

/// One record of the mirror.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorRecord {
    /// The record id: the hash of the storage produce of the record, as `ids=` takes it.
    pub id: [u8; ID_LEN],
    /// The embedding of the record, rounded to `f64` and then to `f32`.
    pub embedding: Vec<f32>,
    /// The channel digest of the collection of the record.
    pub channel: String,
    /// True for a persistent record.
    pub persistent: bool,
    /// The first element of the payload, when it is a string.
    pub document: Option<String>,
}

/// One write of the mirror.
#[derive(Clone, Debug, PartialEq)]
pub enum MirrorWrite {
    Upsert(MirrorRecord),
    Delete([u8; ID_LEN]),
}

impl MirrorWrite {
    /// The record id of the write.
    pub fn id(&self) -> &[u8; ID_LEN] {
        match self {
            MirrorWrite::Upsert(record) => &record.id,
            MirrorWrite::Delete(id) => id,
        }
    }
}

/// The net writes of one transaction to the mirror collection of its space.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorWrites {
    /// The mirror collection of the space.
    pub collection: String,
    /// The distance of the collection.
    pub metric: MirrorMetric,
    /// The writes, in record-id order.
    pub writes: Vec<MirrorWrite>,
}

/// The writes of one collection, ready to publish.
#[derive(Clone, Debug, PartialEq)]
pub struct MirrorBatch {
    /// The mirror collection.
    pub collection: String,
    /// The distance of the collection.
    pub metric: MirrorMetric,
    /// The records to upsert, in record-id order.
    pub upserts: Vec<MirrorRecord>,
    /// The record ids to delete, in order.
    pub deletes: Vec<[u8; ID_LEN]>,
}

/// One store that a plan stages on a data channel of a space with a mirror.
#[derive(Clone, Debug)]
struct CapturedStore {
    record: ListParWithRandom,
    persistent: bool,
    channel: String,
}

/// The capture state of a plan of a VectorDB space with a mirror.
#[derive(Clone, Debug)]
struct ActiveCapture {
    space: SpaceId,
    collection: String,
    metric: MirrorMetric,
    /// The captured stores, by their staged index.
    stores: BTreeMap<usize, CapturedStore>,
}

/// The mirror capture of a plan. It is inactive for every space without a mirror.
#[derive(Clone, Debug, Default)]
pub struct MirrorCapture {
    active: Option<ActiveCapture>,
}

/// The kind of an event of a record id in a transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventKind {
    Store,
    Remove,
}

impl MirrorCapture {
    /// The inactive capture.
    pub fn inactive() -> Self { Self::default() }

    /// The capture of a plan over the space of `config`: active for a VectorDB space with the
    /// param `mirror` and a metric that the mirror supports. The config was validated when the
    /// factory created the space, so its reads here are not metered.
    pub fn of(config: &SpaceConfig) -> Self {
        if !config.params.contains_key(MIRROR_PARAM) {
            return Self::inactive();
        }
        let unmetered = ScanMeter::new(None);
        let active = match (family_of(config), vdb_config(config, &unmetered)) {
            (Ok(family), Ok(vdb)) if family.inner == SpaceInner::VectorDb => {
                MirrorMetric::of(vdb.metric).map(|metric| ActiveCapture {
                    space: config.space_id,
                    collection: collection_name(&config.space_id),
                    metric,
                    stores: BTreeMap::new(),
                })
            }
            _ => None,
        };
        Self { active }
    }

    /// True when the capture records the stores of its plan.
    pub fn is_active(&self) -> bool { self.active.is_some() }

    /// Records the store of `record` on `channel` at the staged index `index`. A channel that is
    /// not a data channel of the space is not mirrored.
    pub fn store(
        &mut self,
        index: usize,
        channel: &Par,
        record: &ListParWithRandom,
        persistent: bool,
    ) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let Some(channel) = data_channel_digest(channel, &active.space) else {
            return;
        };
        active.stores.insert(index, CapturedStore {
            record: record.clone(),
            persistent,
            channel,
        });
    }

    /// The net writes of the committed transaction of `receipt`, or `None` for an inactive
    /// capture or a transaction without writes.
    pub fn writes(&self, receipt: &ParSpaceReceipt) -> Option<MirrorWrites> {
        let active = self.active.as_ref()?;
        let mut events: Vec<([u8; ID_LEN], EventKind, Option<&CapturedStore>)> = Vec::new();
        for (index, op) in receipt.ops.iter().enumerate() {
            match op {
                SpaceOpReceipt::Stored { produce } => {
                    if let (Some(store), Some(id)) =
                        (active.stores.get(&index), record_id(&produce.hash))
                    {
                        events.push((id, EventKind::Store, Some(store)));
                    }
                }
                SpaceOpReceipt::Fired {
                    comm,
                    results,
                    produce: incoming,
                    ..
                } => {
                    let channels: BTreeSet<Blake2b256Hash> = results
                        .iter()
                        .filter(|result| is_data_channel(&result.channel, &active.space))
                        .map(|result| stable_hash_provider::hash(&result.channel))
                        .collect();
                    for produce in &comm.produces {
                        let incoming = incoming
                            .as_ref()
                            .is_some_and(|incoming| incoming.hash == produce.hash);
                        if produce.persistent
                            || incoming
                            || !channels.contains(&produce.channel_hash)
                        {
                            continue;
                        }
                        if let Some(id) = record_id(&produce.hash) {
                            events.push((id, EventKind::Remove, None));
                        }
                    }
                }
                SpaceOpReceipt::Parked { .. } => {}
            }
        }
        let mut first: BTreeMap<[u8; ID_LEN], EventKind> = BTreeMap::new();
        let mut last: BTreeMap<[u8; ID_LEN], (EventKind, Option<&CapturedStore>)> = BTreeMap::new();
        for (id, kind, store) in events {
            first.entry(id).or_insert(kind);
            last.insert(id, (kind, store));
        }
        let writes: Vec<MirrorWrite> = last
            .into_iter()
            .filter_map(|(id, (kind, store))| match (first.get(&id), kind) {
                (Some(EventKind::Store), EventKind::Store) => store
                    .and_then(|store| mirror_record(id, store))
                    .map(MirrorWrite::Upsert),
                (Some(EventKind::Remove), EventKind::Remove) => Some(MirrorWrite::Delete(id)),
                _ => None,
            })
            .collect();
        match writes.is_empty() {
            true => None,
            false => Some(MirrorWrites {
                collection: active.collection.clone(),
                metric: active.metric,
                writes,
            }),
        }
    }
}

/// The 32-byte record id of a produce hash.
fn record_id(hash: &Blake2b256Hash) -> Option<[u8; ID_LEN]> {
    <[u8; ID_LEN]>::try_from(hash.0.as_slice()).ok()
}

/// True when `channel` is a data channel of the space `space`.
fn is_data_channel(channel: &Par, space: &SpaceId) -> bool {
    matches!(namespace::reified_id_of(channel), Some(ReifiedId::Data(native, _)) if native.sid == *space)
}

/// The channel digest of the collection of the data channel `channel` of the space `space`.
fn data_channel_digest(channel: &Par, space: &SpaceId) -> Option<String> {
    match namespace::reified_id_of(channel) {
        Some(ReifiedId::Data(native, key)) if native.sid == *space => {
            CollectionKey::from_bytes(key.to_vec()).map(|key| channel_digest(&key))
        }
        _ => None,
    }
}

/// The mirror record of the store `store` with the id `id`, or `None` for a record without an
/// embedding or with an embedding that is not finite as `f32` values.
fn mirror_record(id: [u8; ID_LEN], store: &CapturedStore) -> Option<MirrorRecord> {
    let decoded = record::unwrap_vdb_datum(&store.record).ok()?;
    let embedding = f32_embedding(decoded.embedding.as_ref()?)?;
    let document =
        decoded
            .datum
            .pars
            .first()
            .and_then(|par| match (sole_expr(par), par.connective_used) {
                (Some(ExprInstance::GString(text)), false) => Some(text.clone()),
                _ => None,
            });
    Some(MirrorRecord {
        id,
        embedding,
        channel: store.channel.clone(),
        persistent: store.persistent,
        document,
    })
}

/// The `f32` values of the numeric vector `par`, each rounded to `f64` and then to `f32`, or
/// `None` when a value is not a number or not finite as an `f32` value.
fn f32_embedding(par: &Par) -> Option<Vec<f32>> {
    let unmetered = ScanMeter::new(None);
    let vector = num::vector_of(par, None, &unmetered).ok()?;
    let values = vector.rationals(&unmetered).ok()?;
    let mut embedding = Vec::with_capacity(values.len());
    for value in values.iter() {
        let element = num::round_to_f64(value, &unmetered).ok()? as f32;
        if !element.is_finite() {
            return None;
        }
        embedding.push(element);
    }
    Some(embedding)
}

/// The writes of one collection in the journal.
#[derive(Debug)]
struct CollectionJournal {
    metric: MirrorMetric,
    writes: BTreeMap<[u8; ID_LEN], MirrorWrite>,
}

/// The mirror journal of one evaluation. It keeps the last write of each record id of each
/// collection.
#[derive(Debug, Default)]
pub struct MirrorJournal {
    collections: Mutex<BTreeMap<String, CollectionJournal>>,
}

impl MirrorJournal {
    /// An empty journal.
    pub fn new() -> Self { Self::default() }

    /// Appends the net writes of one transaction: the last write of each record id wins.
    pub fn record(&self, writes: MirrorWrites) {
        let mut collections = self
            .collections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let collection =
            collections
                .entry(writes.collection)
                .or_insert_with(|| CollectionJournal {
                    metric: writes.metric,
                    writes: BTreeMap::new(),
                });
        for write in writes.writes {
            collection.writes.insert(*write.id(), write);
        }
    }

    /// Removes and returns the writes of the journal, one batch for each collection.
    pub fn take(&self) -> Vec<MirrorBatch> {
        let collections = std::mem::take(
            &mut *self
                .collections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        collections
            .into_iter()
            .map(|(collection, journal)| {
                let mut upserts = Vec::new();
                let mut deletes = Vec::new();
                for write in journal.writes.into_values() {
                    match write {
                        MirrorWrite::Upsert(record) => upserts.push(record),
                        MirrorWrite::Delete(id) => deletes.push(id),
                    }
                }
                MirrorBatch {
                    collection,
                    metric: journal.metric,
                    upserts,
                    deletes,
                }
            })
            .collect()
    }

    /// Drops the writes of the journal.
    pub fn clear(&self) { drop(self.take()); }

    /// True when the journal holds no write.
    pub fn is_empty(&self) -> bool {
        self.collections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use models::rhoapi::expr::ExprInstance;
    use models::rhoapi::{BindPattern, EList, ListParWithRandom, Par, TaggedContinuation};
    use rspace_plus_plus::rspace::rspace_interface::{ContResult, RSpaceResult};
    use rspace_plus_plus::rspace::space_store_handle::{SpaceOpReceipt, SpaceTxnReceipt};
    use rspace_plus_plus::rspace::trace::event::{Consume, Produce, COMM};

    use super::*;
    use crate::rust::interpreter::spaces::namespace::{gprivate, Durability, NativeSpace, SID_LEN};
    use crate::rust::interpreter::spaces::record::{expr_par, wrap_vdb_datum};
    use crate::rust::interpreter::spaces::types::qualifier::SpaceQualifier;

    fn sid() -> SpaceId { [7; SID_LEN] }

    fn space() -> NativeSpace {
        NativeSpace {
            sid: sid(),
            durability: Durability::Durable,
        }
    }

    fn int(value: i64) -> Par { expr_par(ExprInstance::GInt(value)) }

    fn string(value: &str) -> Par { expr_par(ExprInstance::GString(value.to_string())) }

    fn double(value: f64) -> Par { expr_par(ExprInstance::GDouble(value.to_bits())) }

    fn list(values: Vec<Par>) -> Par {
        expr_par(ExprInstance::EListBody(EList {
            ps: values,
            locally_free: Vec::new(),
            connective_used: false,
            remainder: None,
        }))
    }

    fn vector(values: &[f64]) -> Par { list(values.iter().copied().map(double).collect()) }

    /// The config of a VectorDB space of two dimensions, with the params `extra`.
    fn config(qualifier: SpaceQualifier, extra: &[(&str, Par)]) -> SpaceConfig {
        let mut config = SpaceConfig::new(sid(), "hashmap", "vectordb", qualifier);
        config.params.insert("capacity".to_string(), int(8));
        config.params.insert("dimensions".to_string(), int(2));
        for (key, value) in extra {
            config.params.insert((*key).to_string(), value.clone());
        }
        config
    }

    fn mirrored() -> SpaceConfig {
        config(SpaceQualifier::Default, &[("mirror", string("chroma"))])
    }

    fn key(byte: u8) -> CollectionKey {
        CollectionKey::from_bytes(vec![byte, byte]).expect("a key")
    }

    /// The data channel of the collection `byte` of the space.
    fn data(byte: u8) -> Par { gprivate(namespace::data_channel(space(), &key(byte))) }

    /// A VectorDB record of `payload` with the embedding `embedding` and the random state `salt`.
    fn record(payload: Par, embedding: Option<&[f64]>, salt: u8) -> ListParWithRandom {
        let datum = ListParWithRandom {
            pars: vec![payload],
            random_state: vec![salt; 8],
            ..ListParWithRandom::default()
        };
        let embedding = embedding.map(vector);
        wrap_vdb_datum(0, embedding.as_ref(), &datum).expect("a VectorDB record")
    }

    fn id(produce: &Produce) -> [u8; ID_LEN] { record_id(&produce.hash).expect("a 32-byte hash") }

    /// The receipt of a COMM that removes `removed` from `channel`, and whose produce side is
    /// `incoming` when the fire is produce-triggered.
    fn fired(
        channel: &Par,
        removed: Vec<(Produce, ListParWithRandom)>,
        incoming: Option<Produce>,
    ) -> SpaceOpReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        let patterns = vec![BindPattern::default()];
        let continuation = TaggedContinuation::default();
        let channels = vec![channel.clone()];
        SpaceOpReceipt::Fired {
            comm: COMM {
                consume: Consume::create(&channels, &patterns, &continuation, false),
                produces: removed.iter().map(|(produce, _)| produce.clone()).collect(),
                peeks: BTreeSet::new(),
                times_repeated: BTreeMap::new(),
            },
            continuation: ContResult {
                continuation,
                persistent: false,
                channels,
                patterns,
                peek: false,
            },
            results: removed
                .iter()
                .map(|(produce, record)| RSpaceResult {
                    channel: channel.clone(),
                    matched_datum: record.clone(),
                    removed_datum: record.clone(),
                    persistent: produce.persistent,
                })
                .collect(),
            produce: incoming,
        }
    }

    fn stored(
        channel: &Par,
        record: &ListParWithRandom,
        persistent: bool,
    ) -> SpaceOpReceipt<Par, BindPattern, ListParWithRandom, TaggedContinuation> {
        SpaceOpReceipt::Stored {
            produce: Produce::create(channel, record, persistent),
        }
    }

    #[test]
    fn mirror_capture_is_active_only_for_a_mirrored_vectordb_space() {
        assert!(MirrorCapture::of(&mirrored()).is_active());
        for config in [
            config(SpaceQualifier::Default, &[]),
            SpaceConfig::new(sid(), "hashmap", "queue", SpaceQualifier::Default),
            config(SpaceQualifier::Default, &[
                ("mirror", string("chroma")),
                ("metric", string("manhattan")),
            ]),
        ] {
            let mut capture = MirrorCapture::of(&config);
            assert!(!capture.is_active(), "{config:?}");
            let record = record(vector(&[1.0, 0.0]), Some(&[1.0, 0.0]), 1);
            capture.store(0, &data(1), &record, false);
            let receipt = SpaceTxnReceipt {
                ops: vec![stored(&data(1), &record, false)],
            };
            assert_eq!(capture.writes(&receipt), None, "{config:?}");
        }
    }

    /// The net write of each record id follows its first and its last event in the transaction,
    /// and only the records of the data channels of the space are mirrored.
    #[test]
    fn mirror_capture_net_writes_follow_the_first_and_the_last_event() {
        let channel = data(1);
        let counter = gprivate(namespace::collection_counter(space(), &key(1)));
        let new = record(string("doc"), Some(&[0.6, 0.8]), 1);
        let gone = record(vector(&[1.0, 0.0]), Some(&[1.0, 0.0]), 2);
        let peeked = record(vector(&[0.0, 1.0]), Some(&[0.0, 1.0]), 3);
        let transient = record(vector(&[1.0, 1.0]), Some(&[1.0, 1.0]), 4);
        let persistent = record(vector(&[0.5, 0.5]), Some(&[0.5, 0.5]), 5);
        let incoming = record(vector(&[0.25, 0.75]), Some(&[0.25, 0.75]), 6);
        let plain = record(string("no vector"), None, 7);
        let counted = record(int(3), None, 8);
        let produce = |record: &ListParWithRandom, persistent: bool| {
            Produce::create(&channel, record, persistent)
        };
        let mut capture = MirrorCapture::of(&mirrored());
        // Index 0: a new record. Index 1: a removal. Indices 2 and 3: Rule E, a removal and the
        // store again. Indices 4 and 5: a new record that leaves in the same transaction. Index
        // 6: a persistent record that a COMM matches. Index 7: a produce-triggered fire. Index
        // 8: a record without an embedding. Index 9: a counter of the collection.
        capture.store(0, &channel, &new, false);
        capture.store(3, &channel, &peeked, false);
        capture.store(4, &channel, &transient, false);
        capture.store(8, &channel, &plain, false);
        capture.store(9, &counter, &counted, false);
        let receipt = SpaceTxnReceipt {
            ops: vec![
                stored(&channel, &new, false),
                fired(&channel, vec![(produce(&gone, false), gone.clone())], None),
                fired(
                    &channel,
                    vec![(produce(&peeked, false), peeked.clone())],
                    None,
                ),
                stored(&channel, &peeked, false),
                stored(&channel, &transient, false),
                fired(
                    &channel,
                    vec![(produce(&transient, false), transient.clone())],
                    None,
                ),
                fired(
                    &channel,
                    vec![(produce(&persistent, true), persistent.clone())],
                    None,
                ),
                fired(
                    &channel,
                    vec![(produce(&incoming, false), incoming.clone())],
                    Some(produce(&incoming, false)),
                ),
                stored(&channel, &plain, false),
                stored(&counter, &counted, false),
            ],
        };
        let writes = capture.writes(&receipt).expect("the transaction writes");
        assert_eq!(writes.collection, collection_name(&sid()));
        assert_eq!(writes.metric, MirrorMetric::Cosine);
        let mut expected = vec![
            MirrorWrite::Upsert(MirrorRecord {
                id: id(&produce(&new, false)),
                embedding: vec![0.6, 0.8],
                channel: channel_digest(&key(1)),
                persistent: false,
                document: Some("doc".to_string()),
            }),
            MirrorWrite::Delete(id(&produce(&gone, false))),
        ];
        expected.sort_by_key(|write| *write.id());
        assert_eq!(writes.writes, expected);
    }

    #[test]
    fn mirror_capture_skips_a_non_data_channel_and_another_space() {
        let mut capture = MirrorCapture::of(&mirrored());
        let other = NativeSpace {
            sid: [9; SID_LEN],
            durability: Durability::Durable,
        };
        let foreign = gprivate(namespace::data_channel(other, &key(1)));
        let record = record(vector(&[1.0, 0.0]), Some(&[1.0, 0.0]), 1);
        capture.store(0, &foreign, &record, false);
        capture.store(1, &string("public"), &record, false);
        let receipt = SpaceTxnReceipt {
            ops: vec![
                stored(&foreign, &record, false),
                stored(&string("public"), &record, false),
                fired(
                    &foreign,
                    vec![(Produce::create(&foreign, &record, false), record.clone())],
                    None,
                ),
            ],
        };
        assert_eq!(capture.writes(&receipt), None);
    }

    /// The embedding of a mirror record is each value rounded to `f64` and then to `f32`. A
    /// value outside the `f32` range makes the record ineligible for the mirror.
    #[test]
    fn mirror_record_rounds_its_embedding_and_keeps_a_string_document() {
        let channel = data(2);
        let tenth = record(list(vec![string("doc"), int(1)]), Some(&[0.1, 2.0]), 1);
        let huge = record(string("huge"), Some(&[1e300, 0.0]), 2);
        let mut capture = MirrorCapture::of(&config(SpaceQualifier::Default, &[
            ("mirror", string("chroma")),
            ("metric", string("euclidean")),
        ]));
        capture.store(0, &channel, &tenth, true);
        capture.store(1, &channel, &huge, false);
        let receipt = SpaceTxnReceipt {
            ops: vec![
                stored(&channel, &tenth, true),
                stored(&channel, &huge, false),
            ],
        };
        let writes = capture.writes(&receipt).expect("the transaction writes");
        assert_eq!(writes.metric, MirrorMetric::L2);
        let [MirrorWrite::Upsert(written)] = writes.writes.as_slice() else {
            panic!("one upsert: {:?}", writes.writes);
        };
        assert_eq!(written.embedding, vec![0.1f64 as f32, 2.0]);
        assert!(written.persistent);
        assert_eq!(
            written.document, None,
            "the payload is a list, not a string"
        );
    }

    #[test]
    fn mirror_metric_maps_the_metrics_that_chroma_indexes() {
        assert_eq!(
            MirrorMetric::of(Metric::Cosine).map(MirrorMetric::space),
            Some("cosine")
        );
        assert_eq!(
            MirrorMetric::of(Metric::Dot).map(MirrorMetric::space),
            Some("ip")
        );
        assert_eq!(
            MirrorMetric::of(Metric::Euclidean).map(MirrorMetric::space),
            Some("l2")
        );
        for metric in [Metric::Manhattan, Metric::Hamming, Metric::Jaccard] {
            assert_eq!(MirrorMetric::of(metric), None, "{metric:?}");
        }
    }

    fn upsert(byte: u8) -> MirrorWrite {
        MirrorWrite::Upsert(MirrorRecord {
            id: [byte; ID_LEN],
            embedding: vec![f32::from(byte), 0.0],
            channel: "c".to_string(),
            persistent: false,
            document: None,
        })
    }

    fn writes(collection: &str, writes: Vec<MirrorWrite>) -> MirrorWrites {
        MirrorWrites {
            collection: collection.to_string(),
            metric: MirrorMetric::Cosine,
            writes,
        }
    }

    /// The journal keeps the last write of each record id of each collection, and it gives one
    /// batch for each collection, in record-id order.
    #[test]
    fn mirror_journal_keeps_the_last_write_of_each_record() {
        let journal = MirrorJournal::new();
        assert!(journal.is_empty());
        journal.record(writes("rrs-b", vec![upsert(3), upsert(1)]));
        journal.record(writes("rrs-a", vec![upsert(2)]));
        journal.record(writes("rrs-b", vec![
            MirrorWrite::Delete([1; ID_LEN]),
            upsert(4),
        ]));
        journal.record(writes("rrs-b", vec![upsert(3)]));
        assert!(!journal.is_empty());
        let MirrorWrite::Upsert(two) = upsert(2) else {
            unreachable!("an upsert")
        };
        let MirrorWrite::Upsert(three) = upsert(3) else {
            unreachable!("an upsert")
        };
        let MirrorWrite::Upsert(four) = upsert(4) else {
            unreachable!("an upsert")
        };
        assert_eq!(journal.take(), vec![
            MirrorBatch {
                collection: "rrs-a".to_string(),
                metric: MirrorMetric::Cosine,
                upserts: vec![two],
                deletes: Vec::new(),
            },
            MirrorBatch {
                collection: "rrs-b".to_string(),
                metric: MirrorMetric::Cosine,
                upserts: vec![three, four],
                deletes: vec![[1; ID_LEN]],
            },
        ]);
        assert!(journal.is_empty(), "take empties the journal");
        journal.record(writes("rrs-a", vec![upsert(5)]));
        journal.clear();
        assert!(journal.is_empty());
        assert_eq!(journal.take(), Vec::new());
    }
}
