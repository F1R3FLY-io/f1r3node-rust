# Bounded node observation verification

These models cover selected safety properties of the Batch A interface and Batch B1 capture.
They do not prove the full Rust implementation or qualify a live node.

## Bounds and assumptions

`ObserverSession` explores two sessions, two logical time units, and normalized frame sizes at the empty, maximum, and excessive boundaries.
Each accepted connection consumes the session budget, including connections that close without a response.
The session number represents a fresh challenge. UUID collision resistance remains a trusted assumption.

Boolean inputs represent the peer and request identity checks. The model does not verify cryptographic functions or Linux credentials.
The deadline invariant constrains response admission. Runtime scheduling and native operation latency remain outside the model.

`BoundedCapture` explores two environments, two committed writes per environment, one generation change, and three ordered guards.
The model samples transaction identity before open, at open, and after reads for each environment.
External writers can advance an environment between these operations. The model does not make separate publication writes atomic.
Generation changes represent injected interference, including changes that normal guard ownership would prevent.

The lock deadline can expire before or between acquisitions. A contended acquisition cannot succeed after expiry.
An uncontended acquisition can succeed after expiry, consistent with the lock implementation.
The model does not require a guard acquired before expiry to disappear when the deadline expires.

Limit validation precedes guard access. Backend validation precedes transaction creation.
Copied bytes use normalized sizes of one, two, and three, with a limit of two.
Incomplete capture cannot produce an accepted result. Earlier copied rows can exist before rejection.
LMDB transaction semantics, native allocation, decoding, and operating-system behavior remain trusted or require executable tests.

Both models permit stuttering and include no fairness assumptions. They establish bounded safety results, not eventual completion or universal timing guarantees.

## Source correspondence

| Model operations and invariants | Rust implementation |
| --- | --- |
| `Begin`, `SessionBudget`, `OneRequest` | `Observer::run` and its connection loop in `node/src/rust/soak_observer.rs`. |
| `FreshChallenge`, `BoundIdentity` | The hello record, peer checks, and request validation in the observer session. |
| `BoundFrame`, `BoundDeadline` | Framed reads and writes inside the complete-session timeout. |
| `ValidAdmission`, `BoundLockWait`, `GuardOrder` | `CaptureLimits::validate`, `lock_deadline`, `soak_capture_access`, and the metadata state copy. |
| `OpenIdentity`, `ValidatedIdentity` | `BoundedLmdbReader::open` and `BoundedLmdbReader::validate` in `shared/src/rust/store/soak_snapshot.rs`. |
| `BoundBytes`, `CompleteRows` | Reader budgets, preflight decoding, and row checks in `capture_observed`. |
| `GenerationStable` | The generation comparison after reader validation in `capture_observed`. |
| `Detached`, `ReadOnly` | Reader consumption, guard release, and detached sealing in `capture_observed`. |

The [binding inventory](bindings.json) maps all 23 required properties to named executable tests.
Each entry identifies the supporting model invariants or states that only Rust tests provide evidence.
This inventory is a source review aid, not a machine-checked refinement proof.
Hash checks establish which bytes were tested. Hash checks alone cannot establish semantic correspondence.

Filesystem permissions, startup configuration, cleanup, canonical encoding, and scratch independence require Rust tests and source review.
The shutdown regression checks source ordering. It does not execute a complete production node shutdown.
The open-identity race has a model counterexample and source inspection, but no deterministic executable race injection.
The generation test directly injects a change after validation. Disabling the rejection branch must make that test fail.

## Reproduction

1. Run `cargo test --locked --manifest-path scripts/node-observation/Cargo.toml` from the repository root.
2. Run `cargo run --locked --manifest-path scripts/node-observation/Cargo.toml -- models --jar /path/to/tla2tools.jar --output /path/to/new-output`.
3. Run `bash scripts/ci/test-check-tla-invariants.sh` with Bash 4 or later.

The model runner requires two successful positive checks and 16 exact negative counterexamples.
A negative check must exit 12, name its expected invariant, and include a transition trace.
Timeouts, parser errors, unrelated violations, and incomplete output fail verification.
The CI gate includes these configurations in both the bounded and full tiers.

The acceptance package must identify the isolated source inputs, test executables, execution logs, and property bindings.
Named maintainer acceptance remains necessary after all technical checks pass.

The Rust tool provides `models` and `bindings` commands. Both commands share the same model result validation.
The tool has a separate Cargo manifest and lockfile. Its dependencies do not change the node workspace lockfile.
