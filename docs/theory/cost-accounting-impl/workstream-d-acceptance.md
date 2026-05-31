# Workstream D — Concurrent Acceptance + phlo→token + Removals (execution design)

**Status:** Execution design (grounded, ready to implement). Spec `publications/cost-accounting/cost-accounted-rho.tex` is law. Conforms to the approved plan and `../cost-accounting-decision-records.md` (DR-9 token-per-COMM, DR-11 acceptance gate). **The per-signature supply seam (`Σ⟦s⟧` representation, producer/consumer, decrement) is governed by [supply-realization-c-d-handoff.md](supply-realization-c-d-handoff.md) + DR-13.**

## Central representation decision (load-bearing)

Today the runtime is the **s₀ collapse** (spec Remark 11): one `Sig` per deploy, installed once at
`rholang/.../interpreter.rs:117-122` (`SignedProcess::metered(parsed, self.c.signature(), initial_phlo)`),
held as scalar fields in `RuntimeBudget` (`accounting/mod.rs:35,41,42`). The normalized `Par` carries no
per-layer signature, so a static Δ_s has nothing to count layers on.

**Adopted (option B):** Δ_s reads the **fully-desugared `Par`** (`?!`→send+for and uniform-signing expanded
per §7.4 — the *semantic* count, e.g. 8 not 6) and is **parameterized by the deploy's envelope `Sig`**
(from `Cosigned`, supporting `Sig::And` compounds). Each `{·}_σ` layer is attributed to the **whole-signature
value** σ (Def 7.4 — no per-component split); the Split/Join closure (`effectiveΣ`) handles split-vs-combined
granularity. The signature dimension comes from the **envelope**, the layer count from the **desugared `Par`**.
No proto change to `Par`. The N=1 (single-signature) scalar fast-path is preserved verbatim.

## Staged plan (dependency spine: D0 → D1 → D2 → D3 → D4 → D5 → D6; strict: **D2 before D4.1**)

### D0 — per-signature token pool (`accounting/mod.rs`)
- `BillableTokenEvent` gains `sig_hash: [u8;32]` (placed right after `deploy_id` so the derived `Ord` makes
  per-lane order a refinement of the global order). New `Sig::lane_hash(&self) -> [u8;32]` (canonical digest;
  reuse `to_proto`+encode or `SignatureChannel::from_sig`, mod.rs:1198).
- `RuntimeBudget`: keep the scalar fields (fast path, N=1, every legacy deploy → `lanes` empty → existing
  `attempt_one`/`reconcile`/`total_cost` run byte-identically); add `lanes: Arc<DashMap<[u8;32], Lane>>`
  (mirror `rspace.rs:64-65` `phase_a_locks`). `Lane { sig, initial_tokens, consumed_tokens, attempt_queue,
  accumulator, reconciliation }`. Extract `reconcile_lane(initial, attempts) -> CanonicalReconciliation`
  (the current single walk) and call it scalar or per-lane. `total_cost()` = Σ over lanes (commutative,
  order-independent). `MeteredMachine` (`metering.rs:44,59`) stamps `sig_hash` (one compound lane per deploy
  in D-scope; intra-deploy multi-σ is Stage-C funding-slots).
- Proofs: `ChannelSeparation.v` `fuel_gate_no_app_channel_overlap` (:179) ⇒ new `lane_pool_disjoint`
  corollary; `RuntimeBudgetRefinement.v` `rb_state`/`rb_total_cost` ⇒ `rb_pool` = N independent instances,
  `rb_pool_total_cost = Σ rb_total_cost`. Loom: extend `loom_runtime_budget_reconciliation` to 2 lanes.

### D1 — `accounting/delta_sigma.rs` (NEW, pure, linear-time)
- `demand(desugared: &Par, deploy_sig: &Sig) -> DemandEntry{ known_lower_bound, unknown }` per Def 17:
  `Δ_s({P}_s)=1+Δ_s(P)`, `Δ_s({P}_{s'})=Δ_s(P)` for s'≠s, `Δ_s(for/send/par)` recurse, `Δ_s(*x)` resolve-or-`unknown`.
  Includes `desugar_for_funding` (§7.4: uniform signing = 2 layers/for; `?!` = for on each side).
- `supply(sig, pre_state_hash) -> i64` decodes the **single balance datum** `(TOKEN_TAG, n)` on `Σ⟦s⟧` (via
  `SignatureChannel::from_sig`) read from the merged pre-state with `RuntimeManager::get_data(pre_state_hash,
  &from_sig(s).par)` (runtime_manager.rs:969); returns `n` (0 if absent). Supply is a **balance**, not a
  per-message count (DR-13): `Σ_s` is the layer COUNT (Def 17) and the runtime's token normal form is already
  a coalesced balance (`Token::Count{sig,remaining}`, accounting/mod.rs:1156-1164); O(1) per read (literal
  messages would be O(n), bottlenecking the gate). `effective_supply` = Split/Join closure
  (`effectiveΣ_{s₁∘s₂}=Σ_{s₁∘s₂}+min(Σ_{s₁},Σ_{s₂})`, `effectiveΣ_{s₁}=Σ_{s₁}+Σ_{s₁∘s₂}`).
- `is_funded(analysis, margin)`: Def 19 + Thm 20 over-approx — reject `unknown` unless
  `effectiveΣ_s ≥ known_lower_bound + margin`; `margin`+resolution are **shard-genesis constants**.
- Tests: §7.4 8-token count; App. B 3-layer handler; closure arithmetic; `unknown` reject ±margin.

### D2 — block-assembly per-signature-group gate (`block_creator.rs::prepare_user_deploys`)

> **Governed by [wd-d2-acceptance-gate.md](wd-d2-acceptance-gate.md)** (authoritative). Key refinements: the gate runs in `create()` after `compute_parents_post_state` (block_creator.rs:790), NOT literally inside `prepare_user_deploys` (no pre-state there); the **settlement debit** (`post Σ⟦s⟧ = pre − ΣΔ`) rides `CloseBlockDeploy::post_eval`/`post_eval_replay`; `ReplayAdmissionMismatch` guards the admitted set; margin = on-chain `min_phlo_price`. Tracked follow-ons (NOT consensus-critical): speculative execution-on-receipt → **D2-perf**; compound multi-pool debit → **D3** (D2 caps compounds at their own pool; single-signer exact).
- New `admit_by_funding(deploys, pre_state_reader, margin) -> (admitted, rejected)`: group by
  `lane_hash(deploy_sig)`; per group sum `Δ_s`, read `Σ_s` once from the merged pre-state
  (`compute_parents_post_state` result, block_creator.rs:777-784); admit the largest **canonical-order
  prefix** (block_creator.rs:315-324 order) with cumulative `Δ_s ≤ effectiveΣ_s`; reject it + all after
  (§7.7 reject-both / no-partial). **No global lock, no global barrier** — groups are independent `BTreeMap`
  entries (per-signature, §7.6).
- **Gate-before-speculate** (DR-11): O(AST) gate runs at assembly; only passing deploys execute. Any
  execution-on-receipt is **speculative** to a discardable soft-checkpoint (`create_soft_checkpoint`/
  `revert_to_soft_checkpoint`, runtime.rs:620,657) that never feeds acceptance/commit; I/O sinks (stdout,
  peer sends) gated on a new `committed` flag (speculative ⇒ false).
- **Replay**: `replay_admission_mismatch` (sibling to `replay_cost_mismatch`, replay_runtime.rs:442) recomputes
  `admit_by_funding` with the same genesis margin/resolution and asserts admitted==processed_deploys,
  rejected==rejected_deploys. Determinism guards: pure analyzer, `BTreeMap` groups, Σ_s from deterministic
  merged pre-state, canonical deploy order.

### D3 — DC phlo→token (fresh-genesis per DR-6) — **LANDED** (`bf082ee8`/`20705442`/`d2a47fbd`)
The plan below LANDED as the 4 D3 commits. Annotations record where the
implementation refined the plan (b1 diagnostic-refinement: annotate, don't
delete).
- Removed `DeployData.phlo_limit`/`phlo_price` + ALL escrow arithmetic
  (`checked_total_phlo_charge[_value]`, `refund_amount_for_token_cost[_value]`,
  `total_phlo_charge`, `validate_phlo`) + proto fields (tags RESERVED).
  `Validate::phlo_price` block rule + its dispatch removed; `min_phlo_price`
  RETAINED as the gate margin. Reshaped `Cosigned` (`signed.rs` `from_*` drop the
  `phlo_limit` param; `Cosigner.phlo_share` DELETED outright per OD-4 — NOT
  zeroed/reserved — and the share-sum `CosignedError` variants removed).
- Demoted `costs.rs` per-op gas to **diagnostic**: send/receive issue
  `BillableTokenEvent{kind: Comm}` (the consensus cost unit, OD-3) and
  `new`/`match`/`if` issue `kind: Reduction` (diagnostic, cost 0);
  Primitive/Substitution stay diagnostic. `reconcile_lane` counts each committed
  `Comm` as 1, everything else as 0 ⇒ consensus consumed cost = the COMM count
  (DR-9). Pinned by the §7.4 = **8** test.
- **D1→D3 counting-granularity handoff — LANDED.** The plan proposed exposing a
  SEPARATE `comm_node_count` field and switching the gate to it. The IMPLEMENTED
  refinement is simpler and equivalent: `demand()`'s `known_lower_bound` itself
  was flipped to the per-COMM count (`demand_par` keeps `+1` for send/receive
  only; `new`/`match`/`if` stop `.plus_one()` but still recurse). The gate is
  granularity-agnostic and consumes `known_lower_bound` unchanged — so no
  separate `comm_node_count` field is needed; the gate's demand, the runtime's
  consumed `total_cost()`, and the settlement debit all became per-COMM in
  lockstep (§7.4 re-pins 9→8; App.B 6→5). Validated end-to-end by
  `gate_demand_equals_runtime_comm_count` / `settlement_debit_equals_comm_count`
  / `consensus_cost_excludes_per_op_gas` (delta_sigma_spec.rs + cost_accounting_spec.rs).
- Accepted deploys run **unmetered-for-liveness** (OD-1): `evaluate_cosigned`
  installs an `unsafe_max` token budget (no OOP cap) but stays metered, so
  `total_cost()` still returns the real per-COMM count. The escrow precharge/
  refund fan-out (`play_deploy_with_cost_accounting_cosigned` + replay twin) was
  rewritten to gate-funded (KEEP the inner soft-checkpoint); `costacc/
  {pre_charge,refund}_deploy.rs` + the precharge/refund seeds + PoS.rhox
  `chargeDeploy`/`refundDeploy` were deleted (genesis still installs + works).
- Migrated references: `construct_deploy.rs`, `web_api.rs`/grpc/API,
  `options.rs`/CLI (removed `--phlo-*`), `validate.rs`/dispatcher; fuzz/kani
  retargeted to token-supply/Δ_s + gate no-underflow (no `escrow=limit×price`).
  Formal: Rocq per-COMM `consumed = token-drop` bridge (FuelEventDecomposition.v,
  kernel-verified axiom-free), TLA+ schedule-independence under per-COMM, Sage
  settlement re-pinned to the per-signature funding/no-underflow model.

### D4 — removals (after D2)
- **D4.1 precharge/refund (one atomic commit):** delete `costacc/{pre_charge_deploy,refund_deploy}.rs`; rewrite
  `runtime.rs::play_deploy_with_cost_accounting_cosigned` (566-786) removing the pre-charge/refund fan-outs
  (keep the inner soft-checkpoint for failed-deploy rollback); drop the refund-replay coupling in
  `replay_runtime.rs:406`; delete PoS.rhox `chargeDeploy`/`refundDeploy` (KEEP `sysAuthTokenOps`/`createUnfVault`);
  delete the precharge/refund seeds in `system_deploy_util.rs`; reconcile `MultiSignerRefinement.v`
  `pos_charge`/`pos_refund` (keep distinctness lemmas).
- **D4.2 merge — DONE (DR-15).** KEPT `dag_merger::merge`/`resolve_conflicts`/`compute_merged_state`/number-channel
  path (the §2.3 channel-based reconciliation). The orphaned `conflict_set_merger::merge` wrapper (zero production
  callers — `dag_merger` calls `resolve_conflicts`/`compute_merged_state` directly) was **removed**, and its two
  test consumers (the in-file `merge_rejects_negative_channel_balance`, and `tests/merging/merge_number_channel_spec.rs`)
  **re-pointed** to those same two primitives (identical coverage; no test disabled). Channel-based `conflicts()`
  was NOT replaced with a signature predicate.
- **D4.3 run-to-completion — DONE (DR-15): NO production change.** Run-to-completion (the legacy RChain §2.1
  serialize-execute-commit model) was never ported — the reducer already uses per-channel locks and
  `dag_merger::merge` reconciles pre-computed event-log diffs, never re-executing deploys. `compute_parents_post_state`'s
  `parents.len()` dispatch is the multi-parent **block-merge dispatcher** (the §2.3 keep-path entry point), NOT an
  RtC gate; the literal "gate on writes-a-shared-DATA-channel instead of `parents.len()`; disjoint early-return" is a
  **fork-risk misread** (0/1-parent cases have no shared-channel pair; an empty return for disjoint 2+ parents emits a
  wrong post-state — the merged state is the deterministic number-channel fold, never empty). Disjointness is already an
  empty-conflict number-channel fold. Reducer (`reduce.rs`) unchanged. Added a **determinism regression pin** to the
  existing `compute_parents_post_state_regression_spec.rs` (the disjoint sibling-parent merge is byte-identical under
  reversed parent order — spec §2.3 order-determinism).

### D5 — funding proof (Rocq) + TLA+
- `LinearLogicResources.v`: define **pure** `delta_s` (LLUnit→0, LLAtom→1, LLTensor→sum, else 0 — NOT the ILLE
  `ll_required_units`); `funding_decidable` (Def 19) + `delta_s_tensor_additive` + reuse
  `ll_no_double_spend_single_witness` (:359) for "competing proofs, ≤1 succeeds" (Remark 21). Append to the
  `Print Assumptions` heredoc in `scripts/check-cost-accounted-rho-proofs.sh`.
- `EvalScheduling.tla`: `AcceptanceGate(group)` action; invariants `NoDoubleSpendAtBlock`,
  `RejectBothOnOversubscription`, `GateBeforeExecute`. `RuntimeBudgetReplay.tla`: admission-decision schedule-
  independence (mirror `ConsumedAndVerdictScheduleIndependent`:503).

### D6 — verification (all LOCAL-ONLY)
Rust tests: `reject_both_on_oversubscription`, `desugar_eight_token_count`, `speculative_discard_and_io_isolation`,
`per_signature_group_gate`, `gate_decision_replay_determinism`, `merge_idempotency`, `per_lane_reconcile_is_sum_of_scalar`,
`legacy_single_sig_byte_identical`. loom: extend `loom_runtime_budget_reconciliation` (2 lanes),
`loom_multi_sig_fanout`. Rocq/TLA+/Sage via the `check-cost-accounted-rho-*` scripts.
**Dominant perf cost = data-channel merge** (kept) — measure via `DAG_MERGE_*` metrics + a new
`data_channel_merge_bench.rs`; the gate is O(AST) off the merge critical path.

## Commit sequence
D0a (event+lane_hash, no behavior change) → D0b (lane pool + `rb_pool` proof + loom) → D1 (`delta_sigma.rs`) →
D5a (Rocq `delta_s`/`funding_decidable`) → D2 (gate + speculative discard + `replay_admission_mismatch`) →
D5b (TLA+ `AcceptanceGate`) → D3 (phlo→token) → D4.1 (precharge/refund, atomic) → D4.2/D4.3 (dead-wrapper removal +
RtC reinterpreted as already-channel-based, DR-15) → D6 (full verification sweep).

## Cross-workstream couplings
- **B (g/#P split)** changes the `Sig` enum (`Hash`→`Ground|Quote`) that D0's `lane_hash` digests — `lane_hash`
  is shape-agnostic, but land B's `Sig` change before/with D0 to avoid rework.
- **C (economic)** populates the per-signature token supply on `Σ⟦s⟧` channels that D2's gate reads — **C's
  wallet/minting must exist before D2's `supply()` is meaningful.** Order: B core → C economic → D acceptance.

## Risks
Consensus fork via non-deterministic gate (pure analyzer + genesis-pinned margin + canonical order + replay
recompute); per-lane cost order-independence (sig_hash-second `Ord`); the single-Sig representation gap
(resolved by option B); DC blast radius (staged behind nextest); precharge removal strictly after the gate.
