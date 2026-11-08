use std::time::Duration;

use casper::rust::casper_conf::de_duration;
use serde::{Deserialize, Serialize};

/// `deploy-queue { ... }` in the node configuration. Node-local; it is not a
/// consensus parameter and per-node values may differ.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployQueueConf {
    #[serde(default)]
    pub enabled: bool,

    #[serde(rename = "amqp-url", default = "default_amqp_url")]
    pub amqp_url: String,

    /// Empty means `f1r3node.deploy.<shard>.<validator-pk-prefix>`.
    #[serde(rename = "queue-name", default)]
    pub queue_name: String,

    #[serde(default = "default_prefetch")]
    pub prefetch: u16,

    #[serde(rename = "max-length", default = "default_max_length")]
    pub max_length: u32,

    /// `x-message-ttl = deploy-lifespan * heartbeat.check-interval * ttl-factor`;
    /// `0` disables the queue-side TTL.
    #[serde(rename = "ttl-factor", default = "default_ttl_factor")]
    pub ttl_factor: f64,

    /// Soft pool bound used when the node has no `pool_admission` cap.
    #[serde(rename = "max-pool-backlog", default = "default_max_pool_backlog")]
    pub max_pool_backlog: usize,

    #[serde(
        rename = "max-backoff",
        default = "default_max_backoff",
        deserialize_with = "de_duration"
    )]
    pub max_backoff: Duration,

    #[serde(rename = "deploy-exchange", default = "default_deploy_exchange")]
    pub deploy_exchange: String,

    #[serde(rename = "status-exchange", default = "default_status_exchange")]
    pub status_exchange: String,

    #[serde(rename = "dead-letter-exchange", default = "default_dlx")]
    pub dead_letter_exchange: String,

    #[serde(default)]
    pub tls: DeployQueueTlsConf,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployQueueTlsConf {
    #[serde(default)]
    pub enabled: bool,
    #[serde(rename = "ca-file", default)]
    pub ca_file: String,
    #[serde(rename = "cert-file", default)]
    pub cert_file: String,
    #[serde(rename = "key-file", default)]
    pub key_file: String,
}

impl Default for DeployQueueConf {
    fn default() -> Self {
        Self {
            enabled: false,
            amqp_url: default_amqp_url(),
            queue_name: String::new(),
            prefetch: default_prefetch(),
            max_length: default_max_length(),
            ttl_factor: default_ttl_factor(),
            max_pool_backlog: default_max_pool_backlog(),
            max_backoff: default_max_backoff(),
            deploy_exchange: default_deploy_exchange(),
            status_exchange: default_status_exchange(),
            dead_letter_exchange: default_dlx(),
            tls: DeployQueueTlsConf::default(),
        }
    }
}

fn default_amqp_url() -> String { "amqp://127.0.0.1:5672/%2f".to_string() }
fn default_prefetch() -> u16 { 64 }
fn default_max_length() -> u32 { 100_000 }
fn default_ttl_factor() -> f64 { 1.0 }
fn default_max_pool_backlog() -> usize { 8192 }
fn default_max_backoff() -> Duration { Duration::from_secs(30) }
fn default_deploy_exchange() -> String { "f1r3node.deploy".to_string() }
fn default_status_exchange() -> String { "f1r3node.deploy.status".to_string() }
fn default_dlx() -> String { "f1r3node.deploy.dlx".to_string() }

impl DeployQueueConf {
    /// Startup validation; mirrors the style of `validate_config`.
    pub fn validate(&self) -> Result<(), String> {
        if !self.enabled {
            return Ok(());
        }
        if self.prefetch == 0 {
            return Err("deploy-queue.prefetch must be positive".into());
        }
        if self.max_length == 0 {
            return Err("deploy-queue.max-length must be positive".into());
        }
        if self.ttl_factor.is_nan() || self.ttl_factor < 0.0 {
            return Err("deploy-queue.ttl-factor must be >= 0".into());
        }
        if !(self.amqp_url.starts_with("amqp://") || self.amqp_url.starts_with("amqps://")) {
            return Err(format!(
                "deploy-queue.amqp-url must start with amqp:// or amqps://, got {:?}",
                self.amqp_url
            ));
        }
        if self.tls.enabled && !self.amqp_url.starts_with("amqps://") {
            return Err("deploy-queue.tls.enabled requires an amqps:// amqp-url".into());
        }
        Ok(())
    }
}
