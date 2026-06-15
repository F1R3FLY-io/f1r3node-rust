# 14 · Test Plan

**Exhaustive test plan covering every use case (112 scenarios) and
every change documented in the design and verification doc set
(across the spec, verification, design, and diagrams).** This
document specifies
both **example-based** (concrete-trace) tests and **property-based**
(invariant) tests, organized by component layer and theorem family.
The harness and use-case tests live under `casper/tests/slashing/`;
the test specifications below are normative for maintaining them.

## 14.1 Philosophy and goals

| Goal                                                           | Mechanism                                                                                                         |
|----------------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------|
| **Soundness.** No honest validator slashed.                    | Property-based test of T-1 (`detection_sound`).                                                                   |
| **Completeness.** Every Byzantine action eventually slashed.   | Property-based test of T-2 + T-3.                                                                                 |
| **Atomicity.** Concurrent detections preserve evidence.        | Property-based test of T-9.2 over n-thread schedules.                                                             |
| **Determinism.** Replay produces identical post-states.        | Property-based test of T-15 (bisimilarity-projection equality across replay).                                     |
| **Liveness.** Slash transition reaches finite-time conclusion. | Property-based test of T-9.4 (transfer-failure error path).                                                       |
| **Mutual destruction.** Neglecters slashed.                    | Example-based + property-based test of T-11 + T-12.                                                               |
| **BFT-quorum preservation.** Under `|closure| ≤ F`.            | Property-based test of T-12 with bounded `|closure|` generator; counter-example test (UC-26) for `|closure| > F`. |
| **Bug-fix correctness.** Each T-9.M proven property holds.     | Property-based test per T-9.M, plus example-based pre-fix counter-example traces (UC-41/42/43).                   |
| **Three-tier execution agreement.** Harness, oracle, and production agree on every observable under arbitrary event sequences. | Triple-bisimilarity property tests over `SlashingObserver` (§14.2.4). |
| **Mutation coverage.** Every test-suite weakness surfaces as a surviving mutant.                            | Nightly `cargo-mutants` run with survival-rate threshold ≤ 5 % (§14.8.8). |

The test plan **must catch regressions before they reach production**.
Concretely:
- Every Rocq-mechanized theorem (T-1 through T-15a/b, T-Idem,
  T-9.1–T-9.15) and every Rholang-level guard property
  (`T-AuthCheck`) maps to **at least one** Rust property test that
  fails if the property is violated at runtime.
- Every TLA+ invariant (`Inv_*`) maps to a Rust integration test
  that asserts the same invariant on a small randomized trace.
- Every documented bug (#1–#11) has a **pre-fix counter-example**
  test that fails on the pre-fix code path (proving the bug was
  real) and a **post-fix passing** test (proving the fix closes it).
- The traceability ledger (`slashing-traceability.md`) gates Rust source
  work. Model-only
  boundaries and projection risks receive formal classifications and
  regression fixtures; they do not require production source changes
  unless reproduced on the production path.

## 14.2 Test infrastructure

### 14.2.1 The `SlashingTestHarness` API

A unified harness in `casper/tests/slashing/harness.rs` exposes the
slashing-pipeline state via the abstractions used in the LTS of
spec §4-§8:

```rust
pub struct SlashingTestHarness {
    /// Number of validators bonded at genesis.
    pub validator_count: usize,
    /// Stake per validator at genesis.
    pub stake_per_validator: i64,
    /// In-memory DAG; each block has (sender, seq, hash, justifications).
    pub dag: DagState,
    /// Equivocation tracker; (Validator, baseSeqNum) → BTreeSet<BlockHash>.
    pub tracker: EquivocationTrackerStore,
    /// Bond map, active-set, and Coop vault balance — projection of on-chain state.
    pub pos_state: PoSState,
    /// Whether to use the locked or unlocked tracker access (toggle for bug #2 testing).
    pub locked: bool,
}

impl SlashingTestHarness {
    pub fn new(validator_count: usize, stake: i64) -> Self;
    pub fn sign_block(&mut self, validator: &str, seq: u64) -> BlockHash;
    pub fn sign_block_distinct(&mut self, validator: &str, seq: u64) -> BlockHash;
    pub fn propose(&mut self, proposer: &str, parents: &[BlockHash], deploys: &[SlashDeploy]) -> BlockHash;
    pub fn detect(&self, block: BlockHash) -> Status;          // Valid | Admissible | Ignorable | Neglected
    pub fn has_record(&self, validator: &str, base_seq: u64) -> bool;
    pub fn record_witnesses(&self, validator: &str, base_seq: u64) -> BTreeSet<BlockHash>;
    pub fn execute_slash(&mut self, target: &str, block_hash: BlockHash) -> SlashResult;
    pub fn bond(&self, validator: &str) -> i64;
    pub fn coop_vault(&self) -> i64;
    pub fn is_active(&self, validator: &str) -> bool;
    pub fn fork_choice(&self) -> Vec<&str>;                    // returns validators counted in GHOST
}
```

### 14.2.2 Generators (proptest / quickcheck)

For property-based tests, the harness exposes generators that
produce randomized but well-formed inputs:

```rust
pub fn gen_validator_id() -> impl Strategy<Value = ValidatorId>;
pub fn gen_block_hash() -> impl Strategy<Value = BlockHash>;
pub fn gen_seq_num(max: u64) -> impl Strategy<Value = u64>;
pub fn gen_dag_state(n_validators: usize, depth: usize) -> impl Strategy<Value = DagState>;
pub fn gen_equivocation(dag: &DagState) -> impl Strategy<Value = (ValidatorId, BlockHash, BlockHash)>;
pub fn gen_thread_schedule(n_threads: usize, ops_per_thread: usize) -> impl Strategy<Value = Schedule>;
pub fn gen_bonds_map(validator_count: usize, max_stake: i64) -> impl Strategy<Value = BondsMap>;
```

Generators are **shrinking-aware** — minimal counter-examples are
preferred for failure diagnosis.

### 14.2.3 Oracles

Each property test compares the harness's observed state against
an **oracle** computed from the formal LTS. The oracle for each
theorem is a hand-written Rust function that mirrors the Rocq
definition. Discrepancies between the harness
and the oracle indicate either:
- A bug in the harness (test infrastructure).
- A bug in the implementation (the property is violated).

The oracle for `slash` is `PoSContract.slash : PoSState → Validator
→ PoSState × bool`; the oracle for `detect` is
`EquivocationDetector.detect : DAGState → Block → DetectionStatus`.

### 14.2.4 Tier model

The principled architecture defines **three tiers**, each
implementing the read-only `SlashingObserver` trait (`bond`,
`coop_vault`, `is_active`, `has_record`, `record_witnesses`,
`fork_choice`):

| Tier | Implementation | Cardinality | Speed | Role |
|------|----------------|-------------|-------|------|
| 1    | `SlashingProductionAdapter` wraps `TestNode`+`BlockDagKeyValueStorage`+Rholang | 5–8 example + 3 triple-bisim | Slow (LMDB+RSpace) | Source of truth |
| 2    | `RocqOracleAdapter` wraps `oracle.rs` over `(DagState, EqRecordSet, PoSState)` | 30+ proptest cases | Fast (pure) | Formal-mechanization mirror |
| 3    | `SlashingTestHarness` — in-memory LTS state machine | 50+ UC + proptest cases | Fastest (in-memory) | Refinement of (2) + adapter for (1) |

The **triple-bisimilarity proptests** (Track 3 / §14.5
generalized) drive the same generated event sequence through
all three tiers and assert that every `SlashingObserver` method
returns identical values across all three. Disagreement on any
observable points to drift in whichever tier is the outlier;
without disagreement, all three are observationally equivalent
on the operations exercised.

The harness exists *because it is not the production*: a fast
in-memory state machine enables 10,000-case proptests to run in
seconds and shrinks failures to minimal counter-examples. The
oracle exists *because it is not the harness*: it is a faithful
hand-translation of the Rocq theorems, so a harness↔oracle
disagreement diagnoses harness drift away from the formal model.
The production adapter exists *because it is the production*:
disagreement vs. either tier diagnoses production drift away
from the spec.

See `docs/theory/slashing/design/14a-tier-architecture.md` for
the full tier-model documentation and the rationale for the
hybrid C+D+E architecture.

## 14.3 Example-based tests (112 use cases)

The 112 use cases from spec §12 are mapped to Rust integration
tests. Each test follows this pattern (UC-01 shown as the canonical
example):

```rust
#[test]
fn uc_01_admissible_single() {
    let mut harness = SlashingTestHarness::new(3, 100);

    // Step 1: Validator A signs two distinct blocks at seq 5.
    let b1  = harness.sign_block("A", 5);
    let b1p = harness.sign_block_distinct("A", 5);

    // Step 2: Validator B proposes at seq 6 citing both.
    let b2 = harness.propose("B", &[b1, b1p], &[]);

    // Step 3: detection fires.
    assert_eq!(harness.detect(b1p), Status::AdmissibleEquivocation);

    // Step 4: record exists.
    assert!(harness.has_record("A", 4));

    // Step 5: validator C's next block carries SlashDeploy(b1p, A).
    let b3 = harness.propose("C", &[b2], &[SlashDeploy::for_offender("A", b1p)]);

    // Step 6: A is slashed.
    assert_eq!(harness.bond("A"), 0);
    assert_eq!(harness.coop_vault(), 100);
    assert!(!harness.is_active("A"));
    assert!(!harness.fork_choice().contains(&"A"));    // T-10
}
```

The current mapping of all **112 use cases** and theorem-derived
fixtures is given below. **Naming-convention note.** Core UCs
(UC-01..UC-27) and the explicit Tier-A and §14.3.4 Tier-C entries
follow the `casper/tests/slashing/uc_NN_<descriptive>.rs` convention.
Tier B (UC-28..UC-36, §14.3.3) and the Sage-derived UCs in the
UC-55..UC-100 + UC-109..UC-111 range use **descriptive filenames
without the `uc_NN_` prefix**, because each such test has a strong
semantic identity (e.g. `weighted_neglect_chain.rs`,
`integration_t_invalid_parents.rs`) and the UC numbering is a
*post-hoc* index, not a primary key. Both conventions coexist by
design; the index in the tables below maps every UC number to its
canonical Rust test module so the convention split is transparent
to any reader scanning by UC number.

The Tier B mapping (UC-28..UC-36) is presented as a prose list at
§14.3.3 rather than a tabulated row-per-UC because each Tier-B test
exercises exactly one slashable `InvalidBlock` variant under
post-fix #3 and the assertion is uniform across all nine.

### 14.3.1 Core scenarios (UC-01 through UC-25)

| #     | Current Rust test module                                           | Asserts                                                                                                                  |
|-------|-----------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| UC-01 | `casper/tests/slashing/uc_01_admissible_single.rs`        | A slashed; bond zero; vault += 100; A excluded from fork-choice (T-1, T-2, T-7, T-10).                                   |
| UC-02 | `casper/tests/slashing/uc_02_concurrent_admissible.rs`         | f validators equivocate; all slashed; quorum preserved if `|closure| ≤ F` (T-1, T-12).                                   |
| UC-03 | `casper/tests/slashing/uc_03_ignorable_unrequested.rs`          | Unsolicited equivocation now slashed under post-fix #1 (T-9.1).                                                          |
| UC-04 | `casper/tests/slashing/uc_04_neglect_two_level.rs`                | A equivocates → A slashed; B neglects → B slashed in same block (T-11, T-12).                                            |
| UC-05 | `casper/tests/slashing/uc_05_justification_regression.rs` | JustificationRegression triggers EquivocationRecord under post-fix #3 (T-9.3).                                           |
| UC-06 | `casper/tests/slashing/uc_06_self_regression.rs`          | Self-regression detected (Boolean predicate; T-9.6).                                                                     |
| UC-07 | `casper/tests/slashing/integration_t_invalid_bonds_cache.rs`      | InvalidBondsCache slashed under post-fix #3 (T-9.3).                                                                     |
| UC-08 | `casper/tests/slashing/uc_08_contains_expired_deploy.rs`           | ContainsExpiredDeploy slashed (T-9.3).                                                                                   |
| UC-09 | `casper/tests/slashing/uc_09_contains_time_expired_deploy.rs`      | ContainsTimeExpiredDeploy slashed (T-9.3).                                                                               |
| UC-10 | `casper/tests/slashing/integration_t_invalid_block_number.rs`     | InvalidBlockNumber slashed (T-9.3).                                                                                      |
| UC-11 | `casper/tests/slashing/uc_11_stake_zero_protocol_unreachable.rs`               | Stake-0 bonded validator: invariant unreachable post-fix #5; pre-fix counter-example fails detection (T-9.5).            |
| UC-12 | `casper/tests/slashing/uc_12_tracker_race.rs`             | Concurrent insert: post-fix preserves both witnesses; pre-fix loses one (T-9.2).                                         |
| UC-13 | `casper/tests/slashing/uc_13_transfer_failure.rs`         | Transfer-failure deterministic return; post-fix #4 returns `(false, "transfer failed")` (T-9.4).                         |
| UC-14 | `casper/tests/slashing/uc_14_replay_after_crash.rs`             | Detector crash mid-RMW: schedule of length 1 with suspended write; record-monotonicity preserved (T-9.2 corollary).      |
| UC-15 | `casper/tests/slashing/uc_15_proposer_crash_recovery.rs`           | Proposer crash after detection: behavioral; next proposer takes over.                                                    |
| UC-16 | `casper/tests/slashing/uc_16_slashed_parent_fork_choice.rs`           | Multi-parent block with slashed parent: parent counted once in DAG, excluded from fork-choice (T-10).                    |
| UC-17 | `casper/tests/slashing/uc_17_forkchoice_mixed.rs`         | Mixed slashed/active: only active votes counted in GHOST (T-10).                                                         |
| UC-18 | `casper/tests/slashing/uc_18_bonded_proposer_slash_deploy.rs`       | Bonded-proposer slash-deploy emission targets each outstanding offender (T-9.8 positive companion).                                    |
| UC-19 | `casper/tests/slashing/uc_19_two_level_bond_zero.rs`      | Two-level where neglecter is bond-zero: only equivocator slashed (T-11, T-9.5).                                          |
| UC-20 | `casper/tests/slashing/uc_20_seq_gap_equivocation.rs`, `casper/src/rust/equivocation_detector.rs` unit tests | Skipped seq number across equivocation: canonical child detection succeeds post-fix #7 (T-9.7). |
| UC-21 | `casper/tests/slashing/uc_21_auth_token_check.rs`         | Spoofed auth token rejected at first PoS guard (T-AuthCheck).                                                            |
| UC-22 | `casper/tests/slashing/uc_22_unbonded_proposer.rs`        | Unbonded proposer post-fix #8: no slash deploys emitted (T-9.8); pre-fix Rust: block rejected at proposer-bond layer.    |
| UC-23 | `casper/tests/slashing/uc_23_self_correcting.rs`          | Self-correcting block (Rust widening): admitted if it carries SlashDeploy(_, A); offender slashed in same block (T-9.9). |
| UC-24 | `casper/tests/slashing/uc_24_slash_idempotence_trace.rs`              | Slash twice on same validator: second is no-op (T-Idem; alias T-9).                                                      |
| UC-25 | `casper/tests/slashing/uc_25_coop_vault_accounting.rs`         | After slash, `vault.balance += pre_slash.bond[v]` (T-8).                                                                 |

### 14.3.2 Tier A — Audit blockers (8 tests)

| #     | Current Rust test module                                            | Asserts                                                                                                                                   |
|-------|------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| UC-26 | `casper/tests/slashing/uc_26_quorum_drop.rs`  | `|closure| > F` ⟹ active set drops below quorum (counter-example to T-12 outside its precondition; T-11 closure-termination still holds). |
| UC-27 | `casper/tests/slashing/uc_27_neglected_invalid_block.rs`   | `NeglectedInvalidBlock` dispatch under post-fix #3 (T-3, T-6, T-9.3).                                                                     |
| UC-37 | `casper/tests/slashing/uc_37_self_regression_dag_level.rs` | DAG-level self-regression (T-9.6 DAG-level).                                                                                              |
| UC-38 | `casper/tests/slashing/uc_38_neglected_detection.rs`       | `detect_neglected` soundness + completeness (T-6).                                                                                        |
| UC-39 | `casper/tests/slashing/uc_39_cross_impl_bisim.rs`               | R-relation invariant preserved across pipeline step (T-13a/b/c, T-14, T-15a/b).                                                           |
| UC-41 | `casper/tests/slashing/uc_41_ignorable_pre_fix_alias.rs`     | Pre-fix #1: ignorable equivocation silently dropped (regression test).                                                                    |
| UC-42 | `casper/tests/slashing/uc_42_dispatcher_pre_fix_alias.rs`   | Pre-fix #3: non-equivocation slashable variants not recorded (regression).                                                                |
| UC-43 | `casper/tests/slashing/uc_43_seqnum_pre_fix_alias.rs`, `casper/tests/slashing/prop_t_9_7_seqnum_density.rs` | Pre-fix #7: exact `baseSeqNum + 1` lookup misses gap; post-fix canonical search detects and same-branch latest messages do not overcount. |

### 14.3.3 Tier B — Slashable-variant catalog completion (9 tests)

UC-28 through UC-36 — one test per remaining slashable
`InvalidBlock` variant, each asserting that post-fix #3 routes the
verdict through the standard slash pipeline. Current Rust test modules at:

  `casper/tests/slashing/integration_t_invalid_parents.rs`
  `casper/tests/slashing/integration_t_invalid_follows.rs`
  `casper/tests/slashing/integration_t_invalid_sequence_number.rs`
  `casper/tests/slashing/integration_t_invalid_shard_id.rs`
  `casper/tests/slashing/integration_t_invalid_repeat_deploy.rs`
  `casper/tests/slashing/uc_33_deploy_not_signed.rs`
  `casper/tests/slashing/integration_t_invalid_transaction.rs`
  `casper/tests/slashing/integration_t_invalid_block_hash_records.rs`
  `casper/tests/slashing/integration_t_contains_future_deploy.rs`

### 14.3.4 Tier C — Operational, adversarial, and frontier fixtures

| #     | Current Rust test module                                                | Asserts                                                                                      |
|-------|----------------------------------------------------------|----------------------------------------------------------------------------------------------|
| UC-40 | `casper/tests/slashing/uc_40_vault_accounting_failure.rs`      | Vault transfer fail → coop balance unchanged; A returns to EquivocatorRecorded (T-8, T-9.4). |
| UC-44 | `casper/tests/slashing/uc_44_simultaneous_independent_equivocations.rs` | n parallel equivocations; all detected (T-1, T-9.2, T-12).                                   |
| UC-45 | `casper/tests/slashing/uc_45_slash_replay_attack.rs`           | Replayed slash deploy: second is no-op (T-Idem, T-9.8).                                      |
| UC-46 | `casper/tests/slashing/uc_46_partition_merge_equivocations.rs`            | Partition + merge: post-merge equivocation detected once (T-1, T-9.2, T-15).                 |
| UC-47 | `casper/tests/slashing/uc_47_48_validator_set_changes.rs`   | Validator joins during pending slash: still slashed; new joiner unaffected (T-Idem, T-10).   |
| UC-48 | `casper/tests/slashing/uc_47_48_validator_set_changes.rs`  | Validator leaves during pending slash: re-joins as Unbonded; record persists (T-Idem, T-10). |
| UC-49 | `casper/tests/slashing/uc_49_genesis_edge_cases.rs`                 | Genesis-time invalid sender: slash routed correctly (T-9.3).                                 |
| UC-50 | `casper/tests/slashing/uc_50_multi_slash_in_one_block.rs`      | k slash deploys in same block: all atomically applied; replay deterministic (T-Idem, T-11).  |
| UC-51 | `casper/tests/slashing/uc_51_53_dag_topologies.rs`           | Deep DAG (>100 blocks): detection scales linearly (T-1, T-15).                               |
| UC-52 | `casper/tests/slashing/uc_51_53_dag_topologies.rs`           | Wide DAG (high parent-fanout): detection unaffected by parent count (T-1, T-15).             |
| UC-53 | `casper/tests/slashing/uc_51_53_dag_topologies.rs`               | Single-chain (no forks) equivocation: still detected (T-1, T-9.6).                           |
| UC-54 | `casper/tests/slashing/uc_54_record_invariants.rs`             | Record monotonicity + uniqueness invariants (T-4, T-5).                                      |
| UC-55 | `casper/tests/slashing/weighted_neglect_chain.rs`        | Stake-weighted neglect chain: closure stake bound is required for weighted quorum (T-12W).   |
| UC-56 | `casper/tests/slashing/zero_stake_direct_offender.rs`    | Zero-stake/stale direct offender cannot seed closure after current bonded filtering.         |
| UC-57 | `casper/tests/slashing/stale_evidence_filtered.rs`       | Off-era evidence is filtered before current validator closure propagates.                    |
| UC-58 | `casper/tests/slashing/evidence_visibility_gap.rs`       | Partial evidence visibility does not create neglect edges absent visible unreported evidence.|
| UC-59 | `casper/tests/slashing/duplicate_neglect_edges.rs`       | Duplicate neglect edges are idempotent and produce the same closure.                         |
| UC-60 | `casper/tests/slashing/disconnected_neglect_cycle.rs`    | A neglect cycle with no path to a direct offender is not slashed.                            |
| UC-61 | `casper/tests/slashing/bounded_arithmetic_projection.rs` | Slash accounting uses checked arithmetic or rejects fixed-width overflow projections.        |
| UC-62 | `casper/tests/slashing/quorum_intersection_after_slash.rs` | Any two active quorums intersect after slashing under the strict BFT bound.                |
| UC-63 | `casper/tests/slashing/closure_fixed_point_certificate.rs` | Closure is stable at `|V|` and each slashed validator has a path certificate.              |
| UC-64 | `casper/tests/slashing/epoch_evidence_rollover.rs`       | Stale or off-era evidence cannot seed current-epoch closure.                                 |
| UC-65 | `casper/tests/slashing/record_normalization.rs`          | Record equality is modulo hash order and duplicate witnesses.                                |
| UC-66 | `casper/tests/slashing/evidence_view_divergence.rs`      | Different active evidence views can compute different closure; equal active views agree.      |
| UC-67 | `casper/tests/slashing/report_time_closure_shrinkage.rs` | Reports remove active neglect edges; closure need not be monotone over report time.           |
| UC-68 | `casper/tests/slashing/rebonded_identity_boundary.rs`    | Rebonded/stale evidence is filtered unless an explicit carryover policy maps it current.      |
| UC-69 | `casper/tests/slashing/theorem_assumption_counterexamples.rs` | Each key theorem hypothesis has a minimal counterexample when removed.                  |
| UC-70 | `casper/tests/slashing/weighted_amplification_boundary.rs` | Weighted amplification is possible outside the bounded-closure precondition.               |
| UC-71 | `casper/tests/slashing/partial_batch_failure_atomicity.rs` | Abort-on-first-failure batch slashing is order-dependent unless atomic.                    |
| UC-72 | `casper/tests/slashing/projection_risk_regressions.rs`   | Canonical record keys, evidence retention, duplicate normalization, and arithmetic envelopes. |
| UC-73 | `casper/tests/slashing/hypothesis_reduced_scenarios.rs`  | Hypothesis-minimized witnesses replay deterministically and stay in classified buckets.     |
| UC-74 | `casper/tests/slashing/proposer_fairness_boundary.rs`    | Observed evidence is not bounded-live without proposer evidence-inclusion fairness.         |
| UC-75 | `casper/tests/slashing/delimiter_free_record_key_collision.rs` | Delimiter-free record keys collide; canonical pair encoding does not.              |
| UC-76 | `casper/tests/slashing/hypothesis_multi_epoch_state_machine.rs` | Rule-based multi-epoch state machine keeps churn/evidence traces classified.       |
| UC-77 | `casper/tests/slashing/semantic_attack_campaign_classification.rs` | Semantic campaign traces stay in documented boundary/projection/assumption buckets. |
| UC-78 | `casper/tests/slashing/metamorphic_graph_record_frontier.rs` | Edge order, duplicate edges, report suppression, and record normalization remain metamorphic-safe. |
| UC-79 | `casper/tests/slashing/hypothesis_assumption_minimization.rs` | Minimized assumption witnesses remain reproducible.                                |
| UC-80 | `casper/tests/slashing/hypothesis_rust_differential_corpus.rs` | JSON frontier traces replay in Rust with the expected classification.              |
| UC-81 | `casper/tests/slashing/hypothesis_bundle_evidence_state_machine.rs` | Bundle-reused validators and edges preserve active-edge admissibility.             |
| UC-82 | `casper/tests/slashing/hypothesis_feature_combination_coverage.rs` | Frontier feature combinations remain classified and replayable.                    |
| UC-83 | `casper/tests/slashing/hypothesis_adversarial_scheduler.rs` | Partitions, gossip, reports, pruning, and proposers remain in documented buckets.  |
| UC-84 | `casper/tests/slashing/hypothesis_liveness_as_safety.rs` | Finite liveness bounds fail only when proposer fairness is absent.                 |
| UC-85 | `casper/tests/slashing/hypothesis_arithmetic_projection_stress.rs` | Exact arithmetic and fixed-width projections diverge only at documented boundaries. |
| UC-86 | `casper/tests/slashing/hypothesis_assumption_weakening.rs` | Dropping each documented precondition reproduces the expected counterexample.      |
| UC-87 | `casper/tests/slashing/hypothesis_persistent_corpus.rs` | Persistent Hypothesis corpus runs accumulate examples without changing deterministic quick/deep expectations. |
| UC-88 | `casper/tests/slashing/hypothesis_objective_guided_frontier.rs` | Objective-guided campaign scoring returns only documented boundary, projection, assumption, or bisimilar classes. |
| UC-89 | `casper/tests/slashing/hypothesis_rust_replay_fixtures.rs` | Formal-oracle, fixed-Rust, and Scala/projection replay fixtures match the expected classification. |
| UC-90 | `casper/tests/slashing/hypothesis_partition_gossip_state_machine.rs` | Partition, gossip, merge, report, pruning, and proposer traces remain in documented buckets. |
| UC-91 | `casper/tests/slashing/hypothesis_rust_metamorphic_checks.rs` | Edge order, duplicate edges, validator renaming, and record-hash normalization remain metamorphic-safe in Rust-facing fixtures. |
| UC-92 | `casper/tests/slashing/hypothesis_precondition_fuzzing.rs` | Dropped theorem and projection preconditions classify as boundary, projection-risk, or assumption-counterexample witnesses. |
| UC-93 | `casper/tests/slashing/deep_sage_graph_threat_model.rs` | Reverse-reachability chains to direct offenders reproduce the documented closure and path certificates. |
| UC-94 | `casper/tests/slashing/deep_sage_stake_damage_optimization.rs` | MIP/fallback stake-damage witnesses remain outside the weighted closure-bound precondition. |
| UC-95 | `casper/tests/slashing/deep_sage_retention_pruning.rs` | Retention windows below the slash delay lose slashability; minimum safe windows preserve closure. |
| UC-96 | `casper/tests/slashing/deep_sage_epoch_churn_identity.rs` | Strict epoch identity filters stale evidence unless an explicit loose/carryover policy is enabled. |
| UC-97 | `casper/tests/slashing/deep_sage_economic_safety_envelopes.rs` | Arithmetic envelopes accept safe totals and reject fixed-width overflow projections. |
| UC-98 | `casper/tests/slashing/minimal_counterexample_catalog_replay.rs` | The minimal counterexample catalog replays every documented assumption and projection witness. |
| UC-99 | `casper/tests/slashing/threat_vector_ranking_priorities.rs` | Threat-vector ranking keeps projection risks above assumption counterexamples and policy boundaries. |
| UC-100 | `casper/tests/slashing/hypothesis_rust_replay_fixtures.rs` | Defensive adversarial campaign fixtures stay classified across Rust latest-message DAG detectability, multi-node view, projection, and objective witnesses. |
| UC-101 | `casper/tests/slashing/uc_101_detector_missing_nested_pointer.rs` | Missing nested offender pointers contribute `∅` and cannot abort the detector. |
| UC-102 | `casper/tests/slashing/uc_102_detector_order_independence.rs` | Latest-message traversal order cannot change the detector verdict. |
| UC-103 | `casper/tests/slashing/uc_103_detector_preconditioned_bisim.rs` | Complete-pointer latest-message views preserve the pre-fix/Scala verdict. |
| UC-104 | `casper/tests/slashing/uc_104_detector_no_unsafe_lookup.rs` | Missing direct block-store entries are skipped, not treated as fatal detector errors. |
| UC-105 | `casper/tests/slashing/uc_105_detector_detected_hash_order.rs` | A previously detected hash remains decisive regardless of traversal position. |
| UC-106 | `casper/tests/slashing/uc_106_detector_two_child_order.rs` | Neglect requires two distinct offender-child hashes, not two paths. |
| UC-107 | `casper/tests/slashing/uc_107_detector_validator_churn.rs` | Validator-set churn plus incomplete pointers is deterministic and non-exploitable. |
| UC-108 | `casper/tests/slashing/uc_108_detector_duplicate_child.rs` | Duplicate paths to one child do not create a false neglected-equivocation verdict. |
| UC-109 | `casper/tests/slashing/frontier_monotonicity_merge_basis.rs` | Adding evidence cannot shrink fixed-universe closure; merged views over-approximate inputs; minimal slash bases stay necessary; detector traversal is bounded on cycles; contribution order is stable; fixed-point replay is idempotent; report retention prevents edge reactivation; no-seed cycles stay unslashed; slash history matches closure prefixes; edge orientation is enforced; redundant paths raise denial cost; unsupported slash targets do not authorize slashing; reports are pair-scoped; report growth cannot expand closure; reports do not remove direct evidence; validator renaming is equivariant; bisimilarity deltas are classified. |
| UC-110 | `casper/tests/slashing/horizon_search_fixtures.rs` | Cross-coupled horizon fixtures keep retention ≥ gossip + inclusion delay, proposer fairness explicit, detector contribution gates total and distinct-child based, stale rebond evidence epoch-filtered, weighted damage outside the closure-bound precondition, merged views over-approximating inputs, checked arithmetic rejecting wrapping projections, pair-scoped reports, and edge-order/duplicate evidence metamorphic. |
| UC-111 | `casper/tests/slashing/horizon_v2_search_fixtures.rs` | Rust-aligned horizon-v2 fixtures keep detector DAG contribution gates total and order-independent, detected-hash records retained until dependency checks complete, finality-aware retention ≥ finality depth + gossip + inclusion delay, weighted damage and evidence-denial cost explicit, stale era evidence filtered, and generated differential rows classified. |
| UC-112 | `casper/tests/slashing/uc_112_record_lifecycle_retention.rs` | The production detector update path retains existing detected hashes when a later block also detects the same equivocation, proving Finding 96 is not reproduced by current Rust. |

## 14.4 Property-based tests

Property-based tests run thousands of randomized inputs per
property and shrink failures to minimal counter-examples. Each
property below corresponds to a Rocq theorem.

The Sage/Hypothesis frontier suite also runs less-directed searches for
novelty/coverage, generated multi-epoch traces, exact-vs-projection
differentials, production-shaped DAG traces, semantic attack campaigns,
attack objectives, objective-guided scoring, objective-frontier fixture
selection, defensive adversarial vulnerability campaigns,
exact-vs-runtime projection matrices, differential-oracle replay rows,
mutation/metamorphic properties, Rust metamorphic and replay fixtures,
bundle-based state machines, partition/gossip state machines,
adversarial schedulers, liveness-as-safety checks, arithmetic projection
stress, assumption minimization, assumption weakening, precondition
fuzzing, Rust differential corpus generation, and automatic trace
classification, evidence-addition monotonicity, view-merge confluence,
minimal slash-basis extraction, record-key namespace projection,
cross-coupled horizon campaigns, and Rust-aligned horizon-v2 detector
DAG/lifecycle campaigns.
The v3 search horizon adds feedback-directed targets for uncovered Rust
features, detector traversal depth, retention-window boundaries,
stake-damage Pareto points, replay divergence classes, public
equivocation-detector classification paths, and candidate-to-SlashDeploy
lifecycle validation.
Mutable evidence, bundle, multi-epoch,
partition/gossip, campaign, horizon, and horizon-v2 checks use Hypothesis rule-based state
machines. Any new failure from that suite must be reduced to a
deterministic Sage witness before it is promoted to a Rocq theorem,
TLA+ invariant, or normative use case.

### 14.4.1 Detection properties (T-1, T-2, T-3, T-6)

```rust
proptest! {
    /// T-1 — Detection soundness. No honest validator wrongly classified.
    #[test]
    fn prop_detection_sound(
        dag in gen_dag_state(3, 10),
        b in gen_block_in(&dag),
    ) {
        let verdict = detect(&dag, &b);
        if verdict == Status::AdmissibleEquivocation {
            // There must exist a real equivocation witness.
            prop_assert!(equivocates_witness(&dag, b.sender, b.seq).is_some());
        }
    }

    /// T-2 — Detection completeness. Every real equivocation is detected.
    #[test]
    fn prop_detection_complete(
        dag in gen_dag_state(3, 10),
        (v, b1, b2) in gen_equivocation(&dag),
    ) {
        let verdict = detect(&dag, &b2);
        prop_assert!(matches!(verdict, Status::AdmissibleEquivocation | Status::IgnorableEquivocation));
    }

    /// T-3 — Slashable-set extension under post-fix #1.
    #[test]
    fn prop_slashable_extends(verdict in any::<InvalidBlock>()) {
        prop_assert_eq!(
            is_slashable_post_fix(verdict),
            is_slashable_pre_fix(verdict) || verdict == InvalidBlock::IgnorableEquivocation
        );
    }

    /// T-6 — Neglect detection sound + complete.
    #[test]
    fn prop_neglect_detection(
        dag in gen_dag_state(3, 10),
        b in gen_block_in(&dag),
    ) {
        let neglected = detect_neglected(&dag, &b);
        if neglected {
            // There must exist a justification j with invalid latest msg + bonded validator.
            prop_assert!(b.justifications.iter().any(|j|
                dag.lookup(j.latest_block_hash).map_or(false, |bl| bl.invalid)
                && dag.bonds_map[j.validator] > 0
            ));
        }
    }
}
```

### 14.4.2 Storage properties (T-4, T-5, T-9.2)

```rust
proptest! {
    /// T-4 — Record monotonicity. Hashes are never lost.
    #[test]
    fn prop_record_monotone(
        store in gen_eq_store(),
        key in gen_eq_key(),
        h in gen_block_hash(),
    ) {
        let before = hashes_at_key(&store, &key);
        let after = update_record(&store, key, h);
        prop_assert!(before.is_subset(&hashes_at_key(&after, &key)));
    }

    /// T-5 — Record-key uniqueness. Update preserves unique keys.
    #[test]
    fn prop_record_unique(
        store in gen_eq_store(),
        key in gen_eq_key(),
        h in gen_block_hash(),
    ) {
        let after = update_record(&store, key.clone(), h);
        let count = after.iter().filter(|r| r.key == key).count();
        prop_assert_eq!(count, 1);
    }

    /// T-5N — Record equivalence ignores hash order and duplicate witnesses.
    #[test]
    fn prop_record_normalization(
        records in gen_equivocation_records_with_duplicates(),
    ) {
        let normalized = normalize_records(&records);
        prop_assert!(records_equiv_mod_hash_order_and_duplicates(&records, &normalized));
    }

    /// T-5K — Canonical record-key encoding is injective.
    #[test]
    fn prop_record_key_canonical_injective(
        k1 in gen_eq_key(),
        k2 in gen_eq_key(),
    ) {
        if canonical_record_key(&k1) == canonical_record_key(&k2) {
            prop_assert_eq!(k1, k2);
        }
        prop_assume!(k1 != k2);
        prop_assert_ne!(naive_record_key_projection(&k1), canonical_record_key(&k1));
    }

    /// T-9.2 — Atomic no-overwrite under arbitrary thread schedules.
    #[test]
    fn prop_atomic_no_overwrite(
        ops in gen_thread_schedule(4, 16),
    ) {
        let initial = EquivocationTrackerStore::default();
        let final_state = apply_schedule_locked(&initial, &ops);
        // Every hash inserted by any thread must be present in the final state.
        for op in &ops {
            if let Op::Update { key, hash } = op {
                prop_assert!(hashes_at_key(&final_state, key).contains(hash));
            }
        }
    }
}
```

### 14.4.3 Effect-layer properties (T-7, T-8, T-Idem, T-AuthCheck)

```rust
proptest! {
    /// T-7 — Slash zeros bond.
    #[test]
    fn prop_slash_zeros_bond(ps in gen_pos_state(), v in gen_validator()) {
        let (ps_post, _) = slash(ps.clone(), v.clone());
        prop_assert_eq!(ps_post.all_bonds.get(&v).copied().unwrap_or(0), 0);
    }

    /// T-8 — Slash transfers stake (when transfer succeeds).
    #[test]
    fn prop_slash_transfers_stake(ps in gen_pos_state(), v in gen_validator()) {
        let pre_bond = ps.all_bonds.get(&v).copied().unwrap_or(0);
        let pre_vault = ps.coop_vault_balance;
        let (ps_post, ok) = slash(ps, v);
        if ok {
            prop_assert_eq!(ps_post.coop_vault_balance, pre_vault + pre_bond);
        } else {
            prop_assert_eq!(ps_post.coop_vault_balance, pre_vault);  // unchanged on failure
        }
    }

    /// T-Idem — Slash idempotence.
    #[test]
    fn prop_slash_idempotent(ps in gen_pos_state(), v in gen_validator()) {
        let (ps1, _) = slash(ps, v.clone());
        let (ps2, _) = slash(ps1.clone(), v);
        prop_assert_eq!(ps1, ps2);
    }

    /// T-IdemFail — Partial batch failure must be atomic or explicitly order-dependent.
    #[test]
    fn prop_batch_slash_failure_atomicity(
        ps in gen_pos_state(),
        slash_set in gen_slash_set(),
        failing in gen_validator(),
    ) {
        let outcomes = all_batch_orders(&slash_set)
            .map(|order| slash_batch_abort_on_failure(ps.clone(), &order, failing.clone()))
            .collect::<BTreeSet<_>>();
        prop_assert!(outcomes.len() == 1 || batch_slash_is_atomic_policy());
    }

    /// T-AuthCheck — Spoofed auth token rejected.
    #[test]
    fn prop_auth_check(
        ps in gen_pos_state(),
        v in gen_validator(),
        bad_token in gen_random_bytes(32),
    ) {
        let result = slash_with_auth(ps.clone(), v, bad_token);
        prop_assert!(result.is_err());
        prop_assert_eq!(result.unwrap_err(), SlashError::InvalidAuthToken);
    }
}
```

### 14.4.4 Two-level closure properties (T-11, T-12)

```rust
proptest! {
    /// T-11 — Level-2 closure terminates within |V| iterations.
    #[test]
    fn prop_closure_terminates(
        universe in gen_validator_set(2..=10),
        graph in gen_neglect_graph(&universe),
    ) {
        let initial = direct_equivocators(&universe, &graph);
        let result = slash_iter(&universe, &graph, &initial, universe.len());
        prop_assert!(result.is_subset(&universe));
    }

    /// T-12 — Under |closure| ≤ F, BFT-quorum preserved.
    #[test]
    fn prop_t12_bft_preserves_quorum(
        n in 4_usize..=10,
        graph_seed in any::<u64>(),
    ) {
        let universe = (0..n).collect();
        let graph = gen_bounded_neglect_graph(&universe, /* max closure */ n / 3);
        let initial = direct_equivocators(&universe, &graph);
        let closure = slash_iter(&universe, &graph, &initial, universe.len());
        // BFT precondition: |closure| ≤ ⌊(n-1)/3⌋
        prop_assume!(closure.len() <= (n - 1) / 3);
        let active = universe.difference(&closure);
        let bft_quorum_lower_bound = n - (n - 1) / 3;
        prop_assert!(active.count() >= bft_quorum_lower_bound);
    }

    /// T-12W — Under weighted closure stake ≤ F_stake, weighted quorum is preserved.
    #[test]
    fn prop_t12_weighted_preserves_quorum(
        stakes in gen_stake_map(4..=10),
        graph_seed in any::<u64>(),
    ) {
        let universe = stakes.keys().cloned().collect();
        let graph = gen_weighted_bounded_neglect_graph(&universe, &stakes);
        let initial = direct_equivocators(&universe, &graph);
        let closure = slash_iter(&universe, &graph, &initial, universe.len());
        let total: u128 = stakes.values().sum();
        let fault = (total.saturating_sub(1)) / 3;
        let slashed: u128 = closure.iter().map(|v| stakes[v]).sum();
        prop_assume!(slashed <= fault);
        prop_assert!(total - slashed >= total - fault);
    }

    /// T-12F — Stale/off-era offenders are filtered before closure.
    #[test]
    fn prop_current_validator_filtering(
        current in gen_validator_set(3..=10),
        evidence in gen_evidence_domain_with_stale(&current),
        graph in gen_neglect_graph_over_evidence(&evidence),
    ) {
        let closure = slash_iter_current_filtered(&current, &evidence, &graph);
        prop_assert!(closure.is_subset(&current));
    }

    /// T-12G — Closure depends on reachability, not duplicate edge multiplicity.
    #[test]
    fn prop_duplicate_edges_do_not_change_closure(
        universe in gen_validator_set(3..=10),
        graph in gen_neglect_graph(&universe),
    ) {
        let duplicated = duplicate_some_edges(&graph);
        let initial = direct_equivocators(&universe, &graph);
        prop_assert_eq!(
            slash_iter(&universe, &graph, &initial, universe.len()),
            slash_iter(&universe, &duplicated, &initial, universe.len())
        );
    }

    /// T-12I — Count and weighted active quorums intersect under the strict bounds.
    #[test]
    fn prop_active_quorums_intersect(
        stakes in gen_stake_map(4..=10),
        closure in gen_bounded_slash_closure(&stakes),
    ) {
        let active = active_after_closure(&stakes, &closure);
        for (q1, q2) in active_quorums(&active) {
            prop_assert!(!q1.is_disjoint(&q2));
        }
        for (q1, q2) in active_weighted_quorums(&active, &stakes) {
            prop_assert!(!q1.is_disjoint(&q2));
        }
    }

    /// T-12C/T-12D — Closure stabilizes and quorum drops have certificates.
    #[test]
    fn prop_closure_fixed_point_and_drop_certificate(
        universe in gen_validator_set(3..=10),
        graph in gen_neglect_graph(&universe),
    ) {
        let initial = direct_equivocators(&universe, &graph);
        let closure = slash_iter(&universe, &graph, &initial, universe.len());
        prop_assert_eq!(closure, slash_iter(&universe, &graph, &closure, 1));
        if active_below_quorum(&universe, &closure) {
            prop_assert!(quorum_drop_certificate(&universe, &closure).is_some());
        }
    }

    /// T-12E/T-12A — Epoch filtering and arithmetic envelopes constrain implementation projections.
    #[test]
    fn prop_epoch_filter_and_arithmetic_envelope(
        evidence in gen_epoch_tagged_evidence(),
        bonds in gen_bonds_map(),
    ) {
        prop_assert!(epoch_filtered_closure(&evidence).iter().all(|v| v.epoch == current_epoch()));
        prop_assume!(vault_plus_all_bonds_fits(&bonds, u128::MAX));
        prop_assert!(slash_accounting_projection_safe(&bonds, u128::MAX));
    }

    /// T-12V/T-12RPT/T-12EID/T-12RET — Views, reports, epoch identity, and retention are explicit.
    #[test]
    fn prop_view_report_epoch_retention_boundaries(
        view_a in gen_evidence_view(),
        view_b in gen_evidence_view_equivalent_or_divergent(),
        reports in gen_report_set(),
        epoch_evidence in gen_epoch_tagged_evidence(),
    ) {
        if active_edges(&view_a) == active_edges(&view_b) {
            prop_assert_eq!(closure_for_view(&view_a), closure_for_view(&view_b));
        }
        prop_assert!(active_edges_after_reports(&view_a, &reports).is_disjoint(&reports));
        prop_assert!(stale_epoch_evidence(&epoch_evidence).iter().all(|e| !eligible_current(e)));
        prop_assert!(retained_direct_evidence_precondition(&epoch_evidence));
    }

    /// T-12HYP/T-12AMP — Each theorem assumption has a documented counterexample when removed.
    #[test]
    fn prop_assumption_counterexamples_are_reproducible(
        witness in gen_slashing_assumption_counterexample(),
    ) {
        prop_assert!(witness.violates_claim_when_assumption_removed());
    }
}
```

### 14.4.5 Bisimilarity properties (T-13, T-14, T-15)

```rust
proptest! {
    /// T-13a — Bonds-bisimulation preserved under bm_slash.
    #[test]
    fn prop_t13a_bonds_bisim_preserved(
        b1 in gen_bonds_map(),
        b2 in gen_bonds_map_bisim(&b1),
        v in gen_validator(),
    ) {
        prop_assert!(bonds_bisim(&bm_slash(&b1, &v), &bm_slash(&b2, &v)));
    }

    /// T-13b — Records-bisimulation monotone under update.
    #[test]
    fn prop_t13b_records_bisim_monotone(
        s1 in gen_eq_store(), s2 in gen_eq_store_bisim(&s1),
        key in gen_eq_key(), h in gen_block_hash(),
    ) {
        let s1p = update_record(&s1, key.clone(), h.clone());
        let s2p = update_record(&s2, key, h);
        prop_assert!(records_bisim_strong(&s1p, &s2p));
    }

    /// T-13c — Fork-choice-bisimulation preserved under filter.
    #[test]
    fn prop_t13c_forkchoice_bisim_preserved(
        lm1 in gen_latest_messages(),
        lm2 in gen_latest_messages_bisim(&lm1),
        b1 in gen_bonds_map(),
        b2 in gen_bonds_map_bisim(&b1),
    ) {
        for v in all_validators() {
            prop_assert_eq!(
                fc_lookup(&filter_slashed(&lm1, &b1), &v),
                fc_lookup(&filter_slashed(&lm2, &b2), &v),
            );
        }
    }

    /// T-14 — Weak barbed equivalence.
    #[test]
    fn prop_t14_refl(s in gen_5_component_state()) {
        prop_assert!(weak_barbed_equiv(&s, &s));
    }
    #[test]
    fn prop_t14_sym(s1 in gen_5_component_state(), s2 in gen_5_component_state_bisim(&s1)) {
        prop_assert_eq!(weak_barbed_equiv(&s1, &s2), weak_barbed_equiv(&s2, &s1));
    }

    /// T-15 — Pipeline composition preserves R.
    #[test]
    fn prop_t15_pipeline_step_preserves(
        s1 in gen_5_component_state(),
        s2 in gen_5_component_state_bisim(&s1),
        offender in gen_validator(),
        base_seq in any::<u64>(),
        h in gen_block_hash(),
    ) {
        let s1p = pipeline_step(&s1, &offender, base_seq, &h);
        let s2p = pipeline_step(&s2, &offender, base_seq, &h);
        prop_assert!(weak_barbed_equiv(&s1p, &s2p));
    }
}
```

### 14.4.6 Bug-fix properties (T-9.1 through T-9.15; T-9.2 in §14.4.2)

One property per bug fix, each testing the `t_9_M_*` Rocq lemma.
T-9.2 (atomic no-overwrite) is grouped with the other storage
properties in §14.4.2 above. The core bug-fix property set covers
#1, #3–#11, and the authorization regression suite in §14.12 covers
#12–#16:

```rust
/// T-9.1 — Post-fix ignorable implies real equivocation.
#[test]
fn prop_t91_ignorable_safety(...) { ... }

/// T-9.3 — Dispatcher routes every is_slashable variant.
#[test]
fn prop_t93_dispatch_complete(...) { ... }

/// T-9.4 — Transfer-failure deterministic outcome.
#[test]
fn prop_t94_transfer_failure_safety(...) { ... }

/// T-9.5 — slash preserves active-implies-bonded invariant.
#[test]
fn prop_t95_slash_preserves_invariant(...) { ... }

/// T-9.6 — Self-regression DAG-level detection.
#[test]
fn prop_t96_self_regression_in_dag(...) { ... }

/// T-9.7 — canonical self-chain child handles seq-num gaps.
#[test]
fn prop_t97_finds_descendant_with_gap(...) { ... }

/// T-9.8 — Unbonded proposer no-emit (post-fix when applied; positive companion: bonded equals pre-fix).
#[test]
fn prop_t98_unbonded_proposer_no_slash(...) { ... }

/// T-9.9 — Post-fix rejection iff neglected ∧ ¬has_slash.
#[test]
fn prop_t99_post_fix_rejection_iff(...) { ... }

/// T-9.10 — Failed withdrawal leaves the withdrawer obligation retryable.
#[test]
fn prop_t910_withdraw_failure_retryable(...) { ... }

/// T-9.11 — Missing pointers are non-contributing and duplicate children are idempotent.
#[test]
fn prop_t911_detector_total_and_distinct_child(...) { ... }

/// T-9.12/T-9.13 — Slash deploys require current-epoch authorized evidence.
#[test]
fn prop_t912_current_epoch_authorization(...) { ... }

/// T-9.14 — Fixed-width sequence arithmetic is checked.
#[test]
fn prop_t914_checked_sequence_arithmetic(...) { ... }

/// T-9.15 — Duplicate justifications are rejected before detector projection.
#[test]
fn prop_t915_duplicate_justification_rejected(...) { ... }
```

## 14.5 Cross-implementation tests (Rust ↔ Rocq-mirrored oracle)

Bisimilarity is the headline claim (T-15). Tests assert that the
Rust implementation and the Rocq oracle produce identical
observable state transitions across randomized event sequences.

```rust
proptest! {
    /// For every randomized event sequence, the Rust harness and the Rocq
    /// oracle (hand-mirrored from the Rocq definitions) agree on all five
    /// projection components (BondMap, EqRecords, SlashedSet, CoopVault,
    /// ForkChoiceLatestMessages).
    #[test]
    fn prop_rust_vs_rocq_bisim(events in gen_event_sequence(50)) {
        let mut rust_state = SlashingTestHarness::new(3, 100);
        let mut rocq_state = RocqOracle::new(3, 100);
        for event in events {
            apply_to_rust(&mut rust_state, &event);
            apply_to_rocq(&mut rocq_state, &event);
            prop_assert!(weak_barbed_equiv(&project(&rust_state), &project(&rocq_state)));
        }
    }
}
```

## 14.6 TLA+ model-check integration

The verification doc §10 enumerates the named TLA+ invariants used
to model-check the slashing subsystem; §14 references **40+** of
them, including the Sage-promoted two-level invariants, the
rewrite-introduced `Inv_RecordHasWitness`, and the authorized
slash-flow invariants. Each bounded post-fix configuration is
re-checked via `tlc` as part of CI. The exhaustive detector safety
configuration is opt-in:

```bash
# CI script lives on `analysis/slashing` (`scripts/ci/check-tla-invariants.sh`)
# alongside the TLA+ sources themselves.
tlc -workers auto -config MC_AuthorizedSlashFlow.cfg MC_AuthorizedSlashFlow.tla
tlc -workers auto -config MC_JustificationProjection.cfg MC_JustificationProjection.tla
RUN_EXHAUSTIVE_TLA=1 bash scripts/ci/check-tla-invariants.sh
```

Specifically:
- `MC_EquivocationDetectorEager.cfg` — `Inv_DetectionSound`,
  `Inv_TaxonomyCorrect`, `Inv_RecordHasWitness`, `Inv_LivenessAsSafety`.
- `MC_ConcurrentTracker.cfg` — `Inv_NoOverwrite`,
  `Inv_RecordMonotone` (Locked=⊤).
- `MC_SlashFlow.cfg` — `Inv_BondsZeroAfterSlash`,
  `Inv_ForfeitedToCoopVault`, `Inv_StakeConservation`,
  `Inv_SlashedExcludedFromFC`, `Inv_SlashedRemoved`.
- `MC_TwoLevelSlashing.cfg` — `Inv_LevelClosureTerminates`,
  `Inv_ActiveSetAboveQuorum`, `Inv_ActiveStakeAboveWeightedQuorum`,
  `Inv_FilteredClosureInCurrentValidators`,
  `Inv_NeglectEdgesVisibleUnreported`,
  `Inv_NoUnexpectedDifferentialDivergence`,
  `Inv_ActiveQuorumsIntersect`,
  `Inv_ActiveStakeQuorumsIntersect`,
  `Inv_ClosureStableAtMaxLevel`,
  `Inv_EpochEligibleInCurrent`,
  `Inv_StaleEvidenceNotEligible`,
  `Inv_ReportsSuppressNeglectEdges`,
  `Inv_ArithmeticSafeEnvelope`,
  `Inv_ViewEdgesVisibleUnreported`,
  `Inv_SameViewSameClosure`,
  `Inv_ValidatorRenamingEquivariance`,
  `Inv_CarryoverPolicyCurrent`,
  `Inv_NoCarryoverNoMappedDirect`,
  `Inv_EvidenceRetentionForDirectOffenders`,
  `Inv_CanonicalRecordKeyInjective`,
  `Inv_BatchNoFailureOrderIndependent`,
  `Inv_PartialBatchFailureRequiresAtomicPolicy`,
  `Inv_ProposerFairnessForBoundedLiveness`,
  `Inv_UnsignedArithmeticBoundary`, and
  `Inv_SignedArithmeticBoundary`.
- `MC_AuthorizedSlashFlow.cfg` — `Inv_OnlyAuthorizedSlashCanBePending`,
  `Inv_StaleEvidenceCannotSlashRebondedKey`,
  `Inv_NoInvalidLatestLivenessGap`,
  `Inv_RejectedSlashWithoutEvidenceNoPending`,
  `Inv_InvalidAuthSlashNoPending`, and
  `Inv_BondsZeroAfterSlash`.
- `MC_JustificationProjection.cfg` —
  `Inv_DuplicateJustificationsRejected`,
  `Inv_AcceptedImpliesUniqueJustifications`, and
  `Inv_AcceptedProjectionCardinality`.

A TLC violation immediately fails CI.
`MC_EquivocationDetector_safety.cfg` is the exhaustive detector
safety run; it is intentionally excluded from the default PR-gate
script until the shorter frontier has stabilized, and is enabled by
`RUN_EXHAUSTIVE_TLA=1`.

### 14.6.1 Trace replay against the Rust harness

In addition to running TLC against each `MC_*.cfg`, the suite drives
hand-curated TLC schedules through the Rust `SlashingTestHarness`
via the trace-replay infrastructure at:
- `casper/tests/slashing/tla_projection.rs` — projection from each
  TLA+ Action symbol (e.g. `SignHonest`, `SignEquivocating`,
  `ExecuteSlash`, `WithdrawSucceeds`) to the corresponding harness
  operation.
- `casper/tests/slashing/tla_trace_replay.rs` — five `#[test]`s,
  one per spec, each loading a JSON trace from
  `casper/tests/slashing/tla_traces/*.json` and asserting the
  harness's projected final state matches the TLA+ model's
  expected post-state for that schedule.
- The sanity-check + workflow doc for regenerating traces from TLC
  counter-examples is `scripts/ci/dump-tla-traces.sh`.

Five spec-trace pairs are checked in:
- `MC_EquivocationDetector` ↔ `mc_equivocation_detector.json`
- `MC_ConcurrentTracker`    ↔ `mc_concurrent_tracker.json`
- `MC_SlashFlow`            ↔ `mc_slash_flow.json`
- `MC_TwoLevelSlashing`     ↔ `mc_two_level_slashing.json`
- `MC_WithdrawFlow`         ↔ `mc_withdraw_flow.json`

Run via `cargo test -p casper --test mod -- slashing::tla_trace_replay`.

This complements the property-based triple-bisim tests
(`prop_t_triple_bisim_*`) by pinning specific TLC-exercisable
schedules so a regression that only surfaces along that exact
schedule is caught deterministically. The two layers are
complementary:
- Property tests sample a random region of the trace space.
- TLA+ trace replay locks in the canonical schedules a model
  checker would explore exhaustively at small scales.

## 14.7 Pre-fix regression tests (one per bug, #1–#11)

These tests are the **backstop**: they reproduce the pre-fix
counter-example for each bug. If a future PR accidentally
re-introduces the pre-fix behaviour, the regression test fires.

**Out-of-band approach.** The bug-fix commits land sequentially —
one bug per commit (or one bundle per logically-paired bugs, e.g.
#1 + #3) — so each pre-fix counter-example is observable by
checking out the parent of the fix commit and re-running the
post-fix UC test. We do **not** carry pre-fix code paths into the
production build (no Cargo feature gating, no `#[cfg(...)]` shims)
because:

- The pre-fix code is the bug itself; keeping it compiled is dead
  weight that drifts as the surrounding code evolves.
- Two parallel paths gated by features double the test surface
  for every fix and obscure which behaviour is the canonical one.
- `git log` plus `cargo test` against a parent commit is the same
  verification with zero source-tree clutter.

**File-naming convention.** When a regression test is needed,
place it at `casper/tests/slashing/pre_fix_bug_<N>.rs`. Each file
contains a single `#[test]` that:

1. Constructs the smallest counter-example trace for the bug.
2. Asserts the *post-fix* invariant that the bug violated (the
   test would have failed on the parent commit).

Three of the eleven pre-fix tests share their counter-example trace
with a UC entry in §14.3.2 (UC-41 = bug #1, UC-42 = bug #3, UC-43
= bug #7); for those, the `pre_fix_bug_<N>.rs` file simply imports
and runs the same trace as a sanity backstop. The other eight
(bugs #2, #4, #5, #6, #8, #9, #10, #11) have no UC-numbered positive
counterpart and exist only as bug-specific regression tests.

```rust
// casper/tests/slashing/pre_fix_bug_1.rs
#[test]
fn pre_fix_bug_1_ignorable_dos() {
    let mut harness = SlashingTestHarness::new(3, 100);
    let _b1 = harness.sign_block("A", 5);
    let b1p = harness.sign_block_distinct("A", 5);
    // No other block cites b1p (unsolicited).
    assert_eq!(harness.detect(b1p), Status::IgnorableEquivocation);
    // Post-fix invariant: the dispatcher mints an EquivocationRecord
    // so the proposer can issue a SlashDeploy. (Pre-fix this assert
    // failed because the variant was non-slashable and the dispatcher
    // returned Ok(dag.clone()) with no record; running this test
    // against the parent commit reproduces the bug.)
    assert!(harness.has_record("A", 4));
}
```

## 14.8 Coverage criteria

The test suite is considered **exhaustive** when:

1. **Use-case coverage:** Every UC-NN in spec §12 has a passing
   integration test (112 tests).
2. **Theorem coverage:** Every theorem in spec/verification has at
   least one property-based test that fails on a property violation.
   §14.4 covers **49 distinct theorem labels**: T-1, T-2, T-3, T-4,
   T-5, T-6, T-7, T-8, T-Idem, T-11, T-12, T-13a, T-13b, T-13c,
   T-14, T-15a, T-15b, T-AuthCheck, T-9.1, T-9.2, T-9.3, T-9.4,
   T-9.5, T-9.6, T-9.7, T-9.8, T-9.9, T-9.10, T-9.11, T-12R, T-12W, T-12F, T-12G,
   T-12I, T-12C, T-12D, T-12E, T-12A, T-12V, T-12RPT,
   T-12EID, T-12HYP, T-12AMP, T-12RET, T-12PF, T-5N, T-5K, T-5DF,
   T-IdemMany, T-IdemFail, T-15D.
   T-10 (`fork_choice_exclusion`) is exercised by example test UC-01
   rather than a dedicated property test.
3. **InvalidBlock variant coverage:** All 17 pre-fix slashable +
   1 post-fix slashable = 18 slashable variants are exercised
   (Tier B + Core + Tier A tests).
4. **TLA+ invariant coverage:** All cited invariants have a
   passing model-check run.
5. **Pre-fix counter-example coverage:** Every documented bug
   (#1–#11) has a regression test at
   `casper/tests/slashing/pre_fix_bug_<N>.rs` whose assertions
   would fail against the parent of the fix commit (out-of-band
   verification per §14.7).
6. **Concurrency coverage:** The atomic-tracker property test
   (T-9.2) runs over schedules of length 1, 2, 4, 8, 16 with
   thread counts 2, 4, 8.
7. **Boundary coverage:** UC-26 (`|closure| > F`) is exercised
   for n ∈ {4, 7, 10, 13} to confirm BFT-bound semantics.
8. **Mutation coverage:** Run `cargo mutants` against the slashing
   modules; surviving mutants indicate test-suite weakness.

## 14.9 Manual Runbook

The slashing test jobs are documented as manual commands, not active GitHub
workflow jobs. On an Ubuntu runner or developer machine, use this common
setup before running the Rust-backed jobs:

```sh
rustup toolchain install nightly-2026-02-09
rustup default nightly-2026-02-09
sudo apt-get update
sudo apt-get install -y protobuf-compiler libssl-dev pkg-config
```

Manual equivalent of the former **example-based** job:

```sh
cargo test -p casper -- uc_
cargo test -p casper -- slashing::integration_t_
```

Static UC traceability sanity check:

```sh
for i in $(seq 1 112); do
  if [ "$i" -lt 100 ]; then uc=$(printf "%02d" "$i"); else uc="$i"; fi
  rg -q "(UC-${uc}|uc_${uc}(_|\\b))" casper/tests/slashing || echo "missing UC-${uc}"
done
```

Manual equivalent of the former **property-based** job:

```sh
PROPTEST_CASES=10000 cargo test --release -p casper -- slashing::prop_t_
```

Manual equivalent of the former **pre-fix-regressions** matrix job:

```sh
for bug in 1 2 3 4 5 6 7 8 9 10 11; do
  cargo test -p casper -- "slashing::pre_fix_bug_${bug}"
done
```

Manual equivalent of the former **loom-interleavings** job:

```sh
LOOM_MAX_PREEMPTIONS=3 LOOM_LOG=off cargo test --release -p casper -- slashing::loom_t_9_2
```

Manual equivalent of the former **tla-model-check** job (TLA+ sources +
`scripts/ci/check-tla-invariants.sh` are in this repository):

```sh
sudo apt-get update
sudo apt-get install -y default-jre wget
mkdir -p ~/.tla
wget -qO ~/.tla/tla2tools.jar \
  https://github.com/tlaplus/tlaplus/releases/latest/download/tla2tools.jar
TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" \
bash scripts/ci/check-tla-invariants.sh
```

The exhaustive detector safety check is intentionally opt-in because it can
run for many hours:

```sh
RUN_EXHAUSTIVE_TLA=1 TLA_TOOLS_JAR="$HOME/.tla/tla2tools.jar" \
  bash scripts/ci/check-tla-invariants.sh
```

Manual equivalent of the former **rocq-build** job:

```sh
sudo apt-get update
sudo apt-get install -y opam
opam init --disable-sandboxing -y
eval "$(opam env)"
opam install -y coq
make -C formal/rocq/slashing -j1
! grep -rn "Axiom\|Admitted\|Parameter\|Conjecture" \
  formal/rocq/slashing/theories/
```

Manual equivalent of the former **mutation-coverage** scheduled/manual job:

```sh
cargo install --locked cargo-mutants
cargo mutants --in-place --no-shuffle --timeout 120 --baseline=skip
```

Manual equivalent of the former **nightly-extended-proptest**
scheduled/manual job:

```sh
PROPTEST_CASES=100000 cargo test --release -p casper -- slashing::prop_t_
```

Manual production-integration run:

```sh
cargo test -p casper -- slashing::integration_t_
PROPTEST_CASES=25 cargo test -p casper -- slashing::prop_t_triple_bisim
```

Manual search-horizon fuzz and Kani runs are documented in
`docs/theory/slashing/slashing-search-horizon.md` (preserved on
`analysis/slashing`).

## 14.10 Test-development priorities

When implementing the suite, the recommended order is:

1. **Phase A (week 1):** Build the `SlashingTestHarness` API
   (§14.2.1). Implement UC-01 (the canonical example) end-to-end.
   Validates the harness against the simplest scenario.

2. **Phase B (week 2):** Implement Core scenarios UC-02 through
   UC-25 (24 tests). These cover the headline pipeline.

3. **Phase C (week 3):** Implement Tier B slashable-variant
   completion UC-28 through UC-36 (9 tests). These exercise the
   dispatcher under post-fix #3.

4. **Phase D (week 4):** Implement Tier A audit-blocker tests
   UC-26, UC-27, UC-37–UC-43 (8 tests). These close the §10.8
   verification-doc findings.

5. **Phase E (week 5):** Implement Tier C operational/adversarial
   tests UC-40, UC-44–UC-112 (70 tests).

6. **Phase F (weeks 6–7):** Implement property-based tests for
   theorems/properties T-1 through T-15a/b, T-Idem, T-AuthCheck,
   T-9.1–T-9.15
   (incl. T-9.10' and T-9.10″) plus
   T-12R/T-12W/T-12F/T-12G/T-12I/T-12C/T-12D/T-12E/T-12A,
   T-5N, T-5K, T-5DF, T-IdemMany, T-IdemFail, T-12V/T-12RPT/T-12EID,
   T-12HYP/T-12AMP/T-12RET/T-12PF, and T-15D (52 properties; T-10 is
   example-tested in UC-01; T-9.12–T-9.15 are covered by the
   authorization regression module in §14.12).

7. **Phase G (week 8):** Integrate cross-implementation
   bisimilarity tests (Rust ↔ Rocq-mirrored oracle) and TLA+
   model-check CI.

Each phase ends with all its tests passing in CI. Phases B–E may
proceed in parallel after Phase A.

## 14.11 Test-suite metrics

| Metric                                    | Target                                           |
|-------------------------------------------|--------------------------------------------------|
| Example-based test count                  | 114+ (use cases plus authorization regressions)  |
| Property-based test count                 | 56+ (one per theorem family; T-10 by example test UC-01) |
| Pre-fix counter-example test count        | 11 (one per pre-authorization bug #1–#11)        |
| Cross-implementation bisim test count     | 1 (parameterized)                                |
| TLA+ invariant model-check coverage       | 40+ (all enumerated in §14.6 plus authorization flow) |
| Mutation-test surviving-mutants threshold | < 5 % of mutants survive                         |
| Property-test cases per property          | 10,000 (CI run)                                  |
| End-to-end CI runtime                     | < 30 minutes                                     |

## 14.12 Authorization Regression Tests

The post-2026 vulnerability fixes add the
`slash_authorization_regressions` integration module.

| Test | Covers |
| --- | --- |
| `stale_invalid_evidence_is_not_an_authorized_slash_candidate` | Same-key/stale-evidence mitigation: old epoch evidence is not proposed. |
| `current_epoch_invalid_evidence_is_authorized_once_per_offender` | Invalid-index candidate generation and per-offender deduplication. |
| `received_stale_slash_deploy_is_rejected_before_replay` | Received slash deploy authorization before Rholang replay. |
| `duplicate_justification_validators_are_invalid` | Duplicate justifications are rejected before detector projection. |
| `checked_sequence_arithmetic_rejects_boundaries` | Checked `seq − 1` and proposer `seq + 1` boundary behavior. |
| `unauthorized_slash_status_is_slashable` | Unauthorized slash deploys are slashable proposer faults. |

These tests are example-based and property-based regressions. The matching formal coverage is in
`ValidatorLifetime.v`, `BugFixSlashAuthorization.v`,
`BugFixSeqArithmetic.v`, `BugFixDuplicateJustifications.v`, and
`AuthorizedSlashFlow.tla`. Duplicate-justification projection is also
checked by `JustificationProjection.tla`.

| Property | Covers |
| --- | --- |
| `checked_next_seq_matches_i32_successor` | T-9.14 `seq + 1` agrees with mathematical successor when representable and rejects overflow. |
| `checked_base_seq_rejects_nonpositive` | T-9.14 `seq − 1` rejects nonpositive record-key domains. |
| `checked_base_seq_matches_positive_i32_predecessor` | T-9.14 `seq − 1` agrees with mathematical predecessor when representable. |
| `epoch_for_block_number_matches_floor_division` | T-9.12/T-9.13 epoch projection for valid block-number domains. |
| `epoch_for_block_number_rejects_invalid_domains` | T-9.12/T-9.13 rejection of negative block numbers and non-positive epoch lengths. |

## 14.13 What this test plan does *not* cover

- **Network-layer tests** (gossip, peer discovery) — covered by
  `comm/` test suite separately.
- **End-to-end shard tests** — covered by `system-integration/`
  separately.
- **Performance/throughput benchmarks** — covered by
  `casper/benches/` separately.
- **Validator key-management tests** — covered by `crypto/` test
  suite.
- **Genesis-bootstrap tests** — covered by `node/tests/`
  separately.

These adjacencies are the responsibility of other test suites; the
slashing test plan here covers the slashing subsystem only.

---

**End of design document set.** Return to [README](README.md).
