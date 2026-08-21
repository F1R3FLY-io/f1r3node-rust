use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Casper configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CasperConf {
    #[serde(rename = "fault-tolerance-threshold")]
    pub fault_tolerance_threshold: f32,

    #[serde(rename = "validator-public-key")]
    pub validator_public_key: Option<String>,
    #[serde(rename = "validator-private-key")]
    pub validator_private_key: Option<String>,
    #[serde(rename = "validator-private-key-path")]
    pub validator_private_key_path: Option<PathBuf>,

    #[serde(rename = "shard-name")]
    pub shard_name: String,
    #[serde(rename = "parent-shard-id")]
    pub parent_shard_id: String,

    #[serde(rename = "casper-loop-interval", deserialize_with = "de_duration")]
    pub casper_loop_interval: Duration,
    #[serde(rename = "requested-blocks-timeout", deserialize_with = "de_duration")]
    pub requested_blocks_timeout: Duration,
    #[serde(rename = "finalization-rate")]
    pub finalization_rate: i32,
    #[serde(rename = "max-number-of-parents")]
    pub max_number_of_parents: i32,
    #[serde(rename = "max-parent-depth")]
    pub max_parent_depth: i32,
    #[serde(
        rename = "fork-choice-stale-threshold",
        deserialize_with = "de_duration"
    )]
    pub fork_choice_stale_threshold: Duration,
    #[serde(
        rename = "fork-choice-check-if-stale-interval",
        deserialize_with = "de_duration"
    )]
    pub fork_choice_check_if_stale_interval: Duration,
    #[serde(rename = "synchrony-constraint-threshold")]
    pub synchrony_constraint_threshold: f32,
    #[serde(rename = "height-constraint-threshold")]
    pub height_constraint_threshold: i64,

    #[serde(rename = "round-robin-dispatcher")]
    pub round_robin_dispatcher: RoundRobinDispatcher,

    #[serde(rename = "genesis-block-data")]
    pub genesis_block_data: GenesisBlockData,

    #[serde(rename = "genesis-ceremony")]
    pub genesis_ceremony: GenesisCeremony,

    #[serde(rename = "min-phlo-price")]
    pub min_phlo_price: i64,

    #[serde(rename = "heartbeat")]
    pub heartbeat_conf: HeartbeatConf,

    #[serde(rename = "finalizer", default)]
    pub finalizer: FinalizerConf,

    #[serde(
        rename = "synchrony-recovery-stall-window",
        deserialize_with = "de_duration",
        default = "default_synchrony_recovery_stall_window"
    )]
    pub synchrony_recovery_stall_window: Duration,
    #[serde(
        rename = "synchrony-recovery-cooldown",
        deserialize_with = "de_duration",
        default = "default_synchrony_recovery_cooldown"
    )]
    pub synchrony_recovery_cooldown: Duration,
    #[serde(
        rename = "synchrony-recovery-max-bypasses",
        default = "default_synchrony_recovery_max_bypasses"
    )]
    pub synchrony_recovery_max_bypasses: u32,
    #[serde(
        rename = "synchrony-finalized-baseline-enabled",
        default = "default_synchrony_finalized_baseline_enabled"
    )]
    pub synchrony_finalized_baseline_enabled: bool,
    #[serde(
        rename = "synchrony-finalized-baseline-max-distance",
        default = "default_synchrony_finalized_baseline_max_distance"
    )]
    pub synchrony_finalized_baseline_max_distance: u64,

    #[serde(
        rename = "max-user-deploys-per-block",
        default = "default_max_user_deploys_per_block"
    )]
    pub max_user_deploys_per_block: u32,

    /// Disable late block filtering in DagMerger.
    /// When true (default), all blocks are included in merged state regardless of when
    /// they were observed. This prevents deploy loss during network partitions.
    #[serde(
        rename = "disable-late-block-filtering",
        default = "default_disable_late_block_filtering"
    )]
    pub disable_late_block_filtering: bool,

    /// Enable background garbage collection for mergeable channels.
    /// When enabled, uses safe reachability-based GC (required for multi-parent mode).
    /// When disabled (default), mergeable data is retained.
    #[serde(
        rename = "enable-mergeable-channel-gc",
        default = "default_enable_mergeable_channel_gc"
    )]
    pub enable_mergeable_channel_gc: bool,

    /// Interval for garbage collecting mergeable channels (only when GC enabled).
    /// Background process that safely deletes mergeable data when provably unreachable.
    #[serde(
        rename = "mergeable-channels-gc-interval",
        deserialize_with = "de_duration",
        default = "default_mergeable_channels_gc_interval"
    )]
    pub mergeable_channels_gc_interval: Duration,

    /// Depth buffer for mergeable channels garbage collection (only when GC enabled).
    /// Additional safety margin beyond max-parent-depth before deleting data.
    #[serde(
        rename = "mergeable-channels-gc-depth-buffer",
        default = "default_mergeable_channels_gc_depth_buffer"
    )]
    pub mergeable_channels_gc_depth_buffer: i32,
}

fn default_synchrony_recovery_stall_window() -> Duration { Duration::from_secs(60) }

fn default_synchrony_recovery_cooldown() -> Duration { Duration::from_secs(20) }

fn default_synchrony_recovery_max_bypasses() -> u32 { 2 }

fn default_synchrony_finalized_baseline_enabled() -> bool { true }

fn default_synchrony_finalized_baseline_max_distance() -> u64 { 2048 }

fn default_max_user_deploys_per_block() -> u32 { 128 }

fn default_disable_late_block_filtering() -> bool { true }

/// Default for `client_fuel_allocations`: no additional client fuel at genesis.
fn default_client_fuel_allocations() -> Vec<ClientFuelAllocation> { Vec::new() }

/// Additional fuel credited to a client's canonical SystemVault at genesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientFuelAllocation {
    /// Hex-encoded client public key used to derive its native vault address.
    #[serde(rename = "public-key")]
    pub public_key: String,
    /// Phlogiston added to the client's SystemVault balance at genesis.
    /// Must be `>= 0` (a negative seed is a config error; validated at wiring).
    #[serde(rename = "amount")]
    pub amount: i64,
}

fn default_enable_mergeable_channel_gc() -> bool { false }

fn default_mergeable_channels_gc_interval() -> Duration {
    Duration::from_secs(5 * 60) // 5 minutes
}

fn default_mergeable_channels_gc_depth_buffer() -> i32 { 10 }

/// Default value for `max_cosigners_per_deploy`. 64 is generous
/// defense-in-depth — real-world multi-sig wallets rarely exceed 10–15
/// cosigners. The PoS contract enforces this cap inside `chargeDeploy`.
/// Test fixtures and other defaulting paths MUST reference this constant
/// rather than hardcoding `64` so the default has a single source of truth.
pub const DEFAULT_MAX_COSIGNERS_PER_DEPLOY: u32 = 64;

fn default_max_cosigners_per_deploy() -> u32 { DEFAULT_MAX_COSIGNERS_PER_DEPLOY }

/// Default fuel credited to a validator's SystemVault when it joins the validator set.
pub const DEFAULT_INITIAL_PHLOGISTON: i64 = 1_000_000;

fn default_initial_phlogiston() -> i64 { DEFAULT_INITIAL_PHLOGISTON }

/// Default fuel credited to each eligible active validator at an epoch boundary.
pub const DEFAULT_EPOCH_PHLOGISTON: i64 = 1_000_000;

fn default_epoch_phlogiston() -> i64 { DEFAULT_EPOCH_PHLOGISTON }

/// Round robin dispatcher configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundRobinDispatcher {
    #[serde(rename = "max-peer-queue-size")]
    pub max_peer_queue_size: u32,
    #[serde(rename = "give-up-after-skipped")]
    pub give_up_after_skipped: u32,
    #[serde(rename = "drop-peer-after-retries")]
    pub drop_peer_after_retries: u32,
}

/// Genesis block data configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisBlockData {
    #[serde(rename = "genesis-data-dir")]
    pub genesis_data_dir: String,
    #[serde(rename = "bonds-file")]
    pub bonds_file: String,
    #[serde(rename = "wallets-file")]
    pub wallets_file: String,

    #[serde(rename = "bond-minimum")]
    pub bond_minimum: i64,
    #[serde(rename = "bond-maximum")]
    pub bond_maximum: i64,

    #[serde(rename = "epoch-length")]
    pub epoch_length: i32,
    #[serde(rename = "quarantine-length")]
    pub quarantine_length: i32,

    #[serde(rename = "number-of-active-validators")]
    pub number_of_active_validators: u32,

    #[serde(rename = "deploy-timestamp")]
    pub deploy_timestamp: Option<i64>,

    #[serde(rename = "genesis-block-number")]
    pub genesis_block_number: i64,

    #[serde(rename = "pos-multi-sig-public-keys")]
    pub pos_multi_sig_public_keys: Vec<String>,

    #[serde(rename = "pos-multi-sig-quorum")]
    pub pos_multi_sig_quorum: u32,

    /// Per-deploy hard cap on the number of cosigners in a multi-signature
    /// deploy. Substituted into the PoS contract at genesis as
    /// `$$maxCosignersPerDeploy$$`; `chargeDeploy` rejects when the
    /// in-flight cosigner Map reaches this size. Defense-in-depth against
    /// adversarial deploys with thousands of cosigners exhausting block
    /// resources. Default `64` (generous — real-world multi-sig wallets
    /// rarely exceed 10–15). Must be `>= 1`.
    #[serde(
        rename = "max-cosigners-per-deploy",
        default = "default_max_cosigners_per_deploy"
    )]
    pub max_cosigners_per_deploy: u32,

    /// Fuel credited to a validator's canonical SystemVault when it first bonds.
    #[serde(rename = "initial-phlogiston", default = "default_initial_phlogiston")]
    pub initial_phlogiston: i64,

    /// Fuel credited to each eligible active validator at an epoch boundary.
    #[serde(rename = "epoch-phlogiston", default = "default_epoch_phlogiston")]
    pub epoch_phlogiston: i64,

    /// Additional genesis balances for client SystemVaults. Each entry is
    /// coalesced with any native-token vault balance for the same address before
    /// the blessed vault-generator deploys are constructed.
    #[serde(
        rename = "client-fuel-allocations",
        default = "default_client_fuel_allocations"
    )]
    pub client_fuel_allocations: Vec<ClientFuelAllocation>,

    /// Full display name of the native token. Substituted into the
    /// TokenMetadata Rholang contract at genesis and registered at
    /// `rho:system:tokenMetadata`. Immutable after genesis.
    #[serde(rename = "native-token-name")]
    pub native_token_name: String,

    /// Ticker symbol of the native token. Immutability rules are identical
    /// to `native-token-name`. Operators MUST set this in config before genesis.
    #[serde(rename = "native-token-symbol")]
    pub native_token_symbol: String,

    /// Number of decimal places used to display the native token
    /// (1 token = 10^decimals dust). Immutability rules are identical to
    /// `native-token-name`. Operators MUST set this in config before genesis.
    #[serde(rename = "native-token-decimals")]
    pub native_token_decimals: u32,
}

/// Maximum decimal places accepted for native token. Matches the de-facto
/// ERC-20 standard (ETH uses 18). Values above 18 exceed IEEE-754 double
/// safe-integer range (2^53), which breaks every JavaScript-based wallet
/// and block explorer. No production blockchain uses more than 18
/// (BTC=8, SOL=9, ATOM=6, DOT=10, KSM=12, ETH=18).
pub const MAX_NATIVE_TOKEN_DECIMALS: u32 = 18;

impl GenesisBlockData {
    /// Lower the serde-parsed task #13b client funding-slot allocations
    /// (`[(hex public-key, amount)]`) to `[(crypto::PublicKey, amount)]`,
    /// hex-decoding each key ONCE at startup so a malformed key or a negative
    /// amount fails fast (loudly at launch) rather than being baked into genesis
    /// or silently producing a degenerate `Σ⟦c⟧` seed. Empty in, empty out
    /// (existing shards). The lowered list is wired into `CasperShardConf` and
    /// then into the canonical genesis supply commitment.
    pub fn lowered_client_fuel_allocations(
        &self,
    ) -> Result<Vec<(crypto::rust::public_key::PublicKey, i64)>, String> {
        let mut out = Vec::with_capacity(self.client_fuel_allocations.len());
        for alloc in &self.client_fuel_allocations {
            if alloc.amount < 0 {
                return Err(format!(
                    "client-fuel-allocations: amount must be >= 0 for public-key {}; got {}",
                    alloc.public_key, alloc.amount
                ));
            }
            let bytes = hex::decode(&alloc.public_key).map_err(|e| {
                format!(
                    "client-fuel-allocations: public-key {:?} is not valid hex: {}",
                    alloc.public_key, e
                )
            })?;
            if bytes.is_empty() {
                return Err(
                    "client-fuel-allocations: public-key must decode to non-empty bytes"
                        .to_string(),
                );
            }
            out.push((
                crypto::rust::public_key::PublicKey::from_bytes(&bytes),
                alloc.amount,
            ));
        }
        Ok(out)
    }

    pub fn validate_cost_accounting_parameters(&self) -> Result<(), String> {
        if self.epoch_length <= 0 {
            return Err(format!(
                "epoch-length must be positive; got {}",
                self.epoch_length
            ));
        }
        if self.max_cosigners_per_deploy == 0 {
            return Err("max-cosigners-per-deploy must be at least 1".to_string());
        }
        if self.initial_phlogiston < 0 {
            return Err(format!(
                "initial-phlogiston must be non-negative; got {}",
                self.initial_phlogiston
            ));
        }
        if self.epoch_phlogiston < 0 {
            return Err(format!(
                "epoch-phlogiston must be non-negative; got {}",
                self.epoch_phlogiston
            ));
        }

        let allocations = self.lowered_client_fuel_allocations()?;
        let mut totals = std::collections::BTreeMap::<Vec<u8>, i64>::new();
        for (public_key, amount) in allocations {
            let entry = totals.entry(public_key.bytes.to_vec()).or_default();
            *entry = entry.checked_add(amount).ok_or_else(|| {
                format!(
                    "client-fuel-allocations overflow for public-key {}",
                    hex::encode(&public_key.bytes)
                )
            })?;
        }
        Ok(())
    }

    /// Validates native-token-* fields. Called during config load so a
    /// misconfigured node fails startup loudly rather than baking bad
    /// values into genesis or serving misleading metadata via `/api/status`.
    pub fn validate_native_token(&self) -> Result<(), String> {
        if self.native_token_name.trim().is_empty() {
            return Err(format!(
                "native-token-name must be non-empty and non-whitespace; got {:?}",
                self.native_token_name
            ));
        }
        if self.native_token_symbol.trim().is_empty() {
            return Err(format!(
                "native-token-symbol must be non-empty and non-whitespace; got {:?}",
                self.native_token_symbol
            ));
        }
        if self.native_token_decimals > MAX_NATIVE_TOKEN_DECIMALS {
            return Err(format!(
                "native-token-decimals={} exceeds maximum of {} (industry standard; \
                 ETH=18, BTC=8, SOL=9, ATOM=6); values above {} exceed IEEE-754 \
                 double safe-integer range and break JavaScript clients",
                self.native_token_decimals, MAX_NATIVE_TOKEN_DECIMALS, MAX_NATIVE_TOKEN_DECIMALS
            ));
        }
        Ok(())
    }
}

/// Genesis ceremony configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisCeremony {
    #[serde(rename = "required-signatures")]
    pub required_signatures: i32,

    #[serde(rename = "approve-interval", deserialize_with = "de_duration")]
    pub approve_interval: Duration,

    #[serde(rename = "approve-duration", deserialize_with = "de_duration")]
    pub approve_duration: Duration,

    #[serde(rename = "autogen-shard-size")]
    pub autogen_shard_size: u32,

    #[serde(rename = "genesis-validator-mode")]
    pub genesis_validator_mode: bool,

    #[serde(rename = "ceremony-master-mode")]
    pub ceremony_master_mode: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatConf {
    pub enabled: bool,
    #[serde(rename = "check-interval", deserialize_with = "de_duration")]
    pub check_interval: Duration,
    #[serde(rename = "max-lfb-age", deserialize_with = "de_duration")]
    pub max_lfb_age: Duration,
    #[serde(
        rename = "self-propose-cooldown",
        deserialize_with = "de_duration",
        default = "default_self_propose_cooldown"
    )]
    pub self_propose_cooldown: Duration,
    /// Minimum age of this validator's latest proposal before the pending-deploy
    /// recovery backstop is allowed to fire.
    #[serde(
        rename = "stale-recovery-min-interval",
        deserialize_with = "de_duration",
        default = "default_stale_recovery_min_interval"
    )]
    pub stale_recovery_min_interval: Duration,
    /// When pending deploys land, opens a grace window during which the lag cap
    /// relaxes to `advanced.deploy_recovery_max_lag`.
    #[serde(
        rename = "deploy-finalization-grace",
        deserialize_with = "de_duration",
        default = "default_deploy_finalization_grace"
    )]
    pub deploy_finalization_grace: Duration,
    /// EXPERIMENTAL tuning knobs. See [`HeartbeatAdvancedConf`].
    #[serde(default)]
    pub advanced: HeartbeatAdvancedConf,
}

impl Default for HeartbeatConf {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval: Duration::from_secs(5),
            max_lfb_age: Duration::from_secs(15),
            self_propose_cooldown: default_self_propose_cooldown(),
            stale_recovery_min_interval: default_stale_recovery_min_interval(),
            deploy_finalization_grace: default_deploy_finalization_grace(),
            advanced: HeartbeatAdvancedConf::default(),
        }
    }
}

fn default_self_propose_cooldown() -> Duration { Duration::from_secs(15) }

fn default_stale_recovery_min_interval() -> Duration { Duration::from_secs(12) }

fn default_deploy_finalization_grace() -> Duration { Duration::from_secs(25) }

/// EXPERIMENTAL: tuning knobs for the heartbeat proposer's lag caps.
///
/// These thresholds bound DAG width relative to replay cost in lieu of
/// adaptive backpressure. Treat as unstable API; field names may change.
///
/// All fields must be non-negative; HOCON values < 0 are rejected
/// at deserialization time. The proposer treats these as caps on a
/// non-negative lag count (`lfb_lag_blocks`), so a negative value would
/// silently disable the corresponding code path (e.g. `lag <= cap` where
/// `cap < 0` is never true, leaving pending deploys unproposed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatAdvancedConf {
    /// If the validator has pending deploys but is already > N blocks
    /// ahead of LFB, suppress pending-deploy proposing. Prevents lag
    /// amplification: more deploys → more blocks → wider DAG → slower
    /// finalization → still "ahead" → keeps proposing forever. Lower →
    /// harder load-relief valve.
    #[serde(
        rename = "pending-deploy-max-lag",
        deserialize_with = "de_non_negative_i64",
        default = "default_pending_deploy_max_lag"
    )]
    pub pending_deploy_max_lag: i64,
    /// During an active deploy-finalization grace window, the lag cap
    /// widens to this value. The "absolute safe lag during recovery"
    /// ceiling.
    ///
    /// Invariant: must be `>= pending_deploy_max_lag` to take effect.
    /// The proposer computes the recovery cap as
    /// `max(pending_deploy_max_lag, deploy_recovery_max_lag)`, so a
    /// value below `pending_deploy_max_lag` collapses to that floor and
    /// the knob has no effect.
    #[serde(
        rename = "deploy-recovery-max-lag",
        deserialize_with = "de_non_negative_i64",
        default = "default_deploy_recovery_max_lag"
    )]
    pub deploy_recovery_max_lag: i64,
    #[serde(
        rename = "empty-frontier-max-unfinalized-blocks",
        deserialize_with = "de_non_negative_i64",
        default = "default_empty_frontier_max_unfinalized_blocks"
    )]
    pub empty_frontier_max_unfinalized_blocks: i64,
}

impl Default for HeartbeatAdvancedConf {
    fn default() -> Self {
        Self {
            pending_deploy_max_lag: default_pending_deploy_max_lag(),
            deploy_recovery_max_lag: default_deploy_recovery_max_lag(),
            empty_frontier_max_unfinalized_blocks: default_empty_frontier_max_unfinalized_blocks(),
        }
    }
}

fn default_pending_deploy_max_lag() -> i64 { 20 }

fn default_deploy_recovery_max_lag() -> i64 { 64 }

fn default_empty_frontier_max_unfinalized_blocks() -> i64 { 64 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalizerConf {
    #[serde(
        rename = "yield-interval",
        deserialize_with = "de_duration",
        default = "default_finalizer_yield_interval"
    )]
    pub yield_interval: Duration,
    #[serde(
        rename = "catchup-yield-interval",
        deserialize_with = "de_duration",
        default = "default_finalizer_catchup_yield_interval"
    )]
    pub catchup_yield_interval: Duration,
}

impl Default for FinalizerConf {
    fn default() -> Self {
        Self {
            yield_interval: default_finalizer_yield_interval(),
            catchup_yield_interval: default_finalizer_catchup_yield_interval(),
        }
    }
}

fn default_finalizer_yield_interval() -> Duration { Duration::from_millis(1) }

fn default_finalizer_catchup_yield_interval() -> Duration { Duration::from_millis(1) }

pub fn de_duration<'de, D>(deserializer: D) -> Result<Duration, D::Error>
where D: serde::Deserializer<'de> {
    use serde::de::Error as _;
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DurIn {
        Str(String),
        Secs(u64),
        FloatSecs(f64),
    }
    match DurIn::deserialize(deserializer)? {
        DurIn::Str(s) => humantime::parse_duration(&s)
            .map_err(|e| D::Error::custom(format!("invalid duration {s:?}: {e}"))),
        DurIn::Secs(n) => Ok(Duration::from_secs(n)),
        DurIn::FloatSecs(f) => {
            if f < 0.0 {
                return Err(D::Error::custom("negative duration"));
            }
            Ok(Duration::from_secs_f64(f))
        }
    }
}

/// Reject negative `i64` values at deserialization time. The lag-cap
/// fields on `HeartbeatAdvancedConf` are typed as `i64` to match the
/// proposer's comparison sites, but a negative value silently disables
/// the corresponding code path — fail fast instead.
fn de_non_negative_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where D: serde::Deserializer<'de> {
    use serde::de::Error as _;
    use serde::Deserialize;
    let v = i64::deserialize(deserializer)?;
    if v < 0 {
        return Err(D::Error::custom(format!("value must be >= 0, got {}", v)));
    }
    Ok(v)
}

#[cfg(test)]
mod native_token_validation_tests {
    use super::*;

    fn valid_genesis() -> GenesisBlockData {
        GenesisBlockData {
            genesis_data_dir: String::new(),
            bonds_file: String::new(),
            wallets_file: String::new(),
            bond_minimum: 0,
            bond_maximum: 0,
            epoch_length: 1,
            quarantine_length: 0,
            number_of_active_validators: 0,
            deploy_timestamp: None,
            genesis_block_number: 0,
            pos_multi_sig_public_keys: Vec::new(),
            pos_multi_sig_quorum: 0,
            max_cosigners_per_deploy: DEFAULT_MAX_COSIGNERS_PER_DEPLOY,
            initial_phlogiston: DEFAULT_INITIAL_PHLOGISTON,
            epoch_phlogiston: DEFAULT_EPOCH_PHLOGISTON,
            client_fuel_allocations: Vec::new(),
            native_token_name: "F1R3FLY".into(),
            native_token_symbol: "F1R3".into(),
            native_token_decimals: 8,
        }
    }

    #[test]
    fn accepts_valid_baseline() { valid_genesis().validate_native_token().unwrap(); }

    #[test]
    fn rejects_empty_name() {
        let mut g = valid_genesis();
        g.native_token_name = String::new();
        let err = g.validate_native_token().unwrap_err();
        assert!(
            err.contains("native-token-name must be non-empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_whitespace_only_name() {
        let mut g = valid_genesis();
        g.native_token_name = "   ".into();
        let err = g.validate_native_token().unwrap_err();
        assert!(
            err.contains("native-token-name must be non-empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_empty_symbol() {
        let mut g = valid_genesis();
        g.native_token_symbol = String::new();
        let err = g.validate_native_token().unwrap_err();
        assert!(
            err.contains("native-token-symbol must be non-empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_whitespace_only_symbol() {
        let mut g = valid_genesis();
        g.native_token_symbol = "   ".into();
        let err = g.validate_native_token().unwrap_err();
        assert!(
            err.contains("native-token-symbol must be non-empty"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn rejects_decimals_above_max() {
        let mut g = valid_genesis();
        g.native_token_decimals = MAX_NATIVE_TOKEN_DECIMALS + 1;
        let err = g.validate_native_token().unwrap_err();
        assert!(
            err.contains("native-token-decimals=19"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn accepts_decimals_at_max() {
        let mut g = valid_genesis();
        g.native_token_decimals = MAX_NATIVE_TOKEN_DECIMALS;
        g.validate_native_token().unwrap();
    }

    #[test]
    fn accepts_valid_cost_accounting_parameters() {
        valid_genesis()
            .validate_cost_accounting_parameters()
            .unwrap();
    }

    #[test]
    fn rejects_invalid_cost_accounting_parameters() {
        let mut invalid_epoch_length = valid_genesis();
        invalid_epoch_length.epoch_length = 0;
        assert!(invalid_epoch_length
            .validate_cost_accounting_parameters()
            .is_err());

        let mut invalid_cosigner_limit = valid_genesis();
        invalid_cosigner_limit.max_cosigners_per_deploy = 0;
        assert!(invalid_cosigner_limit
            .validate_cost_accounting_parameters()
            .is_err());

        let mut invalid_initial_phlogiston = valid_genesis();
        invalid_initial_phlogiston.initial_phlogiston = -1;
        assert!(invalid_initial_phlogiston
            .validate_cost_accounting_parameters()
            .is_err());

        let mut invalid_epoch_phlogiston = valid_genesis();
        invalid_epoch_phlogiston.epoch_phlogiston = -1;
        assert!(invalid_epoch_phlogiston
            .validate_cost_accounting_parameters()
            .is_err());

        let mut empty_client_key = valid_genesis();
        empty_client_key
            .client_fuel_allocations
            .push(ClientFuelAllocation {
                public_key: String::new(),
                amount: 1,
            });
        assert!(empty_client_key
            .validate_cost_accounting_parameters()
            .is_err());

        let mut overflowing_clients = valid_genesis();
        overflowing_clients.client_fuel_allocations = vec![
            ClientFuelAllocation {
                public_key: "01".to_string(),
                amount: i64::MAX,
            },
            ClientFuelAllocation {
                public_key: "01".to_string(),
                amount: 1,
            },
        ];
        assert!(overflowing_clients
            .validate_cost_accounting_parameters()
            .is_err());
    }
}
