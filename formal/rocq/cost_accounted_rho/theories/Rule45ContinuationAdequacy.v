(* ════════════════════════════════════════════════════════════════════════
   Rule45ContinuationAdequacy.v

   GAP-2 / #7 adequacy: the Rule-4/5 continuation re-seal is COST-BENIGN.

   The spec's Rule 4/5 RHS (cost-accounted-rho §3.6, tex L714-742) is
   `T{@U/y} ∥ S ∥ S'`: the continuation is sealed under the RECEIVER's signature
   `s₁` (uniform signing, §3.8), NOT the compound `s₁∘s₂`. The Rocq model
   (`ca_rule4` / `ca_rule5`, CostAccountedReduction.v) re-seals the bare-`proc`
   continuation under the COMPOUND `SAnd s1 s2`. That re-seal is a consequence of
   the proc-under-system representation (DR-17): `SSigned : proc -> sig -> system`
   carries a bare `proc`, so the continuation cannot retain its own signed-term
   seal natively and the rule supplies the consuming signatures instead.

   This module proves that re-seal cannot change the COST that the cost theorems
   and consensus meter, because a SEAL CARRIES NO FUEL. `system_token_count`
   (CostAccountedSyntax.v:208) returns 0 on `SSigned _ _` regardless of the
   signature, so the token count of a Rule-4/5 result is the released token's
   size alone — identical whether the continuation is sealed under the compound
   `s₁∘s₂` or the spec's receiver `s₁`. The over-attribution of `s₂` onto the
   continuation is therefore invisible to the cost layer.

   Scope of this result (stated honestly): the seal still AUTHORIZES the
   continuation's next communication (a different seal admits a different next
   rule); what is proved here is that the seal contributes ZERO to the cost, so
   the choice of seal cannot change `system_token_count`. Two further facts make
   the re-seal benign at every consensus-relevant layer, and are structural (no
   proof needed here): (1) the static demand `Δ_s` that consensus meters
   (`delta_sigma.rs` / `LinearLogicResources.v`) is computed from the desugared
   `Par`'s COMM-node structure and never references `ca_step`'s per-step seals;
   (2) the production runtime (the s₀-collapse) meters by COMM count under one
   envelope signature and likewise does not re-derive cost from intermediate
   seals. The exact native-seal model is the Option-B mutually-inductive grammar,
   recorded separately as a representation migration. `ca_cost_deterministic`
   (Confluence.v) already establishes that the terminal cost of a FIXED system is
   path-independent; this module establishes that the cost is also independent of
   the continuation's seal.

   No Axiom, no Admitted: all proofs Qed-closed (by computation).                *)

From CostAccountedRho Require Import RhoSyntax.
From CostAccountedRho Require Import CostAccountedSyntax.

(* A signed process holds no free fuel: the seal authorizes, it does not fund.
   This is the load-bearing fact — `system_token_count (SSigned _ _) = 0`. *)
Theorem signed_process_holds_no_fuel :
  forall (P : proc) (s : sig), system_token_count (SSigned P s) = 0.
Proof. intros; reflexivity. Qed.

(* Cost is independent of the continuation's seal: re-sealing under any signature
   leaves the token count unchanged. In particular the Rocq `SAnd s1 s2` re-seal
   and the spec's receiver seal `s1` give the same cost. *)
Theorem continuation_seal_is_cost_irrelevant :
  forall (P : proc) (s1 s2 : sig),
    system_token_count (SSigned P s1) = system_token_count (SSigned P s2).
Proof. intros; reflexivity. Qed.

(* On the concrete Rule-4/5 result shape `(P)^seal ∥ t`: the token count is the
   released token `t`'s size alone, independent of the seal — so the compound
   re-seal `SAnd s1 s2` and the spec's receiver seal yield identical cost. *)
Theorem rule45_result_cost_independent_of_seal :
  forall (P : proc) (seal seal' : sig) (t : token),
    system_token_count (SPar (SSigned P seal) (SToken t))
      = system_token_count (SPar (SSigned P seal') (SToken t)).
Proof. intros; reflexivity. Qed.

(* Axiom-freedom witnesses (must report "Closed under the global context"). *)
Print Assumptions continuation_seal_is_cost_irrelevant.
Print Assumptions rule45_result_cost_independent_of_seal.
