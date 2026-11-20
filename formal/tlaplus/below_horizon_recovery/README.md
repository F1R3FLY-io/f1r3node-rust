# Below-horizon recovery model

Protocol-level TLA+ model of the recovery flow for #1059. It discharges the correctness property the Rust fix delivers: an observer whose stored state falls below peers' block-store GC horizon eventually catches up to at or above that anchor. The model also verifies that the signal-delivery path that landed in PR #1078 still fires under both configurations.

## Model ↔ Rust mapping

| Model element | Rust element |
| --- | --- |
| `peersFloor` | Peers' latest finalized height — `BlockDagRepresentation::last_finalized_block()` depth on the responder. |
| `peersAnchor` | Peer's `effective_anchor_number()` on `MultiParentCasper` — the body prune floor; advanced by `mergeable_channels_gc::job`. |
| `observerHeight` | Observer's latest validated DAG height — `max over dag.latest_message_hashes()`. |
| `observerPhase` | `EngineCell` contents mapped to `{Running, Restarting, Initializing}`. `Restarting` is the transient between a process exit and `CasperLaunchImpl::launch()` returning. |
| `belowHorizonFlag` | `IngestView::below_horizon_floor()` returning `Some(_)`; set by `IngestState::block_pruned`. |
| `LaunchRoutesLFS` | The follow-up fix — launch-time routing to the Initializing engine (LFS state sync) when `observerHeight < peersAnchor` on resume. |
| `ObserverReceivesPrunedResponse` | `Running::handle_block_pruned_response` forwarding `IngestInput::BlockPruned` to the ingest state; landed in PR #1078. |
| `LaunchDecisionLFS` / `LaunchDecisionRunning` | The branch inside the modified `Initializing::on_approved_block` that short-circuits to Running when within window or proceeds with `request_approved_state` when below. |
| `InitializingCompletes` | `Initializing::request_approved_state` → `create_casper_and_transition_to_running`. |
| `HorizonDepth` | `max_parent_depth + mergeable_channels_gc_depth_buffer + deploy_lifespan` from `CasperShardConf` when `enable_block_store_gc` is set — see `horizon_required_height` in `casper/src/rust/engine/running.rs`. |

## Configurations

`MC_BelowHorizonRecovery.cfg` — post-fix (`LaunchRoutesLFS = TRUE`). Must pass every invariant and both liveness properties.

`MC_BelowHorizonRecovery_no_lfs_pre_fix.cfg` — pre-fix (`LaunchRoutesLFS = FALSE`). Models the state in which PR #1078 landed but the follow-up has not. Must violate `Live_ObserverCatchesUp` with an infinite trace of `ObserverRestart → LaunchDecisionRunning → ObserverReceivesPrunedResponse` that never advances `observerHeight` past `peersAnchor`. This is the pattern the soak run on `dev c06f21df1` observed: 52 `BlockPrunedResponse` log lines, 1825/10min `AreaFull` drops, and no progress after 17.5 h.

## Running locally

```
java -XX:+UseParallelGC -jar ~/.tla/tla2tools.jar \
     -workers auto \
     -config MC_BelowHorizonRecovery.cfg \
       MC_BelowHorizonRecovery.tla
```

## Scope

Does not model:

- the specific retry ladder in `fetch_scheduler` (compressed into `ObserverCatchUp`);
- block-hash identity, peer identity, or equivocation handling;
- pending-area byte / count accounting or the within-session `AreaFull` symptom (bounded transient, handled by Rust unit tests on `IngestState::block_pruned` with the dependency-cascade eviction);
- the trust gap in the horizon probe for post-genesis `ApprovedBlock` — tracked separately in #1079.
