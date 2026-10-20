//! D-S2 (DR-95): decoding of the RSpace history records that hold rhoapi
//! terms. Changed by DR-131 S6: the cold-store decoder (`decode_datum`,
//! `decode_continuation` and `decode_join`) replaced the History mode of the
//! typed decoder.

use std::sync::Arc;

use models::rhoapi::tagged_continuation::TaggedCont;
use models::rhoapi::var::VarInstance;
use models::rhoapi::{
    cost_signature, BindPattern, CostAuthority, CostRegion, CostSignature, CostStack,
    ListParWithRandom, Par, ParWithRandom, TaggedContinuation, Var,
};
use proptest::prelude::*;
use rspace_plus_plus::rspace::hashing::native_source::{channels_hash_from_keys, GroupKeys};
use rspace_plus_plus::rspace::history::history_repository::{
    HistoryRepository, HistoryRepositoryInstances,
};
use rspace_plus_plus::rspace::history::native_reader::{
    decode_continuation, decode_datum, decode_join, NativeLeafKind,
};
use rspace_plus_plus::rspace::hot_store_action::{
    HotStoreAction, InsertAction, InsertContinuations, InsertData, InsertJoins, NativeExportAction,
};
use rspace_plus_plus::rspace::internal::{Datum, WaitingContinuation};
use rspace_plus_plus::rspace::serializers::cold_store_decode::ColdStoreDecodeError;
use rspace_plus_plus::rspace::shared::in_mem_key_value_store::InMemoryKeyValueStore;
use serde::Serialize;

use crate::rust::interpreter::accounting::random_par_term as term;

/// Changed by DR-131 S6: decodes the history bytes of `value` with the
/// cold-store decoder of its record kind and checks that the decoded value
/// encodes to the same bytes. The History mode, the legacy mode and their
/// reservations are deleted.
fn round_trip<T: Serialize>(value: &T, decode: impl Fn(&[u8]) -> Result<T, ColdStoreDecodeError>) {
    let bytes = bincode::serialize(value).expect("history record encodes");
    let decoded = decode(&bytes).expect("the cold-store decoder decodes the record");
    assert_eq!(
        bincode::serialize(&decoded).expect("decoded record encodes"),
        bytes
    );
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

    /// Changed by DR-131 S6: the test was
    /// `history_records_cover_actual_allocations`. The reservation checks
    /// went with host work. The test keeps its check that each generated
    /// waiting continuation, datum and joined channel list decodes to the
    /// encoded value, now with the cold-store decoder (D-S2, DR-95).
    #[test]
    fn history_records_decode_to_the_encoded_value(
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
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
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
        round_trip(&waiting, decode_continuation::<BindPattern, TaggedContinuation>);
        round_trip(&datum, decode_datum::<ListParWithRandom>);
        round_trip(&data, decode_join::<Par>);
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
        .with_records(kind, projection, |rows| {
            Ok::<_, ()>(rows.iter().map(<[u8]>::to_vec).collect())
        })
        .expect("the stored leaf reads")
        .expect("the leaf is stored")
}

/// Decodes rows with the cold-store decoder of their kind, as a cold fill
/// does.
fn decoded<T>(
    rows: &[Vec<u8>],
    decode: impl Fn(&[u8]) -> Result<T, ColdStoreDecodeError>,
) -> Vec<T> {
    rows.iter()
        .map(|row| decode(row).expect("the row decodes"))
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
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
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
        let prepared = history
            .prepare_native_checkpoint(inserts(&channels, &datums, &waiting, &joins))
            .expect("the first checkpoint");
        let history = history.commit_native_checkpoint(prepared).expect("the first commit");
        let root = history.root();
        let keys = GroupKeys::build(&channels).expect("group keys");
        let group = channels_hash_from_keys(&keys.channels);
        let data_rows = stored_rows(history.as_ref(), NativeLeafKind::Data, &keys.channels[0].0);
        let continuation_rows = stored_rows(history.as_ref(), NativeLeafKind::Continuations, &group);
        let join_rows = stored_rows(history.as_ref(), NativeLeafKind::Joins, &keys.channels[0].0);
        prop_assert!(sorted(&data_rows) && sorted(&continuation_rows) && sorted(&join_rows));
        let read_data: Vec<Datum<ListParWithRandom>> =
            decoded(&data_rows, decode_datum::<ListParWithRandom>);
        let read_waiting: Vec<Waiting> =
            decoded(&continuation_rows, decode_continuation::<BindPattern, TaggedContinuation>);
        let read_joins: Vec<Vec<Par>> = decoded(&join_rows, decode_join::<Par>);
        prop_assert_eq!(encoded(&read_data), data_rows);
        prop_assert_eq!(encoded(&read_waiting), continuation_rows);
        prop_assert_eq!(encoded(&read_joins), join_rows);
        let prepared = history
            .prepare_native_checkpoint(inserts(&channels, &read_data, &read_waiting, &read_joins))
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
            .prepare_native_checkpoint_borrowed(&views)
            .expect("the borrowed re-export");
        let borrowed = history.commit_native_checkpoint(prepared).expect("the borrowed commit");
        prop_assert_eq!(borrowed.root(), root);
    }
}
