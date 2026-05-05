# 14 · Test Plan

**Exhaustive test plan covering every use case (54 scenarios) and
every change documented in the design and verification doc set
(across the spec, verification, design, and diagrams).** This
document specifies
both **example-based** (concrete-trace) tests and **property-based**
(invariant) tests, organized by component layer and theorem family.
Implementing the harness and the 54 + N tests is out of scope for
this document; the test specifications below are normative for
whoever implements them.

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

The test plan **must catch regressions before they reach production**.
Concretely:
- Every Rocq-mechanized theorem (T-1 through T-15a/b, T-Idem,
  T-AuthCheck, T-9.1–T-9.9) maps to **at least one** Rust property
  test that fails if the property is violated at runtime.
- Every TLA+ invariant (`Inv_*`) maps to a Rust integration test
  that asserts the same invariant on a small randomized trace.
- Every documented bug (#1–#9) has a **pre-fix counter-example**
  test that fails on the pre-fix code path (proving the bug was
  real) and a **post-fix passing** test (proving the fix closes it).

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
theorem is the corresponding Rocq function (extracted to OCaml /
Rust via `Extraction`) or a hand-written Rust function that
mirrors the Rocq definition. Discrepancies between the harness
and the oracle indicate either:
- A bug in the harness (test infrastructure).
- A bug in the implementation (the property is violated).

The oracle for `slash` is `PoSContract.slash : PoSState → Validator
→ PoSState × bool`; the oracle for `detect` is
`EquivocationDetector.detect : DAGState → Block → DetectionStatus`.

## 14.3 Example-based tests (54 use cases)

The 54 use cases from spec §12 are mapped to Rust integration
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

The complete mapping of all 54 use cases:

### 14.3.1 Core scenarios (UC-01 through UC-25)

| #     | Test stub                                           | Asserts                                                                                                                  |
|-------|-----------------------------------------------------|--------------------------------------------------------------------------------------------------------------------------|
| UC-01 | `casper/tests/slashing/admissible_single.rs`        | A slashed; bond zero; vault += 100; A excluded from fork-choice (T-1, T-2, T-7, T-10).                                   |
| UC-02 | `casper/tests/slashing/admissible_multi.rs`         | f validators equivocate; all slashed; quorum preserved if `|closure| ≤ F` (T-1, T-12).                                   |
| UC-03 | `casper/tests/slashing/ignorable_fixed.rs`          | Unsolicited equivocation now slashed under post-fix #1 (T-9.1).                                                          |
| UC-04 | `casper/tests/slashing/two_level.rs`                | A equivocates → A slashed; B neglects → B slashed in same block (T-11, T-12).                                            |
| UC-05 | `casper/tests/slashing/justification_regression.rs` | JustificationRegression triggers EquivocationRecord under post-fix #3 (T-9.3).                                           |
| UC-06 | `casper/tests/slashing/self_regression.rs`          | Self-regression detected (Boolean predicate; T-9.6).                                                                     |
| UC-07 | `casper/tests/slashing/invalid_bonds_cache.rs`      | InvalidBondsCache slashed under post-fix #3 (T-9.3).                                                                     |
| UC-08 | `casper/tests/slashing/expired_deploy.rs`           | ContainsExpiredDeploy slashed (T-9.3).                                                                                   |
| UC-09 | `casper/tests/slashing/time_expired_deploy.rs`      | ContainsTimeExpiredDeploy slashed (T-9.3).                                                                               |
| UC-10 | `casper/tests/slashing/invalid_block_number.rs`     | InvalidBlockNumber slashed (T-9.3).                                                                                      |
| UC-11 | `casper/tests/slashing/stake_zero.rs`               | Stake-0 bonded validator: invariant unreachable post-fix #5; pre-fix counter-example fails detection (T-9.5).            |
| UC-12 | `casper/tests/slashing/tracker_race.rs`             | Concurrent insert: post-fix preserves both witnesses; pre-fix loses one (T-9.2).                                         |
| UC-13 | `casper/tests/slashing/transfer_failure.rs`         | Transfer-failure deterministic return; post-fix #4 returns `(false, "transfer failed")` (T-9.4).                         |
| UC-14 | `casper/tests/slashing/detect_crash.rs`             | Detector crash mid-RMW: schedule of length 1 with suspended write; record-monotonicity preserved (T-9.2 corollary).      |
| UC-15 | `casper/tests/slashing/proposer_crash.rs`           | Proposer crash after detection: behavioral; next proposer takes over.                                                    |
| UC-16 | `casper/tests/slashing/slashed_parent.rs`           | Multi-parent block with slashed parent: parent counted once in DAG, excluded from fork-choice (T-10).                    |
| UC-17 | `casper/tests/slashing/forkchoice_mixed.rs`         | Mixed slashed/active: only active votes counted in GHOST (T-10).                                                         |
| UC-18 | `casper/tests/slashing/replay_determinism.rs`       | Bonded-proposer slash-deploy emission: replay determinism (T-9.8 positive companion).                                    |
| UC-19 | `casper/tests/slashing/two_level_bond_zero.rs`      | Two-level where neglecter is bond-zero: only equivocator slashed (T-11, T-9.5).                                          |
| UC-20 | `casper/tests/slashing/seqnum_gap.rs`               | Skipped seq number across equivocation: detection succeeds post-fix #7 (T-9.7).                                          |
| UC-21 | `casper/tests/slashing/auth_token_spoof.rs`         | Spoofed auth token rejected at first PoS guard (T-AuthCheck).                                                            |
| UC-22 | `casper/tests/slashing/unbonded_proposer.rs`        | Unbonded proposer post-fix #8: no slash deploys emitted (T-9.8); pre-fix Rust: block rejected at proposer-bond layer.    |
| UC-23 | `casper/tests/slashing/self_correcting.rs`          | Self-correcting block (Rust widening): admitted if it carries SlashDeploy(_, A); offender slashed in same block (T-9.9). |
| UC-24 | `casper/tests/slashing/idempotence.rs`              | Slash twice on same validator: second is no-op (T-Idem; alias T-9).                                                      |
| UC-25 | `casper/tests/slashing/vault_accounting.rs`         | After slash, `vault.balance += pre_slash.bond[v]` (T-8).                                                                 |

### 14.3.2 Tier A — Audit blockers (8 tests)

| #     | Test stub                                            | Asserts                                                                                                                                   |
|-------|------------------------------------------------------|-------------------------------------------------------------------------------------------------------------------------------------------|
| UC-26 | `casper/tests/slashing/f_neglectful_quorum_drop.rs`  | `|closure| > F` ⟹ active set drops below quorum (counter-example to T-12 outside its precondition; T-11 closure-termination still holds). |
| UC-27 | `casper/tests/slashing/neglected_invalid_block.rs`   | `NeglectedInvalidBlock` dispatch under post-fix #3 (T-3, T-6, T-9.3).                                                                     |
| UC-37 | `casper/tests/slashing/self_regression_dag_level.rs` | DAG-level self-regression (T-9.6 DAG-level).                                                                                              |
| UC-38 | `casper/tests/slashing/neglected_detection.rs`       | `detect_neglected` soundness + completeness (T-6).                                                                                        |
| UC-39 | `casper/tests/slashing/bisim_audit.rs`               | R-relation invariant preserved across pipeline step (T-13a/b/c, T-14, T-15a/b).                                                           |
| UC-41 | `casper/tests/slashing/ignorable_pre_fix_dos.rs`     | Pre-fix #1: ignorable equivocation silently dropped (regression test).                                                                    |
| UC-42 | `casper/tests/slashing/dispatcher_pre_fix_drop.rs`   | Pre-fix #3: non-equivocation slashable variants not recorded (regression).                                                                |
| UC-43 | `casper/tests/slashing/seqnum_pre_fix_miss.rs`       | Pre-fix #7: BFS misses gap; equivocation undetected (regression).                                                                         |

### 14.3.3 Tier B — Slashable-variant catalog completion (9 tests)

UC-28 through UC-36 — one test per remaining slashable
`InvalidBlock` variant, each asserting that post-fix #3 routes the
verdict through the standard slash pipeline. Test stubs at:

  `casper/tests/slashing/invalid_parents.rs`
  `casper/tests/slashing/invalid_follows.rs`
  `casper/tests/slashing/invalid_sequence_number.rs`
  `casper/tests/slashing/invalid_shard_id.rs`
  `casper/tests/slashing/invalid_repeat_deploy.rs`
  `casper/tests/slashing/deploy_not_signed.rs`
  `casper/tests/slashing/invalid_transaction.rs`
  `casper/tests/slashing/invalid_block_hash.rs`
  `casper/tests/slashing/future_deploy.rs`

### 14.3.4 Tier C — Operational and adversarial (12 tests)

| #     | Test stub                                                | Asserts                                                                                      |
|-------|----------------------------------------------------------|----------------------------------------------------------------------------------------------|
| UC-40 | `casper/tests/slashing/vault_accounting_failure.rs`      | Vault transfer fail → coop balance unchanged; A returns to EquivocatorRecorded (T-8, T-9.4). |
| UC-44 | `casper/tests/slashing/multi_validator_concurrent_eq.rs` | n parallel equivocations; all detected (T-1, T-9.2, T-12).                                   |
| UC-45 | `casper/tests/slashing/slash_replay_attack.rs`           | Replayed slash deploy: second is no-op (T-Idem, T-9.8).                                      |
| UC-46 | `casper/tests/slashing/partition_merge_eq.rs`            | Partition + merge: post-merge equivocation detected once (T-1, T-9.2, T-15).                 |
| UC-47 | `casper/tests/slashing/validator_join_during_slash.rs`   | Validator joins during pending slash: still slashed; new joiner unaffected (T-Idem, T-10).   |
| UC-48 | `casper/tests/slashing/validator_leave_during_slash.rs`  | Validator leaves during pending slash: re-joins as Unbonded; record persists (T-Idem, T-10). |
| UC-49 | `casper/tests/slashing/genesis_slash.rs`                 | Genesis-time invalid sender: slash routed correctly (T-9.3).                                 |
| UC-50 | `casper/tests/slashing/multi_slash_in_one_block.rs`      | k slash deploys in same block: all atomically applied; replay deterministic (T-Idem, T-11).  |
| UC-51 | `casper/tests/slashing/deep_dag_admissible.rs`           | Deep DAG (>100 blocks): detection scales linearly (T-1, T-15).                               |
| UC-52 | `casper/tests/slashing/wide_dag_admissible.rs`           | Wide DAG (high parent-fanout): detection unaffected by parent count (T-1, T-15).             |
| UC-53 | `casper/tests/slashing/single_chain_eq.rs`               | Single-chain (no forks) equivocation: still detected (T-1, T-9.6).                           |
| UC-54 | `casper/tests/slashing/record_invariants.rs`             | Record monotonicity + uniqueness invariants (T-4, T-5).                                      |

## 14.4 Property-based tests

Property-based tests run thousands of randomized inputs per
property and shrink failures to minimal counter-examples. Each
property below corresponds to a Rocq theorem.

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

    /// T-14 — Weak barbed bisimulation refl + sym.
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

### 14.4.6 Bug-fix properties (T-9.1 and T-9.3 through T-9.9; T-9.2 in §14.4.2)

One property per bug fix, each testing the `t_9_M_*` Rocq lemma.
T-9.2 (atomic no-overwrite) is grouped with the other storage
properties in §14.4.2 above; the eight listed here cover #1, #3–#9:

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

/// T-9.7 — BFS finds descendant with seq-num gap.
#[test]
fn prop_t97_finds_descendant_with_gap(...) { ... }

/// T-9.8 — Unbonded proposer no-emit (post-fix when applied; positive companion: bonded equals pre-fix).
#[test]
fn prop_t98_unbonded_proposer_no_slash(...) { ... }

/// T-9.9 — Post-fix rejection iff neglected ∧ ¬has_slash.
#[test]
fn prop_t99_post_fix_rejection_iff(...) { ... }
```

## 14.5 Cross-implementation tests (Rust ↔ extracted-from-Rocq)

Bisimilarity is the headline claim (T-15). Tests assert that the
Rust implementation and the Rocq oracle produce identical
observable state transitions across randomized event sequences.

```rust
proptest! {
    /// For every randomized event sequence, the Rust harness and the Rocq
    /// oracle (extracted to OCaml + FFI'd to Rust) agree on all five
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
to model-check the slashing subsystem; §14 references **13** of
them (§10.6 of the verification doc lists 12 plus
the rewrite-introduced `Inv_RecordHasWitness`). Each is re-checked
via `tlc` as part of CI:

```bash
# CI script: scripts/ci/check-tla-invariants.sh
for spec in formal/tlaplus/slashing/MC_*.tla; do
    tlc -workers 12 -deadlock -lncheck final "$spec" \
        || { echo "TLC violation in $spec"; exit 1; }
done
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
  `Inv_ActiveSetAboveQuorum` (under BFT precondition).

A TLC violation immediately fails CI.

## 14.7 Pre-fix regression tests (one per bug, #1–#9)

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

Three of the nine pre-fix tests share their counter-example trace
with a UC entry in §14.3.2 (UC-41 = bug #1, UC-42 = bug #3, UC-43
= bug #7); for those, the `pre_fix_bug_<N>.rs` file simply imports
and runs the same trace as a sanity backstop. The other six
(bugs #2, #4, #5, #6, #8, #9) have no UC-numbered positive
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
   integration test (54 tests).
2. **Theorem coverage:** Every theorem in spec/verification has at
   least one property-based test that fails on a property violation.
   §14.4 covers **27 distinct theorem labels**: T-1, T-2, T-3, T-4,
   T-5, T-6, T-7, T-8, T-Idem, T-11, T-12, T-13a, T-13b, T-13c,
   T-14, T-15a, T-15b, T-AuthCheck, T-9.1, T-9.2, T-9.3, T-9.4,
   T-9.5, T-9.6, T-9.7, T-9.8, T-9.9. T-10 (`fork_choice_exclusion`)
   is exercised by example test UC-01 rather than a dedicated
   property test.
3. **InvalidBlock variant coverage:** All 17 pre-fix slashable +
   1 post-fix slashable = 18 slashable variants are exercised
   (Tier B + Core + Tier A tests).
4. **TLA+ invariant coverage:** All **13 cited invariants** have a
   passing model-check run.
5. **Pre-fix counter-example coverage:** Every documented bug
   (#1–#9) has a regression test at
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

## 14.9 CI integration

The slashing test suite runs in CI on every PR:

```yaml
# .github/workflows/slashing-tests.yml
name: Slashing test suite
on: [push, pull_request]
jobs:
  example-based:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --workspace -p casper --tests slashing
  property-based:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo test --release -p casper -- prop_t_
        env:
          PROPTEST_CASES: 10000
  pre-fix-regressions:
    runs-on: ubuntu-latest
    strategy:
      matrix:
        bug: [1, 2, 3, 4, 5, 6, 7, 8, 9]
    steps:
      - uses: actions/checkout@v4
      - run: cargo test -p casper -- pre_fix_bug_${{ matrix.bug }}
  tla-model-check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: scripts/ci/check-tla-invariants.sh
  rocq-build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          systemd-run --user --scope -p MemoryMax=96G -p CPUQuota=1800% \
            -p IOWeight=30 -p TasksMax=200 \
            make -C formal/rocq/slashing -j1
```

A PR cannot merge unless **all five jobs pass** (`example-based`,
`property-based`, `pre-fix-regressions`, `tla-model-check`,
`rocq-build`).

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
   tests UC-40, UC-44–UC-54 (12 tests).

6. **Phase F (weeks 6–7):** Implement property-based tests for
   theorems T-1 through T-15a/b, T-Idem, T-AuthCheck, T-9.1–T-9.9
   (27 properties; T-10 is example-tested in UC-01).

7. **Phase G (week 8):** Integrate cross-implementation
   bisimilarity tests (Rust ↔ extracted Rocq oracle) and TLA+
   model-check CI.

Each phase ends with all its tests passing in CI. Phases B–E may
proceed in parallel after Phase A.

## 14.11 Test-suite metrics

| Metric                                    | Target                                           |
|-------------------------------------------|--------------------------------------------------|
| Example-based test count                  | 54 (one per use case)                            |
| Property-based test count                 | 27 (one per theorem; T-10 by example test UC-01) |
| Pre-fix counter-example test count        | 9 (one per bug)                                  |
| Cross-implementation bisim test count     | 1 (parameterized)                                |
| TLA+ invariant model-check coverage       | 13 (all enumerated in §14.6)                     |
| Mutation-test surviving-mutants threshold | < 5 % of mutants survive                         |
| Property-test cases per property          | 10,000 (CI run)                                  |
| End-to-end CI runtime                     | < 30 minutes                                     |

## 14.12 What this test plan does *not* cover

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
