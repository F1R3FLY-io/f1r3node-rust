# Cold-Store Compression Model

This model supports the pending [compression claim](../../../docs/claims/rspace-cold-store-compression.md).
It checks the storage adapter, not consensus or the LZ4 algorithm.
The [construction proof](../../rocq/cold_store_compression/README.md) covers related abstract properties without a trace-length bound.

## State and Scope

The model has one logical key, two canonical values, two physical formats, and three workers.
A row can be absent, valid, or corrupt.
An arbitrary write can replace a row between a deletion snapshot and its conditional commit.
Each worker reserves one to three accounting units against a total limit of four and an active-worker limit of two.

`Read` records whether corrupt data could be accepted or compressed bytes could escape through an export.
It does not model error strings, parsing, digest computation, or hash collisions.
For compressed rows, `valid` abstracts the production length, format, digest, and leaf-key checks.
For raw rows, it abstracts the envelope and key-encoding checks only.
After the 2026-11-27 correction, ordinary raw reads trust stored payloads without recomputing their hash.
The model does not establish detection of structurally valid raw payload corruption.
`FinishDelete` abstracts one atomic backend compare-and-delete attempt.
The model does not establish scheduler fairness, retry termination, decoder progress, or process memory bounds.

## Implementation Correspondence

| Model action or invariant | Production behavior | Executable binding |
| --- | --- | --- |
| `Write`, `Read`, `CanonicalExport` | Encode physical rows; return canonical values through reads and iteration | `formal_store_model_agrees_with_operation_traces`; checkpoint and export integration tests |
| `CorruptionRefused` | Reject malformed raw envelopes and compressed records that fail full validation | Malformed-frame and exact-length codec regressions |
| `BeginDelete`, `FinishDelete`, `ReplacementPreserved` | Decode a snapshot; compare-and-delete that exact physical row; retry replacement races | Deterministic representation-replacement and different-value races |
| `Acquire`, `Release`, `BudgetBounded`, `BudgetAccounted` | Shared mutex-protected reservations and permit release | `formal_budget_model_agrees_with_reservation_traces`; concurrent-budget tests |

The binding tests call the [production adapter](../../../rspace++/src/rspace/history/cold_store_codec.rs).
Their independent logical-store and reservation models compare state after each generated operation.
They are finite tests, not a mechanically checked refinement from Rust to TLA+.
Source manifests identify the exact checked files; hashes alone do not establish correspondence.

## Configurations

| Configuration suffix | Expected result | Deliberate defect |
| --- | --- | --- |
| No suffix | All invariants pass | None |
| `integrity_unsafe` | `CorruptionRefused` fails | Accept a corrupt read |
| `export_unsafe` | `CanonicalExport` fails | Return physical compressed bytes |
| `delete_unsafe` | `ReplacementPreserved` fails | Delete without comparing the physical snapshot |
| `budget_unsafe` | `BudgetBounded` fails | Admit work without checking the budget |
| `release_unsafe` | `BudgetAccounted` fails | Release a worker without returning its reservation |

Every negative control must return TLC exit code 12 with the named invariant and a counterexample.
An execution error or timeout is not a successful control.
The positive model and controls are registered in the existing CI invariant gate.

## Reproduction

Run the [focused evidence script](../../../scripts/ci/check-cold-store-compression.sh) with TLC 1.7.4 and Coq 8.16.1 available:

```bash
TLA_TOOLS_JAR=/absolute/path/tla2tools.jar \
  scripts/ci/run-tests.sh bash scripts/ci/check-cold-store-compression.sh
```

The script checks the pinned TLC checksum, six model outcomes, the Coq kernel results, and theorem assumptions.
It writes logs and a source manifest below `target/cold-store-validation/formal`.
Rust binding tests must also pass; the formal script does not substitute for them.
The [validation record](../../../docs/cbc-evidence/cold-store-compression-2026-11-26.md) records the executed checks and remaining limits.
