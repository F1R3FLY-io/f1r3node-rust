# Sealed-Floor Merge v2 — Status

Branch: `sealed-floor-merge-wip` (off `feat/sealed-floor-merge-v2`). Companion to the reference-branch PR
[F1R3FLY-io/f1r3node-rust#77](https://github.com/F1R3FLY-io/f1r3node-rust/pull/77)
(`feat/floor-sealed-merge`), which this branch rebuilds cleanly per the sealed-floor design.

## Where we are

This branch rebases the multi-parent merge on a **node-deterministic finalized floor** with
**record-driven recovery**, replacing the reference branch's main_parent-tip base + gas-cell
recovery (which let finalized state regress). BASE + RECOVERY + native single-value-cell
conflict handling are landed, and the green-gate convergence proxy is green through the
under-load three-writer scenario.

The green-gate proxy (`casper/tests/batch2/map_cell_convergence_spec.rs`) drives concurrent
single-value-cell writes (the same shape as the PoS bonds map and the
`test_user_contract_concurrency` / `test_validator_lifecycle` integration tests). Its previously
known under-load failure modes are fixed.

## What we accomplished

**Foundation / BASE (§3a)**
- `finalized_floor` (`casper/src/rust/finality/floor.rs`): a per-block, **justification-derived,
  node-deterministic** finalized cut (parent-floor inheritance + per-parent advancement +
  sound-base selection), plus a node-deterministic fault-tolerance witness.
- Merge base = `floor.post_state`; scope = `closure(parents) \ closure(floor)`.

**Recovery (§3b)**
- `canonical_won_sigs` (`interpreter_util.rs`): record-driven done-detection — a deploy is
  re-proposable unless its **latest canonical disposition** (winner in `body.deploys`, loser in
  `body.rejected_deploys`) across the **full merge scope** is a WIN. Proposer (`block_creator`)
  and validator (`repeat_deploy`) gate on the **same record** → **InvalidRepeatDeploy eliminated**,
  double-apply closed. Walks all parents (not just the main-parent chain) so a deploy already won
  on a co-parent is caught.

**Merge (§3c)**
- `ChannelChange::combine` nets dependent-chain intermediates instead of orphaning added/removed
  datums onto the floor base.
- `EventLogIndex` tracks user/system event logs separately while conflict detection combines both,
  so user deploys and system deploys participate in the same deterministic conflict check.
- Event-indexed conflict detection rejects concurrent consume+produce writers of one non-foldable
  cell natively, while allowing identical emits and mergeable channels.
- A floor-availability guard rejects stale non-mergeable removals that are no longer present on the
  finalized floor base.

**Recovery (§3b completion)**
- Pending deploys are retained through block accept and removed only once finalized, preventing
  accepted-but-orphaned deploys from being lost before record-driven recovery can re-propose them.

**Determinism / robustness**
- Deterministic slash-deploy replay (block-derived invalid-blocks map).
- On-demand mergeable-entry recompute (`ensure_scope_mergeable_present`) to heal a cross-node
  merge-validity fork for LFS-imported blocks.
- LMD-GHOST main-parent selection, bonds-equality parent-filter removal, total-order rejection
  tiebreaking, fresh-joiner latest-message placeholders, poison-tolerant shared-LMDB test lock,
  and LFS requester retry/deadline hardening are ported.
- The FT threshold is sourced from genesis PoS state and the node config is overridden from the
  on-chain value on startup.
- Active-committee block bonds are read from the finalized-floor state and validation checks the
  same floor-derived committee.
- Multi-value `IntegerAdd` channels now fail loudly; `BitmaskOr` remains foldable.
- REST/gRPC deploy lookup responses expose effect-level deploy finalization state and rejection
  count in addition to block-level `isFinalized`.

**Test infrastructure**
- Green-gate proxy with production-like heartbeat (`TestNode.allow_empty_blocks`), a **cross-node
  node-identity assertion** (`finalized_keys_all_nodes` reads every node, not just node 0), and a
  **deterministic single-value-cell datum-count check** (`m_datum_count` via `get_data`) that turns
  a flaky cross-node peek into a precise "`@"m"` holds N datums at block #B" failure.

## Remaining (known)

No sealed-floor merge correctness items are currently open on this branch.

### Deliberate non-port
- `0bb91b22` changed reference-branch web endpoints to call `exploratory_deploy(None)` because that
  branch had redefined `None` as FS(LFB). In v2, `None` still means a speculative merge over current
  DAG tips, while `/api/validators`, `/api/validator/{pk}`, and `/api/epoch/rewards` already resolve
  `None` to the LFB and pass an explicit block hash. Directly porting `0bb91b22` here would make
  those endpoints less finalized, not more. Revisit only if v2 later changes `exploratory_deploy(None)`
  to mean finalized-floor state.

## Validation

- `cargo fmt --all --check`
- `PROTOC=... cargo test -p rspace_plus_plus -- --nocapture`
- `PROTOC=... cargo test -p casper --no-run`
- `PROTOC=... cargo test -p node --no-run`
- `PROTOC=... cargo test -p casper fold_bitmask_or -- --nocapture`
- `PROTOC=... cargo test -p casper optimal_rejection -- --nocapture`
- `PROTOC=... cargo test -p casper tuple_space_gives_up_and_surfaces_error_when_peer_silent -- --nocapture`
- `PROTOC=... cargo test -p casper lfs_horizon_requester -- --nocapture`
- `PROTOC=... cargo test -p casper block_approver_protocol -- --nocapture`
- `PROTOC=... cargo test -p casper deploy_finalization_status -- --nocapture`
- `PROTOC=... cargo test -p node heartbeat -- --nocapture`
- `PROTOC=... cargo test -p node deploy_response -- --nocapture`
- `PROTOC=... cargo test -p node find_deploy -- --nocapture`
- `PROTOC=... cargo test -p casper two_writers_converge -- --ignored --nocapture`
- `PROTOC=... cargo test -p casper three_writers_converge -- --ignored --nocapture`
