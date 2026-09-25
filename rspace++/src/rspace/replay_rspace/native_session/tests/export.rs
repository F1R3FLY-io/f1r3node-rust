use std::panic::AssertUnwindSafe;

use super::*;

async fn reopened(source: &Session, root: &Blake2b256Hash) -> Session {
    let history = source.space.get_history_repository().reset(root).unwrap();
    Session::new(Arc::new(history), Arc::new(Box::new(Matcher)), Epoch::default()).unwrap()
}

#[tokio::test]
async fn export_persists_the_restored_state_and_permanently_closes_the_session() {
    let session = session().await;
    let empty = session.checkpoint().await.unwrap();
    put(&session, "first").await;
    let first = session.checkpoint().await.unwrap();
    put(&session, "discarded").await;
    session.restore(first).await.unwrap();
    let export = session.export().await.unwrap();
    assert_eq!(*export.evidence(), 1);
    assert_eq!(values(&reopened(&session, export.root()).await).await, ["first"]);
    assert!(session.export().await.is_err());
    assert!(session.checkpoint().await.is_err());
    assert!(session.restore(empty).await.is_err());
    assert!(session.completed_evidence().await.is_err());
    assert!(session.get_data(&"c".into()).await.is_err());
    let (root, evidence) = export.into_parts();
    assert_eq!(evidence, 1);
    assert!(
        session
            .space
            .get_history_repository()
            .contains_root(&root)
            .unwrap()
    );
}

#[tokio::test]
async fn incomplete_or_host_rejected_export_preserves_the_live_session() {
    let session = session().await;
    put(&session, "first").await;
    let before = session.space.get_history_repository().root();
    session.epoch.reject_complete.store(true, Ordering::Release);
    assert!(session.export().await.is_err());
    assert_eq!(values(&session).await, ["first"]);
    assert!(!session.epoch.state.lock().unwrap().3);
    session
        .epoch
        .reject_complete
        .store(false, Ordering::Release);
    session.epoch.reject_work.store(true, Ordering::Release);
    assert!(session.export().await.is_err());
    assert!(session.get_data(&"c".into()).await.is_err());
    session.epoch.reject_work.store(false, Ordering::Release);
    assert_eq!(values(&session).await, ["first"]);
    assert_eq!(session.space.get_history_repository().root(), before);
    assert!(!session.epoch.state.lock().unwrap().3);
    session.export().await.unwrap();
}

#[tokio::test]
async fn export_waits_for_every_operation_lease_and_cancellation_before_capture_is_safe() {
    let session = session().await;
    put(&session, "first").await;
    let first = session.gate.read().await;
    let second = session.gate.read().await;
    assert!(session.export().now_or_never().is_none());
    let mut export = Box::pin(session.export());
    assert!(export.as_mut().now_or_never().is_none());
    drop(first);
    assert!(export.as_mut().now_or_never().is_none());
    drop(second);
    let result = export.await.unwrap();
    assert_eq!(*result.evidence(), 1);
    assert_eq!(values(&reopened(&session, result.root()).await).await, ["first"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn competing_exports_publish_once_without_mixing_independent_sessions() {
    let first = Arc::new(session().await);
    let second = Arc::new(
        Session::new(
            first.space.get_history_repository(),
            Arc::new(Box::new(Matcher)),
            Epoch::default(),
        )
        .unwrap(),
    );
    put(&first, "first").await;
    put(&second, "second").await;
    let start = Arc::new(tokio::sync::Barrier::new(4));
    let mut workers = Vec::new();
    for session in [first.clone(), first.clone(), second.clone()] {
        let start = start.clone();
        workers.push(tokio::spawn(async move {
            start.wait().await;
            session.export().await
        }));
    }
    start.wait().await;
    let left = workers.remove(0).await.unwrap();
    let right = workers.remove(0).await.unwrap();
    assert_ne!(left.is_ok(), right.is_ok());
    let first_export = left.or(right).unwrap();
    let second_export = workers.remove(0).await.unwrap().unwrap();
    assert_ne!(first_export.root(), second_export.root());
    assert_eq!(values(&reopened(&first, first_export.root()).await).await, ["first"]);
    assert_eq!(values(&reopened(&second, second_export.root()).await).await, ["second"]);
}

#[tokio::test]
async fn persistence_panic_cannot_publish_evidence_or_reopen_the_session() {
    let session = session().await;
    put(&session, "first").await;
    let history = session.space.get_history_repository().history();
    assert!(
        std::panic::catch_unwind(AssertUnwindSafe(|| {
            let _guard = history.lock().unwrap();
            panic!("injected history failure");
        }))
        .is_err()
    );
    assert!(
        AssertUnwindSafe(session.export())
            .catch_unwind()
            .await
            .is_err()
    );
    assert!(session.epoch.state.lock().unwrap().3);
    assert!(session.export().await.is_err());
    assert!(session.checkpoint().await.is_err());
    assert!(session.completed_evidence().await.is_err());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn exported_state_and_ledger_match_the_retained_history(
        prefix in prop::collection::vec((0_u8..5, "[a-z]{0,12}"), 0..12),
        suffix in prop::collection::vec((0_u8..5, "[a-z]{0,12}"), 0..12),
        restore in any::<bool>(),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let session = session().await;
            for (channel, value) in &prefix { put_at(&session, &channel.to_string(), value).await; }
            let checkpoint = session.checkpoint().await.unwrap();
            for (channel, value) in &suffix { put_at(&session, &channel.to_string(), value).await; }
            if restore { session.restore(checkpoint).await.unwrap(); }
            let retained = if restore { prefix.len() } else { prefix.len() + suffix.len() } as u64;
            let result = session.export().await.unwrap();
            assert_eq!(*result.evidence(), retained * (retained + 1) / 2);
            let reader = reopened(&session, result.root()).await;
            for channel in 0_u8..5 {
                let mut expected: Vec<_> = prefix.iter().chain(suffix.iter().filter(|_| !restore))
                    .filter(|(key, _)| *key == channel).map(|(_, value)| value.clone()).collect();
                let mut actual: Vec<_> = reader.get_data(&channel.to_string()).await.unwrap().into_iter()
                    .map(|datum| datum.a).collect();
                expected.sort(); actual.sort();
                assert_eq!(actual, expected);
            }
        });
    }
}
