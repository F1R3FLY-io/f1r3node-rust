use super::*;

fn templates(names: &[String]) -> Vec<(Vec<String>, Install<String, String>)> {
    names
        .iter()
        .map(|name| {
            (vec![name.clone()], Install {
                patterns: vec![format!("pattern-{name}")],
                continuation: format!("continuation-{name}"),
            })
        })
        .collect()
}

async fn assert_templates(session: &Session, names: &[String]) {
    for (channels, install) in templates(names) {
        let entries = session.get_continuations(&channels).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].patterns, install.patterns);
        assert_eq!(entries[0].continuation, install.continuation);
        assert!(entries[0].persist);
        assert!(entries[0].peeks.is_empty());
        assert_eq!(session.get_joins(&channels[0]).await.unwrap(), vec![channels]);
    }
}

async fn check_initialization(names: Vec<String>) {
    let base = session().await;
    let history = base.space.get_history_repository();
    let root = history.root();
    let initialized = Session::new_with_installs(
        history.clone(),
        Arc::new(Box::new(Matcher)),
        Epoch::default(),
        templates(&names),
    )
    .unwrap();
    assert_templates(&initialized, &names).await;
    let checkpoint = initialized.checkpoint().await.unwrap();
    put_at(&initialized, "_temporary", "temporary").await;
    initialized.restore(checkpoint).await.unwrap();
    assert_templates(&initialized, &names).await;
    assert!(
        initialized
            .get_data(&"_temporary".to_owned())
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(history.root(), root);
    for accepted in 0..names.len() {
        let epoch = Epoch::default();
        *epoch.remaining_calls.lock().unwrap() = Some(accepted);
        assert!(
            Session::new_with_installs(
                history.clone(),
                Arc::new(Box::new(Matcher)),
                epoch,
                templates(&names),
            )
            .is_err()
        );
        assert_eq!(history.root(), root);
        let independent =
            Session::new(history.clone(), Arc::new(Box::new(Matcher)), Epoch::default()).unwrap();
        for name in &names {
            assert!(
                independent
                    .get_continuations(std::slice::from_ref(name))
                    .await
                    .unwrap()
                    .is_empty()
            );
            assert!(independent.get_joins(name).await.unwrap().is_empty());
        }
    }
}

#[tokio::test]
async fn private_initialization_is_complete_restorable_and_failure_atomic() {
    check_initialization(vec![]).await;
    check_initialization(vec!["first".to_owned(), "second".to_owned(), "third".to_owned()]).await;
}

#[tokio::test]
async fn private_initialization_rejects_invalid_shapes_without_publication() {
    let base = session().await;
    for (channels, patterns) in [
        (vec![], vec![]),
        (vec!["invalid".to_owned()], vec![]),
        (vec!["invalid".to_owned()], vec!["p".to_owned(), "q".to_owned()]),
    ] {
        let mut requests = templates(&["valid-first".to_owned()]);
        requests.push((channels, Install {
            patterns,
            continuation: "k".to_owned(),
        }));
        assert!(
            Session::new_with_installs(
                base.space.get_history_repository(),
                Arc::new(Box::new(Matcher)),
                Epoch::default(),
                requests,
            )
            .is_err()
        );
        assert!(
            base.get_continuations(&["valid-first".to_owned()])
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn private_initialization_rejects_matching_prestate_without_consuming_it() {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    play.produce("occupied".to_owned(), "stored".to_owned(), false)
        .await
        .unwrap();
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let root = history.root();
    assert!(
        Session::new_with_installs(
            history.clone(),
            Arc::new(Box::new(Matcher)),
            Epoch::default(),
            templates(&["valid-first".to_owned(), "occupied".to_owned()]),
        )
        .is_err()
    );
    assert_eq!(history.root(), root);
    let fresh = Session::new(history, Arc::new(Box::new(Matcher)), Epoch::default()).unwrap();
    assert!(
        fresh
            .get_continuations(&["valid-first".to_owned()])
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fresh.get_data(&"occupied".to_owned()).await.unwrap(),
        play.get_data(&"occupied".to_owned()).await
    );
}

#[tokio::test]
async fn every_installation_reservation_cut_leaves_history_and_other_sessions_unchanged() {
    let base = session().await;
    let history = base.space.get_history_repository();
    let root = history.root();
    let names = ["first".to_owned(), "second".to_owned()];
    let baseline_epoch = Epoch::default();
    let baseline = Session::new_with_installs(
        history.clone(),
        Arc::new(Box::new(Matcher)),
        baseline_epoch.clone(),
        templates(&names),
    )
    .unwrap();
    let calls = baseline_epoch.calls.load(Ordering::Relaxed);
    assert!(calls > names.len());
    assert!(baseline_epoch.bytes.load(Ordering::Relaxed) > 0);
    assert_templates(&baseline, &names).await;
    for accepted in 0..calls {
        let epoch = Epoch::default();
        *epoch.remaining_calls.lock().unwrap() = Some(accepted);
        assert!(
            Session::new_with_installs(
                history.clone(),
                Arc::new(Box::new(Matcher)),
                epoch,
                templates(&names),
            )
            .is_err(),
            "accepted={accepted}"
        );
        assert_eq!(history.root(), root);
    }
    for name in &names {
        assert!(
            base.get_continuations(std::slice::from_ref(name))
                .await
                .unwrap()
                .is_empty()
        );
        assert!(base.get_joins(name).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn repeated_channel_installations_preserve_greedy_matching_and_join_deduplication() {
    let mut stores = InMemoryStoreManager::new();
    let (play, _) = RSpace::<String, String, String, String>::create_with_replay(
        stores.r_space_stores().await.unwrap(),
        Arc::new(Box::new(Matcher)),
    )
    .unwrap();
    let channel = "occupied".to_owned();
    play.produce(channel.clone(), "one".to_owned(), false)
        .await
        .unwrap();
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let channels = vec![channel.clone(), channel.clone()];
    let install = Install {
        patterns: vec!["a".to_owned(), "b".to_owned()],
        continuation: "body".to_owned(),
    };
    let initialized = Session::new_with_installs(
        history.clone(),
        Arc::new(Box::new(Matcher)),
        Epoch::default(),
        vec![(channels.clone(), install.clone())],
    )
    .unwrap();
    assert_eq!(
        initialized
            .get_continuations(&channels)
            .await
            .unwrap()
            .len(),
        1
    );
    assert_eq!(initialized.get_joins(&channel).await.unwrap(), vec![channels.clone()]);
    assert_eq!(initialized.get_data(&channel).await.unwrap().len(), 1);
    play.produce(channel.clone(), "two".to_owned(), false)
        .await
        .unwrap();
    play.create_checkpoint().await.unwrap();
    let history = play.get_history_repository();
    let root = history.root();
    assert!(
        Session::new_with_installs(
            history.clone(),
            Arc::new(Box::new(Matcher)),
            Epoch::default(),
            vec![(channels, install)],
        )
        .is_err()
    );
    assert_eq!(history.root(), root);
}

#[tokio::test]
async fn later_installation_replaces_the_same_continuation_key() {
    let base = session().await;
    let history = base.space.get_history_repository();
    let channel = "same".to_owned();
    let initialized =
        Session::new_with_installs(history, Arc::new(Box::new(Matcher)), Epoch::default(), vec![
            (vec![channel.clone()], Install {
                patterns: vec!["first".to_owned()],
                continuation: "first body".to_owned(),
            }),
            (vec![channel.clone()], Install {
                patterns: vec!["second".to_owned()],
                continuation: "second body".to_owned(),
            }),
        ])
        .unwrap();
    let continuations = initialized
        .get_continuations(&[channel.clone()])
        .await
        .unwrap();
    assert_eq!(continuations.len(), 1);
    assert_eq!(continuations[0].patterns, ["second"]);
    assert_eq!(continuations[0].continuation, "second body");
    assert_eq!(initialized.get_joins(&channel).await.unwrap(), vec![vec![channel]]);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]
    #[test]
    fn generated_private_initializations_preserve_all_templates_and_isolate_failures(
        names in prop::collection::btree_set("[a-z]{1,10}", 0..12),
    ) {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(
            check_initialization(names.into_iter().collect())
        );
    }
}
