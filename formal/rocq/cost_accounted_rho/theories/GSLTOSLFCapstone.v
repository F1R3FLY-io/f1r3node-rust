(* ════════════════════════════════════════════════════════════════════════
   GSLTOSLFCapstone.v

   Capstone discharging the spec's two GSLT/OSLF structural claims as explicit
   theorems, FRAMEWORK-INDEPENDENTLY (no MeTTaIL):

     §6.1  "the cost-accounted rho calculus is itself such a triple"
           (a GSLT = Types, Equations on types, Rewrites on types).
     §7.7  "the OSLF functor ... can be extended to generate a linear resource
            logic whose judgments ARE funding proofs; the static analysis is a
            proof search in this logic, and the validator is a proof checker."

   This module adds NO new mathematical content and NO axioms: every conjunct
   is an already-proven, Qed-closed result from the development, assembled into
   the spec's claimed structure. It is the explicit, machine-checked discharge
   of claims §6.1/§7.7 that were previously only asserted in prose.

   GSLT and OSLF are framework-general; this realizes them directly in Rocq.
   The native finite located spatial/modal fragment is realized directly and
   connected to the funding judgment below.  Only the literal embedding into
   MeTTaIL and replacement of this reference realization by generated artifacts
   belong to the still-developing MeTTaIL/OSLF integration.

   No `Axiom`, no `Admitted`: all proofs `Qed`-closed.                          *)

From Stdlib Require Import Permutation.
From Stdlib Require Import List.
Import ListNotations.
From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CostAccountedReduction.
From CostAccountedRho Require Import SystemStructEquiv.
From CostAccountedRho Require Import Confluence.
From CostAccountedRho Require Import LinearLogicResources.
From CostAccountedRho Require Import CALocatedPurses.
From CostAccountedRho Require Import CAOSLFSpatialModal.

(* ════════════════════ §6.1 — the calculus is a well-formed GSLT ═══════════
   Types: the four sorts [proc] / [system] / [sig] / [token] (present by
   construction). Equations: the structural equivalences [≡] (process) and
   [≡sys] (system) are equivalence relations and congruences for the type
   constructors. Rewrites: the rewrite relation [ca_step] is confluent, has
   unique normal forms, and has path-independent (deterministic) cost.        *)

Definition GSLT_Equations_WellFormed : Prop :=
  (* [≡sys] is an equivalence relation *)
  (forall S, sys_equiv S S)
  /\ (forall S1 S2, sys_equiv S1 S2 -> sys_equiv S2 S1)
  /\ (forall S1 S2 S3, sys_equiv S1 S2 -> sys_equiv S2 S3 -> sys_equiv S1 S3)
  (* [≡sys] congruence: parallel composition and signing *)
  /\ (forall S1 S1' S2 S2',
        sys_equiv S1 S1' -> sys_equiv S2 S2' -> sys_equiv (SPar S1 S2) (SPar S1' S2'))
  /\ (forall P P' s, struct_equiv P P' -> sys_equiv (SSigned P s) (SSigned P' s))
  (* [≡] (process level) is an equivalence relation *)
  /\ (forall P, struct_equiv P P)
  /\ (forall P Q, struct_equiv P Q -> struct_equiv Q P)
  /\ (forall P Q R, struct_equiv P Q -> struct_equiv Q R -> struct_equiv P R)
  (* [≡] congruence: parallel composition *)
  /\ (forall P P' Q Q',
        struct_equiv P P' -> struct_equiv Q Q' -> struct_equiv (PPar P Q) (PPar P' Q')).

Theorem gslt_equations_wellformed : GSLT_Equations_WellFormed.
Proof.
  unfold GSLT_Equations_WellFormed. repeat split.
  - exact sse_refl.
  - exact sse_sym.
  - exact sse_trans.
  - exact sse_par_cong.
  - exact sse_signed_cong.
  - exact se_refl.
  - exact se_sym.
  - exact se_trans.
  - exact se_par_cong.
Qed.

Definition GSLT_Rewrites_WellFormed : Prop :=
  (* the rewrite relation is confluent *)
  (forall S, confluent S)
  (* normal forms are unique *)
  /\ (forall S T1 T2,
        ca_reachable S T1 -> ca_terminal T1 ->
        ca_reachable S T2 -> ca_terminal T2 -> T1 = T2)
  (* cost is deterministic (path-independent) *)
  /\ (forall S T1 T2,
        ca_reachable S T1 -> ca_terminal T1 ->
        ca_reachable S T2 -> ca_terminal T2 ->
        system_token_count T1 = system_token_count T2).

Theorem gslt_rewrites_wellformed : GSLT_Rewrites_WellFormed.
Proof.
  unfold GSLT_Rewrites_WellFormed. repeat split.
  - exact ca_confluent.
  - exact ca_normal_form_unique.
  - exact ca_cost_deterministic.
Qed.

(* ═══════════════ §7.7 — OSLF-generated linear resource logic ══════════════
   The funding judgment [funds Σ Δ := Δ ≤ Σ] (the resource inequality
   [Σ_s ≥ Δ_s]) is decidable (proof search terminates — Thm 20); the gate is a
   sound proof checker (accepts iff funded); an underfunded deploy is rejected;
   and the resource logic is genuinely LINEAR — no contraction, so a single
   token cannot be duplicated and two deployments cannot both consume it ("at
   most one competitor wins", §7.7).                                          *)

Definition OSLF_Funding_Logic_Sound : Prop :=
  (* the judgment IS the resource inequality Σ ≥ Δ *)
  (forall n d, funds n d <-> d <= n)
  (* proof search is decidable (Thm 20): the validator always reaches a verdict *)
  /\ (forall (n : nat) (f : ll_formula), funds n (delta_s f) \/ ~ funds n (delta_s f))
  (* the validator is a SOUND proof checker: accepts iff the demand is funded *)
  /\ (forall (n : nat) (f : ll_formula), is_funded_balance n f = true <-> funds n (delta_s f))
  (* an underfunded deploy (positive demand, empty/absent pool) is REJECTED *)
  /\ (forall (f : ll_formula), delta_s f > 0 -> is_funded_balance 0 f = false)
  (* the resource logic is LINEAR: no contraction (a linear token cannot be
     duplicated) — the proof-theoretic content of "≤1 competitor wins" *)
  /\ (forall a, ~ Permutation (linear_ctx_atoms [LLAtom a])
                              (linear_ctx_atoms [LLTensor (LLAtom a) (LLAtom a)])).

Theorem oslf_funding_logic_sound : OSLF_Funding_Logic_Sound.
Proof.
  unfold OSLF_Funding_Logic_Sound.
  split. { intros n d. unfold funds. split; intro H; exact H. }
  split. { intros n f. destruct (funding_decidable n f) as [H|H]; [left|right]; exact H. }
  split. { exact funding_check_balance_sound. }
  split. { exact strict_reject_when_underfunded. }
  exact ll_linear_no_contraction.
Qed.

(* ═══════════════ §9.2/§10 — finite located spatial/modal logic ════════════
   Exact observations decide linear use and graded post-state claims.  Static
   upper bounds prove only safety properties.  Spatial composition factors the
   proof by disjoint surfaces, so local purse sufficiency composes globally. *)

Definition OSLF_Spatial_Modal_Logic_Sound : Prop :=
  (forall supply demand surface,
      linear_check (mk_oslf_observation supply demand true) surface = OSatisfied
      <-> 1 <= supply surface /\ demand surface = 1)
  /\ (forall observation surface amount,
      observed_supply (after_spend observation surface amount) surface =
        observed_supply observation surface - amount
      /\ observed_demand (after_spend observation surface amount) surface =
        observed_demand observation surface - amount)
  /\ (forall left right left_verdict right_verdict,
      spatial_check left right left_verdict right_verdict =
      spatial_check right left right_verdict left_verdict)
  /\ (forall supply demand locations,
      (forall surface, In surface locations ->
        sufficient_check
          (mk_oslf_observation supply demand false) surface = OSatisfied) ->
      total demand locations <= total supply locations)
  /\ (forall supply upper actual surface,
      actual surface <= upper surface ->
      sufficient_check
        (mk_oslf_observation supply upper false) surface = OSatisfied ->
      actual surface <= supply surface)
  /\ (forall supply upper surface,
      1 <= supply surface -> 1 <= upper surface ->
      spend_check (mk_oslf_observation supply upper false) surface 1 =
        OIndeterminate)
  /\ (forall supply upper surface amount,
      supply surface < amount ->
      spend_check (mk_oslf_observation supply upper false) surface amount =
        OUnsatisfied).

Theorem oslf_spatial_modal_logic_sound : OSLF_Spatial_Modal_Logic_Sound.
Proof.
  unfold OSLF_Spatial_Modal_Logic_Sound.
  split.
  - intros supply demand surface. split.
    + intro H. apply exact_linear_check_sound. exact H.
    + intros [Hs Hd]. apply exact_linear_check_complete; assumption.
  - split.
    + exact modal_poststate_is_exact.
    + split.
      * exact spatial_is_commutative.
      * split.
        -- exact spatial_local_sufficiency_composes.
        -- split.
           ++ exact conservative_sufficiency_is_sound.
           ++ split.
              ** exact upper_bound_cannot_assert_modal_spend.
              ** exact upper_bound_insufficient_supply_rejects.
Qed.

(* ═══════════════════════════════ Capstone ════════════════════════════════
   The cost-accounted rho calculus is a well-formed GSLT, and its funding
   judgment is the sound, decidable, linear OSLF resource logic.              *)

Theorem cost_accounted_calculus_is_gslt_with_oslf_logic :
  GSLT_Equations_WellFormed
  /\ GSLT_Rewrites_WellFormed
  /\ OSLF_Funding_Logic_Sound
  /\ OSLF_Spatial_Modal_Logic_Sound.
Proof.
  split.
  - exact gslt_equations_wellformed.
  - split.
    + exact gslt_rewrites_wellformed.
    + split.
      * exact oslf_funding_logic_sound.
      * exact oslf_spatial_modal_logic_sound.
Qed.

(* Axiom-freedom witnesses (printed during compilation; must report
   "Closed under the global context"). *)
Print Assumptions cost_accounted_calculus_is_gslt_with_oslf_logic.
Print Assumptions oslf_spatial_modal_logic_sound.
