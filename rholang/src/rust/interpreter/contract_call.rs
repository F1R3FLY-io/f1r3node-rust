use std::pin::Pin;
use std::sync::Arc;

use models::rhoapi::{ListParWithRandom, Par};
use rspace_plus_plus::rspace::rspace_interface::ProduceCommitGuard;

use super::dispatch::{decode_non_deterministic_output, DispatchType, RhoDispatch};
use super::errors::InterpreterError;
use super::rho_runtime::RhoISpace;

/**
 * This is a tool for unapplying the messages sent to the system contracts.
 *
 * The unapply returns (Producer, Seq[Par]).
 *
 * The Producer is the function with the signature (Seq[Par], Par) => F[Unit] which can be used to send a message
 * through a channel. The first argument with type Seq[Par] is the content of the message and the second argument is
 * the channel.
 *
 * Note that the random generator and the sequence number extracted from the incoming message are required for sending
 * messages back to the caller so they are given as the first argument list to the produce function.
 *
 * The Seq[Par] returned by unapply contains the message content and can be further unapplied as needed to match the
 * required signature.
 *
 * @param space the rspace instance
 * @param dispatcher the dispatcher
 *
 * See rholang/src/main/scala/coop/rchain/rholang/interpreter/ContractCall.scala
 */
pub struct ContractCall {
    pub space: RhoISpace,
    pub dispatcher: RhoDispatch,
}

pub type Producer = Box<
    dyn FnOnce(
            &[Par],
            &Par,
        )
            -> Pin<Box<dyn futures::Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
        + Send,
>;

pub type OwnedProducer = Box<
    dyn FnOnce(
            Vec<Par>,
            Par,
            Option<Arc<dyn ProduceCommitGuard>>,
        )
            -> Pin<Box<dyn futures::Future<Output = Result<Vec<Par>, InterpreterError>> + Send>>
        + Send,
>;

impl ContractCall {
    pub fn unapply(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Option<(Producer, bool, Vec<Par>, Vec<Par>)> {
        self.unapply_with_guard(contract_args, None)
    }

    pub fn unapply_guarded(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        guard: Arc<dyn ProduceCommitGuard>,
    ) -> Option<(Producer, bool, Vec<Par>, Vec<Par>)> {
        self.unapply_with_guard(contract_args, Some(guard))
    }

    fn unapply_with_guard(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
        guard: Option<Arc<dyn ProduceCommitGuard>>,
    ) -> Option<(Producer, bool, Vec<Par>, Vec<Par>)> {
        let (produce, is_replay, previous, args) = self.unapply_owned(contract_args)?;
        let borrowed: Producer =
            Box::new(move |values, channel| produce(values.to_vec(), channel.clone(), guard));
        Some((borrowed, is_replay, previous, args))
    }

    pub fn unapply_owned(
        &self,
        contract_args: (Vec<ListParWithRandom>, bool, Vec<Par>),
    ) -> Option<(OwnedProducer, bool, Vec<Par>, Vec<Par>)> {
        let (mut messages, is_replay, previous) = contract_args;
        if messages.len() == 1 {
            let ListParWithRandom {
                pars: args,
                random_state: rand,
            } = messages.pop()?;

            let space = self.space.clone();
            let dispatcher = self.dispatcher.clone();
            let produce: OwnedProducer = Box::new(move |values, channel, guard| {
                Box::pin(async move {
                    let payload = ListParWithRandom {
                        pars: values,
                        random_state: rand,
                    };
                    let produce_result = match guard.as_deref() {
                        Some(guard) => {
                            space
                                .produce_guarded(channel, payload, false, guard)
                                .await?
                        }
                        None => space.produce(channel, payload, false).await?,
                    };

                    let is_replay = space.is_replay().await;

                    let dispatch_result = match produce_result {
                        Some((cont, channels, produce)) => {
                            dispatcher
                                .dispatch(
                                    cont.continuation,
                                    channels.iter().map(|c| c.matched_datum.clone()).collect(),
                                    is_replay,
                                    // ★★ THE SAME CONSENSUS-CLASS `Par` READ as
                                    // `reduce.rs`'s `continue_produce_process` — the
                                    // system-contract path's copy of it. It is now the
                                    // same FUNCTION, not merely the same rule written
                                    // out again: the ceiling, its arithmetic, the
                                    // replay-failure path and the reachability finding
                                    // all live at `dispatch::decode_non_deterministic_output`,
                                    // next to the `dispatch_type` encoder that produced
                                    // these bytes.
                                    decode_non_deterministic_output(&produce.output_value)?,
                                    // System-contract producer: outside the deploy's parallel tree, so
                                    // its continuation eval starts at the empty coordinate.
                                    smallvec::SmallVec::new(),
                                )
                                .await
                        }

                        None => Ok(DispatchType::Skip),
                    };

                    match dispatch_result {
                        Ok(dispatch_type) => match dispatch_type {
                            // ★ The RETURN leg of the same ceiling, and the one
                            // place it runs on the PLAY path: these bytes were
                            // produced by `dispatch_type`'s `encode_to_vec` a
                            // few frames below, so this is an encode→decode
                            // round trip with no wire between its ends.
                            //
                            // ⚠ It is reached only when producing the operation's
                            // result onto its `ack` channel fires a continuation
                            // that is ITSELF a non-deterministic system process
                            // (`ack` bound to a system channel of matching
                            // arity). For the ordinary `ParBody` ack the arm is
                            // `DeterministicCall` and nothing is decoded — which
                            // is why the read ceiling's first encounter with real
                            // bytes is on the validator, not here.
                            //
                            // Same function as every other member of the class;
                            // see `dispatch::decode_non_deterministic_output` for
                            // the ceiling and the write-side reachability finding
                            // (`rho:ollama:models` at depth 1 is the deepest any
                            // registered operation returns, so this decode has
                            // thirty-two levels of headroom).
                            DispatchType::NonDeterministicCall(items) => {
                                decode_non_deterministic_output(&items)
                            }
                            DispatchType::FailedNonDeterministicCall(e) => Err(e),
                            DispatchType::DeterministicCall => Ok(Vec::new()),
                            DispatchType::Skip => Ok(Vec::new()),
                        },
                        Err(e) => Err(e),
                    }
                })
                    as Pin<
                        Box<
                            dyn futures::Future<Output = Result<Vec<Par>, InterpreterError>> + Send,
                        >,
                    >
            });

            Some((produce, is_replay, previous, args))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, HashMap};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{OnceLock, RwLock};

    use models::rhoapi::tagged_continuation::TaggedCont;
    use models::rhoapi::{BindPattern, TaggedContinuation};
    use models::rust::utils::{new_freevar_par, new_gint_par, new_gstring_par};
    use rspace_plus_plus::rspace::errors::RSpaceError;
    use rspace_plus_plus::rspace::rspace::RSpace;
    use rspace_plus_plus::rspace::shared::in_mem_store_manager::InMemoryStoreManager;
    use rspace_plus_plus::rspace::shared::key_value_store_manager::KeyValueStoreManager;

    use super::*;
    use crate::rust::interpreter::dispatch::RholangAndScalaDispatcher;
    use crate::rust::interpreter::matcher::r#match::Matcher;
    use crate::rust::interpreter::system_processes::{BodyRefs, RhoDispatchMap};

    struct Authority(Arc<RwLock<bool>>, Arc<AtomicUsize>);

    impl ProduceCommitGuard for Authority {
        fn with_commit(&self, commit: Box<dyn FnOnce() + '_>) -> Result<(), RSpaceError> {
            self.1.fetch_add(1, Ordering::SeqCst);
            let live = self.0.read().expect("authority read");
            if !*live {
                return Err(RSpaceError::ProduceCommitDenied);
            }
            commit();
            drop(live);
            Ok(())
        }
    }

    async fn fixture() -> ContractCall {
        let mut manager = InMemoryStoreManager::new();
        let stores = manager.r_space_stores().await.expect("in-memory stores");
        let space = RSpace::<Par, BindPattern, ListParWithRandom, TaggedContinuation>::create(
            stores,
            Arc::new(Box::new(Matcher)),
        )
        .expect("space");
        let table: RhoDispatchMap = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        ContractCall {
            space: Arc::new(Box::new(space)),
            dispatcher: Arc::new(RholangAndScalaDispatcher {
                _dispatch_table: table,
                reducer: Arc::new(OnceLock::new()),
            }),
        }
    }

    #[tokio::test]
    async fn guarded_producer_checks_at_await_and_dispatches_without_authority_lock() {
        check_guarded_producer(false).await;
    }

    #[tokio::test]
    async fn owned_guarded_producer_checks_at_await_and_dispatches_without_authority_lock() {
        check_guarded_producer(true).await;
    }

    async fn check_guarded_producer(owned: bool) {
        for matched in [false, true] {
            let call = fixture().await;
            let live = Arc::new(RwLock::new(true));
            let invocations = Arc::new(AtomicUsize::new(0));
            let guard = Arc::new(Authority(live.clone(), invocations.clone()));
            let received = Arc::new(AtomicUsize::new(0));
            let channel = new_gstring_par("guarded-reply".to_owned(), Vec::new(), false);
            let value = new_gint_par(42, Vec::new(), false);
            let random_state = vec![7; 32];
            if matched {
                let callback_live = live.clone();
                let callback_received = received.clone();
                let expected_value = value.clone();
                let expected_random = random_state.clone();
                call.dispatcher._dispatch_table.write().await.insert(
                    777,
                    Box::new(move |args| {
                        let live = callback_live.clone();
                        let received = callback_received.clone();
                        let value = expected_value.clone();
                        let random = expected_random.clone();
                        Box::pin(async move {
                            *live
                                .try_write()
                                .expect("receiver must run after authority release") = false;
                            assert_eq!(args.0.len(), 1);
                            assert_eq!(args.0[0].pars, vec![value]);
                            assert_eq!(args.0[0].random_state, random);
                            received.fetch_add(1, Ordering::SeqCst);
                            Ok(Vec::new())
                        })
                    }),
                );
                assert!(call
                    .space
                    .consume(
                        vec![channel.clone()],
                        vec![BindPattern {
                            patterns: vec![new_freevar_par(0, Vec::new())],
                            remainder: None,
                            free_count: 1,
                        }],
                        TaggedContinuation {
                            tagged_cont: Some(TaggedCont::ScalaBodyRef(777)),
                            guard: None,
                        },
                        false,
                        BTreeSet::new()
                    )
                    .await
                    .expect("waiting reply receiver")
                    .is_none());
            }
            let input = (
                vec![ListParWithRandom {
                    pars: Vec::new(),
                    random_state: random_state.clone(),
                }],
                false,
                Vec::new(),
            );
            let prepare = || {
                if owned {
                    let (producer, _, _, _) =
                        call.unapply_owned(input.clone()).expect("owned producer");
                    producer(vec![value.clone()], channel.clone(), Some(guard.clone()))
                } else {
                    let (producer, _, _, _) = call
                        .unapply_guarded(input.clone(), guard.clone())
                        .expect("producer");
                    producer(std::slice::from_ref(&value), &channel)
                }
            };
            let unpolled = prepare();
            assert_eq!(invocations.load(Ordering::SeqCst), 0);
            drop(unpolled);
            assert_eq!(invocations.load(Ordering::SeqCst), 0);
            assert!(call.space.get_data(&channel).await.is_empty());
            let future = prepare();
            *live.write().expect("revoke after future creation") = false;
            assert!(matches!(
                future.await,
                Err(InterpreterError::RSpaceError(
                    RSpaceError::ProduceCommitDenied
                ))
            ));
            assert_eq!(invocations.load(Ordering::SeqCst), 1);
            assert_eq!(received.load(Ordering::SeqCst), 0);
            assert!(call.space.get_data(&channel).await.is_empty());
            assert_eq!(
                call.space
                    .get_waiting_continuations(vec![channel.clone()])
                    .await
                    .len(),
                usize::from(matched)
            );

            *live.write().expect("restore authority") = true;
            assert!(prepare().await.expect("authorized reply").is_empty());
            assert_eq!(invocations.load(Ordering::SeqCst), 2);
            assert_eq!(received.load(Ordering::SeqCst), usize::from(matched));
            let stored = call.space.get_data(&channel).await;
            if matched {
                assert!(stored.is_empty());
                assert!(!*live.read().expect("receiver revoked authority"));
            } else {
                assert_eq!(stored.len(), 1);
                assert_eq!(stored[0].a.pars, vec![value]);
                assert_eq!(stored[0].a.random_state, random_state);
            }
        }
    }

    #[tokio::test]
    async fn owned_split_moves_complete_arguments_and_rejects_only_outer_arity() {
        let call = fixture().await;
        for length in [0, 1, 3] {
            let args = vec![new_gint_par(17, Vec::new(), false); length];
            let previous = vec![new_gint_par(23, Vec::new(), false); 2];
            let args_pointer = args.as_ptr();
            let previous_pointer = previous.as_ptr();
            let expected_args = args.clone();
            let expected_previous = previous.clone();
            let (producer, replay, previous, args) = call
                .unapply_owned((
                    vec![ListParWithRandom {
                        pars: args,
                        random_state: vec![3; 32],
                    }],
                    true,
                    previous,
                ))
                .expect("one message");
            assert!(replay);
            assert_eq!(args, expected_args);
            assert_eq!(previous, expected_previous);
            assert_eq!(args.as_ptr(), args_pointer);
            assert_eq!(previous.as_ptr(), previous_pointer);
            drop(producer);
        }
        for length in [0, 2] {
            let messages = vec![
                ListParWithRandom {
                    pars: Vec::new(),
                    random_state: vec![3; 32]
                };
                length
            ];
            assert!(call.unapply_owned((messages, false, Vec::new())).is_none());
        }
    }

    #[tokio::test]
    async fn owned_and_borrowed_ordinary_producers_preserve_payload_and_random_state() {
        for owned in [false, true] {
            let call = fixture().await;
            let values = vec![
                new_gint_par(8, Vec::new(), false),
                new_gint_par(7, Vec::new(), false),
                new_gint_par(8, Vec::new(), false),
            ];
            let expected = values.clone();
            let channel = new_gstring_par("owned-ordinary".into(), Vec::new(), false);
            let query = channel.clone();
            let random_state: Vec<u8> = (0..32).collect();
            let args = (
                vec![ListParWithRandom {
                    pars: Vec::new(),
                    random_state: random_state.clone(),
                }],
                true,
                Vec::new(),
            );
            let output = if owned {
                let (produce, replay, _, _) = call.unapply_owned(args).unwrap();
                assert!(replay);
                produce(values, channel, None).await.unwrap()
            } else {
                let (produce, replay, _, _) = call.unapply(args).unwrap();
                assert!(replay);
                produce(&values, &channel).await.unwrap()
            };
            assert!(output.is_empty());
            let stored = call.space.get_data(&query).await;
            assert_eq!(stored.len(), 1);
            assert_eq!(stored[0].a.pars, expected);
            assert_eq!(stored[0].a.random_state, random_state);
            assert!(!stored[0].persist);
        }
    }

    #[tokio::test]
    async fn owned_and_borrowed_dispatch_preserve_replay_and_all_result_arms() {
        for owned in [false, true] {
            for nondeterministic in [false, true] {
                for fails in [false, true] {
                    let call = fixture().await;
                    let channel = new_gstring_par("owned-callback".into(), Vec::new(), false);
                    let body_ref = if nondeterministic {
                        BodyRefs::GPT4
                    } else {
                        777
                    };
                    let value = new_gint_par(42, Vec::new(), false);
                    let values = vec![
                        value.clone(),
                        value.clone(),
                        new_gint_par(91, Vec::new(), false),
                    ];
                    let returned = values.clone();
                    let random = vec![9; 32];
                    let callback_random = random.clone();
                    let callback_value = value.clone();
                    let invocations = Arc::new(AtomicUsize::new(0));
                    let callback_count = invocations.clone();
                    call.dispatcher._dispatch_table.write().await.insert(
                        body_ref,
                        Box::new(move |args| {
                            assert!(!args.1, "dispatch uses the space's replay state");
                            assert!(
                                args.2.is_empty(),
                                "previous output comes from the actual produce event"
                            );
                            assert_eq!(args.0.len(), 1);
                            assert_eq!(args.0[0].pars, vec![callback_value.clone()]);
                            assert_eq!(args.0[0].random_state, callback_random);
                            callback_count.fetch_add(1, Ordering::SeqCst);
                            let values = returned.clone();
                            Box::pin(async move {
                                if fails {
                                    Err(InterpreterError::IllegalArgumentError(
                                        "callback failure".into(),
                                    ))
                                } else {
                                    Ok(values)
                                }
                            })
                        }),
                    );
                    call.space
                        .consume(
                            vec![channel.clone()],
                            vec![BindPattern {
                                patterns: vec![new_freevar_par(0, Vec::new())],
                                remainder: None,
                                free_count: 1,
                            }],
                            TaggedContinuation {
                                tagged_cont: Some(TaggedCont::ScalaBodyRef(body_ref)),
                                guard: None,
                            },
                            false,
                            BTreeSet::new(),
                        )
                        .await
                        .unwrap();
                    let previous = vec![new_gint_par(99, Vec::new(), false)];
                    let args = (
                        vec![ListParWithRandom {
                            pars: Vec::new(),
                            random_state: random,
                        }],
                        true,
                        previous.clone(),
                    );
                    let result = if owned {
                        let (produce, replay, actual_previous, _) =
                            call.unapply_owned(args).unwrap();
                        assert!(replay);
                        assert_eq!(actual_previous, previous);
                        produce(vec![value], channel, None).await
                    } else {
                        let (produce, replay, actual_previous, _) = call.unapply(args).unwrap();
                        assert!(replay);
                        assert_eq!(actual_previous, previous);
                        produce(&[value], &channel).await
                    };
                    assert_eq!(invocations.load(Ordering::SeqCst), 1);
                    match result {
                        Err(InterpreterError::IllegalArgumentError(ref message)) => {
                            assert!(fails);
                            assert_eq!(message, "callback failure");
                        }
                        Ok(output) => {
                            assert!(!fails);
                            assert_eq!(output, if nondeterministic { values } else { Vec::new() });
                        }
                        other => panic!("unexpected dispatch outcome {other:?}"),
                    }
                }
            }
        }
    }
}
