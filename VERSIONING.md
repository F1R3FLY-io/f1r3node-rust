# Versioning

## Version Scheme

This repository uses [Semantic Versioning](https://semver.org/) with `v` prefix tags (e.g., `v0.2.0`, `v1.0.0`).

Versions are managed automatically by the nightly release workflow and can be bumped manually via `scripts/release.sh`.

## Lineage from Legacy Repository

This repo was extracted from [`F1R3FLY-io/f1r3fly`](https://github.com/F1R3FLY-io/f1r3fly) (the `rust/dev` branch). The legacy repo used `rust-v*` prefixed tags to distinguish Rust releases from Scala releases.

### Tag Mapping

| This repo | Legacy repo | Notes |
|-----------|-------------|-------|
| `v0.2.0` | `rust-v0.2.0` | Baseline — same codebase at point of extraction |
| `v0.3.0+` | — | New releases from this standalone repo |

### How to Verify Continuity

```bash
# This repo's baseline
git log v0.2.0 --oneline -1

# Legacy repo's equivalent
cd ../f1r3fly
git log rust-v0.2.0 --oneline -1
```

The commit histories diverge after extraction. This repo's `v0.2.0` represents the same Rust codebase as the legacy `rust-v0.2.0`, with the Nix/SBT/Scala build system removed.

## Release Process

### Automated (Nightly)

The `nightly-release.yml` workflow runs daily at 08:00 UTC (midnight Pacific):

1. Checks if `master` has commits since the last `v*` tag
2. If changes exist: bumps the minor version
3. Updates `node/Cargo.toml` version
4. Generates `CHANGELOG.md` via [git-cliff](https://git-cliff.org/)
5. Commits, tags `vX.Y.Z`, pushes
6. Tag push triggers CI: Docker build, integration tests, Docker Hub release

### Manual

For major or patch bumps:

```bash
./scripts/release.sh major   # 0.2.0 → 1.0.0
./scripts/release.sh minor   # 0.2.0 → 0.3.0 (same as nightly)
./scripts/release.sh patch   # 0.2.0 → 0.2.1
```

## Docker Image Tags

| Tag | Source |
|-----|--------|
| `f1r3flyindustries/f1r3node-rust:latest` | Latest master release |
| `f1r3flyindustries/f1r3node-rust:v0.3.0` | Specific version (from `git describe`) |
| `f1r3flyindustries/f1r3node-rust:v0.3.0-amd64` | Architecture-specific |

## Changelog

Generated automatically by git-cliff from conventional commits. See `CHANGELOG.md`.
