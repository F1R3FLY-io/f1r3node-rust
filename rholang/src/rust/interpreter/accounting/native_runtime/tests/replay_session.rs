use models::rust::host_work::HostWorkDimension;
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
        used: 0,
    };
    bind(&recording, Arc::from([]), Vec::new()).unwrap()
}

async fn session(
    trace: CheckedNativeOperationTrace,
    budget: HostWorkBudget,
) -> NativeRuntimeReplaySession<String, String, String, String> {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    trace
        .into_session(
            play.get_history_repository(),
            Arc::new(Box::new(Matcher)),
            budget,
        )
        .unwrap()
}

#[tokio::test]
async fn checked_session_restores_after_host_exhaustion_without_reopening_closed_epochs() {
    let budget = host();
    let session = session(empty_trace(), budget.clone()).await;
    session.check_complete().await.unwrap();
    let evidence = session.completed_evidence().await.unwrap();
    assert_eq!(evidence.usage(), 0);
    assert!(evidence.observations().has_complete_measurements());
    assert!(evidence.observations().rows.is_empty());
    let checkpoint = session.checkpoint().await.unwrap();
    assert!(budget
        .reserve(HostWorkDimension::VerificationOperations, u64::MAX.into())
        .is_err());
    assert!(session.completed_evidence().await.is_err());
    let rejected = session
        .checkpoint()
        .await
        .err()
        .expect("host budget was exhausted");
    assert_eq!(
        InterpreterError::from(rejected),
        InterpreterError::HostWorkRejected
    );
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
    let incomplete = session(bind(&recording, rows, log).unwrap(), host()).await;
    let other = session(empty_trace(), host()).await;
    let foreign = other.checkpoint().await.unwrap();
    assert!(incomplete.restore(foreign).await.is_err());
    assert!(incomplete.check_complete().await.is_err());
    let checkpoint = incomplete.checkpoint().await.unwrap();
    incomplete.restore(checkpoint).await.unwrap();
    assert!(incomplete.check_complete().await.is_err());
    other.check_complete().await.unwrap();
}

#[tokio::test]
async fn borrowed_history_uses_the_production_host_budget_without_refunding_reads() {
    use rspace_plus_plus::rspace::hashing::stable_hash_provider::hash;
    use rspace_plus_plus::rspace::history::native_reader::{NativeLeafKind, NativeReadError};
    use rspace_plus_plus::rspace::rspace_interface::ISpace;

    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.produce("c".into(), "v".into(), false).await.unwrap();
    let checkpoint = play.create_checkpoint().await.unwrap();
    let repository = play.get_history_repository();
    let reader = repository.native_history_reader(checkpoint.root.0.as_slice().try_into().unwrap());
    let channel = hash(&"c".to_owned()).0.as_slice().try_into().unwrap();
    let budget = host();
    let mut previous_backing = 0;
    for _ in 0..2 {
        assert_eq!(
            reader
                .with_records(NativeLeafKind::Data, &channel, &budget, |rows| Ok(
                    rows.len()
                ))
                .unwrap(),
            Some(1)
        );
        let backing = budget.usage(HostWorkDimension::SearchStateBytes).get();
        assert!(backing > previous_backing);
        previous_backing = backing;
    }
    assert!(budget
        .reserve(HostWorkDimension::VerificationOperations, u64::MAX.into())
        .is_err());
    let result: Result<Option<()>, _> =
        reader.with_records(NativeLeafKind::Data, &channel, &budget, |_| {
            panic!("rejected history read reached the consumer")
        });
    assert!(matches!(result, Err(NativeReadError::Host(_))));
    assert_eq!(
        budget.usage(HostWorkDimension::SearchStateBytes).get(),
        previous_backing
    );
}
