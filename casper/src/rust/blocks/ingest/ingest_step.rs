use std::future::Future;
use std::time::Instant;

use tracing::Instrument;

// The block processor's source label is part of the exported names that the
// monitoring rules read.
use crate::rust::metrics_constants::{
    BLOCK_PROCESSING_STORAGE_TIME_METRIC, BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC,
    BLOCK_PROCESSOR_METRICS_SOURCE, BLOCK_VALIDATION_TIME_METRIC,
};

/// A timed stage of block ingest: each step owns its span and its histogram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestStep {
    /// The store write of an admitted block, to disk or the pending area.
    Storage,
    /// Building the casper snapshot a validation runs against.
    ValidationSetup,
    Validation,
}

impl IngestStep {
    pub fn time<T>(self, step: impl FnOnce() -> T) -> T {
        let started = Instant::now();
        let output = self.span().in_scope(step);
        self.record(started);
        output
    }

    pub async fn time_async<F: Future>(self, step: F) -> F::Output {
        let started = Instant::now();
        let output = step.instrument(self.span()).await;
        self.record(started);
        output
    }

    fn name(self) -> &'static str {
        match self {
            IngestStep::Storage => "storage",
            IngestStep::ValidationSetup => "validation-setup",
            IngestStep::Validation => "validation",
        }
    }

    fn metric(self) -> &'static str {
        match self {
            IngestStep::Storage => BLOCK_PROCESSING_STORAGE_TIME_METRIC,
            IngestStep::ValidationSetup => BLOCK_PROCESSING_VALIDATION_SETUP_TIME_METRIC,
            IngestStep::Validation => BLOCK_VALIDATION_TIME_METRIC,
        }
    }

    fn span(self) -> tracing::Span { tracing::debug_span!("block_ingest_step", step = self.name()) }

    fn record(self, started: Instant) {
        metrics::histogram!(self.metric(), "source" => BLOCK_PROCESSOR_METRICS_SOURCE)
            .record(started.elapsed().as_secs_f64());
    }
}
