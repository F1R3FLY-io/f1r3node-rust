# Demo: Deploy a PeTTa contract to a standalone node (Docker Compose)

This walkthrough starts a single-node F1R3FLY network with Docker Compose and
deploys [`03-backtracking.rho`](./03-backtracking.rho), a Rholang contract that
runs a MeTTa program through the `rho:petta:execute` system contract.

The MeTTa program shows **backtracking**: `(parent Tom)` matches two rules, so
MeTTa explores both and the outer `(grandparent Tom)` returns two answers. The
contract then folds over the returned list and prints each grandparent.

```metta
(= (parent Tom) Bob)
(= (parent Tom) Liz)
(= (parent Bob) Ann)
(= (parent Liz) Pat)
(= (grandparent $x) (parent (parent $x)))
!(grandparent Tom)
```

For what PeTTa is and how the service works, see
[`README.md`](./README.md).

## 1. Prerequisites

- Docker and Docker Compose.
- Run every command below from the repository root.

**Build the node image from this branch.** 

```bash
docker build -f node/Dockerfile -t f1r3fly-rust:demo .
```

This image bundles the PeTTa interpreter (SWI-Prolog plus the bubblewrap/seccomp
sandbox).

## 2. Start the node

The demo uses an override file, `docker/standalone.override.yml` (already in this
repo), that the base `standalone.yml` needs for PeTTa:

```yaml
services:
  standalone:
    privileged: true          # bubblewrap mounts its own /proc; blocked otherwise
```

`privileged: true` is required to run `rho:petta:execute`: PeTTa runs each MeTTa
program in a bubblewrap sandbox that mounts its own `/proc`, which Docker's
default confinement blocks (`bwrap: Can't mount proc ... Operation not
permitted`). `seccomp`/`apparmor=unconfined` alone is not enough.

Start the node with the demo image, the bonded development key from
`docker/.env.example`, and both compose files:

```bash
export F1R3FLY_RUST_IMAGE=f1r3fly-rust:demo
export STANDALONE_PRIVATE_KEY=5f668a7ee96d944a4494cc947e4005e172d7ab3461ee5538f1f2a45a835e9657

docker compose -f docker/standalone.yml -f docker/standalone.override.yml up -d
docker compose -f docker/standalone.yml -f docker/standalone.override.yml logs -f standalone
```

Wait until the node finishes genesis. It is ready when this returns
`{"ready":true}`:

```bash
curl -fsS http://127.0.0.1:40403/api/ready
```
## 4. Deploy the contract

Copy the contract into the container, then deploy it. The `deploy` command takes
positional arguments; the private-key path argument is ignored when you pass a
key, so `_` is a safe placeholder.

```bash
docker compose -f docker/standalone.yml cp \
  rholang/examples/system-contract/swipl/03-backtracking.rho \
  standalone:/opt/docker/03-backtracking.rho

docker compose -f docker/standalone.yml exec standalone \
  /opt/docker/bin/node --profile=docker \
  deploy 500000 1 0 "$STANDALONE_PRIVATE_KEY" _ /opt/docker/03-backtracking.rho root
```

Argument order:

| Value | Meaning |
|-------|---------|
| `500000` | phlo limit |
| `1` | phlo price |
| `0` | valid-after-block |
| `$STANDALONE_PRIVATE_KEY` | signing key |
| `_` | private-key path (ignored — a key was given) |
| `/opt/docker/03-backtracking.rho` | contract path in the container |
| `root` | shard id (the standalone default) |

The command prints a deploy id when the node accepts the deploy.

The valid-after-block value `0` (genesis) is valid right after startup. If the
node has produced many blocks and rejects the deploy as expired, pass a recent
block number instead of `0`. Get one from:

```bash
curl -s http://127.0.0.1:40403/api/blocks/1
```

## 5. See the result

The standalone node proposes blocks automatically (the heartbeat proposer is
on), so within a few seconds the contract runs and `rho:io:stdout` prints its
output. 

It prints one line per backtracked answer:

```
"Grandparent of Tom: Ann"
"Grandparent of Tom: Pat"
```

## 6. Teardown

```bash
docker compose -f docker/standalone.yml -f docker/standalone.override.yml down -v
```
