use std::sync::Arc;

use tokio::sync::{watch, RwLock};

use super::engine::{noop, Engine};
use crate::rust::blocks::block_processing_queue::{
    PreparedRecoveryContext, RecoveryControl, RecoveryRegistration,
};
use crate::rust::errors::CasperError;

struct EngineSlot {
    engine: Arc<dyn Engine>,
    recovery: Option<Arc<RecoveryControl>>,
    registration: Option<RecoveryRegistration>,
}

/// EngineCell is a concurrency-safe mutable container for the current Engine instance.
///
/// This is the Rust equivalent of Scala's Cell[F, Engine[F]] (see EngineCell.scala).
/// It provides async operations that match the Scala F[_] monadic interface.
///
/// Usage:
///   let engine_cell = EngineCell::init().await?;
///   let engine = engine_cell.read().await?;  // Returns Arc<dyn Engine>
///   engine_cell.set(Arc::new(MyEngine::new(...))).await?;
///
/// This implementation provides 1:1 API compatibility with the Scala EngineCell.
/// Uses Arc internally to avoid expensive cloning on read operations.\
#[derive(Clone)]
pub struct EngineCell {
    inner: Arc<RwLock<EngineSlot>>,
    startup_failure: watch::Sender<Option<CasperError>>,
}

impl EngineCell {
    /// Initialize EngineCell with NoopEngine (equivalent to Cell.mvarCell[F, Engine[F]](Engine.noop))
    pub fn init() -> Self {
        let engine = Arc::new(noop());
        let (startup_failure, _) = watch::channel(None);
        EngineCell {
            inner: Arc::new(RwLock::new(EngineSlot {
                engine,
                recovery: None,
                registration: None,
            })),
            startup_failure,
        }
    }

    /// Read the current engine (equivalent to Cell.read: F[Engine[F]])
    /// This is the most frequently used method in the Scala codebase
    #[inline]
    pub async fn get(&self) -> Arc<dyn Engine> { self.inner.read().await.engine.clone() }

    /// Set the engine to a new instance (equivalent to Cell.set(s: Engine[F]): F[Unit])
    #[inline]
    pub async fn set(&self, engine: Arc<dyn Engine>) {
        let mut slot = self.inner.write().await;
        let (retired, cleanup) = if let Some(recovery) = slot.recovery.clone() {
            let (retired, cleanup) = recovery.startup().clear_and_publish(|| {
                (
                    std::mem::replace(&mut slot.engine, engine),
                    slot.registration.take(),
                )
            });
            (retired, Some(cleanup))
        } else {
            ((std::mem::replace(&mut slot.engine, engine), None), None)
        };
        drop(slot);
        if let Some(cleanup) = cleanup {
            cleanup.finish();
        }
        drop(retired);
    }

    pub async fn set_running(
        &self,
        engine: Arc<dyn Engine>,
        recovery: Arc<RecoveryControl>,
        prepared: PreparedRecoveryContext,
    ) -> Result<(), CasperError> {
        let mut replacement = Some(engine);
        let mut slot = self.inner.write().await;
        if slot
            .recovery
            .as_ref()
            .is_some_and(|bound| !Arc::ptr_eq(bound, &recovery))
        {
            return Err(CasperError::RuntimeError(
                "Engine cell cannot replace its recovery controller".to_string(),
            ));
        }
        let (retired, cleanup) = recovery
            .startup()
            .publish(prepared, |registration| {
                let retired = (
                    std::mem::replace(
                        &mut slot.engine,
                        replacement.take().expect("unpublished engine is owned"),
                    ),
                    slot.registration.replace(registration),
                );
                slot.recovery = Some(recovery.clone());
                retired
            })
            .map_err(CasperError::from)?;
        drop(slot);
        cleanup.finish();
        drop(retired);
        Ok(())
    }

    pub fn subscribe_startup_failure(&self) -> watch::Receiver<Option<CasperError>> {
        self.startup_failure.subscribe()
    }

    pub fn report_startup_failure(&self, error: CasperError) -> bool {
        self.startup_failure.send_if_modified(|current| {
            if current.is_some() {
                false
            } else {
                *current = Some(error);
                true
            }
        })
    }
}
