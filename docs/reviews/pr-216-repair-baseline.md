# PR 216 repair baseline

## Purpose

This record freezes the branch, pull request, CI, and repair state before further changes.

The record separates confirmed causes from downstream symptoms and unverified repair candidates.

## Git baseline

| Item | Value | Meaning |
| --- | --- | --- |
| Local branch | `feature/cost-accounted-rho` | Active repair branch |
| Local and remote branch head | `3980ed402b4b3248f065d1f07daedbd3dc8b4493` | Published branch baseline |
| Last merged `dev` head | `231067178af39f481ed62d678070b74b11b10c3a` | Current branch merge base |
| Current remote `dev` head | `375933475456407cdb47c455a9146f9aa93579c6` | Required next integration base |
| GitHub merge-test head | `025f5e0ec54defee22fb782082597c876bac7108` | Synthetic merge used by current CI |
| Branch relation to current `dev` | 397 commits ahead, 6 commits behind | The histories have diverged |
| Pull request state | Open draft, mergeable, behind | The branch is not release-ready |

GitHub built the merge-test head from remote `dev` first and the feature branch second.

The local `dev` worktree is stale and cannot define the next merge target.

## Working-tree baseline

The working tree contains 70 changed paths. No path is staged.

| Surface | Path count | Current purpose |
| --- | ---: | --- |
| Casper implementation and tests | 9 | Signed-floor, proposal, validation, and replay repairs |
| Formal models | 42 | Committee, proposal-readiness, replay-anchor, and concurrency proofs |
| Documentation | 11 | Protocol, glossary, threat, verification, and review records |
| Models and protobuf | 3 | Fail-closed wire decoding and admission manifest generation |
| Node | 1 | Heartbeat proposal behavior |
| Verification scripts | 3 | Finalized-floor and TLA+ gates |
| Dependency lock | 1 | Current dependency resolution |

These changes are uncommitted and unverified as one set.

## Current CI failure

The current CI run tested the synthetic merge head, not the feature head alone.

Six required jobs failed before their intended tests ran.

| Job class | Direct failure | Root cause |
| --- | --- | --- |
| Lint | Rust type error `E0308` | Two stale Rholang call sites pass `i8` to `split_byte(u8)` |
| Example tests | Compilation stopped | Same type error |
| Property tests | Compilation stopped | Same type error |
| Loom tests | Compilation stopped | Same type error |
| Search-horizon tests | Compilation stopped | Same type error |
| Regression backstop 8 | Compilation stopped | Same type error |

Remote `dev` commit `cdb72085e` changed `split_byte` from `i8` to `u8`.

The change repaired the valid index range `128..=255` without changing its byte encoding.

The feature branch still casts two indices through `i8` in `rholang/src/rust/interpreter/util/mod.rs`.

The synthetic merge preserved both incompatible changes. Rust then rejected the merged source.

This failure is an integration regression. It is not a Casper consensus failure.

The current `dev` merge must preserve the unsigned interface and remove both stale casts.

## Previous CI failures

Earlier CI runs reached tests before the latest `dev` interface change.

| Failure | Classification | Confirmed cause or next proof obligation |
| --- | --- | --- |
| DAG insertion rejected uncertified normal blocks | Invalid fixture | New admission rules require certified sender authority |
| Concurrent registry test rejected an empty deploy identity | Invalid fixture | The fixture created a protocol-v6 block without its canonical 32-byte identity |
| Legacy floor-cache request expected a response | Stale expectation | Certified recovery rejects unauthenticated floor-cache authority |
| Heartbeat and asymmetric recovery timed out | Consensus liveness defect | Proposal readiness depended on an uncertified derived floor |
| Nodes reported negative fault tolerance after recovery | Committee-state defect | Recovery mixed certified and derived active-validator views |
| Nodes recorded `UnauthorizedSlashDeploy` | Committee-state symptom | Slash authorization used a committee view inconsistent with the certified replay anchor |
| Recovered validators reported `UnknownRootError` | Replay-state defect | Replay selected state outside the durable certified-floor availability contract |
| Deploys remained pending | Downstream symptom | Invalid-block loops and stalled finality prevented inclusion or finalization |
| Query endpoints lost Casper availability | Downstream symptom | Node recovery or resource protection removed the active Casper instance |
| Node resident memory exceeded the ceiling | Resource-lifetime defect | Repeated replay, invalid-block work, and retained runtime state exceeded bounded ownership |

The signed-floor repair remains a proof blocker until multi-parent preservation has executable evidence.

The performance repair must bound work and lifetime without serializing validators.

## External-review root causes

The [external review adjudication](pr-216-external-review-adjudication.md) records additional confirmed defects.

Those defects include restore retry ownership, quarantine lifetime, ticket atomicity, and validator-fuel custody.

The adjudication also records mint atomicity, epoch retention, work bounds, and verification-gate gaps.

These defects can survive after the compilation failure is fixed. Each defect has a separate pgmcp task.

## Changed protocol surfaces

| Surface | Risk | Required decision source |
| --- | --- | --- |
| Admission authority | Invalid blocks can enter storage | Certified admission invariant |
| Finalized-floor selection | Validators can replay from different anchors | Signed-certificate contract |
| Active validator committee | Votes and slashes can use different authority | Certified-floor committee state |
| Checkpoint recertification | Removed deploys can retain stale certification | Immutable snapshot and recertification rule |
| Replay state availability | Recovery can request unknown roots | Durable floor and horizon contract |
| Vault custody roles | Fees can consume transferable or stakeable funds | Cost-accounting papers and DR-27 through DR-38 |
| Retry and quarantine ownership | Recovery can stall or forget evidence | Bounded lifecycle state machine |
| Runtime and queue lifetime | A live shard can exhaust memory | Deterministic work and ownership bounds |

## Merge impact

The current remote `dev` integration is now an explicit pgmcp task.

The task follows this baseline and precedes every remaining implementation task.

Current `dev` changes two files after the merge base. Only `rholang/tests/reduce_spec.rs` has a textual conflict.

The merge must use current remote `dev`, not the stale local `dev` worktree.

Each conflict needs a semantic decision. Cost-accounting conflicts must preserve this branch's proved model.

Unrelated `dev` fixes must remain. Overlapping Casper fixes must preserve the strongest proved invariant.

No merge commit is authorized by this record. The repository policy requires a new user instruction.

## Reproduction references

- Current CI run: <https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/33819245443>
- Current formal-gate run: <https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/33819245183>
- Previous full CI run: <https://github.com/F1R3FLY-io/f1r3node-rust/actions/runs/33691050185>
- Pull request: <https://github.com/F1R3FLY-io/f1r3node-rust/pull/216>

## Completion condition

This baseline remains valid until the next remote `dev` fetch or branch commit.

After either event, update the commit table and rerun the scheduler.
