//! Issue #24 Track A metric (CLAIM-CASPER-STAGE-METRICS-001, TASK-021-17).
//!
//! A probe task asks the async runtime to wake it every interval and records
//! how late each wake is. CPU work that holds the runtime's worker threads
//! delays every other task, so the lateness is a proxy for worker saturation.
//! It needs no `tokio_unstable` metrics.

use std::time::Duration;

use tokio::time::Instant;

pub const SCHEDULER_DELAY_TIME_METRIC: &str = "runtime.scheduler.delay.time";
pub const SCHEDULER_DELAY_PROBE_INTERVAL: Duration = Duration::from_millis(100);

async fn probe_once(interval: Duration) {
    let deadline = Instant::now() + interval;
    tokio::time::sleep_until(deadline).await;
    let delay = Instant::now().saturating_duration_since(deadline);
    metrics::histogram!(SCHEDULER_DELAY_TIME_METRIC, "source" => super::SYSTEM_METRICS_SOURCE)
        .record(delay.as_secs_f64());
}

pub async fn run_scheduler_delay_probe(interval: Duration) {
    loop {
        probe_once(interval).await;
    }
}

/// Starts the probe on the current runtime. Outside a runtime it does nothing.
pub fn start_scheduler_delay_probe() -> bool {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(run_scheduler_delay_probe(SCHEDULER_DELAY_PROBE_INTERVAL));
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use metrics_util::debugging::{DebugValue, DebuggingRecorder};

    use super::*;

    fn samples(snapshotter: &metrics_util::debugging::Snapshotter) -> Vec<f64> {
        snapshotter
            .snapshot()
            .into_vec()
            .into_iter()
            .filter(|(key, ..)| key.key().name() == SCHEDULER_DELAY_TIME_METRIC)
            .flat_map(|(.., value)| match value {
                DebugValue::Histogram(values) => {
                    values.into_iter().map(|v| v.into_inner()).collect()
                }
                _ => Vec::new(),
            })
            .collect()
    }

    #[test]
    fn a_wake_delayed_by_a_blocked_worker_records_the_delay() {
        let recorder = DebuggingRecorder::new();
        let snapshotter = recorder.snapshotter();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap();
        metrics::with_local_recorder(&recorder, || {
            runtime.block_on(async {
                let probe = tokio::spawn(probe_once(Duration::from_millis(10)));
                tokio::task::yield_now().await;
                std::thread::sleep(Duration::from_millis(60));
                probe.await.unwrap();
            });
        });

        let recorded = samples(&snapshotter);
        assert_eq!(
            recorded.len(),
            1,
            "one wake records one sample: {recorded:?}"
        );
        assert!(
            recorded[0] >= 0.04,
            "the blocked worker delays the wake: {recorded:?}"
        );
    }

    #[test]
    fn the_probe_does_not_start_outside_a_runtime() {
        assert!(!start_scheduler_delay_probe());
    }
}
