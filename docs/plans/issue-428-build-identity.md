---
issue: 428
date: 2026-10-21
status: implemented
language: en
---

# Issue #428: Node Build Identity

The node must identify its source build through its version output.
[Issue #428](https://github.com/F1R3FLY-io/f1r3node-rust/issues/428) reports empty parentheses in `/api/status` on development images.
The image publication job knows the build identity, but the binary does not receive it.

## Baseline

The investigation used `dev` at `98eb647e6cfd864e4765f22e1fe97d84cac713fe`.
GitHub reported `master` at `66258ba258728f02552bd4e6643bfb183321aa1e` during the investigation.
The two branches contained identical versions of `node/build.rs`, `node/Dockerfile`, and `version_info.rs`.

[PR #743](https://github.com/F1R3FLY-io/f1r3node-rust/pull/743) proposes status and versioning documentation.
That draft contains no implementation of this fix.

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
2. Otherwise, run `git describe --tags --always` in the source checkout.
3. If Git fails or returns no valid identity, use `unknown`.

Reject an explicit identity that contains control characters.
Accept Git output only after a successful exit and valid UTF-8 decoding.
Reject empty Git output and embedded control characters.

The build script exports the selected value through `cargo:rustc-env=F1R3NODE_BUILD_ID`.
The [version module](../../node/src/rust/web/version_info.rs) reads that value at compile time.
HTTP status, `/version`, gRPC status, and the startup log retain their common version function.

Cargo tracks the input environment variable and the actual Git metadata paths.
The paths include `HEAD`, `refs`, and `packed-refs` when those paths exist.
This replaces the old `.git/HEAD` path relative to the `node` package.

The Docker builder declares `ARG VERSION` immediately before the compilation layer.
That layer passes the value to Cargo as `F1R3NODE_BUILD_ID`.
A changed build argument invalidates the compilation layer without copying `.git` into the image.

The main workflow passes `build_base.outputs.VERSION` to the reusable pipeline through its optional `build_id` input.
Other callers calculate the fallback from a complete checkout of their selected source.
CI rejects an empty identity, `unknown`, whitespace, or control characters before building the image.

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
| amd64 and arm64 | Both images report the expected source identity. |
| HTTP, gRPC, and startup log | Each output matches the expected complete version string. |
| Publication | The embedded identity uses the same source value as the immutable image tag. |
| Fork or explicit target | The identity describes the selected source checkout. |

## Validation

The following checks passed:

- Seven Rust regression tests in [build_identity.rs](../../node/tests/build_identity.rs), compiled directly with `rustc --test`.
- Clippy for the new identity module and regression tests, with `-D warnings`.
- Rust formatting checks for the changed Rust files.
- [Workflow behavior tests](../../.github/scripts/test-build-identity-workflow.sh) for selected identities, invalid inputs, and inconsistent node output.
- `.github/scripts/check-workflow-invariants.sh`.
- `.github/scripts/check-dockerfile-workspace.sh`.
- `.github/scripts/test-image-publish-tags.sh`.
- Actionlint `1.7.12` for both changed workflows, with external ShellCheck and pyflakes disabled.
- `git diff --check` and relative document link checks.

An isolated Cargo package compiled copies of the actual build script, identity module, and version module.
Repeated builds with the same target directory changed the output from identity A to identity B.
Without an explicit identity or Git repository, the package returned `F1r3node Rust 0.4.46 (unknown)`.

Full node compilation, the Docker smoke test, and both architecture builds remain pending.

## Scope

The fix adds no dependencies and changes no consensus rules, shard state, or API schema.
It preserves publication and promotion of tested binaries.
It also preserves the literal Docker version label used by the [promotion script](../../.github/scripts/promote-release.sh) and [release script](../../scripts/release.sh).

The draft pull request targets `dev` and uses `Refs #428`.
Close the issue after promotion to `master` and confirmation on a published image.

## Build References

- The [Cargo documentation](https://doc.rust-lang.org/cargo/reference/build-scripts.html) defines build script inputs, output directives, and change detection.
- The [Docker documentation](https://docs.docker.com/build/building/variables/) defines build argument scope and use in build commands.
