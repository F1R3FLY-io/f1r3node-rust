> Last updated: 2026-08-21

# Crate: node (Orchestrator/Entry Point)

**Path**: `node/`

Main binary. Manages node lifecycle, configuration, gRPC/HTTP servers, CLI, and diagnostics.

## Boot Sequence

```
main()
  -> Options::try_parse() (clap CLI)
  -> IF "run" subcommand:
       configuration::builder::build()  (HOCON + CLI merge)
       create Zipkin exporter layer when metrics.zipkin is enabled
       init_logging_with_layers(&cfg.logging, Some(&data_dir), layers)
       check_host(), check_ports(), load_private_key_from_file()
       initialize_diagnostics()  (Prometheus, InfluxDB, Sigar)
       node_runtime::start()
         -> NodeIdentifier from TLS certificate
         -> setup_node_program()
              -> Initialize LMDB stores (block, DAG, casper buffer, deploy, eval, play, replay, reporting)
              -> Create RuntimeManager (play/replay) with history
              -> Create Estimator, ValidatorIdentity
              -> Create count-and-byte-bounded block processor queue
              -> Create proposer queue (oneshot channels)
              -> Create API services and server instances
         -> Spawn concurrent tasks via JoinSet:
              CasperLoop, UpdateForkChoiceLoop, EngineInit, CasperLaunch,
              BlockProcessorInstance, ProposerInstance, HeartbeatProposer, ServersInstances
         -> Monitor tasks, graceful shutdown on SIGTERM/SIGINT
  -> ELSE (CLI subcommand):
       run_cli() -> route to deploy/propose/repl/keygen/status/etc.
```

## Configuration

**Config precedence** (highest wins):
1. CLI arguments (`--native-token-name=X`, `--network-id=Y`, etc.)
2. Config file (`rnode.conf` in data directory)
3. Default config (`defaults.conf` baked into the binary)

**Config build pipeline** (`configuration/mod.rs::build()`):
1. Load embedded `defaults.conf` via HOCON → `HoconLoader::new().load_str(EMBEDDED_DEFAULTS)`. The defaults are baked into the binary at compile time via `include_str!`; no `node/src/main/resources/` directory is required at runtime.
2. Merge user config on top (if `--config-file` is passed or `<data_dir>/rnode.conf` exists) → `default_config.load_file(config_file)`
3. Resolve HOCON substitutions (e.g. `protocol-client.network-id = ${protocol-server.network-id}`)
4. Deserialize merged HOCON into `NodeConf` struct → `merged_config.resolve()`
5. Apply CLI overrides → `node_conf.override_config_values(options)` (via `config_mapper.rs`)
6. Validate → `validate_config(&node_conf)` (e.g. native token non-empty, decimals ≤ 18, quorum ≤ keys)

**Important**: HOCON substitutions resolve in step 3, before CLI overrides in step 5. A CLI flag like `--network-id` must override both `protocol_server.network_id` AND `protocol_client.network_id` because the substitution `${protocol-server.network-id}` already resolved to the HOCON default.

**`NodeConf`** fields:
- `protocol_server` -- P2P (port 40400, network-id, TLS)
- `protocol_client` -- Bootstrap peer, network-id (must match server), timeouts
- `peers_discovery` -- Kademlia (port 40404)
- `api_server` -- gRPC external (40401), internal (40402), HTTP (40403), admin (40405)
- `storage` -- Data directory (default `~/.rnode`)
- `casper` -- Validator key, parents, finalization, heartbeat, genesis block data (bonds, wallets, native token metadata)
- `metrics` -- Prometheus, InfluxDB, Zipkin, Sigar toggles
- `logging` -- Format, sink, file rotation/retention (see [Logging](#logging))
- `dev` -- Dev mode, deployer private key
- `openai` -- LLM integration settings

### CLI Flag Overrides

The following flags override HOCON configuration at startup. CLI flags always take precedence.

| Flag | HOCON Target | Description |
|------|-------------|-------------|
| `--log-level <EXPR>` | `logging.filter` | EnvFilter expression. `RUST_LOG` still wins if set. |
| `--log-format <FORMAT>` | `logging.format` | `json` (default) or `pretty` for human-readable terminal output. |
| `--log-sink <SINK>` | `logging.sink` | `stdout` (default), `file`, or `both`. File is written to `<data-dir>/logs/node.log`. |
| `--ceremony-master-mode` | `casper.genesis_ceremony.ceremony_master_mode = true` | Enable ceremony master mode (creates genesis block if none found) |
| `--enable-mergeable-channel-gc` | `casper.enable_mergeable_channel_gc = true` | Enable mergeable channel garbage collection |
| `--disable-mergeable-channel-gc` | `casper.enable_mergeable_channel_gc = false` | Disable mergeable channel GC (takes precedence over `--enable-mergeable-channel-gc`) |
| `--heartbeat-enabled` | `casper.heartbeat.enabled = true` | Enable heartbeat block proposing for liveness |
| `--heartbeat-disabled` | `casper.heartbeat.enabled = false` | Disable heartbeat proposing (takes precedence over `--heartbeat-enabled`) |
| `--heartbeat-check-interval` | `casper.heartbeat.check-interval` | How often the heartbeat loop wakes; after the initial stall timeout, this is also the recovery-round interval |
| `--heartbeat-max-lfb-age` | `casper.heartbeat.max-lfb-age` | Input to the one-time observed-LFB stall timeout, which is `max(max_lfb_age, check_interval)` |
| `--heartbeat-self-propose-cooldown` | `casper.heartbeat.self-propose-cooldown` | Minimum interval between this validator's heartbeat proposals |
| `--heartbeat-stale-recovery-min-interval` | `casper.heartbeat.stale-recovery-min-interval` | Minimum age of this validator's latest proposal before the pending-deploy recovery backstop may fire |
| `--heartbeat-deploy-finalization-grace` | `casper.heartbeat.deploy-finalization-grace` | Grace window opened when pending deploys or a new user-deploy parent are observed; relaxes the pending-deploy lag cap |
| `--heartbeat-advanced-pending-deploy-max-lag` | `casper.heartbeat.advanced.pending-deploy-max-lag` | EXPERIMENTAL. Lag threshold above which pending-deploy proposals throttle |
| `--heartbeat-advanced-deploy-recovery-max-lag` | `casper.heartbeat.advanced.deploy-recovery-max-lag` | EXPERIMENTAL. Wider lag cap during the deploy-finalization grace window |
| `--heartbeat-advanced-empty-frontier-max-unfinalized-blocks` | `casper.heartbeat.advanced.empty-frontier-max-unfinalized-blocks` | EXPERIMENTAL. Exact unfinalized-DAG cap for idle empty recovery while this validator is already ahead |
| `--native-token-name` | `casper.genesis_block_data.native_token_name` | Native token display name (genesis-locked) |
| `--native-token-symbol` | `casper.genesis_block_data.native_token_symbol` | Native token ticker symbol (genesis-locked) |
| `--native-token-decimals` | `casper.genesis_block_data.native_token_decimals` | Native token decimal places, 0-18 (genesis-locked) |

**Precedence rules for paired flags**: When both an enable and disable flag are provided for the same setting, the disable flag wins. The config mapper evaluates `--disable-*` after `--enable-*`, so the disable always takes final effect.

### Config Mapping

CLI flags are applied to the parsed `NodeConf` by `config_mapper.rs`:

- `--ceremony-master-mode` unconditionally sets `casper.genesis_ceremony.ceremony_master_mode = true`.
- `--disable-mergeable-channel-gc` / `--enable-mergeable-channel-gc` override `casper.enable_mergeable_channel_gc`. The disable flag is checked first; only if it is absent does the enable flag apply.
- `--heartbeat-disabled` / `--heartbeat-enabled` follow the same pattern for `casper.heartbeat.enabled`.

## gRPC Services

| Service | Port | Methods |
|---------|------|---------|
| **DeployService** | 40401 (external) | `do_deploy`, `show_main_chain`, `get_blocks`, `get_block`, `find_deploy`, `exploratory_deploy`, `last_finalized_block`, `is_finalized`, `bond_status`, `get_data_at_name`, `listen_for_continuation_at_name`, `status`, `machine_verifiable_dag`, `visualize_dag`, `get_blocks_by_heights`, `get_event_by_hash` |
| **ProposeService** | 40402 (internal) | `propose`, `propose_result` |
| **ReplService** | 40402 (internal) | `run` (single command), `eval` (full program) |
| **LspService** | 40402 (internal) | `validate` (Rholang syntax diagnostics) |
| **Transport (P2P)** | 40400 | `packet_handler`, `streamed_blob_handler` |
| **Kademlia RPC** | 40404 | `ping`, `lookup` |

## HTTP REST API

| Port | Purpose |
|------|---------|
| 40403 | Public REST (deploy, blocks, finalization, balance, validators, epoch, status) via Axum |
| 40405 | Admin (propose, propose_result) |

**`/api/status`** returns node identity, network membership, native token metadata, and operational state. HTTP and gRPC endpoints return identical fields:

```json
{
  "version": {"api": "1", "node": "..."},
  "address": "rnode://...",
  "networkId": "testnet",
  "shardId": "root",
  "peers": 4,
  "nodes": 4,
  "minPhloPrice": 1,
  "nativeTokenName": "F1R3CAP",
  "nativeTokenSymbol": "F1R3",
  "nativeTokenDecimals": 8,
  "peerList": [...],
  "lastFinalizedBlockNumber": 1234,
  "isValidator": true,
  "isReadOnly": false,
  "isReady": true,
  "currentEpoch": 12,
  "epochLength": 100
}
```

- `lastFinalizedBlockNumber` — block number of the LFB, or -1 if casper not yet initialized
- `isValidator` — true if the node has a propose function (can create blocks)
- `isReadOnly` — true if the node is running in read-only mode
- `isReady` — true after the engine enters Running state; clients can poll this instead of parsing logs
- `currentEpoch` — `lastFinalizedBlockNumber / epochLength`
- `epochLength` — blocks per epoch, from genesis configuration

## View Parameters

All block and deploy endpoints support a `?view=full|summary` query parameter:

| Endpoint | Default | `?view=summary` | `?view=full` |
|----------|---------|-----------------|--------------|
| `GET /api/block/{hash}` | **full** (block + deploys + transfers) | Block header only | — |
| `GET /api/last-finalized-block` | **full** (block + deploys + transfers) | Block header only | — |
| `GET /api/deploy/{id}` | **full** (all deploy fields) | Core fields only | — |
| `GET /api/blocks` | **summary** (block headers) | — | Headers + deploys |
| `GET /api/blocks/{depth}` | **summary** (block headers) | — | Headers + deploys |
| `GET /api/blocks/{start}/{end}` | **summary** (block headers) | — | Headers + deploys |

Single-item lookups default to full. Lists default to summary. Unknown view values fall back to the endpoint's default.

## High-Level Query Endpoints

Convenience endpoints for common queries. Most wrap `exploratory_deploy` with Rholang queries against system contracts — **readonly nodes only** (validators return errors). `/api/epoch` and `/api/bond-status` use direct APIs and work on all node types.

All query endpoints accept an optional `?block_hash=` parameter to query against a specific block's post-state. Defaults to the last finalized block if omitted.

### `GET /api/balance/{address}`

Returns the vault balance for a wallet address. The address must be a REV address (Base58-encoded, starts with `1111`). Queries the SystemVault contract at `rho:vault:system`.

```json
{"address": "04abc...", "balance": 1000000, "blockNumber": 42, "blockHash": "abc..."}
```

### `GET /api/registry/{uri}`

Looks up a registry URI (e.g. `rho:id:...`). Unwraps the `(true, data)` tuple from the registry — returns the inner data directly. If the URI is not found, returns `"not found"`.

```json
{"uri": "rho:id:abc...", "data": [<RhoExpr>], "blockNumber": 42, "blockHash": "abc..."}
```

### `GET /api/validators`

Returns the active validator set with stake from the PoS contract at `rho:system:pos` (`getBonds`).

```json
{"validators": [{"publicKey": "04abc...", "stake": 100}], "totalStake": 300, "blockNumber": 42, "blockHash": "abc..."}
```

### `GET /api/epoch`

Returns current epoch info. `epochLength` and `quarantineLength` are from genesis configuration (cached at startup). `currentEpoch` and `blocksUntilNextEpoch` are derived from the block number. No exploratory deploy — available on both validators and readonly nodes.

```json
{"currentEpoch": 15, "epochLength": 100, "quarantineLength": 10, "blocksUntilNextEpoch": 3, "lastFinalizedBlockNumber": 1497, "blockHash": "abc..."}
```

### `GET /api/epoch/rewards`

Current epoch rewards from the PoS contract. Readonly only.

### `POST /api/estimate-cost`

Estimate committed-COMM plus canonical RSpace byte cost without settling a
user purse. Takes `{"term": "..."}`, returns `{"cost": 39, ...}`. Readonly
only.

### `GET /api/validator/{pubkey}`

Status of a specific validator — bond and stake. Readonly only.

### `GET /api/bond-status/{pubkey}`

Check if a public key is bonded. Uses `BlockAPI::bond_status` directly — available on all node types.

See [api-reference.md](api-reference.md) for complete endpoint documentation with curl examples.

## Rholang Type System (RhoExpr)

API responses from `explore-deploy`, `data-at-name-by-block-hash`, `registry`, and related endpoints return Rholang values as `RhoExpr` — a JSON-serializable representation of all Rholang types.

### Supported types

| Category | RhoExpr variant | JSON example |
|----------|----------------|-------------|
| **Primitives** | | |
| Boolean | `ExprBool` | `{"ExprBool": {"data": true}}` |
| Integer | `ExprInt` | `{"ExprInt": {"data": 42}}` |
| String | `ExprString` | `{"ExprString": {"data": "hello"}}` |
| URI | `ExprUri` | `{"ExprUri": {"data": "rho:io:stdout"}}` |
| Bytes | `ExprBytes` | `{"ExprBytes": {"data": "0a1b2c"}}` |
| **Extended numerics** | | |
| Float (f64) | `ExprFloat` | `{"ExprFloat": {"data": 3.14}}` |
| BigInt | `ExprBigInt` | `{"ExprBigInt": {"data": "12345678901234567890"}}` |
| BigRational | `ExprBigRat` | `{"ExprBigRat": {"numerator": "1", "denominator": "3"}}` |
| FixedPoint | `ExprFixedPoint` | `{"ExprFixedPoint": {"value": "31415", "scale": 4}}` |
| **Collections** | | |
| Tuple | `ExprTuple` | `{"ExprTuple": {"data": [...]}}` |
| List | `ExprList` | `{"ExprList": {"data": [...]}}` |
| Set | `ExprSet` | `{"ExprSet": {"data": [...]}}` |
| Map | `ExprMap` | `{"ExprMap": {"data": {"key": ...}}}` |
| Par (parallel) | `ExprPar` | `{"ExprPar": {"data": [...]}}` |
| **Unforgeable names** | | |
| Private | `ExprUnforg` | `{"ExprUnforg": {"data": {"UnforgPrivate": {"data": "hex..."}}}}` |
| Deploy ID | `ExprUnforg` | `{"ExprUnforg": {"data": {"UnforgDeploy": {"data": "hex..."}}}}` |
| Deployer ID | `ExprUnforg` | `{"ExprUnforg": {"data": {"UnforgDeployer": {"data": "hex..."}}}}` |
| System auth | `ExprUnforg` | `{"ExprUnforg": {"data": "UnforgSysAuthToken"}}` |
| **Bundle** | `ExprBundle` | `{"ExprBundle": {"data": ..., "read": true, "write": false}}` |
| **Operators** | | |
| Arithmetic | `ExprPlus`, `ExprMinus`, `ExprMult`, `ExprDiv`, `ExprMod` | `{"ExprPlus": {"left": ..., "right": ...}}` |
| Comparison | `ExprLt`, `ExprLte`, `ExprGt`, `ExprGte`, `ExprEq`, `ExprNeq` | `{"ExprEq": {"left": ..., "right": ...}}` |
| Logical | `ExprNot`, `ExprNeg`, `ExprAnd`, `ExprOr` | `{"ExprAnd": {"left": ..., "right": ...}}` |
| String | `ExprConcat`, `ExprInterpolate`, `ExprDiff` | `{"ExprConcat": {"left": ..., "right": ...}}` |
| **Other** | | |
| Pattern match | `ExprMatches` | `{"ExprMatches": {"target": ..., "pattern": ...}}` |
| Method call | `ExprMethod` | `{"ExprMethod": {"target": ..., "name": "method", "args": [...]}}` |
| Variable | `ExprVar` | `{"ExprVar": {"index": 0}}` |
| Process | `ExprUnknown` | `{"ExprUnknown": {"type_name": "Process"}}` |
| Unknown | `ExprUnknown` | `{"ExprUnknown": {"type_name": "..."}}` |

### Design

- **No silent drops**: every Rholang type has a representation. Unknown future types render as `ExprUnknown` with a type name — never silently disappear from responses.
- **Map keys**: any RhoExpr can be a map key. Primitives use natural string representation; complex types are serialized to JSON strings.
- **Extended numerics**: `BigInt`, `BigRat`, and `FixedPoint` are represented as decimal strings (not binary) for client readability. `Float` is IEEE 754 f64.
- **Process-level constructs** (sends, receives, new bindings) are represented as `ExprUnknown { type_name: "Process" }` rather than full AST serialization. These are rarely returned by data queries.
- **Deploy not found**: returns HTTP 404 (not 400) so clients can distinguish "not yet in block" from "invalid request."

### Key files

- `api/web_api.rs` — `RhoExpr` enum, `expr_from_par_proto()`, `expr_from_expr_proto()`, `unforg_from_proto()`, `extract_key_from_expr()`

**See also:** [Exploratory Deploy](exploratory-deploy.md)

## WebSocket Events

The `/ws/events` endpoint on the HTTP port (40403) streams real-time node events. See [websocket-events.md](websocket-events.md) for full documentation.

9 event types are streamed: 3 block lifecycle (`block-created`, `block-added`, `block-finalised`), 4 genesis ceremony (`sent-unapproved-block`, `block-approval-received`, `sent-approved-block`, `approved-block-received`), and 2 node lifecycle (`entered-running-state`, `node-started`).

Events published during startup are buffered and replayed to clients that connect after the node is running. The buffer is sealed when engine initialization completes.

## Error Handling & Shutdown

`handle_unrecoverable_errors()` in `node_runtime.rs` is the top-level error boundary. Any `Err` from `NodeRuntime::main()` is caught, logged via `tracing::error!`, and the process exits with code 1. This covers:
- Config validation failures (empty token name, invalid decimals)
- Genesis ceremony failures (required signatures not met)
- Token metadata verification mismatch (joiner config disagrees with on-chain state)
- Mergeable-channel cache replay failures at bootstrap: a block missing from the block store, a replay error, or a post-state hash mismatch while repopulating the mergeable-channel cache. The cache is locally replay-derived because the synchronized block does not authenticate a peer's auxiliary merge vector. Legacy response payloads are ignored. A corrupt or partial block store therefore **fails startup loudly** rather than continuing with a silently incomplete cache; a node reaches Running only after exact local reconstruction succeeds.
- Any runtime panic or unrecoverable error

The error chain propagates cleanly: `verify_token_metadata_matches_config → Err(CasperError) → ? in casper_launch.launch() → ? in NodeRuntime::main() → handle_unrecoverable_errors → process::exit(1)`. Destructors fire in order; no mid-async process::exit calls.

## API Server Startup

`bind_tcp_listener_with_retry()` in `servers_instances.rs` handles `AddrInUse` resilience for HTTP/Admin servers: 60 attempts with 500ms delay between retries.

`APIServers::build()` in `api_servers.rs` constructs all gRPC services (Repl, Propose, Deploy, LSP) with shared dependencies (engine cell, block store, connections, epoch_length, is_ready). `WebApiImpl` in `web_api.rs` handles the HTTP REST layer and caches config-derived values (network-id, shard-id, min-phlo-price, native token metadata, epoch-length) for fast `/api/status` responses without per-request config reads. The `is_ready` flag is a shared `AtomicBool` set by the event listener in `setup.rs` when `EnteredRunningState` fires.

## Transfer Extraction

Transfer data (from/to/amount/success) is extracted from block execution reports and inlined on `DeployInfo` for `get_block` and `last_finalized_block` responses.

### Architecture

Transfers are extracted from `BlockReportAPI`, which replays blocks using `ReportingRspace` to capture full COMM event data. Results are cached in `ReportStore` — each block is replayed once, then served from cache forever.

```
API handler (get_block / last_finalized_block)
  → BlockReportAPI.block_report(hash, false)
    → ReportStore check (cached? → return immediately)
    → ReportingCasper.trace(block) → full replay → cache in ReportStore
  → extract_transfers_from_report(&report, &transfer_unforgeable)
    → scan COMM events on transfer_unforgeable channel
    → parse from/to/amount/success from produce data
  → populate DeployInfo.transfers / DeployInfoSerde.transfers
```

### Behavior by node type

| Node type | HTTP `transfers` field | gRPC `transfers` / `transfersAvailable` |
|-----------|----------------------|----------------------------------------|
| **Readonly** | `"transfers": [...]` (populated) or `"transfers": []` (no transfers) | `transfers: [...]`, `transfersAvailable: true` |
| **Validator** | Field **omitted** (block replay unavailable) | `transfers: []`, `transfersAvailable: false` |

- HTTP uses `Option<Vec<TransferInfoSerde>>` with `skip_serializing_if = "Option::is_none"` — field absent when `None`
- gRPC uses `repeated TransferInfo` (always present, may be empty) + `bool transfersAvailable` to distinguish

### Key files

- `web/block_info_enricher.rs` — `extract_transfers_from_report()` standalone function, `find_transfers_in_report()` per-deploy scanner
- `web/transaction.rs` — `transfer_unforgeable()` (computes transfer channel Par from SystemVault.rho), `helpers` module for parsing produce event data
- `api/web_api.rs` — `WebApiImpl.enrich_transfers()` for HTTP path
- `api/deploy_grpc_service_v1.rs` — `DeployGrpcServiceV1Impl.enrich_proto_transfers()` for gRPC path
- `runtime/setup.rs` — wires `BlockReportAPI` + `transfer_unforgeable` into API services, proactive cache on finalization events

### Proactive caching

On finalization, a background task calls `block_report_api.block_report(hash, false)` to pre-warm `ReportStore`. On validators this is a no-op (block report rejected). On readonly nodes, the first API query for a block hits the pre-warmed cache.

## Find Deploy Retry

Both gRPC and REST APIs retry `find_deploy` on `DeployNotFoundError`:

| API | Retry Interval | Max Attempts |
|-----|----------------|--------------|
| gRPC | 100ms | 80 |
| REST | 50ms | 1 |

These values are hardcoded (previously configurable via `F1R3_*` env vars, removed in v0.4.10).

## Runtime Instances

**`BlockProcessorInstance`** -- Receives blocks, validates, applies to DAG. Semaphore-bounded parallelism. Re-queues on `FinalizationInProgress`.

Inbound block admission is bounded independently by message count and encoded
bytes. The byte ceiling is the configured
`protocol-server.grpc-max-recv-stream-message-size`, so every block the
transport can accept can also be admitted when the budget is empty. A
reservation covers both queue residence and in-flight replay. Temporary count
or byte pressure releases the decoded payload and reopens an existing
retriever request without evicting other unresolved work. A previously
untracked block arriving while the finite request map is full can still enter
the independently byte-bounded queue. If count or byte pressure prevents that
admission, its payload is released and its hash becomes eligible again on a
later announcement or dependency scan. A queue-coordinator mutex serializes
startup and replay-completion dependency-buffer scans; the scanner checks
deterministically ordered hashes while materializing only one full block at a
time, then moves selected blocks into the byte-owning queue without cloning.
Observe
`block-processing.queue.pending`, `block-processing.admission.bytes`,
`block-processing.admission.bytes-limit`, and
`block-processing.admission.deferred.total{reason=...}` together with
`block.requests.capacity-deferred.total`.

The preceding P2P layer has its own finite byte/item, HTTP/2, handler, peer-map,
and completion boundaries. It reports stream success only after a remote ACK,
keeps accepted work alive across concurrent cleanup, and never contributes
transport-local ordering or metadata to consensus state. See
[P2P Transport Resource and Completion Semantics](transport-resource-lifecycle.md).

### Block-processing tuning env vars

| Env var | Default | Purpose |
|---------|--------:|---------|
| `F1R3_MALLOC_TRIM_EVERY_BLOCKS` | `1` | Linux/glibc only: ask the allocator to return whole free replay and RSpace arena pages to the operating system after every N completed incoming block-processing tasks. The default closes the block-lifecycle allocation boundary on validators, joining validators, and read-only nodes; every local proposal attempt closes the corresponding creator boundary. Set a larger interval only after demonstrating that the resulting peak RSS remains within the deployment's memory envelope. `0` disables explicit trimming. See [Block-Heap Lifecycle and Reclamation](../casper/theory/cost-accounting-impl/block-heap-lifecycle.md). |
| `F1R3_MISSING_DEPENDENCY_QUARANTINE_MS` | `120000` | How long a block whose dependencies exceeded the retry budget stays quarantined before another fetch round. Was 10s through v0.4.16; raised to 120s to stop request storms against slow peers. Lower it on small local networks where dependencies resolve fast. |

**`ProposerInstance`** -- Dequeues proposal requests. Non-blocking locking (try_lock). 5-minute timeout for stuck proposals. Min-interval between proposals is 250ms (hardcoded).

**`HeartbeatProposer`** -- Periodic proposals for network liveness. All heartbeat settings live in `defaults.conf` under the `casper.heartbeat` section and accept CLI overrides. Stable knobs are at the top level; experimental tuning knobs are nested under `advanced.*` and may change shape in a future release.

| HOCON key | Default | Purpose |
|-----------|--------:|---------|
| `heartbeat.enabled` | `true` | Enable the heartbeat proposer |
| `heartbeat.check-interval` | 5s | How often the loop evaluates its decision tree |
| `heartbeat.max-lfb-age` | 15s | Input to the one-time observed-LFB stall timeout |
| `heartbeat.self-propose-cooldown` | 3s | Min interval between self-proposals |
| `heartbeat.stale-recovery-min-interval` | 3s | Min age of this validator's latest proposal before the pending-deploy backstop may fire |
| `heartbeat.deploy-finalization-grace` | 25s | Grace window opened when pending deploys land; relaxes lag caps |
| `heartbeat.advanced.pending-deploy-max-lag` | 20 | EXPERIMENTAL. Lag threshold above which pending-deploy proposals throttle |
| `heartbeat.advanced.deploy-recovery-max-lag` | 64 | EXPERIMENTAL. Wider lag cap during the deploy-finalization grace window. Must be >= `pending-deploy-max-lag` to take effect (else collapses to that floor). |
| `heartbeat.advanced.empty-frontier-max-unfinalized-blocks` | 64 | EXPERIMENTAL. Idle empty recovery stops at this exact unfinalized-DAG boundary when the validator is already ahead. |

**Deploy grace window**: When pending deploys or a new user-deploy parent are
observed, a grace window opens (default 25s) and widens the pending-deploy lag cap
from `pending-deploy-max-lag` to `deploy-recovery-max-lag`. It does not waive the
self-propose cooldown.

**Observed-LFB rotating recovery**: Each heartbeat task measures monotonic elapsed
time since it first observed the current LFB hash. Producer timestamps, frontier
movement, and latest-message churn do not reset that clock. The first local
recovery round opens after
$`\max(\mathtt{max\mbox{-}lfb\mbox{-}age},\mathtt{check\mbox{-}interval})`$;
later rounds open
every `check-interval`. A delayed wake exposes the earliest uncompleted available
round, so the task catches up in order without skipping a rotating leader. A
nonleader completes that local round without proposing; a selected leader retains
the round until the serialized proposer starts or succeeds. The unique leader is
selected from the canonical snapshot committee by
$`(\mathtt{nonnegative\_lfb\_height}+\mathtt{local\_round}) \bmod
\mathtt{committee\_size}`$, so an offline leader
is rotated past. Validators may occupy different local rounds. This scheduling
does not change block validation or the mutual causal and state-preserving clique
certificates required for finality.

## Logging

Structured logging uses the `tracing` crate. The subscriber is initialised from `NodeConf.logging` after config is built, so operators can control it from `rnode.conf`.

### Configuration (`logging { }` in HOCON)

| Key | Default | Values |
|---|---|---|
| `filter` | `"info"` | Any `EnvFilter` expression, e.g. `"info,f1r3fly.casper=debug"` |
| `format` | `"json"` | `"json"` (structured, for aggregators) · `"pretty"` (human-readable, for terminals) |
| `sink` | `"stdout"` | `"stdout"` · `"file"` · `"both"` |
| `file.rotation` | `"daily"` | `"never"` · `"hourly"` · `"daily"` |
| `file.retention` | `14` | Number of rotated files to keep; `0` = unlimited |

When `sink` includes `"file"`, logs are written to `<data-dir>/logs/node.log`. The `logs/` subdirectory is created automatically. In Docker the data dir is `/var/lib/rnode`, so log files land at `/var/lib/rnode/logs/node.log`.

### Precedence (highest wins)

1. `RUST_LOG` environment variable
2. `--log-level` / `--log-format` / `--log-sink` CLI flags
3. `logging.*` from `rnode.conf` or `defaults.conf`

### Target taxonomy

All explicit `target:` values in the codebase follow the convention `f1r3fly.<area>.<concern>` with underscores. Examples:

```
f1r3fly.casper                       # general consensus events
f1r3fly.casper.compute_parents_post_state.timing
f1r3fly.casper.mem_profile           # RSS memory samples (debug level)
f1r3fly.rspace                       # tuple-space events
f1r3fly.merge.dag_merger.state_changes
f1r3fly.node.transaction
```

To enable mem profiling:
```bash
RUST_LOG="warn,f1r3fly.casper.mem_profile=debug" ./node run ...
```

### JSON output

The JSON layer emits one object per event with `span` and `spans` fields for trace correlation with the OpenTelemetry/Zipkin integration. Example:

```json
{"timestamp":"2026-05-28T12:00:00Z","level":"INFO","target":"f1r3fly.casper","message":"compute-state-started","span":{"name":"run"},"spans":[]}
```

## Diagnostics

`initialize_diagnostics()` sets up:
- Prometheus (`/metrics` HTTP endpoint)
- InfluxDB (HTTP batch and/or UDP reporters)
- Sigar (CPU, memory, disk system metrics)

Zipkin is initialized before the process-wide `tracing` subscriber so its
OpenTelemetry layer shares the same span stream as stdout and file logging.
Enable it with `metrics.zipkin = true` or `--zipkin`. The batch exporter uses
an asynchronous Reqwest 0.12 client over Rustls, installs the B3 propagation
format, and flushes the global tracer provider during orderly shutdown. Set
`OTEL_EXPORTER_ZIPKIN_ENDPOINT` to the collector's v2 spans endpoint; the
default is `http://127.0.0.1:9411/api/v2/spans`. Set
`OTEL_EXPORTER_ZIPKIN_TIMEOUT` to the export timeout in milliseconds; the
default is 10,000. Startup fails instead of advertising tracing when the
exporter cannot be constructed.

## CLI Subcommands

| Command | Purpose |
|---------|---------|
| `run` | Start node |
| `eval FILE` | Execute Rholang file |
| `repl` | Interactive REPL |
| `deploy VALID_AFTER_BLOCK [KEY] [KEY_PATH] FILE SHARD` | Sign and deploy a contract; capacity comes from authenticated purses |
| `propose` | Trigger block proposal |
| `show-block HASH` | Display block |
| `show-blocks DEPTH` | Recent blocks |
| `visualize-dag DEPTH` | DAG structure |
| `keygen PATH` | Generate key pair |
| `last-finalized-block` | Latest finalized block |
| `is-finalized HASH` | Check finalization |
| `bond-status KEY` | Validator bond query |
| `cont-at-name NAMES` | RSpace continuation subscription |
| `status` | Node status |

## Tests

Integration tests in `tests/`: `rho_trie_traverser_test.rs`. Inline tests in `block_info_enricher.rs` (2 unit tests for transfer extraction logic).

**See also:** [node/ crate README](../../node/README.md) | [Docker Setup](../../docker/README.md)

[← Back to docs index](../README.md)
