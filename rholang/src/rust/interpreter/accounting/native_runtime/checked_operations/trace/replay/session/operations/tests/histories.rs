use futures::FutureExt;
use proptest::prelude::*;

use super::*;

async fn replay_history(inputs: &[(u8, u8)], persistent: bool) {
    let channels: Vec<_> = (0..4)
        .map(|index| Par {
            exprs: vec![models::rhoapi::Expr {
                expr_instance: Some(models::rhoapi::expr::ExprInstance::GInt(index)),
            }],
            ..Default::default()
        })
        .collect();
    let auth = authority(1);
    let data: Vec<_> = inputs
        .iter()
        .enumerate()
        .map(|(index, (channel, _))| ListParWithRandom {
            random_state: vec![
                if persistent {
                    *channel % 4
                } else {
                    index as u8
                };
                17
            ],
            cost_authority: Some(auth.clone()),
            ..Default::default()
        })
        .collect();
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let history = play.get_history_repository();
    let budget = RuntimeBudget::new(Cost::unsafe_max());
    budget
        .reset_for_native_execution(config(u64::MAX, [0, 1, 1, 0]))
        .unwrap();
    let _scope = budget.enter_comm_accounting_scope();
    play.set_accounting_observer(Some(test_accounting_observer(budget.clone())));
    let orders: Vec<_> = (0..inputs.len())
        .map(|index| OperationOrder {
            session: budget.deploy_id(),
            path: vec![((inputs.len() - index) as u64, 0)].into(),
        })
        .collect();
    for (index, (channel, _)) in inputs.iter().enumerate() {
        let channel = &channels[usize::from(*channel % 4)];
        let datum = &data[index];
        let source = Produce::create(channel, datum, persistent);
        let observation =
            observation_construction::produce_introduction(&source, channel, datum, &auth).unwrap();
        budget
            .register_introduction_authority(
                observation.event_id,
                AuthorityByteEventKind::ProduceIntroduction,
                &auth,
            )
            .unwrap();
        assert!(operation_context::scope(
            orders[index].clone(),
            play.produce(channel.clone(), datum.clone(), persistent),
        )
        .await
        .unwrap()
        .is_none());
    }
    let recording = budget.native_budget_recording().unwrap().unwrap();
    if persistent && inputs.len() > 4 {
        assert!(!recording.retries.is_empty());
    }
    let rows = budget.native_operation_recording().unwrap().unwrap();
    let log = play.create_soft_checkpoint().await.log;
    let trace = with_contract(u64::MAX, [0, 1, 1, 0], |contract| {
        contract
            .check_operation_journal(recording.session, &recording, rows, journal_limits())
            .unwrap()
    })
    .bind_trace(log.into(), NativeOperationTraceLimits {
        events: 64,
        source_entries: 1024,
        source_bytes: 1_000_000,
        telemetry_items: 1024,
        telemetry_bytes: 1_000_000,
    })
    .unwrap();
    let replay = Arc::new(
        trace
            .into_session(history, Arc::new(Box::new(Matcher)))
            .unwrap(),
    );
    let root = replay.checkpoint().await.unwrap();
    // Changed by DR-132 (D1-A): replay gives a live produce the first open row
    // of its recorded source, not the row of its name. A dependency wait thus
    // needs the first row of a source with an earlier row on its channel. A
    // persistent history puts one datum on each channel, so its rows of one
    // channel share one source and never wait here.
    // if let Some(index) = (1..inputs.len()).find(|index| {
    //     inputs[..*index]
    //         .iter()
    //         .any(|(channel, _)| *channel % 4 == inputs[*index].0 % 4)
    // }) {
    let same_source = |left: usize, right: usize| {
        inputs[left].0 % 4 == inputs[right].0 % 4 && data[left] == data[right]
    };
    if let Some(index) = (1..inputs.len()).find(|index| {
        (0..*index).all(|earlier| !same_source(earlier, *index))
            && inputs[..*index]
                .iter()
                .any(|(channel, _)| *channel % 4 == inputs[*index].0 % 4)
    }) {
        let mut cancelled = Box::pin(operation_context::scope(
            orders[index].clone(),
            replay.produce(
                channels[usize::from(inputs[index].0 % 4)].clone(),
                data[index].clone(),
                persistent,
                &auth,
            ),
        ));
        assert!(cancelled.as_mut().now_or_never().is_none());
        drop(cancelled);
        let unchanged = replay
            .checkpoint()
            .now_or_never()
            .expect("cancelled dependency wait retained its lease")
            .unwrap();
        replay.restore(unchanged).await.unwrap();
    }
    let mut submission: Vec<_> = (0..inputs.len()).collect();
    submission.sort_by_key(|index| (inputs[*index].1, std::cmp::Reverse(*index)));
    let mut tasks = Vec::new();
    for index in submission {
        let replay = replay.clone();
        let channel = channels[usize::from(inputs[index].0 % 4)].clone();
        let datum = data[index].clone();
        let order = orders[index].clone();
        let auth = auth.clone();
        tasks.push(tokio::spawn(async move {
            operation_context::scope(order, replay.produce(channel, datum, persistent, &auth)).await
        }));
    }
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        for task in tasks {
            assert!(task.await.unwrap().unwrap().is_none());
        }
    })
    .await
    .expect("valid dependency history did not complete");
    replay.check_complete().await.unwrap();
    for channel in &channels {
        assert_eq!(
            replay.get_data(channel).await.unwrap(),
            play.get_data(channel).await
        );
    }
    let abandoned = replay.checkpoint().await.unwrap();
    replay.restore(root).await.unwrap();
    for channel in &channels {
        assert!(replay.get_data(channel).await.unwrap().is_empty());
    }
    assert!(replay.check_complete().await.is_err());
    assert!(replay.restore(abandoned).await.is_err());
    for index in 0..inputs.len() {
        assert!(operation_context::scope(
            orders[index].clone(),
            replay.produce(
                channels[usize::from(inputs[index].0 % 4)].clone(),
                data[index].clone(),
                persistent,
                &auth,
            ),
        )
        .await
        .unwrap()
        .is_none());
    }
    replay.check_complete().await.unwrap();
    for channel in &channels {
        assert_eq!(
            replay.get_data(channel).await.unwrap(),
            play.get_data(channel).await
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn persistent_retries_replay_in_reverse_submission_order_and_restore() {
    models::rust::record_encoding::set_record_encoding(
        models::rust::record_encoding::RecordEncoding::Cost,
    );
    replay_history(&[(0, 0); 8], true).await;
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn generated_parallel_histories_preserve_exact_tuple_prefixes(
        inputs in prop::collection::vec((any::<u8>(), any::<u8>()), 1..24),
        persistent in any::<bool>(),
    ) {
        models::rust::record_encoding::set_record_encoding(models::rust::record_encoding::RecordEncoding::Cost);
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(replay_history(&inputs, persistent));
    }
}
