---
kind: tdd-plan
scope: casper block buffer release and the node block processing queue (issue #24, TASK-021-12)
produced_by: /tdd
produced_at: 2026-10-07T00:40:00Z
task: TASK-021-12
branch: fix/issue-24-deepening-resolution
pr: 653
glossary: docs/Glossary.md
test_runner: "other:cargo test --release -p casper --test mod blocks; cargo test --release -p block_storage; cargo test --release -p node"
system_boundaries:
  - metrics-recorder
  - clock
  - block-store-tempdir-lmdb
  - test-transport
conformance_audit:
  status: warnings
  notes:
    - "B1 to B4 assert metrics. A metric is the interface that the next soak reads, so each test asserts the metric through the recorder at the boundary, never through internal state."
    - "B5 and B6 describe a defect that the current code may show only under a race. B5 builds the stale-link state directly through the public buffer interface. B6 must force the interleaving through the public interfaces, not through sleeps."
    - "B7 introduces a deep module (a two-lane release queue). Its tests use only the queue interface. The node pipeline test for B7 asserts processing order through the block processing result channel."
    - "The EPIC-021 cbc_policy requires a pending claim before any code change. Step 0 registers it; no cycle starts before it exists."
evidence:
  issue_comment_2026_09_25: "Under sustained load, 56% of received blocks park on missing parents, 7.1 s median and 39.6 s p90, while replay takes 0.5 to 1.1 s."
  issue_comment_2026_10_06: "No per-node stage separates failing iterations; the tail is in finality rounds and parent parking."
  soak_37469364217: "dev 778cc6754 after PR #620 and #622: 8 of 49 iterations failed, all test_load 'N deploy(s) not finalized within 45s'."
  code_trace_2026_10_07:
    - "A released child is sent to the tail of the one shared FIFO block queue, which MAX_PARALLEL_BLOCKS = 2 permits drain (node/src/rust/instances/block_processor_instance.rs). Each ancestor level costs one full queue trip."
    - "The release scan runs after every processed block, holds a processing permit, and reads and decodes every buffered block (casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs)."
    - "The resolver returns children that still have child_to_parent links, but check_if_of_interest drops a block that buffer_contains reports as AlreadyProcessed (casper/src/rust/blocks/block_processor.rs). A stale link can then hold a child until the 180 s stale prune."
step_0:
  title: "Register the pending CbC claim and classify the files"
  done: true
  claim_registered: "2026-10-07: docs/claims/casper-buffer-release.md, CLAIM-CASPER-BUFFER-001, status pending."
  cbc_identify: "2026-10-07: all four artifacts tagged cbc=mandatory in .gitattributes (three high, block_processor_instance.rs medium). The scanner proposed none of them."
  acceptance:
    - "docs/claims/casper-buffer-release.md registers CLAIM-CASPER-BUFFER-001 with status pending. The claim states three properties. Release liveness: a buffered block whose dependencies are all validated is released for processing. No stale hold: a released block is not dropped as already processed. Release order: a released block is processed before a gossip block that entered the queue after the release, and gossip still progresses."
    - "/cbc identify classifies the touched files. Each one is cbc=mandatory, or a maintainer decision records why it stays untagged."
behaviors:
  - id: B1
    statement: "Releasing a parked block records the time that the block was parked"
    priority: must
    deep_module: false
    done: false
    notes: "Histogram casper.buffer.park.time, observed when a parked block is released. Tracer bullet: it proves the release path end to end and gives the soak its first missing measurement."
    cycle_log: []
  - id: B2
    statement: "A released block records the time from its release to the start of its processing"
    priority: must
    deep_module: false
    done: false
    notes: "Histogram block-processing.release.queue-wait.time. This measures the queue position cost that the trace ranks first."
    cycle_log: []
  - id: B3
    statement: "The release scan records its duration"
    priority: should
    deep_module: false
    done: false
    notes: "Histogram casper.buffer.release-scan.time."
    cycle_log: []
  - id: B4
    statement: "A recovery re-request for a dependency is counted apart from ordinary request retries"
    priority: should
    deep_module: false
    done: false
    notes: "Counter block.requests.recovery. Today recovery and ordinary retries share block.requests.retries with no label."
    cycle_log: []
  - id: B5
    statement: "A buffered block whose dependencies are all validated is released and processed, even when a stale parent link remains"
    priority: must
    deep_module: false
    done: false
    notes: "Defect regression. Write the test first and confirm that it fails on the current code, where the block is dropped as AlreadyProcessed."
    cycle_log: []
  - id: B6
    statement: "A block whose dependency check races with the validation of its parent never keeps a stale relation to that parent"
    priority: must
    deep_module: false
    done: false
    notes: "The dependency check and the relation insert become atomic with respect to the parent's DAG insert and buffer removal."
    cycle_log: []
  - id: B7
    statement: "A released block is processed before gossip blocks that entered the queue after its release, and gossip blocks still progress"
    priority: must
    deep_module: true
    done: false
    notes: "A two-lane release queue: released blocks first, with a bounded share for gossip so that gossip cannot starve. The node pipeline uses the queue in place of the single FIFO."
    cycle_log: []
  - id: B8
    statement: "After a block is processed, only its buffered descendants are considered for release, and the processing permit is free during that work"
    priority: must
    deep_module: false
    done: false
    notes: "Use the buffer parent-to-child links of the processed block. Keep the full scan for startup and for periodic reconciliation."
    cycle_log: []
  - id: B9
    statement: "The soak ISSUE24_METRICS record includes the park, release queue wait, release scan, and recovery re-request metrics"
    priority: should
    deep_module: false
    done: false
    notes: "Extend scripts/bench/extend-issue24-metrics.sh and its test. Coordinate with PR #659, which changes scripts/bench/aggregate-perf-report.sh."
    cycle_log: []
verification:
  - "A comparison soak of this branch against the scheduled dev soak on the same base. The decision measures are sustained finalization p95, the test_load failure count, and the new park and queue-wait metrics."
  - "The PR stays a draft until the soak evidence exists and the maintainer accepts the CbC evidence for CLAIM-CASPER-BUFFER-001."
---

# Issue #24: parked-block release (TASK-021-12)

This plan follows the issue #24 evidence. The finalization tail is in finality rounds, and parked blocks stretch the rounds. The code trace of 2026-10-07 shows three release-path causes: queue position, the full release scan, and a stale-link hold.

The order is deliberate. Step 0 registers the CbC claim. B1 to B4 add the measurements, so a soak can attribute the effect of each fix. B5 to B8 fix the release path, each with a failing test first. B9 connects the new metrics to the soak record.

The recovery re-request for parents that the node already holds (trace item D) is out of scope for this plan. It adds network load but does not delay a local release.
