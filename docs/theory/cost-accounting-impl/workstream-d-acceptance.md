# Workstream D — Concurrent Acceptance + phlo→token + Removals (execution design)

**Status:** Historical staging design. The landed end-to-end contract is
[`end-to-end-authority-settlement.md`](end-to-end-authority-settlement.md).
This document remains useful for the D0–D6 implementation history, but its
static-only admission, margin fallback, and unbounded-execution text are
superseded by DR-31 state-bound dependent evidence, authority-derived finite
capacity, replay-checked exact settlement, native signed regions, first-class
stacks, and authenticated physical draws. The “central representation
decision” below is historical: `Par`, stored data, and continuations now retain
cost authority, and production no longer uses the $`s_0`$ collapse.

> **⚠ SUPERSEDED on the funding key by §D2.9** ([wd-d2-acceptance-gate.md](wd-d2-acceptance-gate.md)). This sketch keys the supply pool by `lane_hash(deploy_sig)` (the per-deploy **wire-signature** envelope); the landed implementation keys it by `funding_sig = Sig::Ground(pk)` (single) / the `Sig::And`-fold of `Sig::Ground(pkᵢ)` (multi), so `Σ⟦signer⟧ == Σ⟦wallet⟧` (the genesis-seeded wallet `Σ⟦Ground(pk)⟧`). `deploy_id` remains wire-sig-derived (correct, byte-identical). Read this doc for the D0–D6 staging spine; read `wd-d2-acceptance-gate.md` §D2.9 for the authoritative funding key.

## Central representation decision (load-bearing)

At the start of this historical workstream, the runtime used one `Sig` per deploy, installed once at
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
This historical prototype was removed after native persisted `CostAuthority`
became the sole per-purse accounting path. The abstract `rb_pool` proofs still
establish compositional and permutation-independent purse settlement; they no
longer correspond to a second `RuntimeBudget` ledger.

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
- `demand(desugared: &Par, deploy_sig: &Sig) -> DemandEntry{ certified_upper_bound, unknown }` per Def 17:
  `Δ_s({P}_s)=1+Δ_s(P)`, `Δ_s({P}_{s'})=Δ_s(P)` for s'≠s, `Δ_s(for/send/par)` recurse, `Δ_s(*x)` resolve-or-`unknown`.
  Includes `desugar_for_funding` (§7.4: uniform signing = 2 layers/for; `?!` = for on each side).
- `supply(sig, pre_state_hash) -> i64` decodes the **single balance datum** `(TOKEN_TAG, n)` on `Σ⟦s⟧` (via
  `SignatureChannel::from_sig`) read from the merged pre-state with `RuntimeManager::get_data(pre_state_hash,
  &from_sig(s).par)` (runtime_manager.rs:969); returns `n` (0 if absent). Supply is a **balance**, not a
  per-message count (DR-13): `Σ_s` is the layer COUNT (Def 17) and the runtime's token normal form is already
  a coalesced balance (`Token::Count{sig,remaining}`, accounting/mod.rs:1156-1164); O(1) per read (literal
  messages would be O(n), bottlenecking the gate). `effective_supply` = Split/Join closure
  (`effectiveΣ_{s₁∘s₂}=Σ_{s₁∘s₂}+min(Σ_{s₁},Σ_{s₂})`, `effectiveΣ_{s₁}=Σ_{s₁}+Σ_{s₁∘s₂}`).
- Landed refinement: admission consumes a checked `DemandCertificate`.
  Structural analysis produces a finite upper bound for closed programs and
  returns `Unprovable` for unresolved syntax. Production supplies exact
  state-bound evidence for authenticated resident continuations; a GSLT or
  MeTTaIL producer can supply a checked conservative bound through the same
  trait. The obligation is `effectiveΣ_s ≥ certified_upper_bound + fee`.
- Tests: §7.4 eight-token count; Appendix B handler; closure arithmetic;
  unprovable-demand rejection; exact cost-plus-fee boundary.

### D2 — block-assembly per-signature-group gate (`block_creator.rs::prepare_user_deploys`)

> **Governed by [end-to-end-authority-settlement.md](end-to-end-authority-settlement.md).** The gate runs in `create()` after `compute_parents_post_state`. Production computes exact state-bound evidence under finite capacity, iterates after exhausted or underfunded removals, and funds `κ + fee`; a structural or external conservative certificate funds `Δ^max + fee`. `CloseBlockDeploy` debits replay-checked realized `κ + fee`; `ReplayAdmissionMismatch` guards the admitted set.
- New `admit_by_funding(deploys, pre_state_reader) -> AdmissionOutcome`: group by
  `funding_sig.lane_hash()` (§D2.9 — the signer's wallet key `Σ⟦Ground(pk)⟧`, NOT `lane_hash(deploy_sig)`); per group sum `Δ_s`, read `Σ_s` once from the merged pre-state
  (`compute_parents_post_state` result, block_creator.rs:777-784); admit the largest **canonical-order
  prefix** (block_creator.rs:315-324 order) with cumulative `Δ_s ≤ effectiveΣ_s`; reject it + all after
  (§7.7 reject-both / no-partial). **No global lock, no global barrier** — groups are independent `BTreeMap`
  entries (per-signature, §7.6).
- **Dependent proof before commit** (DR-31): bounded proof evaluation runs at
  assembly in a spawned scratch runtime rooted at authenticated state. Only a
  completed, funded fixed point yields an opaque admission token. Checkpoint and
  replay independently reproduce its consensus evidence.
- **Replay**: `replay_admission_mismatch` (sibling to `replay_cost_mismatch`, replay_runtime.rs:442) recomputes
  the same certificate validation, pre-state root, and residual ledger and asserts admitted==processed_deploys,
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
  RETAINED as ingress economic configuration, not as a proof input. Reshaped `Cosigned` (`signed.rs` `from_*` drop the
  `phlo_limit` param; `Cosigner.phlo_share` DELETED outright per OD-4 — NOT
  zeroed/reserved — and the share-sum `CosignedError` variants removed).
- Demoted reducer per-op gas to **diagnostic**. Successful binary and join
  matches issue `BillableTokenEvent{kind: Comm}` through the RSpace observer;
  unmatched send/receive and new/match/if do not add consensus cost.
  Primitive/Substitution remain diagnostic. `reconcile_lane` counts each
  committed atomic `Comm` as one and everything else as zero.
- **D1→D3 counting-granularity handoff — refined.** `demand()` counts potential
  send/receive introductions for the closed non-persistent fragment. That value
  is a conservative structural reservation, not the runtime COMM count.
  Persistent I/O and unresolved dequotation are structurally unprovable.
  Production obtains exact demand by executing the canonical candidate sequence
  from the authenticated state and binding the realized atomic-COMM cost and
  adjacent roots into state-bound evidence. The §7.4 desugared shape therefore
  has eight structural introductions but four realized atomic matches.
- DR-31 supersedes historical OD-1: the single committed user execution and
  constrained replay install finite authority-derived capacity. Exhaustion rejects and cannot
  certify; under protocol 4, `total_cost()` returns the one-unit-per-COMM
  execution projection plus canonical RSpace bytes (DR-47). The escrow precharge/
  refund fan-out (`play_deploy_with_cost_accounting_cosigned` + replay twin) was
  rewritten to gate-funded (KEEP the inner soft-checkpoint); `costacc/
  {pre_charge,refund}_deploy.rs` + the precharge/refund seeds + PoS.rhox
  `chargeDeploy`/`refundDeploy` were deleted (genesis still installs + works).
- Migrated references: `construct_deploy.rs`, `web_api.rs`/grpc/API,
  `options.rs`/CLI (removed `--phlo-*`), `validate.rs`/dispatcher; fuzz/kani
  retargeted to token-supply/Δ_s + gate no-underflow (no `escrow=limit×price`).
  Formal: Rocq proves atomic-COMM debit and settlement; TLA+ explores arrival
  permutations, rejection rollback, joins, and replay equality; Sage checks the
  per-signature funding/no-underflow model.

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
Consensus fork via non-deterministic gate (pure analyzer + proof-bearing reservation + canonical order + replay
recompute); per-lane cost order-independence (sig_hash-second `Ord`); the single-Sig representation gap
(resolved by option B); DC blast radius (staged behind nextest); precharge removal strictly after the gate.
