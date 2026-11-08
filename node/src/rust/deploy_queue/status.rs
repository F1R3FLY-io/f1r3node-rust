use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use lapin::options::{BasicPublishOptions, ConfirmSelectOptions, ExchangeDeclareOptions};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, Connection, ExchangeKind};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use super::consumer::ConsumerContext;

/// Schema version of the status event JSON.
pub const STATUS_SCHEMA: u32 = 1;
/// Routing key of block-height heartbeats on the status exchange.
pub const TIP_ROUTING_KEY: &str = "shard.tip";
/// Hex characters of the signature carried in a per-deploy routing key.
pub const SIG_PREFIX_HEX_CHARS: usize = 64;

pub fn sig_prefix(sig_hex: &str) -> &str {
    let end = sig_hex
        .char_indices()
        .nth(SIG_PREFIX_HEX_CHARS)
        .map_or(sig_hex.len(), |(i, _)| i);
    &sig_hex[..end]
}

/// One event on the status exchange. Serialized as JSON; field names are
/// the contract with `f1r3node-async-rust`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusEvent {
    pub schema: u32,
    pub event: String,
    pub shard: String,
    pub node: String,
    pub ts_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deployer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub errored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_number: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deploy_lifespan: Option<i64>,
    /// Decimal string: the shard floor is a `TokenAmount` (up to u256).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_phlo_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_user_deploys_per_block: Option<i32>,
}

impl StatusEvent {
    pub fn new(event: &str, shard: &str, node: &str) -> Self {
        Self {
            schema: STATUS_SCHEMA,
            event: event.to_string(),
            shard: shard.to_string(),
            node: node.to_string(),
            ts_ms: now_ms(),
            sig: None,
            deployer: None,
            reason: None,
            terminal: None,
            duplicate: None,
            errored: None,
            cost: None,
            block_number: None,
            block_hash: None,
            deploy_lifespan: None,
            min_phlo_price: None,
            max_user_deploys_per_block: None,
        }
    }

    /// `<deployer-pk-hex>.<sig-prefix>` for per-deploy events, `shard.tip`
    /// for tips. AMQP routing keys are short strings (255 bytes at most) and
    /// an uncompressed secp256k1 key (130 hex) plus a DER signature (~142
    /// hex) would exceed that, so the key carries the first
    /// `SIG_PREFIX_HEX_CHARS` of the signature; the body carries it whole.
    /// Clients bind `<deployer-pk-hex>.#`.
    pub fn routing_key(&self) -> String {
        match (&self.deployer, &self.sig) {
            (Some(d), Some(s)) => format!("{d}.{}", sig_prefix(s)),
            _ => TIP_ROUTING_KEY.to_string(),
        }
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Process-wide view of the running consumer for `GET /api/deploy-queue`.
/// Set once by `deploy_queue::spawn`; `None` until the queue is enabled.
pub static REGISTRY: std::sync::OnceLock<QueueView> = std::sync::OnceLock::new();

/// What the HTTP route reports: static configuration plus live counters.
#[derive(Debug)]
pub struct QueueView {
    pub queue_name: String,
    pub deploy_exchange: String,
    pub status_exchange: String,
    pub prefetch: u16,
    pub max_length: u32,
    pub message_ttl_ms: Option<u64>,
    pub stats: Arc<QueueStats>,
}

impl QueueView {
    pub fn snapshot(&self) -> serde_json::Value {
        let mut v = self.stats.snapshot();
        if let serde_json::Value::Object(ref mut m) = v {
            m.insert("enabled".into(), serde_json::Value::Bool(true));
            m.insert("queue".into(), self.queue_name.clone().into());
            m.insert(
                "deploy_exchange".into(),
                self.deploy_exchange.clone().into(),
            );
            m.insert(
                "status_exchange".into(),
                self.status_exchange.clone().into(),
            );
            m.insert("prefetch".into(), self.prefetch.into());
            m.insert("max_length".into(), self.max_length.into());
            m.insert(
                "ttl_ms".into(),
                self.message_ttl_ms
                    .map_or(serde_json::Value::Null, Into::into),
            );
        }
        v
    }
}

/// JSON for the route whether or not the queue is enabled.
pub fn http_snapshot() -> serde_json::Value {
    match REGISTRY.get() {
        Some(view) => view.snapshot(),
        None => serde_json::json!({ "enabled": false }),
    }
}

/// Counters behind `GET /api/deploy-queue` and the Prometheus gauges.
#[derive(Debug, Default)]
pub struct QueueStats {
    pub connected: AtomicBool,
    pub stalled: AtomicBool,
    pub consumed_accepted: AtomicU64,
    pub consumed_duplicate: AtomicU64,
    pub consumed_rejected: AtomicU64,
    pub deferred: AtomicU64,
    pub status_published: AtomicU64,
    pub status_publish_failed: AtomicU64,
    pub last_depth: AtomicU64,
}

impl QueueStats {
    pub fn snapshot(&self) -> serde_json::Value {
        serde_json::json!({
            "connected": self.connected.load(Ordering::Relaxed),
            "stalled": self.stalled.load(Ordering::Relaxed),
            "depth": self.last_depth.load(Ordering::Relaxed),
            "consumed": {
                "accepted": self.consumed_accepted.load(Ordering::Relaxed),
                "duplicate": self.consumed_duplicate.load(Ordering::Relaxed),
                "rejected": self.consumed_rejected.load(Ordering::Relaxed),
            },
            "deferred": self.deferred.load(Ordering::Relaxed),
            "status_published": self.status_published.load(Ordering::Relaxed),
            "status_publish_failed": self.status_publish_failed.load(Ordering::Relaxed),
        })
    }
}

/// Publishes status events on a dedicated confirm-mode channel. The channel
/// is (re)bound by the consumer whenever a connection is (re)established;
/// until then events are dropped and counted, never buffered: a status event
/// is a convenience for clients, the pool is the source of truth.
#[derive(Clone)]
pub struct StatusPublisher {
    ctx: Arc<ConsumerContext>,
    channel: Arc<Mutex<Option<Channel>>>,
}

impl StatusPublisher {
    pub fn new(ctx: Arc<ConsumerContext>) -> Self {
        Self {
            ctx,
            channel: Arc::new(Mutex::new(None)),
        }
    }

    /// Open a fresh channel on `conn`, declare the status exchange and switch
    /// the publisher to it.
    pub async fn bind(&self, conn: &Connection) -> lapin::Result<()> {
        let ch = conn.create_channel().await?;
        ch.exchange_declare(
            self.ctx.conf.status_exchange.as_str().into(),
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
        ch.confirm_select(ConfirmSelectOptions::default()).await?;
        *self.channel.lock().await = Some(ch);
        Ok(())
    }

    pub async fn unbind(&self) { *self.channel.lock().await = None; }

    pub fn event(&self, name: &str) -> StatusEvent {
        StatusEvent::new(name, &self.ctx.shard_id, &self.ctx.validator_pk_hex)
    }

    pub async fn publish(&self, ev: StatusEvent) {
        let payload = match serde_json::to_vec(&ev) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(error = %e, "deploy queue: could not encode status event");
                return;
            }
        };
        let guard = self.channel.lock().await;
        let Some(ch) = guard.as_ref() else {
            self.ctx
                .stats
                .status_publish_failed
                .fetch_add(1, Ordering::Relaxed);
            return;
        };
        let props = BasicProperties::default()
            .with_content_type("application/json".into())
            .with_app_id("f1r3node".into())
            .with_timestamp((ev.ts_ms / 1000) as u64)
            .with_delivery_mode(1);
        let result = ch
            .basic_publish(
                self.ctx.conf.status_exchange.as_str().into(),
                ev.routing_key().as_str().into(),
                BasicPublishOptions::default(),
                &payload,
                props,
            )
            .await;
        match result {
            Ok(confirm) => match confirm.await {
                Ok(_) => {
                    self.ctx
                        .stats
                        .status_published
                        .fetch_add(1, Ordering::Relaxed);
                }
                Err(e) => {
                    self.ctx
                        .stats
                        .status_publish_failed
                        .fetch_add(1, Ordering::Relaxed);
                    tracing::debug!(error = %e, "deploy queue: status publish not confirmed");
                }
            },
            Err(e) => {
                self.ctx
                    .stats
                    .status_publish_failed
                    .fetch_add(1, Ordering::Relaxed);
                tracing::debug!(error = %e, "deploy queue: status publish failed");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_key_is_deployer_dot_sig_or_tip() {
        let mut ev = StatusEvent::new("accepted", "s", "n");
        assert_eq!(ev.routing_key(), "shard.tip");
        ev.deployer = Some("ab".into());
        ev.sig = Some("cd".into());
        assert_eq!(ev.routing_key(), "ab.cd");
    }

    #[test]
    fn routing_key_fits_an_amqp_short_string() {
        let mut ev = StatusEvent::new("accepted", "s", "n");
        ev.deployer = Some("0".repeat(130));
        ev.sig = Some("f".repeat(144));
        let key = ev.routing_key();
        assert!(key.len() <= 255, "{}", key.len());
        assert_eq!(key, format!("{}.{}", "0".repeat(130), "f".repeat(64)));
    }

    #[test]
    fn json_omits_absent_fields() {
        let mut ev = StatusEvent::new("tip", "s", "n");
        ev.block_number = Some(7);
        let j = serde_json::to_value(&ev).unwrap();
        assert_eq!(j["event"], "tip");
        assert_eq!(j["block_number"], 7);
        assert!(j.get("sig").is_none());
        assert_eq!(j["schema"], STATUS_SCHEMA);
    }
}
