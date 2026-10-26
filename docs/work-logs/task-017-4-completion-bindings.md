---
task: TASK-017-4
claimed_by: pi-casper-harness
handoff_status: in_progress
construction: not-applicable
---

# TASK-017-4 Remaining Bindings

The user requested all six remaining items. The starting revision is `c17733c53cf76709a3ae4760ac43a320ddc54222`.

## Scope

The production Bash driver retains its resource protection and legacy load path. New profile execution must use qualified, pinned inputs.

Missing profile modules or required capabilities prevent launch. Fixture evidence cannot authorize node execution or satisfy a product verdict.

No node campaign, external repin, Git publication, or claim waiver is authorized.

## Work Plan

1. Validate executable bytes, effective configuration, source files, approval records, and capability qualification before profile admission.
2. Preserve iteration identities and captured artifacts across segments. Refuse inconsistent checkpoints and directory reuse.
3. Add profile dispatch and common record validation. Correlate observations, fault receipts, and restart identities.
4. Bind lifecycle controls to the actual driver. Retain each invocation and its input, output, expected result, and exit status.
5. Check capture completeness and reconstruct publication verdicts from retained evidence. Preserve product failures independently of termination.
6. Integrate verification and claim checks into workflows. Check completion-helper support without changing unrelated task states.

## Design Constraints

The manifest remains an exact-byte identity record. A separate, digest-pinned approval record supplies the admission authority.

Profile implementations export the three specified functions. A dedicated lifecycle fixture adapter exercises the shared boundary without claiming seven-profile coverage.

An executor is a separately pinned process boundary. The driver must not infer executor support from generic node query primitives.

Capture writes append-only iteration records. Publication verifies their hashes and required coverage rather than trusting a cached success field.

The shared claim includes later tasks. Task-specific binding evidence must not discharge future soak or post-merge obligations.

## Progress

The [Rust and Bash migration](./task-017-4-rust-bash-migration.md) replaces the newly added Python code. It records 88 driver invocations and fresh bounded-model results.

Admission, history, lifecycle dispatch, evidence reconstruction, and workflow fixtures have partial implementation coverage. Full claim gating and completion-helper support remain unresolved.

The seven node profiles remain unimplemented. The candidate matrix remains non-dispatchable, and TASK-017-4 remains open.
