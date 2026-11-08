# Deploy Queue (RabbitMQ)

A node can take deploys from a RabbitMQ queue that it alone consumes, in front
of the normal gRPC/HTTP ingress. The queue buffers bursts from many clients so
the node drains them at its own pace; nothing downstream of
[deploy admission](../casper/GLOSSARY.md#deploy-admission) changes. The
design, with the reasoning behind each choice, is in
[`docs/designs/deploy-queue/`](../designs/deploy-queue/).

Key properties:

- **Node-local.** Every node runs beside its own broker. No queue service is
  shared between validators, so a broker outage affects one node's intake
  only, and the node keeps running on gRPC/HTTP ingress.
- **Same checks as any other deploy.** Each delivery goes through
  `BlockAPI::deploy`, exactly as a gRPC `doDeploy` does. The queue changes when
  and how fast deploys reach the node, not what the node does with them.
- **Push, not poll.** The broker delivers to the node's consumer; the node
  never polls the queue.
- **Bounded by the block window.** Deploys keep their
  [deploy lifespan](../casper/GLOSSARY.md#deploy-lifespan) validity, which is a
  shard-wide genesis value. A burst the shard cannot drain inside the window
  sheds its oldest deploys with a `rejected` status event, never silently.

## Enabling it

```hocon
deploy-queue {
  enabled = true
  amqp-url = "amqp://f1r3:f1r3@127.0.0.1:5672/%2f"
}
```

All keys and their defaults are documented in `defaults.conf` under
`deploy-queue`. The queue is disabled by default. A node without a validator
identity (read-only/observer) cannot admit deploys and does not start the
consumer even when enabled.

On start-up the node declares, idempotently:

| Object | Default name | Notes |
| --- | --- | --- |
| exchange | `f1r3node.deploy` | direct, durable; clients publish here |
| queue | `f1r3node.deploy.<shard-name>.<first 16 hex of validator pk>` | durable; `x-max-length` = `max-length`, `x-overflow = reject-publish`, `x-message-ttl` derived (below), dead-letters to the DLX |
| exchange | `f1r3node.deploy.status` | topic, durable; verdicts and lifecycle events |
| exchange + queue | `f1r3node.deploy.dlx`, `f1r3node.deploy.expired` | where TTL-expired messages land |

Set `queue-name` to override the queue name. The broker user the node
connects with needs `configure`, `write` and `read` on these names; a client
needs only `write` on `f1r3node.deploy` and `read` on `f1r3node.deploy.status`.

### Docker

`docker/deploy-queue.yml` overlays `docker/shard.yml` with one broker per
validator; `docker/deploy-queue-standalone.yml` does the same for
`docker/standalone.yml`:

```bash
docker compose -f docker/shard.yml -f docker/deploy-queue.yml up -d
```

## Publishing a deploy

Body: the raw `DeployDataProto` bytes (the same message `doDeploy` takes).
Routing key: the node's queue name, on exchange `f1r3node.deploy`. Recommended
properties: `content_type = application/x-protobuf`, `message_id = <sig hex>`,
persistent delivery, publisher confirms on.

The reference client is `F1R3FLY-io/f1r3node-async-rust` (`f1r3deploy` CLI and
a Rust library). A publish that the broker nacks because the queue is at
`max-length` is the back-pressure signal: slow down and retry the same bytes.

## What the node does with a delivery

The node decodes the bytes, verifies the signature, and calls
`BlockAPI::deploy`. Only that call's outcome decides the acknowledgement:

| Outcome | Queue action | Status event |
| --- | --- | --- |
| admitted | ack | `accepted` |
| already known (redelivery or client retry) | ack | `accepted` with `duplicate: true` |
| terminal (bad signature, wrong shard, parse error, block window closed, phlo price under the floor, undecodable bytes, …) | ack | `rejected` with `reason`, `terminal: true` |
| transient (Casper engine not ready yet, local pool at its cap, storage error) | left unacked, retried with backoff | `deferred` (at most once a minute per deploy) |
| operator condition (validator not bonded, node read-only) | as transient, plus a warning and `deploy_queue_stalled = 1` | `deferred` |

A delivery is acked only after the node has a verdict, so a crash between
admission and the ack causes one redelivery, which is a `duplicate`. Held
(unacked) deliveries fill the `prefetch` window and the broker stops pushing
until the node catches up: that is how the queue absorbs a burst.

## Status events

JSON messages on the topic exchange `f1r3node.deploy.status`:

| Event | Routing key | When |
| --- | --- | --- |
| `accepted`, `rejected`, `deferred` | `<deployer pk hex>.<first 64 hex of sig>` | the consumer's verdict |
| `included` | same | the deploy is in a block this node added (`block_number`, `block_hash`, `errored`, `cost`) |
| `finalized` | same | that block finalized (`errored: true` means the deploy failed in execution) |
| `tip` | `shard.tip` | every block the node adds or creates: `block_number`, `block_hash`, `deploy_lifespan`, `min_phlo_price` (decimal string), `max_user_deploys_per_block` |

The routing key carries a signature prefix because AMQP routing keys are
capped at 255 bytes; the body always carries the full `sig` and `deployer`.
Clients bind `<deployer pk hex>.#` to follow all their deploys, and
`shard.tip` to choose `validAfterBlockNumber` without an HTTP call. Every
event has `schema: 1`, `shard`, `node` (validator pk hex) and `ts_ms`.

## Queue-side TTL

`x-message-ttl = deploy-lifespan × heartbeat.check-interval × ttl-factor`
(default factor `1.0`; `0` disables it). It is housekeeping so a dead node's
queue does not fill with deploys that are certainly past their block window;
ingress re-checks the window on every delivery regardless. Expired messages
go to `f1r3node.deploy.expired`.

## Observability

`GET /api/deploy-queue` returns `{enabled, queue, deploy_exchange,
status_exchange, prefetch, max_length, ttl_ms, connected, stalled, depth,
consumed{accepted,duplicate,rejected}, deferred, status_published,
status_publish_failed}`.

Metrics: `deploy_queue_connected`, `deploy_queue_stalled`,
`deploy_queue_depth`, `deploy_queue_consumed_total{outcome}`,
`deploy_queue_rejected_total{reason}`, `deploy_queue_deferred_seconds`,
`deploy_queue_admission_seconds`.

## TLS

Use an `amqps://` URL and set `deploy-queue.tls.enabled = true` with
`ca-file` pointing at the broker CA's PEM bundle. Client certificates are not
supported yet.
