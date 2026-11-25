# Reuse Compiled Validated Deploy Terms

Status: draft proposal. Reuse rate, cache identity, and retained-memory cost remain to be validated.
Date: 2026-11-25.

## Objective

Reuse successful compilation of the same validated deploy across admission and execution.
Preserve normalizer bindings, input validation, execution charging, and independent execution state.
The [performance acceptance conditions](performance-overview.md#shared-acceptance-conditions) apply.

## Current Path

[Deploy admission](../../casper/src/rust/engine/multi_parent_casper/block_admission.rs) compiles a deploy and discards `_parsed_term` after validation.
The [interpreter](../../rholang/src/rust/interpreter/interpreter.rs) compiles again in `inj_attempt`.
The compiler performs parsing, normalization, and term checks before returning a `Par`.
Other execution or replay attempts can repeat that work.

This observation does not establish a high cache hit rate.
Short-lived runtimes, foreign deploys, and unique traffic can reduce reuse.
The same source text is not necessarily the same compiled term.

The [normalizer environment](../../models/src/rust/normalizer_env.rs) includes the deploy signature, deployer public key, and validated parameters.
It also provides legacy aliases for deploy and deployer identifiers.
Different signatures with identical source text can therefore produce different normalized terms.
A source-text-only cache is unsafe for the final normalized `Par`.

## Phase 0: Measure Exact Reuse

Measure compile count, source bytes, term size, and time per execution phase.
Count reuse for exact validated compile inputs with a bounded experimental recorder.
Do not retain an unbounded set of signatures merely to estimate cache hits.
Keep identities out of exported metrics and logs.

Separate local admission-to-proposal reuse from replay reuse and repeated source with different signatures.
Measure the existing normalized-term build phase before adding another timer.
Record AST clone cost because the reducer currently consumes a term by value.

## Cache Identity

Restrict the first cache to validated user-deploy compilation.
Do not reuse artifacts for arbitrary interpreter calls or system queries with different environments.
Keep all existing admission and parameter checks before reuse.

Define a typed identity that covers every compiler input:

- Exact source bytes, before any compiler preprocessing.
- Validated deploy signature and deployer public key.
- Validated named parameters, their types, values, and defined ordering semantics.
- Compiler and normalization context identity, including relevant options and URI bindings.

Prefer exact structural equality for the identity after hashing.
Do not trust an unverified signature field as the sole key.
Do not serialize an unordered environment map to obtain a supposedly stable key.
Do not use a shortened hash without full identity validation.

The first version may conservatively distinguish inputs that compile to equal terms.
That loses a possible hit but avoids merging different environments.
An audit must identify any compiler input not represented above before implementation proceeds.
Process-local entries disappear on restart, so binaries do not exchange compiled artifacts.

## Ownership and Integration

Use one explicitly owned, bounded cache accessible to admission and execution.
A cache owned only by a newly spawned runtime would lose the intended cross-phase reuse.
Store only successful immutable compilation artifacts.
Do not cache runtime effects, random generators, charge results, or mutable reducer state.

Share the cached AST through `Arc` while it remains immutable.
Clone at the existing by-value reducer boundary if that boundary requires ownership.
Measure this clone rather than claiming allocation-free execution.
Changing reducer ownership is a separate proposal.

Limit both entry count and accounted bytes, including keys and nested AST allocations.
Count shared allocations once and distinguish retained entries from caller-held references.
Skip caching oversized artifacts without rejecting otherwise valid deploys.
Set cache limits from the baseline and memory budget, not from a desired hit rate.

Compile outside cache locks.
Concurrent identical misses may initially compile independently under existing admission limits.
Add shared in-flight work only if bounded waiters, cancellation, and failure release are separately designed.
Do not add an unbounded pending-compilation map or bypass existing execution limits.

## Charging and Failure Semantics

Initialize the execution budget and charge parsing cost before the cache lookup at the existing execution boundary.
A cache hit must not lower the deploy's phlo charge.
Insufficient phlo must return the same result even when a compiled artifact exists.
Admission compilation remains distinct from execution charging.

Use the original compiler path on a miss, disabled cache, or unavailable optional cache entry.
Do not turn compilation errors into successful empty terms.
Do not cache failed compilations in the initial design.
Preserve the current order of input, parameter, budget, and compiler errors.

Reset execution-local tracking and construct random state on every execution as before.
Cache reuse does not prove a deploy already executed or belongs on the canonical branch.
It must not bypass merge recovery or duplicate-deploy checks.

## Regression Plan

| Case | Required assertion |
| --- | --- |
| Same source, different signatures | Correct distinct deploy-ID bindings |
| Same source, different deployers or parameters | Correct distinct normalized bindings and results |
| Canonical and legacy identifier URIs | Same alias behavior with warm and cold caches |
| Parameter-order variations and invalid parameters | Existing ordering and validation semantics remain exact |
| Forged or unvalidated identity fields | No admission bypass or artifact substitution |
| Cached term with insufficient phlo | Same charge failure and no execution |
| Parser errors and compiler limits | Same rejection and error behavior as the uncached path |
| Eviction, concurrent misses, and cancellation | No stale mutable state, unbounded waiters, or leaked reservations |
| Multi-validator execution with different cache states | Identical roots, events, charges, and verdicts |

Compare repeated execution of one deploy with repeated source submitted as distinct deploys.
Do not write a test that accidentally treats those cases as equivalent.

## Benchmark and Decision

Measure compile time saved, identity construction time, lookup time, AST copy time, and retained bytes.
Use unique terms, repeated exact deploys, and repeated source with different signatures.
Include small terms and large continuation-producing terms.
Measure admission, proposal execution, and replay separately.

Keep the cache disabled by default during evaluation.
Proceed only if exact reuse produces a material CPU saving within an explicit retained-memory budget.
Defer the cache if AST copying or identity construction consumes the expected benefit.
Disabling it restores compilation without changing disk state or consensus behavior.
