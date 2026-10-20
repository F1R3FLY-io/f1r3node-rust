use super::*;
use crate::rspace::trace::event::Consume;

struct EmptyReader;

impl HistoryReaderBase<String, String, String, String> for EmptyReader {
    fn get_data_proj(
        &self,
        _: &String,
    ) -> Result<Vec<Datum<String>>, crate::rspace::errors::RSpaceError> {
        Ok(vec![])
    }
    fn get_continuations_proj(
        &self,
        _: &Vec<String>,
    ) -> Result<Vec<WaitingContinuation<String, String>>, crate::rspace::errors::RSpaceError> {
        Ok(vec![])
    }
    fn get_joins_proj(
        &self,
        _: &String,
    ) -> Result<Vec<Vec<String>>, crate::rspace::errors::RSpaceError> {
        Ok(vec![])
    }
}

type Store = Box<dyn HotStore<String, String, String, String>>;

fn store() -> Store { HotStoreInstances::create_from_hr(Box::new(EmptyReader)) }

struct Authorization(bool);

impl RestingRetirementAuthorization for Authorization {
    fn authorize(&mut self) -> Result<(), RSpaceError> {
        if self.0 {
            Ok(())
        } else {
            Err(RSpaceError::InterpreterError("authorization rejected".into()))
        }
    }
}

#[test]
fn matched_produce_retirement_preserves_persistent_and_installed_joins() {
    let channels = vec!["left".repeat(25), "right".repeat(25)];
    let data = channels
        .iter()
        .cloned()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    for persistent in [false, true] {
        for installed in [false, true] {
            let prepare = || {
                let store = store();
                let waiting = WaitingContinuation {
                    patterns: vec!["pattern".to_owned(); 2].into(),
                    continuation: "body".repeat(30).into(),
                    persist: persistent,
                    peeks: Default::default(),
                    source: Consume::default(),
                };
                if installed {
                    store.install_continuation(&channels, waiting.clone());
                }
                store
                    .put_continuation(&channels, waiting)
                    .expect("the in-memory hot store accepts the continuation");
                for channel in &channels {
                    store
                        .put_datum(channel, Datum::create(channel, "value".repeat(30), false))
                        .expect("the in-memory hot store accepts the datum");
                    store
                        .put_join(channel, &channels)
                        .expect("the in-memory hot store accepts the join");
                }
                store
            };
            let baseline = prepare();
            let legacy = prepare();
            let mut authorization = Authorization(true);
            baseline
                .retire_resting_match(
                    &channels,
                    i32::from(installed),
                    persistent,
                    &data,
                    &[(0, 0), (1, 0)],
                    &mut authorization,
                )
                .unwrap();
            for channel in &channels {
                legacy.remove_datum(channel, 0).unwrap();
            }
            if !persistent {
                legacy
                    .remove_continuation(&channels, i32::from(installed))
                    .unwrap();
            }
            for channel in &channels {
                legacy.remove_join(channel, &channels).unwrap();
            }
            let state = baseline.snapshot();
            let expected = legacy.snapshot();
            assert_eq!(state.data_flat(), expected.data_flat());
            assert_eq!(state.continuations_flat(), expected.continuations_flat());
            assert_eq!(state.joins_flat(), expected.joins_flat());
            assert_eq!(
                state.installed_continuations_flat(),
                expected.installed_continuations_flat()
            );
        }
    }
}

#[test]
fn installed_only_persistent_retirement_needs_no_ordinary_continuation_row() {
    let channel = "installed-only".repeat(4);
    let channels = vec![channel.clone()];
    let prepare = || {
        let store = store();
        let waiting = WaitingContinuation {
            patterns: vec!["pattern".to_owned()].into(),
            continuation: "body".to_owned().into(),
            persist: true,
            peeks: Default::default(),
            source: Consume::default(),
        };
        store.install_continuation(&channels, waiting);
        store.install_join(&channel, &channels);
        store
            .put_datum(&channel, Datum::create(&channel, "payload".to_owned(), false))
            .unwrap();
        store
    };
    let data = vec![RSpaceResult {
        channel: channel.clone(),
        matched_datum: String::new(),
        removed_datum: String::new(),
        persistent: false,
    }];
    let baseline = prepare();
    let mut authorization = Authorization(true);
    baseline
        .retire_resting_match(&channels, 0, true, &data, &[(0, 0)], &mut authorization)
        .unwrap();
    let state = baseline.snapshot();
    assert!(state.data_flat().get(&channel).is_none_or(Vec::is_empty));
    assert!(
        state
            .continuations_flat()
            .get(&channels)
            .is_none_or(Vec::is_empty)
    );
    assert!(state.installed_continuations_flat().contains_key(&channels));
    assert!(state.installed_joins_flat().contains_key(&channel));

    let store = prepare();
    let before = store.snapshot();
    assert_eq!(
        store.retire_resting_match(&channels, 0, true, &data, &[(0, 0)], &mut Authorization(false)),
        Err(RSpaceError::InterpreterError("authorization rejected".into()))
    );
    let after = store.snapshot();
    assert_eq!(after.data_flat(), before.data_flat());
    assert_eq!(after.continuations_flat(), before.continuations_flat());
    assert_eq!(after.installed_continuations_flat(), before.installed_continuations_flat());
    assert_eq!(after.installed_joins_flat(), before.installed_joins_flat());
}

#[test]
fn matched_produce_retirement_prepares_data_continuation_and_joins_together() {
    let left = "left".repeat(35);
    let right = "right".repeat(35);
    let channels = vec![left.clone(), right.clone()];
    let waiting = WaitingContinuation {
        patterns: vec!["left pattern".to_owned(), "right pattern".to_owned()].into(),
        continuation: "body".repeat(40).into(),
        persist: false,
        peeks: Default::default(),
        source: Consume::default(),
    };
    let prepare = || {
        let store = store();
        for channel in &channels {
            store
                .put_datum(channel, Datum::create(channel, "old".repeat(70), false))
                .expect("the in-memory hot store accepts the old datum");
            store
                .put_datum(channel, Datum::create(channel, "new".repeat(70), false))
                .expect("the in-memory hot store accepts the new datum");
            store
                .put_join(channel, &channels)
                .expect("the in-memory hot store accepts the join");
        }
        store
            .put_continuation(&channels, waiting.clone())
            .expect("the in-memory hot store accepts the continuation");
        let snapshot = store.snapshot();
        (store, snapshot)
    };
    let data = channels
        .iter()
        .cloned()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    let retirement = [(0, 1), (1, 0)];
    let (baseline, original) = prepare();
    baseline
        .retire_resting_match(&channels, 0, false, &data, &retirement, &mut Authorization(true))
        .unwrap();
    let (legacy, _) = prepare();
    for (position, index) in retirement {
        legacy.remove_datum(&data[position].channel, index).unwrap();
    }
    legacy.remove_continuation(&channels, 0).unwrap();
    for datum in &data {
        legacy.remove_join(&datum.channel, &channels).unwrap();
    }
    let state = baseline.snapshot();
    let expected = legacy.snapshot();
    assert_eq!(state.data_flat(), expected.data_flat());
    assert_eq!(state.continuations_flat(), expected.continuations_flat());
    assert_eq!(state.joins_flat(), expected.joins_flat());
    assert_eq!(original.continuations_flat()[&channels].len(), 1);
    let (store, snapshot) = prepare();
    assert!(
        store
            .retire_resting_match(
                &channels,
                99,
                false,
                &data,
                &retirement,
                &mut Authorization(true),
            )
            .is_err()
    );
    let state = store.snapshot();
    assert_eq!(state.data_flat(), snapshot.data_flat());
    assert_eq!(state.continuations_flat(), snapshot.continuations_flat());
    assert_eq!(state.joins_flat(), snapshot.joins_flat());
}

// Added by the merge of feature/cost-accounted-rho (finding R2-06): a refused
// resting COMM leaves the store unchanged when no row is persistent. The
// retirement has then prepared all of its updates: one datum on each channel,
// the ordinary continuation and, without an installed continuation, the joins.
// The authorized control shows that each of these updates is real.
#[test]
fn refused_retirement_of_non_persistent_rows_changes_nothing() {
    let left = "left".repeat(30);
    let right = "right".repeat(30);
    let channels = vec![left, right];
    let data = channels
        .iter()
        .cloned()
        .map(|channel| RSpaceResult {
            channel,
            matched_datum: String::new(),
            removed_datum: String::new(),
            persistent: false,
        })
        .collect::<Vec<_>>();
    let retirement = [(0, 1), (1, 0)];
    for installed in [false, true] {
        let prepare = || {
            let store = store();
            let waiting = WaitingContinuation {
                patterns: vec!["left pattern".to_owned(), "right pattern".to_owned()].into(),
                continuation: "body".repeat(30).into(),
                persist: false,
                peeks: Default::default(),
                source: Consume::default(),
            };
            if installed {
                store.install_continuation(&channels, waiting.clone());
                for channel in &channels {
                    store.install_join(channel, &channels);
                }
            }
            store
                .put_continuation(&channels, waiting)
                .expect("the in-memory hot store accepts the continuation");
            for channel in &channels {
                for value in ["old", "new"] {
                    store
                        .put_datum(channel, Datum::create(channel, value.repeat(30), false))
                        .expect("the in-memory hot store accepts the datum");
                }
                store
                    .put_join(channel, &channels)
                    .expect("the in-memory hot store accepts the join");
            }
            store
        };

        let refused = prepare();
        let before = refused.snapshot();
        assert_eq!(
            refused.retire_resting_match(
                &channels,
                i32::from(installed),
                false,
                &data,
                &retirement,
                &mut Authorization(false),
            ),
            Err(RSpaceError::InterpreterError("authorization rejected".into())),
            "installed = {installed}"
        );
        let after = refused.snapshot();
        assert_eq!(after.data_flat(), before.data_flat(), "installed = {installed}");
        assert_eq!(
            after.continuations_flat(),
            before.continuations_flat(),
            "installed = {installed}"
        );
        assert_eq!(after.joins_flat(), before.joins_flat(), "installed = {installed}");
        assert_eq!(
            after.installed_continuations_flat(),
            before.installed_continuations_flat(),
            "installed = {installed}"
        );
        assert_eq!(
            after.installed_joins_flat(),
            before.installed_joins_flat(),
            "installed = {installed}"
        );

        let authorized = prepare();
        authorized
            .retire_resting_match(
                &channels,
                i32::from(installed),
                false,
                &data,
                &retirement,
                &mut Authorization(true),
            )
            .expect("the authorized retirement succeeds");
        let retired = authorized.snapshot();
        assert_ne!(retired.data_flat(), before.data_flat(), "installed = {installed}");
        assert_ne!(
            retired.continuations_flat(),
            before.continuations_flat(),
            "installed = {installed}"
        );
        match installed {
            false => assert_ne!(
                retired.joins_flat(),
                before.joins_flat(),
                "the last ordinary continuation takes its joins with it"
            ),
            true => assert_eq!(
                retired.joins_flat(),
                before.joins_flat(),
                "an installed continuation keeps the joins"
            ),
        }
    }
}
