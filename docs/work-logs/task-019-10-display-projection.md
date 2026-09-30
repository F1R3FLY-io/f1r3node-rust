# TASK-019-10: Detached display projection

```yaml
handoff_status: ready
claimed_by: pi-session-01a0ab62-71b3-7248-a800-37a6fde2e4fa
claimed_at: 2026-09-30T17:45:58Z
implementation_baseline: 3ab092cc58fb30f4da39e6c8b28b8d25206c661b
claim_id: CLAIM-CASPER-NODE-OBSERVATION-005
claim_status: pending
scope_files: 72
```

## Authorization and handoff

The user approved the 72-file scope and continuation on 2026-09-30.
The [plan](../plans/casper-node-observation-batch-e.md) records all sixteen decisions and the five ratified tags.
Agent B verified the seven SHA-256 digests in Agent A's committed step-13 handoff.
The handoff commit is `3ab092cc58fb30f4da39e6c8b28b8d25206c661b`.
The working files also matched those seven digests before Batch E changes.

Agent A retains Batch D verification, evidence, and acceptance work.
Its 28 registration records remain untouched until its step-15 refresh.
Batch E successor records must preserve the committed Batch D record through `previous_record`.
The binding-driver record also requires Agent A's refresh first.
The Batch E scope changes no binding-driver code.

## Registration

Claim 005 was registered as pending before the first code change.
The five ratified mandatory tags were applied.
The dispatch and block API files received new pending source-bound records before their source changes.

The new initial-fault file received its pending record when its source first existed.
All three new records bind the current source and claim bytes, and retain the registration digests.
The sixteen shared existing records await the committed step-15 refresh.
No claim was discharged or accepted.

## Work sequence

- [x] Verify the committed step-13 handoff and source scope.
- [x] Register the pending claim, tags, and two new production-file records.
- [x] Extract production arithmetic and pass the first isolated duplicate-record fixture.
- [ ] Complete arithmetic boundary and differential tests.
- [ ] Capture bounded tracker rows in the existing transaction interval.
- [ ] Add the optional display request and detached calculation.
- [ ] Preserve legacy and Batch D request digests and tests.
- [ ] Add the bounded model, controls, integer proofs, and bindings.
- [ ] Complete coordinated source-specific records and compact evidence.
- [ ] Request named maintainer acceptance.

## Initial arithmetic cycle

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
