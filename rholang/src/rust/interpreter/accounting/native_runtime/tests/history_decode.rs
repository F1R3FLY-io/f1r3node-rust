//! D-S2 (DR-95): History-mode decoding of the RSpace history records that
//! hold rhoapi terms.

use std::cell::Cell;
use std::sync::Arc;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    cost_signature, BindPattern, CostAuthority, CostRegion, CostSignature, CostStack,
    ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::hashing::native_source::{channels_hash_from_keys, GroupKeys};
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::native_reader::{
    decode_history_record, decode_record, NativeLeafKind, NativeReadCharge, NativeReadMeter,
};
use rspace_plus_plus::rspace::hot_store_action::{
    HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins, NativeExportAction,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::de::DeserializeOwned;
use serde::Serialize;
use shared::rust::closed_decode::ClosedDecode;

use crate::rust::interpreter::accounting::native_runtime::clone_backing::tests::{measured, term};

/// Sums the reserved backing bytes without allocating.
struct Backing(Cell<usize>);

impl NativeReadMeter for Backing {
    type Error = ();

    fn reserve(&self, charge: NativeReadCharge) -> Result<(), ()> {
        self.0
            .set(self.0.get().checked_add(charge.backing_bytes).ok_or(())?);
        Ok(())
    }
}

/// Decodes the history bytes of `value` in History mode and in the legacy
/// mode. Returns the History reservation, the legacy reservation and the
/// bytes that the History decode allocated.
fn decode_both<T>(value: &T) -> (usize, usize, usize)
where T: Serialize + DeserializeOwned + ClosedDecode {
    let bytes = bincode::serialize(value).expect("history record encodes");
    let history = Backing(Cell::new(0));
    let (decoded, allocated) = measured(|| decode_history_record::<T, _>(&bytes, &history));
    let decoded = decoded.expect("History mode decodes the record");
    assert_eq!(
        bincode::serialize(&decoded).expect("decoded record encodes"),
        bytes
    );
    let legacy = Backing(Cell::new(0));
    let reference = decode_record::<T, _>(&bytes, &legacy).expect("legacy mode decodes the record");
    assert_eq!(
        bincode::serialize(&reference).expect("reference record encodes"),
        bytes
    );
    (history.0.get(), legacy.0.get(), allocated)
}

fn pattern() -> impl Strategy<Value = BindPattern> {
    (
        prop::collection::vec(term(), 0..3),
        prop::option::of(any::<i32>()),
        any::<i32>(),
    )
        .prop_map(|(patterns, remainder, free_count)| BindPattern {
            patterns,
            remainder: remainder.map(|index| Var {
                var_instance: Some(VarInstance::FreeVar(index)),
            }),
            free_count,
        })
}

fn signature() -> impl Strategy<Value = CostSignature> {
    term().prop_map(|par| CostSignature {
        value: Some(cost_signature::Value::Quote(par)),
    })
}

fn authority() -> impl Strategy<Value = Option<CostAuthority>> {
    prop::option::of(
        prop::collection::vec(
            (prop::collection::vec(any::<u8>(), 0..33), signature()).prop_map(
                |(instance_id, signature)| CostRegion {
                    instance_id,
                    signature: Some(signature),
                },
            ),
            0..3,
        )
        .prop_map(|regions| CostAuthority { regions }),
    )
}

fn continuation() -> impl Strategy<Value = TaggedContinuation> {
    let body = prop_oneof![
        (term(), prop::collection::vec(any::<u8>(), 0..64)).prop_map(|(body, random_state)| {
            TaggedCont::ParBody(ParWithRandom {
                body: Some(body),
                random_state,
            })
        }),
        any::<i64>().prop_map(TaggedCont::ScalaBodyRef),
    ];
    (prop::option::of(term()), authority(), body).prop_map(|(guard, cost_authority, body)| {
        TaggedContinuation {
            guard,
            cost_authority,
            tagged_cont: Some(body),
        }
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// D-S2 (DR-95): for generated waiting continuations, data and joined
    /// channel lists, History mode decodes the encoded value, reserves backing
    /// for every byte that the decode allocates, and reserves no more than the
    /// legacy rule.
    #[test]
    fn history_records_cover_actual_allocations(
        channels in prop::collection::vec(term(), 1..3),
        patterns in prop::collection::vec(pattern(), 1..3),
        continuation in continuation(),
        peeks in prop::collection::btree_set(any::<i32>(), 0..8),
        data in prop::collection::vec(term(), 0..3),
        random_state in prop::collection::vec(any::<u8>(), 0..64),
        cost_authority in authority(),
        cost_stack in prop::option::of(prop::collection::vec(signature(), 0..3)),
        persist in any::<bool>(),
    ) {
        let waiting = WaitingContinuation::create(&channels, &patterns, &continuation, persist, peeks);
        let datum = Datum::create(
            &channels[0],
            ListParWithRandom {
                pars: data.clone(),
                random_state,
                cost_authority,
                cost_stack: cost_stack.map(|cells| CostStack { cells }),
            },
            persist,
        );
        for (history, legacy, allocated) in [decode_both(&waiting), decode_both(&datum), decode_both(&data)] {
            prop_assert!(allocated <= history, "allocated {} reserved {}", allocated, history);
            prop_assert!(history <= legacy, "history {} legacy {}", history, legacy);
        }
    }
}

type History =
    dyn HistoryRepository<Par, BindPattern, ListParWithRandom, TaggedContinuation> + Send + Sync;
type Waiting = WaitingContinuation<BindPattern, TaggedContinuation>;

/// The stored rows of the leaf of `projection`, read the way a cold fill
/// reads them.
fn stored_rows(history: &History, kind: NativeLeafKind, projection: &[u8; 32]) -> Vec<Vec<u8>> {
    let root: [u8; 32] = history
        .root()
        .0
        .as_slice()
        .try_into()
        .expect("a 32-byte root");
    history
        .native_history_reader(root)
        .with_records(kind, projection, &Backing(Cell::new(0)), |rows| {
            Ok(rows.iter().map(<[u8]>::to_vec).collect())
        })
        .expect("the stored leaf reads")
        .expect("the leaf is stored")
}

/// Decodes rows in History mode, as a cold fill does.
fn decoded<T: DeserializeOwned + ClosedDecode>(rows: &[Vec<u8>]) -> Vec<T> {
    rows.iter()
        .map(|row| {
            decode_history_record::<T, _>(row, &Backing(Cell::new(0))).expect("the row decodes")
        })
        .collect()
}

fn encoded<T: Serialize>(values: &[T]) -> Vec<Vec<u8>> {
    values
        .iter()
        .map(|value| bincode::serialize(value).expect("the value encodes"))
        .collect()
}

fn sorted(rows: &[Vec<u8>]) -> bool { rows.windows(2).all(|pair| pair[0] <= pair[1]) }

/// The owned insert actions of one channel's data, its group's continuations
/// and its joins.
fn inserts(
    channels: &[Par],
    data: &[Datum<ListParWithRandom>],
    waiting: &[Waiting],
    joins: &[Vec<Par>],
) -> Vec<HotStoreAction<Par, BindPattern, ListParWithRandom, TaggedContinuation>> {
    vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: channels[0].clone(),
            data: data.to_vec(),
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: channels.to_vec(),
            continuations: waiting.to_vec(),
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: channels[0].clone(),
            joins: joins.to_vec(),
        })),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// D-S3 (D-C3, DR-97): a clean cold fill re-encodes to exactly the stored
    /// leaf. For generated rhoapi data, continuations and joins committed to a
    /// history, the stored rows are sorted, and the rows that a cold fill
    /// decodes re-encode to the stored rows in stored order. Exporting the
    /// decoded values again, as owned actions or as borrowed views with
    /// shared continuations, keeps the root.
    #[test]
    fn clean_cold_fill_reencodes_to_stored_leaf(
        channels in prop::collection::vec(term(), 1..3),
        patterns in prop::collection::vec(pattern(), 1..3),
        continuations in prop::collection::vec(continuation(), 1..3),
        peeks in prop::collection::btree_set(any::<i32>(), 0..8),
        data in prop::collection::vec((prop::collection::vec(term(), 0..3), any::<bool>()), 1..3),
        random_state in prop::collection::vec(any::<u8>(), 0..64),
        cost_authority in authority(),
    ) {
        let waiting: Vec<Waiting> = continuations
            .iter()
            .map(|continuation| {
                WaitingContinuation::create(&channels, &patterns, continuation, false, peeks.clone())
            })
            .collect();
        let datums: Vec<Datum<ListParWithRandom>> = data
            .into_iter()
            .map(|(pars, persist)| {
                Datum::create(
                    &channels[0],
                    ListParWithRandom {
                        pars,
                        random_state: random_state.clone(),
                        cost_authority: cost_authority.clone(),
                        cost_stack: None,
                    },
                    persist,
                )
            })
            .collect();
        let joins = vec![channels.clone()];
        let history = HistoryRepositoryInstances::<
            Par,
            BindPattern,
            ListParWithRandom,
            TaggedContinuation,
        >::lmdb_repository(
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
            Arc::new(InMemoryKeyValueStore::new()),
        )
        .expect("an in-memory history");
        let free = |_: usize, _: usize, _: usize| Ok::<(), RSpaceError>(());
        let prepared = history
            .prepare_native_checkpoint(inserts(&channels, &datums, &waiting, &joins), &free)
            .expect("the first checkpoint");
        let history = history.commit_native_checkpoint(prepared).expect("the first commit");
        let root = history.root();
        let keys = GroupKeys::build(&channels, &free).expect("a free meter");
        let group = channels_hash_from_keys(&keys.channels, &free).expect("a free meter");
        let data_rows = stored_rows(history.as_ref(), NativeLeafKind::Data, &keys.channels[0].0);
        let continuation_rows = stored_rows(history.as_ref(), NativeLeafKind::Continuations, &group);
        let join_rows = stored_rows(history.as_ref(), NativeLeafKind::Joins, &keys.channels[0].0);
        prop_assert!(sorted(&data_rows) && sorted(&continuation_rows) && sorted(&join_rows));
        let read_data: Vec<Datum<ListParWithRandom>> = decoded(&data_rows);
        let read_waiting: Vec<Waiting> = decoded(&continuation_rows);
        let read_joins: Vec<Vec<Par>> = decoded(&join_rows);
        prop_assert_eq!(encoded(&read_data), data_rows);
        prop_assert_eq!(encoded(&read_waiting), continuation_rows);
        prop_assert_eq!(encoded(&read_joins), join_rows);
        let prepared = history
            .prepare_native_checkpoint(inserts(&channels, &read_data, &read_waiting, &read_joins), &free)
            .expect("the owned re-export");
        let owned = history.commit_native_checkpoint(prepared).expect("the owned commit");
        prop_assert_eq!(owned.root(), root.clone());
        let shared: Vec<Arc<Waiting>> = read_waiting.iter().cloned().map(Arc::new).collect();
        let views = [
            NativeExportAction::InsertData { channel: &channels[0], data: &read_data },
            NativeExportAction::InsertContinuations { channels: &channels, continuations: &shared },
            NativeExportAction::InsertJoins { channel: &channels[0], joins: &read_joins },
        ];
        let prepared = history
            .prepare_native_checkpoint_borrowed(&views, &free)
            .expect("the borrowed re-export");
        let borrowed = history.commit_native_checkpoint(prepared).expect("the borrowed commit");
        prop_assert_eq!(borrowed.root(), root);
    }
}
