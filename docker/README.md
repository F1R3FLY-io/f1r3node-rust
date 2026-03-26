# Docker Workflows

This directory contains the container image definition, compose files, configuration, genesis data, and monitoring assets for running the Rust node with Docker.

## Quick Start

### Standalone Node

```bash
docker compose -f docker/standalone.yml up
```

Reset to genesis:

```bash
docker compose -f docker/standalone.yml down -v
```

### Multi-Node Shard

```bash
docker compose -f docker/shard.yml up -d
docker compose -f docker/shard.yml logs -f
```

Reset the shard:

```bash
docker compose -f docker/shard.yml down -v
```

## Build A Local Image

Using the helper script:

```bash
./node/docker-commands.sh build-local
```

Using Docker directly:

```bash
docker build -f node/Dockerfile -t f1r3fly-rust-node:local .
```

Run compose with the local image:

```bash
F1R3FLY_IMAGE=f1r3fly-rust-node:local docker compose -f docker/standalone.yml up
F1R3FLY_IMAGE=f1r3fly-rust-node:local docker compose -f docker/shard.yml up
```

## Compose Files

| File | Purpose |
| --- | --- |
| `standalone.yml` | One-node development network with instant finalization |
| `shard.yml` | Bootstrap node, validators, observer |
| `monitoring.yml` | Prometheus and Grafana (joins shard network) |
| `validator4.yml` | Additional validator joining an existing shard |
| `observer.yml` | Read-only observer joining an existing shard |

## Configuration And Genesis Inputs

| Path | Purpose |
| --- | --- |
| `conf/bootstrap.conf` | Bootstrap configuration |
| `conf/default.conf` | Shared validator and observer configuration |
| `conf/standalone-dev.conf` | Standalone configuration |
| `genesis/bonds.txt` | Validator bond set for shard mode |
| `genesis/wallets.txt` | Initial shard wallets |
| `genesis/standalone-bonds.txt` | Standalone bond set |
| `genesis/standalone-wallets.txt` | Standalone wallets |

## Default Port Layout

| Node | Protocol | gRPC Ext | gRPC Int | HTTP | Discovery | Admin |
| --- | --- | --- | --- | --- | --- | --- |
| Bootstrap | `40400` | `40401` | `40402` | `40403` | `40404` | `40405` |
| Validator 1 | `40410` | `40411` | `40412` | `40413` | `40414` | `40415` |
| Validator 2 | `40420` | `40421` | `40422` | `40423` | `40424` | `40425` |
| Validator 3 | `40430` | `40431` | `40432` | `40433` | `40434` | `40435` |
| Validator 4 | `40440` | `40441` | `40442` | `40443` | `40444` | `40445` |
| Observer | `40450` | `40451` | `40452` | `40453` | `40454` | `40455` |

## Monitoring

Start monitoring alongside the shard:

```bash
docker compose -f docker/shard.yml up -d
docker compose -f docker/monitoring.yml up -d
```

- Prometheus: `http://localhost:9090`
- Grafana: `http://localhost:3000`

## Smoke Testing

The compose shard can be validated with the helper scripts already in this repository:

```bash
./scripts/ci/check-casper-init-sla.sh docker/shard.yml 180
./scripts/ci/collect-casper-init-artifacts.sh docker/shard.yml /tmp/casper-init-artifacts
```

If you use the separate `rust-client` repository for end-to-end API checks, point it at the ports exposed by `shard.yml`.

## Image Notes

`node/Dockerfile` builds the `node` binary in a multi-stage image and copies:

- the node executable
- runtime resources from `node/src/main/resources`
- contract resources from `casper/src/main/resources`
- Rholang examples from `rholang/examples`
