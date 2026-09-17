# Joining an Existing Network

How a node that was not part of a shard's genesis joins it, catches up, and —
optionally — becomes a validator.

A node that starts with an empty data directory and a `--bootstrap` address
does not replay the chain from genesis. It fetches the network's last approved
state and the recent blocks around it (Last Finalized State, or LFS), then
starts following the tip. The history below that point is deliberately not
imported.

## What a joiner needs

| Flag | Purpose |
|---|---|
| `--bootstrap` | Address of a node already in the network (see below) |
| `--host` | The address **other nodes** use to reach this one |
| `--protocol-port` | Protocol port to listen on (default 40400) |
| `--discovery-port` | Discovery port to listen on (default 40404) |
| `--allow-private-addresses` | Required when peers advertise private or container addresses — a LAN, a Docker network, or a tunnel |
| `--no-upnp` | Skip UPnP port mapping; use when you have mapped ports yourself |

The bootstrap address is an `rnode://` URL:

```
rnode://<nodeId>@<host>?protocol=<port>&discovery=<port>
```

`<nodeId>` is the peer's TLS-derived identity, not a name you choose. Read it
from that peer's `GET /api/status`, whose `address` field is exactly this URL.

`--host` is what the joiner advertises about itself. Peers dial it back on the
handshake, so it must be an address they can actually reach. Getting this wrong
is the usual cause of a node that connects out but never receives anything.

## Starting a joiner

`docker/observer.yml` and `docker/validator4.yml` are working examples against
a shard started from `docker/shard.yml`. Both take the bootstrap node id and
host from `docker/.env`, and both set `--allow-private-addresses` because
shard members advertise Docker hostnames.

For a node outside that Docker network, the same flags apply, with `--host` set
to an address the shard's members can reach.

## Watching the join

The join is over when the node logs its transition:

```
Making a transition to Running state. Approved #<height> (<hash>...)
```

Before that, `GET /api/status` reports `"isReady": false` and
`lastFinalizedBlockNumber: -1`, and `GET /api/ready` is the clean readiness
probe. A joiner against a healthy shard typically reaches Running in one to two
minutes after fetching on the order of a hundred blocks.

The startup line worth keeping is:

```
request_approved_state: start (block #<approved>, min_height <floor>, ...)
```

`min_height` is the floor of the imported window: `approved` minus the sum of
`max-parent-depth`, the mergeable-store GC buffer, and `deploy-lifespan`. A
joiner that keeps fetching well below its own `min_height` is not converging —
capture the logs rather than waiting it out.

## Restarting a joined node

Restart on the same data directory and the node logs `Approved block found,
reconnecting` and is back in about a second. There is no second LFS fetch.

**Never delete the data directory of a node peers already know.** The node
identity is the TLS certificate inside it, so wiping it produces a new identity
and a full re-join, while peers keep the old one in their routing tables.

## Bonding a joiner as a validator

A node can join as an observer or as a validator; the join itself is identical.
Start it in its final role rather than converting it later: a node started with
`--validator-public-key` / `--validator-private-key` that is not yet bonded
logs that it is not bonded, skips proposing, and picks up its bond when it
takes effect, with no restart.

Bond with the PoS contract — `rholang/examples/bond/bond.rho`, or a client that
wraps it. Three things decide whether it works:

1. **Submit the bond deploy to a bonded validator.** The joiner cannot include
   its own bond, and a node that cannot propose accepts the deploy and strands
   it: deploy queues are node-local.
2. **Fund the joiner's vault first.** A deploy pays `phlo-limit × phlo-price`
   from the deployer's vault, and a bond also moves its stake. An under-funded
   vault does not reject the deploy — the deploy is included in a block and
   fails there with
   `systemDeployError: "Deploy payment failed: Insufficient funds"`. On a
   running shard, fund it by transfer from an already-funded key; editing
   `wallets.txt` only affects a fresh genesis.
3. **Wait for the epoch boundary.** Bonds, withdrawals and payouts apply only
   at a boundary, set by `casper.genesis-block-data.epoch-length`. Between
   boundaries a bond is recorded but not active, and `GET /api/bond-status/{pk}`
   reports it as not yet bonded.

Unbonding is the mirror image: the withdrawal is recorded, the validator leaves
the bond set at the next boundary, and stake plus accumulated rewards are paid
out after `casper.genesis-block-data.quarantine-length`.

**Only bond a validator you intend to keep running.** A bonded validator that
does not propose is counted in consensus while contributing nothing to it.

## When a join does not finish

- **Peers stay at 0.** The joiner cannot be dialed back. Check `--host`, and
  add `--allow-private-addresses` if the shard advertises private or container
  addresses.
- **It fetches past its own `min_height` and keeps going.** Something is
  dragging the import window below its intended floor. Keep the logs.
- **It reaches Running but a bonded validator never proposes.** Being bonded is
  not enough: the validator enters the active set only at an epoch boundary.
  Note that `/api/status`'s `isValidator` reports whether autopropose is
  enabled, not whether the node is a bonded validator.
