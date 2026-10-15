# TASK-017-12 Candidate Drift Review (2026-09-19)

---
handoff_status: ready
next_steps:
  - Repin the candidates to the dev revision current at dispatch, from the CI-built dev images, not a master canary.
  - Update the matrix harness_revision to the branch tip at dispatch.
  - Ask the maintainers about the five stale mandatory records that dev carries.
---

## Scope

This review compares the candidate matrix pins with the current `dev` branch. It does not qualify a candidate, pin a workload, or authorize dispatch.

TASK-017-12 stays blocked on TASK-017-8 through TASK-017-11. This review prepares its qualification step.

## Pins in the matrix

| Field | Matrix value | Current value | Drift |
| --- | --- | --- | --- |
| Node revision (both candidates) | `a2fe60c72` | `dev` at `6940a5beb` | 120 commits, 9 merged pull requests |
| Harness revision | `f29c59d01` | branch tip `09b0a6006` | 171 commits |
| System-integration pin | `b3d14b27e` | `b3d14b27e` in `.github/oci-validation.env` | none |
| Workload configuration | null, blocked | null, blocked | none, profiles 005 to 008 not yet implemented |

The matrix records `authority: current-dev` and `matrix_status: not-dispatchable`. A repin at dispatch is the design, not a defect. The review question is what changed and which profile assumptions it touches.

## Node changes since the pin

Nine pull requests merged into `dev` after `a2fe60c72`. Four change consensus or node code.

| Pull request | Change | Decision area | Affected profile |
| --- | --- | --- | --- |
| #435 | Fork-choice scoring from the finalized floor, floor-error abstention, a bound on the fault-tolerance threshold, arrival-depth telemetry, and bootstrap node-id parsing. | D-02, D-03 | Authority and finality (TASK-017-5) |
| #438 | Merge rejection groups resolve independently. Merge branches share through Arc. | D-08 | Merge and accounting (TASK-017-8) |
| #444 | Certificate helper keeps the leading zero byte of a 64-byte coordinate pair. | none | TLS identity only |
| #390 | Cost-accounting specification and decision ledger. | D-12 | Protocol and Phlo (TASK-017-11) |

The other five pull requests change documentation, the soak driver, the soak disk models, and the network guide. The driver change is already bound on this branch.

Node configuration also changed: `node/src/main/resources/defaults.conf`, the configuration module, the node environment, the runtime, and the web routes. Workload pins must be generated against the defaults current at dispatch.

## Images

CI publishes a node image on each `dev` push to Docker Hub and OCIR. The tag is `dev-<version>` with a `:dev` channel pointer.

The latest canary, `v0.4.46-canary.1910`, is a `master` build. It is not a `current-dev` candidate.

At dispatch, resolve the per-platform manifest digests of the `dev-<version>` image that CI built for the chosen revision. Record them in the matrix as the existing candidates do.

## Ledger debt that a dev candidate carries

Five mandatory records on `dev` no longer match their sources. They cover the two merging files after #438, the finality deploy lifecycle, the heartbeat proposer, and the certificate helper after #444. This branch inherits them and did not cause them.

A soak candidate built from `dev` carries that debt. The maintainers should decide whether qualification requires those records to be current.

## Dispatch blockers, current state

| Blocker in the matrix | State on 2026-09-19 |
| --- | --- |
| Executable profile workload configurations remain unpinned. | Open. Four profiles remain unimplemented. |
| Profile adapters and required capabilities remain unqualified. | Open. Live adapters are unqualified for claims 002 to 004. |
| Full harness verification remains pending. | Partly closed. Claims 001 to 004 are discharged for bounded scope. |
| Campaign resource limits require approval. | Open. Maintainer decision. |
| B44 containment remains open. | Open. |

## Recommendation for qualification

1. Repin both candidates to the `dev` revision current at dispatch, from the CI-built dev images.
2. Update `harness_revision` in the matrix to the branch tip at dispatch.
3. Regenerate workload pins against the current node defaults after the four remaining profiles land.
4. Review each merged pull request against its decision area before dispatch, using the table above as the starting list.
5. Record the maintainers' decision on the inherited ledger debt in the matrix.
