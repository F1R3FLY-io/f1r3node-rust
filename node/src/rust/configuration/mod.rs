//! Configuration module for F1r3fly node.
//!
//! This module provides configuration management for the F1r3fly node,
//! including command-line argument parsing, configuration file loading,
//! and configuration merging with proper precedence.

pub mod commandline;
pub mod config_check;
pub mod model;

pub use commandline::Options;
pub use model::{NodeConf, Profile};

/// Embedded HOCON defaults — what every node starts from before applying
/// the optional `<data-dir>/rnode.conf` override and CLI flags. Baked in
/// at compile time so the binary is self-contained (no `DEFAULT_DIR` env
/// var, no on-disk `node/src/main/resources/defaults.conf` lookup).
const EMBEDDED_DEFAULTS: &str = include_str!("../../main/resources/defaults.conf");

/// Configuration building and parsing functionality
pub mod builder {
    use std::collections::HashMap;
    use std::env;
    use std::path::PathBuf;

    use casper::rust::blocks::proposer::block_creator::FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS;

    use super::*;
    use crate::rust::configuration::commandline::ConfigMapper;

    /// Builds Configuration instance from CLI options.
    /// If config file is provided as part of CLI options, it shall be parsed and merged
    /// with CLI options having higher priority.
    ///
    /// Returns the resolved configuration along with any non-fatal warning
    /// messages produced during validation. The caller is responsible for
    /// emitting the warnings via `tracing::warn!` after the tracing
    /// subscriber has been installed.
    pub fn build(
        options: Options,
    ) -> eyre::Result<(NodeConf, Profile, Option<PathBuf>, Vec<String>)> {
        let profile = options
            .profile
            .as_ref()
            .and_then(|p| profiles().get(p).cloned())
            .unwrap_or_else(|| default_profile());

        let (data_dir, config_file_path) = options
            .subcommand
            .as_ref()
            .and_then(|subcommand| match &subcommand {
                &commandline::options::OptionsSubCommand::Run(run_options) => Some((
                    run_options
                        .data_dir
                        .clone()
                        .unwrap_or_else(|| profile.data_dir.0.clone()),
                    run_options
                        .config_file
                        .clone()
                        .unwrap_or_else(|| profile.data_dir.0.join("rnode.conf")),
                )),
                _ => None,
            })
            .unwrap_or_else(|| {
                (
                    profile.data_dir.0.clone(),
                    profile.data_dir.0.join("rnode.conf"),
                )
            });

        let config_file: Option<PathBuf> = if config_file_path.exists() {
            Some(config_file_path)
        } else {
            None
        };

        // Build configuration from multiple sources with proper precedence:
        // 1. CLI options (highest priority)
        // 2. Config file (`<data-dir>/rnode.conf` or `--config-file <path>`)
        // 3. Embedded defaults baked into the binary (lowest priority)
        let default_config = hocon::HoconLoader::new().load_str(super::EMBEDDED_DEFAULTS)?;

        // Merging the embedded defaults with the optional override
        let merged_config = config_file
            .as_ref()
            .map(|config_file| default_config.load_file(config_file))
            .unwrap_or(Ok(default_config))?;

        let mut node_conf: NodeConf = merged_config.resolve()?;

        // Set data_dir if it's empty (HOCON couldn't resolve ${default-data-dir})
        if node_conf.storage.data_dir.as_os_str().is_empty() {
            node_conf.storage.data_dir = data_dir.clone();
            // Also fix TLS paths which depend on data_dir
            node_conf.tls.certificate_path = data_dir.join("node.certificate.pem");
            node_conf.tls.key_path = data_dir.join("node.key.pem");
            // Fix genesis data dir which also depends on data_dir
            node_conf.casper.genesis_block_data.genesis_data_dir =
                data_dir.join("genesis").to_string_lossy().to_string();
            node_conf.casper.genesis_block_data.bonds_file = data_dir
                .join("genesis")
                .join("bonds.txt")
                .to_string_lossy()
                .to_string();
            node_conf.casper.genesis_block_data.wallets_file = data_dir
                .join("genesis")
                .join("wallets.txt")
                .to_string_lossy()
                .to_string();
        }

        // override config values with CLI options
        node_conf.override_config_values(options);

        // Validate configuration, collecting non-fatal warnings to emit
        // after the tracing subscriber is installed.
        let mut warnings = validate_config(&node_conf)?;

        let (node_conf, dev_warnings) = check_dev_mode(node_conf);
        warnings.extend(dev_warnings);

        Ok((node_conf, profile, config_file, warnings))
    }

    /// Validate configuration parameters. Returns non-fatal warning
    /// messages; fatal errors are returned via `Err`.
    pub(crate) fn validate_config(node_conf: &NodeConf) -> eyre::Result<Vec<String>> {
        let mut warnings = Vec::new();
        let pos_multi_sig_quorum = node_conf.casper.genesis_block_data.pos_multi_sig_quorum;
        let pos_multi_sig_public_keys_length = node_conf
            .casper
            .genesis_block_data
            .pos_multi_sig_public_keys
            .len();

        if pos_multi_sig_quorum > pos_multi_sig_public_keys_length as u32 {
            eyre::bail!(
                "defaults.conf: The value 'pos-multi-sig-quorum' should be less or equal the length of 'pos-multi-sig-public-keys' \
                (the actual values are '{}' and '{}' respectively)",
                pos_multi_sig_quorum,
                pos_multi_sig_public_keys_length
            );
        }

        // Reject empty/whitespace native token name/symbol and out-of-range
        // decimals before the node starts. Catches misconfigured shell variable
        // expansion, typos, and values outside the industry-standard range.
        node_conf
            .casper
            .genesis_block_data
            .validate_native_token()
            .map_err(|e| eyre::eyre!("native token config invalid: {}", e))?;

        // The proposer computes its recovery cap as
        // `max(pending_deploy_max_lag, deploy_recovery_max_lag)`. When
        // deploy_recovery is set below pending_deploy, the recovery knob
        // collapses to the pending floor and has no effect — warn the
        // operator instead of letting the misconfiguration sit silently.
        let pending_deploy_max_lag = node_conf
            .casper
            .heartbeat_conf
            .advanced
            .pending_deploy_max_lag;
        let deploy_recovery_max_lag = node_conf
            .casper
            .heartbeat_conf
            .advanced
            .deploy_recovery_max_lag;
        if deploy_recovery_max_lag < pending_deploy_max_lag {
            warnings.push(format!(
                "casper.heartbeat.advanced.deploy-recovery-max-lag ({}) is less than \
                pending-deploy-max-lag ({}); the recovery knob has no effect under this \
                configuration. Set deploy-recovery-max-lag >= pending-deploy-max-lag.",
                deploy_recovery_max_lag, pending_deploy_max_lag,
            ));
        }

        // A negative threshold weakens "finalized" from a BFT certificate
        // (mutual-witnessing clique; revert requires >= theta equivocating
        // stake) to bare majority agreement per snapshot, which can flip
        // between views under concurrent proposal. Legitimate for test/dev
        // shards that want instant finalization; a production shard should
        // run theta >= 0 — make the choice visible, never accidental.
        let ftt = node_conf.casper.fault_tolerance_threshold;
        if ftt < 0.0 {
            warnings.push(format!(
                "casper.fault-tolerance-threshold ({}) is negative: finalization is bare \
                majority agreement per snapshot, not a BFT certificate. This is a test/dev \
                regime; production shards should use a threshold >= 0.",
                ftt,
            ));
        }

        // A build longer than the citability window is born below the
        // parent-depth horizon; warn above a third of it (the derived
        // default sits at a fifth).
        let play_budget = node_conf.casper.deploy_play_budget;
        let max_parent_depth = node_conf.casper.max_parent_depth;
        if !play_budget.is_zero() && max_parent_depth != i32::MAX && max_parent_depth > 0 {
            let citability_window = node_conf
                .casper
                .heartbeat_conf
                .check_interval
                .saturating_mul(max_parent_depth as u32);
            if play_budget > citability_window / 3 {
                warnings.push(format!(
                    "casper.deploy-play-budget ({:?}) exceeds a third of the citability \
                    window (max-parent-depth {} x heartbeat.check-interval {:?} = {:?}): \
                    a carrier built for that long risks being born below the parent-depth \
                    horizon, where its deploys can only expire. Lower the budget or raise \
                    max-parent-depth.",
                    play_budget,
                    max_parent_depth,
                    node_conf.casper.heartbeat_conf.check_interval,
                    citability_window,
                ));
            }
        }

        // A dropped delivery freezes the receiver's view until the retriever's
        // anchor fires; the citability window must leave recovery a wide
        // margin. The anchor is swept by the casper maintenance loop, so the
        // EFFECTIVE re-request clock is the slower of the two.
        let recovery_anchor = std::time::Duration::from_millis(
            casper::rust::engine::block_retriever::UNRESOLVED_REREQUEST_ANCHOR_MS,
        )
        .max(node_conf.casper.casper_loop_interval);
        if max_parent_depth != i32::MAX && max_parent_depth > 0 {
            let citability_window = node_conf
                .casper
                .heartbeat_conf
                .check_interval
                .saturating_mul(max_parent_depth as u32);
            if citability_window < recovery_anchor.saturating_mul(10) {
                warnings.push(format!(
                    "the citability window (max-parent-depth {} x heartbeat.check-interval \
                    {:?} = {:?}) is under 10x the dependency re-request anchor's effective \
                    clock ({:?}): one lost block delivery can outlive the window before \
                    recovery retries, converting a dropped packet into a finality stall. \
                    Raise max-parent-depth or slow the cadence.",
                    max_parent_depth,
                    node_conf.casper.heartbeat_conf.check_interval,
                    citability_window,
                    recovery_anchor,
                ));
            }
        }

        // Values the node cannot behave correctly under: fail startup rather
        // than run inverted windows or nonsense rules.
        if max_parent_depth <= 0 {
            return Err(eyre::eyre!(
                "casper.max-parent-depth ({}) must be positive (or i32::MAX to disable \
                the depth check): the parent-spread validity rule and the citability \
                horizon are undefined at zero or below",
                max_parent_depth,
            ));
        }
        if node_conf.casper.deploy_lifespan <= 0 {
            return Err(eyre::eyre!(
                "casper.deploy-lifespan ({}) must be positive: a non-positive validity \
                window inverts the expiry and repeat-deploy rules",
                node_conf.casper.deploy_lifespan,
            ));
        }
        if node_conf.casper.min_phlo_price < 0 {
            return Err(eyre::eyre!(
                "casper.min-phlo-price ({}) must be non-negative",
                node_conf.casper.min_phlo_price,
            ));
        }
        if node_conf.casper.genesis_block_data.epoch_length <= 0 {
            return Err(eyre::eyre!(
                "casper.genesis-block-data.epoch-length ({}) must be positive: epoch \
                arithmetic (slash activation, closeBlock boundaries) divides by it",
                node_conf.casper.genesis_block_data.epoch_length,
            ));
        }

        // The mpd sentinel's real side effect is easy to miss: with no
        // citability horizon, nothing is ever provably beyond contest.
        if max_parent_depth == i32::MAX {
            warnings.push(
                "casper.max-parent-depth is i32::MAX (depth check disabled): terminal \
                Expired/Failed deploy verdicts are never written — deploys that miss \
                inclusion stay Pending forever by design"
                    .to_string(),
            );
        }

        // The item-2 lesson: every request leg toward an unreachable peer
        // costs up to network-timeout, so a timeout at or above the proposal
        // cadence lets one dead peer tax each round end to end.
        let network_timeout = node_conf.protocol_client.network_timeout;
        let check_interval = node_conf.casper.heartbeat_conf.check_interval;
        if network_timeout > check_interval {
            warnings.push(format!(
                "protocol-client.network-timeout ({:?}) exceeds \
                heartbeat.check-interval ({:?}): one unreachable peer can tax every \
                proposal round by a full timeout",
                network_timeout, check_interval,
            ));
        }

        if !node_conf.casper.enable_mergeable_channel_gc {
            warnings.push(
                "casper.enable-mergeable-channel-gc is false: mergeable-channel data is \
                retained forever and the store grows without bound"
                    .to_string(),
            );
        }

        // I3: the width cap only bounds validity-window burn during a stall
        // if it sits at or below the citability depth.
        let width_cap = node_conf
            .casper
            .heartbeat_conf
            .advanced
            .empty_frontier_max_unfinalized_blocks;
        if max_parent_depth != i32::MAX && width_cap > max_parent_depth as i64 {
            return Err(eyre::eyre!(
                "casper.heartbeat.advanced.empty-frontier-max-unfinalized-blocks ({}) \
                exceeds casper.max-parent-depth ({}): a width cap above the citability \
                depth cannot stop validity-window burn during a stall, which is its only \
                job. Set the cap at or below max-parent-depth (with margin for proposals \
                already in flight)",
                width_cap,
                max_parent_depth,
            ));
        }
        // I4: a cap at or below the hard finality-lag tier inverts the
        // backpressure ladder — legitimate in tests, surprising in production.
        if width_cap <= FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS {
            warnings.push(format!(
                "casper.heartbeat.advanced.empty-frontier-max-unfinalized-blocks ({}) is \
                at or below the hard finality-lag backpressure tier ({}): the width cap \
                engages before the backpressure ladder that is meant to precede it",
                width_cap, FINALITY_LAG_HARD_BACKPRESSURE_BLOCKS,
            ));
        }
        // I4 tail: mpd < deploy-lifespan.
        if max_parent_depth != i32::MAX
            && node_conf.casper.deploy_lifespan <= max_parent_depth as i64
        {
            warnings.push(format!(
                "casper.deploy-lifespan ({}) is at or below casper.max-parent-depth ({}): \
                deploys can expire inside the citability window",
                node_conf.casper.deploy_lifespan, max_parent_depth,
            ));
        }

        Ok(warnings)
    }

    /// Check dev mode and adjust configuration accordingly. Returns the
    /// (possibly modified) NodeConf along with any non-fatal warnings.
    fn check_dev_mode(node_conf: NodeConf) -> (NodeConf, Vec<String>) {
        if node_conf.dev_mode {
            (node_conf, Vec::new())
        } else {
            let mut warnings = Vec::new();
            if node_conf.dev.deployer_private_key.is_some() {
                warnings
                    .push("Node is not in dev mode, ignoring --deployer-private-key".to_string());
            }
            let updated = NodeConf {
                dev: model::DevConf {
                    deployer_private_key: None,
                },
                ..node_conf
            };
            (updated, warnings)
        }
    }

    fn docker_profile() -> Profile {
        Profile {
            name: "docker",
            data_dir: (
                PathBuf::from("/var/lib/rnode"),
                "Defaults to /var/lib/rnode",
            ),
        }
    }

    fn default_profile() -> Profile {
        // Resolve $HOME (fallback to current dir if not set)
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .or_else(|| env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));

        let path = home.join(".rnode");

        Profile {
            name: "default",
            data_dir: (path, "Defaults to $HOME/.rnode"),
        }
    }

    pub fn profiles() -> HashMap<String, Profile> {
        let mut map = HashMap::new();
        let def = default_profile();
        let dock = docker_profile();
        map.insert(def.name.to_string(), def);
        map.insert(dock.name.to_string(), dock);
        map
    }
}

// Re-export commonly used types
pub use builder::build;

#[cfg(test)]
mod heartbeat_conf_hocon_tests {
    //! Targeted HOCON deserialization tests for the heartbeat tuning fields.
    //!
    //! Lives here (rather than in `casper::casper_conf`) because the `hocon`
    //! crate is a `node` dependency, not a `casper` dependency. Exercises
    //! the same `serde::Deserialize` path the production binary uses.

    use std::time::Duration;

    use casper::rust::casper_conf::{HeartbeatAdvancedConf, HeartbeatConf};

    fn parse_heartbeat(hocon_text: &str) -> HeartbeatConf {
        try_parse_heartbeat(hocon_text).expect("HOCON should deserialize into HeartbeatConf")
    }

    fn try_parse_heartbeat(hocon_text: &str) -> Result<HeartbeatConf, String> {
        let loader = hocon::HoconLoader::new()
            .load_str(hocon_text)
            .map_err(|e| format!("hocon load: {e}"))?;
        loader.resolve().map_err(|e| format!("hocon resolve: {e}"))
    }

    #[test]
    fn full_block_with_advanced_round_trips() {
        let cfg = parse_heartbeat(
            r#"
            enabled = true
            check-interval = 7 seconds
            max-lfb-age = 8 seconds
            self-propose-cooldown = 9 seconds
            stale-recovery-min-interval = 11 seconds
            finality-progress-timeout = 30 seconds
            deploy-finalization-grace = 22 seconds
            advanced {
              frontier-chase-max-lag = 1
              pending-deploy-max-lag = 33
              deploy-recovery-max-lag = 99
              empty-frontier-max-unfinalized-blocks = 44
            }
            "#,
        );

        assert!(cfg.enabled);
        assert_eq!(cfg.check_interval, Duration::from_secs(7));
        assert_eq!(cfg.max_lfb_age, Duration::from_secs(8));
        assert_eq!(cfg.self_propose_cooldown, Duration::from_secs(9));
        assert_eq!(cfg.stale_recovery_min_interval, Duration::from_secs(11));
        assert_eq!(cfg.finality_progress_timeout, Duration::from_secs(30));
        assert_eq!(cfg.deploy_finalization_grace, Duration::from_secs(22));
        assert_eq!(cfg.advanced.frontier_chase_max_lag, 1);
        assert_eq!(cfg.advanced.pending_deploy_max_lag, 33);
        assert_eq!(cfg.advanced.deploy_recovery_max_lag, 99);
        assert_eq!(cfg.advanced.empty_frontier_max_unfinalized_blocks, 44);
    }

    #[test]
    fn missing_new_fields_fall_back_to_defaults() {
        // A HOCON config that omits the new keys must still parse and use
        // the defaults declared on HeartbeatConf / HeartbeatAdvancedConf.
        let cfg = parse_heartbeat(
            r#"
            enabled = false
            check-interval = 5 seconds
            max-lfb-age = 5 seconds
            "#,
        );

        assert_eq!(cfg.self_propose_cooldown, Duration::from_secs(3));
        assert_eq!(cfg.stale_recovery_min_interval, Duration::from_secs(3));
        assert_eq!(cfg.deploy_finalization_grace, Duration::from_secs(25));
        assert_eq!(cfg.advanced, HeartbeatAdvancedConf::default());
    }

    #[test]
    fn partial_advanced_block_defaults_remaining_fields() {
        // A partial advanced block fills missing fields with defaults
        // rather than failing to parse.
        let cfg = parse_heartbeat(
            r#"
            enabled = false
            check-interval = 5 seconds
            max-lfb-age = 5 seconds
            advanced {
              pending-deploy-max-lag = 7
            }
            "#,
        );

        assert_eq!(cfg.advanced.frontier_chase_max_lag, 20);
        assert_eq!(cfg.advanced.pending_deploy_max_lag, 7);
        assert_eq!(cfg.advanced.deploy_recovery_max_lag, 64);
        assert_eq!(cfg.advanced.empty_frontier_max_unfinalized_blocks, 12);
    }

    #[test]
    fn negative_advanced_lag_values_are_rejected() {
        // Negative caps would silently disable the corresponding code
        // path in the proposer (e.g. `lag <= cap` where cap < 0 is
        // never true). Each advanced field rejects at
        // deserialization time.
        for field in &[
            "frontier-chase-max-lag",
            "pending-deploy-max-lag",
            "deploy-recovery-max-lag",
            "empty-frontier-max-unfinalized-blocks",
        ] {
            let hocon = format!(
                r#"
                enabled = false
                check-interval = 5 seconds
                max-lfb-age = 5 seconds
                advanced {{
                  {field} = -1
                }}
                "#
            );
            let result = try_parse_heartbeat(&hocon);
            assert!(
                result.is_err(),
                "negative {field} should fail HOCON deserialization, got Ok({:?})",
                result.ok()
            );
            let err = result.unwrap_err();
            assert!(
                err.contains("value must be >= 0"),
                "error for {field} should mention non-negative requirement, got: {err}"
            );
        }
    }
}

#[cfg(test)]
mod embedded_defaults_tests {
    use std::time::Duration;

    use shared::rust::tracing_init::{LogFormat, LogRotation, LogSink};

    use super::*;

    #[test]
    fn embedded_defaults_deserialize_into_node_conf() {
        let cfg: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        assert_eq!(
            cfg.logging.filter,
            "info,tonic=error,hyper=error,tower=error,reqwest=error,heed=error,h2=error,comm::rust::transport::transport_layer=warn,casper::rust::engine::block_retriever=warn,casper::rust::engine::multi_parent_casper::validation_dispatcher=warn,casper::rust::util::rholang::interpreter_util=warn"
        );
        assert!(matches!(cfg.logging.format, LogFormat::Json));
        assert!(matches!(cfg.logging.sink, LogSink::Stdout));
        assert!(matches!(cfg.logging.file.rotation, LogRotation::Daily));
        assert_eq!(cfg.logging.file.retention, 14);
        assert_eq!(cfg.api_server.exploratory_deploy_max_concurrent, 1);
        assert_eq!(cfg.api_server.exploratory_deploy_phlo_limit, 5_000_000);
        assert_eq!(
            cfg.api_server.exploratory_deploy_execution_timeout,
            Duration::from_secs(15)
        );
    }

    /// A negative fault-tolerance threshold weakens "finalized" from a BFT
    /// certificate to bare majority agreement per snapshot — a legitimate
    /// test/dev sentinel, but one an operator must choose with eyes open.
    /// Startup surfaces it as a warning; non-negative thresholds stay silent.
    #[test]
    fn a_negative_fault_tolerance_threshold_warns_at_startup() {
        let mut cfg: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        cfg.casper.fault_tolerance_threshold = -1.0;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("fault-tolerance-threshold") && w.contains("majority")),
            "negative ftt must warn that finalization is majority-agreement, got {warnings:?}"
        );

        cfg.casper.fault_tolerance_threshold = 0.0;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings
                .iter()
                .any(|w| w.contains("fault-tolerance-threshold")),
            "non-negative ftt must not warn, got {warnings:?}"
        );
    }

    /// The play budget exists to keep a proposed block citable: a build longer
    /// than `max-parent-depth` heights of heartbeat cadence is born below the
    /// parent-depth horizon and its deploys can only expire. An operator
    /// override above a third of that window defeats the knob's purpose —
    /// startup surfaces it; the derived default and sane overrides stay silent.
    #[test]
    fn an_oversized_deploy_play_budget_warns_at_startup() {
        let mut cfg: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        // Shipped geometry: mpd 15 x check-interval 5s = 75s window; a 60s
        // budget leaves no citability margin at all.
        cfg.casper.deploy_play_budget = Duration::from_secs(60);
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("deploy-play-budget") && w.contains("max-parent-depth")),
            "a budget above a third of the citability window must warn, got {warnings:?}"
        );

        cfg.casper.deploy_play_budget = Duration::from_secs(10);
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings.iter().any(|w| w.contains("deploy-play-budget")),
            "a budget inside the citability margin must not warn, got {warnings:?}"
        );

        cfg.casper.deploy_play_budget = Duration::ZERO;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings.iter().any(|w| w.contains("deploy-play-budget")),
            "the derived sentinel must not warn, got {warnings:?}"
        );

        // Disabled depth checks mean no citability horizon to violate.
        cfg.casper.deploy_play_budget = Duration::from_secs(3600);
        cfg.casper.max_parent_depth = i32::MAX;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings.iter().any(|w| w.contains("deploy-play-budget")),
            "a disabled parent-depth check must not warn on any budget, got {warnings:?}"
        );
    }

    /// A citability window near the recovery anchor warns; shipped geometry
    /// and a disabled depth check stay silent.
    #[test]
    fn a_citability_window_near_the_recovery_anchor_warns_at_startup() {
        let mut cfg: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings.iter().any(|w| w.contains("re-request anchor")),
            "shipped geometry must not warn, got {warnings:?}"
        );

        // 4 x 1s = 4s window against a 500ms anchor: under the 10x margin.
        // The width cap follows mpd down to keep the I3 check satisfied.
        cfg.casper.max_parent_depth = 4;
        cfg.casper
            .heartbeat_conf
            .advanced
            .empty_frontier_max_unfinalized_blocks = 4;
        cfg.casper.heartbeat_conf.check_interval = Duration::from_secs(1);
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("re-request anchor") && w.contains("max-parent-depth")),
            "a citability window under 10x the anchor must warn, got {warnings:?}"
        );

        cfg.casper.max_parent_depth = i32::MAX;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            !warnings.iter().any(|w| w.contains("re-request anchor")),
            "a disabled parent-depth check must not warn, got {warnings:?}"
        );
    }

    /// Startup must refuse geometry the validity rules are undefined under;
    /// shipped defaults must pass with none of the new warnings.
    #[test]
    fn invalid_consensus_values_fail_startup() {
        let base: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        let shipped = builder::validate_config(&base).expect("shipped defaults must validate");
        assert!(
            !shipped.iter().any(|w| w.contains("network-timeout")
                || w.contains("mergeable-channel-gc")
                || w.contains("depth check disabled")),
            "shipped geometry must be silent on the new checks, got {shipped:?}"
        );

        let mut cfg = base.clone();
        cfg.casper.max_parent_depth = 0;
        assert!(
            builder::validate_config(&cfg).is_err(),
            "max-parent-depth 0 must fail startup"
        );

        let mut cfg = base.clone();
        cfg.casper.deploy_lifespan = 0;
        assert!(
            builder::validate_config(&cfg).is_err(),
            "deploy-lifespan 0 must fail startup"
        );

        let mut cfg = base.clone();
        cfg.casper.min_phlo_price = -1;
        assert!(
            builder::validate_config(&cfg).is_err(),
            "negative min-phlo-price must fail startup"
        );

        let mut cfg = base;
        cfg.casper.genesis_block_data.epoch_length = 0;
        assert!(
            builder::validate_config(&cfg).is_err(),
            "epoch-length 0 must fail startup"
        );
    }

    /// Side-effect configurations warn: the mpd sentinel disables terminal
    /// deploy verdicts, an oversized network timeout taxes every proposal
    /// round, and disabled mergeable-GC grows the store without bound.
    #[test]
    fn side_effect_configurations_warn_at_startup() {
        let base: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        let mut cfg = base.clone();
        cfg.casper.max_parent_depth = i32::MAX;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings.iter().any(|w| w.contains("depth check disabled")),
            "the mpd sentinel must surface its terminal-verdict side effect, got {warnings:?}"
        );

        let mut cfg = base.clone();
        cfg.protocol_client.network_timeout = Duration::from_secs(15);
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings.iter().any(|w| w.contains("network-timeout")),
            "a network timeout above the proposal cadence must warn, got {warnings:?}"
        );

        let mut cfg = base;
        cfg.casper.enable_mergeable_channel_gc = false;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings.iter().any(|w| w.contains("mergeable-channel-gc")),
            "disabled mergeable-GC must warn about unbounded growth, got {warnings:?}"
        );
    }

    /// I3/I4 width-cap geometry: shipped values pass silently, a cap above
    /// max-parent-depth fails startup, a cap at or below the hard tier warns.
    #[test]
    fn width_cap_geometry_is_validated_at_startup() {
        let base: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");

        let shipped = builder::validate_config(&base).expect("shipped geometry must validate");
        assert!(
            !shipped
                .iter()
                .any(|w| w.contains("empty-frontier-max-unfinalized-blocks")
                    || (w.contains("deploy-lifespan") && w.contains("max-parent-depth"))),
            "shipped geometry must be silent on the width-cap checks, got {shipped:?}"
        );

        let mut cfg = base.clone();
        cfg.casper
            .heartbeat_conf
            .advanced
            .empty_frontier_max_unfinalized_blocks = 64;
        assert!(
            builder::validate_config(&cfg).is_err(),
            "a width cap above max-parent-depth must fail startup"
        );

        let mut cfg = base.clone();
        cfg.casper.max_parent_depth = i32::MAX;
        cfg.casper
            .heartbeat_conf
            .advanced
            .empty_frontier_max_unfinalized_blocks = 64;
        assert!(
            builder::validate_config(&cfg).is_ok(),
            "a disabled depth check leaves no citability depth for the cap to violate"
        );

        let mut cfg = base.clone();
        cfg.casper
            .heartbeat_conf
            .advanced
            .empty_frontier_max_unfinalized_blocks = 4;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("empty-frontier-max-unfinalized-blocks")
                    && w.contains("backpressure")),
            "a cap at or below the hard backpressure tier must warn, got {warnings:?}"
        );

        let mut cfg = base;
        cfg.casper.deploy_lifespan = 15;
        let warnings = builder::validate_config(&cfg).expect("validate");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("deploy-lifespan") && w.contains("citability window")),
            "a lifespan at or below max-parent-depth must warn, got {warnings:?}"
        );
    }

    /// The full heartbeat block, pinned twice over: the SHIPPED defaults.conf
    /// values, and the serde fallbacks a sparse operator conf (omitting every
    /// optional heartbeat key) lands on. The two must be identical — a
    /// deployment that copies less of the file must not get different
    /// behavior than every tested one (self-propose-cooldown once shipped 3 s
    /// while the code fallback was 15 s, and frontier-chase-max-lag shipped
    /// 20 against a fallback of 0, the configuration the file itself warns
    /// stops validators contributing under load).
    #[test]
    fn the_heartbeat_block_ships_pinned_values_and_matching_fallbacks() {
        let cfg: NodeConf = hocon::HoconLoader::new()
            .load_str(EMBEDDED_DEFAULTS)
            .expect("load defaults.conf")
            .resolve()
            .expect("deserialize NodeConf");
        let shipped = &cfg.casper.heartbeat_conf;

        assert!(shipped.enabled);
        assert_eq!(shipped.check_interval, Duration::from_secs(5));
        assert_eq!(shipped.max_lfb_age, Duration::from_secs(5));
        assert_eq!(shipped.self_propose_cooldown, Duration::from_secs(3));
        assert_eq!(shipped.stale_recovery_min_interval, Duration::from_secs(3));
        assert_eq!(shipped.finality_progress_timeout, Duration::from_secs(30));
        assert_eq!(shipped.deploy_finalization_grace, Duration::from_secs(25));
        assert_eq!(shipped.advanced.frontier_chase_max_lag, 20);
        assert_eq!(shipped.advanced.pending_deploy_max_lag, 20);
        assert_eq!(shipped.advanced.deploy_recovery_max_lag, 64);
        assert_eq!(shipped.advanced.empty_frontier_max_unfinalized_blocks, 12);

        let sparse: casper::rust::casper_conf::HeartbeatConf = hocon::HoconLoader::new()
            .load_str(
                r#"
                enabled = true
                check-interval = 5 seconds
                max-lfb-age = 5 seconds
                "#,
            )
            .expect("load sparse heartbeat conf")
            .resolve()
            .expect("deserialize HeartbeatConf");
        assert_eq!(sparse.self_propose_cooldown, shipped.self_propose_cooldown);
        assert_eq!(
            sparse.stale_recovery_min_interval,
            shipped.stale_recovery_min_interval
        );
        assert_eq!(
            sparse.finality_progress_timeout,
            shipped.finality_progress_timeout
        );
        assert_eq!(
            sparse.deploy_finalization_grace,
            shipped.deploy_finalization_grace
        );
        assert_eq!(sparse.advanced, shipped.advanced);
    }
}
