use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use casper::rust::api::block_api::BlockAPI;
use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::ProposeFunction;
use futures::StreamExt;
use lapin::message::Delivery;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicQosOptions, ExchangeDeclareOptions,
    QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::{AMQPValue, FieldTable};
use lapin::{Connection, ConnectionProperties, ExchangeKind};
use models::casper::DeployDataProto;
use models::rust::casper::protocol::casper_message::DeployData;
use prost::Message;
use tokio::sync::Mutex;

use super::classify::{classify, classify_decode_failure, Verdict};
use super::config::DeployQueueConf;
use super::status::{QueueStats, StatusPublisher};

pub const METRIC_CONNECTED: &str = "deploy_queue_connected";
pub const METRIC_STALLED: &str = "deploy_queue_stalled";
pub const METRIC_DEPTH: &str = "deploy_queue_depth";
pub const METRIC_CONSUMED_TOTAL: &str = "deploy_queue_consumed_total";
pub const METRIC_REJECTED_TOTAL: &str = "deploy_queue_rejected_total";
pub const METRIC_DEFERRED_SECONDS: &str = "deploy_queue_deferred_seconds";
pub const METRIC_ADMISSION_SECONDS: &str = "deploy_queue_admission_seconds";

const INITIAL_BACKOFF: Duration = Duration::from_millis(250);
const RECONNECT_INITIAL: Duration = Duration::from_secs(1);
const DEPTH_SAMPLE_EVERY: Duration = Duration::from_secs(5);
const DEFERRED_EVENT_EVERY: Duration = Duration::from_secs(60);
const POOL_COUNT_EVERY: Duration = Duration::from_secs(1);

/// Everything a delivery handler needs; shared by all handler tasks.
pub struct ConsumerContext {
    pub conf: DeployQueueConf,
    pub shard_id: String,
    pub validator_pk_hex: String,
    pub queue_name: String,
    pub message_ttl_ms: Option<u64>,
    pub engine_cell: Arc<EngineCell>,
    pub trigger_propose: Option<Arc<ProposeFunction>>,
    pub is_node_read_only: bool,
    pub stats: Arc<QueueStats>,
}

/// Connect, declare, consume; on any loss of the connection or consumer,
/// back off and start again. Runs for the life of the node.
pub async fn run_forever(ctx: Arc<ConsumerContext>, publisher: StatusPublisher) {
    let pool_gauge = Arc::new(Mutex::new(PoolGauge::default()));
    let mut backoff = RECONNECT_INITIAL;
    loop {
        match session(ctx.clone(), publisher.clone(), pool_gauge.clone()).await {
            Ok(()) => {
                tracing::warn!(queue = %ctx.queue_name, "deploy queue: consumer stream ended; reconnecting");
                backoff = RECONNECT_INITIAL;
            }
            Err(e) => {
                tracing::warn!(
                    queue = %ctx.queue_name,
                    error = %e,
                    retry_in = ?backoff,
                    "deploy queue: broker session failed"
                );
            }
        }
        publisher.unbind().await;
        set_connected(&ctx, false);
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(ctx.conf.max_backoff.max(RECONNECT_INITIAL));
    }
}

async fn connect(conf: &DeployQueueConf) -> lapin::Result<Connection> {
    let props =
        ConnectionProperties::default().with_connection_name("f1r3node-deploy-queue".into());
    if conf.tls.enabled && !conf.tls.ca_file.is_empty() {
        let pem = std::fs::read_to_string(&conf.tls.ca_file).map_err(|e| {
            lapin::Error::from(std::io::Error::new(
                e.kind(),
                format!("deploy-queue.tls.ca-file {}: {e}", conf.tls.ca_file),
            ))
        })?;
        if !conf.tls.cert_file.is_empty() {
            tracing::warn!("deploy-queue.tls.cert-file is set but client identities are not supported yet; ignoring");
        }
        let tls = lapin::tcp::OwnedTLSConfig {
            identity: None,
            cert_chain: Some(pem),
        };
        let runtime = lapin::runtime::default_runtime()?;
        Connection::connect_with_config(&conf.amqp_url, props, tls, runtime).await
    } else {
        Connection::connect(&conf.amqp_url, props).await
    }
}

/// One broker session: from connect to the end of the consumer stream.
async fn session(
    ctx: Arc<ConsumerContext>,
    publisher: StatusPublisher,
    pool_gauge: Arc<Mutex<PoolGauge>>,
) -> lapin::Result<()> {
    let conn = connect(&ctx.conf).await?;
    let channel = conn.create_channel().await?;
    declare_topology(&ctx, &channel).await?;
    publisher.bind(&conn).await?;
    channel
        .basic_qos(ctx.conf.prefetch, BasicQosOptions::default())
        .await?;
    let mut consumer = channel
        .basic_consume(
            ctx.queue_name.as_str().into(),
            format!(
                "f1r3node-{}",
                &ctx.validator_pk_hex[..ctx.validator_pk_hex.len().min(16)]
            )
            .into(),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    set_connected(&ctx, true);
    tracing::info!(queue = %ctx.queue_name, "deploy queue: consuming");

    let depth_ctx = ctx.clone();
    let depth_channel = channel.clone();
    let depth_task = tokio::spawn(async move {
        loop {
            tokio::time::sleep(DEPTH_SAMPLE_EVERY).await;
            let declared = depth_channel
                .queue_declare(
                    depth_ctx.queue_name.as_str().into(),
                    QueueDeclareOptions {
                        passive: true,
                        ..Default::default()
                    },
                    FieldTable::default(),
                )
                .await;
            match declared {
                Ok(q) => {
                    let depth = q.message_count() as u64;
                    depth_ctx.stats.last_depth.store(depth, Ordering::Relaxed);
                    metrics::gauge!(METRIC_DEPTH).set(depth as f64);
                }
                Err(_) => break,
            }
        }
    });

    while let Some(next) = consumer.next().await {
        let delivery = match next {
            Ok(d) => d,
            Err(e) => {
                depth_task.abort();
                return Err(e);
            }
        };
        let ctx = ctx.clone();
        let publisher = publisher.clone();
        let pool_gauge = pool_gauge.clone();
        tokio::spawn(async move {
            handle_delivery(ctx, publisher, pool_gauge, delivery).await;
        });
    }
    depth_task.abort();
    Ok(())
}

async fn declare_topology(ctx: &ConsumerContext, ch: &lapin::Channel) -> lapin::Result<()> {
    let conf = &ctx.conf;
    let durable = ExchangeDeclareOptions {
        durable: true,
        ..Default::default()
    };
    ch.exchange_declare(
        conf.deploy_exchange.as_str().into(),
        ExchangeKind::Direct,
        durable.clone(),
        FieldTable::default(),
    )
    .await?;
    ch.exchange_declare(
        conf.dead_letter_exchange.as_str().into(),
        ExchangeKind::Direct,
        durable,
        FieldTable::default(),
    )
    .await?;

    let expired_queue = format!("{}.expired", conf.deploy_exchange);
    let mut expired_args = FieldTable::default();
    expired_args.insert("x-max-length".into(), AMQPValue::LongUInt(conf.max_length));
    expired_args.insert(
        "x-overflow".into(),
        AMQPValue::LongString("drop-head".into()),
    );
    ch.queue_declare(
        expired_queue.as_str().into(),
        QueueDeclareOptions {
            durable: true,
            ..Default::default()
        },
        expired_args,
    )
    .await?;
    ch.queue_bind(
        expired_queue.as_str().into(),
        conf.dead_letter_exchange.as_str().into(),
        ctx.queue_name.as_str().into(),
        QueueBindOptions::default(),
        FieldTable::default(),
    )
    .await?;

    let mut args = FieldTable::default();
    args.insert("x-max-length".into(), AMQPValue::LongUInt(conf.max_length));
    args.insert(
        "x-overflow".into(),
        AMQPValue::LongString("reject-publish".into()),
    );
    args.insert(
        "x-dead-letter-exchange".into(),
        AMQPValue::LongString(conf.dead_letter_exchange.as_str().into()),
    );
    if let Some(ttl) = ctx.message_ttl_ms {
        args.insert(
            "x-message-ttl".into(),
            AMQPValue::LongUInt(ttl.min(u32::MAX as u64) as u32),
        );
    }
    ch.queue_declare(
        ctx.queue_name.as_str().into(),
        QueueDeclareOptions {
            durable: true,
            ..Default::default()
        },
        args,
    )
    .await?;
    ch.queue_bind(
        ctx.queue_name.as_str().into(),
        conf.deploy_exchange.as_str().into(),
        ctx.queue_name.as_str().into(),
        QueueBindOptions::default(),
        FieldTable::default(),
    )
    .await?;
    Ok(())
}

/// Decode, verify, admit, classify, act. A delivery is acknowledged only
/// after `BlockAPI::deploy` has returned a verdict for it; transient
/// outcomes keep it unacknowledged and retry with backoff, so the prefetch
/// window holds the broker back while the node catches up.
async fn handle_delivery(
    ctx: Arc<ConsumerContext>,
    publisher: StatusPublisher,
    pool_gauge: Arc<Mutex<PoolGauge>>,
    delivery: Delivery,
) {
    let received = Instant::now();
    let proto = match DeployDataProto::decode(&delivery.data[..]) {
        Ok(p) => p,
        Err(e) => {
            let verdict = classify_decode_failure(format!("undecodable DeployDataProto: {e}"));
            finish(&ctx, &publisher, &delivery, None, None, &verdict, received).await;
            return;
        }
    };
    let (sig_hex, deployer_hex) = (hex::encode(&proto.sig), hex::encode(&proto.deployer));
    let signed = match DeployData::from_proto(proto) {
        Ok(s) => s,
        Err(e) => {
            let verdict = classify_decode_failure(format!("signature check failed: {e}"));
            finish(
                &ctx,
                &publisher,
                &delivery,
                Some(&sig_hex),
                Some(&deployer_hex),
                &verdict,
                received,
            )
            .await;
            return;
        }
    };

    let mut backoff = INITIAL_BACKOFF;
    let mut last_deferred_event: Option<Instant> = None;
    let mut alarmed = false;
    loop {
        if let Some(reason) = pool_backlog_exceeded(&ctx, &pool_gauge).await {
            let verdict = Verdict::Transient(reason);
            defer(
                &ctx,
                &publisher,
                &sig_hex,
                &deployer_hex,
                &verdict,
                &mut last_deferred_event,
            )
            .await;
            tokio::time::sleep(backoff).await;
            backoff = (backoff * 2).min(ctx.conf.max_backoff);
            continue;
        }

        let started = Instant::now();
        let result = BlockAPI::deploy(
            &ctx.engine_cell,
            signed.clone(),
            &ctx.trigger_propose,
            ctx.is_node_read_only,
            &ctx.shard_id,
        )
        .await;
        metrics::histogram!(METRIC_ADMISSION_SECONDS).record(started.elapsed().as_secs_f64());
        let verdict = classify(&result);

        match &verdict {
            Verdict::Transient(_) | Verdict::TransientAlarm(_) => {
                if matches!(verdict, Verdict::TransientAlarm(_)) && !alarmed {
                    alarmed = true;
                    ctx.stats.stalled.store(true, Ordering::Relaxed);
                    metrics::gauge!(METRIC_STALLED).set(1.0);
                    tracing::warn!(
                        sig = %sig_hex,
                        reason = verdict.reason().unwrap_or(""),
                        "deploy queue: admission stalled on an operator condition; holding deliveries"
                    );
                }
                defer(
                    &ctx,
                    &publisher,
                    &sig_hex,
                    &deployer_hex,
                    &verdict,
                    &mut last_deferred_event,
                )
                .await;
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(ctx.conf.max_backoff);
            }
            _ => {
                if alarmed {
                    ctx.stats.stalled.store(false, Ordering::Relaxed);
                    metrics::gauge!(METRIC_STALLED).set(0.0);
                }
                finish(
                    &ctx,
                    &publisher,
                    &delivery,
                    Some(&sig_hex),
                    Some(&deployer_hex),
                    &verdict,
                    received,
                )
                .await;
                return;
            }
        }
    }
}

async fn defer(
    ctx: &ConsumerContext,
    publisher: &StatusPublisher,
    sig_hex: &str,
    deployer_hex: &str,
    verdict: &Verdict,
    last_event: &mut Option<Instant>,
) {
    ctx.stats.deferred.fetch_add(1, Ordering::Relaxed);
    let due = last_event.is_none_or(|t| t.elapsed() >= DEFERRED_EVENT_EVERY);
    if due {
        *last_event = Some(Instant::now());
        let mut ev = publisher.event("deferred");
        ev.sig = Some(sig_hex.to_string());
        ev.deployer = Some(deployer_hex.to_string());
        ev.reason = verdict.reason().map(str::to_string);
        publisher.publish(ev).await;
    }
}

/// Ack and report a final verdict (accepted, duplicate, or terminal).
async fn finish(
    ctx: &ConsumerContext,
    publisher: &StatusPublisher,
    delivery: &Delivery,
    sig_hex: Option<&str>,
    deployer_hex: Option<&str>,
    verdict: &Verdict,
    received: Instant,
) {
    let waited = received.elapsed().as_secs_f64();
    metrics::histogram!(METRIC_DEFERRED_SECONDS).record(waited);
    metrics::counter!(METRIC_CONSUMED_TOTAL, "outcome" => verdict.label()).increment(1);
    match verdict {
        Verdict::Accepted => {
            ctx.stats.consumed_accepted.fetch_add(1, Ordering::Relaxed);
        }
        Verdict::Duplicate => {
            ctx.stats.consumed_duplicate.fetch_add(1, Ordering::Relaxed);
        }
        Verdict::Terminal(reason) => {
            ctx.stats.consumed_rejected.fetch_add(1, Ordering::Relaxed);
            metrics::counter!(METRIC_REJECTED_TOTAL, "reason" => reason_class(reason)).increment(1);
            tracing::info!(sig = sig_hex.unwrap_or("?"), reason = %reason, "deploy queue: rejected");
        }
        Verdict::Transient(_) | Verdict::TransientAlarm(_) => {
            unreachable!("transient verdicts are not finished")
        }
    }

    if let Err(e) = delivery.ack(BasicAckOptions::default()).await {
        tracing::warn!(error = %e, sig = sig_hex.unwrap_or("?"), "deploy queue: ack failed; the broker will redeliver");
    }

    let name = match verdict {
        Verdict::Accepted | Verdict::Duplicate => "accepted",
        _ => "rejected",
    };
    let mut ev = publisher.event(name);
    ev.sig = sig_hex.map(str::to_string);
    ev.deployer = deployer_hex.map(str::to_string);
    match verdict {
        Verdict::Duplicate => ev.duplicate = Some(true),
        Verdict::Terminal(reason) => {
            ev.reason = Some(reason.clone());
            ev.terminal = Some(true);
        }
        _ => {}
    }
    if ev.sig.is_some() && ev.deployer.is_some() {
        publisher.publish(ev).await;
    }
}

/// Coarse reason label for the rejected counter, so the metric's
/// cardinality stays bounded while the status event carries the full text.
fn reason_class(reason: &str) -> &'static str {
    let r = reason.to_ascii_lowercase();
    if r.contains("undecodable") {
        "decode"
    } else if r.contains("signature") {
        "signature"
    } else if r.contains("shardid") {
        "shard"
    } else if r.contains("expired") {
        "expired"
    } else if r.contains("phlo price") {
        "phlo_price"
    } else if r.contains("parse") {
        "parse"
    } else if r.contains("forbidden") {
        "system_key"
    } else {
        "other"
    }
}

fn set_connected(ctx: &ConsumerContext, up: bool) {
    ctx.stats.connected.store(up, Ordering::Relaxed);
    metrics::gauge!(METRIC_CONNECTED).set(if up { 1.0 } else { 0.0 });
}

/// Soft pool bound for branches without `pool_admission`: count the local
/// pool at most once a second and hold deliveries while it is over
/// `max-pool-backlog`.
#[derive(Default)]
pub struct PoolGauge {
    sampled_at: Option<Instant>,
    pending: usize,
}

async fn pool_backlog_exceeded(ctx: &ConsumerContext, gauge: &Mutex<PoolGauge>) -> Option<String> {
    let cap = ctx.conf.max_pool_backlog;
    if cap == 0 {
        return None;
    }
    let mut g = gauge.lock().await;
    let stale = g.sampled_at.is_none_or(|t| t.elapsed() >= POOL_COUNT_EVERY);
    if stale {
        let eng = ctx.engine_cell.get().await;
        if let Some(casper) = eng.with_casper() {
            if let Ok(list) = casper.list_pending_deploys().await {
                g.pending = list.len();
            }
        } else {
            g.pending = 0;
        }
        g.sampled_at = Some(Instant::now());
    }
    if g.pending >= cap {
        Some(format!(
            "local deploy pool at soft cap ({} >= {})",
            g.pending, cap
        ))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Against a live broker (`F1R3_AMQP_URL`), with no Casper engine: a
    /// signed deploy is decoded, verified and HELD (engine not available is
    /// transient, so the delivery stays unacked and a `deferred` event goes
    /// out), while undecodable bytes are acked and reported `rejected`.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn broker_session_holds_transient_and_rejects_garbage() {
        let Ok(url) = std::env::var("F1R3_AMQP_URL") else {
            eprintln!("F1R3_AMQP_URL not set; skipping");
            return;
        };
        use crypto::rust::private_key::PrivateKey;
        use crypto::rust::signatures::secp256k1::Secp256k1;
        use crypto::rust::signatures::signatures_alg::SignaturesAlg;
        use crypto::rust::signatures::signed::Signed;
        use lapin::options::BasicPublishOptions;
        use lapin::BasicProperties;

        let mut conf = DeployQueueConf::default();
        conf.enabled = true;
        conf.amqp_url = url.clone();
        conf.queue_name = format!("f1r3node.deploy.unit.{}", std::process::id());
        conf.prefetch = 4;
        conf.max_pool_backlog = 0;
        let sk = PrivateKey::new(
            hex::decode("5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657")
                .unwrap()
                .into(),
        );
        let validator_pk_hex = hex::encode(&Secp256k1.to_public(&sk).bytes);
        let ctx = Arc::new(ConsumerContext {
            conf: conf.clone(),
            shard_id: "unit".into(),
            validator_pk_hex: validator_pk_hex.clone(),
            queue_name: conf.queue_name.clone(),
            message_ttl_ms: Some(250_000),
            engine_cell: Arc::new(EngineCell::init()),
            trigger_propose: None,
            is_node_read_only: false,
            stats: Arc::new(QueueStats::default()),
        });
        let publisher = StatusPublisher::new(ctx.clone());
        let pool_gauge = Arc::new(Mutex::new(PoolGauge::default()));
        let sess = tokio::spawn(session(ctx.clone(), publisher, pool_gauge));
        tokio::time::sleep(Duration::from_millis(800)).await;
        assert!(
            ctx.stats.connected.load(Ordering::Relaxed),
            "consumer did not connect"
        );

        // Subscribe to this deployer's status events.
        let conn = Connection::connect(&url, ConnectionProperties::default())
            .await
            .unwrap();
        let ch = conn.create_channel().await.unwrap();
        let q = ch
            .queue_declare(
                "".into(),
                QueueDeclareOptions {
                    exclusive: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .unwrap();
        ch.queue_bind(
            q.name().as_str().into(),
            conf.status_exchange.as_str().into(),
            format!("{validator_pk_hex}.#").as_str().into(),
            QueueBindOptions::default(),
            FieldTable::default(),
        )
        .await
        .unwrap();
        let mut status = ch
            .basic_consume(
                q.name().as_str().into(),
                "t".into(),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .unwrap();

        // A well-formed, signed deploy.
        let data = DeployData {
            term: "Nil".into(),
            time_stamp: 1,
            phlo_price: shared::rust::token_amount::TokenAmount::parse("1").unwrap(),
            phlo_limit: 10,
            valid_after_block_number: 0,
            shard_id: "unit".into(),
            expiration_timestamp: None,
            parameters: Vec::new(),
        };
        let signed = Signed::create(data, Box::new(Secp256k1), sk).unwrap();
        let bytes = DeployData::to_proto(signed).encode_to_vec();
        ch.basic_publish(
            conf.deploy_exchange.as_str().into(),
            conf.queue_name.as_str().into(),
            BasicPublishOptions::default(),
            &bytes,
            BasicProperties::default(),
        )
        .await
        .unwrap();
        let ev = tokio::time::timeout(Duration::from_secs(5), status.next())
            .await
            .expect("status event")
            .unwrap()
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&ev.data).unwrap();
        assert_eq!(json["event"], "deferred", "{json}");
        assert!(
            json["reason"].as_str().unwrap().contains("engine"),
            "{json}"
        );
        ev.ack(BasicAckOptions::default()).await.unwrap();
        assert_eq!(ctx.stats.consumed_accepted.load(Ordering::Relaxed), 0);
        assert!(ctx.stats.deferred.load(Ordering::Relaxed) >= 1);

        // Garbage is terminal: acked and reported, but routed only when a
        // deployer is known, so check the counter instead of an event.
        ch.basic_publish(
            conf.deploy_exchange.as_str().into(),
            conf.queue_name.as_str().into(),
            BasicPublishOptions::default(),
            b"not a protobuf",
            BasicProperties::default(),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert_eq!(ctx.stats.consumed_rejected.load(Ordering::Relaxed), 1);

        // The held delivery is still unacked on the broker.
        let declared = ch
            .queue_declare(
                conf.queue_name.as_str().into(),
                QueueDeclareOptions {
                    passive: true,
                    ..Default::default()
                },
                FieldTable::default(),
            )
            .await
            .unwrap();
        assert_eq!(
            declared.message_count(),
            0,
            "held delivery must be in flight, not ready"
        );
        sess.abort();
        let _ = ch
            .queue_delete(
                conf.queue_name.as_str().into(),
                lapin::options::QueueDeleteOptions::default(),
            )
            .await;
    }

    #[test]
    fn reason_classes_are_bounded() {
        assert_eq!(reason_class("undecodable DeployDataProto: x"), "decode");
        assert_eq!(
            reason_class("Deploy shardId 'a' is not as expected"),
            "shard"
        );
        assert_eq!(
            reason_class("Deploy validAfterBlockNumber 1 has expired at block 9"),
            "expired"
        );
        assert_eq!(
            reason_class("Phlo price 1 is less than minimum price 2."),
            "phlo_price"
        );
        assert_eq!(reason_class("parse error: ..."), "parse");
        assert_eq!(reason_class("something else"), "other");
    }
}
