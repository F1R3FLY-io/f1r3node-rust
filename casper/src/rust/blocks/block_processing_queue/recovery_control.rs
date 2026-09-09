use std::sync::atomic::AtomicU8;
use std::sync::Arc;

use block_storage::rust::casperbuffer::casper_buffer_key_value_storage::CasperBufferKeyValueStorage;
use block_storage::rust::util::startup_scan::{StartupScan, StartupScanAction, StartupScanError};
use models::rust::block_hash::BlockHashSerde;
use tokio::sync::Notify;

use super::super::super::casper::MultiParentCasper;
use super::super::super::errors::CasperError;
use super::startup_owner::{
    ActiveStartup, PreparedStartupContext, StartupContext, StartupError, StartupOwner,
    StartupRegistration, StartupTicket,
};
use super::Ordering;

#[path = "recovery_signal_state.rs"]
mod recovery_signal_state;
pub use recovery_signal_state::{RecoverySignalState, RecoveryWake};

#[derive(Debug, Default)]
pub struct RecoverySignal {
    state: RecoverySignalState,
    notify: Notify,
}

impl RecoverySignal {
    pub fn request(&self, proposal: bool) {
        if self.state.request(proposal) {
            self.notify.notify_one();
        }
    }

    pub fn take(&self) -> RecoveryWake { self.state.take() }

    pub(crate) fn stop(&self) {
        if self.state.stop() {
            self.notify.notify_one();
        }
    }

    pub fn is_stopped(&self) -> bool { self.state.is_stopped() }

    pub async fn wait(&self) -> RecoveryWake {
        loop {
            match self.take() {
                RecoveryWake::Idle => self.notify.notified().await,
                action => return action,
            }
        }
    }
}

type CasperContext = dyn MultiParentCasper + Send + Sync;
pub type RecoveryStartupOwner =
    StartupOwner<CasperContext, StartupScan<BlockHashSerde>, CasperError>;
pub type RecoveryStartupContext =
    StartupContext<CasperContext, StartupScan<BlockHashSerde>, CasperError>;
pub type RecoveryActiveStartup =
    ActiveStartup<CasperContext, StartupScan<BlockHashSerde>, CasperError>;
pub type PreparedRecoveryContext =
    PreparedStartupContext<CasperContext, StartupScan<BlockHashSerde>, CasperError>;
pub(crate) type RecoveryRegistration =
    StartupRegistration<CasperContext, StartupScan<BlockHashSerde>, CasperError>;

impl StartupTicket<CasperContext, StartupScan<BlockHashSerde>, CasperError> {
    pub async fn capture_buffer_snapshot(
        &mut self,
        buffer: &CasperBufferKeyValueStorage,
    ) -> Result<(), CasperError> {
        self.capture_snapshot(|| Ok(StartupScan::new(buffer.snapshot_pendant_candidates())))
            .await
            .map_err(CasperError::from)
    }
}

impl ActiveStartup<CasperContext, StartupScan<BlockHashSerde>, CasperError> {
    pub fn next_scan_action(
        &mut self,
        has_capacity: impl FnMut() -> bool,
        is_block: impl FnOnce(&BlockHashSerde) -> bool,
    ) -> Result<StartupScanAction<BlockHashSerde>, StartupScanError> {
        self.work_mut().next_action(has_capacity, is_block)
    }

    pub fn record_scan_presence(&mut self, present: bool) -> Result<(), StartupScanError> {
        self.work_mut().record_presence(present)
    }

    pub fn record_scan_processed(&mut self) -> Result<(), StartupScanError> {
        self.work_mut().record_processed()
    }
}

#[derive(Default)]
pub struct RecoveryControl {
    signal: Arc<RecoverySignal>,
    startup: Arc<RecoveryStartupOwner>,
}

impl RecoveryControl {
    pub fn signal(&self) -> Arc<RecoverySignal> { self.signal.clone() }

    pub fn startup(&self) -> Arc<RecoveryStartupOwner> { self.startup.clone() }

    pub fn context(&self) -> Result<Option<Arc<CasperContext>>, CasperError> {
        self.startup.context().map_err(CasperError::from)
    }

    pub fn stop(&self) {
        let cleanup = self.startup.stop_deferred();
        self.signal.stop();
        cleanup.finish();
    }
}

impl From<StartupError<CasperError>> for CasperError {
    fn from(error: StartupError<CasperError>) -> Self {
        match error {
            StartupError::Scan(error) | StartupError::Callback(error) => error,
            error => Self::RuntimeError(format!("Startup recovery failed: {error:?}")),
        }
    }
}

#[derive(Debug)]
pub struct RecoveryReleaseWake(Arc<RecoverySignal>);

impl RecoveryReleaseWake {
    pub fn new(signal: Arc<RecoverySignal>) -> Self { Self(signal) }
}

impl Drop for RecoveryReleaseWake {
    fn drop(&mut self) { self.0.request(false); }
}

pub struct RecoveryStopGuard(pub Arc<RecoveryControl>);

impl Drop for RecoveryStopGuard {
    fn drop(&mut self) { self.0.stop(); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn notification_before_wait_is_retained_and_coalesced() {
        let signal = RecoverySignal::default();
        signal.request(false);
        signal.request(true);
        let action = tokio::time::timeout(std::time::Duration::from_secs(1), signal.wait())
            .await
            .unwrap();
        assert_eq!(action, RecoveryWake::Work { proposal: true });
        assert_eq!(signal.take(), RecoveryWake::Idle);
    }

    #[tokio::test]
    async fn stop_wakes_parked_driver_and_release_cannot_restart_it() {
        let control = Arc::new(RecoveryControl::default());
        let signal = control.signal();
        let waiting_signal = signal.clone();
        let driver = tokio::spawn(async move { waiting_signal.wait().await });
        tokio::task::yield_now().await;
        let wake = RecoveryReleaseWake::new(signal.clone());
        drop(RecoveryStopGuard(control.clone()));
        drop(wake);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), driver)
                .await
                .unwrap()
                .unwrap(),
            RecoveryWake::Stopped
        );
        assert!(signal.is_stopped());
        assert!(control.startup().is_stopped());
    }

    #[tokio::test]
    async fn canceling_a_wait_does_not_consume_pending_work() {
        let signal = Arc::new(RecoverySignal::default());
        let waiting_signal = signal.clone();
        let driver = tokio::spawn(async move { waiting_signal.wait().await });
        tokio::task::yield_now().await;
        driver.abort();
        assert!(driver.await.unwrap_err().is_cancelled());
        signal.request(true);
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), signal.wait())
                .await
                .unwrap(),
            RecoveryWake::Work { proposal: true }
        );
    }
}
