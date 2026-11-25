use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, OnceLock};
use std::thread::JoinHandle;
use std::time::Duration;

use eyre::Result;
use metrics_exporter_prometheus::PrometheusHandle;
use parking_lot::Mutex;
use tracing::{info, warn};

use crate::rust::diagnostics::prometheus_config::PrometheusConfiguration;

static GLOBAL_REPORTER: OnceLock<Arc<NewPrometheusReporter>> = OnceLock::new();
const UPKEEP_INTERVAL: Duration = Duration::from_secs(5);

pub struct NewPrometheusReporter {
    prometheus_handle: PrometheusHandle,
    upkeep: Mutex<Option<UpkeepWorker>>,
}

pub struct PrometheusUpkeep {
    reporter: Arc<NewPrometheusReporter>,
}

struct UpkeepWorker {
    users: usize,
    stop: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

impl UpkeepWorker {
    fn start(interval: Duration, mut upkeep: impl FnMut() + Send + 'static) -> Result<Self> {
        let (stop, stopped) = mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("prometheus-upkeep".to_owned())
            .spawn(move || {
                while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                    upkeep();
                }
            })?;

        Ok(Self {
            users: 1,
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for UpkeepWorker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if self
            .thread
            .take()
            .is_some_and(|thread| thread.join().is_err())
        {
            warn!("Prometheus upkeep worker panicked");
        }
    }
}

impl Drop for PrometheusUpkeep {
    fn drop(&mut self) {
        let mut worker = self.reporter.upkeep.lock();
        if let Some(active) = worker.as_mut() {
            active.users -= 1;
            if active.users == 0 {
                drop(worker.take());
            }
        }
    }
}

impl NewPrometheusReporter {
    pub fn initialize() -> Result<Arc<Self>> {
        let config = PrometheusConfiguration::default();
        Self::initialize_with_config(config)
    }

    pub fn initialize_with_config(config: PrometheusConfiguration) -> Result<Arc<Self>> {
        if let Some(reporter) = GLOBAL_REPORTER.get() {
            return Ok(Arc::clone(reporter));
        }

        let recorder = config.build_recorder()?;

        let handle = recorder.handle();

        let reporter = GLOBAL_REPORTER.get_or_init(|| {
            if let Err(e) = metrics::set_global_recorder(recorder) {
                warn!("Failed to set global metrics recorder: {}", e);
            }

            info!("Prometheus metrics exporter initialized");

            Arc::new(Self {
                prometheus_handle: handle,
                upkeep: Mutex::new(None),
            })
        });

        Ok(Arc::clone(reporter))
    }

    pub fn global() -> Option<Arc<Self>> { GLOBAL_REPORTER.get().map(Arc::clone) }

    pub fn start_upkeep(self: &Arc<Self>) -> Result<PrometheusUpkeep> {
        let mut worker = self.upkeep.lock();
        if let Some(active) = worker.as_mut() {
            active.users += 1;
        } else {
            let handle = self.prometheus_handle.clone();
            *worker = Some(UpkeepWorker::start(UPKEEP_INTERVAL, move || {
                handle.run_upkeep();
            })?);
        }

        Ok(PrometheusUpkeep {
            reporter: Arc::clone(self),
        })
    }

    pub fn scrape_data(&self) -> String { self.prometheus_handle.render() }
}

#[cfg(test)]
mod upkeep_tests {
    use metrics::{Key, Level, Metadata, Recorder};
    use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusRecorder};

    use super::*;

    fn reporter() -> (Arc<NewPrometheusReporter>, PrometheusRecorder) {
        let recorder = PrometheusBuilder::new()
            .set_buckets(&[1.0, 2.0])
            .unwrap()
            .build_recorder();
        let reporter = Arc::new(NewPrometheusReporter {
            prometheus_handle: recorder.handle(),
            upkeep: Mutex::new(None),
        });
        (reporter, recorder)
    }

    #[test]
    fn upkeep_aggregates_without_a_runtime_or_scrapes_and_preserves_histograms() {
        let (reporter, recorder) = reporter();
        let histogram = recorder.register_histogram(
            &Key::from_name("upkeep_duration_seconds"),
            &Metadata::new("upkeep-test", Level::INFO, None),
        );
        for _ in 0..10_000 {
            histogram.record(0.5);
            histogram.record(1.5);
            histogram.record(2.5);
        }

        let handle = reporter.prometheus_handle.clone();
        let (completed, received) = mpsc::sync_channel(1);
        let worker = UpkeepWorker::start(Duration::from_millis(1), move || {
            handle.run_upkeep();
            let _ = completed.try_send(());
        })
        .unwrap();
        received.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(worker);

        let output = reporter.scrape_data();
        for expected in [
            "upkeep_duration_seconds_count 30000",
            "upkeep_duration_seconds_sum 45000",
            "upkeep_duration_seconds_bucket{le=\"1\"} 10000",
            "upkeep_duration_seconds_bucket{le=\"2\"} 20000",
            "upkeep_duration_seconds_bucket{le=\"+Inf\"} 30000",
        ] {
            assert!(output.lines().any(|line| line == expected), "{output}");
        }
    }

    #[test]
    fn concurrent_starts_share_one_worker_until_the_last_guard_is_dropped() {
        let (reporter, _recorder) = reporter();
        let starters: Vec<_> = (0..8)
            .map(|_| {
                let reporter = Arc::clone(&reporter);
                std::thread::spawn(move || reporter.start_upkeep().unwrap())
            })
            .collect();
        let mut guards: Vec<_> = starters
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        {
            let worker = reporter.upkeep.lock();
            assert_eq!(worker.as_ref().unwrap().users, guards.len());
        }

        let last = guards.pop().unwrap();
        drop(guards);
        {
            let worker = reporter.upkeep.lock();
            let worker = worker.as_ref().unwrap();
            assert_eq!(worker.users, 1);
            assert!(!worker.thread.as_ref().unwrap().is_finished());
        }
        drop(last);
        assert!(reporter.upkeep.lock().is_none());

        let restarted = reporter.start_upkeep().unwrap();
        assert_eq!(reporter.upkeep.lock().as_ref().unwrap().users, 1);
        drop(restarted);
        assert!(reporter.upkeep.lock().is_none());
    }
}
