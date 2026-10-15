> Last updated: 2026-03-23

# Crate: comm (P2P Networking)

**Path**: `comm/`

P2P networking with Kademlia DHT discovery, custom TLS transport, and connection management.

## Network Stack

```
P2P Layer (PacketHandler)
    |
RP Layer (Remote Protocol - connection lifecycle)
    |
Transport Layer (TLS/gRPC - encrypted channels)
    |
Discovery Layer (Kademlia DHT - peer finding)
```

## Peer Identity

```rust
pub struct PeerNode {
    pub id: NodeIdentifier,   // 32-byte hex-encoded node ID
    pub endpoint: Endpoint,   // host, tcp_port, udp_port
}
// URI format: rnode://hexid@host?protocol=8080&discovery=8081
```

## Kademlia DHT Discovery

**`KademliaNodeDiscovery`**:
1. Identify sparsest distance buckets
2. Create target keys that differ at specific bit positions
3. Query random peers for nodes near targets (Lookup RPC)
4. Filter and add discovered peers

**`PeerTable`** -- k-bucket routing table:
- 256 buckets (XOR distance metric), k=20, alpha=3
- LRU with stale peer replacement via ping

**gRPC Kademlia service** (port 40404):
- `SendPing` / `SendLookup` RPCs
- Network ID validation prevents cross-network discovery
- Kademlia messages do not carry the chain identity. A peer that is refused for another chain is evicted from the routing table and suppressed from rediscovery for 5 minutes (see [Chain identity](#chain-identity)).

## TLS Transport

**Custom verification**: `HostnameTrustManager` extracts P256 public key from peer certificate, derives F1R3FLY address (Keccak256), and matches against advertised identity. Prevents MITM without a CA.

**`TransportLayer` trait**:
- `send(peer, msg)`, `broadcast(peers, msg)`, `stream(peer, blob)`, `disconnect(peer)`
- Helpers: `send_with_retry()`, `send_to_bootstrap()`

## Connection Management

```rust
pub struct ConnectionsCell {
    pub peers: Arc<Mutex<Connections>>,
}
```
- `add_conns()`, `remove_conns()`, `refresh_conn()`, `random(max)`

## Remote Protocol (RP)

**Protocol messages**: `Heartbeat`, `ProtocolHandshake`, `ProtocolHandshakeResponse`, `Disconnect`, `Packet`

**Connection lifecycle**:
1. Local sends `ProtocolHandshake` with network_id and chain_id
2. Peer validates network_id and chain_id before it acknowledges the message, then responds with `ProtocolHandshakeResponse`
3. Connection added to `ConnectionsCell`
4. Periodic `Heartbeat` for liveness
5. Explicit `Disconnect` on teardown. A `Disconnect` from a peer that is not connected is ignored and does not change connection logs or metrics.

**Fan-out**: `broadcast` and `stream_mult` fail only when the message reached no peer. One refusing or unreachable peer does not fail the operation for the other peers.

### Chain identity

A relaunched network can reuse its `networkId`, so `networkId` cannot separate two launches. A node that kept the state of the previous chain would pass the `networkId` check and push blocks of a chain that no longer exists (issue #13). The chain identity separates the two launches.

**Value**: the hash of the shard's genesis block, held in `ChainIdCell` (`rp/chain_id.rs`). It is write-once: a second, different value stops the node.

**Wire**: `Header.chainId` (unary messages) and `ChunkHeader.chainId` (streams). An empty value means that the sender does not know its chain yet.

**When a node learns its identity**:
- Restart on existing data: from storage, at startup (`node/src/rust/runtime/setup.rs`).
- First launch: when the node enters the Running state (`engine::transition_to_running`).
- Joining node: adopted from the header of a unary message from the configured bootstrap peer, then verified against the genesis that the node records.

**Decision rules** (`ChainIdCell::check`):

| Peer's chain_id | Pull (requests, handshake, heartbeat, disconnect) | Push (blocks, gossip, responses) |
| --- | --- | --- |
| Same as ours | accepted | accepted |
| Different from ours | refused | refused |
| Empty | accepted | refused |
| Ours not known yet | accepted | accepted |

A message type that is not in the pull list counts as a push.

**Where the check runs**:

| Path | Site | Effect |
| --- | --- | --- |
| Unary message (all types) | `TransportLayerService::send` | refused before it is acknowledged or enqueued; the sender gets gRPC status `FailedPrecondition`, which maps to `CommError::WrongChainId` |
| Chunk stream | circuit breaker in `grpc_transport_receiver.rs` | the stream is cut at its header chunk |

**What a node does with a refusal**:

| Refused message | Action |
| --- | --- |
| Our handshake (`find_and_connect`) | evict the peer from the routing table at once and suppress its rediscovery for 5 minutes. The configured bootstrap stays pinned. |
| Our heartbeat (`clear_connections`) | evict the peer from the connections at once, without a failure streak |
| Our broadcast (`send_packet_to_peers`) | drop the peer from the connections and close its channel |

`CommError::WrongNetwork` (a `networkId` mismatch, gRPC status `PermissionDenied`) gets the same handling.

**Rollout**: this is a breaking protocol change. A node without it names no chain, so every push from it is refused. All nodes of a network must upgrade together. There is no compatibility setting.

**Known gaps**:
- The stream-path checks run with parameters in thread-local storage and can be skipped when the stream task resumes on another worker thread (#664).
- A joining node adopts its identity only from unary messages, because a stream sender is not bound to the TLS certificate (#663).
- Before a node learns its identity (for example during the genesis ceremony), it accepts any peer. A peer of another chain that connects in that window is dropped by the first refused heartbeat or broadcast after the identity is known.
- A peer on an older build names no chain. Its handshake and heartbeat are pulls and are accepted, so it stays connected, and its pushes are refused.

## Stream Processing (Large Payloads)

**`Chunker`** splits large messages:
- Compress if content > 500KB
- Split into fragments (max_message_size - 2KB buffer)
- Header chunk with metadata + data chunks
- Circuit breaker pattern for error detection

## UPnP

`assure_port_forwarding(ports)` -- Discovers UPnP gateways, maps TCP ports with "F1r3fly" description. Falls back to AWS/WhatIsMyIP for external IP.

## Metrics

Sources: `f1r3fly.comm.rp.connect`, `f1r3fly.comm.rp.handle`, `f1r3fly.comm.discovery.kademlia`, `f1r3fly.comm.rp.transport`

Counters: connect, disconnect, ping, lookup, send, chain-id.refused (messages refused for chain identity, by source `f1r3fly.comm.rp.transport`). Histograms: connect-time, ping-time, lookup-time, send-time.

## Tests

19 test files in `tests/`: transport specs (stream_handler, grpc_transport, transport_layer, uri_parse), discovery specs (distance, kademlia_rpc, kademlia, peer_table), RP specs (find_and_connect, clear_connections, connect, connections), `who_am_i_spec.rs`. Chunker includes inline unit tests.

**See also:** [comm/ crate README](../../comm/README.md)

[← Back to docs index](../README.md)
