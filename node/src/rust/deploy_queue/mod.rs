//! Node-local deploy queue: a RabbitMQ consumer in front of `BlockAPI::deploy`.
//!
//! Each node owns one broker and one queue. Deliveries are decoded as raw
//! `DeployDataProto` bytes, verified, and admitted through the same
//! `BlockAPI::deploy` call the gRPC and HTTP handlers make. The outcome of
//! that call alone decides the acknowledgement (`classify`). Verdicts and
//! block lifecycle events are published to a topic exchange so an async
//! client can follow a deploy without polling.
//!
//! See `docs/designs/deploy-queue/deploy-queue-design.pdf`.

pub mod classify;
pub mod config;
pub mod consumer;
pub mod lifecycle;
pub mod naming;
pub mod status;

use std::sync::Arc;

use casper::rust::engine::engine_cell::EngineCell;
use casper::rust::ProposeFunction;
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use crate::rust::configuration::model::NodeConf;

/// Starts the deploy-queue consumer and lifecycle publisher as supportive
/// tasks. Returns `false` when the queue is disabled or cannot be enabled on
/// this node. Never fails the node: a broker that is unreachable is retried
/// with backoff for as long as the node runs.
pub fn spawn(
    conf: &NodeConf,
    validator_pk_hex: Option<String>,
    engine_cell: Arc<EngineCell>,
    trigger_propose: Option<Arc<ProposeFunction>>,
    event_bus: F1r3flyEvents,
) -> bool {
    let dq = &conf.deploy_queue;
    if !dq.enabled {
        tracing::info!("Deploy queue disabled (deploy-queue.enabled = false)");
        return false;
    }
    let is_node_read_only = conf.casper.validator_private_key.is_none();
    let Some(validator_pk) = validator_pk_hex else {
        tracing::warn!(
            "Deploy queue enabled but this node has no validator identity; \
             a read-only node cannot admit deploys, so the consumer will not start"
        );
        return false;
    };
    let queue_name = naming::queue_name(dq, &conf.casper.shard_name, &Some(validator_pk.clone()));
    let ctx = Arc::new(consumer::ConsumerContext {
        conf: dq.clone(),
        shard_id: conf.casper.shard_name.clone(),
        validator_pk_hex: validator_pk,
        queue_name,
        message_ttl_ms: naming::message_ttl_ms(
            dq.ttl_factor,
            conf.casper.deploy_lifespan,
            conf.casper.heartbeat_conf.check_interval,
        ),
        engine_cell,
        trigger_propose,
        is_node_read_only,
        stats: Arc::new(status::QueueStats::default()),
    });
    tracing::info!(
        queue = %ctx.queue_name,
        amqp_url = %naming::redact_url(&dq.amqp_url),
        prefetch = dq.prefetch,
        max_length = dq.max_length,
        message_ttl_ms = ?ctx.message_ttl_ms,
        "Deploy queue enabled; starting consumer"
    );
    let _ = status::REGISTRY.set(status::QueueView {
        queue_name: ctx.queue_name.clone(),
        deploy_exchange: dq.deploy_exchange.clone(),
        status_exchange: dq.status_exchange.clone(),
        prefetch: dq.prefetch,
        max_length: dq.max_length,
        message_ttl_ms: ctx.message_ttl_ms,
        stats: ctx.stats.clone(),
    });
    let publisher = status::StatusPublisher::new(ctx.clone());
    tokio::spawn(consumer::run_forever(ctx.clone(), publisher.clone()));
    tokio::spawn(lifecycle::run(ctx, publisher, event_bus));
    true
}
