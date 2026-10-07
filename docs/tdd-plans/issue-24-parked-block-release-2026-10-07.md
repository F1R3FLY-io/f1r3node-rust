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
cycle_order: [B1, B7, B2, B10, B3, B4, B5, B6, B8, B9]
cycle_order_decision: "2026-10-07: the user chose to build the B7 release queue before B2. The node block queue has no test seam, so B2's release-wait metric becomes a behavior of the standalone queue module. Wiring the queue into BlockProcessorInstance follows as its own step."
behaviors:
  - id: B1
    statement: "Releasing a parked block records the time that the block was parked"
    priority: must
    deep_module: false
    done: true
    notes: "Histogram casper.buffer.park.time, observed when a parked block is released. Tracer bullet: it proves the release path end to end and gives the soak its first missing measurement."
    cycle_log:
      - date: 2026-10-07
        tracer: true
        test: "block-storage atomic_buffer_dag_transition::releasing_a_parked_block_records_its_park_time"
        red: "left: 0, right: 1. Removing the last missing parent released the child but recorded no park time."
        green: "CasperBufferKeyValueStorage records casper.buffer.park.time (seconds, source f1r3fly.casper.casper-buffer) for each child that a parent removal releases, from the child's existing first_seen_ms entry, before that entry is deleted. put_pendant uses a non-recording removal, so a block that never parked records nothing."
        files:
          - block-storage/src/rust/casperbuffer/casper_buffer_key_value_storage.rs
          - block-storage/tests/atomic_buffer_dag_transition.rs
          - block-storage/Cargo.toml
        suite: "block-storage: every target passed (201 tests). clippy -D warnings clean. casper --test mod blocks:: and sync::recovery_purge_race: 36 passed."
        observations:
          - "The park time starts at first_seen_ms, which add_relation already sets for the child, so the change adds no clock."
          - "The release point is the buffer, not the processing queue. B2 measures the queue wait that follows the release."
          - "A release through buffer_dag_transition (the atomic DAG insert and buffer removal) records the park time too, because it calls remove_unlocked."
  - id: B2
    statement: "A released block records the time from its release to the start of its processing"
    priority: must
    deep_module: false
    done: true
    notes: "Histogram block-processing.release.queue-wait.time. This measures the queue position cost that the trace ranks first."
    cycle_log:
      - date: 2026-10-07
        blocked: true
        reason: "The wait lives in the BlockProcessorInstance queue (node). The instance is built only in node_runtime.rs from a real BlockProcessor, and each queue item carries a live Arc<dyn Casper>. No node test builds one, and the casper BlockProcessor fixture is in casper/tests, out of reach of node. A pipeline test for this metric would need a full Casper engine."
        proposal: "Do B7 first as a standalone two-lane release queue module in node. The queue records the release wait when it hands out a released block, so B2 becomes a queue behavior with a direct test. Wiring the queue into BlockProcessorInstance is a separate step, covered by the casper suites and the soak."
      - date: 2026-10-07
        test: "node --test release_queue::a_released_block_records_its_wait_from_release_to_processing"
        red: "left: 0, right: 1. The queue handed out a released block and a gossip block and recorded no wait. A first run failed to compile on the missing metric name, so the name was added alone before the behavior RED."
        green: "The released lane keeps the release Instant with each item. pop records block-processing.release.queue-wait.time (seconds, source f1r3fly.casper.block-processor) when it hands out a released block. A gossip block records nothing."
        files:
          - node/src/rust/instances/release_queue.rs
          - node/tests/release_queue.rs
          - node/Cargo.toml
        suite: "node --test release_queue: 2 passed. clippy -D warnings clean for node, all targets. cargo fmt clean."
        observations:
          - "The wait covers the time in the queue only. The time from a pop to the start of validation (the permit wait) stays out of this metric. B10 decides where the pipeline pops."
          - "B10 is ratified (2026-10-07) and runs next."
  - id: B3
    statement: "The release scan records its duration"
    priority: should
    deep_module: false
    done: true
    notes: "Histogram casper.buffer.release-scan.time."
    cycle_log:
      - date: 2026-10-07
        test: "casper --test mod sync::buffer_release_scan_spec::the_release_scan_records_its_duration"
        red: "left: 0, right: 1. A release scan on a one-node TestNode network recorded no duration. The first runs failed to compile (a missing metric name, then the wrong trait import), so the name and the import were fixed before the behavior RED."
        green: "buffer_get_dependency_free_from_buffer times the scan, now in the private scan_dependency_free_from_buffer, and records casper.buffer.release-scan.time (seconds, source f1r3fly.casper). The time is recorded on the error paths too."
        files:
          - casper/src/rust/engine/multi_parent_casper/buffer_resolver.rs
          - casper/src/rust/metrics_constants.rs
          - casper/tests/sync/buffer_release_scan_spec.rs
          - casper/tests/sync/mod.rs
        suite: "cargo test --release -p casper: every target passed (mod 1000 passed, 11 ignored; lib 500 passed; the other targets passed). clippy -D warnings clean for the casper lib and the mod test target. cargo fmt clean."
        observations:
          - "cargo clippy -p casper --all-targets fails on wal_payload_retriever.rs:537 (assert_eq with a literal bool, lib test build). The file is the same as origin/dev, so the failure comes from dev and is out of scope for this branch."
          - "The test uses the real MultiParentCasperImpl through the Casper trait. No internal part is mocked."
          - "With B10, the scan runs inside the scheduler handler and still holds the processing slot. This metric is the baseline for B8."
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
    notes: "A two-lane release queue: released blocks first, with a bounded share for gossip so that gossip cannot starve. The node pipeline uses the queue in place of the single FIFO."
    done: true
    cycle_log:
      - date: 2026-10-07
        test: "node --test release_queue::a_released_block_is_processed_before_later_gossip_and_gossip_still_progresses"
        red: "With one FIFO lane (the current mpsc behavior), gossip-1 came out first, ahead of every released block."
        green: "node/src/rust/instances/release_queue.rs: ReleaseQueue with a released lane and a bounded gossip lane. pop takes released blocks first. After gossip_share_after released blocks in a row it takes one waiting gossip block, so gossip cannot starve."
        files:
          - node/src/rust/instances/release_queue.rs
          - node/src/rust/instances/mod.rs
          - node/tests/release_queue.rs
        suite: "node --test release_queue passed. clippy -D warnings clean for node, all targets. cargo fmt clean. The module is not wired into the pipeline yet, so no other test can regress."
        observations:
          - "The pipeline wiring is a separate behavior (B10), as the cycle_order decision records."
          - "B2 (release queue wait) is now a queue behavior: pop can record the wait of a released item."
  - id: B10
    statement: "The node block pipeline takes blocks from the release queue, with released blocks pushed to the released lane and gossip blocks to the gossip lane"
    priority: must
    deep_module: false
    done: true
    notes: "Wiring of the B7 queue into BlockProcessorInstance in place of the single mpsc FIFO. Added 2026-10-07 after B7. Ratified 2026-10-07."
    design_decision: "2026-10-07: the user chose the testable scheduler. run_scheduler in release_queue.rs owns the slot-then-pick loop and does not know about Casper. BlockProcessorInstance calls it with the real validation. The casper gossip sender is unchanged. A forwarder task moves gossip into the gossip lane and waits for room, so the bounded channel still applies backpressure."
    cycle_log:
      - date: 2026-10-07
        test: "node --test release_queue::a_block_released_while_gossip_waits_is_processed_before_that_gossip"
        red: "run_scheduler was first written with the current pipeline behavior (take a block, then wait for a slot, and send released blocks to the back of the one FIFO). Order: gossip-1, gossip-2, gossip-3, released-1. Expected: gossip-1, released-1, gossip-2, gossip-3."
        green: "run_scheduler waits for a processing slot first and then takes the next block, so a block released while gossip waits is taken first. Released blocks go to the released lane. ReleaseQueue gained push_gossip_wait (waits for room in the gossip lane) and close (pop returns None when the queue is closed and empty). BlockProcessorInstance forwards the gossip channel into the queue, calls run_scheduler with MAX_PARALLEL_BLOCKS slots, and returns dependency-free pendants from the handler instead of sending them to the channel tail."
        files:
          - node/src/rust/instances/release_queue.rs
          - node/src/rust/instances/block_processor_instance.rs
          - node/tests/release_queue.rs
        suite: "cargo test --release -p node: every target passed (258 lib tests, 3 release_queue tests, the other test files). clippy -D warnings clean for node, all targets. cargo fmt clean."
        observations:
          - "The gossip lane holds MAX_PARALLEL_BLOCKS blocks, so the total queue bound grows by 2 over the 2048 channel capacity. The released lane is bounded by the in-flight cap MAX_BLOCKS_IN_PROCESSING, because each pendant is marked in flight before it is released."
          - "RELEASED_BLOCKS_BEFORE_GOSSIP_TURN = 4 is a first value. The soak queue-wait metric (B2) is the evidence to tune it."
          - "The instance no longer sends to its own block_queue_tx. It drops that sender at start, so the forwarder can see the channel close. The 'Dropping dependency-free pendant because block queue is closed' warning is gone, because pendants never pass through the channel now."
          - "The release scan still holds the processing slot, as before. B8 moves it out."
          - "No test drives BlockProcessorInstance itself. The casper suites and the comparison soak cover the real validation path."
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
