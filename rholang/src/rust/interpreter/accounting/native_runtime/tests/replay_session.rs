use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;

struct Matcher;

impl Match<String, String, String> for Matcher {
    fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }
}

fn empty_trace() -> CheckedNativeOperationTrace {
    let recording = NativeBudgetRecording {
        session: [0; 32],
        attempts: Arc::from([]),
        retries: Arc::from([]),
        transfers: Arc::from([]),
        used: 0,
        paths: fixture_paths(),
        // Added by G3 (DR-135, S4).
        owner_table: Arc::from([]),
    };
    bind(&recording, Arc::from([]), Vec::new()).unwrap()
}

async fn session(
    trace: CheckedNativeOperationTrace,
) -> NativeRuntimeReplaySession<String, String, String, String> {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    trace
        .into_session(play.get_history_repository(), Arc::new(Box::new(Matcher)))
        .unwrap()
}

/// Changed by DR-131: the host-work exhaustion between the checkpoint and the
/// restore went with host work. The test keeps its checks of the restore and
/// of the closed session.
#[tokio::test]
async fn checked_session_restores_without_reopening_closed_epochs() {
    let session = session(empty_trace()).await;
    session.check_complete().await.unwrap();
    let evidence = session.completed_evidence().await.unwrap();
    assert_eq!(evidence.usage(), 0);
    assert!(evidence.observations().has_complete_measurements());
    assert!(evidence.observations().rows.is_empty());
    let checkpoint = session.checkpoint().await.unwrap();
    session.restore(checkpoint).await.unwrap();
    session.check_complete().await.unwrap();
    session.close().await.unwrap();
    assert!(session.checkpoint().await.is_err());
    assert!(session.check_complete().await.is_err());
    assert!(session.completed_evidence().await.is_err());
    assert!(session.get_data(&"c".into()).await.is_err());
}

#[tokio::test]
async fn checked_session_rejects_foreign_checkpoint_and_preserves_incomplete_trace() {
    let (recording, rows, log) = trace_fixture();
    let incomplete = session(bind(&recording, rows, log).unwrap()).await;
    let other = session(empty_trace()).await;
    let foreign = other.checkpoint().await.unwrap();
    assert!(incomplete.restore(foreign).await.is_err());
    assert!(incomplete.check_complete().await.is_err());
    let checkpoint = incomplete.checkpoint().await.unwrap();
    incomplete.restore(checkpoint).await.unwrap();
    assert!(incomplete.check_complete().await.is_err());
    other.check_complete().await.unwrap();
}

/// Changed by DR-131: the session has no execution budget to exhaust and no
/// inspection budget. The test keeps its checks that a cold query reads the
/// retained value without a root change, and that a closed session rejects
/// the query.
#[tokio::test]
async fn cold_session_queries_read_the_retained_value_until_close() {
    use rspace_plus_plus::rspace::rspace_interface::ISpace;

    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.produce("cold".into(), "retained".into(), false)
        .await
        .unwrap();
    let root = play.create_checkpoint().await.unwrap().root;
    let session = empty_trace()
        .into_session(play.get_history_repository(), Arc::new(Box::new(Matcher)))
        .unwrap();
    assert_eq!(
        session.get_data(&"cold".into()).await.unwrap()[0].a,
        "retained"
    );
    assert_eq!(play.get_history_repository().root(), root);
    session.close().await.unwrap();
    assert!(session.get_data(&"cold".into()).await.is_err());
}

/// Changed by DR-131: the decoder's charges went with host work, and S6
/// replaced the typed decoder with the cold-store decoder (`decode_datum`).
/// The test keeps its check of the record schema.
#[test]
fn guarded_history_decoder_preserves_the_production_rho_record_schema() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    use models::rhoapi::{ListParWithRandom, Par};
    use rspace_plus_plus::rspace::history::native_reader::decode_datum;
    use rspace_plus_plus::rspace::internal::Datum;
    use rspace_plus_plus::rspace::trace::event::Produce;

    let value = ListParWithRandom {
        pars: vec![Par {
            locally_free: vec![1, 2, 3],
            ..Default::default()
        }],
        random_state: vec![7; 32],
        ..Default::default()
    };
    let datum = Datum {
        source: Produce::create(&Par::default(), &value, false),
        a: value,
        persist: false,
    };
    let bytes = bincode::serialize(&datum).unwrap();
    let expected: Datum<ListParWithRandom> = bincode::deserialize(&bytes).unwrap();
    assert_eq!(decode_datum::<ListParWithRandom>(&bytes).unwrap(), expected);
}
