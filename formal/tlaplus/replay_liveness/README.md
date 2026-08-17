# Replay Hot-Loop Model

This model checks work growth for persistent-contract replay when the channel store is empty.

| Model element | Rust element |
| --- | --- |
| `processed` | Replayed persistent fires |
| `commClones` | Recorded `COMM` clones |
| `matcherRuns` | Candidate matcher calls |
| `IndexedReplay` | Empty-store replay short circuit |
| `liveTasks` | Recursive singleton evaluator tasks retained until completion |
| `schedulerYields` | Cooperative yields from the inline singleton evaluator |
| `InlineSingleton` | Singleton evaluation without a new Tokio task |
| `YieldInterval` | Maximum singleton evaluations between cooperative yields |

`MC_ReplayHotLoop.cfg` must pass the linear-work invariants and completion property.

`MC_ReplayHotLoop_quadratic_pre_fix.cfg` must violate `Inv_LinearCloneWork`.

`MC_ReplayHotLoop_task_chain_pre_fix.cfg` must violate `Inv_NoRecursiveTaskChain`.

This model does not discharge indexed candidate selection equivalence when store data exists.
