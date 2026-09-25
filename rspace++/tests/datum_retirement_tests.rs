use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use proptest::prelude::*;
use rspace_plus_plus::rspace::errors::RSpaceError;
use rspace_plus_plus::rspace::r#match::Match;
use rspace_plus_plus::rspace::rspace::RSpace;
use rspace_plus_plus::rspace::rspace_interface::{
    ISpace, RSpaceAccountingObserver, RSpaceOperationSource, ReplayOperationDirective,
};
use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;
use rspace_plus_plus::rspace::trace::event::{COMM, Consume, Produce};

struct Exact;
impl Match<String, String, String> for Exact {
    fn get(&self, pattern: &String, datum: &String) -> Option<String> {
        (pattern == datum).then(|| datum.clone())
    }
}

#[derive(Default)]
struct Capture {
    comms: Mutex<Vec<COMM>>,
    directive: Option<ReplayOperationDirective>,
    denied: bool,
}

impl RSpaceAccountingObserver<String, String, String, String> for Capture {
    fn replay_operation_directive(
        &self,
        _: RSpaceOperationSource<'_>,
    ) -> Result<Option<ReplayOperationDirective>, RSpaceError> {
        Ok(self.directive.clone())
    }
    fn observe_produce(
        &self,
        _: &Produce,
        _: &String,
        _: &String,
        _: bool,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }
    fn observe_consume(
        &self,
        _: &Consume,
        _: &[String],
        _: &[String],
        _: &String,
        _: bool,
        _: &BTreeSet<i32>,
    ) -> Result<(), RSpaceError> {
        Ok(())
    }
    fn observe_comm(
        &self,
        comm: &COMM,
        _: &String,
        _: bool,
        _: &[(&String, bool)],
    ) -> Result<(), RSpaceError> {
        self.comms.lock().unwrap().push(comm.clone());
        if self.denied {
            Err(RSpaceError::OutOfPhlogistons)
        } else {
            Ok(())
        }
    }
}

async fn exercise(selected: &[bool], produce: bool, native: bool, persistent: bool) {
    let channel = "channel".to_string();
    let values: Vec<_> = (0..selected.len())
        .map(|index| format!("value-{index}"))
        .collect();
    let mut wanted: Vec<_> = values
        .iter()
        .zip(selected)
        .filter(|(_, pick)| **pick)
        .map(|(v, _)| v.clone())
        .collect();
    if wanted.is_empty() {
        return;
    }
    let mut channels = vec![channel.clone(); wanted.len()];
    if produce {
        channels.push("gate".into());
        wanted.push("go".into());
    }
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Exact)),
    )
    .unwrap();
    let initial = play.create_checkpoint().await.unwrap();
    let capture = Arc::new(Capture::default());
    play.set_accounting_observer(Some(capture.clone()));
    for value in values.iter().rev() {
        play.produce(channel.clone(), value.clone(), persistent)
            .await
            .unwrap();
    }
    let matched = if produce {
        assert!(
            play.consume(channels.clone(), wanted.clone(), "body".into(), false, BTreeSet::new())
                .await
                .unwrap()
                .is_none()
        );
        play.produce("gate".into(), "go".into(), false)
            .await
            .unwrap()
            .unwrap()
            .1
    } else {
        play.consume(channels.clone(), wanted.clone(), "body".into(), false, BTreeSet::new())
            .await
            .unwrap()
            .unwrap()
            .1
    };
    assert_eq!(
        matched
            .iter()
            .map(|d| d.matched_datum.clone())
            .collect::<Vec<_>>(),
        wanted
    );
    let mut survivors: Vec<_> = values
        .iter()
        .zip(selected)
        .filter(|(_, pick)| persistent || !**pick)
        .map(|(v, _)| v.clone())
        .collect();
    survivors.sort();
    let mut remaining: Vec<_> = play
        .get_data(&channel)
        .await
        .into_iter()
        .map(|d| d.a)
        .collect();
    remaining.sort();
    assert_eq!(remaining, survivors, "play removed the wrong occurrences");
    let expected = capture.comms.lock().unwrap()[0].clone();
    replay
        .rig_and_reset(initial.root, play.create_checkpoint().await.unwrap().log)
        .await
        .unwrap();
    for value in values.iter().rev() {
        replay
            .produce(channel.clone(), value.clone(), persistent)
            .await
            .unwrap();
    }
    if produce {
        replay
            .consume(channels.clone(), wanted.clone(), "body".into(), false, BTreeSet::new())
            .await
            .unwrap();
    }
    if native {
        replay.set_accounting_observer(Some(Arc::new(Capture {
            comms: Mutex::new(Vec::new()),
            directive: Some(ReplayOperationDirective::AcceptedComm(Arc::new(expected))),
            denied: false,
        })));
    }
    let matched = if produce {
        replay
            .produce("gate".into(), "go".into(), false)
            .await
            .unwrap()
            .unwrap()
            .1
    } else {
        replay
            .consume(channels, wanted.clone(), "body".into(), false, BTreeSet::new())
            .await
            .unwrap()
            .unwrap()
            .1
    };
    assert_eq!(
        matched
            .iter()
            .map(|d| d.matched_datum.clone())
            .collect::<Vec<_>>(),
        wanted
    );
    let mut remaining: Vec<_> = replay
        .get_data(&channel)
        .await
        .into_iter()
        .map(|d| d.a)
        .collect();
    remaining.sort();
    assert_eq!(remaining, survivors, "replay removed the wrong occurrences");
    replay.check_replay_data().await.unwrap();
}

#[tokio::test]
async fn repeated_channel_retirement_keeps_unmatched_occurrences() {
    for produce in [false, true] {
        for native in [false, true] {
            for persistent in [false, true] {
                exercise(&[true, true, false], produce, native, persistent).await;
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(96))]
    #[test]
    fn arbitrary_retirement_preserves_exact_complement(selected in prop::collection::vec(any::<bool>(), 1..16),
        produce in any::<bool>(), native in any::<bool>(), persistent in any::<bool>()) {
        tokio::runtime::Runtime::new().unwrap().block_on(exercise(&selected, produce, native, persistent));
    }
}

#[tokio::test]
async fn denied_trigger_need_not_be_a_selected_comm_participant() {
    use rspace_plus_plus::rspace::internal::WaitingContinuation;
    let channel = "channel".to_string();
    let channels = vec![channel.clone()];
    let mut stores = InMemoryStoreManager::new();
    let (play, replay) = RSpace::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Exact)),
    )
    .unwrap();
    play.produce(channel.clone(), "eligible".to_string(), false)
        .await
        .unwrap();
    play.get_store()
        .put_continuation(
            &channels,
            WaitingContinuation::create(
                &channels,
                &vec!["eligible".to_string()],
                &"body".to_string(),
                false,
                BTreeSet::new(),
            ),
        )
        .unwrap();
    play.get_store().put_join(&channel, &channels);
    let initial = play.create_checkpoint().await.unwrap();
    let capture = Arc::new(Capture {
        denied: true,
        ..Capture::default()
    });
    play.set_accounting_observer(Some(capture.clone()));
    assert_eq!(
        play.produce(channel.clone(), "unused".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    let expected = capture.comms.lock().unwrap()[0].clone();
    assert!(
        !expected
            .produces
            .contains(&Produce::create(&channel, &"unused".to_string(), false))
    );
    replay
        .rig_and_reset(initial.root, play.create_checkpoint().await.unwrap().log)
        .await
        .unwrap();
    replay.set_accounting_observer(Some(Arc::new(Capture {
        directive: Some(ReplayOperationDirective::RejectedComm(Arc::new(expected))),
        denied: true,
        ..Capture::default()
    })));
    let before = replay.get_data(&channel).await;
    assert_eq!(
        replay
            .produce(channel.clone(), "unused".to_string(), false)
            .await,
        Err(RSpaceError::OutOfPhlogistons)
    );
    assert_eq!(replay.get_data(&channel).await, before);
    assert_eq!(replay.get_waiting_continuations(channels).await.len(), 1);
    assert!(
        replay
            .create_soft_checkpoint()
            .await
            .produce_counter
            .is_empty()
    );
}
