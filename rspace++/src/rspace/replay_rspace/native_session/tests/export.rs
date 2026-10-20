use std::panic::AssertUnwindSafe;

use super::*;
use crate::rspace::errors::HistoryError;
use crate::rspace::hot_store_action::{
    DeleteAction, DeleteContinuations, DeleteData, DeleteJoins, HotStoreAction, InsertAction,
    InsertContinuations, InsertData, InsertJoins,
};

fn checkpoint_actions() -> Vec<HotStoreAction<String, String, String, String>> {
    let data_channel = "data".to_owned();
    let join_channel = "join".to_owned();
    let continuation_channels = vec!["left".to_owned(), "right".to_owned()];
    let patterns = vec!["first".to_owned(), "second".to_owned()];
    let continuation = "body".to_owned();
    vec![
        HotStoreAction::Insert(InsertAction::InsertData(InsertData {
            channel: data_channel.clone(),
            data: vec![
                Datum::create(&data_channel, "z".to_owned(), false),
                Datum::create(&data_channel, "a".to_owned(), true),
            ],
        })),
        HotStoreAction::Insert(InsertAction::InsertJoins(InsertJoins {
            channel: join_channel,
            joins: vec![vec!["right".to_owned()], vec!["left".to_owned()]],
        })),
        HotStoreAction::Insert(InsertAction::InsertContinuations(InsertContinuations {
            channels: continuation_channels.clone(),
            continuations: vec![WaitingContinuation::create(
                &continuation_channels,
                &patterns,
                &continuation,
                false,
                Default::default(),
            )],
        })),
        HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
            channel: "absent data".to_owned(),
        })),
        HotStoreAction::Delete(DeleteAction::DeleteJoins(DeleteJoins {
            channel: "absent joins".to_owned(),
        })),
        HotStoreAction::Delete(DeleteAction::DeleteContinuations(DeleteContinuations {
            channels: vec!["absent continuation".to_owned()],
        })),
    ]
}

/// Changed by DR-131 S6: the test was
/// `empty_native_checkpoint_prepays_repository_clone`. The allocation and
/// cut checks went with host work. The test keeps its check that an empty
/// checkpoint keeps the root.
#[tokio::test]
async fn empty_native_checkpoint_keeps_the_root() {
    let session = session().await;
    let history = session.space.get_history_repository();
    let original = history.root();
    let prepared = history
        .prepare_native_checkpoint(Vec::new())
        .expect("an empty preparation");
    assert_eq!(history.commit_native_checkpoint(prepared).unwrap().root(), original);
    assert_eq!(history.root(), original);
}

/// Changed by DR-131 S6: the test was
/// `native_checkpoint_preparation_matches_legacy_and_rejects_every_short_cut`.
/// The cut checks went with host work. The test keeps its checks that the
/// preparation publishes nothing and that the commit equals the legacy
/// checkpoint.
#[tokio::test]
async fn native_checkpoint_preparation_matches_legacy() {
    let native_session = session().await;
    let history = native_session.space.get_history_repository();
    let legacy_session = session().await;
    let legacy_history = legacy_session.space.get_history_repository();
    let actions = checkpoint_actions();
    let original = history.root();
    let prepared = history.prepare_native_checkpoint(actions.clone()).unwrap();
    assert_eq!(prepared.cold_actions.len(), 3);
    let staged_root = prepared.prepared_history.as_ref().unwrap().next.root();
    assert_ne!(staged_root, original);
    assert_eq!(history.root(), original);
    let legacy = legacy_history.checkpoint(actions);
    let native = history.commit_native_checkpoint(prepared).unwrap();
    assert_eq!(native.root(), staged_root);
    assert_eq!(native.root(), legacy.root());
    assert_ne!(native.root(), original);
    let native_reader = native.get_history_reader(&native.root()).unwrap();
    let legacy_reader = legacy.get_history_reader(&legacy.root()).unwrap();
    assert_eq!(
        native_reader.get_data_proj_generic(&"data".to_owned()),
        legacy_reader.get_data_proj_generic(&"data".to_owned())
    );
    assert_eq!(
        native_reader.get_joins_proj_generic(&"join".to_owned()),
        legacy_reader.get_joins_proj_generic(&"join".to_owned())
    );
    assert_eq!(
        native_reader.get_continuations_proj_generic(&vec!["left".to_owned(), "right".to_owned()]),
        legacy_reader.get_continuations_proj_generic(&vec!["left".to_owned(), "right".to_owned()])
    );
}

/// Changed by DR-131 S6: the test was
/// `no_op_native_checkpoint_reserves_root_copies_before_publication`. The
/// charge and cut checks went with host work. The test keeps its checks that
/// a no-op checkpoint stages and commits the original root.
#[tokio::test]
async fn no_op_native_checkpoint_keeps_the_root() {
    let session = session().await;
    let history = session.space.get_history_repository();
    let original = history.root();
    let actions = vec![HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
        channel: "absent".to_owned(),
    }))];
    let prepared = history.prepare_native_checkpoint(actions).unwrap();
    assert_eq!(prepared.prepared_history.as_ref().unwrap().next.root(), original);
    assert_eq!(history.root(), original);
    assert_eq!(history.commit_native_checkpoint(prepared).unwrap().root(), original);
}

#[tokio::test]
async fn duplicate_history_keys_reject_before_publication() {
    let session = session().await;
    let history = session.space.get_history_repository();
    let original = history.root();
    let duplicate = HotStoreAction::Delete(DeleteAction::DeleteData(DeleteData {
        channel: "same".to_owned(),
    }));
    let result = history.prepare_native_checkpoint(vec![duplicate.clone(), duplicate]);
    assert!(matches!(result, Err(RSpaceError::HistoryError(HistoryError::ActionError(_)))));
    assert_eq!(history.root(), original);
}

#[tokio::test]
async fn prepared_history_rejects_a_foreign_repository_before_writes() {
    let source = session().await;
    let target = session().await;
    let source_history = source.space.get_history_repository();
    let target_history = target.space.get_history_repository();
    let original = source_history.root();
    let actions = checkpoint_actions();
    let prepared = source_history
        .prepare_native_checkpoint(actions.clone())
        .unwrap();
    assert!(matches!(
        target_history.commit_native_checkpoint(prepared),
        Err(RSpaceError::InterpreterError(_))
    ));
    assert_eq!(source_history.root(), original);
    assert_eq!(target_history.root(), original);
    let empty = source_history
        .prepare_native_checkpoint(Vec::new())
        .unwrap();
    assert!(matches!(
        target_history.commit_native_checkpoint(empty),
        Err(RSpaceError::InterpreterError(_))
    ));
    let prepared = source_history
        .prepare_native_checkpoint(actions.clone())
        .unwrap();
    let reset = source_history.reset(&original).unwrap();
    assert!(matches!(
        reset.commit_native_checkpoint(prepared),
        Err(RSpaceError::InterpreterError(_))
    ));
    let prepared = source_history.prepare_native_checkpoint(actions).unwrap();
    assert_ne!(
        source_history
            .commit_native_checkpoint(prepared)
            .unwrap()
            .root(),
        original
    );
}

#[tokio::test]
async fn native_checkpoint_rejects_a_changed_prepared_root_before_publication() {
    let session = session().await;
    let history = session.space.get_history_repository();
    let original = history.root();
    let mut prepared = history
        .prepare_native_checkpoint(checkpoint_actions())
        .unwrap();
    let correct = prepared.root.clone().unwrap();
    let forged = Blake2b256Hash::new(b"different prepared root");
    prepared.root = Some(forged.clone());
    assert!(matches!(
        history.commit_native_checkpoint(prepared),
        Err(RSpaceError::InterpreterError(_))
    ));
    assert_eq!(history.root(), original);
    assert!(!history.contains_root(&correct).unwrap());
    assert!(!history.contains_root(&forged).unwrap());
}

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
async fn empty_export_returns_the_original_root_and_closes_the_session() {
    let session = session().await;
    let original = session.space.get_history_repository().root();
    let export = session.export().await.unwrap();
    assert_eq!(export.root(), &original);
    assert_eq!(*export.evidence(), 0);
    assert!(session.export().await.is_err());
    assert!(session.close().await.is_err());
}

/// Changed by DR-131 S6: the test was
/// `incomplete_or_host_rejected_export_preserves_the_live_session`. The
/// host-work rejection went with host work. The test keeps its checks that
/// an incomplete export preserves the live session.
#[tokio::test]
async fn incomplete_export_preserves_the_live_session() {
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
                    .map(|datum| datum.a.as_ref().clone()).collect();
                expected.sort(); actual.sort();
                assert_eq!(actual, expected);
            }
        });
    }
}
