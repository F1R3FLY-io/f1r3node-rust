#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_completion.rs"]
mod startup_completion;
#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_snapshot_lease.rs"]
mod startup_snapshot_lease;
#[path = "../../../../casper/src/rust/blocks/block_processing_queue/startup_owner.rs"]
mod startup_owner;

mod engine {
    pub trait Engine: Send + Sync {}
    struct Noop;
    impl Engine for Noop {}
    pub fn noop() -> impl Engine { Noop }
}

mod rust {
    pub mod errors {
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum CasperError {
            RuntimeError(String),
        }

        impl From<crate::startup_owner::StartupError<Self>> for CasperError {
            fn from(error: crate::startup_owner::StartupError<Self>) -> Self {
                Self::RuntimeError(format!("{error:?}"))
            }
        }
    }

    pub mod blocks {
        pub mod block_processing_queue {
            use std::sync::Arc;

            use crate::rust::errors::CasperError;
            use crate::startup_owner::{PreparedStartupContext, StartupOwner, StartupRegistration};

            pub type Owner = StartupOwner<usize, (), CasperError>;
            pub type PreparedRecoveryContext = PreparedStartupContext<usize, (), CasperError>;
            pub type RecoveryRegistration = StartupRegistration<usize, (), CasperError>;

            #[derive(Default)]
            pub struct RecoveryControl {
                startup: Arc<Owner>,
            }

            impl RecoveryControl {
                pub fn startup(&self) -> Arc<Owner> { self.startup.clone() }
            }
        }
    }
}

mod engine_cell {
    include!("../../../../casper/src/rust/engine/engine_cell.rs");

    #[cfg(test)]
    mod publication_tests {
        use std::future::{poll_fn, Future};
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::sync::Barrier;
        use std::task::Poll;

        use super::*;

        struct EngineProbe {
            cell: EngineCell,
            recovery: Arc<RecoveryControl>,
            dropped: Arc<AtomicBool>,
        }

        impl Engine for EngineProbe {}

        impl Drop for EngineProbe {
            fn drop(&mut self) {
                assert!(
                    self.cell.inner.try_read().is_ok(),
                    "engine destroyed under engine guard"
                );
                assert!(
                    self.recovery.startup().state_is_unlocked_for_test(),
                    "engine destroyed under controller guard"
                );
                self.dropped.store(true, Ordering::SeqCst);
            }
        }

        #[tokio::test]
        async fn non_running_replacement_revokes_registration_despite_old_engine_readers() {
            let cell = EngineCell::init();
            let recovery = Arc::new(RecoveryControl::default());
            let casper = Arc::new(1);
            let prepared = recovery.startup().prepare(&casper);
            let context = prepared.handle();
            let dropped = Arc::new(AtomicBool::new(false));
            let engine = Arc::new(EngineProbe {
                cell: cell.clone(),
                recovery: recovery.clone(),
                dropped: dropped.clone(),
            });
            cell.set_running(engine.clone(), recovery.clone(), prepared)
                .await
                .unwrap();
            let old_reader = cell.get().await;
            let mut ticket = context.request_startup(false).unwrap();
            ticket.capture_snapshot(|| Ok(())).await.unwrap();
            cell.set(Arc::new(noop())).await;
            assert!(recovery.startup().context().unwrap().is_none());
            assert_eq!(
                ticket.phase(),
                crate::startup_completion::StartupPhase::Cancelled
            );
            assert!(!dropped.load(Ordering::SeqCst));
            drop(old_reader);
            drop(engine);
            assert!(dropped.load(Ordering::SeqCst));
            assert!(cell
                .inner
                .read()
                .await
                .recovery
                .as_ref()
                .is_some_and(|bound| Arc::ptr_eq(bound, &recovery)));
        }

        #[tokio::test]
        async fn rejected_input_engine_is_destroyed_outside_publication_guards() {
            for failure in 0..3 {
                let cell = EngineCell::init();
                let recovery = Arc::new(RecoveryControl::default());
                let other = Arc::new(RecoveryControl::default());
                let casper = Arc::new(1);
                cell.set_running(
                    Arc::new(noop()),
                    recovery.clone(),
                    recovery.startup().prepare(&casper),
                )
                .await
                .unwrap();
                cell.set(Arc::new(noop())).await;
                let before = cell.get().await;
                let dropped = Arc::new(AtomicBool::new(false));
                let engine = Arc::new(EngineProbe {
                    cell: cell.clone(),
                    recovery: recovery.clone(),
                    dropped: dropped.clone(),
                });
                let (input_controller, prepared) = match failure {
                    0 => (other.clone(), other.startup().prepare(&casper)),
                    1 => (recovery.clone(), other.startup().prepare(&casper)),
                    _ => {
                        recovery.startup().stop();
                        (recovery.clone(), recovery.startup().prepare(&casper))
                    }
                };
                assert!(cell
                    .set_running(engine, input_controller, prepared)
                    .await
                    .is_err());
                assert!(dropped.load(Ordering::SeqCst));
                assert!(Arc::ptr_eq(&before, &cell.get().await));
                assert!(recovery.startup().context().unwrap().is_none());
            }
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn engine_guard_precedes_controller_guard_and_covers_publication() {
            let cell = EngineCell::init();
            let recovery = Arc::new(RecoveryControl::default());
            let casper = Arc::new(7);
            let prepared = recovery.startup().prepare(&casper);
            let locked = Arc::new(Barrier::new(2));
            let release = Arc::new(Barrier::new(2));
            let holding = recovery.startup();
            let entered = locked.clone();
            let released = release.clone();
            let holder = std::thread::spawn(move || {
                holding.with_locked_state_for_test(|| {
                    entered.wait();
                    released.wait();
                })
            });
            locked.wait();
            let publishing = cell.clone();
            let controller = recovery.clone();
            let task = tokio::spawn(async move {
                publishing
                    .set_running(Arc::new(noop()), controller, prepared)
                    .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    if cell.inner.try_read().is_err() {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .unwrap();
            assert!(!recovery.startup().state_is_unlocked_for_test());
            release.wait();
            holder.join().unwrap();
            task.await.unwrap().unwrap();
            let slot = cell.inner.read().await;
            assert!(slot.registration.is_some());
            assert_eq!(*recovery.startup().context().unwrap().unwrap(), 7);
        }

        #[tokio::test]
        async fn cancellation_before_engine_lock_does_not_publish_context() {
            let cell = EngineCell::init();
            let recovery = Arc::new(RecoveryControl::default());
            let casper = Arc::new(3);
            let prepared = recovery.startup().prepare(&casper);
            let context = prepared.handle();
            let guard = cell.inner.write().await;
            let mut publication =
                Box::pin(cell.set_running(Arc::new(noop()), recovery.clone(), prepared));
            poll_fn(|cx| {
                assert!(publication.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            drop(publication);
            drop(guard);
            assert!(recovery.startup().context().unwrap().is_none());
            assert!(matches!(
                context.request_startup(false),
                Err(crate::startup_owner::StartupError::Cancelled)
            ));
        }

        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn competing_publications_keep_the_engine_and_context_pair_consistent() {
            for _ in 0..64 {
                let cell = EngineCell::init();
                let recovery = Arc::new(RecoveryControl::default());
                let mut tasks = tokio::task::JoinSet::new();
                let mut engines: Vec<Arc<dyn Engine>> = Vec::new();
                let contexts = [Arc::new(1), Arc::new(2)];
                for context in &contexts {
                    let engine: Arc<dyn Engine> = Arc::new(noop());
                    engines.push(engine.clone());
                    let cell = cell.clone();
                    let controller = recovery.clone();
                    let prepared = controller.startup().prepare(context);
                    tasks
                        .spawn(async move { cell.set_running(engine, controller, prepared).await });
                }
                while let Some(result) = tasks.join_next().await {
                    result.unwrap().unwrap();
                }
                let slot = cell.inner.read().await;
                let context = recovery.startup().context().unwrap().unwrap();
                let winner = engines
                    .iter()
                    .position(|engine| Arc::ptr_eq(engine, &slot.engine))
                    .unwrap();
                assert!(Arc::ptr_eq(&context, &contexts[winner]));
            }
        }

        #[tokio::test]
        async fn startup_failure_reporting_retains_the_first_error() {
            let cell = EngineCell::init();
            let mut failure = cell.subscribe_startup_failure();
            let first = CasperError::RuntimeError("first".to_string());
            assert!(cell.report_startup_failure(first.clone()));
            assert!(!cell.report_startup_failure(CasperError::RuntimeError("second".to_string())));
            failure.changed().await.unwrap();
            assert_eq!(*failure.borrow_and_update(), Some(first));
        }
    }
}
