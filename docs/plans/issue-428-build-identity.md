---
issue: 428
date: 2026-10-22
status: implemented
language: en
---

# Issue #428: Node Build Identity

The node must report its [build identity](../Glossary.md#build-identity) through its version output.
[Issue #428](https://github.com/F1R3FLY-io/f1r3node-rust/issues/428) reports empty parentheses in `/api/status` on development images.
The image publication job knows the build identity, but the binary does not receive it.

## Cause

The [Dockerfile](../../node/Dockerfile) copies source directories without `.git`.
Its original `ARG VERSION` declaration belongs to the final stage, after compilation.
The builder therefore receives no source identity through that argument.

The original [build script](../../node/build.rs) calls `git rev-parse --short=7 HEAD` without checking the exit status.
Without a repository, Git returns exit code `128` and empty standard output.
The script accepts that empty output, so its `unknown` fallback does not run.

An isolated Rust program reproduced `F1r3node Rust 0.4.46 ()` with the original selection code.
The program returned a short hash inside the repository and `unknown` when Git was unavailable.

Continuous integration (CI) also had two independent version calculations.
[ci.yml](../../.github/workflows/ci.yml) determines the publication identity from complete history and tags.
The [Docker workflow](../../.github/workflows/_integration-pipeline.yml) originally calculated its value from a separate shallow checkout.
Those calculations could produce different identities for the same source commit.

## Design

Keep the existing string format: `F1r3node Rust <package-version> (<build-id>)`.
Keep the HTTP and protobuf schemas unchanged.

Select the identity in this order:

1. Use a nonempty `F1R3NODE_BUILD_ID` value after trimming surrounding whitespace.
2. Otherwise, run `git describe --tags --always --exclude '*/*'` in the source checkout. Tags that contain a slash are not image versions.
3. If Git fails or returns no valid identity, use `unknown`.

After trimming surrounding whitespace, reject an explicit identity that contains control characters.
The Rust selection function permits ordinary spaces inside an explicit identity.
CI applies a stricter rule and rejects all whitespace.
Accept Git output only after a successful exit and valid UTF-8 decoding.
Reject empty Git output and embedded control characters.

The build script exports the selected value through `cargo:rustc-env=F1R3NODE_BUILD_ID`.
The [version module](../../node/src/rust/web/version_info.rs) reads that value at compile time.
HTTP status, `/version`, gRPC status, and the startup log retain their common version function.

Cargo tracks the input environment variable and the actual Git metadata paths.
The paths include `HEAD`, the current branch ref, `refs/tags`, and an existing `packed-refs` file.
For a packed branch, track its nearest existing directory under `refs/heads` until Git creates the loose ref.
Remote refs do not trigger a rebuild through a recursive watch of `refs`.
Resolve relative Git paths against the package directory without `--path-format=absolute`.
An explicit identity does not depend on Git metadata.
This replaces the old `.git/HEAD` path relative to the `node` package.

The Docker builder declares `ARG VERSION` immediately before the compilation layer.
That layer passes the value to Cargo as `F1R3NODE_BUILD_ID`.
A changed build argument invalidates the compilation layer without copying `.git` into the image.
This fix accepts that compilation cost to prevent reuse of a binary with an incorrect identity.
The compilation step has no persistent Cargo target cache.
The pinned Buildx setup action creates a builder and removes its state during cleanup with the current default inputs.
A cache mount alone does not establish reuse between jobs with that lifecycle.
Any cache change needs a separate lifecycle design and a controlled build-time comparison.
No controlled comparison of documentation-only commits has been completed.

For a manual image build with a known identity, use:

```bash
docker build -f node/Dockerfile --build-arg VERSION="$(git describe --tags --always --exclude '*/*')" -t f1r3fly-rust:local .
```

A manual build without `VERSION` uses `unknown` because the build context does not include Git metadata.

The main workflow passes `build_base.outputs.VERSION` to the reusable pipeline through its optional `build_id` input.
Other callers calculate the fallback from a complete checkout of their selected source.
CI rejects an empty identity, `unknown`, whitespace, or control characters before building the image.
CI reports an explicit error if it cannot read the package version from `node/Cargo.toml`.

Each architecture job checks the identity inside its image before export.
The check copies the node binary from a stopped container and searches for the expected identity.
The check does not execute the binary.
The amd64 smoke test checks the complete version string through HTTP, gRPC, and the startup log.

The matrix exports only architecture-independent values: the source identity and the package version.
Both jobs use the same checkout and identity input, so either completed job supplies the same outputs.
Keep architecture-specific values outside these shared outputs.

Image tag prefixes and the publication conversion from `+` to `__` remain separate from the source identity.
Later canary or stable tags retain the original build identity when they reuse a tested binary.

## Acceptance Checks

| Check | Required result |
|---|---|
| Explicit identity without Git | The binary reports the supplied identity. |
| Explicit identity with Git | Git does not replace the supplied identity. |
| Missing or blank input | Selection uses Git, then `unknown` if necessary. |
| Failed Git, empty output, or invalid UTF-8 | Selection returns `unknown`. |
| Control characters | Input cannot create additional Cargo directives. |
| Repeated Cargo builds with a shared target | Changing identity A to B changes the binary output. |
| Repeated Docker builds with a shared cache | Changing `VERSION` changes the binary output. |
| Git metadata | Track the current branch and tags, including packed refs, without tracking remote refs. |
| Relative and absolute Git paths | Both path forms resolve without requiring `--path-format=absolute`. |
| amd64 and arm64 | Each image contains the expected source identity in its node binary. |
| HTTP, gRPC, and startup log on amd64 | Each output matches the expected complete version string. |
| Publication | The embedded identity uses the same source value as the immutable image tag. |
| Fork or explicit target | The identity describes the selected source checkout. |

## Scope

The fix adds no dependencies and changes no consensus rules, shard state, or API schema.
It preserves publication and promotion of tested binaries.
It also preserves the literal Docker version label used by the [promotion script](../../.github/scripts/promote-release.sh) and [release script](../../scripts/release.sh).

Close issue #428 after promotion to `master` and confirmation on a published image.

## Build References

- The [Cargo documentation](https://doc.rust-lang.org/cargo/reference/build-scripts.html) defines build script inputs, output directives, and change detection.
- The [Docker documentation](https://docs.docker.com/build/building/variables/) defines build argument scope and use in build commands.
