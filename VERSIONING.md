# Versioning

## Version Scheme

This repository uses [Semantic Versioning](https://semver.org/) with `v` prefix tags (e.g., `v0.4.13`, `v1.0.0`).

Versions are managed automatically by the nightly release workflow and can be bumped manually via `scripts/release.sh`.

## Lineage from Legacy Repository

This repo was extracted from [`F1R3FLY-io/f1r3fly`](https://github.com/F1R3FLY-io/f1r3fly) (the `rust/dev` branch). The legacy repo used `rust-v*` prefixed tags to distinguish Rust releases from Scala releases.

This standalone repo keeps the Rust release line but drops the `rust-` prefix. The standalone baseline should match the version currently declared in `node/Cargo.toml`. On this branch that baseline is `0.4.13`, so the first standalone tag should be `v0.4.13` unless the crate version changes before tagging.

### Tag Mapping

| This repo | Legacy repo | Notes |
|-----------|-------------|-------|
| `v0.4.13` | Rust `0.4.x` line | Initial standalone baseline should match the current crate version |
| `v0.5.0+` | — | Subsequent standalone releases from this repo |

### How to Verify Continuity

```bash
# Current standalone version
grep '^version = ' node/Cargo.toml

# Latest standalone release tag, if one already exists
git tag -l 'v[0-9]*' --sort=-v:refname | head -1
```

If no standalone `v*` tag exists yet, the first one should match `node/Cargo.toml`. After that, nightly releases continue from the latest standalone tag.

## Release Process

### Automated (Nightly)

The `nightly-release.yml` workflow runs daily at 08:00 UTC (midnight Pacific):

1. If no standalone `v*` tag exists, creates a baseline tag matching `node/Cargo.toml`
2. Otherwise checks if `master` has commits since the last `v*` tag
3. If changes exist: bumps the minor version
4. Updates `node/Cargo.toml` and the Dockerfile version label
5. Generates `CHANGELOG.md` via [git-cliff](https://git-cliff.org/)
6. Commits, tags `vX.Y.Z`, pushes
7. Tag push triggers CI: Docker build, integration tests, Docker Hub release

### Manual

For major or patch bumps:

```bash
./scripts/release.sh major   # 0.4.13 -> 1.0.0
./scripts/release.sh minor   # 0.4.13 -> 0.5.0
./scripts/release.sh patch   # 0.4.13 -> 0.4.14
```

## Docker Image Tags

| Tag | Source |
|-----|--------|
| `f1r3flyindustries/f1r3node-rust:latest` | Latest master release |
| `f1r3flyindustries/f1r3node-rust:v0.4.13` | Specific version (from `git describe`) |
| `f1r3flyindustries/f1r3node-rust:v0.4.13-amd64` | Architecture-specific |

## Changelog

Generated automatically by git-cliff from conventional commits. See `CHANGELOG.md`.
