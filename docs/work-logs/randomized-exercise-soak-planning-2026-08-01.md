# Work Log: Randomized Exercise Soak Planning

## Session

- **Started:** 2026-08-01T16:55:13Z
- **Branch:** `feature/randomized-exercise-soak`
- **Parallel repository:** `F1R3FLY-io/system-integration`
- **Parallel branch:** `feature/randomized-exercise-soak-catalogue`
- **Status:** planning artifacts ready for review

## Decisions

- Exercise epochs are bounded executions of versioned valid-operation workloads.
- Workload IDs use `SOAK-EPOCH-NNN`, separate from planning EPIC/EPOCH IDs.
- Exact identity is the epoch ID/revision plus catalog schema, definition SHA/digest, orchestrator SHA, seed, provider, topology, and effective limits.
- Initial delivery uses the existing six-node single shard and both providers.
- The first six profiles cover steady traffic, bursts, channel contention, large valid deploys, dependent chains, and mixed contracts.
- Selection is seeded, weighted, and coverage-constrained rather than purely random.
- Multiple epochs may run per segment, but admission must preserve checkpoint and reset reserves.
- Ordinary workload failures preserve evidence, reset, and continue. Safety, host-protection, or reset failures stop immediately.
- Newly discovered valid-workload regressions are minimized and added as experimental epochs before promotion.
- Infrastructure failures remain outside the workload catalog.
- Executable catalog ownership belongs to system-integration; orchestration and reporting belong here.

## Artifacts

- `docs/randomized-exercise-soak.md`
- `docs/UserStories.md`: US-005 through US-008
- `docs/ToDos.md`: EPIC-011 through EPIC-016
- `docs/ci-pins.md`
- `../system-integration/docs/specs/randomized-exercise-soak-contract.md`
- `../system-integration/docs/handoffs/agent-session-f1r3node-rust--pi-session-019fa4ad--20260801T165513Z.md`

## Cross-Repository Handoff

The system-integration handoff requests an executor/catalog contract, deterministic valid workload generators, provider parity, finalized-state invariants, clean reset, structured results, and replay manifests. Interface changes are complete only when mirrored contract fixtures pass in both repositories. The follow-up pin design splits privileged `runnerRef` from fast-moving `catalogRef` in `.github/ci-pins.jsonc`; compatible catalog additions should require only a one-line catalog pin bump.

## Next Work

1. Ratify EPIC-011's identity and schema.
2. Receive the executor agent's proposed CLI and result schema.
3. Add mirrored compatibility fixtures.
4. Validate the pinned catalog before OCI launch.
5. Implement the deterministic planner after duration and capability metadata stabilize.

## Blockers

- Executor CLI and result schema are awaiting the parallel system-integration agent.
- Epoch duration estimates require implementation evidence before minimum weekend coverage can become release-gating.
