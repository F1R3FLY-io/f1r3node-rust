(* ════════════════════════════════════════════════════════════════════════
   ContinuedGSLTCapstone.v — the cost-accounted rho calculus discharges the
   central structural claims of "Continued Interactive GSLTs and the Cost
   Endofunctor" (continued-gslt-cost-v2.tex), axiom-free (Stage 6 capstone).

   Analogous to GSLTOSLFCapstone (which discharged §6.1/§7.7 of the concrete
   paper), this assembles the NATIVE four-sort results into the new paper's
   load-bearing claims for the rho instance:

   - Wrapping by construction (continued-gslt-cost-v2 §"Wrapping by construction"):
     no-leak is a sorting invariant — every continuation is a wrapped thunk and a
     wrapped redex never fires without consuming a token.
   - The Cost monad's laws descend from the TWO constituent monoids (Prop "the
     cost monad", :1064): the signature commutative monoid (Sig,*,()) and the
     temporal token-stack free monoid (cons,++,()).
   - GAP-2 dissolved: the split-process COMM keeps the continuation's own seal
     (no SAnd re-seal) — the syntactic realization the native grammar enables.
   - Cost determinism on the funded fragment (the consensus-relevant class).
   - "Stack consumption is the modulus" (Prop, :530): the run length is bounded
     by the consumed stack.

   These are all Qed-closed already in the native modules; the capstone is the
   single named theorem asserting the paper's claims hold. Graded-HML adequacy
   and the internalisation adjunction (which require the native translation /
   bisimulation, the Stage-4 work) are tracked separately. Axiom-free.         *)

From CostAccountedRho Require Import CostAccountedSyntax.
From CostAccountedRho Require Import CASyntax.
From CostAccountedRho Require Import CAReduction.
From CostAccountedRho Require Import WrappingSubjectReduction.
From CostAccountedRho Require Import SignatureMonoid.
From CostAccountedRho Require Import CATokenConservation.
From CostAccountedRho Require Import CAConfluence.
From CostAccountedRho Require Import CAStepDeterminism.
From CostAccountedRho Require Import CACostDeterminism.
From CostAccountedRho Require Import CAModulus.

(* ── the paper's claims, as predicates over the native calculus ─────────── *)

Definition Wrapping_By_Construction : Prop :=
  (forall S S', ca_step S S' -> well_wrapped S')
  /\ (forall (P : caproc) (s : sig) (S' : signed_term), ~ ca_step (STSigned P s) S').

Definition Cost_Monad_Laws : Prop :=
  (forall s t, SAnd s t ≡sig SAnd t s)
  /\ (forall s t u, SAnd (SAnd s t) u ≡sig SAnd s (SAnd t u))
  /\ (forall s, SAnd SUnit s ≡sig s)
  /\ (forall s, SAnd s SUnit ≡sig s)
  /\ (forall t u v, tok_concat (tok_concat t u) v = tok_concat t (tok_concat u v))
  /\ (forall t, tok_concat TUnit t = t)
  /\ (forall t, tok_concat t TUnit = t).

Definition GAP2_Dissolved : Prop :=
  forall x T U s1 s2 t,
    ca_step (STPar (STPar (STSigned (CPInput x T) s1) (STSigned (CPOutput x U) s2))
                   (STStack (TGate (SAnd s1 s2) t)))
            (STPar (subst_st T 0 (CQuote U)) (STStack t)).

Definition Cost_Determinism : Prop :=
  forall S T1 T2, HF S ->
    ca_reachable S T1 -> ca_terminal T1 ->
    ca_reachable S T2 -> ca_terminal T2 ->
    st_total_fuel T1 = st_total_fuel T2.

Definition Stack_Modulus : Prop :=
  forall n S T, HF S -> ca_reachable_n n S T -> n <= st_total_fuel S.

(* ── the claims hold ────────────────────────────────────────────────────── *)

Lemma wrapping_by_construction_holds : Wrapping_By_Construction.
Proof.
  split.
  - intros S S' H. apply (subject_reduction_wrapping S S' H), well_wrapped_universal.
  - apply no_leak_requires_token.
Qed.

Lemma cost_monad_laws_hold : Cost_Monad_Laws.
Proof.
  repeat split;
    first [ apply sig_monoid_comm | apply sig_monoid_assoc
          | apply sig_monoid_unit_l | apply sig_monoid_unit_r
          | apply tok_concat_assoc | apply tok_concat_unit_l
          | apply tok_concat_unit_r ].
Qed.

Lemma gap2_dissolved_holds : GAP2_Dissolved.
Proof. unfold GAP2_Dissolved. exact gap2_split_combined_keeps_own_seal. Qed.

Lemma cost_determinism_holds : Cost_Determinism.
Proof. exact ca_cost_deterministic_funded. Qed.

Lemma stack_modulus_holds : Stack_Modulus.
Proof. exact funded_run_bounded. Qed.

(* ── the capstone ───────────────────────────────────────────────────────── *)

Theorem continued_gslt_cost_capstone :
  Wrapping_By_Construction
  /\ Cost_Monad_Laws
  /\ GAP2_Dissolved
  /\ Cost_Determinism
  /\ Stack_Modulus.
Proof.
  split; [ exact wrapping_by_construction_holds
         | split; [ exact cost_monad_laws_hold
                  | split; [ exact gap2_dissolved_holds
                           | split; [ exact cost_determinism_holds
                                    | exact stack_modulus_holds ]]]].
Qed.
