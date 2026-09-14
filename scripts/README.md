# Scripts

Helper scripts intended to be run from the repository root.

## Available Scripts

| Script | Purpose |
| --- | --- |
| `scripts/run_rust_tests.sh` | Runs the release test suite crate by crate |
| `scripts/delete_data.sh` | Deletes `.log` and `.mdb` files under `docker/` |
| `scripts/setup-hooks.sh` | Installs the pre-commit and pre-push hooks |

## Formal Verification Scripts

These scripts need the tools in the [Formal Verification Tooling](../README.md#formal-verification-tooling) section of the repository README.

| Script | Purpose |
| --- | --- |
| `scripts/ci/check-formal-invariants.sh` | Runs the TLA+ and Rocq gates that scheduled CI runs. Use `--tla`, `--rocq`, or `--exhaustive`. |
| `scripts/ci/check-tla-invariants.sh` | Runs every TLA+ configuration in its gating list. The formal-invariants script calls it. |
| `scripts/ci/dump-tla-traces.sh` | Prints the counterexample traces from a TLC run |
| `scripts/ci/slashing-search-horizon.sh` | Runs the slashing fuzz tiers |
| `scripts/check-deploy-lifecycle-ALL.sh` | Runs the complete evidence set for the deploy-lifecycle area |
| `scripts/check-finalized-floor-ALL.sh` | Runs the complete evidence set for the finalized-floor area |
| `scripts/check-fork-choice-ALL.sh` | Runs the complete evidence set for the fork-choice area |
| `scripts/check-merge-algebra-ALL.sh` | Runs the complete evidence set for the merge-algebra area |
| `scripts/check-rspace-guards-ALL.sh` | Runs the complete evidence set for the RSpace guards area |
| `scripts/check-runtime-isolation-ALL.sh` | Runs the complete evidence set for the runtime-isolation area |
| `scripts/check-slashing-ALL.sh` | Runs the complete evidence set for the slashing area |

Each `check-*-ALL.sh` script runs the Rocq proofs, the TLA+ configurations, and the Rust tests for one theory area. Run one from the repository root. See [docs/formal-verification.md](../docs/formal-verification.md) for the method behind each area.

## Verification Storage

The storage tools need Linux, Python 3, Git, and standard command-line tools.
They do not change the mandatory verification gates or acceptance settings.
The tools do not discharge correctness claims.

### Source snapshots

Capture selected source files instead of copying a working directory recursively:

```bash
python3 -I scripts/verification-storage.py snapshot "$PWD" \
  "$HOME/soak-evidence/f1r3node-rust/source-store-v1" \
  scripts formal/tlaplus
```

The tool captures working-tree bytes for selected Git-tracked files, including staged additions.
Use `--extra-file PATH` for each required untracked file.
Do not select secret-bearing paths.
Capture external dependencies separately at their required revisions.
A file selection does not establish dependency completeness.

The tool excludes these generated paths:

- Any `target`, `node_modules`, `__pycache__`, `.venv`, or `.git` path component.
- Files with the `.pyc` suffix.
- State directories under `formal/tlaplus/*/states`.

The receipt lists excluded tracked paths.
The tool refuses symbolic links, Git links, unresolved Git stages, and source changes during capture.
The source store must be outside the repository.
Every requested selection must match an input.
Reuse also checks the complete directory set and read-only directory permissions.

The default limits are 64 MiB per snapshot, 20,000 files, and a 1 GiB source-store admission budget.
The tool also requires 10 GiB of free space before capture.
Use the corresponding `--max-*` and `--min-free-bytes` options to select other positive limits.
An exceeded limit causes failure, not a verification pass.

The snapshot identifier binds file paths, file bytes, and permission modes.
Identical inputs reuse one verified snapshot.
The stored source files and source directories are read-only.
Keep each attempt receipt, result, command, tool identity, and counterexample separately from the shared snapshot.
Do not modify an existing snapshot to prepare another candidate.

These controls are not filesystem quotas or hostile-filesystem isolation.
The capture does not preserve ownership, extended attributes, or a filesystem image.
A successful capture does not establish power-loss durability or archive retention.

### Evidence workflow

Keep the capture receipt beside the result of each attempt.
Pass the receipt's snapshot directory to the test fixture instead of copying an earlier candidate.
The following example uses the public-driver telemetry fixture.
`PINNED_HARNESS` must identify a verified harness tree at the required revision.
The harness tree must contain `integration-tests/test`.
The example requires the existing integration-test Python environment.

```bash
EVIDENCE_ROOT="$HOME/soak-evidence/f1r3node-rust"
mkdir -p "$EVIDENCE_ROOT"
ATTEMPT=$(mktemp -d "$EVIDENCE_ROOT/telemetry-XXXXXXXX")
python3 -I scripts/verification-storage.py snapshot "$PWD" \
  "$EVIDENCE_ROOT/source-store-v1" scripts formal/tlaplus \
  > "$ATTEMPT/source.json"
SOURCE=$(python3 -I -c \
  'import json,sys; print(json.load(open(sys.argv[1]))["snapshot"] + "/source")' \
  "$ATTEMPT/source.json")
../system-integration/.venv/bin/python -I \
  "$SOURCE/scripts/bench/test-soak-telemetry.py" "$SOURCE" \
  "${PINNED_HARNESS:?Set a verified harness directory}" \
  "$ATTEMPT/summary" summary
```

Use a shell with `set -euo pipefail` for this procedure.
Keep the command output and exit status with the receipt.
The fixture substitutes external workload boundaries and does not establish production Docker containment.

### Local Cargo builds

Use the optional local profile for focused development checks:

```bash
bash scripts/cargo-low-disk.sh check -p graphz
bash scripts/cargo-low-disk.sh test -p graphz
```

The wrapper disables incremental compilation and keeps line-table debug information.
The default cache is `${XDG_CACHE_HOME:-$HOME/.cache}/f1r3node/low-disk`.
`CBC_CARGO_CACHE` selects another absolute cache path outside the working directory.
`CBC_CACHE_LIMIT_GIB` sets the admission limit, which defaults to 64 GiB.
`CBC_MIN_FREE_GIB` sets the free-space threshold, which defaults to 10 GiB.

The wrapper refuses recognized continuous-integration and coverage environments, release builds, and profile overrides.
It leaves workspace profiles and normal acceptance commands unchanged.
The admission check does not limit growth during a build.
A filesystem quota requires separate configuration and validation.
Do not use local-profile results as acceptance evidence.

### Storage audits and retention

Inspect exact storage roots without deleting files:

```bash
python3 -I scripts/verification-storage.py audit target/soak-evidence
```

Audit only directories that still exist.
Do not add overlapping audit rows or count shared files twice.
Keep build caches, temporary verification files, and retained evidence in separate directories.
Keep required executed binaries with the evidence, not only in an expendable cache.
Run focused checks during development, then run all required integration gates against the final source.
Use private TLC state directories for each invocation.

Before removing an extraction, verify its bytes against the retained archive:

```bash
python3 -I scripts/verification-storage.py verify-zip \
  /retained/archive.zip /retained/extracted --sha256 EXPECTED_SHA256
```

The command checks the archive digest, paths, directory sets, regular-file sets, and file bytes.
It rejects duplicate archive paths, symbolic links, special files, and declared expansion limits that exceed its budget.
It does not delete files or authorize deletion.
The original archive must remain retrievable after any approved removal.
Preserve original manifests and record the archive location, digest, removed paths, and retrieval method.
Restore archived inputs before a replay that requires their original paths.

Do not run blanket `cargo clean` in this checkout.
Historical evidence remains under `target/soak-evidence`.
Before cache removal, check active writers and acquire the applicable Cargo build lock.
Do not remove evidence-bound source copies without an audited archive and complete source lineage.

Run the storage regression suite with this command:

```bash
python3 -I scripts/test-verification-storage.py
```

The suite uses disposable Git indexes and substitutes only the external Cargo command for wrapper tests.
It does not compile the node or run an acceptance soak.

## Usage

Examples:

```bash
./scripts/run_rust_tests.sh
./scripts/delete_data.sh
bash scripts/ci/check-formal-invariants.sh --all
./scripts/check-slashing-ALL.sh
```
