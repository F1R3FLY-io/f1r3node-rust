//! Shared `tracing` subscriber initialisation for both the production
//! binary (`init`) and test suites (`init_for_tests`).

mod bounded_file;

use std::path::Path;

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
#[serde(default)]
pub struct LogFileConfig {
    pub rotation: LogRotation,
    pub retention: usize,
    #[serde(
        rename = "max-file-size-bytes",
        alias = "max_file_size_bytes",
        deserialize_with = "de_positive_bytes"
    )]
    pub max_file_size_bytes: u64,
    #[serde(
        rename = "max-total-size-bytes",
        alias = "max_total_size_bytes",
        deserialize_with = "de_positive_bytes"
    )]
    pub max_total_size_bytes: u64,
}

fn de_positive_bytes<'de, D>(deserializer: D) -> std::result::Result<u64, D::Error>
where D: serde::Deserializer<'de> {
    let value = i64::deserialize(deserializer)?;
    u64::try_from(value)
        .ok()
        .filter(|value| *value > 0)
        .ok_or_else(|| serde::de::Error::custom("A log byte limit must be a positive integer."))
}

impl Default for LogFileConfig {
    fn default() -> Self {
        Self {
            rotation: LogRotation::Never,
            retention: 0,
            max_file_size_bytes: 100 * 1024 * 1024,
            max_total_size_bytes: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl LogFileConfig {
    fn validate(&self) -> std::io::Result<()> {
        if self.max_file_size_bytes == 0 || self.max_total_size_bytes < self.max_file_size_bytes {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "logging.file requires 0 < max-file-size-bytes <= max-total-size-bytes",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogRotation {
    #[default]
    Never,
    Minutely,
    Hourly,
    Daily,
}

/// RAII guard returned by `init`. Must be held for the lifetime of the
/// process; dropping flushes any buffered file writes.
#[derive(Default)]
pub struct TracingGuards {
    _file: Option<WorkerGuard>,
}

/// Resolves the filter (RUST_LOG > cfg.filter) and installs the layered
/// subscriber. `data_dir` is required when `cfg.sink` includes file output;
/// logs are written to `<data_dir>/logs/node.log`.
pub fn init(cfg: &LoggingConfig, data_dir: Option<&Path>) -> Result<TracingGuards> {
    cfg.file.validate()?;
    let filter = resolve_filter(&cfg.filter);
    let mut guards = TracingGuards::default();
    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = Vec::new();

    let to_stdout = matches!(cfg.sink, LogSink::Stdout | LogSink::Both);
    let to_file = matches!(cfg.sink, LogSink::File | LogSink::Both);

    if to_stdout {
        layers.push(make_layer(cfg.format, std::io::stdout));
    }
    if to_file {
        let dir = data_dir.ok_or_else(|| {
            eyre!("logging.sink includes file output but no data directory is available")
        })?;
        let (writer, guard) = make_file_writer(dir, &cfg.file)?;
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
    data_dir: &Path,
    cfg: &LogFileConfig,
) -> Result<(tracing_appender::non_blocking::NonBlocking, WorkerGuard)> {
    let appender = bounded_file::BoundedFile::new(data_dir, cfg)
        .map_err(|e| eyre!("failed to build bounded file appender: {}", e))?;
    Ok(tracing_appender::non_blocking(appender))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn make_file_writer_creates_logs_subdir_in_data_dir() {
        let data_dir = tempdir().expect("tempdir");
        let cfg = LogFileConfig {
            rotation: LogRotation::Never,
            retention: 0,
            ..LogFileConfig::default()
        };

        let (_writer, _guard) = make_file_writer(data_dir.path(), &cfg).expect("make_file_writer");

        assert!(
            data_dir.path().join("logs").is_dir(),
            "logs/ subdirectory should have been created inside data_dir"
        );
    }

    #[test]
    fn default_config_is_info_json_to_stdout_without_rotation() {
        let cfg = LoggingConfig::default();
        assert_eq!(cfg.filter, "info");
        assert!(matches!(cfg.format, LogFormat::Json));
        assert!(matches!(cfg.sink, LogSink::Stdout));
        assert!(matches!(cfg.file.rotation, LogRotation::Never));
        assert_eq!(cfg.file.retention, 0);
    }

    #[test]
    fn config_enums_deserialize_from_lowercase_names() {
        assert!(matches!(
            serde_json::from_str::<LogFormat>("\"pretty\"").unwrap(),
            LogFormat::Pretty
        ));
        assert!(matches!(
            serde_json::from_str::<LogSink>("\"both\"").unwrap(),
            LogSink::Both
        ));
        assert!(matches!(
            serde_json::from_str::<LogRotation>("\"minutely\"").unwrap(),
            LogRotation::Minutely
        ));
        assert!(serde_json::from_str::<LogFormat>("\"Pretty\"").is_err());

        assert_eq!(serde_json::to_string(&LogSink::File).unwrap(), "\"file\"");
        assert_eq!(
            serde_json::to_string(&LogRotation::Daily).unwrap(),
            "\"daily\""
        );
    }

    #[test]
    fn config_round_trips_through_json() {
        let cfg = LoggingConfig {
            filter: "debug,hyper=warn".to_string(),
            format: LogFormat::Pretty,
            sink: LogSink::Both,
            file: LogFileConfig {
                rotation: LogRotation::Hourly,
                retention: 5,
                max_file_size_bytes: 256,
                max_total_size_bytes: 1024,
            },
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let decoded: LoggingConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.filter, cfg.filter);
        assert!(matches!(decoded.format, LogFormat::Pretty));
        assert!(matches!(decoded.sink, LogSink::Both));
        assert!(matches!(decoded.file.rotation, LogRotation::Hourly));
        assert_eq!(decoded.file.retention, 5);
        assert_eq!(decoded.file.max_file_size_bytes, 256);
        assert_eq!(decoded.file.max_total_size_bytes, 1024);
    }

    #[test]
    fn make_file_writer_supports_every_rotation_and_retention() {
        for rotation in [
            LogRotation::Never,
            LogRotation::Minutely,
            LogRotation::Hourly,
            LogRotation::Daily,
        ] {
            let data_dir = tempdir().expect("tempdir");
            let cfg = LogFileConfig {
                rotation,
                retention: 3,
                ..LogFileConfig::default()
            };
            let (_writer, _guard) =
                make_file_writer(data_dir.path(), &cfg).expect("make_file_writer");
            assert!(data_dir.path().join("logs").is_dir(), "{rotation:?}");
        }
    }

    #[test]
    fn file_writer_limits_hot_error_output_by_bytes() {
        use std::io::Write;

        let data_dir = tempdir().unwrap();
        let cfg: LogFileConfig = serde_json::from_str(
            r#"{"rotation":"never","retention":0,"max-file-size-bytes":128,"max-total-size-bytes":384}"#,
        )
        .unwrap();
        let (mut writer, guard) = make_file_writer(data_dir.path(), &cfg).unwrap();
        for _ in 0..1000 {
            writer
                .write_all(b"ERROR repeated transport accept failure\n")
                .unwrap();
        }
        drop(writer);
        drop(guard);
        let sizes: Vec<u64> = std::fs::read_dir(data_dir.path().join("logs"))
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .collect();
        assert!(sizes.iter().all(|size| *size <= 128), "{sizes:?}");
        assert!(sizes.iter().sum::<u64>() <= 384, "{sizes:?}");
        assert!(sizes.iter().sum::<u64>() > 0);
    }

    #[test]
    fn json_error_events_remain_parseable_under_size_rotation() {
        let data_dir = tempdir().unwrap();
        let cfg = LogFileConfig {
            max_file_size_bytes: 512,
            max_total_size_bytes: 1536,
            ..LogFileConfig::default()
        };
        let (writer, guard) = make_file_writer(data_dir.path(), &cfg).unwrap();
        let subscriber = Registry::default().with(make_layer(LogFormat::Json, writer));
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..1000 {
                tracing::error!(
                    fault_code = 24,
                    "Repeated accept failure in a controlled fixture"
                );
            }
        });
        drop(guard);
        let mut total = 0;
        let mut events = 0;
        for entry in std::fs::read_dir(data_dir.path().join("logs")).unwrap() {
            let contents = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(contents.len() <= 512);
            total += contents.len();
            for line in contents
                .split(|byte| *byte == b'\n')
                .filter(|line| !line.is_empty())
            {
                let event: serde_json::Value = serde_json::from_slice(line).unwrap();
                assert_eq!(event["fault_code"], 24);
                events += 1;
            }
        }
        assert!(events > 0);
        assert!(total <= 1536);
    }

    #[test]
    fn init_with_file_sink_requires_a_data_dir() {
        let cfg = LoggingConfig {
            sink: LogSink::File,
            ..LoggingConfig::default()
        };
        let err = match init(&cfg, None) {
            Ok(_) => panic!("init without data_dir should fail"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("no data directory"), "{err}");
    }

    // The global subscriber can be installed only once per process, so every
    // interaction with it stays inside this single test.
    #[test]
    fn init_installs_subscriber_once_and_init_for_tests_is_idempotent() {
        let data_dir = tempdir().expect("tempdir");
        let cfg = LoggingConfig {
            sink: LogSink::Both,
            format: LogFormat::Pretty,
            ..LoggingConfig::default()
        };

        let _guards = init(&cfg, Some(data_dir.path())).expect("first init");
        assert!(data_dir.path().join("logs").is_dir());
        tracing::info!("subscriber installed");

        assert!(init(&cfg, Some(data_dir.path())).is_err());

        init_for_tests();
        init_for_tests();
    }
}
