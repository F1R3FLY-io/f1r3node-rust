# TASK-017-12 Preparation

---
handoff_status: paused
next_steps:
  - A Linux agent qualifies the live adapters for claims 002 to 004 against a real node, using the checklist below.
  - The maintainer decides the resource proposal below.
  - The user decides whether to run the infrastructure dry run below. It spends OCI runner time.
  - At dispatch, rerun the resolution procedure against the dev commit current at that time and update the matrix.
---

## Scope

This log records preparation for TASK-017-12. It covers the repin procedure, a resource proposal, an adapter-qualification checklist, and an infrastructure dry-run plan.

Workload pins, adapter qualification runs, and the baseline soak wait for TASK-017-8 through TASK-017-11 and for a Linux agent. The [drift review](./task-017-12-drift-review-2026-09-19.md) is the input to this preparation.

## Candidate identity resolution

CI publishes each `dev` push to Docker Hub under two tag forms. The `:dev` tag is a mutable pointer. The `dev-<git describe>` tag is immutable and names the commit.

A candidate must cite the immutable tag. The procedure is a script that takes the dev commit and the harness revision and returns one matrix entry per platform.

1. Find the immutable tag on Docker Hub whose name ends in the commit's short hash.
2. Read the tag's manifest list and take each platform's manifest digest.
3. Pull each platform image by digest and read its config digest.
4. Extract `/opt/docker/bin/node` from each image without running it and hash the binary.
5. Emit the entries with `workload_configuration_status` blocked until profile requests exist.

The script lives in the session scratchpad as `resolve-dev-candidate.sh`. The executing agent adopts it at dispatch. It is not a mandatory artifact and has no ledger record.

### Dry run at dev `6940a5beb` (2026-09-19)

| Field | dev-amd64 | dev-arm64 |
| --- | --- | --- |
| Image tag | `f1r3flyindustries/f1r3fly-rust:dev-v0.4.46-canary.1758-183-g6940a5be` | same |
| Platform manifest digest | `sha256:607751b4db7ecf3e1e3354b2163bf84b5cfb918ad1a780539b374f3ada368663` | `sha256:4f944006f0f5288bc10fe0a2ee1bea7b60728982c2a2ca4278d5aa43d12f743c` |
| Image config digest | `sha256:bd1d291e690f256a022bb7c684cd7cb5fbdc17056ec1fe4cce60bdb606b18cc2` | `sha256:6af530f8c3b0291dce60003f4dae04cf67295aca8036f113f6668fdb5ea6ed73` |
| Node binary digest | `sha256:5e3c3b3181fb24e04ee1e1da183c4bf03069bbca31e7ad9e4bbda7e3215863af` | `sha256:b078605600cbfda69922cbf32adb60512d93c9917c07fa56669b7ba03d4936f3` |
| Node binary bytes | 76,260,512 | 74,792,168 |

CI run 35423287293 built this commit and succeeded. The matrix is not updated by this dry run. The values will be stale by dispatch and are recorded to prove the procedure.

The matrix's existing entries also cite a CI artifact identity record. The executing agent decides whether to keep that field or to rely on the registry digests above.

## Resource proposal for maintainer approval

| Item | Proposal | Basis |
| --- | --- | --- |
| Candidates | dev-amd64 and dev-arm64, repinned at dispatch | Matrix authority is current-dev. |
| First run | `preflight_only` dispatch of the soak workflow | Proves the repaired driver and the harness build on the OCI runner before any node runs. |
| Baseline duration | `daily-24h` per candidate | The workflow's dev integration soak. The 60-hour stability soak waits for a passing baseline. |
| Repetitions | One per candidate for the baseline | A second repetition only if the first reports a non-passing outcome. |
| Memory | Fleet default 48 GB, harness RSS ceiling from the driver's host-reserve rule | The driver refuses a ceiling under 5,000 MB. |
| Quota | Two runner VMs for up to 26 hours each | OCI daily VM quota applies. A LimitExceeded result means retry the next day, not debug. |

Deferred policy findings return to the team. No comparative or alternate-policy run is part of the baseline.

## Adapter qualification checklist

The three accepted profiles bind synthetic transcripts. Their live adapters are unqualified. Qualification means a real node interface produces the record the profile guide specifies, with the identity and ordering guarantees the guide names.

| Profile | Adapters to qualify | What the guide requires |
| --- | --- | --- |
| Authority and finality (002) | same-DAG, electorate, fault-tolerance | Paired input identity, majority and threshold boundaries, finality decisions with correlated heads. |
| Publication and restart (003) | publication boundary, exit, restart, atomic snapshot, durable work | Observed cut points, linked restart receipts, whole tuples, an atomic flag that the node interface supports. |
| Recovery and custody (004) | source occurrence identity, pause receipt, delivery receipt | Occurrence identity preserved, not replaced by position, signature, or execution order. Pause and delivery receipts from observed states. |

Each adapter needs a qualification record that names the node revision, the interface, the fixture, and the observed result. A qualification record for one node revision does not transfer to another.

This work needs Linux, a node build, and the harness. It belongs to a Linux agent after its profile work, or to a third agent.

## Infrastructure dry run

The merge-recovery soak workflow has never run on this branch. The repaired driver and the harness build step are exercised on pull requests only through the bindings gate.

A manual dispatch with `preflight_only=true` runs the full integration preflight and stops. It launches an OCI runner and spends quota. It needs the user's decision.

```
gh workflow run merge-recovery-soak.yml --ref formal/soak-casper-consensus -f target_ref=formal/soak-casper-consensus -f preflight_only=true
```

## Status

- Repin procedure: written and proven on the current dev commit.
- Resource proposal: drafted, awaiting maintainer decision.
- Adapter checklist: written, awaiting a Linux agent.
- Dry run: planned, awaiting the user's decision.
