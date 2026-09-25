use proptest::prelude::*;

use super::*;

proptest! {
    #[test]
    fn arbitrary_attempt_sequences_preserve_state_trace_and_lifecycle(
        actions in prop::collection::vec((any::<bool>(), 0usize..3, any::<bool>(), any::<bool>(), any::<bool>()), 0..40)
    ) {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let observer = Arc::new(Observer::new(false));
            let space = space(observer.clone()).await;
            let mut data = [0usize; 3];
            let mut continuations = [0usize; 3];
            for (produce, channel, reject_start, reject_introduction, reject_comm) in actions {
                observer.reject_start.store(reject_start, Ordering::Release);
                observer.reject_operation.store(reject_introduction, Ordering::Release);
                observer.reject.store(reject_comm, Ordering::Release);
                observer.operations.lock().unwrap().clear();
                let name = channel.to_string();
                let matching = if produce { continuations[channel] > 0 } else { data[channel] > 0 };
                let rejected = reject_start || reject_introduction || (matching && reject_comm);
                let result = if produce {
                    space.produce(name.clone(), "datum".to_string(), false).await.map(|result| result.is_some())
                } else {
                    space.consume(vec![name.clone()], vec![Any], "body".to_string(), false, BTreeSet::new()).await.map(|result| result.is_some())
                };
                assert_eq!(result, if rejected { Err(RSpaceError::OutOfPhlogistons) } else { Ok(matching) });
                if !rejected {
                    match (produce, matching) {
                        (true, true) => continuations[channel] -= 1,
                        (false, true) => data[channel] -= 1,
                        (true, false) => data[channel] += 1,
                        (false, false) => continuations[channel] = 1,
                    }
                }
                for index in 0..3 {
                    assert_eq!(space.get_data(&index.to_string()).await.len(), data[index]);
                    assert_eq!(space.get_waiting_continuations(vec![index.to_string()]).await.len(), continuations[index]);
                }
                let trace = space.take_event_log().await;
                assert_eq!(trace.len(), if rejected { 0 } else if matching { 2 } else { 1 });
                assert_eq!(trace.iter().filter(|event| matches!(event, Event::Comm(_))).count(), usize::from(!rejected && matching));
                let events = observer.operations.lock().unwrap();
                if reject_start {
                    assert!(events.is_empty());
                    continue;
                }
                let mut expected = 3;
                if matching && !reject_introduction {
                    expected += 1;
                    assert!(matches!(events[2], OperationEvent::Comm));
                }
                assert_eq!(events.len(), expected);
                assert!(matches!(events[1], OperationEvent::Introduction));
                let OperationEvent::Start { source, channels, .. } = &events[0] else { panic!("missing start") };
                assert_eq!(channels, &[name]);
                assert_eq!(events.last().unwrap(), &OperationEvent::Finish {
                    source: source.clone(),
                    completion: if rejected {
                        RSpaceOperationCompletion::Rejected
                    } else if matching {
                        RSpaceOperationCompletion::Matched
                    } else {
                        RSpaceOperationCompletion::Stored
                    },
                });
            }
        });
    }
}
