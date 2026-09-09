---
doc_type: work_log
task: pr216-buffer-pruning-upstream-repair
status: in_progress
date: 2026-09-08
---

# Pruning repair: current upstream comparison

## User decision

The user authorizes repair if current upstream `dev` has no fix.
If upstream has a fix, adopt that solution instead.
The user also permits an upstream fetch and identifies the sibling `f1r3node-rust-dev` worktree.

This decision permits repair of the demonstrated defect.
It does not establish upstream Casper team approval for other architecture changes.
The repair must preserve consensus validity, voting, finality, fork choice, and wire semantics.

## Updated local references

The fetch updated the local `origin/dev` reference without changing the feature worktree or the sibling worktree.
The command did not write `FETCH_HEAD` or fetch tags.

```sh
git -c core.fsmonitor=false fetch --no-write-fetch-head --no-tags origin refs/heads/dev:refs/remotes/origin/dev
```

| Reference | Commit | Observation |
|---|---|---|
| `origin/dev` | `cdf447ac18710d9702a27379bce6c946f421be46` | Current fetched upstream reference. |
| Sibling `dev` worktree | `62fa58f1183630d08919e4957d29018ecd1b3bcb` | Clean worktree, 921 commits behind `origin/dev`. |

The worktrees share the Git object database.
Use `origin/dev` for current source comparisons.
Do not treat the older sibling checkout as current upstream.

## Defect status

The current upstream files retain the exact Git blobs used by the controlled upstream reproduction.

| File | Git blob |
|---|---|
| `block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs` | `1c15061f86aaecf2515ac96cb28c552830263bfd` |
| `block-storage/src/rust/util/doubly_linked_dag_operations.rs` | `545855f7db425f8de56461f7f3934bb7364ec2bc` |

The demonstrated pruning defect therefore remains in these upstream modules.
No upstream replacement for that defective implementation is available at this reference.
This comparison does not claim a fresh full-dev build, network reproduction, or LMDB crash test.
The [original evidence packet](../casper/theory/finalized-floor/buffer-pruning-preservation.md) states the reproduction boundaries.

## Plan-agent result

The authorized plan agent recommends retaining `parents-map` as the authoritative durable schema.
Its proposed repair separates resident eviction from terminal dependency resolution.
Persistent retry tickets are not established as necessary.

The proposal requires these connected changes:

1. Retain the demonstrated pruning regressions and add a failed-first-publication regression.
2. Evict resident entries without deleting durable rows or unresolved dependency edges.
3. Replace complete resident indexes and eager restart loading with byte-budgeted caches.
4. Make cold lookups fallible and use bounded scans for reverse dependency queries.
5. Preserve finite retry rounds and the existing two-phase startup selection contract.
6. Publish complete dependency rows atomically before acknowledging durable retry ownership.
7. Preserve terminal cleanup semantics and qualify backend transaction resources.

These steps form one repair boundary.
Removing only the destructive call does not establish a complete memory-bound repair.
The existing publication error path also needs its own executable regression before correction.

## Review limits

Captured scans need a concrete resource-lifetime design.
A live cursor does not preserve all guarantees of an immutable captured candidate set.
The agent proposes bounded external sorting and disk-backed captures as one implementation option.
That option remains subject to the existing architecture review boundary.
This work log does not approve or implement it.

The current dependency-row encoding can require allocation proportional to the largest row.
Terminal cleanup can affect an arbitrary number of child rows.
Streaming Rust iteration alone does not bound LMDB transaction resources.
The repair must state and verify these separate limits.

## Verification obligations

The existing model abstracts publication and resolution as atomic operations.
That abstraction does not prove the current sequence of individual dependency writes correct.
The refined model must expose publication failure, acknowledgement, concurrent mutation, cold reads, capture, cancellation, and restart.

Property tests must check each model invariant against implementation behavior.
Concurrency tests must cover the actual publication and cache ownership code.
LMDB failure and reopen tests must check persistence separately from thread interleavings.

This update changes no production source, test fixture, consensus rule, or durable encoding.
No build or formal-verification process ran for this upstream reference check.
No temporary files were created under `/tmp`.
