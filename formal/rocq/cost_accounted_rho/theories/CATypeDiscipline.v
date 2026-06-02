(* ════════════════════════════════════════════════════════════════════════
   CATypeDiscipline.v — the OSLF linear-resource type discipline (CL7).

   continued-gslt-cost-v2's OSLF-generated type discipline: phlogiston is a
   LINEAR resource (no contraction / no double-spend), threaded through the
   multiplicative (tensor ⊗, lolly ⊸), additive (plus ⊕, with &), and exponential
   (bang !, whynot ?) connectives, with the threshold quorum.

   This discipline is CARRIER-INDEPENDENT: LinearLogicResources.v (and the
   identity laws in LLIdentities.v) are defined over the resource / sig_algebra
   abstractions — NOT over the calculus carrier (system vs signed_term) — so the
   entire DILL/OSLF type discipline holds for the native four-sort model
   verbatim. It is re-exported here under native names as CL7. Axiom-free. *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import LinearLogicResources.

(* Linearity: a linear claim consumes exactly its context (the linear identity),
   and linear resources admit NO contraction — the no-double-spend core. *)
Definition ca_linear_identity := dill_linear_identity.
Definition ca_linear_no_contraction := ll_linear_no_contraction.

(* Multiplicative: lolly modus ponens CONSUMES its input context (⊸ elimination
   is resource-consuming, not duplicating). *)
Definition ca_lolly_consumes_input := dill_lolly_modus_ponens_consumes_input_context.
Definition ca_lolly_flow_conservative := ll_lolly_resource_flow_conservative.

(* Additive: & requires BOTH branches available (external choice), ⊕ consumes the
   CHOSEN branch only (internal choice). *)
Definition ca_with_requires_both := ll_with_requires_both_branches_available.
Definition ca_plus_left_consumes_chosen := ll_plus_left_consumes_chosen_branch.

(* Exponential: ! reuse incurs no extra linear cost; ? consumes no linear witness
   (the unrestricted zone is copyable). *)
Definition ca_bang_reuse_free := ll_bang_reuse_no_extra_linear_cost.
Definition ca_whynot_no_linear_cost := ll_whynot_consumes_no_linear_witness.

(* The OSLF funding judgment: the consumed sig_algebra matches the presented
   authority (the gate is a sound linear proof-checker), with the threshold
   quorum sound. *)
Definition ca_sig_algebra_consumed_matches := ll_sig_algebra_consumed_matches_presented.
Definition ca_threshold_quorum_sound := ll_threshold_quorum_sound.
