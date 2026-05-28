//! Shared `tracing` subscriber initialisation for both the production
//! binary (`init`) and test suites (`init_for_tests`).

use eyre::bail;
use serde::Deserialize;
use std::path::PathBuf;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

/// HOCON-deserializable logging configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    /// `EnvFilter` expression. `RUST_LOG`, if set, fully overrides this.
    pub filter: String,
    pub format: LogFormat,
    pub sink: LogSink,
    pub file: LogFileConfig,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            filter: "info".to_string(),
            format: LogFormat::default(),
            sink: LogSink::default(),
            file: LogFileConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSink {
    #[default]
    Stdout,
    File,
    Both,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogFileConfig {
    pub path: PathBuf,
    pub rotation: LogRotation,
    pub retention: usize,
}

impl Default for LogFileConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("logs/node.log"),
            rotation: LogRotation::default(),
            retention: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogRotation {
    #[default]
    Never,
    Hourly,
    Daily,
}

/// Resolves the filter (RUST_LOG > cfg.filter) and installs the
/// layered subscriber. Returns an error if the format/sink combination
/// is not implemented.
pub fn init(cfg: &LoggingConfig) -> eyre::Result<()> {
    let filter = resolve_filter(&cfg.filter);
    match (cfg.format, cfg.sink) {
        (LogFormat::Json, LogSink::Stdout) => init_json_stdout(filter),
        (LogFormat::Pretty, _) => {
            bail!("logging.format = \"pretty\" is not implemented")
        }
        (_, LogSink::File) | (_, LogSink::Both) => {
            bail!("logging.sink = \"file\"/\"both\" is not implemented")
        }
    }
}

/// Idempotent test-suite init. Filter defaults to `"warn"`; subsequent
/// calls within the same process are no-ops.
pub fn init_for_tests() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(json_layer())
        .try_init();
}

fn resolve_filter(cfg_filter: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(cfg_filter))
}

fn init_json_stdout(filter: EnvFilter) -> eyre::Result<()> {
    tracing_subscriber::registry()
        .with(filter)
        .with(json_layer())
        .try_init()?;
    Ok(())
}

fn json_layer<S>() -> tracing_subscriber::fmt::Layer<
    S,
    tracing_subscriber::fmt::format::JsonFields,
    tracing_subscriber::fmt::format::Format<tracing_subscriber::fmt::format::Json>,
>
where
    S: tracing::Subscriber + for<'a> tracing_subscriber::registry::LookupSpan<'a>,
{
    tracing_subscriber::fmt::layer()
        .json()
        .with_target(true)
        .with_file(true)
        .with_line_number(true)
        .with_current_span(false)
        .with_span_list(false)
        .flatten_event(true)
}
