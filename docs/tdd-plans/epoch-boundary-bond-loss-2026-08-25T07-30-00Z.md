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
    done: false
    notes:
      - "RED test: finalize a block carrying a bonding-style state effect on the canonical lineage; drive a later multi-parent merge whose conflict set contends the same channel; assert the finalized effect survives in the merged state."
      - "If the test cannot go RED (finalized effects already protected), record the deviation and pivot B1 to the epoch-boundary close-block path (PoS ejection) as the next mechanism candidate."
      - "Build on the dag_merger test harness patterns from key-contention-starvation plan (round-driven production adjudication)."
  - id: B2
    statement: "The bonds map of a merge block at an epoch boundary contains every validator whose bonding deploy is finalized below the merge"
    priority: must
    deep_module: false
    done: false
    notes:
      - "End-shape test at the block/bonds level, mirroring the SI assertion 'V4 unexpectedly dropped from bonds'. Uses test_node helpers; epoch_length small so the merge lands on a boundary."
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
