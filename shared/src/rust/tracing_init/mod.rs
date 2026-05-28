//! Shared `tracing` subscriber initialisation for both the production
//! binary (`init`) and test suites (`init_for_tests`).

use std::path::PathBuf;

use eyre::{eyre, Result};
use serde::{Deserialize, Serialize};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::Registry;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// HOCON-deserializable logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    #[default]
    Json,
    Pretty,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSink {
    #[default]
    Stdout,
    File,
    Both,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogFileConfig {
    pub path: PathBuf,
    pub rotation: LogRotation,
    /// Number of rotated files to keep. 0 = unlimited.
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

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogRotation {
    #[default]
    Never,
    Hourly,
    Daily,
}

/// RAII guard returned by `init`. Must be held for the lifetime of the
/// process; dropping flushes any buffered file writes.
#[derive(Default)]
pub struct TracingGuards {
    _file: Option<WorkerGuard>,
}

/// Resolves the filter (RUST_LOG > cfg.filter) and installs the
/// layered subscriber for the configured format and sink.
pub fn init(cfg: &LoggingConfig) -> Result<TracingGuards> {
    let filter = resolve_filter(&cfg.filter);
    let mut guards = TracingGuards::default();
    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();

    let to_stdout = matches!(cfg.sink, LogSink::Stdout | LogSink::Both);
    let to_file = matches!(cfg.sink, LogSink::File | LogSink::Both);

    if to_stdout {
        layers.push(make_layer(cfg.format, std::io::stdout));
    }
    if to_file {
        let (writer, guard) = make_file_writer(&cfg.file)?;
        guards._file = Some(guard);
        layers.push(make_layer(cfg.format, writer));
    }

    tracing_subscriber::registry()
        .with(layers)
        .with(filter)
        .try_init()?;
    Ok(guards)
}

/// Idempotent test-suite init. Filter defaults to `"warn"`; subsequent
/// calls within the same process are no-ops.
pub fn init_for_tests() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::registry()
        .with(make_layer(LogFormat::Json, std::io::stdout))
        .with(filter)
        .try_init();
}

fn resolve_filter(cfg_filter: &str) -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(cfg_filter))
}

fn make_layer<W>(format: LogFormat, writer: W) -> Box<dyn Layer<Registry> + Send + Sync>
where W: for<'a> MakeWriter<'a> + Send + Sync + 'static {
    match format {
        LogFormat::Json => Box::new(
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_file(true)
                .with_line_number(true)
                .with_current_span(true)
                .with_span_list(true)
                .flatten_event(true)
                .with_writer(writer),
        ),
        LogFormat::Pretty => {
            use std::io::IsTerminal;
            Box::new(
                tracing_subscriber::fmt::layer()
                    .compact()
                    .with_ansi(std::io::stdout().is_terminal())
                    .with_target(true)
                    .with_thread_ids(false)
                    .with_line_number(false)
                    .with_writer(writer),
            )
        }
    }
}

fn make_file_writer(
    cfg: &LogFileConfig,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let path = &cfg.path;
    let dir = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or_else(|| {
            eyre!(
                "logging.file.path must include a parent directory: {:?}",
                path
            )
        })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| eyre!("logging.file.path must include a file name: {:?}", path))?;

    std::fs::create_dir_all(dir)
        .map_err(|e| eyre!("failed to create logging directory {:?}: {}", dir, e))?;

    let mut builder = tracing_appender::rolling::Builder::new()
        .rotation(match cfg.rotation {
            LogRotation::Never => tracing_appender::rolling::Rotation::NEVER,
            LogRotation::Hourly => tracing_appender::rolling::Rotation::HOURLY,
            LogRotation::Daily => tracing_appender::rolling::Rotation::DAILY,
        })
        .filename_prefix(file_name.to_string_lossy().into_owned());
    if cfg.retention > 0 {
        builder = builder.max_log_files(cfg.retention);
    }
    let appender = builder
        .build(dir)
        .map_err(|e| eyre!("failed to build rolling file appender: {}", e))?;
    Ok(tracing_appender::non_blocking(appender))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn make_file_writer_creates_parent_directory() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("nested").join("subdir").join("node.log");
        let cfg = LogFileConfig {
            path: path.clone(),
            rotation: LogRotation::Never,
            retention: 0,
        };

        let (_writer, _guard) = make_file_writer(&cfg).expect("make_file_writer");

        assert!(
            path.parent().unwrap().is_dir(),
            "parent directory should have been created"
        );
    }
}
