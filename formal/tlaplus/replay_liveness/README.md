# Replay Hot-Loop Model

This model checks work growth for persistent-contract replay when the channel store is empty.

| Model element | Rust element |
| --- | --- |
| `processed` | Replayed persistent fires |
| `commClones` | Recorded `COMM` clones |
| `matcherRuns` | Candidate matcher calls |
| `IndexedReplay` | Empty-store replay short circuit |

`MC_ReplayHotLoop.cfg` must pass the linear-work invariants and completion property.

`MC_ReplayHotLoop_quadratic_pre_fix.cfg` must violate `Inv_LinearCloneWork`.

This model does not discharge indexed candidate selection equivalence when store data exists.
