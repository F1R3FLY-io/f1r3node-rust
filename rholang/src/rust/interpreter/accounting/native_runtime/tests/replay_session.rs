use models::rust::host_work::{HostWorkDimension, HostWorkLimit, HostWorkLimits};
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

use super::*;

struct Matcher;

impl Match<String, String, String> for Matcher {
    fn get(&self, _: &String, datum: &String) -> Option<String> { Some(datum.clone()) }

    fn get_metered(
        &self,
        _: &String,
        datum: &String,
        meter: &(dyn rspace_plus_plus::rspace::hashing::native_source::SourceMeter + Send + Sync),
    ) -> Result<Option<String>, RSpaceError> {
        meter.reserve(1, datum.len(), datum.len())?;
        Ok(Some(datum.clone()))
    }

    fn check_commit_metered(
        &self,
        _: &String,
        _: &[String],
        meter: &(dyn rspace_plus_plus::rspace::hashing::native_source::SourceMeter + Send + Sync),
    ) -> Result<bool, RSpaceError> {
        meter.reserve(1, 0, 0)?;
        Ok(true)
    }
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

#[test]
fn replay_rejects_before_allocating_fixed_owners_without_cleanup_credit() {
    let budget = HostWorkBudget::new(HostWorkLimits::uniform(HostWorkLimit::new(3)));
    assert!(empty_trace().into_replay(budget.clone()).is_err());
    assert_eq!(budget.usage(HostWorkDimension::SearchStateBytes).get(), 0);
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

#[tokio::test]
async fn cold_session_queries_preserve_rejected_execution_and_use_the_inspection_budget() {
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
    let execution = host();
    let session = empty_trace()
        .into_session(
            play.get_history_repository(),
            Arc::new(Box::new(Matcher)),
            execution.clone(),
        )
        .unwrap();
    execution
        .reserve(HostWorkDimension::VerificationOperations, u64::MAX.into())
        .unwrap_err();
    assert_eq!(
        session.get_data(&"cold".into()).await.unwrap_err(),
        RSpaceError::HostWorkRejected
    );
    let inspection = host();
    assert_eq!(
        session
            .get_data_with_host_work(&"cold".into(), &inspection)
            .await
            .unwrap()[0]
            .a,
        "retained"
    );
    assert!(
        inspection
            .usage(HostWorkDimension::VerificationOperations)
            .get()
            > 0
    );
    assert!(inspection.usage(HostWorkDimension::SearchStateBytes).get() > 0);
    assert_eq!(
        session.get_data(&"cold".into()).await.unwrap_err(),
        RSpaceError::HostWorkRejected
    );
    assert_eq!(play.get_history_repository().root(), root);
    session.close().await.unwrap();
    assert!(session
        .get_data_with_host_work(&"cold".into(), &host())
        .await
        .is_err());
}

#[test]
fn guarded_history_decoder_preserves_the_production_rho_record_schema() {
    use models::rhoapi::{ListParWithRandom, Par};
    use rspace_plus_plus::rspace::history::native_reader::{decode_record, NativeReadError};
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
    let budget = host();
    assert_eq!(
        decode_record::<Datum<ListParWithRandom>, _>(&bytes, &budget).unwrap(),
        expected
    );
    assert!(
        budget
            .usage(HostWorkDimension::VerificationOperations)
            .get()
            > 0
    );
    assert!(budget.usage(HostWorkDimension::SearchStateBytes).get() > 0);
    budget
        .reserve(HostWorkDimension::VerificationOperations, u64::MAX.into())
        .unwrap_err();
    assert!(matches!(
        decode_record::<Datum<ListParWithRandom>, _>(&bytes, &budget),
        Err(NativeReadError::Host(InterpreterError::HostWorkRejected))
    ));
}
