---
kind: tdd-plan
scope: key-contention-starvation
produced_by: /tdd
produced_at: 2026-08-20T04:52:46Z
source_issue: https://github.com/F1R3FLY-io/f1r3node-rust/issues/294
glossary: docs/Glossary.md
test_runner: cargo-test
branch: feat/key-contention-phase2
system_boundaries:
  - test-harness-storage   # real in-memory/LMDB test stores via test_node/block_generator; no internal mocks
  - determinism            # fixed keys, sigs, and timestamps; no wall clock, no randomness
conformance_audit:
  status: pass
  notes: []
behaviors:
  - id: B1
    statement: "A deploy chain with more prior on-DAG rejections wins conflict adjudication against an otherwise-equal contender"
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - tracer: true
        test: "rust::merging::dag_merger::tests::overfilled_splitter_prefers_previously_rejected_contender"
        red: "assertion failure: the contender with no prior losses must be the one rejected (behavioral, not compile)"
        green: "keep-one survivor ordering sorts by (unpinned, Reverse(prior_rejections)); stable sort preserves existing determinism on ties"
        files:
          - casper/src/rust/merging/deploy_chain_index.rs   # new pub field prior_rejections (non-identity, default 0)
          - casper/src/rust/merging/dag_merger.rs           # keep-one ordering + test
        suite: "cargo test -p casper --lib: 296 passed"
        discovered:
          - "POLARITY: get_optimal_rejection picks the MINIMUM-sorting rejection option, while keep-one keeps the MINIMUM-sorted chain. A loss-aware term in DeployChainIndex::Ord would invert the optimal-rejection path. Loss-awareness there must prefer the OPTION that rejects the fewest prior losses — separate site, feeds B2/B4."
          - "prior_rejections is not yet populated anywhere in production; index construction plumbing is B5's scope."
  - id: B2
    statement: "Adjudication is unchanged when contenders have equal prior-rejection counts"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - test: "rust::merging::dag_merger::tests::overfilled_splitter_equal_prior_losses_keep_content_order"
        red: "DEVIATION: never went RED — B1's stable sort satisfies this by construction. Recorded as a characterization guard protecting the deterministic baseline while B4/B5 extend adjudication. Flagged for user review."
        green: "no implementation change"
        files:
          - casper/src/rust/merging/dag_merger.rs   # test only
        suite: "cargo test -p casper --lib: 297 passed"
  - id: B3
    statement: "Under sustained equal-cost same-key contention, a repeatedly rejected deploy lands before its validity window closes"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - test: "rust::merging::dag_merger::tests::sustained_same_key_contention_lands_before_window_closes"
        red: "starved deploy lost every merge for 50 rounds and expired — the issue's losing-every-merge shape, driven round-by-round through production adjudication with a fresh equal-cost contender per round"
        green: "prior_rejection_counts implemented: per-sig count of non-duplicate RejectedDeploy records; stamp_prior_rejections sums a chain's deploy counts. Lands round 2."
        files:
          - casper/src/rust/merging/dag_merger.rs   # prior_rejection_counts, stamp_prior_rejections + test
        suite: "cargo test -p casper --lib: 298 passed"
        discovered:
          - "Counting already skips duplicate-flagged records (their semantic: the record does not dispute a standing win) — half of B5 is satisfied; B5's remaining RED is the merge() production wiring: nothing in production calls prior_rejection_counts/stamp_prior_rejections yet."
  - id: B4
    statement: "Proposer and validator compute identical rejection sets when prior losses influence adjudication"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - test: "batch2::loss_priority_spec::rotating_merge_proposers_land_repeatedly_rejected_deploy_before_expiry"
        red: "The first harness mixed agreement with fixed-proposer liveness. Its liveness assertion stayed RED although each peer accepted the rejection sets."
        green: "The B7 harness provides agreement evidence. Both validators accept the repeated rejection decisions and the final landing block."
        mixed_behavior: |
          The first harness tested two different claims. Cross-node block
          acceptance tests proposer-validator agreement. The final landing
          assertion tests liveness under fixed-proposer main-parent base bias.
        expected_control: |
          The ignored fixed-proposer test remains RED for 16 rounds. This
          result records residual base bias and does not block B4 agreement.
        files:
          - casper/src/rust/merging/dag_merger.rs
          - casper/src/rust/merging/conflict_set_merger.rs
          - casper/src/rust/util/rholang/interpreter_util.rs
          - casper/tests/batch2/loss_priority_spec.rs
        suite: "Focused B6 and B7 tests passed. cargo test -p casper --lib: 310 passed."
        design_decision: |
          Phase 2 uses B1 merged-frontier retry packaging with proposer
          rotation as test evidence. C1 needs residual-expiry soak evidence.
          C2 remains in reserve. C3 remains rejected by Principle P4.
        discovered:
          - "Main-parent base bias is separate from proposer-validator rejection-set agreement."
  - id: B6
    statement: "A gated retry is packaged only when one selected parent covers every non-invalid latest-message justification"
    priority: must
    deep_module: false
    done: true
    pending_ratification: false
    notes:
      - "Phase 2, option B1 (merged-frontier retry packaging) from docs/casper/CONSENSUS_PHILOSOPHY.md Section 5. Node-local packaging policy in prepare_user_deploys_with_policy; peer-safe by Ground Truth 2 (deferral is always legal). RED shape: in the racing harness, assert the owner block never packages the retry as a sibling of an unmerged contender."
    cycle_log:
      - test: "rust::blocks::proposer::block_creator::tests::retry_frontier_defers_without_and_accepts_with_redundant_covering_parent"
        red: "The open retry gate selected the buffered retry over two visible sibling parents."
        green: "Retry selection requires one selected parent that covers every non-invalid latest-message justification. The test also accepts a covering parent when another selected parent is redundant."
        files:
          - casper/src/rust/blocks/proposer/block_creator.rs
        suite: "cargo test -p casper --lib: 310 passed"
        discovered:
          - "The policy uses the authenticated parent and justification frontier. It does not predict Rholang keys from deploy source."
  - id: B7
    statement: "Under rotating merge proposers, a repeatedly rejected deploy lands before its validity window closes"
    priority: must
    deep_module: false
    done: true
    pending_ratification: false
    notes:
      - "Phase 2, option A (rotation as the liveness mechanism): reshape loss_priority_spec so merge proposers alternate. With B6 plus loss-aware adjudication this must go deterministically GREEN. The adversarial always-contender-merges shape stays as an #[ignore]d sentinel documenting the residual base-bias facet (C1 escalation trigger)."
    cycle_log:
      - test: "batch2::loss_priority_spec::rotating_merge_proposers_land_repeatedly_rejected_deploy_before_expiry"
        red: "The rotating schedule failed for 16 rounds because B6 required the selected-parent set to contain exactly one block."
        green: "The retry gate accepts any selected parent that covers every non-invalid latest-message justification. The twice-rejected deploy lands on its first eligible covered-frontier round."
        files:
          - casper/src/rust/blocks/proposer/block_creator.rs
          - casper/tests/batch2/loss_priority_spec.rs
        suite: "Focused B6 and B7 tests passed. cargo test -p casper --lib: 310 passed."
        expected_control: "The ignored fixed-proposer test still rejects the retry for all 16 rounds."
        discovered:
          - "A selected-parent set can contain redundant parents. One parent can still cover the complete latest-message frontier."
        refactor_deferred: "The integration tests duplicate one fixture. Extract a schedule helper only if another contention schedule needs the fixture."
  - id: B5
    statement: "Prior-rejection counts derive only from kept (non-duplicate) records visible in the merge scope"
    priority: should
    deep_module: false
    done: true
    cycle_log:
      - test: "rust::merging::dag_merger::tests::scope_counts_aggregate_kept_records_across_visible_blocks"
        red: "stub returned empty counts; assertion expected 2 kept records across a scope block and a base-lineage block, duplicate excluded"
        green: "scope_prior_rejection_counts collects records_of(block) over the caller-assembled visible set and reuses prior_rejection_counts (kept-only)"
        files:
          - casper/src/rust/merging/dag_merger.rs
        suite: "cargo test -p casper --lib: 299 passed"
        discovered:
          - "DESIGN NOTE ratified in test: visible set = merge scope PLUS base-lineage window, because the retry gate settles a rejection below the floor before the retry — scope-only counting would return 0 exactly when priority matters. The block-set assembly and merge() wiring is B4's cycle."
---

# TDD Plan: key-contention starvation (issue #294)

## Interface under test

Merge conflict adjudication as observed through its public results:

- `dag_merger::merge` / `conflict_set_merger::resolve_conflicts` — which
  same-key contender survives keep-one and optimal-rejection selection.
- The recovery composition observable through deploy terminal state
  (`DeployLifecycle` verdicts): a contended deploy must land, not expire.

Tests assert adjudication outcomes and terminal states, never internal
ordering functions or call sequences.

## Defect (from issue #294)

`DeployChainIndex` ordering is a pure function of deploy content (total
cost, max cost, lexicographic signature). Equal-cost same-key contenders
tie down to the signature comparison, which never changes, so the same
deploy loses every merge it enters. The recovery pipeline faithfully
re-proposes it (retry gate, retries-first selection) into the same
deterministic loss until `deploy_lifespan` closes the validity window and
the deploy terminates `Expired`.

## Fix direction (ratified 2026-08-20)

Loss-aware adjudication: derive a per-deploy prior-rejection count from
kept (non-duplicate) `RejectedDeploy` records visible in the merge scope
(on-chain data, per the `floor_context.rs` escape-hatch rule), and rank
higher counts first in conflict adjudication, falling back to the
existing content ordering on ties. Each loss raises the loser's on-chain
count, so starvation is bounded.

## Phase 2 (ratified for implementation)

Phase 1 (B1–B3, B5) fixed the content-deterministic adjudication facet.
B4 proposer-validator agreement is complete. The ignored fixed-proposer
sentinel records a separate main-parent base-bias liveness risk. The canonical
analysis is
[docs/casper/CONSENSUS_PHILOSOPHY.md](../casper/CONSENSUS_PHILOSOPHY.md):
Section 5 holds the remedy ladder (A / B1 / B2 / C1 / C2 / C3 with the
comparison table), Section 6 the ratified principles P1–P6.

Ratified phase-2 path:

- **B6** — merged-frontier retry packaging (ladder option B1): node-local
  packaging policy, peer-safe, small diff.
- **B7** — rotating-proposer test evidence (ladder option A): the active
  GREEN scenario in `loss_priority_spec`; the adversarial shape stays
  `#[ignore]`d as the C1 escalation sentinel.

Escalation: C1 (loss-aware main-parent declaration) only behind soak
evidence of residual expiries; C2 in reserve; C3 rejected (Principle P4:
fork choice stays deploy-content-blind).

## Boundaries

- Real test stores and the in-process harness (`test_node`,
  `block_generator`); no mocks of internal collaborators.
- Determinism via fixed keys and signatures; no wall clock, no
  randomness.

## Cycle log

(appended per /tdd invocation)
