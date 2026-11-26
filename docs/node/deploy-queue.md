# Deploy Queue Client API

A deploy queue runs next to one validator and serves that validator's client ports. A client sends deploys and
reads their status as it would from the node. The queue holds each deploy and releases it as fast as the
validator takes deploys into its blocks. How the queue decides is in the
[deploy-queue README](../../deploy-queue/README.md).

| Port | Protocol | Served by the queue | Sent on to the node |
|------|----------|---------------------|---------------------|
| 40401 | gRPC | `DeployService.doDeploy`, `DeployService.deployFinalizationStatus` for a held deploy, `deployqueue.v1.DeployQueue.watchDeploys` | Every other `DeployService` call |
| 40403 | HTTP | `POST /api/deploy`, `POST /api/v1/deploy`, `GET /api/deploy-finalization-status/{sig}`, `GET /ws/deploys`, `GET /ws/events`, `GET /queue/status`, `GET /queue/metrics` | Every other path |

A queue fronts a validator only. Each time it connects to its node it reads
[`GET /api/status`](api-reference.md#get-apistatus), and it stops with an error when the node is read-only
(`isReadOnly`) or has no validator key (`isValidator`). A validator that is not bonded creates no blocks, so its
queue refuses deploys with `deploy_queue_full` once a citable depth passes without one.

## Submitting a deploy

The request is the node's [`POST /api/deploy`](api-reference.md#post-apideploy) body or the gRPC `doDeploy`
message. A success answers the deploy ID as the node does.

The queue applies the node's admission rules before it accepts a deploy. `validAfterBlockNumber` must name a
block in the open window:

- above `latestBlockNumber - deployLifespan`, or the deploy is expired (`deploy_expired`);
- at most `latestBlockNumber + maxParentDepth`, or the deploy is refused (`invalid_deploy_window`).

`phloLimit` must be at most `maxPhloLimit`, or the deploy is refused (`illegal_argument`).

Read `latestBlockNumber`, `deployLifespan`, `maxParentDepth` and `maxPhloLimit` from
[`GET /api/status`](api-reference.md#get-apistatus).

## Refusals

| `error` | HTTP | gRPC | Meaning | Client action |
|---------|------|------|-------|---------------|
| `invalid_request_body`, `illegal_argument`, `deploy_expired`, `invalid_deploy_window` | 400 | `ServiceError` in the response, as the node; `deploy_expired` and `invalid_deploy_window` messages start with their kind | The deploy is not valid | Correct the deploy |
| `duplicate_deploy` | 409 | `ServiceError` | The queue already holds this deploy | Read its status |
| `deploy_queue_full` | 503 + `Retry-After`, `cause` in the body | `RESOURCE_EXHAUSTED` + `retry-after` and `cause` metadata | At the validator's intake, the deploys ahead would keep this one out of a block until its window closes, or this deployer holds its full share of the queue | Wait for `Retry-After` seconds, then do the action of its `cause` (see below) |
| `node_unavailable` | 503 + `Retry-After` | `UNAVAILABLE` + `retry-after` metadata | The queue is not connected to its node | Wait and send again |
| `deploy_queue_stopped` | 503 | `UNAVAILABLE` | The queue is shutting down | Send again later |
| `queue_store_error` | 500 | `INTERNAL` | The queue could not store the deploy | Send again later |

A refused deploy is not held. A `deploy_queue_full` refusal names its cause in the `cause` field of the HTTP body
and in the `cause` gRPC metadata. Its message also names the cause in words.

```json
{
  "error": "deploy_queue_full",
  "cause": "deployer_share",
  "message": "deploy queue is full for this deployer: it holds 12 of the 12 deploys one deployer may hold, so that other deployers keep room; retry in 30s"
}
```

| `cause` | The queue is full because | Client action after `Retry-After` |
|---------|---------------------------|-----------------------------------|
| `validator` | The validator takes fewer deploys than arrive | Sign the deploy again with a current `validAfterBlockNumber`, then send it |
| `validator_idle` | The validator has created no block over a citable depth while the shard moved on | Sign the deploy again with a current `validAfterBlockNumber`, then send it |
| `shard` | The node is holding its validator back because finality trails the tip, or finality trails the tip by more than `maxParentDepth` and the queue holds every deploy until it catches up | Sign the deploy again with a current `validAfterBlockNumber`, then send it |
| `storage` | The signed deploys the queue holds fill its storage budget (half of its 16 GiB store) | Sign the deploy again with a current `validAfterBlockNumber`, then send it |
| `deployer_share` | This deployer holds its full share of the queue | Send the same signed deploy again |

For `deployer_share`, do not sign the deploy again. The same signed deploy keeps its deploy ID.

While its validator creates no block, the queue takes no deploy, and its `Retry-After` is one citable depth at the
shard's pace. Thus, for `validator_idle`, a client that cannot wait sends the deploy to another validator's queue.

A documented `cause` value keeps its meaning. A later queue can add values. A client handles a
`deploy_queue_full` refusal with no `cause`, or with a value it does not know, as a `deploy_queue_full` refusal
with no cause.

The queue does not compile the Rholang term. The node checks it when the queue releases the deploy, and a term
that does not compile ends the deploy `Rejected` with `rholang_bad_term` in its status detail.

## Deploy status

`GET /api/deploy-finalization-status/{sig}` answers the node's body plus two fields:

```json
{
  "state": "Pending",
  "rejection_count": 0,
  "latest_block_hash": null,
  "queue_state": "queued",
  "detail": null
}
```

| `state` | `queue_state` | Meaning |
|---------|---------------|---------|
| `Pending` | `queued` | Accepted. The deploy waits in the queue. |
| `Pending` | `submitted` | Released. The deploy is in the node's pool. |
| `Pending` | `in-block` | A block carries the deploy. The queue waits for the node's verdict. |
| `Finalized` | `finalized` | The node's verdict. |
| `Failed` | `failed` | The node's verdict. |
| `Expired` | `expired` | The node's verdict, or the window of the deploy closed in the queue. |
| `Rejected` | `rejected` | The node refused the deploy when the queue released it. `detail` holds the node's error kind and message. |

A node never answers `Rejected`. The gRPC status gives it as `DEPLOY_STATE_REJECTED`. Terminal states do not
change. Send the saved signed deploy to `POST /api/deploy-finalization-status` or in the gRPC query's
optional `deploy` field. The queue checks the signature before returning a local status.
For an unknown deploy, the queue forwards the request to the node and returns its verdict, including
`basisBlockHash` and `basisBlockHeight`. See the [reconciliation API](api-reference.md#post-apideploy-finalization-status).

## Watching deploys

`GET /ws/deploys?sigs=<hex>,<hex>` opens a WebSocket that sends the current status of each named deploy, then
every change:

```json
{"deploy_id": "3044...", "status": {"state": "Pending", "rejection_count": 0, "latest_block_hash": null, "queue_state": "submitted", "detail": null}}
```

`status` is `null` for a deploy the queue does not hold. gRPC `deployqueue.v1.DeployQueue.watchDeploys` streams
the same transitions and ends when every named deploy is terminal.

## Queue state

`GET /queue/status` answers the queue's counts per state, what it measures of its validator (own blocks, share
of the shard's blocks, intake per height, pool target, latest cap and its source) and of the shard (seconds per
height, backpressure, finality lag), and `finality_hold` with the lag while the queue holds release for finality.
Queued deploys are counted as `ready` (window open, released earliest deadline first), `waiting` (not yet valid:
`validAfterBlockNumber` above the tip) and `closed` (window closed before release); then `submitted`, `in_block`, and the terminal
`finalized`, `failed`, `expired` and `rejected`. A terminal deploy stays held for one lifespan after its verdict,
so its status can still be read. `held` is the sum of all of these. `payload_bytes` is the size of the signed
deploys held for undecided deploys, which the storage budget caps.
`GET /queue/metrics` answers Prometheus metrics. `deploy_queue_refused` counts refused deploys by `reason`, the
refusal's `error`, and by `cause`, which is `none` for a refusal without one. `GET /ws/events` relays the node's
[event stream](websocket-events.md).
