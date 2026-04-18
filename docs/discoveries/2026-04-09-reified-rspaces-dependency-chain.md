---
doc_type: discovery
discovered_by: claude-session
relevance: [EPOCH-001, EPOCH-002]
---

## Finding

The Reified RSpaces PR chain (#328-#338) has a multi-layer dependency that must be verified before merging can begin:

```
rholang-rs#83 (external parser changes)
    |
    v
new_parser branch (f1r3node)
    |
    v
#328 Reified RSpaces [1/11] (base: new_parser)
    |
    v
#329-#338 (chained sequentially)
```

- PR #328 targets `new_parser` branch, NOT `rust/dev` directly
- `new_parser` depends on `rholang-rs#83` (external crate, referenced by git rev in Cargo.toml)
- If `new_parser` is not yet merged into `rust/dev`, it must be merged first
- If `rholang-rs#83` is not merged/tagged, the entire chain is blocked

## Implications

- EPOCH-001 TASK-001-1 must verify both `new_parser` status and `rholang-rs#83` before any merge work begins
- The agent in f1r3node should check: `git branch -a | grep new_parser` and review `rholang-rs` PR #83 status
- If `rholang-rs#83` is unmerged, this becomes the true critical path — everything else waits
- The `rholang-parser` git rev in `Cargo.toml` will need updating to include #83's changes
