use std::sync::Arc;

use futures::StreamExt;
use shared::rust::shared::f1r3fly_event::{DeployEvent, F1r3flyEvent};
use shared::rust::shared::f1r3fly_events::F1r3flyEvents;

use super::consumer::ConsumerContext;
use super::status::StatusPublisher;

/// Turns the node's own block events into status events, so a client that
/// published to this node can follow its deploy past admission without
/// polling:
///
/// * `BlockAdded` -> `tip` (block height and the shard parameters a client
///   needs) and one `included` per deploy
/// * `BlockFinalised` -> one `finalized` per deploy
///
/// Only deploys with a deployer are routed (the key is `<deployer>.<sig>`);
/// the `tip` goes out on `shard.tip`.
pub async fn run(ctx: Arc<ConsumerContext>, publisher: StatusPublisher, event_bus: F1r3flyEvents) {
    let mut events = event_bus.consume();
    let mut last_tip: i64 = -1;
    while let Some(event) = events.next().await {
        match event {
            F1r3flyEvent::BlockAdded(b) => {
                if b.block_number > last_tip {
                    last_tip = b.block_number;
                    publish_tip(&ctx, &publisher, b.block_number, &b.block_hash).await;
                }
                for d in &b.deploys {
                    publish_deploy_event(&publisher, "included", d, b.block_number, &b.block_hash)
                        .await;
                }
            }
            F1r3flyEvent::BlockCreated(b) => {
                if b.block_number > last_tip {
                    last_tip = b.block_number;
                    publish_tip(&ctx, &publisher, b.block_number, &b.block_hash).await;
                }
            }
            F1r3flyEvent::BlockFinalised(b) => {
                for d in &b.deploys {
                    publish_deploy_event(&publisher, "finalized", d, b.block_number, &b.block_hash)
                        .await;
                }
            }
            _ => {}
        }
    }
    tracing::warn!("deploy queue: event bus closed; lifecycle publisher stopping");
}

async fn publish_tip(
    ctx: &ConsumerContext,
    publisher: &StatusPublisher,
    block_number: i64,
    block_hash: &str,
) {
    let mut ev = publisher.event("tip");
    ev.block_number = Some(block_number);
    ev.block_hash = Some(block_hash.to_string());
    let eng = ctx.engine_cell.get().await;
    if let Some(casper) = eng.with_casper() {
        let conf = casper.casper_shard_conf();
        ev.deploy_lifespan = Some(conf.deploy_lifespan);
        ev.min_phlo_price = Some(conf.min_phlo_price.to_string());
        ev.max_user_deploys_per_block = Some(conf.max_user_deploys_per_block as i32);
    }
    publisher.publish(ev).await;
}

async fn publish_deploy_event(
    publisher: &StatusPublisher,
    name: &str,
    d: &DeployEvent,
    block_number: i64,
    block_hash: &str,
) {
    if d.deployer.is_empty() || d.id.is_empty() {
        return;
    }
    let mut ev = publisher.event(name);
    ev.sig = Some(d.id.clone());
    ev.deployer = Some(d.deployer.clone());
    ev.block_number = Some(block_number);
    ev.block_hash = Some(block_hash.to_string());
    ev.cost = Some(d.cost);
    ev.errored = Some(d.errored);
    publisher.publish(ev).await;
}
