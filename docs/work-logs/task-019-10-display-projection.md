# TASK-019-10: Detached display projection

```yaml
handoff_status: ready
claimed_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
claimed_at: 2026-09-30T17:45:58Z
implementation_baseline: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
claim_id: CLAIM-CASPER-NODE-OBSERVATION-005
claim_status: pending
implementation_checkpoint: c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03
working_tree_artifacts: 7
verification_status: recorded_with_declared_construction_gaps
acceptance_status: pending
verification_report: docs/cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json
source_records: 31
late_source_registrations: 12
scope_files: 72
```

## Authorization and handoff

The user approved the 72-file scope and continuation on 2026-09-30.
The [plan](../plans/casper-node-observation-batch-e.md) records all sixteen decisions and the five ratified tags.
Agent B verified the seven SHA-256 digests in Agent A's committed step-13 handoff.
The handoff commit is `3ab092cc58fb30f4da39e6c8b28b8d25206c661b`.
The working files also matched those seven digests before Batch E changes.

Agent A committed its step-15 refresh at `28606f1103343a6d4e1e425a7d9a96c93459c57a`.
Commit `2d4af9134db60ff1393442e844e313be9e907813` records the separate Claim 004 acceptance.
Sixteen Batch E successor records preserve the exact committed `previous_record` references.
The Batch E scope changes no binding-driver code.
Historical acceptance does not cover changed source bytes.

## Registration

The initial slice registered Claim 005 as pending before the first code change.
The five ratified mandatory tags were applied.
The dispatch and block API files received pending records before their source changes.
The initial-fault file received its pending record when its source first existed.

The initial current-source refresh updated nineteen pending records and created twelve missing records.
All thirty-one records bind the source bytes at the implementation checkpoint.
The refresh preserves registration digests and adds immutable references to the nineteen prior records.
All sixteen original `previous_record` references passed independent hash checks.

The twelve missing records follow their source implementation commits.
Each new record names this registration gap explicitly.
The refresh does not establish registration before source implementation.
Claim 005 remains pending.
The final report records verification observations without named acceptance.

## Work sequence

- [x] Verify the committed step-13 handoff and source scope.
- [x] Register the pending claim, tags, and two new production-file records.
- [x] Extract production arithmetic and pass the first isolated duplicate-record fixture.
- [x] Complete arithmetic boundary and differential tests.
- [x] Capture bounded tracker rows in the existing transaction interval.
- [x] Add the optional display request and detached calculation.
- [x] Preserve legacy and Batch D request digests and tests.
- [x] Add the bounded model, controls, integer proofs, and bindings.
- [x] Complete coordinated source-specific records and compact evidence.
- [x] Request named maintainer acceptance.
- [ ] Receive named maintainer acceptance.


## Final verification cycle on 2026-10-01

The checkout remains on `feature/casper-node-observation`.
The final source checkpoint is `c83b16f30b28fd9ef3fb7dcdccaa1ff4876cbd03`, with the seven modified artifact hashes recorded separately.
Other participants committed the initial record refresh and cleanup.
Agent B made no Git mutation.

The display helper now charges spare hash-table capacity and control storage before allocation.
The helper checks capture generation and transaction identities before calculation.
Both repairs have retained RED controls with exit 100.

The first isolated node run exposed an incorrect test expectation.
Malformed requests close their connection.
Invalid numeric limits instead return typed unavailable responses.
The test now checks that existing protocol behavior without changing the admission path.
The failed run remains in `linux-current-08/`.

| Check | Passed | Exclusions |
| --- | ---: | --- |
| Release capture | 35 | 0 |
| Release observer | 53 | 0 |
| Shared arithmetic | 3 | 378 filtered |
| Batch D unit filter | 57 | 324 filtered |
| Batch D integration filter | 66 | 910 filtered |
| Isolated node library | 259 | 0 |
| Isolated node integration | 22 | 1 ignored subprocess helper |

These checks cover 495 distinct selected tests.
The three arithmetic tests also passed in isolated debug and optimized builds.
Those six repeat invocations add no distinct tests.
The first image entrypoint refused a read-only Rustup setup, and that failed attempt remains recorded.

The exact Linux binding driver passed at the final source archive.
It checked input and executable hashes before and after execution.
The node integration executable passed debug-strip equivalence.
The isolated strip fixture refused twelve executable mutations.

`Rocq 9.1.1` passed the kernel checks and all forty-seven assumption sets.
The NodeAuthority project contains twenty-two closed sets, including seven display results.
The bounded TLA+ gate passed seventeen positive configurations and eighty-six exact negative controls.
Both isolated gate-fixture runs passed.
The direct display model passed with thirty-three distinct states.
Its three controls returned exit 12 with the expected invariant and a trace.

All seventeen properties map to twenty-six executed named tests.
The map is not a machine-checked refinement.
The final dependency manifest checks 2,162 tracked input files.
All thirty-one Batch E source records now bind the final artifact and claim hashes.
All sixteen original prior-record identities and nineteen prior-refresh identities remain unchanged.
Each record adds an immutable reference to its committed predecessor at the final checkpoint.

The twelve late registrations remain explicit gaps.

The compact [report](../cbc-evidence/runs/casper-node-display-projection-batch-e-01/report.json) records commands, hashes, tool identities, failed attempts, and limits.
The [validation](../cbc-evidence/runs/casper-node-display-projection-batch-e-01/validation.json) records independent checks.
Bulk logs remain under `target/node-observation-prep-20260928-01/batch-e/`.
The report SHA-256 is `52f766491135dcf59b72d935e8ef697251fba1cebbe2a5d3f11ae5d47f87393a`.

PR #447 uses base `aa5722e3ecf9f1b79cc3dc28cab92772fd60e109`.
Its current mandatory inventory contains 106 sources.
The Batch E refresh covers its approved thirty-one records, not the other seventy-five sources.
This record does not establish complete stack qualification.

The strict CbC gate returned exit 4 for thirty-one pending records.
That result is expected before named acceptance.
The independent validation checks source hashes, which the status gate does not check.

Rustfmt and strict all-target Clippy passed.
LSP reported no diagnostics, but only one file had confirmed clean coverage.
Three files remained inconclusive.
The earlier observer leak was not reproduced, and no repair is claimed.

## Maintainer acceptance request

The proposed reviewer is `jltatbeach`.
Review Claim 005 against the report hash and the final working-tree source hashes.
Provide named acceptance of E1, E14, and E17 as bounded by design.
Review the stated construction gaps for E7, E11, and E15.
Acknowledge the twelve late source registrations and the trusted-tool assumptions.

Acceptance must identify the maintainer, Claim 005, and the report hash.
Acceptance must cover the recorded working-tree hashes, not only the checkpoint commit.
Claim 005 remains pending, and TASK-019-10 remains in progress until that acceptance.
No publication, live qualification, deployment, or Git permission follows from this request.

## Initial arithmetic cycle (historical)

The new `initial_fault.rs` contains shared weight lookup, integer sums, normalization, and final subtraction.
The production dispatch calls the weight helpers and normalization.
The public block API calls the subtraction helper.
The tracker lock interval, per-record terms, ordinary sums, zero-total result, and `f32` operation order remain unchanged.
The detached display caller is not implemented yet.

The first test uses two records for one weighted validator and one record outside the weight map.
The RED result was exit 101, with `(0, 0)` instead of `(6, 8)`.
The GREEN test passed in both debug and optimized modes in an isolated Linux container.
The fixture also checks the exact normalization and subtraction bits.
This is one unique test with two passing invocations, not complete arithmetic or integration coverage.

The container image is `sha256:fc6351ab4210530ff9fe6768c81c17855cabeff6a69555107a6c413d2f1cd484`.
It ran without network access, with read-only source, dropped capabilities, and a bounded executable temporary directory.
The tool reported Rust 1.98.0, LLVM 22.1.8, and `aarch64-unknown-linux-gnu`.
The raw RED source, logs, and exits remain under `target/node-observation-prep-20260928-01/batch-e/`.

Direct `cargo clippy --locked --offline -p casper --all-targets -- -D warnings` passed after production wiring.
Rustfmt, whitespace checks, and fresh LSP checks passed.
The cached file-only adapter reported unused helpers despite their production callers.
Those findings were marked false-positive without ignore comments.
The direct Clippy result does not establish a passing adapter result.

All seven handed-over source files remain unchanged in this first slice.
The corrected scope check confirmed 72 unique approved paths and twelve touched Batch E paths.
An initial check included the separate coordination log, although this slice did not edit that log.
The corrected check excludes that untouched path and passes.

No full Batch D regression or formal gate result is claimed by this arithmetic cycle.
The first expanded log failed the paragraph limit, and the paragraph break corrected it.
The first ownership check could not load the unavailable Python YAML module.
The later ownership check compares the scoped task text directly.

## Verification boundaries

The parent limit in `CasperShardConf::new()` is zero and remains unchanged.
Node observer tests need `objcopy --strip-debug` before execution, as the handoff specifies.
Verifier-process fixtures must run in isolated Linux containers.
The handoff's exact nextest filters define the Batch D regression sets.
Rustfmt must pass before each separately authorized commit.

The new tracker encoding and floating-point construction remain explicit proof gaps.
Logical observation limits do not establish physical storage or live safety.
Final cleanup remains on stack branch 4.
No staging, commit, push, branch switch, merge, or removal occurred through Agent B.
