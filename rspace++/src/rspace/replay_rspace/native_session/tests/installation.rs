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
