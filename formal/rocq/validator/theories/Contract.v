(* Validator.Contract — the named VALIDATOR BEHAVIORAL CONTRACT, Rocq surface.

   Workstream E, stage E5. This is the Rocq half of the "built-in validator
   proven in all three provers" milestone (Lean: formal/lean/Validator/
   Contract.lean; TLA+: formal/tlaplus/validator/Validator.tla).

   GUIDING PRINCIPLE: re-export, never re-prove. Each `validator_contract_*`
   theorem below states the contract clause and discharges it by `exact` of a
   theorem ALREADY proven, kernel-checked, and reported "Closed under the
   global context" in the CostAccountedRho / Slashing developments. The clause
   TYPE is exactly the obligation's type (transcribed from the obligation's own
   statement), and the proof is the single underlying term, so
   `Print Assumptions validator_contract_X` reports exactly what
   `Print Assumptions <obligation>` reports — axiom-free for all seven.

   Contract <-> obligation <-> spec map
   ────────────────────────────────────────────────────────────────────────
     clause                       obligation                          spec
     ────────────────────────── ─────────────────────────────────── ──────
     validator_contract_S1        FuelGateSafety                       §6.3
                                  .fuel_gate_rejects_mismatched_token
     validator_contract_S2        LinearLogicResources                 §7.6
                                  .funding_decidable
     validator_contract_S3        LinearLogicResources                 §7.7
                                  .ll_no_double_spend_single_witness
     validator_contract_S4        StepDeterminism                      §7.1
                                  .ca_step_deterministic
     validator_contract_P1        MainTheorem                          DR-12
                                  .main_T9_12_stale_evidence_not_authorized
     validator_contract_P1_effect Validator.bm_slash_lookup            DR-12
     validator_contract_P2        MainTheorem                          DR-12
                                  .main_T10_fork_choice_exclusion
     validator_contract_P3        StepDeterminism.ca_step_deterministic DR-12
   ────────────────────────────────────────────────────────────────────────

   S1–S4 come from the CostAccountedRho development; P1/P2 come from the
   Slashing development. The two logical roots import together cleanly (the
   Slashing `Validator` record and the CostAccountedRho process syntax share
   no clashing global definitions). P3's verdict-determinism IS S4's per-step
   determinism (`ca_step_deterministic`); it is re-exported under the P3 name
   below to give the platform obligation its own contract handle, exactly as
   the Lean surface does. The full multi-step schedule-independence of P3 is
   additionally machine-checked by TLC in
   formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla
   (invariants ConsumedAndVerdictScheduleIndependent and
   admission_decision_schedule_independent). *)

From CostAccountedRho Require Import RhoSyntax CostAccountedSyntax Translation
  CostAccountedReduction FuelGateSafety LinearLogicResources StepDeterminism.
From Slashing Require Import Validator ValidatorLifetime ForkChoice MainTheorem.

(* ─── Spec obligations S1–S4 (CostAccountedRho) ─────────────────────────── *)

(* S1 (§6.3 token-presence / reject-malformed): a translated fuel gate keyed on
   one quote can never take a top-level COMM step against a token signed by a
   DIFFERENT quote — distinct quotes give distinct channel names, so the
   send/receive cannot match. The validator therefore rejects a malformed
   (mismatched-token) deploy. The gate's hp/gp encoders and their injectivity
   are explicit parameters (discharged from FuelGateMismatch's section), so the
   clause is closed under the global context. *)
Theorem validator_contract_S1 :
  forall hp : list bool -> RhoSyntax.proc,
  (forall b1 b2 : list bool, hp b1 = hp b2 -> b1 = b2) ->
  forall (gp : list bool -> RhoSyntax.proc) (P : RhoSyntax.proc)
    (bs1 bs2 : list bool) (t : CostAccountedSyntax.token),
  bs1 <> bs2 ->
  forall Q : RhoSyntax.proc,
  RhoSyntax.PPar
    (Translation.P_tr hp gp P (CostAccountedSyntax.SQuote bs1))
    (Translation.T_tr hp gp
       (CostAccountedSyntax.TGate (CostAccountedSyntax.SQuote bs2) t)) <>
  RhoSyntax.PPar
    (RhoSyntax.PInput
       (Translation.N_tr hp gp (CostAccountedSyntax.SQuote bs1)) Q)
    (RhoSyntax.POutput
       (Translation.N_tr hp gp (CostAccountedSyntax.SQuote bs1))
       (Translation.T_tr hp gp t)).
Proof. exact @fuel_gate_rejects_mismatched_token. Qed.

(* S2 (§7.6 acceptance): the funding obligation Σ_s ≥ Δ_s is DECIDABLE — for any
   supply balance and any deployment formula the validator always reaches an
   accept/reject verdict by one integer comparison, before any execution.
   Carries the verdict as data (a sumbool). *)
Theorem validator_contract_S2 :
  forall (n : nat) (f : ll_formula),
  {funds n (delta_s f)} + {~ funds n (delta_s f)}.
Proof. exact @funding_decidable. Qed.

(* S3 (§7.7 linearity): no double-spend from a single witness — a linear token,
   once consumed, cannot be consumed again. *)
Theorem validator_contract_S3 :
  forall a : nat,
  match consume_linear_atom a (LLAtom a :: nil)%list with
  | Some delta => consume_linear_atom a delta
  | None => None
  end = None.
Proof. exact @ll_no_double_spend_single_witness. Qed.

(* S4 (§7.1 transaction atomicity): in a single-token system the per-COMM step
   is DETERMINISTIC — the transaction outcome is a function of the redex, so a
   funded for-comprehension behaves as one atomic transaction. *)
Theorem validator_contract_S4 :
  forall S T1 T2 : CostAccountedSyntax.system,
  single_token_sys S ->
  CostAccountedReduction.ca_step S T1 ->
  CostAccountedReduction.ca_step S T2 -> T1 = T2.
Proof. exact @ca_step_deterministic. Qed.

(* ─── Platform obligations P1–P3 (Slashing + CostAccountedRho) ──────────── *)

(* P1 (slash-authorization soundness, DR-12): stale-epoch evidence cannot
   authorize slashing a rebonded key — evidence bound to an old validator
   lifetime does not authorize action against the same key in a new epoch. *)
Theorem validator_contract_P1 :
  forall (v : Validator) (e_old e_new : ValidatorLifetime.Epoch),
  e_old <> e_new ->
  ValidatorLifetime.evidence_authorizes_lifetime
    {| ValidatorLifetime.vl_validator := v;
       ValidatorLifetime.vl_epoch := e_old |}
    {| ValidatorLifetime.vl_validator := v;
       ValidatorLifetime.vl_epoch := e_new |} = false.
Proof. exact @main_T9_12_stale_evidence_not_authorized. Qed.

(* P1 effect (DR-12): the slash effect zeros exactly the offender's bond at the
   bond-map lookup level. This is the algebraic kernel the protocol-level slash
   results compose over. *)
Theorem validator_contract_P1_effect :
  forall (bm : BondMap) (v : Validator), bm_lookup (bm_slash bm v) v = 0.
Proof. exact @bm_slash_lookup. Qed.

(* P2 (fork-choice exclusion, DR-12): a validator whose bond has been zeroed
   (slashed) is excluded from fork choice — fc_lookup over the slashed-filtered
   latest-message map returns None for it. Lean delegates P2 to the Rocq+TLA+
   surfaces; this is its Rocq contract handle. *)
Theorem validator_contract_P2 :
  forall (lm : ForkChoice.LatestMessages) (bonds : BondMap) (v : Validator),
  bm_lookup bonds v = 0 ->
  ForkChoice.fc_lookup (ForkChoice.filter_slashed lm bonds) v = None.
Proof. exact @main_T10_fork_choice_exclusion. Qed.

(* P3 (determinism / replay-equivalence, DR-12): the validator verdict is a
   deterministic function of the system. At the per-step level this IS S4's
   ca_step_deterministic; re-exported here under the P3 name so the platform
   determinism obligation has its own contract handle. The full multi-step
   schedule-independence is additionally machine-checked by TLC in
   formal/tlaplus/cost_accounted_rho/RuntimeBudgetReplay.tla. *)
Theorem validator_contract_P3 :
  forall S T1 T2 : CostAccountedSyntax.system,
  single_token_sys S ->
  CostAccountedReduction.ca_step S T1 ->
  CostAccountedReduction.ca_step S T2 -> T1 = T2.
Proof. exact @ca_step_deterministic. Qed.
