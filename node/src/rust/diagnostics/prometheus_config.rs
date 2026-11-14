use std::collections::HashMap;

use casper::rust::metrics_constants as casper_metrics;
use metrics_exporter_prometheus::{BuildError, Matcher, PrometheusBuilder, PrometheusRecorder};
use rspace_plus_plus::rspace::metrics_constants as rspace_metrics;

const ELEMENT_BUCKETS: &[f64] = &[
    0.0, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0, 128.0, 256.0, 512.0, 1024.0, 4096.0, 16384.0,
    65536.0,
];

// Prometheus histogram bucket definitions
// These values are taken from Kamon's default Prometheus configuration to maintain
// compatibility with the Scala implementation in NewPrometheusReporter.scala
// Source: kamon-prometheus library defaults (buckets.default-buckets, buckets.time-buckets, buckets.information-buckets)

// Time-based buckets (milliseconds to seconds)
const BUCKET_1MS: f64 = 0.001;
const BUCKET_3MS: f64 = 0.003;
const BUCKET_5MS: f64 = 0.005;
const BUCKET_10MS: f64 = 0.01;
const BUCKET_25MS: f64 = 0.025;
const BUCKET_50MS: f64 = 0.05;
const BUCKET_75MS: f64 = 0.075;
const BUCKET_100MS: f64 = 0.1;
const BUCKET_250MS: f64 = 0.25;
const BUCKET_500MS: f64 = 0.5;
const BUCKET_750MS: f64 = 0.75;
const BUCKET_1S: f64 = 1.0;
const BUCKET_2_5S: f64 = 2.5;
const BUCKET_5S: f64 = 5.0;
const BUCKET_7_5S: f64 = 7.5;
const BUCKET_10S: f64 = 10.0;

// Information-based buckets (bytes)
const BUCKET_512B: f64 = 512.0;
const BUCKET_1KB: f64 = 1024.0;
const BUCKET_2KB: f64 = 2048.0;
const BUCKET_4KB: f64 = 4096.0;
const BUCKET_16KB: f64 = 16384.0;
const BUCKET_64KB: f64 = 65536.0;
const BUCKET_512KB: f64 = 524288.0;
const BUCKET_1MB: f64 = 1048576.0;

#[derive(Debug, Clone)]
pub struct PrometheusConfiguration {
    pub default_buckets: Vec<f64>,
    pub time_buckets: Vec<f64>,
    pub information_buckets: Vec<f64>,
    pub custom_buckets: HashMap<String, Vec<f64>>,
    pub include_environment_tags: bool,
}

impl PrometheusConfiguration {
    pub(super) fn build_recorder(&self) -> Result<PrometheusRecorder, BuildError> {
        let mut builder = PrometheusBuilder::new().set_buckets(&self.default_buckets)?;

        for suffix in ["_time", "_seconds", "_duration"] {
            builder = builder
                .set_buckets_for_metric(Matcher::Suffix(suffix.to_owned()), &self.time_buckets)?;
        }
        for name in [
            casper_metrics::CASPER_INIT_TIME_TO_APPROVED_BLOCK_METRIC,
            casper_metrics::CASPER_INIT_TIME_TO_RUNNING_METRIC,
        ] {
            builder = builder
                .set_buckets_for_metric(Matcher::Full(name.to_owned()), &self.time_buckets)?;
        }

        for name in [
            casper_metrics::BLOCK_SIZE_METRIC,
            rspace_metrics::HISTORY_CHECKPOINT_SERIALIZED_BYTES_METRIC,
        ] {
            builder = builder.set_buckets_for_metric(
                Matcher::Full(name.to_owned()),
                &self.information_buckets,
            )?;
        }

        for name in [
            casper_metrics::PROPOSER_QUEUE_DEPTH_METRIC,
            casper_metrics::BLOCK_ARRIVAL_DEPTH_METRIC,
            casper_metrics::BLOCK_REPLAY_PHASE_USER_DEPLOYS_WORK_METRIC,
            casper_metrics::BLOCK_REPLAY_PHASE_SYSTEM_DEPLOYS_WORK_METRIC,
            casper_metrics::DAG_MERGE_RELATION_ITEMS_METRIC,
            casper_metrics::DAG_MERGE_RELATION_BRANCHES_METRIC,
            casper_metrics::DAG_MERGE_CONFLICT_EDGES_METRIC,
            casper_metrics::DAG_MERGE_REJECTION_OPTIONS_METRIC,
            casper_metrics::DAG_MERGE_STATE_APPLICATION_ACTIONS_METRIC,
            casper_metrics::FLOOR_DISTANCE_METRIC,
            casper_metrics::MERGE_SCOPE_SIZE_METRIC,
            casper_metrics::FLOOR_FRONTIER_ADVANCE_METRIC,
            casper_metrics::PARENTS_POST_STATE_SETTLED_INDEX_BLOCKS_METRIC,
            casper_metrics::PARENTS_POST_STATE_SETTLED_FLOOR_INDEX_BLOCKS_METRIC,
            rspace_metrics::HISTORY_CHECKPOINT_ACTIONS_METRIC,
            rspace_metrics::REPLAY_WAITING_CONTINUATIONS_CHANNEL_DEPTH_METRIC,
            "propagate_ft.scan.blocks",
        ] {
            builder =
                builder.set_buckets_for_metric(Matcher::Full(name.to_owned()), ELEMENT_BUCKETS)?;
        }

        for (name, buckets) in &self.custom_buckets {
            builder = builder.set_buckets_for_metric(Matcher::Full(name.clone()), buckets)?;
        }

        Ok(builder.build_recorder())
    }

    pub fn default() -> Self {
        Self {
            default_buckets: vec![
                BUCKET_5MS,
                BUCKET_10MS,
                BUCKET_25MS,
                BUCKET_50MS,
                BUCKET_75MS,
                BUCKET_100MS,
                BUCKET_250MS,
                BUCKET_500MS,
                BUCKET_750MS,
                BUCKET_1S,
                BUCKET_2_5S,
                BUCKET_5S,
                BUCKET_7_5S,
                BUCKET_10S,
            ],
            time_buckets: vec![
                BUCKET_1MS,
                BUCKET_3MS,
                BUCKET_5MS,
                BUCKET_10MS,
                BUCKET_25MS,
                BUCKET_50MS,
                BUCKET_75MS,
                BUCKET_100MS,
                BUCKET_250MS,
                BUCKET_500MS,
                BUCKET_750MS,
                BUCKET_1S,
                BUCKET_2_5S,
                BUCKET_5S,
                BUCKET_7_5S,
                BUCKET_10S,
            ],
            information_buckets: vec![
                BUCKET_512B,
                BUCKET_1KB,
                BUCKET_2KB,
                BUCKET_4KB,
                BUCKET_16KB,
                BUCKET_64KB,
                BUCKET_512KB,
                BUCKET_1MB,
            ],
            custom_buckets: HashMap::new(),
            include_environment_tags: false,
        }
    }

    pub fn environment_tags(&self) -> HashMap<String, String> {
        if self.include_environment_tags {
            let mut tags = HashMap::new();
            tags.insert("service".to_string(), "rnode".to_string());
            if let Ok(hostname) = std::env::var("HOSTNAME") {
                tags.insert("host".to_string(), hostname);
            }
            tags
        } else {
            HashMap::new()
        }
    }
}

#[cfg(test)]
mod bucket_tests {
    use metrics::{Key, Level, Metadata, Recorder};

    use super::*;

    fn render(config: &PrometheusConfiguration, observations: &[(&str, &[f64])]) -> String {
        let recorder = config.build_recorder().unwrap();
        for (name, values) in observations {
            let histogram = recorder.register_histogram(
                &Key::from_name((*name).to_owned()),
                &Metadata::new("bucket-test", Level::INFO, None),
            );
            for value in *values {
                histogram.record(*value);
            }
        }
        let handle = recorder.handle();
        handle.run_upkeep();
        handle.render()
    }

    fn sample(output: &str, key: &str) -> f64 {
        output
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(' ')?;
                (name == key).then(|| value.parse().unwrap())
            })
            .unwrap_or_else(|| panic!("Missing {key} in:\n{output}"))
    }

    #[test]
    fn time_size_and_element_histograms_use_their_units_and_preserve_totals() {
        let output = render(&PrometheusConfiguration::default(), &[
            (casper_metrics::PROPOSER_CREATE_TIME_METRIC, &[0.002, 2.0]),
            ("connect-time", &[0.002]),
            ("comm_consume_time_seconds", &[0.002]),
            ("exploratory_deploy.duration", &[0.002]),
            (casper_metrics::CASPER_INIT_TIME_TO_RUNNING_METRIC, &[0.002]),
            (casper_metrics::BLOCK_SIZE_METRIC, &[1024.0, 16384.0]),
            (
                rspace_metrics::HISTORY_CHECKPOINT_SERIALIZED_BYTES_METRIC,
                &[4096.0],
            ),
            (casper_metrics::PROPOSER_QUEUE_DEPTH_METRIC, &[0.0, 24.0]),
            (casper_metrics::MERGE_SCOPE_SIZE_METRIC, &[261.0]),
            (rspace_metrics::HISTORY_CHECKPOINT_ACTIONS_METRIC, &[128.0]),
        ]);

        for name in [
            "proposer_create_time",
            "connect_time",
            "comm_consume_time_seconds",
            "exploratory_deploy_duration",
            "casper_init_time_to_running",
        ] {
            assert_eq!(
                sample(&output, &format!("{name}_bucket{{le=\"0.003\"}}")),
                1.0
            );
        }
        assert_eq!(sample(&output, "proposer_create_time_count"), 2.0);
        assert!((sample(&output, "proposer_create_time_sum") - 2.002).abs() < 1e-12);
        assert_eq!(sample(&output, "block_size_bucket{le=\"1024\"}"), 1.0);
        assert_eq!(sample(&output, "block_size_bucket{le=\"16384\"}"), 2.0);
        assert_eq!(sample(&output, "block_size_bucket{le=\"+Inf\"}"), 2.0);
        assert_eq!(sample(&output, "block_size_count"), 2.0);
        assert_eq!(sample(&output, "block_size_sum"), 17408.0);
        assert_eq!(
            sample(
                &output,
                "history_checkpoint_serialized_bytes_bucket{le=\"4096\"}"
            ),
            1.0
        );
        assert_eq!(
            sample(&output, "proposer_queue_depth_bucket{le=\"0\"}"),
            1.0
        );
        assert_eq!(
            sample(&output, "proposer_queue_depth_bucket{le=\"16\"}"),
            1.0
        );
        assert_eq!(
            sample(&output, "proposer_queue_depth_bucket{le=\"32\"}"),
            2.0
        );
        assert_eq!(sample(&output, "proposer_queue_depth_count"), 2.0);
        assert_eq!(sample(&output, "proposer_queue_depth_sum"), 24.0);
        assert_eq!(
            sample(
                &output,
                "compute_parents_post_state_merge_scope_size_bucket{le=\"512\"}"
            ),
            1.0
        );
        assert_eq!(
            sample(&output, "history_checkpoint_actions_bucket{le=\"128\"}"),
            1.0
        );
    }

    #[test]
    fn custom_buckets_override_unit_rules_and_unknown_metrics_keep_the_fallback() {
        let mut config = PrometheusConfiguration::default();
        config.default_buckets = vec![0.5];
        config.custom_buckets.insert(
            casper_metrics::PROPOSER_CREATE_TIME_METRIC.to_owned(),
            vec![1.0, 60.0],
        );
        config
            .custom_buckets
            .insert(casper_metrics::BLOCK_SIZE_METRIC.to_owned(), vec![
                2048.0, 65536.0,
            ]);
        config.custom_buckets.insert(
            casper_metrics::PROPOSER_QUEUE_DEPTH_METRIC.to_owned(),
            vec![3.0, 30.0],
        );

        let output = render(&config, &[
            (casper_metrics::PROPOSER_CREATE_TIME_METRIC, &[30.0]),
            (casper_metrics::BLOCK_SIZE_METRIC, &[4096.0]),
            (casper_metrics::PROPOSER_QUEUE_DEPTH_METRIC, &[24.0]),
            ("unclassified_metric", &[0.25]),
        ]);

        assert_eq!(
            sample(&output, "proposer_create_time_bucket{le=\"1\"}"),
            0.0
        );
        assert_eq!(
            sample(&output, "proposer_create_time_bucket{le=\"60\"}"),
            1.0
        );
        assert_eq!(sample(&output, "block_size_bucket{le=\"2048\"}"), 0.0);
        assert_eq!(sample(&output, "block_size_bucket{le=\"65536\"}"), 1.0);
        assert_eq!(
            sample(&output, "proposer_queue_depth_bucket{le=\"30\"}"),
            1.0
        );
        assert_eq!(
            sample(&output, "unclassified_metric_bucket{le=\"0.5\"}"),
            1.0
        );
        assert!(!output.contains("proposer_create_time_bucket{le=\"0.003\"}"));
        assert!(!output.contains("block_size_bucket{le=\"512\"}"));
    }

    #[test]
    fn empty_bucket_sets_are_rejected_before_installing_a_recorder() {
        for field in ["default", "time", "information", "custom"] {
            let mut config = PrometheusConfiguration::default();
            match field {
                "default" => config.default_buckets.clear(),
                "time" => config.time_buckets.clear(),
                "information" => config.information_buckets.clear(),
                "custom" => {
                    config.custom_buckets.insert("custom".to_owned(), vec![]);
                }
                _ => unreachable!(),
            }
            assert!(config.build_recorder().is_err(), "{field}");
        }
    }
}
