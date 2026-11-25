# Soak Cleanup Verification

## Objective

Prevent a scheduled soak from using cleanup code that lacks current verification and independent maintainer acceptance.
This design implements [US-014](../UserStories.md#us-014-verified-soak-segment-cleanup) and [EPIC-030](../ToDos.md#epic-030-verified-soak-segment-cleanup).

## Boundary

The control checkout supplies the driver, host controls, segment action, verification tools, and evidence.
The node candidate has a separate revision and cannot supply control approval.
The existing Casper harness claim remains separate from the focused cleanup claim.
The new claim states bounded safety under its process visibility and scheduling assumptions.
It does not establish node correctness or complete a live soak.

## Design

Nested Git attributes declare mandatory coverage, required acceptance, and the evidence directory.
Claim inventories and ledgers declare claim IDs and verification status.
Attributes do not duplicate mutable verification results.

The dev base includes the CbC migration from PR #700, merge commit dc285a9958d5ceed69a87cfe37ab86bb71f862b9.
Repository readers support named claims before cleanup records use that format.
Owner files retain existing policy and combine the cleanup requirements with the migrated evidence routes.
Promote dev to master before the next weekend soak.
Recheck admission against the promoted control sources before launch.

Repository checks use Bash.

The shared CbC runtime is an external tool dependency.
A lockfile pins its immutable upstream revision and source hashes.
The Bash installer fetches only the required runtime files into an external cache.
The installer checks every file hash before it publishes the cache entry.
The Bash launcher checks those hashes before each runtime invocation.
Missing or modified runtime files prevent execution.

CI installs the runtime before verification and admission checks.
Local commands use the same installer and verified cache.
The launcher preserves this repository as the working directory.
It forwards the shared command interfaces without duplicating ledger parsing or signature validation.
Its record command forwards JSON to the shared runtime's existing ledger command.
Verification records the validated runtime revision and source hashes.

The launcher, installer, lockfile, and repository checks require CbC verification and independent signed acceptance.
Runtime source remains outside Git tracking and carries no repository attribute tags.
The language policy permits this external tool dependency and adds no exception for copied Python source.

The verification runner records test results against source hashes taken before execution.
A changed source invalidates the recorded result.
The report also binds the Rust harness sources, Cargo inputs, and toolchain configuration.

A bounded model separates child-process state from detached writer state and segment ownership.
A Linux fixture runs two segments and checks detached writer termination, unrelated process survival, and restart state.
Negative controls remove cleanup calls, ignore stop failures, and overwrite retained reasons.
Each control must fail at its expected assertion.

A scoped gate validates the cleanup claim and independent signed acceptance for every covered artifact.
Existing campaign and harness claims retain their records and independent gate status.
Cleanup admission does not discharge those other claims.

CI runs the gate without an optional strictness switch.
The soak workflow runs the same gate against ci-control before building or launching the workload.
The segment action repeats admission before each driver invocation.
A node candidate revision never substitutes for the control revision.

## Trust and acceptance

The operator supplies an allowed-signers file outside the worktree through local Git configuration.
Hosted jobs obtain its public contents from the operator-managed CBC_ACCEPTANCE_ALLOWED_SIGNERS variable.
GitHub withholds this variable from fork pull requests. Those CI runs use the public snapshot in
`.github/cbc-allowed-signers`, checked against its SHA-256 digest in the CI workflow.
The snapshot contains the same approved signers. A signer rotation must update the variable,
the snapshot, and the workflow digest together. The workflow change requires CI maintainer review.
Other CI events and production soaks still require the operator-managed variable.
No private key enters CI.
Agents do not enroll keys or sign production approvals.
Missing configuration and unsigned evidence fail closed.
Tests use disposable keys inside isolated fixture repositories only.

Source, claim, and evidence changes require new acceptance.
The implementation prepares unsigned payloads after verification.
The maintainer inspects and signs those exact payloads independently.
Git history retains earlier committed ledgers.
The review package excludes uncommitted ledger drafts.

## Verification

The acceptance report records the bounded model, Linux fixture, negative controls, and admission tests.
Gate tests cover stale control sources, distinct candidate sources, missing approval, invalid signatures, and changed evidence.
The workflow checks require admission before the driver runs.
The task remains in progress until independent acceptance is current.

## Upstream implementation

```yaml cross-reference
reference:
  purpose: evidence
  repository: https://gitlab.com/smart-assets.io/gitlab-profile
  checkout: ../../../../SA/top-level-gitlab-profile
  revision: 04a8badb8399ea7d54c477b153fa957161743072
  path: AItools/scripts/cbc.sh
  url: https://gitlab.com/smart-assets.io/gitlab-profile/-/blob/04a8badb8399ea7d54c477b153fa957161743072/AItools/scripts/cbc.sh
```

## Operator procedure

1. Install the pinned runtime with `bash scripts/ci/install-cbc-runtime.sh`.
2. Configure the external allowed-signers file through `cbc.acceptanceAllowedSigners`.
3. Record a package with `bash scripts/ci/verify-soak-cleanup.sh --record --output docs/casper/cbc-evidence/runs/<run>`.
   The command writes the unsigned approvals to a temporary directory outside the worktree and prints its path.
   Use `--payloads <directory>` to select an empty directory outside the worktree.
   On a macOS host, follow [Record a package](#record-a-package).
4. Review the verification report and its source hashes.
5. Build the signing kit with `bash scripts/ci/soak-cleanup-kit.sh --payloads <directory> --pull-request <number> --signer <principal> --kit <kit>`.
   The kit must be outside the worktree.
   The script sets the same maintainer identity, timestamp, and review reference in each approval.
6. The maintainer signs each approval in the kit in the `cbc-acceptance` namespace.
7. Import the kit with `bash scripts/ci/soak-cleanup-import.sh --kit <kit> --payloads <directory>`.
8. Set `CBC_ACCEPTANCE_ALLOWED_SIGNERS` to the same approved public signers in the repository configuration.
9. Run `scripts/ci/check-soak-cleanup.sh` before launch.

The agent prepares the package and the kit only.
The maintainer keeps the private key and signs each approval.
The shared skill requires independent maintainer acceptance.
Existing claims can remain pending even when the focused cleanup claim passes.
The full CbC discharge gate continues to report those independent gaps.

The import script checks the complete kit before it imports an approval:

- Each unsigned payload has one approval, and the kit has no other approval.
- The approvals cover each manifest artifact exactly once.
- Each approval still binds the current sources, evidence, and claim set.
- Each approval differs from its payload only in the maintainer identity, timestamp, and review reference.
- All approvals have the same maintainer identity, timestamp, and review reference.
- Each signature verifies for its principal through the allowed-signers file.

After the import, the script runs `scripts/ci/check-soak-cleanup.sh`.
The scoped admission gate passes only when every cleanup approval is current.

The unsigned payloads and the kit are temporary files.
The package keeps the report, the status, the model, the regressions, and the check logs.
Each accepted envelope keeps the signed payload and its signature.

### Record a package

Two checks need Linux. On a macOS host, run them in a container.
Start from a clean worktree, because the report records the control revision.

1. Build the Linux check image one time:

   ```bash
   docker build -t soak-shell-test:local - <<'DOCKERFILE'
   FROM debian:bookworm
   RUN apt-get update && apt-get install -y --no-install-recommends jq procps git perl ca-certificates && rm -rf /var/lib/apt/lists/*
   DOCKERFILE
   ```

2. Build the Linux harness:

   ```bash
   docker run --rm -v "$PWD:/src" -v "$HOME/.cargo/registry:/usr/local/cargo/registry" -w /src \
     -e CARGO_TARGET_DIR=/src/target/linux-soak rust:bookworm \
     cargo build --locked -p casper-soak --bin casper-soak
   ```

3. Get the TLA+ tools that CI uses, and check the hash:

   ```bash
   curl --fail --location --create-dirs --output "$HOME/.tla/tla2tools.jar" \
     https://github.com/tlaplus/tlaplus/releases/download/v1.7.4/tla2tools.jar
   echo "936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88  $HOME/.tla/tla2tools.jar" | shasum -a 256 --check
   ```

4. Record the package:

   ```bash
   TLA2TOOLS_JAR="$HOME/.tla/tla2tools.jar" bash scripts/ci/verify-soak-cleanup.sh --record \
     --linux-image soak-shell-test:local \
     --harness-binary "$PWD/target/linux-soak/debug/casper-soak" \
     --output docs/casper/cbc-evidence/runs/<run>
   ```

5. Keep the check logs. The repository ignores `*.log` files, so add a `.gitignore` file to the run directory:

   ```bash
   printf '!check-*.log\n' > docs/casper/cbc-evidence/runs/<run>/.gitignore
   ```

Name each run `soak-cleanup-<YYYYMMDD>-<NN>`.
