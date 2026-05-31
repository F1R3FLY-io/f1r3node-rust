# Workstream E — Validator Behavioral Contract (multi-prover)

**Status:** Authoritative contract specification (E0). Governing record: **DR-12** ("Validator lifted into
Rholang with a multi-prover behavioral contract"). Spec law: `publications/cost-accounting/cost-accounted-rho.tex`
(§6.3, §6.4, §7.1, §7.6, §7.7). All formal verification is **LOCAL-ONLY** (never `.github/workflows`).

This document fixes the **validator behavioral contract**: the obligation set a Cost-Accounted-Rho validator
must satisfy, the prover(s) that discharge each obligation, the existing artifacts that already discharge
them for the built-in validator, and the duty a *custom* validator inherits or re-discharges. Workstreams
E5/E6 formalize this contract under `formal/{rocq,tlaplus,lean}/validator/`; E7 documents the custom seam.

## 1. The host-language boundary (what is in Rholang vs Rust, and why it is spec-faithful)

The spec constrains the validator at exactly one place. §6.3 (tex:1245–1253): "a proposed block is valid only
if every communication it contains is backed by matching tokens. The revised calculus makes this validity
check **a syntactic predicate**: the block's contents must be well-typed in the cost-accounted grammar, with
token stacks present for every signed communication." §7.6 (tex:1631): "the validator computes Δ_c(D) by
**static analysis** of the deployment's AST"; §7.7 (tex:1758): "**the validator is a proof checker**."

The spec therefore mandates that block-validity be a **syntactically-checkable predicate** computed by a
**proof checker** — it is **agnostic about the host language** of that checker. The realization splits along
this boundary:

- **Economic / adjudication decisions are Rholang** (`casper/src/main/resources/PoS.rhox`, `Exchange.rhox`):
  slash effect (quarantine earmark + minting-halt), epoch mint into `@W_v`, redemption
  (Vindicated/Guilty/Burned), fee conversion, close-block. These are swappable by a custom validator
  (genesis-deployed, parameter-substituted). Landed in Stages A–D.
- **The linear-proof acceptance gate is Rust** (`casper/src/rust/util/rholang/acceptance.rs::admit_by_funding`
  + the pure `rholang/.../accounting/delta_sigma.rs`). This is spec-faithful (§7.6 "static analysis of the
  AST" is exactly the pure Rust pass) and is **forced** by **DR-13**: the per-signature supply pool
  `Σ⟦s⟧ = from_sig(s)` is an unforgeable `GPrivate` channel with no `bytes→GPrivate` surface primitive in
  Rholang, so a Rholang contract cannot name `Σ⟦s⟧` and therefore cannot read the supply the gate needs.
  Moving the gate to Rholang would re-expose `Σ⟦s⟧` (a DR-13-rejected alternative). The gate stays Rust to
  preserve unforgeability.
- **Platform mechanism is Rust** (out-of-spec per DR-12): P2P/TLS, LMDB, the reducer/RSpace engine
  (per-channel locks), equivocation detection, the slash-authorization predicate, the finalization oracle,
  replay. These uniformly enforce the contract for any deployed economic contract.

"Validator → Rholang" is thus realized as: **economic/adjudication decisions in Rholang (done, A–D); the
linear-proof gate as the Rust proof-checker (spec-faithful, DR-13-forced); this contract names and proves the
obligations over both.** E moves no consensus logic.

## 2. The obligation set

Each obligation lists: its statement; spec/DR basis; the prover(s) that discharge it (**TLA+ always**, plus
Rocq and/or Lean); the **existing artifact** that discharges it for the built-in validator (reuse); and the
**custom-validator duty** (re-discharge vs inherit).

### Spec obligations (S1–S4)

**S1 — Token-presence syntactic validity (§6.3).** Every signed communication in an accepted block is backed
by a present, matching token; a term that is malformed in the cost-accounted grammar, or whose token axis
does not match, is rejected.
- Rocq: `FuelGateSafety.v` (`fuel_gate_rejects_mismatched_token`, `…_ground`, `…_cross_axis`,
  `fuel_gate_stuck_isolated`, `fuel_gate_body_protected`).
- TLA+: the token-stack-present rewrite in `CostAccountedRho.tla` / `FullProtocol.tla`.
- Lean (E3): mirror `fuel_gate_rejects_mismatched_token` + `fuel_gate_stuck_isolated`.
- Custom duty: re-discharge over its admission predicate (TLA+ token-presence invariant + Rocq-or-Lean
  fuel-gate-rejects lemma).

**S2 — Acceptance correctness (§7.6).** Accept iff `Σ_s ≥ Δ_s`, decided by static analysis **before** any
execution.
- Rocq: `LinearLogicResources.v` (`funding_decidable` = §7.6 decidability; `delta_s`, `sigma_s`,
  `is_funded_balance`, `delta_s_tensor_additive`, `sigma_s_balance_eq_stack_count`).
- TLA+: `RuntimeBudgetReplay.tla::admission_decision_schedule_independent`.
- Lean (E2): mirror `delta_s`, `sigma_s`, `funds`, `funding_decidable`, `is_funded_balance`.
- Custom duty: re-discharge for its demand/supply functions.

**S3 — Linear no-double-spend / reject-both (§7.7).** Two deployments competing for the same tokens: at most
one is admitted; on the first under-funded deployment in canonical order, reject it and all after it (no
partial admission).
- Rocq: `LinearLogicResources.v` (`ll_no_double_spend_single_witness`,
  `ll_double_spend_requires_duplicate_witness`, `ll_linear_no_contraction`, `ll_linear_no_weakening`,
  `ll_consume_linear_once_atom_exhausts`).
- TLA+: the budget-fitting committed prefix + `NonOopCommittedMultisetComplete`.
- Lean (E2): mirror `ll_no_double_spend_single_witness` + `ll_linear_no_contraction`.
- Rust cross-check (landed): `acceptance.rs` tests `reject_both_on_oversubscription`,
  `drained_present_pool_rejects`.
- Custom duty: re-discharge (at-most-one-of-competing + no-contraction).

**S4 — Transaction atomicity (§7.1).** Each for-comprehension is a funded two-action transaction
(rendezvous + matching); the witness substitution is atomic (all-or-nothing).
- Rocq: `LinearLogicResources.v` (`ll_linear_cut_consumes_cut_witness`,
  `ll_lolly_modus_ponens_consumes_input_context`, `core_token_demand`); atomicity via
  `StepDeterminism.v::ca_step_deterministic` + `FuelEventDecomposition.v`.
- TLA+: single-COMM atomic fire + per-COMM weight in `RuntimeBudgetReplay.tla`.
- Lean (E3): mirror `core_token_demand` + `ca_step_deterministic`.
- Custom duty: **inherit** — a custom validator that uses the platform reducer satisfies S4 by construction
  (it is a calculus theorem, not a per-validator one); it re-cites unless it changes the reducer.

### Platform obligations (P1–P3, labeled out-of-spec per DR-12)

**P1 — Slash-authorization soundness.** A block is slashed for a slash deployment only if on-chain evidence
authorizes it; stale or rebonded-key evidence cannot slash.
- Rocq: `BugFixSlashAuthorization.v`; `MainTheorem.v` (T9.12 stale-evidence-not-authorized, T7, T9).
- TLA+: `AuthorizedSlashFlow.tla` (`Inv_OnlyAuthorizedSlashCanBePending`,
  `Inv_StaleEvidenceCannotSlashRebondedKey`, `Inv_BondsZeroAfterSlash`).
- Lean (E4): mirror the taxonomy core (`bm_slash_lookup`, `bm_slash_idempotent_lookup`,
  `stale_evidence_not_authorized`).
- Custom duty: **inherit** — slash-authorization stays in the Rust shell (DR-12); a custom validator does not
  re-implement P1.

**P2 — Finalization safety.** Finalized blocks form a consistent, non-equivocating chain; the fork choice
excludes slashed validators.
- Rocq: `ForkChoice.v`; `MainTheorem.v` (T10 fork-choice exclusion, T1/T2 detection sound/complete).
- TLA+: `EquivocationDetector` (`Inv_DetectionSound`, `Inv_TaxonomyCorrect`); fork choice in
  `JustificationProjection`.
- Lean (optional E6.5): mirror `main_T1_detection_sound` (detection soundness kernel).
- Custom duty: **inherit** — equivocation detection + finalization oracle stay in the Rust shell.

**P3 — Determinism / replay-equivalence.** The verdict (accept set, settlement debit, consumed cost, OOP
flag) is a pure function of (block, pre-state) — identical on play and replay across schedules.
- Rocq: `StepDeterminism.v::ca_step_deterministic`; `RuntimeBudgetRefinement.v`;
  `FuelEventDecomposition.v` (consumed COMM count determined by endpoints).
- TLA+: `RuntimeBudgetReplay.tla` (`ConsumedAndVerdictScheduleIndependent`,
  `admission_decision_schedule_independent`, `TotalCostMatchesClampedSum`,
  `NonOopCommittedMultisetComplete`).
- Lean (E4): mirror `ca_step_deterministic` (single-step determinism kernel of P3).
- Rust cross-check (landed): `gate_decision_replay_determinism`, `merge_idempotency`.
- Custom duty: **re-discharge (critical)** — TLA+ schedule-independence of its verdict + Rocq-or-Lean
  determinism of its decision function. A non-deterministic custom gate forks the chain.

### Contract summary

| ID | Obligation | Basis | TLA+ | Rocq | Lean (E) | Custom duty |
|----|-----------|-------|------|------|----------|-------------|
| S1 | token-present + reject-malformed | §6.3 | fuel-gate | `FuelGateSafety.v` | E3 | re-discharge |
| S2 | accept iff Σ≥Δ, pre-exec | §7.6 | `admission_decision_schedule_independent` | `funding_decidable` | E2 | re-discharge |
| S3 | linear no-double-spend / reject-both | §7.7 | committed-prefix | `ll_no_double_spend_single_witness` | E2 | re-discharge |
| S4 | for-comp = atomic funded txn | §7.1 | single-COMM fire | `ca_step_deterministic`, `core_token_demand` | E3 | inherit |
| P1 | slash-auth soundness | DR-12 | `AuthorizedSlashFlow` | `BugFixSlashAuthorization` | E4 | inherit |
| P2 | finalization safety | DR-12 | `EquivocationDetector` | `ForkChoice` | E6.5 (opt) | inherit |
| P3 | determinism / replay-equiv | DR-12 | `ConsumedAndVerdictScheduleIndependent` | `StepDeterminism` | E4 | re-discharge |

Of 7 obligations × {TLA+, Rocq} = 14 discharges, all 14 are landed. E's new proving = the **7 Lean mirrors**
plus the named-contract aggregation (`validator/` subtrees re-export, they do not re-prove).

## 3. The multi-prover requirement

- The **built-in validator** is proven in **all three** provers: TLA+ (E5), Rocq (E5 aggregation over the
  landed corpus), Lean (E2–E4, E6). The "all three" milestone is **E6**.
- A **custom validator** ships **TLA+ + (Rocq or Lean)** discharges of **S1–S4 + P3** for its own
  admission/decision functions (P1/P2 inherited from the Rust shell), and is checked by the local
  `check-cost-accounted-rho-{proofs,tla-invariants,lean}.sh` scripts (E7 ships a template).

## 4. Custom-validator seam (forward reference to E7)

Spec-minimal — no plugin framework (§7.7: one well-specified proof-checker, swappable economics). Three
layers, all already present; E7 documents and lightly hardens them: (1) the Rholang economic/adjudication
contract is genesis-deployed with parameter substitution (`genesis/contracts/standard_deploys.rs`,
`pos_generator`), so a custom validator supplies its own `PoS.rhox`-shaped source; (2) the Rust platform
shell (gate, slash-auth, equivocation, finalization) is fixed and enforces the contract mechanically (the
settlement-debit `checked_sub` underflow makes an over-admitting proposer's block a detectable invalid
block); (3) the proof bundle (TLA+ + Rocq-or-Lean over S1–S4 + P3) is the script-checkable obligation set.

## 5. Scope guardrails

E does not move any consensus logic, does not re-prove the landed TLA+/Rocq obligations, and does not port the
full Rocq corpus to Lean. **Lean is scoped to the validator obligation set** — the mirrors named above; it
excludes `StrongNormalization.v`, `Confluence.v`, `Translation.v`, `TranslationFaithfulness.v`,
`Bisimulation.v`, `WeakBarbedEquiv.v`, `Replication.v`, `MultiSignerRefinement.v`, `Settlement.v`,
`Exchange.v` (the full corpus stays Rocq-only, staged behind Rocq per DR-12).
