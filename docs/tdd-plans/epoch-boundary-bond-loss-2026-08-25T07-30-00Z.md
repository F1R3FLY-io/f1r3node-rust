---
kind: tdd-plan
scope: epoch-boundary-bond-loss
produced_by: /tdd
produced_at: 2026-08-25T07:30:00Z
source_issue: https://github.com/F1R3FLY-io/f1r3node-rust/issues/341
glossary: docs/Glossary.md
test_runner: cargo-test
branch: fix/validator-loses-bond-epoch-boundary-issue-341
system_boundaries:
  - test-harness-storage   # real in-memory test stores via test_node/block_generator; no internal mocks
  - determinism            # fixed keys, sigs, and timestamps; no wall clock, no randomness
conformance_audit:
  status: pending
  notes:
    - "Hypothesis to confirm or falsify in B1: the merge layer rejects a FINALIZED bonding deploy's state effects at a later multi-parent merge, and recovery purges them ('finalized canonical wins'), so the bond vanishes from canonical state. Precedent: the bridge-admin registry flake root cause."
behaviors:
  - id: B1
    statement: "A state effect whose carrier block is finalized is never rejected by merge adjudication at a later merge (deterministic reproduction of the #341 bond-loss shape)"
    priority: must
    deep_module: true
    done: true
    cycle_log:
      - tracer: true
        test: "merging::finalized_protection_spec::a_finalized_siblings_deploy_is_never_rejected_by_cost_adjudication"
        red: "behavioral: rejections contained the finalized deploy (0xF1) — cost adjudication rejected the cheap finalized-carrier chain while the expensive contender survived; the exact #341/bridge-admin mechanism"
        green: "finalized-carrier partition in dag_merger::merge, mirroring base protection: scope chains split by carrier finalization (dag.finalized_blocks_set); ordinary chains conflicting with the finalized chains' combined event log are rejected deterministically via partition_base_conflicts and travel the same record/buffer/recovery path as base conflicts; finalized chains re-join the merge set exempt from cost adjudication"
        files:
          - casper/src/rust/merging/dag_merger.rs               # finalized partition + rejection fold-in
          - casper/tests/merging/finalized_protection_spec.rs   # new spec
          - casper/tests/merging/mod.rs                         # module registration
        suite: "cargo test -p casper --release: 314 lib + 900 + 4 integration passed, 0 failed"
        discovered:
          - "Window rule runs BEFORE the finalized partition: a finalized carrier's chain with a closed deploy window can still be window-rejected. Finalized content should be exempt — candidate behavior B4."
          - "Two mutually conflicting finalized chains fall through to ordinary adjudication (deliberate liveness fallback; a finality-safety violation upstream). Documented in the code comment."
          - "Rejection-expansion over block lineage runs after the fold-in: a chain DEPENDING on a rejected chain is rejected even if its carrier is finalized — worth a dedicated behavior if B2's end-shape test does not cover it."
        deferred_refactor:
          - "produce_on/consume_on/chain builders duplicated between base_protection_spec.rs and finalized_protection_spec.rs — extract a shared tests/merging test-helper module."
  - id: B2
    statement: "The bonds map of a merge block at an epoch boundary contains every validator whose bonding deploy is finalized below the merge"
    priority: must
    deep_module: false
    done: true
    cycle_log:
      - test: "batch1::multi_parent_casper_bonding_spec::a_finalized_bond_survives_an_epoch_boundary_merge"
        red: "DEVIATION: never went RED. The test also passes against the pre-B1 tree (verified by a working-tree control run), so the deterministic epoch-boundary shape does not route finalized content into losing adjudication — consistent with the SI campaign's 19 deterministic variants all passing while the heartbeat-race variant fails ~33%. Recorded as a characterization guard for the end shape. Flagged for user review."
        green: "no implementation change"
        files:
          - casper/tests/batch1/multi_parent_casper_bonding_spec.rs   # test only
        suite: "target test passes in 123s; pre-B1 control run also passes"
        discovered:
          - "PoS epoch_length is overridable per test via GenesisParameters (parameters.2.proof_of_stake.epoch_length) — no harness change needed. The default huge epoch exists to dodge close-block epoch-change merge conflicts (genesis_builder comment); this test walks three boundary merges with epoch_length 4 and the pipeline handles them cleanly."
          - "The production #341 failure therefore needs an ingredient beyond boundary merges: the heartbeat propose race and/or LFS-restore history gaps. The preflight (heartbeat 3s cadence) remains the arbiter for the fix's effectiveness; B1's merge-level protection is the mechanism-level guard."
  - id: B3
    statement: "No JustificationRegression verdict is recorded for a lineage whose only difference is carrying the finalized bond effect"
    priority: should
    deep_module: false
    done: false
    notes:
      - "Second-order symptom guard from run 32808149007. Only meaningful once B1/B2 pin the mechanism; drop with a recorded deviation if the mechanism turns out unrelated to justification handling."
---

# TDD plan: epoch-boundary bond loss (#341)

The preflight reproduces the long-hunted "freshly-bonded joiner disappears
from bonds" flake 4-for-4 (issue #341; SI test
`test_joiner_self_proposes_at_epoch_boundary.py` documents the 19-variant
reproduction campaign). Work RED-first: B1 pins the suspected mechanism
(merge-layer rejection of finalized effects) deterministically; B2 pins the
user-visible shape (bonds map); B3 guards the observed side symptom.

Each /tdd invocation walks one RED-GREEN cycle and updates the frontmatter
cycle_log. If B1's hypothesis is falsified, record the deviation in the
cycle_log and re-plan the mechanism behaviors before continuing.
